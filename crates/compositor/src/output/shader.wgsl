// Vertex shader
struct VertexInput {
    @location(0) pos: vec4<f32>,
    @location(1) tex: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) pos: vec4<f32>,
    @location(0) tex: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.pos = input.pos;
    output.tex = input.tex;
    return output;
}

// Fragment shader: samples the page's CEF texture, clips it to a
// rounded rect, and (when focused) blends in an accent-colored ring near
// the edge.
@group(0) @binding(0) var tex0: texture_2d<f32>;
@group(0) @binding(1) var samp0: sampler;

struct PageStyle {
    // xy = page size in pixels, z = corner radius, w = focus border width
    size_radius: vec4<f32>,
    border_color: vec4<f32>,
    // x = 1.0 if this page is focused, 0.0 otherwise
    focused: vec4<f32>,
};
@group(1) @binding(0) var<uniform> style: PageStyle;

// Signed distance to a rounded box centered at the origin (Inigo Quilez's
// formula) — negative inside, positive outside, zero on the edge.
fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius, radius);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = style.size_radius.xy;
    let radius = style.size_radius.z;
    let border_width = style.size_radius.w;

    // Pixel position relative to the page's center.
    let pixel = (input.tex - vec2<f32>(0.5, 0.5)) * size;
    let dist = sd_rounded_box(pixel, size * 0.5, radius);

    // Fixed-width antialiasing band rather than fwidth()-based, so this
    // doesn't depend on derivative support being available.
    let aa = 1.0;
    let outside_alpha = 1.0 - smoothstep(-aa, aa, dist);

    var color = textureSample(tex0, samp0, input.tex);

    if (style.focused.x > 0.5) {
        // 1.0 once `dist` is within `border_width` of the outer edge,
        // 0.0 once deeper inside than that — i.e. the ring band.
        let border_mix = smoothstep(-aa, aa, dist + border_width);
        color = vec4<f32>(mix(color.rgb, style.border_color.rgb, border_mix), color.a);
    }

    color.a = color.a * outside_alpha;
    return color;
}
