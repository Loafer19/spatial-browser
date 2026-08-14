// Routes winit window events to the right page: hit-tests the cursor
// against the canvas (topmost page first), forwards mouse/keyboard input
// to that page's CEF browser in its own local coordinates, and drives
// per-page dragging (Alt+Left-drag moves a page — same convention as
// borderless-window drag in most Linux window managers, including
// Hyprland's own `bindm ... movewindow`). Camera is identity for now (no
// pan/zoom): canvas space == screen space.

use crate::browser::{self, Page};
use crate::hotkeys;
use crate::input::{KeyboardInput, MouseInput};
use crate::output::{FrameOutcome, GpuState, PageDraw, Rect};
use cef::{ImplBrowser, ImplBrowserHost};
use cef_bridge::CURSOR;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{WindowAttributes, WindowId},
};

#[derive(Default)]
pub struct App {
    state: Option<GpuState>,
    pages: Vec<Page>,
    mouse: MouseInput,
    keyboard: KeyboardInput,
    // Raw window-space physical cursor position — updated on every
    // CursorMoved, consulted by MouseInput/MouseWheel events (which don't
    // carry a position of their own) to re-hit-test and to compute
    // whichever page's local coordinates.
    cursor_window: (f32, f32),
    modifiers: ModifiersState,
    // Window size (physical pixels) that the current page rects are laid
    // out against. Tiling window managers often settle the window into
    // its real geometry via a `Resized` event shortly *after* creation —
    // rather than trust the size seen in `resumed()` as final, every
    // `Resized` rescales existing page rects proportionally against
    // whatever size they were last laid out for, keeping draw and
    // hit-test in agreement no matter when the "real" size arrives.
    canvas_size: (f32, f32),
    // Offset from the dragged (always-topmost, since dragging brings a
    // page to front) page's rect origin to the cursor, set at drag start.
    dragging: Option<(f32, f32)>,
}

/// Topmost page (last in z-order) whose rect contains the point, if any.
fn hit_test(pages: &[Page], x: f32, y: f32) -> Option<usize> {
    pages.iter().rposition(|p| p.rect.contains(x, y))
}

