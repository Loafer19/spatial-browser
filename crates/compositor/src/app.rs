// Routes winit window events to the right page: hit-tests the cursor
// against the canvas (topmost page first), forwards mouse/keyboard input
// to that page's CEF browser in its own local coordinates, and drives
// per-page dragging (Alt+Left-drag moves a page — same convention as
// borderless-window drag in most Linux window managers, including
// Hyprland's own `bindm ... movewindow`) and resizing (dragging a page's
// bottom-right corner). Page rects live in world space; see camera.rs
// for the pan/zoom mapping to screen space that hit-testing and drawing
// convert through.

use crate::browser::{self, Page};
use crate::camera::Camera;
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
    // World<->screen mapping for the canvas — pan/zoom. Page rects are
    // stored in world space (see browser::Page::rect); everything that
    // touches screen-space input (hit-testing, dragging) converts through
    // this first.
    camera: Camera,
    // Offset from the dragged (always-topmost, since dragging brings a
    // page to front) page's rect origin (world space) to the cursor, set
    // at drag start.
    dragging: Option<(f32, f32)>,
    // (screen-space cursor position, camera offset) at the start of a
    // middle-button canvas pan.
    panning: Option<((f32, f32), (f32, f32))>,
    // Set while the topmost page's bottom-right corner (see
    // resize_hit_test) is being dragged to resize it. Its top-left stays
    // fixed; only the corner under the cursor moves.
    resizing: bool,
}

/// Screen-space size (regardless of zoom, like a scrollbar grip) of the
/// invisible grab region at a page's bottom-right corner used to resize
/// it — no modifier needed, distinguished from an ordinary click purely
/// by proximity to the corner.
const RESIZE_HANDLE: f32 = 18.0;
const MIN_PAGE_SIZE: f32 = 160.0;

/// Topmost page (last in z-order) whose bottom-right corner (screen
/// space) is within `RESIZE_HANDLE` of the cursor, if any.
fn resize_hit_test(pages: &[Page], camera: &Camera, cursor_screen: (f32, f32)) -> Option<usize> {
    let half = RESIZE_HANDLE / 2.0;
    pages.iter().rposition(|p| {
        let r = camera.rect_to_screen(p.rect);
        let corner = (r.x + r.w, r.y + r.h);
        (cursor_screen.0 - corner.0).abs() <= half && (cursor_screen.1 - corner.1).abs() <= half
    })
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

                if let Some((start, offset_at_start)) = self.panning {
                    let dx = self.cursor_window.0 - start.0;
                    let dy = self.cursor_window.1 - start.1;
                    self.camera.offset = (
                        offset_at_start.0 - dx / self.camera.zoom,
                        offset_at_start.1 - dy / self.camera.zoom,
                    );
                    return;
                }

                let cursor_world = self.camera.screen_to_world(self.cursor_window);

                if self.resizing {
                    let page = self.pages.last_mut().expect("resizing with no pages");
                    let rect = Rect {
                        x: page.rect.x,
                        y: page.rect.y,
                        w: (cursor_world.0 - page.rect.x).max(MIN_PAGE_SIZE),
                        h: (cursor_world.1 - page.rect.y).max(MIN_PAGE_SIZE),
                    };
                    page.set_rect(rect, scale);
                    return;
                }

                if let Some(offset) = self.dragging {
                    let page = self.pages.last_mut().expect("dragging with no pages");
                    let rect = Rect {
                        x: cursor_world.0 - offset.0,
                        y: cursor_world.1 - offset.1,
                        w: page.rect.w,
                        h: page.rect.h,
                    };
                    page.set_rect(rect, scale);
                    return;
                }

                if let Some(i) = hit_test(&self.pages, cursor_world.0, cursor_world.1) {
                    let page = &self.pages[i];
                    let local = (
                        (cursor_world.0 - page.rect.x) as f64,
                        (cursor_world.1 - page.rect.y) as f64,
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
                if button == MouseButton::Middle {
                    match element_state {
                        ElementState::Pressed => {
                            self.panning = Some((self.cursor_window, self.camera.offset));
                        }
                        ElementState::Released => self.panning = None,
                    }
                    return;
                }

                if button == MouseButton::Left && self.modifiers.alt_key() {
                    let cursor_world = self.camera.screen_to_world(self.cursor_window);
                    match element_state {
                        ElementState::Pressed => {
                            if let Some(i) = hit_test(&self.pages, cursor_world.0, cursor_world.1)
                            {
                                let top = bring_to_front(&mut self.pages, i);
                                let rect = self.pages[top].rect;
                                self.dragging =
                                    Some((cursor_world.0 - rect.x, cursor_world.1 - rect.y));
                            }
                        }
                        ElementState::Released => self.dragging = None,
                    }
                    return;
                }

                if button == MouseButton::Left {
                    match element_state {
                        ElementState::Pressed => {
                            if let Some(i) =
                                resize_hit_test(&self.pages, &self.camera, self.cursor_window)
                            {
                                bring_to_front(&mut self.pages, i);
                                self.resizing = true;
                                return;
                            }
                        }
                        ElementState::Released => {
                            if self.resizing {
                                self.resizing = false;
                                return;
                            }
                        }
                    }
                }

                let cursor_world = self.camera.screen_to_world(self.cursor_window);
                let Some(i) = hit_test(&self.pages, cursor_world.0, cursor_world.1) else {
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
                if self.modifiers.control_key() {
                    let scroll_y = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                        winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                    };
                    let factor = 1.1f32.powf(scroll_y);
                    self.camera.zoom_at(self.cursor_window, factor);
                    return;
                }

                let cursor_world = self.camera.screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(&self.pages, cursor_world.0, cursor_world.1) {
                    let host = self.pages[i].browser.host();
                    self.mouse.wheel(delta, host.as_ref());
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let cursor_world = self.camera.screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(&self.pages, cursor_world.0, cursor_world.1) {
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
                if hotkeys::handle(
                    &event,
                    self.modifiers,
                    &mut self.pages,
                    state,
                    &mut self.camera,
                ) {
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
                        rect: self.camera.rect_to_screen(page.rect),
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
