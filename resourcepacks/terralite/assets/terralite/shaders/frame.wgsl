// Frame hook. Runs after the lit world is drawn and before the hotbar.
// The world shader already put sun, shadow, and block light in the vertex
// color. This pass multiplies that picture and can add a mild lava heat haze.
// It does not trace paths.
//
// Bindings: frame_tex 0, frame_samp 1, frame_u 2 (haze.xy = heat, time).
// Entry points are vs_main and fs_main.
// tint is per channel. brightness scales all three. 1, 1, 1 and brightness 1
// leave the picture unchanged when heat is 0. frame.json "enabled": false
// skips this pass.

@group(0) @binding(0)
var frame_tex: texture_2d<f32>;

@group(0) @binding(1)
var frame_samp: sampler;

struct FrameUniforms {
    // x heat amount 0..1, y seconds, z/w unused
    haze: vec4<f32>,
};

@group(0) @binding(2)
var<uniform> frame_u: FrameUniforms;

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
    // Prefer interpolated vs UV (matches the fullscreen triangle); clip/dims
    // is the same for the identity path but vs UV is stabler for warps.
    var uv = input.uv;
    let heat = clamp(frame_u.haze.x, 0.0, 1.0);
    let t = frame_u.haze.y;
    // Rising shimmer — a few pixels of UV crawl, stronger mid-frame over the pool.
    if (heat > 0.01) {
        let mid = 1.0 - abs(uv.y - 0.55) * 1.5;
        let band = clamp(mid, 0.0, 1.0);
        let wave = sin(uv.y * 48.0 + t * 7.0) * 0.55
            + sin(uv.y * 27.0 - t * 4.5 + uv.x * 11.0) * 0.45;
        let drift = sin(uv.x * 22.0 + t * 3.4 + uv.y * 6.0) * 0.40
            + sin(uv.x * 9.0 - t * 2.1) * 0.25;
        // ~6–8px at 1080p when heat is full — mild in motion, readable in a still.
        let px = vec2<f32>(6.0, 8.0) / max(dims, vec2<f32>(1.0, 1.0));
        uv = uv + vec2<f32>(wave, drift) * px * heat * band;
        uv = clamp(uv, vec2<f32>(0.001), vec2<f32>(0.999));
    }
    let color = textureSampleLevel(frame_tex, frame_samp, uv, 0.0).rgb;
    // Warm the mid tones slightly when heat is up — still identity at heat 0.
    let warm = vec3<f32>(1.0 + 0.08 * heat, 1.0 + 0.02 * heat, 1.0 - 0.06 * heat);
    let tint = vec3<f32>(1.0, 1.0, 1.0) * warm;
    let brightness = 1.0;
    return vec4<f32>(clamp(color * tint * brightness, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
