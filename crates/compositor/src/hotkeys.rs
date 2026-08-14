// Canvas-level keyboard shortcuts — closing/opening/reloading a page,
// cycling focus, zooming, back/forward navigation, and the F1 help page
// — that must never reach a page's own content (unlike everything
// routed through input::KeyboardInput, which forwards to whichever CEF
// browser is active). Kept separate from that module for exactly that
// reason: this is about the canvas, not about one page's text input.

use crate::browser::{self, Page};
use crate::output::{GpuState, Rect};
use cef::{ImplBrowser, ImplBrowserHost};
use winit::event::ElementState;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::Window;

// No URL bar yet — new pages open here for now.
const NEW_PAGE_URL: &str = "https://www.google.com";

/// Recognizes the canvas-level shortcuts and applies them. Returns `true`
/// if `event` was one of them (the caller should *not* also forward it to
/// the active page), `false` otherwise.
pub fn handle(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    pages: &mut Vec<Page>,
    gpu: &GpuState,
    window: &Window,
) -> bool {
    if event.state != ElementState::Pressed {
        return false;
    }

    // F1 works with no modifier — it's a dedicated function key, not a
    // letter that could be someone typing into a page.
    if event.physical_key == PhysicalKey::Code(KeyCode::F1) {
        open_help(pages, gpu, window);
        return true;
    }

    // Alt+Left/Right for back/forward (standard browser convention),
    // checked separately from the Ctrl+ bindings below since it's a
    // different modifier.
    if modifiers.alt_key() && !modifiers.control_key() {
        match event.physical_key {
            PhysicalKey::Code(KeyCode::ArrowLeft) => {
                go_back(pages);
                return true;
            }
            PhysicalKey::Code(KeyCode::ArrowRight) => {
                go_forward(pages);
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
            close_topmost(pages);
            true
        }
        PhysicalKey::Code(KeyCode::KeyT) => {
            open_new(pages, gpu, window);
            true
        }
        PhysicalKey::Code(KeyCode::KeyR) => {
            reload_focused(pages);
            true
        }
        PhysicalKey::Code(KeyCode::Tab) => {
            // Focus == topmost (last), so cycling focus is just rotating
            // z-order: rotate_left brings the front page to the back,
            // making the *next* page topmost/focused each press.
            if !pages.is_empty() {
                if modifiers.shift_key() {
                    pages.rotate_right(1);
                } else {
                    pages.rotate_left(1);
                }
            }
            true
        }
        PhysicalKey::Code(KeyCode::Space) => {
            toggle_zoom_focused(pages, window);
            true
        }
        // Page content zoom (CEF's own zoom_level), distinct from
        // Ctrl+Space's canvas-rect zoom above. Equal shares its physical
        // key with `+` on a US layout, matching every browser's Ctrl+=
        // convention for zoom in.
        PhysicalKey::Code(KeyCode::Equal) => {
            page_zoom(pages, cef::ZoomCommand::IN);
            true
        }
        PhysicalKey::Code(KeyCode::Minus) => {
            page_zoom(pages, cef::ZoomCommand::OUT);
            true
        }
        PhysicalKey::Code(KeyCode::Digit0) => {
            page_zoom(pages, cef::ZoomCommand::RESET);
            true
        }
        _ => false,
    }
}

fn close_topmost(pages: &mut Vec<Page>) {
    let Some(page) = pages.pop() else { return };
    if let Some(host) = page.browser.host() {
        host.close_browser(true as _);
    }
}

fn open_new(pages: &mut Vec<Page>, gpu: &GpuState, window: &Window) {
    // Cascade each new page a bit so it doesn't land exactly on the last
    // one; wrap around after a few so it doesn't walk off-screen forever.
    let step = ((pages.len() % 8) as f32) * 32.0;
    let size = window.inner_size();
    let rect = Rect {
        x: 48.0 + step,
        y: 48.0 + step,
        w: (size.width as f32 * 0.5).min(800.0),
        h: (size.height as f32 * 0.5).min(600.0),
    };
    pages.push(browser::spawn(gpu, window, NEW_PAGE_URL, rect));
}

fn reload_focused(pages: &[Page]) {
    if let Some(page) = pages.last() {
        page.browser.reload();
    }
}

fn go_back(pages: &[Page]) {
    if let Some(page) = pages.last() {
        if page.browser.can_go_back() != 0 {
            page.browser.go_back();
        }
    }
}

fn go_forward(pages: &[Page]) {
    if let Some(page) = pages.last() {
        if page.browser.can_go_forward() != 0 {
            page.browser.go_forward();
        }
    }
}

fn page_zoom(pages: &[Page], command: cef::ZoomCommand) {
    if let Some(page) = pages.last() {
        if let Some(host) = page.browser.host() {
            host.zoom(command);
        }
    }
}

fn toggle_zoom_focused(pages: &mut [Page], window: &Window) {
    let Some(page) = pages.last_mut() else {
        return;
    };
    let scale = window.scale_factor();
    match page.zoomed_from.take() {
        Some(previous_rect) => page.set_rect(previous_rect, scale),
        None => {
            let size = window.inner_size();
            let margin = 40.0;
            let zoomed_rect = Rect {
                x: margin,
                y: margin,
                w: size.width as f32 - margin * 2.0,
                h: size.height as f32 - margin * 2.0,
            };
            page.zoomed_from = Some(page.rect);
            page.set_rect(zoomed_rect, scale);
        }
    }
}

fn open_help(pages: &mut Vec<Page>, gpu: &GpuState, window: &Window) {
    let size = window.inner_size();
    let w = (size.width as f32 * 0.6).clamp(420.0, 720.0);
    let h = (size.height as f32 * 0.7).clamp(420.0, 760.0);
    let rect = Rect {
        x: (size.width as f32 - w) / 2.0,
        y: (size.height as f32 - h) / 2.0,
        w,
        h,
    };
    pages.push(browser::spawn(gpu, window, HELP_PAGE_URL, rect));
}

// Tokyo Night palette (this machine's active Omarchy theme, from
// /usr/share/omarchy/themes/tokyo-night/colors.toml) — as rgb(), not hex:
// an unescaped `#` in a `data:` URL starts a fragment, silently
// truncating everything after it from the actual document.
macro_rules! help_row {
    ($key:expr, $desc:expr) => {
        concat!(
            "<div style=\"",
            "display:flex;justify-content:space-between;align-items:center;",
            "padding:10px 14px;background:rgb(36,40,59);border-radius:8px;",
            "border:1px solid rgb(65,72,104)\">",
            "<kbd style=\"background:rgb(122,162,247);color:rgb(19,20,28);",
            "padding:4px 10px;border-radius:6px;font-weight:600;font-size:13px\">",
            $key,
            "</kbd><span>",
            $desc,
            "</span></div>"
        )
    };
}

const HELP_PAGE_URL: &str = concat!(
    "data:text/html,",
    "<body style=\"margin:0;padding:32px;background:rgb(26,27,38);",
    "color:rgb(169,177,214);font-family:ui-monospace,monospace;font-size:15px\">",
    "<h1 style=\"margin:0 0 20px;color:rgb(192,202,245);font-size:20px\">",
    "spatial-browser &mdash; shortcuts</h1>",
    "<div style=\"display:flex;flex-direction:column;gap:8px\">",
    help_row!("Ctrl+T", "New page"),
    help_row!("Ctrl+W", "Close focused page"),
    help_row!("Ctrl+R", "Reload focused page"),
    help_row!("Ctrl+Tab", "Next page (cycle focus)"),
    help_row!("Ctrl+Shift+Tab", "Previous page"),
    help_row!("Ctrl+Space", "Zoom focused page to canvas"),
    help_row!("Ctrl+= / Ctrl+-", "Zoom page content in / out"),
    help_row!("Ctrl+0", "Reset page content zoom"),
    help_row!("Alt+Left/Right", "Back / forward"),
    help_row!("Alt+Left-drag", "Move a page"),
    help_row!("F1", "This page"),
    "</div></body>",
);
