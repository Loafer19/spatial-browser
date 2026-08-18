// Owns the canvas state that gets persisted across restarts — pages
// (z-order = vec order), viewport, theme — behind methods that each mark
// the session dirty as part of doing the mutation. Persistence
// (persistence/mod.rs) only has to poll `dirty()`/`clear_dirty()`;
// nothing has to remember to flag a save at each call site, so any
// future feature (page groups, etc.) that mutates state through
// Session's own methods gets persisted for free.

use crate::browser::Page;
use crate::output::{Rect, THEMES, Theme};
use crate::viewport::Viewport;
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};

// How many recently-closed pages Ctrl+Shift+T can reach back through.
const MAX_CLOSED: usize = 20;

pub struct Session {
    pages: Vec<Page>,
    viewport: Viewport,
    theme: Theme,
    dirty: bool,
    // Viewport stashed while a page is zoomed-to-canvas (see
    // toggle_zoom_focused), restored when toggled back off. Only one page
    // can be zoomed at a time in practice (Page::zoomed_from parallels
    // this), so a single slot is enough.
    zoomed_viewport: Option<Viewport>,
    // Rect + URL of recently closed pages, most-recent last, for
    // Ctrl+Shift+T (see pop_closed). Not persisted — "undo close" only
    // makes sense within the current run.
    closed: Vec<(Rect, String)>,
}

impl Session {
    pub fn new(pages: Vec<Page>, viewport: Viewport, theme: Theme) -> Self {
        Self {
            pages,
            viewport,
            theme,
            dirty: false,
            zoomed_viewport: None,
            closed: Vec::new(),
        }
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// True if anything below changed since the last `clear_dirty`.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn add_page(&mut self, page: Page) {
        self.pages.push(page);
        self.mark_dirty();
    }

