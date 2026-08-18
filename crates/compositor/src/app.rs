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
use crate::input::{KeyboardInput, MouseInput};
use crate::output::{FrameOutcome, GpuState, PageDraw, Rect, THEMES};
use crate::pages;
use crate::persistence::{
    self,
    bookmarks::{self, Bookmark},
    downloads::{self, DownloadRecord},
    typed_history,
};
use crate::session::Session;
use crate::viewport::Viewport;
use cef::{ImplBrowser, ImplBrowserHost};
use cef_bridge::{
    BookmarkAction, CURSOR, DownloadPageAction, PENDING_BOOKMARK, PENDING_DOWNLOAD_ACTION,
    PENDING_DOWNLOADS, PENDING_OMNIBOX, PENDING_POPUPS, PENDING_SWITCH,
};
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

impl Default for App {
    fn default() -> Self {
        Self {
            state: None,
            session: Session::new(Vec::new(), Viewport::default(), THEMES[0]),
            bookmarks: bookmarks::load(),
            typed_history: typed_history::load(),
            downloads: downloads::load(),
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
fn resize_hit_test(pages: &[Page], viewport: &Viewport, cursor_screen: (f32, f32)) -> Option<usize> {
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

/// Replaces the bookmarks-list page identified by `browser_id` (CEF's
/// own per-browser id) with a fresh one at the same rect, if it's still
/// open — used after a delete/rename so the list reflects the change
/// without the user having to close and reopen it themselves. Closes and
/// respawns rather than `load_url` in place: a navigation issued right
/// after CEF just canceled one on that same frame isn't reliable. A free
/// function, not an App method: `self.state` is already mutably borrowed
/// as `state` for most of `window_event`, so this only takes the
/// specific fields it needs.
fn refresh_bookmarks_page(session: &mut Session, gpu: &GpuState, bookmarks: &[Bookmark], browser_id: i32) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
}

/// Same as `refresh_bookmarks_page`, for the downloads-list page after
/// a remove.
fn refresh_downloads_page(
    session: &mut Session,
    gpu: &GpuState,
    downloads: &[DownloadRecord],
    browser_id: i32,
) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let Some(rect) = session.close_at(index) else {
        return;
    };
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
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
                .map(|(rect, url)| browser::spawn(&state, &window, url, rect, false))
                .collect();
            Session::new(pages, Viewport::default(), THEMES[0])
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
                    || resize_hit_test(self.session.pages(), &viewport, self.cursor_window).is_some();

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
                            self.panning = Some((self.cursor_window, self.session.viewport().offset));
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

                let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
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
                    self.session.zoom_viewport_at(self.cursor_window, factor);
                    return;
                }

                let cursor_world = self.session.viewport().screen_to_world(self.cursor_window);
                if let Some(i) = hit_test(self.session.pages(), cursor_world.0, cursor_world.1) {
                    let host = self.session.pages()[i].browser.host();
                    self.mouse.wheel(delta, host.as_ref());
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

                // Set by cef-bridge's OsrRequestHandler when a click or
                // form submit inside the bookmarks-list page
                // (hotkeys::open_bookmarks) hits one of its
                // `bookmark://...` links — that navigation was already
                // canceled there; act on it here instead. `browser_id`
                // identifies exactly which bookmarks-list page asked, so
                // delete/rename can reload that same page in place
                // rather than guessing which open page (if any) it was.
                if let Some((browser_id, action)) =
                    PENDING_BOOKMARK.with_borrow_mut(|pending| pending.take())
                {
                    match action {
                        BookmarkAction::Open(index) => {
                            if let Some(bookmark) = self.bookmarks.get(index) {
                                let size = state.window.inner_size();
                                let rect = self
                                    .session
                                    .cascade_rect((size.width as f32, size.height as f32));
                                self.session.add_page(browser::spawn(
                                    state,
                                    &state.window,
                                    &bookmark.url,
                                    rect,
                                    false,
                                ));
                            }
                        }
                        BookmarkAction::Delete(index) => {
                            if index < self.bookmarks.len() {
                                self.bookmarks.remove(index);
                                bookmarks::save(&self.bookmarks);
                            }
                            refresh_bookmarks_page(&mut self.session, state, &self.bookmarks, browser_id);
                        }
                        BookmarkAction::Rename(index, title, folder) => {
                            if let Some(bookmark) = self.bookmarks.get_mut(index) {
                                bookmark.title = (!title.is_empty()).then_some(title);
                                bookmark.folder = (!folder.is_empty()).then_some(folder);
                            }
                            bookmarks::save(&self.bookmarks);
                            refresh_bookmarks_page(&mut self.session, state, &self.bookmarks, browser_id);
                        }
                    }
                }

                // Set by cef-bridge's OsrRequestHandler when the omnibox
                // page (hotkeys::open_new / omnibox_page_url) submits an
                // `omnibox://go?q=...&url=...` — that navigation was
                // already canceled there. Log the raw typed text, then
                // replace the omnibox page with a real one at the
                // resolved destination (close+respawn rather than
                // `load_url` in place, same reliability reason as
                // refresh_bookmarks_page).
                if let Some((browser_id, submit)) =
                    PENDING_OMNIBOX.with_borrow_mut(|pending| pending.take())
                {
                    typed_history::record(&mut self.typed_history, &submit.raw);
                    if let Some(index) = self
                        .session
                        .pages()
                        .iter()
                        .position(|p| p.browser.identifier() == browser_id)
                    {
                        if let Some(rect) = self.session.close_at(index) {
                            self.session.add_page(browser::spawn(
                                state,
                                &state.window,
                                &submit.url,
                                rect,
                                false,
                            ));
                        }
                    }
                }

                // Set by cef-bridge's OsrRequestHandler when a row click
                // or Enter inside the switcher page (hotkeys::
                // open_switcher) hits a `switcher://go/{id}` link — that
                // navigation was already canceled there. Bring the
                // target page to front and pan it to screen center (kept
                // at the current zoom level), then close the switcher
                // page — a permanent close, not through the closed-page
                // undo stack (pop_closed): this isn't a user-initiated
                // close of *their* content.
                if let Some((switcher_id, target_id)) =
                    PENDING_SWITCH.with_borrow_mut(|pending| pending.take())
                {
                    if let Some(target_index) = self
                        .session
                        .pages()
                        .iter()
                        .position(|p| p.browser.identifier() == target_id)
                    {
                        self.session.bring_to_front(target_index);
                        let rect = self
                            .session
                            .pages()
                            .last()
                            .expect("just brought a page to front")
                            .rect;
                        let viewport = self.session.viewport();
                        let size = state.window.inner_size();
                        let screen_center = (size.width as f32 / 2.0, size.height as f32 / 2.0);
                        let world_center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
                        self.session.pan_viewport_to((
                            world_center.0 - screen_center.0 / viewport.zoom,
                            world_center.1 - screen_center.1 / viewport.zoom,
                        ));
                    }
                    if let Some(switcher_index) = self
                        .session
                        .pages()
                        .iter()
                        .position(|p| p.browser.identifier() == switcher_id)
                    {
                        self.session.close_at(switcher_index);
                    }
                }

                // Appended by cef-bridge's OsrDownloadHandler the first
                // time a download reports complete. Record each into
                // downloads.json and fire a desktop notification —
                // there's no in-canvas download UI (progress bar,
                // toast): a system notification is visible regardless of
                // window focus/workspace, and needs no native GPU text
                // rendering the way an in-canvas toast would.
                let completed = PENDING_DOWNLOADS.with_borrow_mut(std::mem::take);
                for download in completed {
                    let filename = std::path::Path::new(&download.path)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&download.path)
                        .to_string();
                    downloads::record(
                        &mut self.downloads,
                        DownloadRecord {
                            url: download.url,
                            path: download.path,
                        },
                    );
                    if let Err(e) = std::process::Command::new("notify-send")
                        .arg("Download complete")
                        .arg(&filename)
                        .spawn()
                    {
                        log::warn!("notify-send failed (download {filename:?} still saved): {e}");
                    }
                }

                // Set by cef-bridge's OsrRequestHandler when a click
                // inside the downloads-list page (hotkeys::
                // open_downloads) hits a `download://...` link — that
                // navigation was already canceled there.
                if let Some((browser_id, action)) =
                    PENDING_DOWNLOAD_ACTION.with_borrow_mut(|pending| pending.take())
                {
                    match action {
                        DownloadPageAction::Open(index) => {
                            if let Some(download) = self.downloads.get(index) {
                                if let Err(e) =
                                    std::process::Command::new("xdg-open").arg(&download.path).spawn()
                                {
                                    log::warn!("xdg-open failed for {:?}: {e}", download.path);
                                }
                            }
                        }
                        DownloadPageAction::Remove(index) => {
                            if index < self.downloads.len() {
                                self.downloads.remove(index);
                                downloads::save(&self.downloads);
                            }
                            refresh_downloads_page(&mut self.session, state, &self.downloads, browser_id);
                        }
                    }
                }

                // Appended by cef-bridge's OsrLifeSpanHandler when a page
                // tries to open a link in a new tab/window
                // (target="_blank", window.open, middle-click) — canceled
                // there so CEF doesn't spawn its own native popup window
                // outside the canvas. Spawn a regular Page instead,
                // cascaded from the opener's rect if it's still open.
                let popups = PENDING_POPUPS.with_borrow_mut(std::mem::take);
                for (opener_id, url) in popups {
                    let opener_rect = self
                        .session
                        .pages()
                        .iter()
                        .find(|p| p.browser.identifier() == opener_id)
                        .map(|p| p.rect);
                    let size = state.window.inner_size();
                    let rect = match opener_rect {
                        Some(r) => Rect {
                            x: r.x + 40.0,
                            y: r.y + 40.0,
                            w: r.w,
                            h: r.h,
                        },
                        None => self
                            .session
                            .cascade_rect((size.width as f32, size.height as f32)),
                    };
                    self.session
                        .add_page(browser::spawn(state, &state.window, &url, rect, false));
                }

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
