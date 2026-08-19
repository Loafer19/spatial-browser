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
// rounded rect, dims it when unfocused, and blends in a border near the
// edge — a brighter/thicker accent ring when focused, a thin neutral one
// otherwise so an unfocused page never fully blends into a
// same-colored canvas background.
@group(0) @binding(0) var tex0: texture_2d<f32>;
@group(0) @binding(1) var samp0: sampler;

struct PageStyle {
    // xy = page size in pixels, z = corner radius, w = 1.0 if this page
    // has no CEF frame yet (loading placeholder), 0.0 otherwise
    size_radius: vec4<f32>,
    focus_border_color: vec4<f32>,
    unfocused_border_color: vec4<f32>,
    // x = 1.0 if focused else 0.0, y = focus border width,
    // z = unfocused border width, w = dim factor multiplied into an
    // unfocused page's color (1.0 = no dimming)
    flags: vec4<f32>,
};
@group(1) @binding(0) var<uniform> style: PageStyle;

// x = seconds since app start, used only for the loading-placeholder
// pulse below — shared by every page, so it's its own bind group rather
// than duplicated per-page style data.
struct FrameGlobals {
    time: vec4<f32>,
};
@group(2) @binding(0) var<uniform> frame: FrameGlobals;

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
    let is_focused = style.flags.x > 0.5;
    let border_width = select(style.flags.z, style.flags.y, is_focused);
    let border_color = select(style.unfocused_border_color.rgb, style.focus_border_color.rgb, is_focused);
    let dim = select(style.flags.w, 1.0, is_focused);

    // Pixel position relative to the page's center.
    let pixel = (input.tex - vec2<f32>(0.5, 0.5)) * size;
    let dist = sd_rounded_box(pixel, size * 0.5, radius);

    // Fixed-width antialiasing band rather than fwidth()-based, so this
    // doesn't depend on derivative support being available.
    let aa = 1.0;
    let outside_alpha = 1.0 - smoothstep(-aa, aa, dist);

    if (style.size_radius.w > 0.5) {
        // No CEF frame yet — a flat tinted fill (same border color the
        // page will use once it does have content, so this doesn't
        // introduce a third color into the theme) with a pulsing border
        // instead of real content. group 0's texture is still bound to
        // *something* (a 1x1 placeholder — see GpuState::placeholder_bind_group)
        // purely to satisfy the pipeline layout; its contents are never
        // sampled here.
        let pulse = 0.4 + 0.6 * sin(frame.time.x * 4.0);
        var loading_color = vec4<f32>(border_color * 0.15, 1.0);
        let border_mix = smoothstep(-aa, aa, dist + border_width);
        loading_color = vec4<f32>(mix(loading_color.rgb, border_color, border_mix * pulse), loading_color.a);
        loading_color.a = loading_color.a * outside_alpha;
        return loading_color;
    }

    var color = textureSample(tex0, samp0, input.tex);
    // CEF's CPU OSR buffer doesn't reliably paint alpha=255 everywhere
    // (its background/off-page pixels can carry a lower alpha) — with
    // ALPHA_BLENDING that blended page content toward canvas_background,
    // reading as a gray/washed-out filter over everything. Page content
    // is always fully opaque within its own rect; only the corner-round
    // cutout below should ever make a fragment transparent.
    color.a = 1.0;

    // Dimming (unfocused only) applies to the page's own content; the
    // border itself is blended in at full strength afterward so it stays
    // legible against a same-colored canvas background regardless.
    color = vec4<f32>(color.rgb * dim, color.a);

    // 1.0 once `dist` is within `border_width` of the outer edge, 0.0
    // once deeper inside than that — i.e. the ring band. Always drawn
    // (not just when focused) so an unfocused page never fully blends
    // into a same-colored canvas.
    let border_mix = smoothstep(-aa, aa, dist + border_width);
    color = vec4<f32>(mix(color.rgb, border_color, border_mix), color.a);

    color.a = color.a * outside_alpha;
    return color;
}
