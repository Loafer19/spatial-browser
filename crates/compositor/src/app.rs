// Routes winit window events to the right page: hit-tests the cursor
// against the canvas (topmost page first), forwards mouse/keyboard input
// to that page's CEF browser in its own local coordinates, and drives
// per-page dragging (Alt+Left-drag moves a page — same convention as
// borderless-window drag in most Linux window managers, including
// Hyprland's own `bindm ... movewindow`) and resizing (dragging a page's
// bottom-right corner). Page rects live in world space; see camera.rs
// for the pan/zoom mapping to screen space that hit-testing and drawing
// convert through. Canvas state itself (pages/camera/theme) lives in
// session.rs, persisted via persistence.rs.

use crate::bookmarks::{self, Bookmark};
use crate::browser::{self, Page};
use crate::camera::Camera;
use crate::hotkeys;
use crate::input::{KeyboardInput, MouseInput};
use crate::output::{FrameOutcome, GpuState, PageDraw, Rect, THEMES};
use crate::persistence;
use crate::session::Session;
use cef::{ImplBrowser, ImplBrowserHost};
use cef_bridge::{CURSOR, PENDING_BOOKMARK};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{CursorIcon, WindowAttributes, WindowId},
};

// Debounced: a save happens at most this often, however many mutations
// land in between (a drag/resize/zoom-drag calls a Session method on
// every mouse-move frame).
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);

