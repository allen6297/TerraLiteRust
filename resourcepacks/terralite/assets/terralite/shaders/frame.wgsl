// Frame hook. Runs after the lit world is drawn and before the hotbar.
// The world shader already put sun, shadow, and block light in the vertex
// color. This pass only multiplies that picture. It does not trace paths.
//
// Bindings are fixed: frame_tex at group 0 binding 0, frame_samp at binding 1.
// Entry points are vs_main and fs_main.
// tint is per channel. brightness scales all three. 1, 1, 1 and brightness 1
// leave the picture unchanged. frame.json "enabled": false skips this pass.

@group(0) @binding(0)
var frame_tex: texture_2d<f32>;

@group(0) @binding(1)
var frame_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> VsOut {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VsOut;
    let pos = positions[vertex];
    out.clip = vec4<f32>(pos, 0.0, 1.0);
    out.uv = vec2<f32>(pos.x * 0.5 + 0.5, 0.5 - pos.y * 0.5);
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let dims_u = textureDimensions(frame_tex);
    let dims = vec2<f32>(f32(dims_u.x), f32(dims_u.y));
    let uv = input.clip.xy / dims;
    let color = textureSampleLevel(frame_tex, frame_samp, uv, 0.0).rgb;
    let tint = vec3<f32>(1.0, 1.0, 1.0);
    let brightness = 1.0;
    return vec4<f32>(clamp(color * tint * brightness, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
