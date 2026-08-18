// Canvas-level keyboard shortcuts — closing/opening/reloading a page,
// cycling focus, zooming, back/forward navigation, theme switching, the
// canvas-view reset, auto-layout, the page switcher, bookmarks, and the
// F1 help page — that must never reach a page's own content (unlike
// everything routed through input::KeyboardInput, which forwards to
// whichever CEF browser is active). Kept separate from that module for
// exactly that reason: this is about the canvas, not about one page's
// text input. Canvas pan/zoom itself is mouse-driven (middle-drag /
// Ctrl+scroll, see app.rs) — only its keyboard reset lives here. The
// HTML/CSS/JS for the pages some of these open (F1 help, bookmarks
// list, omnibox, switcher) lives in pages/, not here — this file is
// only "what does each shortcut do".

use crate::browser;
use crate::output::{GpuState, Rect};
use crate::pages;
use crate::persistence::bookmarks::{self, Bookmark};
use crate::persistence::downloads::DownloadRecord;
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
    history: &[String],
    downloads: &[DownloadRecord],
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
        PhysicalKey::Code(KeyCode::KeyW) => {
            session.close_topmost();
            true
        }
        PhysicalKey::Code(KeyCode::KeyT) => {
            if modifiers.shift_key() {
                reopen_closed(session, gpu);
            } else {
                open_new(session, gpu, history);
            }
            true
        }
        PhysicalKey::Code(KeyCode::KeyR) => {
            reload_focused(session);
            true
        }
        PhysicalKey::Code(KeyCode::KeyD) => {
            bookmark_focused(session, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyB) => {
            open_bookmarks(session, gpu, bookmarks);
            true
        }
        PhysicalKey::Code(KeyCode::KeyG) => {
            auto_layout(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::KeyK) => {
            open_switcher(session, gpu);
            true
        }
        PhysicalKey::Code(KeyCode::KeyJ) => {
            open_downloads(session, gpu, downloads);
            true
        }
        PhysicalKey::Code(KeyCode::Tab) => {
            // Focus == topmost (last), so cycling focus is just rotating
            // z-order: rotate_left brings the front page to the back,
            // making the *next* page topmost/focused each press.
            session.rotate_focus(modifiers.shift_key());
            true
        }
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
        // Page content zoom (CEF's own zoom_level), distinct from
        // Ctrl+Space's canvas-rect zoom above. Equal shares its physical
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
        PhysicalKey::Code(KeyCode::Digit0) => {
            if modifiers.shift_key() {
                session.reset_viewport();
            } else {
                page_zoom(session, cef::ZoomCommand::RESET);
            }
            true
        }
        _ => false,
    }
}

/// Opens the omnibox page (type-to-search-or-navigate) rather than a
/// fixed URL directly — ephemeral like F1/bookmarks, since submitting it
/// immediately closes and replaces it with a real page for the resolved
/// destination (see app.rs's PENDING_OMNIBOX handling); if left
/// untouched there's nothing worth freezing into session.json either.
fn open_new(session: &mut Session, gpu: &GpuState, history: &[String]) {
    // Cascade each new page a bit so it doesn't land exactly on the last
    // one; wrap around after a few so it doesn't walk off-screen forever.
    // Placed by screen position (current view), then converted to world
    // space — so it lands in view regardless of current pan/zoom.
    let step = ((session.pages().len() % 8) as f32) * 32.0;
    let size = gpu.window.inner_size();
    let viewport = session.viewport();
    let world_origin = viewport.screen_to_world((48.0 + step, 48.0 + step));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: (size.width as f32 * 0.5).min(800.0) / viewport.zoom,
        h: (size.height as f32 * 0.5).min(600.0) / viewport.zoom,
    };
    let url = pages::omnibox::page_url(&session.theme(), history);
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

/// Rearranges every open page into a grid filling the current window —
/// see `Session::auto_layout`.
fn auto_layout(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    session.auto_layout(
        (size.width as f32, size.height as f32),
        gpu.window.scale_factor(),
    );
}

/// Opens the Ctrl+K page switcher: a filterable list of every open,
/// non-ephemeral page. Ephemeral pages (F1 help, bookmarks list, and
/// this switcher itself) are excluded — see pages::switcher::page_url.
fn open_switcher(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.6).clamp(360.0, 640.0);
    let viewport = session.viewport();
    let world_origin =
        viewport.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / viewport.zoom,
        h: h / viewport.zoom,
    };
    let entries: Vec<(i32, String)> = session
        .pages()
        .iter()
        .filter(|p| !p.ephemeral)
        .map(|p| (p.browser.identifier(), p.url()))
        .collect();
    let url = pages::switcher::page_url(&session.theme(), &entries);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Opens the Ctrl+J downloads list.
fn open_downloads(session: &mut Session, gpu: &GpuState, downloads: &[DownloadRecord]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let viewport = session.viewport();
    let world_origin =
        viewport.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / viewport.zoom,
        h: h / viewport.zoom,
    };
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn open_bookmarks(session: &mut Session, gpu: &GpuState, bookmarks: &[Bookmark]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let viewport = session.viewport();
    let world_origin =
        viewport.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / viewport.zoom,
        h: h / viewport.zoom,
    };
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn open_help(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let viewport = session.viewport();
    let world_origin =
        viewport.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / viewport.zoom,
        h: h / viewport.zoom,
    };
    let url = pages::help::page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}
