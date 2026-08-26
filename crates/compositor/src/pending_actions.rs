// Drains and acts on every `PENDING_*` queue cef-bridge's CEF callbacks
// filled since the last frame — clicks on the bookmarks/downloads/
// history/workspace list pages' generated links, omnibox/switcher
// submissions, completed downloads/popups/visits — refreshing whichever
// generated list page needs to reflect the change afterward. Split out
// of app.rs's `RedrawRequested` arm: routing raw window/input events
// (app.rs's actual job) and draining cef-bridge's action queues are
// different concerns that happen to both need to run once a frame, and
// together they'd made that file the largest in the crate by far.

use crate::browser;
use crate::clipboard_bridge;
use crate::output::{GpuState, Rect, THEMES};
use crate::pages;
use crate::persistence::{
    bookmarks::{self, Bookmark},
    downloads::{self, DownloadRecord},
    history::{self, HistoryEntry},
    settings::{self, AppSettings},
    typed_history,
    workspaces::{self, Workspace, WorkspacePage},
};
use crate::session::Session;
use crate::userscripts::{self, RunAt, UserScript};
use crate::userstyles::{self, UserStyle};
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use cef_bridge::{
    BookmarkAction, DownloadPageAction, HistoryPageAction, SettingsPageAction,
    UserscriptsPageAction, WorkspacePageAction, PENDING_BOOKMARK, PENDING_DOWNLOADS,
    PENDING_DOWNLOAD_ACTION, PENDING_HISTORY_ACTION, PENDING_LOAD_START, PENDING_OMNIBOX,
    PENDING_POPUPS, PENDING_SETTINGS_ACTION, PENDING_SWITCH, PENDING_USERSCRIPT_ACTION,
    PENDING_VISITS, PENDING_WORKSPACE_ACTION,
};

/// Pushes the current ad-block toggle/custom-hosts into cef-bridge's
/// own live state (a thread_local, not part of `AppSettings` itself) —
/// called once at startup (app.rs's `Default::default`) and again after
/// every settings change that touches either field, since
/// `on_before_resource_load` (cef-bridge) reads that thread_local
/// directly, not `AppSettings`.
pub(crate) fn sync_blocklist_settings(settings: &AppSettings) {
    cef_bridge::set_enabled(settings.ad_block_enabled);
    cef_bridge::set_custom_hosts(settings.custom_blocked_hosts.clone());
}

