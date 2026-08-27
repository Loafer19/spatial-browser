// Drains and acts on every `PENDING_*` queue cef-bridge's CEF callbacks
// filled since the last frame — clicks on the bookmarks/downloads/
// history/workspace list pages' generated links, omnibox/switcher
// submissions, completed downloads/popups/visits — refreshing whichever
// generated list page needs to reflect the change afterward. Split out
// of app.rs's `RedrawRequested` arm: routing raw window/input events
// (app.rs's actual job) and draining cef-bridge's action queues are
// different concerns that happen to both need to run once a frame, and
// together they'd made that file the largest in the crate by far.

use crate::app::{ContextMenuState, PendingSaveOffer};
use crate::autofill_bridge;
use crate::browser;
use crate::clipboard_bridge;
use crate::hotkeys;
use crate::output::{GpuState, Rect, THEMES};
use crate::pages;
use crate::pages::context_menu::MenuContext;
use crate::persistence::{
    bookmarks::{self, Bookmark},
    downloads::{self, DownloadRecord},
    history::{self, HistoryEntry},
    settings::{self, AppSettings},
    typed_history,
    vault::{self, VaultEntry, VaultSession},
    workspaces::{self, WorkspaceRuntime, WorkspaceStore},
};
use crate::session::Session;
use crate::userscripts::{self, RunAt, UserScript};
use crate::userstyles::{self, UserStyle};
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use cef_bridge::{
    BookmarkAction, ContextAction, DownloadPageAction, HistoryPageAction, PasswordAction,
    SettingsPageAction, UserscriptsPageAction, WorkspacePageAction, PENDING_BOOKMARK,
    PENDING_CONTEXT_ACTION, PENDING_DOWNLOADS, PENDING_DOWNLOAD_ACTION, PENDING_HISTORY_ACTION,
    PENDING_LOAD_START, PENDING_OMNIBOX, PENDING_PASSWORD_ACTION, PENDING_POPUPS,
    PENDING_SETTINGS_ACTION, PENDING_SWITCH, PENDING_USERSCRIPT_ACTION, PENDING_VISITS,
    PENDING_WORKSPACE_ACTION,
};

/// Pushes the current ad-block toggle/custom-hosts into cef-bridge's
/// own live state (a thread_local, not part of `AppSettings` itself) —
/// called once at startup (app.rs's `Default::default`) and again after
/// every settings change that touches either field, since
/// `on_before_resource_load` (cef-bridge) reads that thread_local
/// directly, not `AppSettings`.
pub(crate) fn sync_blocklist_settings(settings: &AppSettings) {
    // Network layer gates request cancel; cosmetic/scriptlets inject later.
    let network_on = settings.ad_block_enabled && settings.filter_network_enabled;
    cef_bridge::set_enabled(network_on);
    cef_bridge::set_peter_lowe_enabled(settings.filter_lists.peter_lowe);
    cef_bridge::set_custom_hosts(settings.custom_blocked_hosts.clone());
    let filters_dir = ensure_filter_lists_dir();
    cef_bridge::rebuild_filter_engine(&cef_bridge::FilterEngineConfig {
        easylist: network_on && settings.filter_lists.easylist,
        easyprivacy: network_on && settings.filter_lists.easyprivacy,
        filters_dir,
    });
}

