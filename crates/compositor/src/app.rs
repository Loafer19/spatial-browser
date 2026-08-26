// Routes winit window events to the right page: hit-tests the cursor
// against the canvas (topmost page first), forwards mouse/keyboard input
// to that page's CEF browser in its own local coordinates, and drives
// per-page dragging (Alt+Left-drag moves a page — same convention as
// borderless-window drag in most Linux window managers, including
// Hyprland's own `bindm ... movewindow`) and resizing (dragging a page's
// bottom-right corner). Page rects live in world space; see viewport.rs
// for the pan/zoom mapping to screen space that hit-testing and drawing
// convert through. Canvas state itself (pages/viewport/theme) lives in
// session.rs, persisted via persistence/mod.rs.

use crate::browser::{self, Page};
use crate::hotkeys;
use crate::input::{
    send_touch_to_host, KeyboardInput, MouseInput, TouchCmd, TouchHit, TouchInput,
};
use crate::viewport::Viewport;
use crate::output::{FrameOutcome, GpuState, PageDraw, Rect, THEMES};
use crate::pending_actions;
use crate::persistence::{
    self,
    bookmarks::{self, Bookmark},
    downloads::{self, DownloadRecord},
    history::{self, HistoryEntry},
    settings::{self, AppSettings},
    typed_history,
    workspaces::{self, Workspace},
};
use crate::session::Session;
use crate::persistence::vault::VaultSession;
use crate::userscripts;
use crate::userstyles;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};

/// Stashed after a login form submit until the following page load.
#[derive(Clone)]
pub struct PendingSaveOffer {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub id: String,
}

/// Open right-click menu (ephemeral page) and the page it applies to.
pub struct ContextMenuState {
    pub menu_browser_id: i32,
    pub target_browser_id: Option<i32>,
    #[allow(dead_code)]
    pub screen_pos: (f32, f32),
}
use cef_bridge::CURSOR;
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
    // Loaded once at startup from typed_history.json, saved immediately
    // on every omnibox submission (see PENDING_OMNIBOX handling below).
    typed_history: Vec<String>,
    // Loaded once at startup from downloads.json, saved immediately on
    // every completed download (see PENDING_DOWNLOADS handling below).
    downloads: Vec<DownloadRecord>,
    // Loaded once at startup from history.json, saved immediately on
    // every completed page visit (see PENDING_VISITS handling below).
    history: Vec<HistoryEntry>,
    // Loaded once at startup from workspaces.json, saved immediately on
    // every save/rename/delete (see PENDING_WORKSPACE_ACTION handling
    // below) — rare, deliberate actions, no need for the canvas
    // session's debounce.
    workspaces: Vec<Workspace>,
    // Loaded once at startup from settings.json, saved immediately on
    // every change (see PENDING_SETTINGS_ACTION handling below).
    // cef-bridge's own live ad-block state (blocklist::ENABLED/
    // CUSTOM_HOSTS) is kept in sync with this at startup and on every
    // change — see `sync_blocklist_settings` below — since that's what
    // `on_before_resource_load` actually reads from, not this struct.
    settings: AppSettings,
    // Loaded at startup from ~/.config/spatial-browser/userscripts/
    // and userstyles/; Ctrl+Shift+U / Reload re-reads both from disk.
    userscripts: Vec<userscripts::UserScript>,
    userstyles: Vec<userstyles::UserStyle>,
    /// Unlocked password vault for this process, if any.
    vault: Option<VaultSession>,
    /// Last generated password shown on the passwords list page.
    generated_password: Option<String>,
    /// Save-offer captured on form submit; re-shown after the next
    /// top-level load for that origin (login navigations wipe in-page UI).
    pending_save_offer: Option<PendingSaveOffer>,
    context_menu: Option<ContextMenuState>,
    /// After right-click on a page, wait for context://hit before opening menu.
    pending_context_hit: Option<(i32, (f32, f32))>,
    mouse: MouseInput,
    touch: TouchInput,
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
    // (screen-space cursor position, viewport offset) at the start of a
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

impl App {
    /// Read by main.rs's own event-loop pacing (the other half of the
    /// frame-rate setting, alongside browser::set_target_frame_rate) —
    /// live, not cached at startup, so turning it up/down in Settings
    /// takes effect on the very next loop iteration.
    pub fn target_fps(&self) -> u32 {
        self.settings.target_fps
    }
}

