// Owns the canvas state that gets persisted across restarts — pages
// (z-order = vec order), camera, theme — behind methods that each mark
// the session dirty as part of doing the mutation. Persistence
// (persistence.rs) only has to poll `dirty()`/`clear_dirty()`; nothing
// has to remember to flag a save at each call site, so any future
// feature (page groups, etc.) that mutates state through Session's own
// methods gets persisted for free.

use crate::browser::Page;
use crate::camera::Camera;
use crate::output::{Rect, THEMES, Theme};
use cef::{ImplBrowser, ImplFrame};

// How many recently-closed pages Ctrl+Shift+T can reach back through.
const MAX_CLOSED: usize = 20;

pub struct Session {
    pages: Vec<Page>,
    camera: Camera,
    theme: Theme,
    dirty: bool,
    // Camera stashed while a page is zoomed-to-canvas (see
    // toggle_zoom_focused), restored when toggled back off. Only one page
    // can be zoomed at a time in practice (Page::zoomed_from parallels
    // this), so a single slot is enough.
    zoomed_camera: Option<Camera>,
    // Rect + URL of recently closed pages, most-recent last, for
    // Ctrl+Shift+T (see pop_closed). Not persisted — "undo close" only
    // makes sense within the current run.
    closed: Vec<(Rect, String)>,
}

impl Session {
    pub fn new(pages: Vec<Page>, camera: Camera, theme: Theme) -> Self {
        Self {
            pages,
            camera,
            theme,
            dirty: false,
            zoomed_camera: None,
            closed: Vec::new(),
        }
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn camera(&self) -> Camera {
        self.camera
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
            cef::ImplBrowserHost::close_browser(&host, true as _);
        }
        self.mark_dirty();
    }

    /// Pops the most recently closed page's rect+URL, if any — the
    /// caller (hotkeys::reopen_closed) spawns a real page for it, since
    /// Session itself doesn't own GpuState/CEF spawning.
    pub fn pop_closed(&mut self) -> Option<(Rect, String)> {
        self.closed.pop()
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

    pub fn pan_camera_to(&mut self, offset: (f32, f32)) {
        self.camera.offset = offset;
        self.mark_dirty();
    }

    pub fn zoom_camera_at(&mut self, pivot: (f32, f32), factor: f32) {
        self.camera.zoom_at(pivot, factor);
        self.mark_dirty();
    }

    pub fn reset_camera(&mut self) {
        self.camera.reset();
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
    /// `viewport` (physical pixels) — a pure numeric viewport/scale in
    /// rather than a whole `GpuState` so this module stays free of
    /// window/wgpu dependencies.
    ///
    /// Also resets the camera to identity for the duration (restored on
    /// toggle-back-off): computing the fill size as `viewport /
    /// camera.zoom` instead would blow up at a small zoom (zoomed way
    /// out) into a world size of tens of thousands of pixels — since a
    /// page's rect drives CEF's actual OSR buffer resolution 1:1, that
    /// either gets clamped (page ends up *not* filling the screen) or,
    /// before that clamp existed, crashed the whole process outright.
    pub fn toggle_zoom_focused(&mut self, viewport: (f32, f32), scale_factor: f64) {
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        match page.zoomed_from.take() {
            Some(previous_rect) => {
                page.set_rect(previous_rect, scale_factor);
                if let Some(camera) = self.zoomed_camera.take() {
                    self.camera = camera;
                }
            }
            None => {
                self.zoomed_camera = Some(self.camera);
                self.camera = Camera::default();
                let margin = 40.0;
                let zoomed_rect = Rect {
                    x: margin,
                    y: margin,
                    w: viewport.0 - margin * 2.0,
                    h: viewport.1 - margin * 2.0,
                };
                page.zoomed_from = Some(page.rect);
                page.set_rect(zoomed_rect, scale_factor);
            }
        }
        self.mark_dirty();
    }
}
