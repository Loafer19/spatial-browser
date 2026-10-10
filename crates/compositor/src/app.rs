// Route winit events: hit-test → CEF input; Alt+Left-drag moves pages;
// bottom-right corner resizes. Page rects are world-space (see viewport.rs).

use crate::browser::{self, Page};
use crate::frame_pacer::{self, FramePacer, PageFrameInfo, Visibility};
use crate::hotkeys;
use crate::hud::{Hud, HudHit};
use crate::input::{send_touch_to_host, KeyboardInput, MouseInput, TouchCmd, TouchHit, TouchInput};
use crate::minimap::Minimap;
use crate::output::{FrameOutcome, GpuState, PageDraw, Rect, THEMES};
use crate::pending_actions;
use crate::persistence::vault::VaultSession;
use crate::persistence::{
    self,
    bookmarks::{self, Bookmark},
    downloads::{self, DownloadRecord},
    history::{self, HistoryEntry},
    settings::{self, AppSettings},
    typed_history,
    workspaces::{self, WorkspaceRuntime, WorkspaceStore},
};
use crate::session::Session;
use crate::userscripts;
use crate::userstyles;
use crate::viewport::Viewport;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};

/// A login a page offered to save, shown as a compositor-owned prompt tile
/// (`pages::save_prompt`) above that page until Save / Never / Not now, a
/// newer offer, the page closing or the vault locking. Only the tile can
/// accept it. Wiped on drop (it holds a password).
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct PendingSaveOffer {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub id: String,
    /// The page that offered it.
    pub target_id: i32,
    /// The prompt tile's browser.
    pub prompt_id: i32,
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

// Debounce session saves (drags call Session every mouse-move frame).
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);

pub struct App {
    state: Option<GpuState>,
    session: Session,
    // Saved immediately on edit (no session debounce).
    bookmarks: Vec<Bookmark>,
    typed_history: Vec<String>,
    downloads: Vec<DownloadRecord>,
    history: Vec<HistoryEntry>,
    workspaces: WorkspaceStore,
    // Parked browsers for non-active slots (no reload on switch).
    workspace_runtime: WorkspaceRuntime,
    hud: Option<Hud>,
    minimap: Option<Minimap>,
    /// Minimap viewfinder drag: (cursor at press, viewport offset at press).
    minimap_drag: Option<((f32, f32), (f32, f32))>,
    // Synced into cef-bridge blocklist/filter statics on change.
    settings: AppSettings,
    userscripts: Vec<userscripts::UserScript>,
    userstyles: Vec<userstyles::UserStyle>,
    vault: Option<VaultSession>,
    generated_password: Option<String>,
    /// Save-offer; re-shown after next load (login navigations wipe DOM UI).
    pending_save_offer: Option<PendingSaveOffer>,
    context_menu: Option<ContextMenuState>,
    /// Wait for context://hit after right-click before opening menu.
    pending_context_hit: Option<crate::pending_actions::ContextHitRequest>,
    mouse: MouseInput,
    touch: TouchInput,
    keyboard: KeyboardInput,
    // Physical cursor; MouseInput/Wheel lack position of their own.
    cursor_window: (f32, f32),
    modifiers: ModifiersState,
    // Physical window size for layout/minimap (not for mutating world rects).
    canvas_size: (f32, f32),
    // Alt+drag: (browser id of the dragged page, world-space cursor offset
    // from its origin at drag start). Tracked by id, not z-order.
    dragging: Option<(i32, (f32, f32))>,
    // Middle-button pan: (cursor, viewport offset) at press.
    panning: Option<((f32, f32), (f32, f32))>,
    // Bottom-right resize of the page with this browser id; top-left fixed.
    resizing: Option<i32>,
    // Override CEF cursor while over/dragging the resize corner.
    resize_hover: bool,
    // Cached CEF cursor — on_cursor_change only fires on change; re-apply
    // each frame so resize_hover override can be cleared.
    cef_cursor: CursorIcon,
    // Last cursor handed to winit (skip redundant set_cursor calls).
    applied_cursor: Option<CursorIcon>,
    last_save: Instant,
    // Redraw / begin-frame scheduling (idle canvas = no frames).
    pacer: FramePacer,
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
            workspaces: WorkspaceStore::load(),
            workspace_runtime: WorkspaceRuntime::new(),
            hud: None,
            minimap: None,
            minimap_drag: None,
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
            resizing: None,
            resize_hover: false,
            cef_cursor: CursorIcon::default(),
            applied_cursor: None,
            last_save: Instant::now(),
            pacer: FramePacer::new(),
        }
    }
}

