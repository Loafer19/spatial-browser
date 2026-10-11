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

mod pointer;
mod render;

use pointer::hit_test;

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
        // GPU / OSR are fixed for the process: before any GpuState or browser.
        let startup = settings::init_startup(&settings);
        log::info!("startup options: {startup:?}");
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

impl App {
    /// One frame-loop step, run by main.rs after CEF work and after every
    /// event pump: drain CEF queues, send due begin frames, request a redraw
    /// if anything visible changed. Returns when the loop must wake next
    /// (None = sleep until an event or CEF asks for work).
    pub fn tick(&mut self, now: Instant) -> Option<Instant> {
        let state = self.state.as_mut()?;
        browser::refill_pool(state, now);

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
                self.on_cursor_moved(position);
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                self.on_mouse_input(element_state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.on_mouse_wheel(delta),
            WindowEvent::Touch(touch) => self.on_touch(touch),
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
                self.on_redraw(event_loop);
            }
            _ => {}
        }
    }
}