/// Prefer `~/.config/spatial-browser/filters/`; seed from bundle or repo
/// `data/filters` on first run.
fn ensure_filter_lists_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    let dest = std::path::PathBuf::from(home).join(".config/spatial-browser/filters");
    if let Err(e) = std::fs::create_dir_all(&dest) {
        log::warn!("filters dir {}: {e}", dest.display());
        return dest;
    }
    for name in ["easylist.txt", "easyprivacy.txt"] {
        let dest_file = dest.join(name);
        if dest_file.exists() {
            continue;
        }
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("filters").join(name));
            }
        }
        candidates.push(std::path::PathBuf::from("data/filters").join(name));
        candidates.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/filters")
                .join(name),
        );
        for src in candidates {
            if src.is_file() {
                match std::fs::copy(&src, &dest_file) {
                    Ok(_) => {
                        log::info!("seeded filter list {} ← {}", name, src.display());
                        break;
                    }
                    Err(e) => log::warn!("copy {} → {}: {e}", src.display(), dest_file.display()),
                }
            }
        }
        if !dest_file.exists() {
            log::warn!(
                "filter list {name} missing — enable EasyList files under {}",
                dest.display()
            );
        }
    }
    dest
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
    workspaces: &mut WorkspaceStore,
    workspace_runtime: &mut WorkspaceRuntime,
    settings: &mut AppSettings,
    userscripts: &mut Vec<UserScript>,
    userstyles: &mut Vec<UserStyle>,
    vault: &mut Option<VaultSession>,
    generated_password: &mut Option<String>,
    pending_save_offer: &mut Option<PendingSaveOffer>,
    context_menu: &mut Option<ContextMenuState>,
    pending_context_hit: &mut Option<(i32, (f32, f32))>,
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
                if !page.ephemeral {
                    let bridge = autofill_bridge::script(&session.theme());
                    frame.execute_java_script(
                        Some(&bridge.as_str().into()),
                        Some(&"".into()),
                        0,
                    );
                    // Re-show save banner after login navigations wipe the DOM.
                    if let Some(offer) = pending_save_offer.as_ref() {
                        let page_origin = vault::normalize_origin(&url);
                        if page_origin == vault::normalize_origin(&offer.origin) {
                            let payload = format!(
                                "{{origin:{},username:{},password:{},id:{}}}",
                                serde_json::to_string(&offer.origin).unwrap_or_default(),
                                serde_json::to_string(&offer.username).unwrap_or_default(),
                                serde_json::to_string(&offer.password).unwrap_or_default(),
                                serde_json::to_string(&offer.id).unwrap_or_default(),
                            );
                            let js = format!(
                                "window.__spatialAutofillShowSave && window.__spatialAutofillShowSave({payload});"
                            );
                            frame.execute_java_script(
                                Some(&js.as_str().into()),
                                Some(&"".into()),
                                0,
                            );
                            *pending_save_offer = None;
                        }
                    }
                }
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

    if let Some((browser_id, action)) =
        PENDING_PASSWORD_ACTION.with_borrow_mut(|pending| pending.take())
    {
        handle_password_action(
            session,
            gpu,
            vault,
            generated_password,
            pending_save_offer,
            browser_id,
            action,
        );
    }

    if let Some((browser_id, action)) =
        PENDING_CONTEXT_ACTION.with_borrow_mut(|pending| pending.take())
    {
        handle_context_action(
            session,
            gpu,
            vault,
            context_menu,
            pending_context_hit,
            workspaces,
            workspace_runtime,
            typed_history,
            settings,
            browser_id,
            action,
        );
    }
    // typed_history is &mut above — fine for open_new which only reads.

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
                if let Some(slot) = workspaces.slots.get(index).cloned() {
                    workspaces::switch_to(
                        workspaces,
                        workspace_runtime,
                        session,
                        gpu,
                        slot.id,
                    );
                }
            }
            WorkspacePageAction::Rename(_index, _name) => {
                // Slots are numbered; rename is a no-op in v2.
                refresh_workspaces_page(session, gpu, workspaces, browser_id);
            }
            WorkspacePageAction::Delete(index) => {
                if let Some(id) = workspaces.slots.get(index).map(|s| s.id) {
                    workspaces::delete_slot(
                        workspaces,
                        workspace_runtime,
                        session,
                        gpu,
                        id,
                    );
                }
                // List page may still be open if a non-active slot was deleted.
                if session
                    .pages()
                    .iter()
                    .any(|p| p.browser.identifier() == browser_id)
                {
                    refresh_workspaces_page(session, gpu, workspaces, browser_id);
                }
            }
            WorkspacePageAction::SaveNew => {
                workspaces::add_and_switch(workspaces, workspace_runtime, session, gpu);
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
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleCleanUrls => {
                settings.clean_urls_enabled = !settings.clean_urls_enabled;
                settings.settings_tab = "general".into();
                settings::save(settings);
                cef_bridge::set_clean_urls_enabled(settings.clean_urls_enabled);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterNetwork => {
                settings.filter_network_enabled = !settings.filter_network_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterCosmetic => {
                settings.filter_cosmetic_enabled = !settings.filter_cosmetic_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterScriptlets => {
                settings.filter_scriptlets_enabled = !settings.filter_scriptlets_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterList(id) => {
                match id.as_str() {
                    "peter_lowe" => {
                        settings.filter_lists.peter_lowe = !settings.filter_lists.peter_lowe;
                    }
                    "easylist" => {
                        settings.filter_lists.easylist = !settings.filter_lists.easylist;
                    }
                    "easyprivacy" => {
                        settings.filter_lists.easyprivacy = !settings.filter_lists.easyprivacy;
                    }
                    _ => {}
                }
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetTab(tab) => {
                settings.settings_tab = AppSettings::normalize_tab(&tab).to_string();
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetSearchEngine(engine) => {
                settings.default_search_engine = engine;
                settings.settings_tab = "general".into();
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetTheme(index) => {
                settings.settings_tab = "appearance".into();
                if let Some(theme) = THEMES.get(index) {
                    session.set_theme(*theme);
                }
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetReaderTheme(index) => {
                if index < crate::reader_mode::READER_THEMES.len() {
                    settings.reader_theme = index;
                    settings.settings_tab = "appearance".into();
                    settings::save(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::AddBlockedHost(host) => {
                let host = host.trim();
                if !host.is_empty() && !settings.custom_blocked_hosts.iter().any(|h| h == host) {
                    settings.custom_blocked_hosts.push(host.to_string());
                    settings.settings_tab = "blocking".into();
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::RemoveBlockedHost(index) => {
                if index < settings.custom_blocked_hosts.len() {
                    settings.custom_blocked_hosts.remove(index);
                    settings.settings_tab = "blocking".into();
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetFrameRate(fps) => {
                settings.target_fps = fps;
                settings.settings_tab = "general".into();
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
pub(crate) fn dismiss_context_menu(
    session: &mut Session,
    context_menu: &mut Option<ContextMenuState>,
) {
    let Some(cm) = context_menu.take() else {
        return;
    };
    if let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == cm.menu_browser_id)
    {
        let _ = session.close_at(index);
    }
}

pub(crate) fn open_context_menu(
    session: &mut Session,
    gpu: &GpuState,
    context_menu: &mut Option<ContextMenuState>,
    screen_pos: (f32, f32),
    ctx: MenuContext,
) {
    dismiss_context_menu(session, context_menu);
    let url = pages::context_menu::page_url(&session.theme(), &ctx);
    let (css_w, css_h) = pages::context_menu::menu_css_size(&ctx);
    // CEF logical size = world_rect / scale_factor (see browser::Page::set_rect).
    // Want logical == css_* at current zoom → world = css * scale / zoom.
    let scale = gpu.window.scale_factor() as f32;
    let zoom = session.viewport().zoom.max(0.05);
    let viewport = session.viewport();
    let world = viewport.screen_to_world(screen_pos);
    let rect = Rect {
        x: world.0,
        y: world.1,
        w: css_w * scale / zoom,
        h: css_h * scale / zoom,
    };
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
    let menu_id = session.pages().last().map(|p| p.browser.identifier());
    if let Some(menu_browser_id) = menu_id {
        *context_menu = Some(ContextMenuState {
            menu_browser_id,
            target_browser_id: ctx.target_browser_id,
            screen_pos,
        });
    }
}

fn handle_context_action(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    context_menu: &mut Option<ContextMenuState>,
    pending_context_hit: &mut Option<(i32, (f32, f32))>,
    workspaces: &mut WorkspaceStore,
    workspace_runtime: &mut WorkspaceRuntime,
    typed_history: &[String],
    settings: &AppSettings,
    browser_id: i32,
    action: ContextAction,
) {
    match action {
        ContextAction::Hit {
            link,
            image,
            password_field,
            page_url,
        } => {
            let (target_id, screen_pos) = match pending_context_hit.take() {
                Some(v) => v,
                None => (browser_id, (40.0, 40.0)),
            };
            open_context_menu(
                session,
                gpu,
                context_menu,
                screen_pos,
                MenuContext {
                    on_canvas: false,
                    page_url: Some(page_url),
                    link,
                    image,
                    password_field,
                    target_browser_id: Some(target_id),
                },
            );
        }
        ContextAction::OpenLink(url) => {
            dismiss_context_menu(session, context_menu);
            let size = gpu.window.inner_size();
            let rect = session.cascade_rect((size.width as f32, size.height as f32));
            session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
        }
        ContextAction::Copy(text) => {
            dismiss_context_menu(session, context_menu);
            let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
        }
        ContextAction::SaveImage(url) => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id)
                .unwrap_or(browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(page) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == target)
            {
                if let Some(frame) = page.browser.main_frame() {
                    let js = format!(
                        "(function(u){{var a=document.createElement('a');a.href=u;a.download='';\
                         a.rel='noopener';document.documentElement.appendChild(a);a.click();a.remove();}})({});",
                        serde_json::to_string(&url).unwrap_or_default()
                    );
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
        ContextAction::GenPassword => {
            dismiss_context_menu(session, context_menu);
            let pw = vault::generate_password(20, true);
            let _ = std::process::Command::new("wl-copy").arg(&pw).spawn();
        }
        ContextAction::FillPassword => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if vault.is_none() {
                hotkeys::open_passwords(session, gpu, None);
                return;
            }
            if let Some(tid) = target {
                if let Some(page) = session.pages().iter().find(|p| p.browser.identifier() == tid)
                {
                    let origin = vault::normalize_origin(&page.url());
                    if let Some(v) = vault.as_ref() {
                        let matches = v.entries_for_origin(&origin);
                        if let Some(frame) = page.browser.main_frame() {
                            match matches.len() {
                                0 => {}
                                1 => {
                                    let js = fill_entry_js(matches[0]);
                                    frame.execute_java_script(
                                        Some(&js.as_str().into()),
                                        Some(&"".into()),
                                        0,
                                    );
                                }
                                _ => {
                                    let items: Vec<String> = matches
                                        .iter()
                                        .map(|e| {
                                            format!(
                                                "{{id:{},username:{}}}",
                                                serde_json::to_string(&e.id).unwrap_or_default(),
                                                serde_json::to_string(&e.username)
                                                    .unwrap_or_default()
                                            )
                                        })
                                        .collect();
                                    let js = format!(
                                        "window.__spatialAutofillShowPicker && window.__spatialAutofillShowPicker([{}]);",
                                        items.join(",")
                                    );
                                    frame.execute_java_script(
                                        Some(&js.as_str().into()),
                                        Some(&"".into()),
                                        0,
                                    );
                                }
                            }
                        }
                    }
                }
            } else {
                hotkeys::open_passwords(session, gpu, vault.as_ref());
            }
        }
        ContextAction::ClosePage => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(tid) = target {
                if let Some(index) = session
                    .pages()
                    .iter()
                    .position(|p| p.browser.identifier() == tid)
                {
                    let _ = session.close_at(index);
                }
            }
        }
        ContextAction::NewPage => {
            dismiss_context_menu(session, context_menu);
            hotkeys::open_new(session, gpu, typed_history, settings);
        }
        ContextAction::SaveWorkspace => {
            dismiss_context_menu(session, context_menu);
            workspaces::add_and_switch(workspaces, workspace_runtime, session, gpu);
        }
        ContextAction::Reader => {
            let target = context_menu.as_ref().and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(tid) = target {
                if let Some(index) = session
                    .pages()
                    .iter()
                    .position(|p| p.browser.identifier() == tid)
                {
                    session.bring_to_front(index);
                }
            }
            hotkeys::toggle_reader_mode(session, settings);
        }
        ContextAction::Dismiss => {
            dismiss_context_menu(session, context_menu);
        }
    }
}

fn handle_password_action(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    generated_password: &mut Option<String>,
    pending_save_offer: &mut Option<PendingSaveOffer>,
    browser_id: i32,
    action: PasswordAction,
) {
    match action {
        PasswordAction::Unlock {
            password,
            create,
            confirm,
        } => {
            let result = if create {
                if confirm.as_deref() != Some(password.as_str()) {
                    Err("Passwords do not match".into())
                } else if password.len() < 8 {
                    Err("Use at least 8 characters".into())
                } else {
                    vault::create(&password).map_err(|e| e.to_string())
                }
            } else {
                VaultSession::unlock(&password).map_err(|e| e.to_string())
            };
            match result {
                Ok(session_vault) => {
                    *vault = Some(session_vault);
                    // Close unlock page if this came from it, then open list.
                    if let Some(index) = session
                        .pages()
                        .iter()
                        .position(|p| p.browser.identifier() == browser_id)
                    {
                        let _ = session.close_at(index);
                    }
                    hotkeys::open_passwords(session, gpu, vault.as_ref());
                    // Re-inject autofill on open pages so query works immediately.
                    reinject_autofill(session);
                }
                Err(msg) => {
                    refresh_unlock_page(session, gpu, browser_id, create, Some(&msg));
                }
            }
        }
        PasswordAction::OpenList => {
            hotkeys::open_passwords(session, gpu, vault.as_ref());
        }
        PasswordAction::Query { origin } => {
            let Some(v) = vault.as_ref() else {
                hotkeys::open_passwords(session, gpu, None);
                return;
            };
            let matches = v.entries_for_origin(&origin);
            if let Some(page) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id)
            {
                if let Some(frame) = page.browser.main_frame() {
                    match matches.len() {
                        0 => {}
                        1 => {
                            let js = fill_entry_js(matches[0]);
                            frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                        }
                        _ => {
                            let items: Vec<String> = matches
                                .iter()
                                .map(|e| {
                                    format!(
                                        "{{id:{},username:{}}}",
                                        serde_json::to_string(&e.id).unwrap_or_default(),
                                        serde_json::to_string(&e.username).unwrap_or_default()
                                    )
                                })
                                .collect();
                            let js = format!(
                                "window.__spatialAutofillShowPicker && window.__spatialAutofillShowPicker([{}]);",
                                items.join(",")
                            );
                            frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                        }
                    }
                }
            }
        }
        PasswordAction::Fill { id } => {
            let Some(v) = vault.as_ref() else {
                return;
            };
            let Some(entry) = v.get(&id).cloned() else {
                return;
            };
            // Prefer the page that requested fill; else focused non-ephemeral.
            let target = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id && !p.ephemeral)
                .or_else(|| session.pages().iter().rev().find(|p| !p.ephemeral));
            if let Some(page) = target {
                if let Some(frame) = page.browser.main_frame() {
                    let js = fill_entry_js(&entry);
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
        PasswordAction::SaveOffer {
            origin,
            username,
            password,
        } => {
            let Some(v) = vault.as_ref() else {
                return;
            };
            if origin.is_empty() || password.is_empty() || v.is_never_save(&origin) {
                return;
            }
            let existing = v.entries_for_origin(&origin);
            let id = existing
                .iter()
                .find(|e| e.username == username)
                .map(|e| e.id.clone())
                .unwrap_or_default();
            if let Some(same) = existing.iter().find(|e| e.username == username) {
                if same.password == password {
                    return;
                }
            }
            // Keep across the login navigation; also try to show now (SPA).
            *pending_save_offer = Some(PendingSaveOffer {
                origin: origin.clone(),
                username: username.clone(),
                password: password.clone(),
                id: id.clone(),
            });
            if let Some(page) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id)
            {
                if let Some(frame) = page.browser.main_frame() {
                    let payload = format!(
                        "{{origin:{},username:{},password:{},id:{}}}",
                        serde_json::to_string(&origin).unwrap_or_default(),
                        serde_json::to_string(&username).unwrap_or_default(),
                        serde_json::to_string(&password).unwrap_or_default(),
                        serde_json::to_string(&id).unwrap_or_default(),
                    );
                    let js = format!(
                        "window.__spatialAutofillShowSave && window.__spatialAutofillShowSave({payload});"
                    );
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
        PasswordAction::Save {
            origin,
            username,
            password,
            id,
        } => {
            let Some(v) = vault.as_mut() else {
                return;
            };
            let entry = VaultEntry {
                id: id.unwrap_or_default(),
                origin: vault::normalize_origin(&origin),
                username,
                password,
                email: None,
                address_line1: None,
                city: None,
                postal_code: None,
                country: None,
                notes: None,
                updated_at: vault::now_unix(),
            };
            if let Err(e) = v.upsert(entry) {
                log::warn!("vault save: {e}");
            }
            *pending_save_offer = None;
        }
        PasswordAction::Never { origin } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.add_never_save(&origin);
            }
            *pending_save_offer = None;
        }
        PasswordAction::Delete { id } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.remove(&id);
            }
            refresh_passwords_page(session, gpu, vault.as_ref(), generated_password.as_deref(), browser_id);
        }
        PasswordAction::Upsert {
            id,
            origin,
            username,
            password,
            email,
        } => {
            if let Some(v) = vault.as_mut() {
                let entry = VaultEntry {
                    id: id.unwrap_or_default(),
                    origin: vault::normalize_origin(&origin),
                    username,
                    password,
                    email,
                    address_line1: None,
                    city: None,
                    postal_code: None,
                    country: None,
                    notes: None,
                    updated_at: vault::now_unix(),
                };
                let _ = v.upsert(entry);
            }
            refresh_passwords_page(session, gpu, vault.as_ref(), generated_password.as_deref(), browser_id);
        }
        PasswordAction::RemoveNever { origin } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.remove_never_save(&origin);
            }
            refresh_passwords_page(session, gpu, vault.as_ref(), generated_password.as_deref(), browser_id);
        }
        PasswordAction::Generate { length, symbols } => {
            *generated_password = Some(vault::generate_password(length, symbols));
            refresh_passwords_page(session, gpu, vault.as_ref(), generated_password.as_deref(), browser_id);
        }
    }
}

fn fill_entry_js(entry: &VaultEntry) -> String {
    let obj = serde_json::json!({
        "username": entry.username,
        "password": entry.password,
        "email": entry.email,
        "address_line1": entry.address_line1,
        "city": entry.city,
        "postal_code": entry.postal_code,
        "country": entry.country,
    });
    format!(
        "window.__spatialAutofillFill && window.__spatialAutofillFill({});",
        obj
    )
}

fn reinject_autofill(session: &Session) {
    let bridge = autofill_bridge::script(&session.theme());
    for page in session.pages() {
        if page.ephemeral {
            continue;
        }
        if let Some(frame) = page.browser.main_frame() {
            frame.execute_java_script(Some(&bridge.as_str().into()), Some(&"".into()), 0);
        }
    }
}

fn refresh_unlock_page(
    session: &mut Session,
    gpu: &GpuState,
    browser_id: i32,
    create: bool,
    error: Option<&str>,
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
    let url = pages::passwords_list::unlock_url(&session.theme(), create, error);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn refresh_passwords_page(
    session: &mut Session,
    gpu: &GpuState,
    vault: Option<&VaultSession>,
    generated: Option<&str>,
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        // Action came from a normal page (e.g. generate from nowhere) — open list.
        hotkeys::open_passwords(session, gpu, vault);
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = match vault {
        Some(v) => {
            pages::passwords_list::page_url(&session.theme(), &v.data.entries, &v.data.never_save, generated)
        }
        None => pages::passwords_list::unlock_url(&session.theme(), !vault::exists(), None),
    };
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

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
    workspaces: &WorkspaceStore,
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
