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
    session.add_page(browser::spawn(gpu, &gpu.window, NEW_PAGE_URL, rect, false));
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
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Builds the bookmarks-list page's `data:` URL — grouped by folder
/// (ungrouped entries first, then each named folder in order of first
/// appearance). Each row is a real `<a href="bookmark://open/{index}">`
/// plus a delete link and a rename form; all three are normal
/// navigations that CEF's `on_before_browse` (cef-bridge) intercepts and
/// cancels, signaling the action back to the compositor
/// (`app.rs`'s `PENDING_BOOKMARK` handling) instead of actually loading
/// `bookmark://...`. `pub(crate)` so app.rs can rebuild this page in
/// place after a delete/rename.
///
/// The favicon is fetched live from the bookmark's own site
/// (`https://{host}/favicon.ico`) rather than captured/stored ourselves;
/// a colored initial-letter tile sits underneath it as a fallback that
/// never needs the network, shown until (or unless) the real icon loads.
pub(crate) fn bookmarks_page_url(theme: &Theme, bookmarks: &[Bookmark]) -> String {
    let mut rows = String::new();
    if bookmarks.is_empty() {
        rows.push_str(&format!(
            "<p style=\"color:{fg};opacity:0.7\">No bookmarks yet &mdash; Ctrl+D on a page to add one.</p>",
            fg = theme.help_fg,
        ));
    }

    let mut folders: Vec<Option<&str>> = Vec::new();
    for bookmark in bookmarks {
        let folder = bookmark.folder.as_deref();
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }

    for folder in folders {
        if let Some(name) = folder {
            rows.push_str(&format!(
                "<h2 style=\"margin:12px 0 0;font-size:13px;text-transform:uppercase;\
                 letter-spacing:0.05em;color:{heading};opacity:0.8\">{name}</h2>",
                heading = theme.help_heading,
                name = html_escape(name),
            ));
        }
        for (index, bookmark) in bookmarks.iter().enumerate() {
            if bookmark.folder.as_deref() != folder {
                continue;
            }
            let host = bookmarks::host_of(&bookmark.url);
            let label = bookmarks::display_label(bookmark);
            let letter = label.chars().next().unwrap_or('?').to_uppercase();
            rows.push_str(&format!(
                "<div style=\"display:flex;align-items:center;gap:10px;padding:10px 14px;\
                 background:{card_bg};border-radius:8px;border:1px solid {card_border}\">\
                 <a href=\"bookmark://open/{index}\" style=\"display:flex;flex-shrink:0\">\
                 <span style=\"position:relative;width:20px;height:20px\">\
                 <span style=\"position:absolute;inset:0;border-radius:4px;background:{key_bg};\
                 color:{key_fg};display:flex;align-items:center;justify-content:center;\
                 font-size:11px;font-weight:700\">{letter}</span>\
                 <img src=\"https://{host}/favicon.ico\" width=\"20\" height=\"20\" \
                 style=\"position:absolute;inset:0;border-radius:4px\" \
                 onerror=\"this.style.display='none'\"></span></a>\
                 <form method=\"get\" action=\"bookmark://rename/{index}\" \
                 style=\"display:flex;align-items:center;gap:6px;flex:1;min-width:0;margin:0\">\
                 <span onclick=\"this.style.display='none';\
                 this.nextElementSibling.style.display='inline-block';\
                 this.nextElementSibling.focus();this.nextElementSibling.select()\" \
                 style=\"cursor:text;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;\
                 flex:1;color:{fg}\" title=\"Click to rename\">{label}</span>\
                 <input name=\"title\" value=\"{label_attr}\" \
                 style=\"display:none;flex:1;min-width:0;background:{bg};color:{fg};\
                 border:1px solid {card_border};border-radius:4px;padding:2px 6px;\
                 font:inherit;font-size:13px\">\
                 <input name=\"folder\" value=\"{folder_attr}\" placeholder=\"folder\" \
                 style=\"width:70px;flex-shrink:0;background:{bg};color:{fg};\
                 border:1px solid {card_border};border-radius:4px;padding:2px 6px;\
                 font:inherit;font-size:12px\">\
                 <button type=\"submit\" title=\"Save\" class=\"bm-icon-btn\" style=\"flex-shrink:0;\
                 margin-left:4px;display:flex;align-items:center;justify-content:center;\
                 width:26px;height:26px;border-radius:6px;background:{bg};color:{fg};\
                 opacity:0.7;border:none;cursor:pointer;font:inherit\">\
                 <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
                 <path d=\"M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z\"/></svg></button>\
                 </form>\
                 <a href=\"bookmark://delete/{index}\" title=\"Delete\" class=\"bm-icon-btn\" \
                 style=\"flex-shrink:0;margin-left:4px;display:flex;align-items:center;\
                 justify-content:center;width:26px;height:26px;border-radius:6px;\
                 background:{bg};color:{fg};opacity:0.7;text-decoration:none\">\
                 <svg width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\">\
                 <path d=\"M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z\"/>\
                 </svg></a>\
                 </div>",
                card_bg = theme.help_card_bg,
                card_border = theme.help_card_border,
                fg = theme.help_fg,
                key_bg = theme.help_key_bg,
                key_fg = theme.help_key_fg,
                bg = theme.help_bg,
                label_attr = html_escape(label),
                folder_attr = html_escape(bookmark.folder.as_deref().unwrap_or("")),
            ));
        }
    }

    format!(
        "data:text/html,<style>.bm-icon-btn:hover{{background:{key_bg}!important;\
         color:{key_fg}!important;opacity:1!important}}</style>\
         <body style=\"margin:0;padding:32px;background:{bg};color:{fg};\
         font-family:ui-monospace,monospace;font-size:15px\">\
         <h1 style=\"margin:0 0 20px;color:{heading};font-size:20px\">Bookmarks</h1>\
         <div style=\"display:flex;flex-direction:column;gap:8px\">{rows}</div></body>",
        key_bg = theme.help_key_bg,
        key_fg = theme.help_key_fg,
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
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
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
