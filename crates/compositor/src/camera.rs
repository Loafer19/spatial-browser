// World-space <-> screen-space mapping for the 2D canvas. Page rects
// (browser.rs::Page::rect) live in world space and drive CEF's actual
// backing resolution — zooming the camera never re-renders a page at a
// different resolution, it only changes where/how large its quad is
// drawn and hit-tested. `offset` is the world-space point that currently
// maps to screen origin (0,0); pan and zoom-to-cursor both just solve
// for a new offset that keeps some reference point fixed on screen.

use crate::output::Rect;

pub struct Camera {
    pub offset: (f32, f32),
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: (0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    const MIN_ZOOM: f32 = 0.2;
    const MAX_ZOOM: f32 = 3.0;

    pub fn screen_to_world(&self, p: (f32, f32)) -> (f32, f32) {
        (
            p.0 / self.zoom + self.offset.0,
            p.1 / self.zoom + self.offset.1,
        )
    }

    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect {
            x: (r.x - self.offset.0) * self.zoom,
            y: (r.y - self.offset.1) * self.zoom,
            w: r.w * self.zoom,
            h: r.h * self.zoom,
        }
    }

    /// Scales zoom by `factor`, keeping the world point under `pivot`
    /// (screen space, usually the cursor) visually stationary.
    pub fn zoom_at(&mut self, pivot: (f32, f32), factor: f32) {
        let world_pivot = self.screen_to_world(pivot);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        self.offset = (
            world_pivot.0 - pivot.0 / self.zoom,
            world_pivot.1 - pivot.1 / self.zoom,
        );
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
