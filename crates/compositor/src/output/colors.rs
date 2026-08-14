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
