// Canvas-level keyboard shortcuts — closing/opening/reloading a page,
// cycling focus, zooming, back/forward navigation, theme switching, the
// canvas-view reset, auto-layout, the page switcher, bookmarks,
// downloads, history, workspaces, settings, and the F1 help page —
// that must never reach a page's own content (unlike everything routed
// through input::KeyboardInput, which forwards to whichever CEF
// browser is active). Kept separate from that module for exactly that
// reason: this is about the canvas, not about one page's text input.
// Canvas pan/zoom itself is mouse-driven (middle-drag / Ctrl+scroll,
// see app.rs) — only its keyboard reset lives here. The HTML/CSS/JS for
// the pages some of these open (F1 help, bookmarks/downloads/history/
// workspace/settings lists, omnibox, switcher) lives in pages/, not
// here — this file is only "what does each shortcut do".

use crate::browser;
use crate::clipboard_bridge;
use crate::output::GpuState;
use crate::pages;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::downloads::DownloadRecord;
use crate::persistence::history::HistoryEntry;
use crate::persistence::settings::AppSettings;
use crate::persistence::workspaces::Workspace;
use crate::reader_mode;
use crate::session::Session;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// Recognizes the canvas-level shortcuts and applies them. Returns `true`
/// if `event` was one of them (the caller should *not* also forward it to
/// the active page), `false` otherwise.
pub fn handle(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &mut Vec<Bookmark>,
    typed_history: &[String],
    downloads: &[DownloadRecord],
    history: &[HistoryEntry],
    workspaces: &[Workspace],
    settings: &AppSettings,
) -> bool {
    if event.state != ElementState::Pressed {
        return false;
    }

    // F1 works with no modifier — it's a dedicated function key, not a
    // letter that could be someone typing into a page.
    if event.physical_key == PhysicalKey::Code(KeyCode::F1) {
        open_help(session, gpu);
        return true;
    }

    // Alt+Left/Right for back/forward (standard browser convention),
    // checked separately from the Ctrl+ bindings below since it's a
    // different modifier.
    if modifiers.alt_key() && !modifiers.control_key() {
        match event.physical_key {
            PhysicalKey::Code(KeyCode::ArrowLeft) => {
                go_back(session);
                return true;
            }
            PhysicalKey::Code(KeyCode::ArrowRight) => {
                go_forward(session);
                return true;
            }
            _ => {}
        }
    }

    if !modifiers.control_key() {
        return false;
    }

    match event.physical_key {
        // --- Pages ---
        PhysicalKey::Code(KeyCode::KeyT) => {
            if modifiers.shift_key() {
                reopen_closed(session, gpu);
            } else {
                open_new(session, gpu, typed_history, settings);
            }
            true
        }
        // Ctrl+Shift+W (Lists group: workspace list) shares this
        // physical key with plain Ctrl+W (close page) — same situation
        // as Digit0/Space above.
        PhysicalKey::Code(KeyCode::KeyW) => {
            if modifiers.shift_key() {
                open_workspaces(session, gpu, workspaces);
            } else {
                session.close_topmost();
            }
            true
        }
        // Ctrl+Shift+R (reader mode) shares this physical key with plain
        // Ctrl+R (reload) — same Shift-disambiguates-a-second-action
        // convention as KeyC/KeyW/Digit0/Space in this match.
        PhysicalKey::Code(KeyCode::KeyR) => {
            if modifiers.shift_key() {
                toggle_reader_mode(session, settings);
            } else {
                reload_focused(session);
            }
            true
        }
        PhysicalKey::Code(KeyCode::KeyV) => {
            paste_into_focused(session);
            true
        }
        // Plain Ctrl+C is deliberately left unhandled (falls through to
        // the page itself) — it's already how in-page text selection
        // gets copied, via clipboard_bridge's injected 'copy' listener.
        // Shift disambiguates a *different* action on the same physical
        // key, same convention as every other Shift-shared binding here.
        PhysicalKey::Code(KeyCode::KeyC) => {
            if modifiers.shift_key() {
                copy_focused_url(session);
                true
            } else {
                false
            }
        }
        // Page content zoom (CEF's own zoom_level), distinct from
        // Ctrl+Space's canvas-rect zoom below. Equal shares its physical
        // key with `+` on a US layout, matching every browser's Ctrl+=
        // convention for zoom in.
        PhysicalKey::Code(KeyCode::Equal) => {
            page_zoom(session, cef::ZoomCommand::IN);
            true
        }
        PhysicalKey::Code(KeyCode::Minus) => {
            page_zoom(session, cef::ZoomCommand::OUT);
            true
        }
        // Ctrl+Shift+0 (Canvas group: reset canvas view) shares this
        // physical key with plain Ctrl+0 (reset page zoom) — one match
        // arm, not a missing group.
        PhysicalKey::Code(KeyCode::Digit0) => {
            if modifiers.shift_key() {
                session.reset_viewport();
            } else {
                page_zoom(session, cef::ZoomCommand::RESET);
            }
            true
        }

        // --- Lists ---
        PhysicalKey::Code(KeyCode::KeyD) => {
            bookmark_focused(session, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyB) => {
            open_bookmarks(session, gpu, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyJ) => {
            open_downloads(session, gpu, downloads);
            true
        }
        PhysicalKey::Code(KeyCode::KeyH) => {
            open_history(session, gpu, history);
            true
        }
        PhysicalKey::Code(KeyCode::KeyK) => {
            open_switcher(session, gpu);
            true
        }
        // Alias for F1 — Ctrl+/ is the "show shortcuts" convention in
        // Linear/Slack/Notion/GitHub, and doesn't need the function-key
        // row (behind an Fn layer on a lot of laptops).
        PhysicalKey::Code(KeyCode::Slash) => {
            open_help(session, gpu);
            true
        }
        // Ctrl+, is the cross-app Preferences convention (Chrome,
        // VSCode, Slack).
        PhysicalKey::Code(KeyCode::Comma) => {
            open_settings(session, gpu, settings);
            true
        }

        // --- Canvas ---
        PhysicalKey::Code(KeyCode::KeyG) => {
            auto_layout(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::Tab) => {
            // Focus == topmost (last), so cycling focus is just rotating
            // z-order: rotate_left brings the front page to the back,
            // making the *next* page topmost/focused each press.
            session.rotate_focus(modifiers.shift_key());
            true
        }
        // Ctrl+Shift+Space (Other group: cycle theme) shares this
        // physical key with plain Ctrl+Space (zoom to canvas) — same
        // situation as Digit0 above.
        PhysicalKey::Code(KeyCode::Space) => {
            if modifiers.shift_key() {
                session.cycle_theme();
            } else {
                let size = gpu.window.inner_size();
                session.toggle_zoom_focused(
                    (size.width as f32, size.height as f32),
                    gpu.window.scale_factor(),
                );
            }
            true
        }

        _ => false,
    }
}

// --- Pages ---

/// Opens the omnibox page (type-to-search-or-navigate) rather than a
/// fixed URL directly — ephemeral like F1/bookmarks, since submitting it
/// immediately closes and replaces it with a real page for the resolved
/// destination (see app.rs's PENDING_OMNIBOX handling); if left
/// untouched there's nothing worth freezing into session.json either.
fn open_new(
    session: &mut Session,
    gpu: &GpuState,
    typed_history: &[String],
    settings: &AppSettings,
) {
    let size = gpu.window.inner_size();
    let rect = session.cascade_rect((size.width as f32, size.height as f32));
    let url = pages::omnibox::page_url(
        &session.theme(),
        typed_history,
        &settings.default_search_engine,
    );
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Reopens the most recently closed page (if any) at its former rect,
/// bringing it to front.
fn reopen_closed(session: &mut Session, gpu: &GpuState) {
    if let Some((rect, url)) = session.pop_closed() {
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
    }
}

fn reload_focused(session: &Session) {
    if let Some(page) = session.pages().last() {
        page.browser.reload();
    }
}

/// Reads the system clipboard directly (via `wl-paste`) and inserts it
/// at the cursor/selection of whatever's focused inside the topmost
/// page, bypassing CEF's own (non-functional in this windowless/OSR
/// embedding — see clipboard_bridge.rs) paste handling entirely rather
/// than forwarding Ctrl+V to it at all. `execCommand('insertText', ...)`
/// — not setting `.value` directly — because it fires the same events a
/// real paste would and works in both plain inputs/textareas and
/// contenteditable elements.
fn paste_into_focused(session: &Session) {
    let Some(page) = session.pages().last() else {
        return;
    };
    let Ok(output) = std::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return; // e.g. clipboard empty — wl-paste exits non-zero
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.is_empty() {
        return;
    }
    if let Some(frame) = page.browser.main_frame() {
        let script = format!(
            "document.execCommand('insertText', false, {});",
            clipboard_bridge::js_literal(&text)
        );
        frame.execute_java_script(Some(&script.as_str().into()), Some(&"".into()), 0);
    }
}

/// Ctrl+Shift+C: copies the focused (topmost) page's current URL to the
/// system clipboard — there's no address bar to select it from at all
/// (see the Settings page's title/URL label toggle for the other half
/// of that gap), so this is the only way to grab it without going
/// through the page's own UI. Same `wl-copy` mechanism as
/// clipboard_bridge's in-page copy bridge, just triggered from the
/// canvas instead of a page's own 'copy' event.
fn copy_focused_url(session: &Session) {
    let Some(page) = session.pages().last() else {
        return;
    };
    let url = page.url();
    if let Err(e) = std::process::Command::new("wl-copy").arg(&url).spawn() {
        log::warn!("wl-copy failed (couldn't copy page URL): {e}");
    }
}

/// Ctrl+Shift+R: toggles the focused (topmost) page's reader mode —
/// see reader_mode.rs for the extraction script and browser::Page's
/// `reader_mode` field for why toggling off just reloads instead of
/// reversing the rewrite in place.
fn toggle_reader_mode(session: &Session, settings: &AppSettings) {
    let Some(page) = session.pages().last() else {
        return;
    };
    if page.reader_mode.get() {
        page.browser.reload();
        page.reader_mode.set(false);
        return;
    }
    let theme = reader_mode::READER_THEMES
        .get(settings.reader_theme)
        .unwrap_or(&reader_mode::READER_THEMES[0]);
    if let Some(frame) = page.browser.main_frame() {
        frame.execute_java_script(
            Some(&reader_mode::extract_script(theme).as_str().into()),
            Some(&"".into()),
            0,
        );
        page.reader_mode.set(true);
    }
}

fn go_back(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_back() != 0 {
            page.browser.go_back();
        }
    }
}

fn go_forward(session: &Session) {
    if let Some(page) = session.pages().last() {
        if page.browser.can_go_forward() != 0 {
            page.browser.go_forward();
        }
    }
}

fn page_zoom(session: &Session, command: cef::ZoomCommand) {
    if let Some(page) = session.pages().last() {
        if let Some(host) = page.browser.host() {
            host.zoom(command);
        }
    }
}

// --- Lists ---

/// Bookmarks the focused page's current URL, if it isn't already saved.
/// Refuses on an ephemeral page (F1 help, the bookmarks list itself) —
/// otherwise Ctrl+D on one of those saves its entire generated HTML as a
/// "URL".
fn bookmark_focused(session: &Session, bookmarks: &mut Vec<Bookmark>) {
    let Some(page) = session.pages().last() else {
        return;
    };
    if page.ephemeral {
        return;
    }
    let Some(url) = page
        .browser
        .main_frame()
        .map(|frame| cef::CefString::from(&frame.url()).to_string())
    else {
        return;
    };
    if bookmarks.iter().any(|b| b.url == url) {
        return;
    }
    bookmarks.push(Bookmark {
        url,
        title: None,
        folder: None,
    });
    bookmarks::save(bookmarks);
}

fn open_bookmarks(session: &mut Session, gpu: &GpuState, bookmarks: &[Bookmark]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+J downloads list.
fn open_downloads(session: &mut Session, gpu: &GpuState, downloads: &[DownloadRecord]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+H history list.
fn open_history(session: &mut Session, gpu: &GpuState, history: &[HistoryEntry]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::history_list::page_url(&session.theme(), history);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+Shift+W workspace list.
fn open_workspaces(session: &mut Session, gpu: &GpuState, workspaces: &[Workspace]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::workspace_list::page_url(&session.theme(), workspaces);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+, settings page.
fn open_settings(session: &mut Session, gpu: &GpuState, settings: &AppSettings) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::settings_list::page_url(&session.theme(), settings);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+K page switcher: a filterable list of every open,
/// non-ephemeral page. Ephemeral pages (F1 help, bookmarks list, and
/// this switcher itself) are excluded — see pages::switcher::page_url.
fn open_switcher(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.6).clamp(360.0, 640.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let entries: Vec<(i32, String)> = session
        .pages()
        .iter()
        .filter(|p| !p.ephemeral)
        .map(|p| (p.browser.identifier(), p.url()))
        .collect();
    let url = pages::switcher::page_url(&session.theme(), &entries);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

// --- Canvas ---

/// Rearranges every open page into a grid filling the current window —
/// see `Session::auto_layout`.
fn auto_layout(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    session.auto_layout(
        (size.width as f32, size.height as f32),
        gpu.window.scale_factor(),
    );
}

// --- Other ---

fn open_help(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = session.centered_rect((size.width as f32, size.height as f32), (w, h));
    let url = pages::help::page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}
