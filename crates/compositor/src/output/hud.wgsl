struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var o: VsOut;
    o.clip = vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, atlas_sampler, i.uv);
    return vec4<f32>(i.color.rgb, i.color.a * t.a);
}