    /// Pops the topmost page and tells CEF to close it, if any. Its rect
    /// and current URL are kept for `pop_closed` (Ctrl+Shift+T) first.
    pub fn close_topmost(&mut self) {
        let Some(page) = self.pages.pop() else {
            return;
        };
        if let Some(url) = page
            .browser
            .main_frame()
            .map(|frame| cef::CefString::from(&frame.url()).to_string())
        {
            self.closed.push((page.rect, url));
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if let Some(host) = page.browser.host() {
            host.close_browser(true as _);
        }
        self.mark_dirty();
    }

    /// Pops the most recently closed page's rect+URL, if any — the
    /// caller (hotkeys::reopen_closed) spawns a real page for it, since
    /// Session itself doesn't own GpuState/CEF spawning.
    pub fn pop_closed(&mut self) -> Option<(Rect, String)> {
        self.closed.pop()
    }

    /// Closes and removes the page at `index`, returning its former
    /// rect — used to refresh a page whose content needs to be rebuilt
    /// from fresh Rust-side data (the bookmarks-list page after a
    /// delete/rename) by closing it and spawning a replacement at the
    /// same rect, rather than reloading in place: a `load_url` issued
    /// right after CEF just canceled a navigation on that same frame
    /// isn't reliable. Does *not* go through the closed-page undo stack
    /// (pop_closed) — this isn't a user-initiated close.
    pub fn close_at(&mut self, index: usize) -> Option<Rect> {
        if index >= self.pages.len() {
            return None;
        }
        let page = self.pages.remove(index);
        let rect = page.rect;
        if let Some(host) = page.browser.host() {
            host.close_browser(true as _);
        }
        self.mark_dirty();
        Some(rect)
    }

    /// Moves the page at `index` to the end of z-order (topmost) and
    /// returns its new index.
    pub fn bring_to_front(&mut self, index: usize) -> usize {
        let page = self.pages.remove(index);
        self.pages.push(page);
        self.mark_dirty();
        self.pages.len() - 1
    }

    /// Rotates z-order so the next (or, if `backward`, previous) page
    /// becomes topmost/focused.
    pub fn rotate_focus(&mut self, backward: bool) {
        if self.pages.is_empty() {
            return;
        }
        if backward {
            self.pages.rotate_right(1);
        } else {
            self.pages.rotate_left(1);
        }
        self.mark_dirty();
    }

    /// Used by drag/resize, both of which always act on the topmost page
    /// (bring_to_front happens at gesture start).
    pub fn set_topmost_rect(&mut self, rect: Rect, scale_factor: f64) {
        if let Some(page) = self.pages.last_mut() {
            page.set_rect(rect, scale_factor);
            self.mark_dirty();
        }
    }

    /// Proportionally rescales every page's rect — used to correct page
    /// layout when the window's real size arrives late (tiling WMs often
    /// settle into final geometry via a `Resized` shortly after creation).
    pub fn rescale_pages(&mut self, scale_x: f32, scale_y: f32, dpi_scale: f64) {
        for page in &mut self.pages {
            let rect = Rect {
                x: page.rect.x * scale_x,
                y: page.rect.y * scale_y,
                w: page.rect.w * scale_x,
                h: page.rect.h * scale_y,
            };
            page.set_rect(rect, dpi_scale);
        }
        self.mark_dirty();
    }

    pub fn pan_viewport_to(&mut self, offset: (f32, f32)) {
        self.viewport.offset = offset;
        self.mark_dirty();
    }

    pub fn zoom_viewport_at(&mut self, pivot: (f32, f32), factor: f32) {
        self.viewport.zoom_at(pivot, factor);
        self.mark_dirty();
    }

    pub fn reset_viewport(&mut self) {
        self.viewport.reset();
        self.mark_dirty();
    }

    /// Arranges every open page into a grid filling `screen_size`
    /// (physical pixels) and resets the viewport to identity — so the
    /// grid is computed directly in screen space rather than juggling a
    /// pan/zoom that would just fight the new layout. Also drops any
    /// zoomed-to-canvas state (both the per-page `zoomed_from` and the
    /// session's stashed `zoomed_viewport`): laying pages out into a
    /// fresh grid makes "restore to the rect before zooming" moot.
    pub fn auto_layout(&mut self, screen_size: (f32, f32), scale_factor: f64) {
        let n = self.pages.len();
        if n == 0 {
            return;
        }
        self.viewport.reset();
        self.zoomed_viewport = None;

        let cols = (n as f32).sqrt().ceil() as usize;
        let rows = (n + cols - 1) / cols;
        let margin = 24.0;
        let gap = 24.0;
        let cell_w = (screen_size.0 - margin * 2.0 - gap * (cols - 1) as f32) / cols as f32;
        let cell_h = (screen_size.1 - margin * 2.0 - gap * (rows - 1) as f32) / rows as f32;

        for (i, page) in self.pages.iter_mut().enumerate() {
            page.zoomed_from = None;
            let col = i % cols;
            let row = i / cols;
            let rect = Rect {
                x: margin + col as f32 * (cell_w + gap),
                y: margin + row as f32 * (cell_h + gap),
                w: cell_w,
                h: cell_h,
            };
            page.set_rect(rect, scale_factor);
        }
        self.mark_dirty();
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.mark_dirty();
    }

    pub fn cycle_theme(&mut self) {
        let current = THEMES
            .iter()
            .position(|t| t.name == self.theme.name)
            .unwrap_or(0);
        self.set_theme(THEMES[(current + 1) % THEMES.len()]);
    }

    /// Toggles the topmost page between its normal rect and filling
    /// `screen_size` (physical pixels) — a pure numeric size rather than a
    /// whole `GpuState` so this module stays free of window/wgpu
    /// dependencies.
    ///
    /// Also resets the viewport to identity for the duration (restored on
    /// toggle-back-off): computing the fill size as `screen_size /
    /// viewport.zoom` instead would blow up at a small zoom (zoomed way
    /// out) into a world size of tens of thousands of pixels — since a
    /// page's rect drives CEF's actual OSR buffer resolution 1:1, that
    /// either gets clamped (page ends up *not* filling the screen) or,
    /// before that clamp existed, crashed the whole process outright.
    pub fn toggle_zoom_focused(&mut self, screen_size: (f32, f32), scale_factor: f64) {
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        match page.zoomed_from.take() {
            Some(previous_rect) => {
                page.set_rect(previous_rect, scale_factor);
                if let Some(viewport) = self.zoomed_viewport.take() {
                    self.viewport = viewport;
                }
            }
            None => {
                self.zoomed_viewport = Some(self.viewport);
                self.viewport = Viewport::default();
                let margin = 40.0;
                let zoomed_rect = Rect {
                    x: margin,
                    y: margin,
                    w: screen_size.0 - margin * 2.0,
                    h: screen_size.1 - margin * 2.0,
                };
                page.zoomed_from = Some(page.rect);
                page.set_rect(zoomed_rect, scale_factor);
            }
        }
        self.mark_dirty();
    }
}