/// Screen-space resize grab size at the bottom-right corner.
const RESIZE_HANDLE: f32 = 18.0;
const MIN_PAGE_SIZE: f32 = 160.0;

/// Topmost page whose bottom-right corner is within `RESIZE_HANDLE`.
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

impl App {
    /// One frame-loop step, run by main.rs after CEF work and after every
    /// event pump: drain CEF queues, send due begin frames, request a redraw
    /// if anything visible changed. Returns when the loop must wake next
    /// (None = sleep until an event or CEF asks for work).
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        let state = self.state.as_mut()?;

        pending_actions::apply(
            &mut self.session,
            state,
            pending_actions::Stores {
                bookmarks: &mut self.bookmarks,
                typed_history: &mut self.typed_history,
                downloads: &mut self.downloads,
                history: &mut self.history,
                workspaces: &mut self.workspaces,
                workspace_runtime: &mut self.workspace_runtime,
                settings: &mut self.settings,
                userscripts: &mut self.userscripts,
                userstyles: &mut self.userstyles,
                vault: &mut self.vault,
                generated_password: &mut self.generated_password,
                pending_save_offer: &mut self.pending_save_offer,
                context_menu: &mut self.context_menu,
                pending_context_hit: &mut self.pending_context_hit,
            },
        );

        if let Some(icon) = CURSOR.with_borrow_mut(|cursor| cursor.take()) {
            self.cef_cursor = icon;
        }
        let cursor = if self.resize_hover {
            CursorIcon::NwseResize
        } else {
            self.cef_cursor
        };
        if self.applied_cursor != Some(cursor) {
            state.window.set_cursor(cursor);
            self.applied_cursor = Some(cursor);
        }

        if let Some(hud) = &mut self.hud {
            hud.tick();
        }

        let viewport = self.session.viewport();
        let pages: Vec<PageFrameInfo> = self
            .session
            .pages()
            .iter()
            .map(|page| PageFrameInfo {
                id: page.browser.identifier(),
                visibility: frame_pacer::classify(
                    viewport.rect_to_screen(page.rect),
                    self.canvas_size,
                ),
                paint_token: page.paint_token(),
                busy: page.load_active.get() || page.texture().is_none(),
            })
            .collect();
        let animating =
            self.session.pages().iter().zip(&pages).any(|(page, info)| {
                info.visibility == Visibility::OnScreen && shader_animates(page)
            });
        let signature = self.frame_signature(&pages);
        let plan = self
            .pacer
            .tick(now, self.settings.target_fps, &pages, signature, animating);

        for page in self.session.pages() {
            if plan.begin_frames.contains(&page.browser.identifier()) {
                if let Some(host) = page.browser.host() {
                    host.send_external_begin_frame();
                }
            }
        }
        if plan.redraw {
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        }

        if self.session.dirty() && self.last_save.elapsed() >= SAVE_DEBOUNCE {
            persistence::save(&self.session);
            self.workspaces.capture_active_from_session(&self.session);
            self.workspaces.save();
            self.session.clear_dirty();
            self.last_save = Instant::now();
        }
        let save_due = self.session.dirty().then(|| self.last_save + SAVE_DEBOUNCE);
        let hud_due = self.hud.as_ref().and_then(Hud::hide_deadline);