/// Drains and acts on every pending action queued since the last frame.
/// Called once per `RedrawRequested`, before rendering.
pub fn apply(
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &mut Vec<Bookmark>,
    typed_history: &mut Vec<String>,
    downloads: &mut Vec<DownloadRecord>,
    history: &mut Vec<HistoryEntry>,
    workspaces: &mut Vec<Workspace>,
    settings: &mut AppSettings,
    userscripts: &mut Vec<UserScript>,
    userstyles: &mut Vec<UserStyle>,
) {
    // Set by cef-bridge's OsrRequestHandler when a click or form submit
    // inside the bookmarks-list page (hotkeys::open_bookmarks) hits one
    // of its `bookmark://...` links — that navigation was already
    // canceled there; act on it here instead. `browser_id` identifies
    // exactly which bookmarks-list page asked, so delete/rename can
    // reload that same page in place rather than guessing which open
    // page (if any) it was.
    if let Some((browser_id, action)) = PENDING_BOOKMARK.with_borrow_mut(|pending| pending.take()) {
        match action {
            BookmarkAction::Open(index) => {
                if let Some(bookmark) = bookmarks.get(index) {
                    let size = gpu.window.inner_size();
                    let rect = session.cascade_rect((size.width as f32, size.height as f32));
                    session.add_page(browser::spawn(gpu, &gpu.window, &bookmark.url, rect, false));
                }
            }
            BookmarkAction::Delete(index) => {
                if index < bookmarks.len() {
                    bookmarks.remove(index);
                    bookmarks::save(bookmarks);
                }
                refresh_bookmarks_page(session, gpu, bookmarks, browser_id);
            }
            BookmarkAction::Rename(index, title, folder) => {
                if let Some(bookmark) = bookmarks.get_mut(index) {
                    bookmark.title = (!title.is_empty()).then_some(title);
                    bookmark.folder = (!folder.is_empty()).then_some(folder);
                }
                bookmarks::save(bookmarks);
                refresh_bookmarks_page(session, gpu, bookmarks, browser_id);
            }
        }
    }

    // Set by cef-bridge's OsrRequestHandler when the omnibox page
    // (hotkeys::open_new / omnibox_page_url) submits an
    // `omnibox://go?q=...&url=...` — that navigation was already
    // canceled there. Log the raw typed text, then replace the omnibox
    // page with a real one at the resolved destination (close+respawn
    // rather than `load_url` in place, same reliability reason as
    // refresh_bookmarks_page).
    if let Some((browser_id, submit)) = PENDING_OMNIBOX.with_borrow_mut(|pending| pending.take()) {
        typed_history::record(typed_history, &submit.raw);
        if let Some(index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == browser_id)
        {
            if let Some(rect) = session.close_at(index) {
                session.add_page(browser::spawn(gpu, &gpu.window, &submit.url, rect, false));
            }
        }
    }

    // Set by cef-bridge's OsrRequestHandler when a row click or Enter
    // inside the switcher page (hotkeys::open_switcher) hits a
    // `switcher://go/{id}` link — that navigation was already canceled
    // there. Bring the target page to front and pan it to screen center
    // (kept at the current zoom level), then close the switcher page —
    // a permanent close, not through the closed-page undo stack
    // (pop_closed): this isn't a user-initiated close of *their*
    // content.
    if let Some((switcher_id, target_id)) = PENDING_SWITCH.with_borrow_mut(|pending| pending.take())
    {
        if let Some(target_index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == target_id)
        {
            session.bring_to_front(target_index);
            let rect = session
                .pages()
                .last()
                .expect("just brought a page to front")
                .rect;
            let viewport = session.viewport();
            let size = gpu.window.inner_size();
            let screen_center = (size.width as f32 / 2.0, size.height as f32 / 2.0);
            let world_center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            session.pan_viewport_to((
                world_center.0 - screen_center.0 / viewport.zoom,
                world_center.1 - screen_center.1 / viewport.zoom,
            ));
        }
        if let Some(switcher_index) = session
            .pages()
            .iter()
            .position(|p| p.browser.identifier() == switcher_id)
        {
            session.close_at(switcher_index);
        }
    }

    // Appended by cef-bridge's OsrDownloadHandler the first time a
    // download reports complete. Record each into downloads.json and
    // fire a desktop notification — there's no in-canvas download UI
    // (progress bar, toast): a system notification is visible
    // regardless of window focus/workspace, and needs no native GPU
    // text rendering the way an in-canvas toast would.
    let completed = PENDING_DOWNLOADS.with_borrow_mut(std::mem::take);
    for download in completed {
        let filename = std::path::Path::new(&download.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&download.path)
            .to_string();
        downloads::record(
            downloads,
            DownloadRecord {
                url: download.url,
                path: download.path,
            },
        );
        if let Err(e) = std::process::Command::new("notify-send")
            .arg("Download complete")
            .arg(&filename)
            .spawn()
        {
            log::warn!("notify-send failed (download {filename:?} still saved): {e}");
        }
    }

    // Set by cef-bridge's OsrRequestHandler when a click inside the
    // downloads-list page (hotkeys::open_downloads) hits a
    // `download://...` link — that navigation was already canceled
    // there.
    if let Some((browser_id, action)) =
        PENDING_DOWNLOAD_ACTION.with_borrow_mut(|pending| pending.take())
    {
        match action {
            DownloadPageAction::Open(index) => {
                if let Some(download) = downloads.get(index) {
                    if let Err(e) = std::process::Command::new("xdg-open")
                        .arg(&download.path)
                        .spawn()
                    {
                        log::warn!("xdg-open failed for {:?}: {e}", download.path);
                    }
                }
            }
            DownloadPageAction::Remove(index) => {
                if index < downloads.len() {
                    downloads.remove(index);
                    downloads::save(downloads);
                }
                refresh_downloads_page(session, gpu, downloads, browser_id);
            }
        }
    }

    // Appended by cef-bridge's OsrLifeSpanHandler when a page tries to
    // open a link in a new tab/window (target="_blank", window.open,
    // middle-click) — canceled there so CEF doesn't spawn its own
    // native popup window outside the canvas. Spawn a regular Page
    // instead, cascaded from the opener's rect if it's still open.
    let popups = PENDING_POPUPS.with_borrow_mut(std::mem::take);
    for (opener_id, url) in popups {
        let opener_rect = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == opener_id)
            .map(|p| p.rect);
        let size = gpu.window.inner_size();
        let rect = match opener_rect {
            Some(r) => Rect {
                x: r.x + 40.0,
                y: r.y + 40.0,
                w: r.w,
                h: r.h,
            },
            None => session.cascade_rect((size.width as f32, size.height as f32)),
        };
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }

    // document-start userscripts — fired from OsrLoadHandler::on_load_start
    // so they run before the page's own scripts when CEF allows it.
    let load_starts = PENDING_LOAD_START.with_borrow_mut(std::mem::take);
    for (browser_id, url) in load_starts {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            if let Some(frame) = page.browser.main_frame() {
                // Both styles and scripts honor `@match spatial-ui` for
                // ephemeral chrome pages (bookmarks, settings, …).
                for js in userstyles::matching_inject_js(&url, page.ephemeral, userstyles) {
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
                for code in userscripts::matching_code(
                    &url,
                    page.ephemeral,
                    userscripts,
                    RunAt::DocumentStart,
                ) {
                    frame.execute_java_script(Some(&code.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
    }

    // Appended by cef-bridge's OsrLoadHandler for every completed
    // top-level navigation. Three things happen for each: the
    // copy-bridge script gets (re-)injected, since a full navigation
    // wipes whatever a previous injection put in the page's DOM (see
    // clipboard_bridge.rs — CEF's own clipboard integration doesn't
    // work at all in this windowless/OSR embedding, confirmed
    // empirically); any document-end/idle userscript / userstyle whose
    // `@match` fits (including `spatial-ui` for ephemeral chrome) gets
    // injected; history is still skipped for ephemeral pages.
    let visits = PENDING_VISITS.with_borrow_mut(std::mem::take);
    for (browser_id, url) in visits {
        if let Some(page) = session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == browser_id)
        {
            if let Some(frame) = page.browser.main_frame() {
                frame.execute_java_script(
                    Some(&clipboard_bridge::COPY_BRIDGE_SCRIPT.into()),
                    Some(&"".into()),
                    0,
                );
                // Styles again on load-end (covers navigations that
                // skipped start, and re-applies after full document swap).
                for js in userstyles::matching_inject_js(&url, page.ephemeral, userstyles) {
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
                for run_at in [RunAt::DocumentEnd, RunAt::DocumentIdle] {
                    for code in
                        userscripts::matching_code(&url, page.ephemeral, userscripts, run_at)
                    {
                        frame.execute_java_script(
                            Some(&code.as_str().into()),
                            Some(&"".into()),
                            0,
                        );
                    }
                }
            }
            if !page.ephemeral {
                history::record(history, &url, now_unix_secs());
            }
        }
    }

    if let Some((browser_id, action)) =
        PENDING_USERSCRIPT_ACTION.with_borrow_mut(|pending| pending.take())
    {
        match action {
            UserscriptsPageAction::Reload => {
                userscripts::reload(userscripts);
                userstyles::reload(userstyles);
            }
            UserscriptsPageAction::OpenDir => {
                userscripts::open_dir();
            }
            UserscriptsPageAction::OpenStylesDir => {
                userstyles::open_dir();
            }
            UserscriptsPageAction::Toggle(name) => {
                userscripts::toggle_enabled(userscripts, &name);
            }
            UserscriptsPageAction::ToggleStyle(name) => {
                userstyles::toggle_enabled(userstyles, &name);
            }
        }
        refresh_userscripts_page(session, gpu, userscripts, userstyles, browser_id);
    }

    // Set by cef-bridge's OsrRequestHandler when a click inside the
    // history-list page (hotkeys::open_history) hits a `history://...`
    // link — that navigation was already canceled there.
    if let Some((browser_id, action)) =
        PENDING_HISTORY_ACTION.with_borrow_mut(|pending| pending.take())
    {
        match action {
            HistoryPageAction::Open(index) => {
                if let Some(entry) = history.get(index) {
                    let size = gpu.window.inner_size();
                    let rect = session.cascade_rect((size.width as f32, size.height as f32));
                    session.add_page(browser::spawn(gpu, &gpu.window, &entry.url, rect, false));
                }
            }
            HistoryPageAction::Remove(index) => {
                if index < history.len() {
                    history.remove(index);
                    history::save(history);
                }
                refresh_history_page(session, gpu, history, browser_id);
            }
            HistoryPageAction::Clear => {
                history.clear();
                history::save(history);
                refresh_history_page(session, gpu, history, browser_id);
            }
        }
    }

    // Set by cef-bridge's OsrRequestHandler when a click inside the
    // workspace-list page (hotkeys::open_workspaces) hits a
    // `workspace://...` link — that navigation was already canceled
    // there.
    if let Some((browser_id, action)) =
        PENDING_WORKSPACE_ACTION.with_borrow_mut(|pending| pending.take())
    {
        match action {
            WorkspacePageAction::Load(index) => {
                if let Some(workspace) = workspaces.get(index).cloned() {
                    // close_topmost (not close_at) so this still goes
                    // through the Ctrl+Shift+T undo stack — except
                    // ephemeral pages (this list itself included),
                    // which close_topmost already excludes from that
                    // stack.
                    while !session.pages().is_empty() {
                        session.close_topmost();
                    }
                    let theme = THEMES
                        .iter()
                        .find(|t| t.name == workspace.theme)
                        .copied()
                        .unwrap_or(THEMES[0]);
                    session.set_theme(theme);
                    session.set_viewport(workspace.viewport);
                    for page in workspace.pages {
                        session.add_page(browser::spawn(
                            gpu,
                            &gpu.window,
                            &page.url,
                            page.rect,
                            false,
                        ));
                    }
                }
            }
            WorkspacePageAction::Rename(index, name) => {
                if let Some(workspace) = workspaces.get_mut(index) {
                    if !name.is_empty() {
                        workspace.name = name;
                    }
                }
                workspaces::save(workspaces);
                refresh_workspaces_page(session, gpu, workspaces, browser_id);
            }
            WorkspacePageAction::Delete(index) => {
                if index < workspaces.len() {
                    workspaces.remove(index);
                    workspaces::save(workspaces);
                }
                refresh_workspaces_page(session, gpu, workspaces, browser_id);
            }
            WorkspacePageAction::SaveNew => {
                let pages = session
                    .pages()
                    .iter()
                    .filter(|p| !p.ephemeral)
                    .map(|p| WorkspacePage {
                        url: p.url(),
                        rect: p.rect,
                    })
                    .collect();
                workspaces.push(Workspace {
                    name: format!("Workspace {}", workspaces.len() + 1),
                    viewport: session.viewport(),
                    theme: session.theme().name.to_string(),
                    pages,
                });
                workspaces::save(workspaces);
                refresh_workspaces_page(session, gpu, workspaces, browser_id);
            }
        }
    }

    // Set by cef-bridge's OsrRequestHandler when a click inside the
    // settings page (hotkeys::open_settings) hits a `settings://...`
    // link — that navigation was already canceled there.
    if let Some((browser_id, action)) =
        PENDING_SETTINGS_ACTION.with_borrow_mut(|pending| pending.take())
    {
        match action {
            SettingsPageAction::ToggleAdBlock => {
                settings.ad_block_enabled = !settings.ad_block_enabled;
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleCleanUrls => {
                settings.clean_urls_enabled = !settings.clean_urls_enabled;
                settings::save(settings);
                cef_bridge::set_clean_urls_enabled(settings.clean_urls_enabled);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetSearchEngine(engine) => {
                settings.default_search_engine = engine;
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetTheme(index) => {
                if let Some(theme) = THEMES.get(index) {
                    session.set_theme(*theme);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetReaderTheme(index) => {
                if index < crate::reader_mode::READER_THEMES.len() {
                    settings.reader_theme = index;
                    settings::save(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::AddBlockedHost(host) => {
                let host = host.trim();
                if !host.is_empty() && !settings.custom_blocked_hosts.iter().any(|h| h == host) {
                    settings.custom_blocked_hosts.push(host.to_string());
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::RemoveBlockedHost(index) => {
                if index < settings.custom_blocked_hosts.len() {
                    settings.custom_blocked_hosts.remove(index);
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetFrameRate(fps) => {
                settings.target_fps = fps;
                settings::save(settings);
                browser::set_target_frame_rate(fps);
                // Applied immediately to every currently-open page's own
                // CEF host, not just future spawns — same reasoning as
                // the (now-removed) page-label toggle had: a setting a
                // user can visibly judge (does it feel smoother?) left
                // stale until next launch would be a much more
                // noticeable inconsistency than most settings here.
                for page in session.pages() {
                    if let Some(host) = page.browser.host() {
                        host.set_windowless_frame_rate(fps as _);
                    }
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
        }
    }
}

/// Replaces the bookmarks-list page identified by `browser_id` (CEF's
/// own per-browser id) with a fresh one at the same rect, if it's still
/// open — used after a delete/rename so the list reflects the change
/// without the user having to close and reopen it themselves. Closes
/// and respawns rather than `load_url` in place: a navigation issued
/// right after CEF just canceled one on that same frame isn't
/// reliable.
fn refresh_bookmarks_page(
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &[Bookmark],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Same as `refresh_bookmarks_page`, for the downloads-list page after
/// a remove.
fn refresh_downloads_page(
    session: &mut Session,
    gpu: &GpuState,
    downloads: &[DownloadRecord],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Same as `refresh_bookmarks_page`, for the history-list page after a
/// remove/clear.
fn refresh_history_page(
    session: &mut Session,
    gpu: &GpuState,
    history: &[HistoryEntry],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::history_list::page_url(&session.theme(), history);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Same as `refresh_bookmarks_page`, for the workspace-list page after
/// a save/rename/delete. Not used for `Load`: that closes this page
/// along with everything else on the canvas rather than refreshing it
/// in place.
fn refresh_workspaces_page(
    session: &mut Session,
    gpu: &GpuState,
    workspaces: &[Workspace],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::workspace_list::page_url(&session.theme(), workspaces);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Same as `refresh_bookmarks_page`, for the settings page after any
/// change — every `SettingsPageAction` refreshes it, since every one
/// changes something the page displays (the ad-block state, which
/// engine/theme is checked, the custom-hosts list).
fn refresh_settings_page(
    session: &mut Session,
    gpu: &GpuState,
    settings: &AppSettings,
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::settings_list::page_url(&session.theme(), settings);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn refresh_userscripts_page(
    session: &mut Session,
    gpu: &GpuState,
    scripts: &[UserScript],
    styles: &[UserStyle],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::userscripts_list::page_url(&session.theme(), scripts, styles);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Seconds since the Unix epoch, UTC — used to stamp a new history
/// entry. `UNIX_EPOCH` is always in the past on any correctly set
/// clock, so the `unwrap` only panics on a system clock set before
/// 1970.
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
