// World ↔ screen mapping. Page rects stay world-sized (CEF resolution);
// viewport zoom only changes draw/hit-test size. `offset` = world point at screen (0,0).

use crate::output::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub offset: (f32, f32),
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: (0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Viewport {
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