        frame_pacer::min_instant(
            frame_pacer::min_instant(self.pacer.next_deadline(), save_due),
            hud_due,
        )
    }

    /// Hash of everything the next frame draws; a change means redraw.
    fn frame_signature(&self, pages: &[PageFrameInfo]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let viewport = self.session.viewport();
        for v in [
            viewport.offset.0,
            viewport.offset.1,
            viewport.zoom,
            self.canvas_size.0,
            self.canvas_size.1,
        ] {
            v.to_bits().hash(&mut h);
        }
        self.session.theme().name.hash(&mut h);
        for (page, info) in self.session.pages().iter().zip(pages) {
            info.id.hash(&mut h);
            info.paint_token.hash(&mut h);
            for v in [page.rect.x, page.rect.y, page.rect.w, page.rect.h] {
                v.to_bits().hash(&mut h);
            }
            page.load_progress.get().to_bits().hash(&mut h);
            (page.load_active.get() && !page.ephemeral).hash(&mut h);
        }
        self.hud.as_ref().map(Hud::visible).hash(&mut h);
        self.workspaces.active.hash(&mut h);
        self.workspaces.slot_count().hash(&mut h);
        h.finish()
    }
}

/// Page quad uses the frame clock (shader.wgsl): loading placeholder pulse
/// before the first paint, sliding load-bar chunk while progress is unknown.
fn shader_animates(page: &Page) -> bool {
    let progress = page.load_progress.get();
    let load_bar = page.load_active.get() && !page.ephemeral && progress < 0.999;
    page.texture().is_none() || (load_bar && progress < 0.02)
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
        browser::install_popup_factory(&state);

        let mut hud = Hud::new(&state.device, &state.queue, state.surface_format());
        let mut minimap = Minimap::new(&state.device, &state.queue, state.surface_format());
        let (cw, ch) = state.size();
        hud.set_screen_size(cw, ch);
        minimap.set_screen_size(cw, ch);

        // Prefer the active live slot when it has been visited; otherwise
        // fall back to the legacy session file / first-run defaults, then
        // capture into the active slot so slots become the source of truth.
        // Restored pages start loading below: give the EasyList build queued
        // in App::default a moment so their first load is filtered.
        if !cef_bridge::wait_filter_engine(std::time::Duration::from_millis(1500)) {
            log::warn!("filter engine still building; restored pages load unfiltered");
        }
        if self.workspaces.active_slot().visited {
            let slot = self.workspaces.active_slot().clone();
            self.session = Session::new(Vec::new(), Viewport::default(), THEMES[0]);
            workspaces::apply_slot_to_session(&mut self.session, &state, &slot);
        } else {
            self.session = persistence::load(&state, &window).unwrap_or_else(|| {
                // Single starter page (not the old side-by-side Google + demo).
                let size = window.inner_size();
                let margin = 48.0;
                let rect = Rect {
                    x: margin,
                    y: margin,
                    w: (size.width as f32 - margin * 2.0).max(320.0),
                    h: (size.height as f32 - margin * 2.0).max(240.0),
                };
                let page = browser::spawn(&state, &window, "https://www.google.com", rect, false);
                Session::new(vec![page], Viewport::default(), THEMES[0])
            });
            self.workspaces.capture_active_from_session(&self.session);
            self.workspaces.save();
        }
        self.canvas_size = (
            window.inner_size().width as f32,
            window.inner_size().height as f32,
        );

        self.hud = Some(hud);
        self.minimap = Some(minimap);
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
        // Anything but our own redraw is user/WM activity: repaint and run
        // visible pages at full rate for a moment (frame_pacer).
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.pacer.note_input(Instant::now());
        }

        match event {
            WindowEvent::CloseRequested => {
                self.workspaces.capture_active_from_session(&self.session);
                self.workspaces.save();
                persistence::save(&self.session);
                persistence::store::flush();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);

                // Page rects live in world space and must NOT be multiplied by
                // the window size ratio. Doing so on every Resized (common at
                // startup on tiling WMs, and whenever the window is resized)
                // drifted saved layouts: after restart pages jumped and pairs
                // spread apart. The viewport maps the same world onto the new
                // screen; use auto-layout if you want pages to reflow.
                let (new_w, new_h) = (size.width as f32, size.height as f32);
                self.canvas_size = (new_w, new_h);
                if let Some(hud) = &mut self.hud {
                    hud.set_screen_size(new_w, new_h);
                }
                if let Some(minimap) = &mut self.minimap {
                    minimap.set_screen_size(new_w, new_h);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor();
                self.cursor_window = (position.x as f32, position.y as f32);
                if let Some(hud) = &mut self.hud {
                    hud.on_cursor(self.cursor_window.1);
                }

                if let Some((start, offset_at_start)) = self.minimap_drag {
                    if let Some(minimap) = &self.minimap {
                        if let Some(map_scale) = minimap.scale() {
                            let dx = self.cursor_window.0 - start.0;
                            let dy = self.cursor_window.1 - start.1;
                            // Dragging the viewfinder right → look further right.
                            self.session.pan_viewport_to((
                                offset_at_start.0 + dx / map_scale,
                                offset_at_start.1 + dy / map_scale,
                            ));
                        }
                    }
                    return;
                }

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
                self.resize_hover = self.resizing.is_some()
                    || resize_hit_test(self.session.pages(), &viewport, self.cursor_window)
                        .is_some();

                if let Some(browser_id) = self.resizing {
                    // Page closed mid-resize → end the gesture.
                    let Some(page_rect) = self.session.rect_of(browser_id) else {
                        self.resizing = None;
                        return;
                    };
                    let rect = Rect {
                        x: page_rect.x,
                        y: page_rect.y,
                        w: (cursor_world.0 - page_rect.x).max(MIN_PAGE_SIZE),
                        h: (cursor_world.1 - page_rect.y).max(MIN_PAGE_SIZE),
                    };
                    self.session.set_rect_of(browser_id, rect, scale);
                    return;
                }

                if let Some((browser_id, offset)) = self.dragging {
                    // Page closed mid-drag → end the gesture.
                    let Some(page_rect) = self.session.rect_of(browser_id) else {
                        self.dragging = None;
                        return;
                    };
                    let rect = Rect {
                        x: cursor_world.0 - offset.0,
                        y: cursor_world.1 - offset.1,
                        w: page_rect.w,
                        h: page_rect.h,
                    };
                    self.session.set_rect_of(browser_id, rect, scale);
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
                                let page = &self.session.pages()[top];
                                let rect = page.rect;
                                self.dragging = Some((
                                    page.browser.identifier(),
                                    (cursor_world.0 - rect.x, cursor_world.1 - rect.y),
                                ));
                            }
                        }
                        ElementState::Released => self.dragging = None,
                    }
                    return;
                }

                if button == MouseButton::Left {
                    match element_state {
                        ElementState::Pressed => {
                            if let Some(hud) = &self.hud {
                                if let Some(hit) = hud.hit_test(
                                    &self.workspaces,
                                    self.cursor_window.0,
                                    self.cursor_window.1,
                                ) {
                                    match hit {
                                        HudHit::Slot(id) => {
                                            workspaces::switch_to(
                                                &mut self.workspaces,
                                                &mut self.workspace_runtime,
                                                &mut self.session,
                                                state,
                                                id,
                                            );
                                        }
                                        HudHit::Add => {
                                            workspaces::add_and_switch(
                                                &mut self.workspaces,
                                                &mut self.workspace_runtime,
                                                &mut self.session,
                                                state,
                                            );
                                        }
                                    }
                                    return;
                                }
                            }
                            if let Some(minimap) = &self.minimap {
                                if minimap.visible()
                                    && minimap
                                        .contains_screen(self.cursor_window.0, self.cursor_window.1)
                                {
                                    if minimap
                                        .hit_viewfinder(self.cursor_window.0, self.cursor_window.1)
                                    {
                                        self.minimap_drag = Some((
                                            self.cursor_window,
                                            self.session.viewport().offset,
                                        ));
                                    } else if let Some(world) = minimap
                                        .screen_to_world(self.cursor_window.0, self.cursor_window.1)
                                    {
                                        let (sw, sh) = self.canvas_size;
                                        let zoom = self.session.viewport().zoom;
                                        self.session.pan_viewport_to((
                                            world.0 - sw / zoom * 0.5,
                                            world.1 - sh / zoom * 0.5,
                                        ));
                                    }
                                    return;
                                }
                            }
                            // Dismiss context menu on any left click outside it
                            // (clicks on the menu itself are CEF navigations).
                            if self.context_menu.is_some() {
                                let cursor_world =
                                    self.session.viewport().screen_to_world(self.cursor_window);
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
                                let top = self.session.bring_to_front(i);
                                self.resizing =
                                    Some(self.session.pages()[top].browser.identifier());
                                return;
                            }
                        }
                        ElementState::Released => {
                            if self.minimap_drag.take().is_some() {
                                return;
                            }
                            if self.resizing.take().is_some() {
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
                                self.pending_context_hit =
                                    Some(crate::pending_actions::ContextHitRequest {
                                        browser_id,
                                        screen_pos: self.cursor_window,
                                        at: std::time::Instant::now(),
                                    });
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
                let cmds = self
                    .touch
                    .handle(&touch, hit, viewport.offset, viewport.zoom);
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
                    pending_actions::Stores {
                        bookmarks: &mut self.bookmarks,
                        typed_history: &mut self.typed_history,
                        downloads: &mut self.downloads,
                        history: &mut self.history,
                        workspaces: &mut self.workspaces,
                        workspace_runtime: &mut self.workspace_runtime,
                        settings: &mut self.settings,
                        userscripts: &mut self.userscripts,
                        userstyles: &mut self.userstyles,
                        vault: &mut self.vault,
                        generated_password: &mut self.generated_password,
                        pending_save_offer: &mut self.pending_save_offer,
                        context_menu: &mut self.context_menu,
                        pending_context_hit: &mut self.pending_context_hit,
                    },
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
                // Begin frames, pending CEF actions, cursor and session
                // saves run in `tick` (every loop iteration), not per frame.
                let viewport = self.session.viewport();
                let pages = self.session.pages();
                let last_index = pages.len().saturating_sub(1);
                let textures: Vec<_> = pages.iter().map(Page::texture).collect();
                let draws: Vec<PageDraw> = pages
                    .iter()
                    .zip(textures.iter())
                    .enumerate()
                    .map(|(i, (page, texture))| {
                        let progress = page.load_progress.get();
                        let active = page.load_active.get() && !page.ephemeral;
                        PageDraw {
                            rect: viewport.rect_to_screen(page.rect),
                            quad: &page.quad,
                            texture: texture.as_ref(),
                            focused: i == last_index,
                            load_progress: progress,
                            load_bar: active && progress < 0.999,
                        }
                    })
                    .collect();

                let theme = self.session.theme();
                if let Some(hud) = &mut self.hud {
                    hud.rebuild(&state.queue, &state.device, &self.workspaces, &theme);
                }
                let focused = self.session.pages().len().checked_sub(1);
                if let Some(minimap) = &mut self.minimap {
                    minimap.rebuild(
                        &state.queue,
                        &state.device,
                        self.session.pages(),
                        &viewport,
                        &theme,
                        focused,
                    );
                }
                let hud_ref = self.hud.as_ref();
                let minimap_ref = self.minimap.as_ref();
                match state.render(
                    &draws,
                    &theme,
                    viewport.offset,
                    viewport.zoom,
                    hud_ref,
                    minimap_ref,
                ) {
                    FrameOutcome::Rendered => self.pacer.rendered(Instant::now()),
                    FrameOutcome::Skip => self.pacer.render_skipped(Instant::now()),
                    FrameOutcome::Reconfigure => {
                        let size = state.window.inner_size();
                        state.resize(size.width, size.height);
                        self.pacer.render_skipped(Instant::now());
                    }
                    FrameOutcome::Fatal => {
                        log::error!("fatal surface validation error, exiting");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}
