// Canvas-level keyboard shortcuts — closing/opening/reloading a page,
// cycling focus, zooming, back/forward navigation, theme switching, the
// canvas-view reset, bookmarks, and the F1 help page — that must never
// reach a page's own content (unlike everything routed through
// input::KeyboardInput, which forwards to whichever CEF browser is
// active). Kept separate from that module for exactly that reason: this
// is about the canvas, not about one page's text input. Canvas pan/zoom
// itself is mouse-driven (middle-drag / Ctrl+scroll, see app.rs) — only
// its keyboard reset lives here.

use crate::bookmarks::{self, Bookmark};
use crate::browser;
use crate::output::{GpuState, Rect, Theme};
use crate::session::Session;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

// No URL bar yet — new pages open here for now.
const NEW_PAGE_URL: &str = "https://www.google.com";

/// Recognizes the canvas-level shortcuts and applies them. Returns `true`
/// if `event` was one of them (the caller should *not* also forward it to
/// the active page), `false` otherwise.
pub fn handle(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &mut Vec<Bookmark>,
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
                open_new(session, gpu);
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
                session.reset_camera();
            } else {
                page_zoom(session, cef::ZoomCommand::RESET);
            }
            true
        }
        _ => false,
    }
}

fn open_new(session: &mut Session, gpu: &GpuState) {
    // Cascade each new page a bit so it doesn't land exactly on the last
    // one; wrap around after a few so it doesn't walk off-screen forever.
    // Placed by screen position (current view), then converted to world
    // space — so it lands in view regardless of current pan/zoom.
    let step = ((session.pages().len() % 8) as f32) * 32.0;
    let size = gpu.window.inner_size();
    let camera = session.camera();
    let world_origin = camera.screen_to_world((48.0 + step, 48.0 + step));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: (size.width as f32 * 0.5).min(800.0) / camera.zoom,
        h: (size.height as f32 * 0.5).min(600.0) / camera.zoom,
    };
    session.add_page(browser::spawn(gpu, &gpu.window, NEW_PAGE_URL, rect));
}

/// Reopens the most recently closed page (if any) at its former rect,
/// bringing it to front.
fn reopen_closed(session: &mut Session, gpu: &GpuState) {
    if let Some((rect, url)) = session.pop_closed() {
        session.add_page(browser::spawn(gpu, &gpu.window, &url, rect));
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
fn bookmark_focused(session: &Session, bookmarks: &mut Vec<Bookmark>) {
    let Some(page) = session.pages().last() else {
        return;
    };
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
    bookmarks.push(Bookmark { url });
    bookmarks::save(bookmarks);
}

fn open_bookmarks(session: &mut Session, gpu: &GpuState, bookmarks: &[Bookmark]) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.5).clamp(380.0, 640.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let camera = session.camera();
    let world_origin =
        camera.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / camera.zoom,
        h: h / camera.zoom,
    };
    let url = bookmarks_page_url(&session.theme(), bookmarks);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect));
}

/// Builds the bookmarks-list page's `data:` URL. Each entry is a real
/// `<a href="bookmark://{index}">` — clicking it is a normal navigation
/// CEF's `on_before_browse` (cef-bridge) intercepts and cancels, signaling
/// the index back to the compositor to open as a real new page instead.
/// The favicon is fetched live from the bookmark's own site
/// (`https://{host}/favicon.ico`) rather than captured/stored ourselves.
fn bookmarks_page_url(theme: &Theme, bookmarks: &[Bookmark]) -> String {
    let mut rows = String::new();
    if bookmarks.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No bookmarks yet &mdash; Ctrl+D on a page to add one.</p>",
            fg = theme.help_fg,
        ));
    }
    for (index, bookmark) in bookmarks.iter().enumerate() {
        let host = bookmarks::host_of(&bookmark.url);
        rows.push_str(&format!(
            "<a href=\"bookmark://{index}\" style=\"display:flex;align-items:center;gap:12px;\
             text-decoration:none;padding:10px 14px;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border};color:{fg}\">\
             <img src=\"https://{host}/favicon.ico\" width=\"16\" height=\"16\" \
             style=\"flex-shrink:0\" onerror=\"this.style.visibility='hidden'\">\
             <span style=\"overflow:hidden;text-overflow:ellipsis;white-space:nowrap\">{host}</span>\
             </a>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            fg = theme.help_fg,
        ));
    }
    format!(
        "data:text/html,<body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Bookmarks</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
    )
}

fn open_help(session: &mut Session, gpu: &GpuState) {
    let size = gpu.window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let camera = session.camera();
    let world_origin =
        camera.screen_to_world(((size.width as f32 - w) / 2.0, (size.height as f32 - h) / 2.0));
    let rect = Rect {
        x: world_origin.0,
        y: world_origin.1,
        w: w / camera.zoom,
        h: h / camera.zoom,
    };
    let url = help_page_url(&session.theme());
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect));
}

const HELP_ENTRIES: &[(&str, &str)] = &[
    ("Ctrl+T", "New page"),
    ("Ctrl+Shift+T", "Reopen closed page"),
    ("Ctrl+W", "Close page"),
    ("Ctrl+R", "Reload page"),
    ("Ctrl+D", "Bookmark page"),
    ("Ctrl+B", "Bookmarks list"),
    ("Ctrl+Tab", "Next page"),
    ("Ctrl+Shift+Tab", "Previous page"),
    ("Ctrl+Space", "Zoom to canvas"),
    ("Ctrl+= / Ctrl+-", "Zoom in / out"),
    ("Ctrl+0", "Reset zoom"),
    ("Alt+Left/Right", "Back / forward"),
    ("Alt+Left-drag", "Move a page"),
    ("Drag corner", "Resize a page"),
    ("Middle-drag", "Pan canvas"),
    ("Shift+Left-drag", "Pan canvas (trackpad)"),
    ("Ctrl+Scroll", "Zoom canvas"),
    ("Ctrl+Shift+0", "Reset canvas view"),
    ("Ctrl+Shift+Space", "Cycle UI theme"),
    ("F1", "This page"),
];

/// Builds the F1 help page's `data:` URL from `theme`'s palette, so it's
/// never out of sync with what's actually on screen. Built at runtime
/// (not a `concat!`-based const like the fixed entry list could be)
/// because the theme is chosen at runtime.
fn help_page_url(theme: &Theme) -> String {
    let mut rows = String::new();
    for (key, desc) in HELP_ENTRIES {
        rows.push_str(&format!(
            "<div style=\"display:flex;justify-content:space-between;align-items:center;\
             gap:16px;padding:10px 14px;background:{card_bg};border-radius:8px;\
             border:1px solid {card_border}\">\
             <kbd style=\"flex-shrink:0;white-space:nowrap;background:{key_bg};color:{key_fg};\
             padding:4px 10px;border-radius:6px;font-weight:600;font-size:13px\">{key}</kbd>\
             <span style=\"text-align:right;white-space:nowrap\">{desc}</span></div>",
            card_bg = theme.help_card_bg,
            card_border = theme.help_card_border,
            key_bg = theme.help_key_bg,
            key_fg = theme.help_key_fg,
        ));
    }
    // Hex colors avoided deliberately: an unescaped `#` in a `data:` URL
    // starts a fragment, silently truncating everything after it from
    // the actual document — every Theme field here is an rgb() string.
    format!(
        "data:text/html,<body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">\
         spatial-browser &mdash; shortcuts ({name})</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        bg = theme.help_bg,
        fg = theme.help_fg,
        heading = theme.help_heading,
        name = theme.name,
    )
}