/// Moves the page at `index` to the end of z-order (topmost) and returns
/// its new index.
fn bring_to_front(pages: &mut Vec<Page>, index: usize) -> usize {
    let page = pages.remove(index);
    pages.push(page);
    pages.len() - 1
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default().with_title("spatial-browser"))
                .expect("failed to create window"),
        );
        let state = pollster::block_on(GpuState::new(window.clone()));

        // Two pages side by side, with margins — proves the canvas holds
        // more than one independently-rendered, independently-placed page
        // before anything fancier (resize handles, grouping, pan/zoom).
        let size = window.inner_size();
        let margin = 24.0;
        let gap = 24.0;
        let page_w = (size.width as f32 - margin * 2.0 - gap) / 2.0;
        let page_h = size.height as f32 - margin * 2.0;
        let rects = [
            Rect {
                x: margin,
                y: margin,
                w: page_w,
                h: page_h,
            },
            Rect {
                x: margin + page_w + gap,
                y: margin,
                w: page_w,
                h: page_h,
            },
        ];
        let urls = [
            "https://www.google.com",
            // Hex colors avoided deliberately: an unescaped `#` in a `data:`
            // URL starts a fragment, silently truncating everything after
            // it from the actual document.
            "data:text/html,<body style=\"background:rgb(42,106,74);color:rgb(255,255,255);font-family:sans-serif;font-size:48px;display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">Page 2</body>",
        ];
        self.pages = rects
            .into_iter()
            .zip(urls)
            .map(|(rect, url)| browser::spawn(&state, &window, url, rect))
            .collect();
        self.canvas_size = (size.width as f32, size.height as f32);

        self.state = Some(state);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);

                let (old_w, old_h) = self.canvas_size;
                let (new_w, new_h) = (size.width as f32, size.height as f32);
                if old_w > 0.0 && old_h > 0.0 {
                    let (scale_x, scale_y) = (new_w / old_w, new_h / old_h);
                    let dpi_scale = state.window.scale_factor();
                    for page in &mut self.pages {
                        let rect = Rect {
                            x: page.rect.x * scale_x,
                            y: page.rect.y * scale_y,
                            w: page.rect.w * scale_x,
                            h: page.rect.h * scale_y,
                        };
                        page.set_rect(rect, dpi_scale);
                    }
                }
                self.canvas_size = (new_w, new_h);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                self.cursor_window = (position.x as f32, position.y as f32);

                if let Some(offset) = self.dragging {
                    let page = self.pages.last_mut().expect("dragging with no pages");
                    let rect = Rect {
                        x: self.cursor_window.0 - offset.0,
                        y: self.cursor_window.1 - offset.1,
                        w: page.rect.w,
                        h: page.rect.h,
                    };
                    page.set_rect(rect, scale);
                    return;
                }

                if let Some(i) = hit_test(&self.pages, self.cursor_window.0, self.cursor_window.1)
                {
                    let page = &self.pages[i];
                    let local = (
                        (self.cursor_window.0 - page.rect.x) as f64,
                        (self.cursor_window.1 - page.rect.y) as f64,
                    );
                    let host = page.browser.host();
                    self.mouse.cursor_moved(local, scale, host.as_ref());
                }
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                if button == MouseButton::Left && self.modifiers.alt_key() {
                    match element_state {
                        ElementState::Pressed => {
                            if let Some(i) =
                                hit_test(&self.pages, self.cursor_window.0, self.cursor_window.1)
                            {
                                let top = bring_to_front(&mut self.pages, i);
                                let rect = self.pages[top].rect;
                                self.dragging = Some((
                                    self.cursor_window.0 - rect.x,
                                    self.cursor_window.1 - rect.y,
                                ));
                            }
                        }
                        ElementState::Released => self.dragging = None,
                    }
                    return;
                }

                let Some(i) = hit_test(&self.pages, self.cursor_window.0, self.cursor_window.1)
                else {
                    return;
                };
                let i = if element_state == ElementState::Pressed {
                    bring_to_front(&mut self.pages, i)
                } else {
                    i
                };
                let page = &self.pages[i];
                let host = page.browser.host();
                self.mouse.button(element_state, button, host.as_ref());
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(i) = hit_test(&self.pages, self.cursor_window.0, self.cursor_window.1)
                {
                    let host = self.pages[i].browser.host();
                    self.mouse.wheel(delta, host.as_ref());
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(i) = hit_test(&self.pages, self.cursor_window.0, self.cursor_window.1)
                {
                    let host = self.pages[i].browser.host();
                    self.mouse.cursor_left(host.as_ref());
                }
            }
            WindowEvent::Focused(focused) => {
                for page in &self.pages {
                    if let Some(host) = page.browser.host() {
                        host.set_focus(focused as _);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                self.keyboard.modifiers_changed(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if hotkeys::handle(&event, self.modifiers, &mut self.pages, state, &state.window) {
                    return;
                }
                // No real focus model yet — the topmost (most recently
                // clicked) page is the closest proxy for "active page".
                if let Some(page) = self.pages.last() {
                    let host = page.browser.host();
                    self.keyboard.key_event(&event, host.as_ref());
                }
            }
            WindowEvent::RedrawRequested => {
                for page in &self.pages {
                    if let Some(host) = page.browser.host() {
                        host.send_external_begin_frame();
                    }
                }
                if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
                    state.window.set_cursor(icon);
                }

                let last_index = self.pages.len().saturating_sub(1);
                let textures: Vec<_> = self.pages.iter().map(Page::texture).collect();
                let draws: Vec<PageDraw> = self
                    .pages
                    .iter()
                    .zip(textures.iter())
                    .enumerate()
                    .map(|(i, (page, texture))| PageDraw {
                        rect: page.rect,
                        quad: &page.quad,
                        texture: texture.as_ref(),
                        focused: i == last_index,
                    })
                    .collect();

                match state.render(&draws) {
                    FrameOutcome::Rendered | FrameOutcome::Skip => {}
                    FrameOutcome::Reconfigure => {
                        let size = state.window.inner_size();
                        state.resize(size.width, size.height);
                    }
                    FrameOutcome::Fatal => {
                        log::error!("fatal surface validation error, exiting");
                        event_loop.exit();
                    }
                }
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}