impl Default for App {
    fn default() -> Self {
        let settings = settings::load();
        pending_actions::sync_blocklist_settings(&settings);
        cef_bridge::set_clean_urls_enabled(settings.clean_urls_enabled);
        browser::set_target_frame_rate(settings.target_fps);
        Self {
            state: None,
            session: Session::new(Vec::new(), Viewport::default(), THEMES[0]),
            bookmarks: bookmarks::load(),
            typed_history: typed_history::load(),
            downloads: downloads::load(),
            history: history::load(),
            workspaces: workspaces::load(),
            settings,
            userscripts: userscripts::load(),
            userstyles: userstyles::load(),
            vault: None,
            generated_password: None,
            pending_save_offer: None,
            context_menu: None,
            pending_context_hit: None,
            mouse: MouseInput::default(),
            touch: TouchInput::default(),
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
fn resize_hit_test(
    pages: &[Page],
    viewport: &Viewport,
    cursor_screen: (f32, f32),
) -> Option<usize> {
    let half = RESIZE_HANDLE / 2.0;
    pages.iter().rposition(|p| {
        let r = viewport.rect_to_screen(p.rect);
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
                "data:text/html;charset=utf-8,<body style=\"background:rgb(42,106,74);color:rgb(255,255,255);font-family:sans-serif;font-size:48px;display:flex;align-items:center;justify-content:center;height:100vh;margin:0\">Page 2</body>",
            ];
            let pages = rects
                .into_iter()
                .zip(urls)
                .map(|(rect, url)| browser::spawn(&state, &window, url, rect, false))
                .collect();
            Session::new(pages, Viewport::default(), THEMES[0])
        });
        self.canvas_size = (
            window.inner_size().width as f32,
            window.inner_size().height as f32,
        );

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
                    let zoom = self.session.viewport().zoom;
                    self.session.pan_viewport_to((
                        offset_at_start.0 - dx / zoom,
                        offset_at_start.1 - dy / zoom,
                    ));
                    return;
                }

                let viewport = self.session.viewport();
                let cursor_world = viewport.screen_to_world(self.cursor_window);
                self.resize_hover = self.resizing
                    || resize_hit_test(self.session.pages(), &viewport, self.cursor_window)
                        .is_some();

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
                            self.panning =
                                Some((self.cursor_window, self.session.viewport().offset));
                        }
                        ElementState::Released => self.panning = None,
                    }
                    return;
                }

                if button == MouseButton::Left && self.modifiers.alt_key() {
                    let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
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
                            // Dismiss context menu on any left click outside it
                            // (clicks on the menu itself are CEF navigations).
                            if self.context_menu.is_some() {
                                let cursor_world = self
                                    .session
                                    .viewport()
                                    .screen_to_world(self.cursor_window);
                                let on_menu = self.context_menu.as_ref().and_then(|cm| {
                                    self.session.pages().iter().find(|p| {
                                        p.browser.identifier() == cm.menu_browser_id
                                            && cursor_world.0 >= p.rect.x
                                            && cursor_world.0 <= p.rect.x + p.rect.w
                                            && cursor_world.1 >= p.rect.y
                                            && cursor_world.1 <= p.rect.y + p.rect.h
                                    })
                                });
                                if on_menu.is_none() {
                                    pending_actions::dismiss_context_menu(
                                        &mut self.session,
                                        &mut self.context_menu,
                                    );
                                }
                            }
                            if let Some(i) = resize_hit_test(
                                self.session.pages(),
                                &self.session.viewport(),
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

                // Right-click → our context menu (not CEF's).
                if button == MouseButton::Right && element_state == ElementState::Pressed {
                    pending_actions::dismiss_context_menu(
                        &mut self.session,
                        &mut self.context_menu,
                    );
                    let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
                    let scale = state.window.scale_factor();
                    match hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                        None => {
                            pending_actions::open_context_menu(
                                &mut self.session,
                                state,
                                &mut self.context_menu,
                                self.cursor_window,
                                crate::pages::context_menu::MenuContext {
                                    on_canvas: true,
                                    ..Default::default()
                                },
                            );
                        }
                        Some(i) => {
                            let i = self.session.bring_to_front(i);
                            let page = &self.session.pages()[i];
                            // Skip reopening menu on the menu page itself.
                            if self.context_menu.as_ref().map(|c| c.menu_browser_id)
                                == Some(page.browser.identifier())
                            {
                                return;
                            }
                            let browser_id = page.browser.identifier();
                            let page_url = page.url();
                            let ephemeral = page.ephemeral;
                            if ephemeral {
                                // Utility pages have no autofill bridge — page-level menu only.
                                pending_actions::open_context_menu(
                                    &mut self.session,
                                    state,
                                    &mut self.context_menu,
                                    self.cursor_window,
                                    crate::pages::context_menu::MenuContext {
                                        on_canvas: false,
                                        page_url: Some(page_url),
                                        target_browser_id: Some(browser_id),
                                        ..Default::default()
                                    },
                                );
                            } else {
                                let local_x = (cursor_world.0 - page.rect.x) / scale as f32;
                                let local_y = (cursor_world.1 - page.rect.y) / scale as f32;
                                self.pending_context_hit = Some((browser_id, self.cursor_window));
                                if let Some(frame) = page.browser.main_frame() {
                                    let js = format!(
                                        "window.__spatialContextHitAt && window.__spatialContextHitAt({local_x},{local_y});"
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
                    return;
                }

                let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
                let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) else {
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
                    self.session.zoom_viewport_at(self.cursor_window, factor);
                    return;
                }

                let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                    let host = self.session.pages()[i].browser.host();
                    self.mouse.wheel(delta, host.as_ref());
                }
            }
            WindowEvent::Touch(touch) => {
                let scale = state.window.scale_factor() as f32;
                let window_pos = (touch.location.x as f32, touch.location.y as f32);
                // Keep cursor_window in sync so pinch-from-trackpad /
                // subsequent mouse wheel zoom still has a sensible pivot
                // after a touch interaction.
                self.cursor_window = window_pos;
                let viewport = self.session.viewport();
                let world = viewport.screen_to_world(window_pos);
                let hit = hit_test(self.session.pages(), world.0, world.1).map(|i| {
                    let page = &self.session.pages()[i];
                    TouchHit {
                        browser_id: page.browser.identifier(),
                        local: (
                            (world.0 - page.rect.x) / scale,
                            (world.1 - page.rect.y) / scale,
                        ),
                    }
                });
                let cmds = self.touch.handle(&touch, hit, viewport.offset, viewport.zoom);
                for cmd in cmds {
                    match cmd {
                        TouchCmd::Send {
                            browser_id,
                            id,
                            local,
                            type_,
                            pressure,
                        } => {
                            if let Some(page) = self
                                .session
                                .pages()
                                .iter()
                                .find(|p| p.browser.identifier() == browser_id)
                            {
                                if let Some(host) = page.browser.host() {
                                    send_touch_to_host(&host, id, local, type_, pressure);
                                }
                            }
                        }
                        TouchCmd::PanViewport { offset } => {
                            self.session.pan_viewport_to(offset);
                        }
                        TouchCmd::SetViewport { offset, zoom } => {
                            self.session.set_viewport(Viewport { offset, zoom });
                        }
                    }
                }
            }
            // macOS/iOS trackpad pinch — no-op on Linux (winit never
            // emits it there); kept so a future macOS build gets canvas
            // zoom without another input path.
            WindowEvent::PinchGesture { delta, .. } => {
                if delta.is_finite() {
                    let factor = (1.0 + delta as f32).clamp(0.5, 2.0);
                    self.session.zoom_viewport_at(self.cursor_window, factor);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
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
                    &self.typed_history,
                    &self.downloads,
                    &self.history,
                    &self.workspaces,
                    &self.settings,
                    &mut self.userscripts,
                    &mut self.userstyles,
                    &mut self.vault,
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

                pending_actions::apply(
                    &mut self.session,
                    state,
                    &mut self.bookmarks,
                    &mut self.typed_history,
                    &mut self.downloads,
                    &mut self.history,
                    &mut self.workspaces,
                    &mut self.settings,
                    &mut self.userscripts,
                    &mut self.userstyles,
                    &mut self.vault,
                    &mut self.generated_password,
                    &mut self.pending_save_offer,
                    &mut self.context_menu,
                    &mut self.pending_context_hit,
                );

                if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
                    self.cef_cursor = icon;
                }
                state.window.set_cursor(if self.resize_hover {
                    CursorIcon::NwseResize
                } else {
                    self.cef_cursor
                });

                let viewport = self.session.viewport();
                let pages = self.session.pages();
                let last_index = pages.len().saturating_sub(1);
                let textures: Vec<_> = pages.iter().map(Page::texture).collect();
                let draws: Vec<PageDraw> = pages
                    .iter()
                    .zip(textures.iter())
                    .enumerate()
                    .map(|(i, (page, texture))| PageDraw {
                        rect: viewport.rect_to_screen(page.rect),
                        quad: &page.quad,
                        texture: texture.as_ref(),
                        focused: i == last_index,
                    })
                    .collect();

                let theme = self.session.theme();
                match state.render(&draws, &theme, viewport.offset, viewport.zoom) {
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
