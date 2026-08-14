// Canvas-level keyboard shortcuts — closing/opening a page — that must
// never reach a page's own content (unlike everything routed through
// input::KeyboardInput, which forwards to whichever CEF browser is
// active). Kept separate from that module for exactly that reason: this
// is about the canvas, not about one page's text input.

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
    if event.state != ElementState::Pressed || !modifiers.control_key() {
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
