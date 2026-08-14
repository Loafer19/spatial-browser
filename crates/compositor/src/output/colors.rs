// Named colors for the compositor's own drawing (canvas background, and
// eventually chrome UI — selection highlight, focus ring, etc.). Page
// content itself has no colors here: it's whatever CEF rendered, drawn
// as a textured quad.

pub const CANVAS_BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.05,
    b: 0.08,
    a: 1.0,
};

// Page chrome: every page gets rounded corners; the topmost (focused)
// one additionally gets an accent-colored ring so it's clear which page
// keyboard input and Alt+drag currently target.
pub const PAGE_CORNER_RADIUS: f32 = 12.0;
pub const FOCUS_BORDER_WIDTH: f32 = 3.0;
pub const FOCUS_BORDER_COLOR: [f32; 4] = [0.35, 0.62, 1.0, 1.0];
