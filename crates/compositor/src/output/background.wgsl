// World-space dot grid (pans/zooms with pages); fullscreen triangle under quads.

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // Oversized triangle covering the whole clip-space square; only the
    // part inside [-1,1] ever rasterizes.
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

struct BackgroundGlobals {
    // xy = viewport offset (world space), z = zoom, w unused — same
    // mapping as Viewport::screen_to_world (viewport.rs), reimplemented
    // here since that's plain Rust, not reachable from a shader.
    viewport: vec4<f32>,
    dot_color: vec4<f32>,
    // x = grid spacing, y = dot radius, both in world-space units, so
    // both scale together with zoom exactly like a grid drawn on the
    // canvas itself would.
    grid: vec4<f32>,
};
@group(0) @binding(0) var<uniform> bg: BackgroundGlobals;

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let zoom = bg.viewport.z;
    let world = frag.xy / zoom + bg.viewport.xy;
    let spacing = bg.grid.x;
    let radius = bg.grid.y;

    let cell = (fract(world / spacing) - 0.5) * spacing;
    let dist = length(cell);
    // Antialiasing band in world units so the dot's screen-space edge
    // sharpness doesn't change with zoom.
    let aa = 0.5 / zoom;
    let alpha = 1.0 - smoothstep(radius - aa, radius + aa, dist);
    return vec4<f32>(bg.dot_color.rgb, bg.dot_color.a * alpha);
}