pub struct App {
    state: Option<GpuState>,
    session: Session,
    // Loaded once at startup from bookmarks.json, saved immediately on
    // every change (Ctrl+D) — unlike session state, bookmark edits are
    // rare and deliberate, so there's no need for the debounce below.
    bookmarks: Vec<Bookmark>,
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
    // True whenever the cursor is over a resize corner (or actively
    // resizing) — overrides whatever cursor shape CEF asked for so the
    // resize affordance is visible before the user commits to a drag.
    resize_hover: bool,
    // Most recent icon CEF asked for via on_cursor_change (cef_bridge::
    // CURSOR) — CEF only fires that callback on an actual change, so this
    // has to be cached and re-applied every frame rather than only when
    // a fresh event arrives; otherwise, once resize_hover's override
    // takes the cursor, there's no later CEF event to hand it back.
    cef_cursor: CursorIcon,
    // Last time `persistence::save` ran — gates the debounce above.
    last_save: Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            state: None,
            session: Session::new(Vec::new(), Camera::default(), THEMES[0]),
            bookmarks: bookmarks::load(),
            mouse: MouseInput::default(),
            keyboard: KeyboardInput::default(),
            cursor_window: (0.0, 0.0),
            modifiers: ModifiersState::default(),
            canvas_size: (0.0, 0.0),
            dragging: None,
            panning: None,
            resizing: false,
            resize_hover: false,
            cef_cursor: CursorIcon::default(),
            last_save: Instant::now(),
        }
    }
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

        self.session = persistence::load(&state, &window).unwrap_or_else(|| {
            // No saved session yet (first run, or a load error) — two
            // pages side by side, with margins, proves the canvas holds
            // more than one independently-rendered, independently-placed
            // page.
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
                // Hex colors avoided deliberately: an unescaped `#` in a
                // `data:` URL starts a fragment, silently truncating
                // everything after it from the actual document.
                "data:text/html,<body style=\"background:rgb(42,106,74);color:rgb(255,255,255);font-family:sans-serif;font-size:48px;display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">Page 2</body>",
            ];
            let pages = rects
                .into_iter()
                .zip(urls)
                .map(|(rect, url)| browser::spawn(&state, &window, url, rect))
                .collect();
            Session::new(pages, Camera::default(), THEMES[0])
        });
        self.canvas_size = (window.inner_size().width as f32, window.inner_size().height as f32);

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
            WindowEvent::CloseRequested => {
                persistence::save(&self.session);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);

                let (old_w, old_h) = self.canvas_size;
                let (new_w, new_h) = (size.width as f32, size.height as f32);
                if old_w > 0.0 && old_h > 0.0 {
                    let (scale_x, scale_y) = (new_w / old_w, new_h / old_h);
                    let dpi_scale = state.window.scale_factor();
                    self.session.rescale_pages(scale_x, scale_y, dpi_scale);
                }
                self.canvas_size = (new_w, new_h);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                self.cursor_window = (position.x as f32, position.y as f32);

                if let Some((start, offset_at_start)) = self.panning {
                    let dx = self.cursor_window.0 - start.0;
                    let dy = self.cursor_window.1 - start.1;
                    let zoom = self.session.camera().zoom;
                    self.session.pan_camera_to((
                        offset_at_start.0 - dx / zoom,
                        offset_at_start.1 - dy / zoom,
                    ));
                    return;
                }

                let camera = self.session.camera();
                let cursor_world = camera.screen_to_world(self.cursor_window);
                self.resize_hover = self.resizing
                    || resize_hit_test(self.session.pages(), &camera, self.cursor_window).is_some();

                if self.resizing {
                    let page = self.session.pages().last().expect("resizing with no pages");
                    let rect = Rect {
                        x: page.rect.x,
                        y: page.rect.y,
                        w: (cursor_world.0 - page.rect.x).max(MIN_PAGE_SIZE),
                        h: (cursor_world.1 - page.rect.y).max(MIN_PAGE_SIZE),
                    };
                    self.session.set_topmost_rect(rect, scale);
                    return;
                }

                if let Some(offset) = self.dragging {
                    let page = self.session.pages().last().expect("dragging with no pages");
                    let rect = Rect {
                        x: cursor_world.0 - offset.0,
                        y: cursor_world.1 - offset.1,
                        w: page.rect.w,
                        h: page.rect.h,
                    };
                    self.session.set_topmost_rect(rect, scale);
                    return;
                }

                if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                    let page = &self.session.pages()[i];
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
                // Middle-drag pans (works with a real mouse); Shift+Left-drag
                // is the trackpad-friendly equivalent — most touchpads have
                // no reliable middle button or a sustained right-click-drag
                // gesture, but a plain click+drag with a held modifier works
                // everywhere, same as Alt+Left-drag below for moving a page.
                if button == MouseButton::Middle
                    || (button == MouseButton::Left && self.modifiers.shift_key())
                {
                    match element_state {
                        ElementState::Pressed => {
                            self.panning = Some((self.cursor_window, self.session.camera().offset));
                        }
                        ElementState::Released => self.panning = None,
                    }
                    return;
                }

                if button == MouseButton::Left && self.modifiers.alt_key() {
                    let cursor_world = self.session.camera().screen_to_world(self.cursor_window);
                    match element_state {
                        ElementState::Pressed => {
                            if let Some(i) =
                                hit_test(self.session.pages(), cursor_world.0, cursor_world.1)
                            {
                                let top = self.session.bring_to_front(i);
                                let rect = self.session.pages()[top].rect;
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
                            if let Some(i) = resize_hit_test(
                                self.session.pages(),
                                &self.session.camera(),
                                self.cursor_window,
                            ) {
                                self.session.bring_to_front(i);
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

                let cursor_world = self.session.camera().screen_to_world(self.cursor_window);
                let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1)
                else {
                    return;
                };
                let i = if element_state == ElementState::Pressed {
                    self.session.bring_to_front(i)
                } else {
                    i
                };
                let page = &self.session.pages()[i];
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
                    self.session.zoom_camera_at(self.cursor_window, factor);
                    return;
                }

                let cursor_world = self.session.camera().screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                    let host = self.session.pages()[i].browser.host();
                    self.mouse.wheel(delta, host.as_ref());
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let cursor_world = self.session.camera().screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                    let host = self.session.pages()[i].browser.host();
                    self.mouse.cursor_left(host.as_ref());
                }
            }
            WindowEvent::Focused(focused) => {
                for page in self.session.pages() {
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
                    &mut self.session,
                    state,
                    &mut self.bookmarks,
                ) {
                    return;
                }
                // No real focus model yet — the topmost (most recently
                // clicked) page is the closest proxy for "active page".
                if let Some(page) = self.session.pages().last() {
                    let host = page.browser.host();
                    self.keyboard.key_event(&event, host.as_ref());
                }
            }
            WindowEvent::RedrawRequested => {
                for page in self.session.pages() {
                    if let Some(host) = page.browser.host() {
                        host.send_external_begin_frame();
                    }
                }

                // Set by cef-bridge's OsrRequestHandler when a click
                // inside the bookmarks-list page (hotkeys::open_bookmarks)
                // hits one of its `bookmark://<index>` links — that
                // navigation was already canceled there; open a real page
                // for it here instead, cascaded like a new page so it
                // doesn't land exactly on top of the list.
                if let Some(index) = PENDING_BOOKMARK.with_borrow_mut(|pending| pending.take()) {
                    if let Some(bookmark) = self.bookmarks.get(index) {
                        let step = ((self.session.pages().len() % 8) as f32) * 32.0;
                        let size = state.window.inner_size();
                        let camera = self.session.camera();
                        let world_origin = camera.screen_to_world((48.0 + step, 48.0 + step));
                        let rect = Rect {
                            x: world_origin.0,
                            y: world_origin.1,
                            w: (size.width as f32 * 0.5).min(800.0) / camera.zoom,
                            h: (size.height as f32 * 0.5).min(600.0) / camera.zoom,
                        };
                        self.session.add_page(browser::spawn(
                            state,
                            &state.window,
                            &bookmark.url,
                            rect,
                        ));
                    }
                }

                if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
                    self.cef_cursor = icon;
                }
                state.window.set_cursor(if self.resize_hover {
                    CursorIcon::NwseResize
                } else {
                    self.cef_cursor
                });

                let camera = self.session.camera();
                let pages = self.session.pages();
                let last_index = pages.len().saturating_sub(1);
                let textures: Vec<_> = pages.iter().map(Page::texture).collect();
                let draws: Vec<PageDraw> = pages
                    .iter()
                    .zip(textures.iter())
                    .enumerate()
                    .map(|(i, (page, texture))| PageDraw {
                        rect: camera.rect_to_screen(page.rect),
                        quad: &page.quad,
                        texture: texture.as_ref(),
                        focused: i == last_index,
                    })
                    .collect();

                let theme = self.session.theme();
                match state.render(&draws, &theme) {
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

                if self.session.dirty() && self.last_save.elapsed() >= SAVE_DEBOUNCE {
                    persistence::save(&self.session);
                    self.session.clear_dirty();
                    self.last_save = Instant::now();
                }

                state.window.request_redraw();
            }
            _ => {}
        }
    }
}
