//! wgpu renderer for the loaded chunk meshes.
//!
//! Back faces are drawn as well as front faces. The greedy mesh only emits
//! exposed sides, and keeping both windings visible means a face is not lost
//! if its triangle order is flipped.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::bytes_of;
use rebuild_world::{
    BAG_SLOTS, BATTERY_CHARGE, BATTERY_MAX, Block, CellChange, Cluster, Emitter, FILL_EMBER,
    FILL_NONE, FILL_WATER, Hooks, Pack, PlayerBox, ScriptView, TAG_BREAK, TAG_PLACE, TANK_BUCKET,
    TANK_MAX, apply_changes, apply_compound_seeps, apply_ember_charges, apply_ember_lamps,
    apply_pump_pushes, apply_water_spouts, battery_at, battery_level, buffer_level, buffer_max,
    compound_accepts_meta, compound_host, encode_bag, encode_break, encode_chat, encode_chest,
    encode_hotbar, encode_place, encode_pose, fill_kind, fill_name, generator_at, generator_on,
    held_light_of, is_battery, is_buffer, is_drain, is_generator, is_lamp, is_pipe, is_pump,
    is_spout, is_tank, is_wire, lamp_at, lamp_on, load_dir, plan_compound_seeps, plan_ember_charges,
    plan_ember_lamps, plan_pump_pushes, plan_water_drains, plan_water_spouts, pump_dir, pump_meta,
    pump_on, shape_of, tank_at, tank_level,
};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event_loop::OwnedDisplayHandle;
use winit::keyboard::KeyCode;
use winit::window::{CursorGrabMode, Window};

use crate::body;
use crate::camera::{Camera, MoveInput};
use crate::gui;
use crate::hotbar;
use crate::look;
use crate::mesh;
use crate::net::Link;
use crate::shade;

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    // x tiles, y sun brightness, z cast-shadow strength, w unused
    params: vec4<f32>,
    sun_dir: vec4<f32>,
    // x map resolution, y cascade count, z blend start, w ray-grid blend
    shadow: vec4<f32>,
    cascades: array<mat4x4<f32>, 3>,
    // x world texel, y depth span, z half extent, w unused
    cascade_info: array<vec4<f32>, 3>,
    // xyz eye, w fog start distance
    fog_eye: vec4<f32>,
    // rgb horizon color, w fog end distance
    fog_color: vec4<f32>,
    // xyz world position of probe (0, 0, 0), w spacing in blocks
    trace_box: vec4<f32>,
};

@group(0) @binding(5)
var trace_vol: texture_3d<f32>;

@group(0) @binding(6)
var trace_samp: sampler;

@group(0) @binding(0)
var<uniform> u: Uniforms;

@group(0) @binding(1)
var block_tex: texture_2d<f32>;

@group(0) @binding(2)
var block_samp: sampler;

@group(0) @binding(3)
var shadow_map: texture_depth_2d_array;

@group(0) @binding(4)
var shadow_samp: sampler_comparison;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) textured: f32,
    @location(5) block: vec3<f32>,
    @location(6) bounce: vec3<f32>,
    @location(7) ao: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Flat, so the column cannot drift across a chunk-sized triangle and
    // sample the next tile. Dirt sits beside grass and reads as red.
    @location(3) @interpolate(flat) textured: f32,
    @location(4) block: vec3<f32>,
    @location(5) bounce: vec3<f32>,
    // Smooth, so a corner fades across the face. This is not the cast shadow.
    @location(6) ao: f32,
    @location(7) world_pos: vec3<f32>,
};

override CASCADE: i32 = 0;

@vertex
fn vs_shadow(input: VsIn) -> @builtin(position) vec4<f32> {
    return u.cascades[CASCADE] * vec4<f32>(input.position, 1.0);
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = u.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    out.normal = input.normal;
    out.uv = input.uv;
    out.textured = input.textured;
    out.block = input.block;
    out.bounce = input.bounce;
    out.ao = input.ao;
    out.world_pos = input.position;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    // A sky disc stores textured < 0 and its circle in uv. It is emissive.
    if (input.textured < -0.5) {
        let p = input.uv * 2.0 - 1.0;
        if (dot(p, p) > 1.0) {
            discard;
        }
        return vec4<f32>(input.block, 1.0);
    }
    // Block light lives in input.block and is not scaled by the sun or moon.
    // input.color is that light plus the ambient term. The day cycle
    // shades only the ambient term. Cast shadow comes from the shadow map at
    // this texel, not from a value stored on the corners. Block light clears it.
    // input.bounce is one bounce off nearby voxels. Brightness scales it.
    // Where the direct shadow covers the texel, that bounce is left out, so
    // a block cannot light the grass it shades. Block light is not in the
    // bounce, so a lantern still owns its faces. input.ao is sky openness:
    // 1 in the open, lower in a cave or against a wall. It darkens the fill
    // only where the cascade has not already shadowed the texel. It does not
    // scale block light.
    // A textured face multiplies the lit color by its column of the pack atlas.
    //
    // A chunk-edge UV of 0 is slightly negative after perspective correction.
    // fract wraps that to just under 1, and the old mapping sat on the shared
    // edge of the next column. Dirt is the column after grass and reads as red
    // on unlit grass. Pick a content texel, then its center.
    let tile = input.uv - floor(input.uv);
    let texel = clamp(floor(tile * 16.0), vec2<f32>(0.0), vec2<f32>(15.0));
    let tiles = max(u.params.x, 1.0);
    let slot = clamp(input.textured - 1.0, 0.0, tiles - 1.0);
    // 18 texels per column: 1 gutter, 16 content, 1 gutter.
    let cell = 18.0;
    let px = texel + vec2<f32>(1.5);
    let atlas_uv = vec2<f32>((px.x + slot * cell) / (tiles * cell), px.y / cell);
    let sampled = textureSampleLevel(block_tex, block_samp, atlas_uv, 0.0);
    // A palette's unused texels are often magenta with zero alpha. The grass
    // fringe is that cutout: keep the colored edge, drop the rest.
    if (input.textured > 0.5 && sampled.a < 0.1) {
        discard;
    }
    let albedo = mix(vec3<f32>(1.0), sampled.rgb, step(0.5, input.textured));
    let sun = normalize(u.sun_dir.xyz);
    let nrm = normalize(input.normal);
    let ndotl = max(dot(nrm, sun), 0.0);
    let hard_facing = 0.34 + 0.66 * ndotl;
    // A small wrap, so two faces of a cube do not step from bright to dark.
    let traced_facing = 0.86 + 0.14 * ndotl;
    let coverage = shadow_coverage(input.world_pos, input.normal, input.block);
    // shadow.w blends in the ray grid. Outside that grid the raster map stays.
    // The sample sits in the air in front of the face, so a probe inside a
    // block does not paint that block. Several taps spread one probe step
    // across the faces on both sides of a block border. Corner darkening
    // stays off. Sky and contact come from the grid, not from a mesh corner.
    let traced_at = input.world_pos + nrm * 0.42;
    let traced = traced_sun(traced_at);
    let use_trace = u.shadow.w * trace_margin(traced_at) * step(0.0, traced.x);
    let sun_vis = traced.x;
    let sky_open = clamp(traced.y, 0.0, 1.0);
    let contact = clamp(traced.z, 0.0, 1.0);
    let facing = mix(hard_facing, traced_facing, use_trace);
    let weight = use_trace;
    // A blocked probe is 0 and an open one is 1. The square root lifts the
    // blend between them so a night tile does not drop to black.
    let sun_soft = sqrt(clamp(sun_vis, 0.0, 1.0));
    let soft_coverage = mix(coverage, 1.0 - sun_soft, weight);
    let floor_shadow = mix(0.42, 0.62, use_trace);
    let shadow_mul = mix(1.0, floor_shadow, soft_coverage * u.params.z);
    let sun_shade = facing * u.params.y * shadow_mul;
    let ambient = input.color - input.block;
    let sky = clamp(input.ao, 0.0, 1.0);
    // Vertex sky darkens only where the ray grid does not cover the texel.
    let extra = (1.0 - sky) * (1.0 - coverage) * (1.0 - use_trace);
    let cave_mul = mix(1.0, 0.16, extra);
    let vis = mix(1.0 - coverage, sun_vis, use_trace);
    // Bounce is baked per face, so it steps where two blocks meet. Inside the
    // ray grid the filtered sky carries that fill instead.
    let indirect = input.bounce * u.params.y * vis * mix(sky, sky_open, use_trace) * (1.0 - use_trace);
    // sky_open and contact are 1 in the open and 0 when closed. Closed stays
    // darker, but the floor stays high so a night probe is a soft shade,
    // not a black square.
    let sky_fill = mix(0.84, 1.0, sqrt(clamp(sky_open, 0.0, 1.0)));
    let crease = mix(0.90, 1.0, sqrt(clamp(contact, 0.0, 1.0)));
    let soften = mix(1.0, sky_fill * crease, use_trace);
    let lit = ambient * sun_shade * cave_mul * soften + indirect + input.block;
    let lit_rgb = lit * albedo;
    let dist = distance(input.world_pos, u.fog_eye.xyz);
    let fog_end = max(u.fog_color.w, u.fog_eye.w + 1.0);
    let fog = smoothstep(u.fog_eye.w, fog_end, dist);
    return vec4<f32>(mix(lit_rgb, u.fog_color.xyz, fog), 1.0);
}

// Selection wire: keep the authored color (white) without sun/grass tint.
@fragment
fn fs_unlit(input: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}

struct ShadowHit {
    coverage: f32,
    edge: f32,
};

// 1 where the texel is behind something else as seen from the sun or moon.
// Coverage is computed here, from the cascade maps. It is not stored on mesh
// vertices. The tightest cascade that contains the point wins, and the seam
// blends into the next one. Lanterns (nonzero block light) stay lit.
fn shadow_coverage(world_pos: vec3<f32>, normal: vec3<f32>, block_light: vec3<f32>) -> f32 {
    if (dot(block_light, block_light) > 1.0e-8) {
        return 0.0;
    }
    let sun = normalize(u.sun_dir.xyz);
    let n = normalize(normal);
    let ndotl = clamp(dot(n, sun), 0.0, 1.0);
    var chosen = 2;
    for (var i = 0; i < 3; i++) {
        if (cascade_edge(i, world_pos) <= 1.0) {
            chosen = i;
            break;
        }
    }
    let primary = cascade_coverage(chosen, world_pos, n, ndotl);
    if (chosen == 2) {
        let fade = smoothstep(0.85, 1.0, primary.edge);
        return primary.coverage * (1.0 - fade);
    }
    let next = cascade_coverage(chosen + 1, world_pos, n, ndotl);
    let blend = smoothstep(u.shadow.z, 1.0, primary.edge);
    return mix(primary.coverage, next.coverage, blend);
}

// x is sun visibility, y sky openness, z contact. x is -1 outside the grid,
// so the caller keeps the raster shadow and the corner darkening.
// One tap, still filtered by the sampler.
fn trace_tap(world_pos: vec3<f32>) -> vec3<f32> {
    let spacing = max(u.trace_box.w, 1.0);
    let uvw = (world_pos - u.trace_box.xyz) / spacing;
    let size = vec3<f32>(textureDimensions(trace_vol));
    let coord = (uvw + vec3<f32>(0.5)) / size;
    if (coord.x < 0.0 || coord.y < 0.0 || coord.z < 0.0 || coord.x > 1.0 || coord.y > 1.0 || coord.z > 1.0) {
        return vec3<f32>(-1.0, -1.0, -1.0);
    }
    return textureSampleLevel(trace_vol, trace_samp, coord, 0.0).rgb;
}

fn add_shade(world_pos: vec3<f32>, w: f32, sum: ptr<function, vec3<f32>>, weight: ptr<function, f32>) {
    if (w <= 0.0) {
        return;
    }
    let tap = trace_tap(world_pos);
    if (tap.x < 0.0) {
        return;
    }
    *sum += tap * w;
    *weight += w;
}

// Probes sit two blocks apart. Taps at one and a half and three blocks fade
// that step across the grass. The reach is the same in the dark, and nothing
// is sampled upward, so a darker probe stays darker.
fn traced_sun(world_pos: vec3<f32>) -> vec3<f32> {
    let center = trace_tap(world_pos);
    if (center.x < 0.0) {
        return center;
    }
    let reach = 3.6;
    let dark = 1.0 - clamp(center.y, 0.0, 1.0);
    var sum = center * 1.0;
    var weight = 1.0;
    let rings = array<f32, 3>(0.8, 1.8, 3.0);
    for (var ring = 0; ring < 3; ring++) {
        let step = rings[ring];
        let diag = step * 0.72;
        let taps = array<vec2<f32>, 8>(
            vec2<f32>(step, 0.0),
            vec2<f32>(-step, 0.0),
            vec2<f32>(0.0, step),
            vec2<f32>(0.0, -step),
            vec2<f32>(diag, diag),
            vec2<f32>(diag, -diag),
            vec2<f32>(-diag, diag),
            vec2<f32>(-diag, -diag),
        );
        for (var i = 0; i < 8; i++) {
            let dist = length(taps[i]);
            let w = (1.0 - smoothstep(0.0, reach, dist)) * (1.0 + dark * 0.55);
            add_shade(world_pos + vec3<f32>(taps[i].x, 0.0, taps[i].y), w, &sum, &weight);
        }
    }
    return sum / weight;
}

// 1 in the middle of the grid, easing to 0 before the edge so the raster
// shadow takes over without a hard box.
fn trace_margin(world_pos: vec3<f32>) -> f32 {
    let spacing = max(u.trace_box.w, 1.0);
    let uvw = (world_pos - u.trace_box.xyz) / spacing;
    let size = vec3<f32>(textureDimensions(trace_vol));
    let coord = (uvw + vec3<f32>(0.5)) / size;
    let edge = min(min(coord.x, coord.y), coord.z);
    let far = min(min(1.0 - coord.x, 1.0 - coord.y), 1.0 - coord.z);
    return smoothstep(0.0, 0.18, min(edge, far));
}

fn cascade_edge(index: i32, world_pos: vec3<f32>) -> f32 {
    let clip = u.cascades[index] * vec4<f32>(world_pos, 1.0);
    let ndc = clip.xy / clip.w;
    return max(abs(ndc.x), abs(ndc.y));
}

// The 3x3 kernel is one hardware compare per tap. It stays axis-aligned: a
// per-texel rotation turned a marginal depth error into jagged triangles on
// the two halves of a ground quad whenever the sun snapped to a new texel.
// Bias is two texels plus the light's slope, and it is capped at four texels
// of this cascade, so a far map can clear its own acne without opening a
// block-wide gap on the near map. A sample outside this cascade's depth is
// not lit; the next cascade answers it.
fn cascade_coverage(index: i32, world_pos: vec3<f32>, n: vec3<f32>, ndotl: f32) -> ShadowHit {
    let info = u.cascade_info[index];
    let texel_world = info.x;
    let facing = max(ndotl, 0.2);
    let tan_theta = sqrt(max(1.0 - ndotl * ndotl, 0.0)) / facing;
    let bias_world = min(texel_world * (2.0 + 2.0 * tan_theta), texel_world * 4.0);
    let origin = world_pos + n * min(bias_world, texel_world);
    let clip = u.cascades[index] * vec4<f32>(origin, 1.0);
    let ndc = clip.xyz / clip.w;
    let edge = max(abs(ndc.x), abs(ndc.y));
    if (ndc.z < 0.0 || ndc.z > 1.0) {
        var miss: ShadowHit;
        miss.coverage = 0.0;
        miss.edge = 2.0;
        return miss;
    }
    if (edge > 1.0) {
        var miss: ShadowHit;
        miss.coverage = 0.0;
        miss.edge = edge;
        return miss;
    }
    // Y-up clip space: +y is the top of the map, and texture v grows downward.
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    let ref_z = ndc.z - bias_world / info.y;
    let map_texel = 1.0 / u.shadow.x;
    // About half a block of penumbra, so a shadow edge fades across a face
    // instead of drawing a line on the block border. Clamped so a far cascade
    // does not smear past its own texels.
    let penumbra = clamp(0.55 / max(texel_world, 1.0e-3), 1.0, 40.0);
    let taps = array<vec2<f32>, 9>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.75, 0.0),
        vec2<f32>(-0.75, 0.0),
        vec2<f32>(0.0, 0.75),
        vec2<f32>(0.0, -0.75),
        vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, 0.5),
        vec2<f32>(0.5, -0.5),
        vec2<f32>(-0.5, -0.5),
    );
    var lit = 0.0;
    for (var i = 0; i < 9; i++) {
        lit += textureSampleCompare(shadow_map, shadow_samp, uv + taps[i] * map_texel * penumbra, index, ref_z);
    }
    var hit: ShadowHit;
    hit.coverage = 1.0 - lit / 9.0;
    hit.edge = edge;
    return hit;
}
"#;

const UI_SHADER: &str = r#"
struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tex: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tex: f32,
};

@group(0) @binding(0) var block_tex: texture_2d<f32>;
@group(0) @binding(1) var block_samp: sampler;
@group(0) @binding(2) var<uniform> ui_params: vec4<f32>;

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(input.position, 0.0, 1.0);
    out.color = input.color;
    out.uv = input.uv;
    out.tex = input.tex;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    // Same guttered atlas as the world shader. tex 0 stays the flat color.
    // A panel or button mixes that color with the named pack tile.
    let tile = input.uv - floor(input.uv);
    let texel = clamp(floor(tile * 16.0), vec2<f32>(0.0), vec2<f32>(15.0));
    let tiles = max(ui_params.x, 1.0);
    let slot = clamp(input.tex - 1.0, 0.0, tiles - 1.0);
    let cell = 18.0;
    let px = texel + vec2<f32>(1.5);
    let atlas_uv = vec2<f32>((px.x + slot * cell) / (tiles * cell), px.y / cell);
    let sampled = textureSample(block_tex, block_samp, atlas_uv).rgb;
    let grain = mix(input.color, sampled, step(0.5, input.tex) * 0.78);
    return vec4<f32>(grain, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UiVertex {
    position: [f32; 2],
    color: [f32; 3],
    uv: [f32; 2],
    tex: f32,
}

const UI_VERTEX_CAP: u64 = 131072;
const UI_INDEX_CAP: u64 = 196608;
/// Enough edges for the largest pack shape we draw a wire around.
const OUTLINE_VERTEX_CAP: u64 = 64 * 24;
const SKY_VERTEX_CAP: u64 = 48;
/// Six triangles on each face of up to 32 solid boxes.
const DIG_VERTEX_CAP: u64 = 32 * 36;

/// Each cascade is this many texels on a side. Three layers: near, mid, far.
const CASCADE_MAP: u32 = 2048;
const CASCADE_COUNT: usize = 3;
/// Half-width of each light box, in blocks. The near box matches the previous
/// texel size (about 1.6 cm). The far box is wider than the old single map.
const CASCADE_HALF: [f32; 3] = [16.0, 48.0, 144.0];

fn cascade_window(index: usize) -> (f32, f32, f32, f32) {
    let half = CASCADE_HALF[index];
    let eye = half * 4.0;
    let near = (eye - half * 2.0).max(1.0);
    let far = eye + half * 2.0;
    (half, eye, near, far)
}

fn cascade_texel(index: usize) -> f32 {
    (CASCADE_HALF[index] * 2.0) / CASCADE_MAP as f32
}

fn cascade_span(index: usize) -> f32 {
    let (_, _, near, far) = cascade_window(index);
    far - near
}

#[cfg(test)]
fn shadow_texel_world() -> f32 {
    cascade_texel(0)
}

/// Receiver slop along the light, in blocks, using the near cascade's texel.
/// `ndotl` is the sun against the face. A face toward the sun gets two texels,
/// a grazing face gets up to four, and that cap stays under a tenth of a block
/// on the near cascade. The fragment shader uses this same formula.
#[cfg(test)]
fn shadow_receiver_bias(ndotl: f32) -> f32 {
    let ndotl = ndotl.clamp(0.0, 1.0);
    let facing = ndotl.max(0.2);
    let tan_theta = (1.0 - ndotl * ndotl).max(0.0).sqrt() / facing;
    let texel = shadow_texel_world();
    (texel * (2.0 + 2.0 * tan_theta)).min(texel * 4.0)
}

const WORLD_ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x3,
    3 => Float32x2,
    4 => Float32,
    5 => Float32x3,
    6 => Float32x3,
    7 => Float32,
];

/// Orthographic view from the sun or moon for cascade 0, snapped to texels.
#[cfg(test)]
pub fn light_view_proj(focus: [f32; 3], direction: [f32; 3]) -> [f32; 16] {
    cascade_view_proj(focus, direction, 0)
}

/// One cascade's light matrix. Depth is 0 near the light and 1 at the far side.
pub fn cascade_view_proj(focus: [f32; 3], direction: [f32; 3], cascade: usize) -> [f32; 16] {
    use glam::camera::rh::proj::directx;
    use glam::camera::rh::view;
    use glam::{Mat4, Vec3};

    let cascade = cascade.min(CASCADE_COUNT - 1);
    let (half, eye_distance, near, far) = cascade_window(cascade);
    let dir = Vec3::from(direction);
    let dir = if dir.length_squared() < 1.0e-8 {
        Vec3::Y
    } else {
        dir.normalize()
    };
    let focus = Vec3::from(focus);
    let eye = focus + dir * eye_distance;
    let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    let view_matrix = view::look_at_mat4(eye, focus, up);
    let texel = cascade_texel(cascade);
    let light_space = view_matrix * focus.extend(1.0);
    // Lock the focus to a texel center. Snapping to the edge between texels
    // shifts every shadow by half a texel.
    let center = |value: f32| (value / texel).floor() * texel + texel * 0.5;
    let snap = Mat4::from_translation(Vec3::new(
        center(light_space.x) - light_space.x,
        center(light_space.y) - light_space.y,
        0.0,
    ));
    let proj = directx::orthographic(-half, half, -half, half, near, far);
    (proj * snap * view_matrix).to_cols_array()
}

fn world_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<mesh::Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &WORLD_ATTRS,
    }
}

struct FrameTarget {
    // The view and bind group keep the texture alive.
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

struct FramePass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    target: Option<FrameTarget>,
    width: u32,
    height: u32,
}

impl FramePass {
    fn load(device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<Self> {
        let hook = match shade::load_frame_hook() {
            Ok(hook) => hook,
            Err(err) => {
                eprintln!("shader: {err}");
                return None;
            }
        };
        if !hook.enabled {
            return None;
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frame"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame"),
            source: wgpu::ShaderSource::Wgsl(hook.source.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frame"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frame"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Some(Self {
            pipeline,
            layout,
            sampler,
            target: None,
            width: 0,
            height: 0,
        })
    }
}

struct DepthBuffer {
    // The view keeps the texture alive; the field stops the texture being dropped.
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Pack atlas, or a 1×1 white pixel when no PNG decodes.
fn upload_block_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let image = crate::texture::load_pack_texture();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block-texture"),
        size: wgpu::Extent3d {
            width: image.width.max(1),
            height: image.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let row = image.width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = row.div_ceil(align) * align;
    let mut bytes = vec![0u8; padded as usize * image.height as usize];
    for y in 0..image.height as usize {
        let src = y * row as usize;
        let dst = y * padded as usize;
        bytes[dst..dst + row as usize].copy_from_slice(&image.rgba[src..src + row as usize]);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded),
            rows_per_image: Some(image.height),
        },
        wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    (view, sampler)
}

impl DepthBuffer {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }
}

struct PendingEdit {
    kind: u8,
    slot: Option<usize>,
}

struct ChunkGpu {
    x: i32,
    z: i32,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

pub struct State {
    instance: wgpu::Instance,
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    view_format: wgpu::TextureFormat,
    configured: bool,
    depth: Option<DepthBuffer>,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    chunk_draws: Vec<ChunkGpu>,
    outline_pipeline: wgpu::RenderPipeline,
    outline_buf: wgpu::Buffer,
    dig_pipeline: wgpu::RenderPipeline,
    dig_buf: wgpu::Buffer,
    dig: crate::dig::Dig,
    break_id: String,
    break_factor: f32,
    ui_pipeline: wgpu::RenderPipeline,
    ui_bind: wgpu::BindGroup,
    // The bind group samples this. Dropping it would free the tile count.
    #[allow(dead_code)]
    ui_tile_buf: wgpu::Buffer,
    ui_vertex_buf: wgpu::Buffer,
    ui_index_buf: wgpu::Buffer,
    world: Cluster,
    link: Link,
    pending: VecDeque<PendingEdit>,
    told_disconnect: bool,
    camera: Camera,
    input: MoveInput,
    hotbar: hotbar::Hotbar,
    bag: hotbar::Bag,
    /// The server has not heard this hotbar yet.
    hotbar_dirty: bool,
    /// The save has not heard this inventory yet.
    bag_dirty: bool,
    pose_dirty: bool,
    pose_left: f32,
    posed_eye: glam::Vec3,
    posed_yaw: f32,
    posed_pitch: f32,
    port: u16,
    gui: gui::Gui,
    menu: gui::Menu,
    /// Remappable keys / mouse. Persists in `saves/keybinds.json`.
    binds: crate::keybinds::Keybinds,
    /// Pause / settings / creative / items — egui on wgpu. World stays separate.
    egui: crate::egui_ui::EguiHost,
    /// True after Play, until quit to title.
    playing: bool,
    loaded_world: String,
    audio: crate::audio::Audio,
    /// False when a dedicated server owns the square.
    local: bool,
    quit: bool,
    /// One-shot window capture aimed at the zombie, then quit.
    shot_path: Option<std::path::PathBuf>,
    shot_frames: u32,
    /// One-shot capture of the Godot-style settings screen, then quit.
    settings_shot_path: Option<std::path::PathBuf>,
    settings_shot_frames: u32,
    keybinds_shot_path: Option<std::path::PathBuf>,
    keybinds_shot_frames: u32,
    /// One-shot creative inventory (flow compound tab), then quit.
    creative_shot_path: Option<std::path::PathBuf>,
    creative_shot_frames: u32,
    /// One-shot night corridor: dirt hand vs torch, then quit.
    held_light_shot_path: Option<std::path::PathBuf>,
    held_light_shot_frames: u32,
    held_light_shot_phase: HeldLightShotPhase,
    held_light_shot_dark: Option<CapturedRgba>,
    held_light_shot_eye: glam::Vec3,
    held_light_shot_yaw: f32,
    held_light_shot_pitch: f32,
    /// One-shot midnight horde near the player, then quit.
    night_spawn_shot_path: Option<std::path::PathBuf>,
    night_spawn_shot_frames: u32,
    night_spawn_shot_eye: glam::Vec3,
    night_spawn_shot_yaw: f32,
    night_spawn_shot_pitch: f32,
    /// One-shot bed sleep → morning, then quit.
    bed_sleep_shot_path: Option<std::path::PathBuf>,
    bed_sleep_shot_frames: u32,
    bed_sleep_shot_eye: glam::Vec3,
    bed_sleep_shot_yaw: f32,
    bed_sleep_shot_pitch: f32,
    /// One-shot sneak at a cliff edge, then quit.
    crouch_shot_path: Option<std::path::PathBuf>,
    crouch_shot_frames: u32,
    crouch_shot_eye: glam::Vec3,
    crouch_shot_yaw: f32,
    crouch_shot_pitch: f32,
    /// One-shot walk FOV vs sprint FOV stitch, then quit.
    sprint_shot_path: Option<std::path::PathBuf>,
    sprint_shot_frames: u32,
    sprint_shot_phase: SprintShotPhase,
    sprint_shot_walk: Option<CapturedRgba>,
    sprint_shot_eye: glam::Vec3,
    sprint_shot_yaw: f32,
    sprint_shot_pitch: f32,
    /// One-shot dry shore vs underwater FOV/tint stitch, then quit.
    swim_shot_path: Option<std::path::PathBuf>,
    swim_shot_frames: u32,
    swim_shot_phase: SwimShotPhase,
    swim_shot_dry: Option<CapturedRgba>,
    swim_shot_dry_eye: glam::Vec3,
    swim_shot_wet_eye: glam::Vec3,
    swim_shot_yaw: f32,
    swim_shot_pitch: f32,
    /// One-shot idle boat vs rowing FOV stitch, then quit.
    boat_shot_path: Option<std::path::PathBuf>,
    boat_shot_frames: u32,
    boat_shot_phase: BoatShotPhase,
    boat_shot_idle: Option<CapturedRgba>,
    boat_shot_eye: glam::Vec3,
    boat_shot_yaw: f32,
    boat_shot_pitch: f32,
    /// One-shot idle cart vs riding FOV stitch, then quit.
    cart_shot_path: Option<std::path::PathBuf>,
    cart_shot_frames: u32,
    cart_shot_phase: CartShotPhase,
    cart_shot_idle: Option<CapturedRgba>,
    cart_shot_eye: glam::Vec3,
    cart_shot_yaw: f32,
    cart_shot_pitch: f32,
    /// One-shot cooling vs ready attack crosshair stitch, then quit.
    attack_shot_path: Option<std::path::PathBuf>,
    attack_shot_frames: u32,
    attack_shot_phase: AttackShotPhase,
    attack_shot_cool: Option<CapturedRgba>,
    attack_shot_eye: glam::Vec3,
    attack_shot_yaw: f32,
    attack_shot_pitch: f32,
    /// One-shot fed vs starving hunger veil stitch, then quit.
    hunger_shot_path: Option<std::path::PathBuf>,
    hunger_shot_frames: u32,
    hunger_shot_phase: HungerShotPhase,
    hunger_shot_fed: Option<CapturedRgba>,
    hunger_shot_eye: glam::Vec3,
    hunger_shot_yaw: f32,
    hunger_shot_pitch: f32,
    /// One-shot idle vs chewing FOV stitch, then quit.
    eat_shot_path: Option<std::path::PathBuf>,
    eat_shot_frames: u32,
    eat_shot_phase: EatShotPhase,
    eat_shot_idle: Option<CapturedRgba>,
    eat_shot_eye: glam::Vec3,
    eat_shot_yaw: f32,
    eat_shot_pitch: f32,
    /// One-shot grounded vs jump FOV stitch, then quit.
    jump_shot_path: Option<std::path::PathBuf>,
    jump_shot_frames: u32,
    jump_shot_phase: JumpShotPhase,
    jump_shot_ground: Option<CapturedRgba>,
    jump_shot_eye: glam::Vec3,
    jump_shot_yaw: f32,
    jump_shot_pitch: f32,
    /// One-shot idle vs place FOV stitch, then quit.
    place_shot_path: Option<std::path::PathBuf>,
    place_shot_frames: u32,
    place_shot_phase: PlaceShotPhase,
    place_shot_idle: Option<CapturedRgba>,
    place_shot_eye: glam::Vec3,
    place_shot_yaw: f32,
    place_shot_pitch: f32,
    /// One-shot idle vs hotbar-swap FOV stitch, then quit.
    swap_shot_path: Option<std::path::PathBuf>,
    swap_shot_frames: u32,
    swap_shot_phase: SwapShotPhase,
    swap_shot_idle: Option<CapturedRgba>,
    swap_shot_eye: glam::Vec3,
    swap_shot_yaw: f32,
    swap_shot_pitch: f32,
    /// One-shot idle vs break FOV stitch, then quit.
    break_shot_path: Option<std::path::PathBuf>,
    break_shot_frames: u32,
    break_shot_phase: BreakShotPhase,
    break_shot_idle: Option<CapturedRgba>,
    break_shot_eye: glam::Vec3,
    break_shot_yaw: f32,
    break_shot_pitch: f32,
    hooks: Hooks,
    entity_time: f32,
    air_peak: f32,
    airborne: bool,
    /// This Space hold already asked `events.jump`.
    jump_asked: bool,
    /// That ask returned false, so Space stays on the ground until release.
    jump_blocked: bool,
    /// This Ctrl-and-walk hold already asked `events.sprint`.
    sprint_asked: bool,
    /// That ask returned false, so the walk stays at normal speed until it stops.
    sprint_blocked: bool,
    /// Selected hotbar slot lift, `1` just placed down to `0`. Also tightens FOV.
    slot_punch: f32,
    /// Brief FOV flick after changing the selected hotbar slot.
    swap_kick: f32,
    /// Brief FOV tighten when a block breaks.
    break_kick: f32,
    /// Walk cycle for the view. The body stays where it is.
    bob_phase: f32,
    /// `0` while still, `1` once a step is showing in the view.
    bob_amount: f32,
    /// `0` standing, `1` fully crouched. Eases when Shift changes.
    crouch: f32,
    /// `1` while sprinting (snaps open, eases shut). Widens the view.
    sprint_kick: f32,
    /// Seconds of continuous sprint. Feeds the breath cue on stop.
    sprint_run: f32,
    /// HUD plate amount for the sprint cue, `0`..`1`.
    sprint_show: f32,
    /// `1` while the boat is moving (eases). Widens the view a little.
    boat_kick: f32,
    /// HUD plate amount for the row cue, `0`..`1`.
    boat_show: f32,
    /// `1` while the minecart is moving (eases). Widens the view a little.
    cart_kick: f32,
    /// HUD plate amount for the ride cue, `0`..`1`.
    cart_show: f32,
    /// `1` while galloping a horse (eases). Widens the view.
    horse_kick: f32,
    /// HUD plate amount for the gallop cue, `0`..`1`.
    horse_show: f32,
    horse_stroke: f32,
    /// One-shot idle horse vs gallop FOV stitch, then quit.
    horse_shot_path: Option<std::path::PathBuf>,
    horse_shot_frames: u32,
    horse_shot_phase: HorseShotPhase,
    horse_shot_idle: Option<CapturedRgba>,
    horse_shot_eye: glam::Vec3,
    horse_shot_yaw: f32,
    horse_shot_pitch: f32,
    /// Seconds until the next paddle stroke while rowing.
    boat_stroke: f32,
    cart_stroke: f32,
    /// `1` when a hit lands, then it fades. Tightens the view.
    hit_kick: f32,
    /// Red edge, `1` just hurt down to `0`.
    hurt_flash: f32,
    /// `1` just ate or drank, fading toward `0`. Dips the view and draws a strip.
    sip: f32,
    /// The strip is a drink, not a bite.
    sipping: bool,
    /// Chew phase while `sip` is up. Feeds FOV and eye bob.
    chew_phase: f32,
    /// Food finish burp still owed once `sip` crosses the cue line.
    burp_due: bool,
    /// Hearts hit zero. Enter, Space, Escape, or a click respawns.
    dead: bool,
    /// Alt place-shape picker for stone / oak families.
    shape_pick: crate::shape_pick::ShapePick,
    /// The plate the player is standing on, and the gate it opened.
    plate_held: Option<PlateHold>,
    /// A neighbor to close once the open place has been answered.
    plate_close: Option<[i32; 3]>,
    /// A door or gate to open or close once the lever flip has been answered.
    lever_follow: Option<([i32; 3], bool)>,
    /// A rung bell and the seconds left before it swings back.
    bell_return: Option<([i32; 3], f32)>,
    /// The lodestone a held compass aims at. Empty aims at the stairs.
    lodestone: Option<[i32; 3]>,
    /// Right mouse is down on a held spyglass.
    zoom_mouse: bool,
    /// Control key is down (trackpad Ctrl+click secondary place).
    ctrl_mod: bool,
    /// Last place/use click time — collapses WindowEvent + DeviceEvent doubles.
    place_click_at: Option<Instant>,
    /// Seconds until the next compound fill seep pulse.
    compound_flow_wait: f32,
    /// Pulses since the last lamp battery drain (slow charge consume).
    lamp_drain_wait: u8,
    /// Pulses until the next ember generator charge tick.
    gen_charge_wait: u8,
    /// One tip when fill first seeps through a pipe this session.
    compound_flow_told: bool,
    /// The full map sheet is open.
    map_open: bool,
    /// F is down on a held spyglass.
    zoom_key: bool,
    /// Seconds until a held goat horn can sound again.
    horn_wait: f32,
    /// The body overlapped water on the previous tick.
    was_wet: bool,
    /// `0` dry through `1` when the eye is underwater.
    water_sub: f32,
    /// Brief FOV punch on the splash that enters water.
    splash_kick: f32,
    /// Brief FOV widen on jump takeoff.
    jump_kick: f32,
    /// Eased afternoon rain mix, `0` clear through `1` at peak wet spell.
    rain_mix: f32,
    /// True after a rain-onset chat tip this wet spell.
    rain_told: bool,
    /// First approach chat tips for the three findable landmarks this session.
    found_camp: bool,
    found_farm: bool,
    found_scarecrow: bool,
    found_shrine: bool,
    found_mine: bool,
    found_rails: bool,
    found_clay: bool,
    /// Once: walked up to the starter compound demo by the pad.
    found_compound: bool,
    /// Once: dusk fireflies near water.
    found_fireflies: bool,
    /// Once: walked up to the pad wolf.
    found_wolf: bool,
    /// Once: walked up to the pad cat.
    found_cat: bool,
    /// Once: walked up to the pad parrot.
    found_parrot: bool,
    found_horse: bool,
    /// Once: holding raw while looking at a campfire or smoker.
    told_cook_heat: bool,
    /// Once: holding blastable stone while looking at a blast furnace.
    told_blast_heat: bool,
    /// Once: holding clay while looking at a furnace.
    told_furnace_heat: bool,
    /// Once: holding bricks or clay at a crafting table with both on the bar.
    told_shape_pot: bool,
    /// Once: look at a pump — tip to toggle.
    told_pump: bool,
    /// Once: look at a tank — tip that it buffers fill.
    told_tank: bool,
    /// Once: look at a battery — tip that it stores ember charge.
    told_battery: bool,
    /// Once: look at a lamp — tip that wires power it.
    told_lamp: bool,
    /// Once: look at a spout — tip that it dumps tank/pipe water.
    told_spout: bool,
    /// Once: look at a drain — tip that it pulls world water into pipes.
    told_drain: bool,
    /// Once: look at a generator — tip that it charges batteries.
    told_generator: bool,
    /// Eased cave-cover drip mix, `0` open through `1` deep under solid.
    drip_mix: f32,
    /// Eased forest leaf rustle, `0` elsewhere through `1` in open woods.
    rustle_mix: f32,
    /// Eased night cricket mix, `0` day through `1` deep night.
    cricket_mix: f32,
    /// Eased dawn bird mix, `0` elsewhere through `1` at sunrise peak.
    bird_mix: f32,
    /// Eased campfire proximity, `0` far through `1` at the fire.
    crackle_mix: f32,
    /// World position of the campfire feeding `crackle_mix`, when nearby.
    crackle_at: Option<[f32; 3]>,
    /// Seconds until the next campfire smoke puff.
    smoke_wait: f32,
    /// Seconds until the next dusk firefly can spawn.
    firefly_wait: f32,
    /// One-shot campfire smoke proof PNG, then quit.
    campfire_smoke_shot_path: Option<std::path::PathBuf>,
    campfire_smoke_shot_frames: u32,
    campfire_smoke_shot_eye: glam::Vec3,
    campfire_smoke_shot_yaw: f32,
    campfire_smoke_shot_pitch: f32,
    /// One-shot campfire cook (raw → cooked) proof PNG, then quit.
    cook_shot_path: Option<std::path::PathBuf>,
    cook_shot_frames: u32,
    cook_shot_eye: glam::Vec3,
    cook_shot_yaw: f32,
    cook_shot_pitch: f32,
    /// While set, cook-shot frames keep a pitch mote rising at this origin.
    note_shot_at: Option<[f32; 3]>,
    note_shot_pitch: u8,
    /// Cook-shot refresh uses book swirl pages instead of a note mote.
    note_shot_book: bool,
    /// Cook-shot refresh uses brew bubbles instead of a note mote.
    note_shot_brew: bool,
    /// One-shot shrine ruin proof PNG, then quit.
    shrine_shot_path: Option<std::path::PathBuf>,
    shrine_shot_frames: u32,
    shrine_shot_eye: glam::Vec3,
    shrine_shot_yaw: f32,
    shrine_shot_pitch: f32,
    /// One-shot mine-mouth chest proof PNG, then quit.
    mine_shot_path: Option<std::path::PathBuf>,
    mine_shot_frames: u32,
    mine_shot_eye: glam::Vec3,
    mine_shot_yaw: f32,
    mine_shot_pitch: f32,
    /// Eased open-day wind mix, `0` night/cave through `1` under noon sky.
    wind_mix: f32,
    /// Eased water-edge proximity, `0` far through `1` at the surface.
    lap_mix: f32,
    /// World position of the water surface feeding `lap_mix`, when nearby.
    lap_at: Option<[f32; 3]>,
    /// Seconds until a held mace can swing again.
    mace_wait: f32,
    /// The next trial key pays a richer drop.
    omen: bool,
    /// The spawner whose wave is running.
    trial_at: Option<[i32; 3]>,
    /// A cleared wave still owes a trial key.
    trial_due: bool,
    /// The live wave is a breeze. A zombie wave leaves this clear.
    wave_breeze: bool,
    /// The next spawner wave calls a breeze.
    trial_flip: bool,
    /// A dropped breeze still owes a rod.
    breeze_due: bool,
    slime_due: bool,
    horn_due: bool,
    /// Seconds left before a golem's copper chest shuts.
    golem_shut: f32,
    /// Bed or charged respawn anchor. Empty respawns by the stairs.
    spawn_at: Option<[i32; 3]>,
    /// Full-screen sleep veil. `1` just woke, fading to `0`.
    sleep_fade: f32,
    /// A struck rod, seconds left on the flash, and whether the neighbor already pulsed.
    rod_strike: Option<([i32; 3], f32, bool)>,
    /// Cast fishing bobber: surface center, seconds until catch, seconds until next splash mote.
    fish_bobber: Option<([f32; 3], f32, f32)>,
    /// A hit target, seconds left on the flash, and whether the neighbor already pulsed.
    target_hit: Option<([i32; 3], f32, bool)>,
    frame: Option<FramePass>,
    drops: crate::drops::Drops,
    drop_buf: Option<wgpu::Buffer>,
    drop_count: u32,
    sheep: crate::sheep::Sheep,
    sheep_buf: Option<wgpu::Buffer>,
    sheep_count: u32,
    pig: crate::mobs::Pig,
    pig_buf: Option<wgpu::Buffer>,
    pig_count: u32,
    horse: crate::mobs::Horse,
    horse_buf: Option<wgpu::Buffer>,
    horse_count: u32,
    zombie: crate::mobs::Zombie,
    /// Natural night/cave Blockbench zombies (capped). Trial zombie stays above.
    night: crate::mobs::NightHorde,
    zombie_buf: Option<wgpu::Buffer>,
    zombie_count: u32,
    breeze: crate::mobs::Breeze,
    breeze_buf: Option<wgpu::Buffer>,
    breeze_count: u32,
    golem: crate::mobs::Golem,
    golem_buf: Option<wgpu::Buffer>,
    golem_count: u32,
    wolf: crate::mobs::Wolf,
    wolf_buf: Option<wgpu::Buffer>,
    wolf_count: u32,
    cat: crate::mobs::Cat,
    cat_buf: Option<wgpu::Buffer>,
    cat_count: u32,
    parrot: crate::mobs::Parrot,
    parrot_buf: Option<wgpu::Buffer>,
    parrot_count: u32,
    frog: crate::mobs::Frog,
    frog_buf: Option<wgpu::Buffer>,
    frog_count: u32,
    axolotl: crate::mobs::Axolotl,
    axolotl_buf: Option<wgpu::Buffer>,
    axolotl_count: u32,
    chicken: crate::mobs::Chicken,
    chicken_buf: Option<wgpu::Buffer>,
    chicken_count: u32,
    bee: crate::mobs::Bee,
    bee_buf: Option<wgpu::Buffer>,
    bee_count: u32,
    fox: crate::mobs::Fox,
    fox_buf: Option<wgpu::Buffer>,
    fox_count: u32,
    goat: crate::mobs::Goat,
    goat_buf: Option<wgpu::Buffer>,
    goat_count: u32,
    /// Mobs from `entity.register`. They stand east of the oak stairs.
    guests: Vec<crate::mobs::Guest>,
    guest_buf: Option<wgpu::Buffer>,
    guest_count: u32,
    boat: crate::boat::Boat,
    boat_buf: Option<wgpu::Buffer>,
    boat_count: u32,
    cart: crate::minecart::Minecart,
    cart_buf: Option<wgpu::Buffer>,
    cart_count: u32,
    others: Vec<crate::peers::Other>,
    peer_buf: Option<wgpu::Buffer>,
    peer_count: u32,
    /// Local Blockbench body. Drawn in third person (F5).
    player: crate::player::Pose,
    player_buf: Option<wgpu::Buffer>,
    player_count: u32,
    /// Pulled-back camera so the local body is on screen.
    third_person: bool,
    hearts: i32,
    cactus_left: f32,
    magma_left: f32,
    cake_eat: Option<[i32; 3]>,
    /// Source cell and landing, set while a dragon egg break is in flight.
    egg_hop: Option<([i32; 3], [i32; 3])>,
    /// Cell whose end crystal is shattering.
    crystal_blast: Option<[i32; 3]>,
    hunger: crate::body::Hunger,
    armor: crate::body::Armor,
    chest_at: Option<(i32, i32, i32)>,
    chest_dirty: bool,
    captured: bool,
    cursor_x: f32,
    cursor_y: f32,
    clock: crate::daylight::Clock,
    /// Lookups the bake was aimed at. A new step queues the full-detail ring.
    shadow_lookups: Vec<[i32; 2]>,
    /// Latest sun or moon step. A bake for it runs across frames.
    wanted_lookups: Vec<[i32; 2]>,
    wanted_sun: [f32; 3],
    /// Chunk the camera is in. A move across a chunk border rebakes far detail.
    wanted_focus: [i32; 2],
    /// Chunk meshes already on the GPU, plus the ones still waiting.
    chunk_bake: mesh::ChunkBake,
    terrain: rebuild_world::TerrainPump,
    terrain_dirty: bool,
    /// Frames to wait after the last generated chunk before a terrain remesh.
    terrain_quiet: u8,
    /// Bumped when the world is replaced so a late worker result is dropped.
    job_epoch: u64,
    terrain_lane: crate::jobs::Lane<(u64, i32, i32), (u64, rebuild_world::Chunk)>,
    mesh_pool: crate::jobs::Pool<mesh::MeshWork, mesh::MeshDone>,
    sky_buf: wgpu::Buffer,
    sky_label: String,
    // Kept so the sample view stays alive. The color pass binds it.
    #[allow(dead_code)]
    shadow_tex: wgpu::Texture,
    #[allow(dead_code)]
    shadow_view: wgpu::TextureView,
    shadow_layers: [wgpu::TextureView; CASCADE_COUNT],
    shadow_pipelines: [wgpu::RenderPipeline; CASCADE_COUNT],
    /// Uniforms only. The color pass's group also samples the shadow map,
    /// and binding that while the map is the depth target is a usage conflict.
    shadow_bind: wgpu::BindGroup,
    trace: crate::trace::Volume,
    trace_tex: wgpu::Texture,
}

impl State {
    pub async fn new(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        cluster: Cluster,
        link: Link,
        local: bool,
        port: u16,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(display),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| format!("{err}. The client needs a display."))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|err| format!("{err}. The client needs a display and a graphics adapter."))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|err| err.to_string())?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(caps.formats[0]);
        let view_format = surface_format.add_srgb_suffix();

        let (block_view, block_sampler) = upload_block_texture(&device, &queue);
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow-map"),
            size: wgpu::Extent3d {
                width: CASCADE_MAP,
                height: CASCADE_MAP,
                depth_or_array_layers: CASCADE_COUNT as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow-sample"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_array_layer: 0,
            array_layer_count: Some(CASCADE_COUNT as u32),
            aspect: wgpu::TextureAspect::DepthOnly,
            ..Default::default()
        });
        let shadow_layers = std::array::from_fn(|layer| {
            shadow_tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some("shadow-layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer as u32,
                array_layer_count: Some(1),
                aspect: wgpu::TextureAspect::DepthOnly,
                ..Default::default()
            })
        });
        let trace_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sun-trace"),
            size: wgpu::Extent3d {
                width: crate::trace::GRID_X as u32,
                height: crate::trace::GRID_Y as u32,
                depth_or_array_layers: crate::trace::GRID_Z as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let trace_view = trace_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let trace_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sun-trace"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow-compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            // Linear compare is a 2x2 percentage-closer filter per tap.
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chunk-uniforms"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(world_vertex_layout())],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(view_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let shadow_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow-uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow-map"),
                bind_group_layouts: &[Some(&shadow_bind_layout)],
                immediate_size: 0,
            });
        let shadow_pipelines = [0.0, 1.0, 2.0].map(|cascade| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow-map"),
                layout: Some(&shadow_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_shadow"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("CASCADE", cascade)],
                        zero_initialize_workgroup_memory: true,
                    },
                    buffers: &[Some(world_vertex_layout())],
                },
                fragment: None,
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    // True depths. A slope bias here pushes the stored surface
                    // away from the light and the shadow starts late.
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        });
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(world_vertex_layout())],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                // Unlit: lit fs_main tints the white wire green/grass near foliage.
                entry_point: Some("fs_unlit"),
                compilation_options: Default::default(),
                targets: &[Some(view_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                // Line lists reject depth bias. The wire is already expanded
                // off the block, and LessEqual keeps that shell visible.
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let outline_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline"),
            size: OUTLINE_VERTEX_CAP * std::mem::size_of::<mesh::Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky"),
            size: SKY_VERTEX_CAP * std::mem::size_of::<mesh::Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dig_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dig-darken"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(world_vertex_layout())],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::Constant,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: -1,
                    slope_scale: -1.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let dig_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dig-darken"),
            size: DIG_VERTEX_CAP * std::mem::size_of::<mesh::Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hotbar"),
            source: wgpu::ShaderSource::Wgsl(UI_SHADER.into()),
        });
        let ui_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hotbar-textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let ui_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hotbar"),
            bind_group_layouts: &[Some(&ui_bind_layout)],
            immediate_size: 0,
        });
        let ui_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 20,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 28,
                    shader_location: 3,
                },
            ],
        };
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hotbar"),
            layout: Some(&ui_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(ui_vertex_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(view_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let ui_vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hotbar-vertices"),
            size: UI_VERTEX_CAP * std::mem::size_of::<UiVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ui_index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hotbar-indices"),
            size: UI_INDEX_CAP * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ui_tile_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui-tiles"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tile_count = [crate::texture::atlas_tile_count(), 0.0, 0.0, 0.0];
        queue.write_buffer(&ui_tile_buf, 0, bytemuck::cast_slice(&tile_count));
        let ui_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hotbar-textures"),
            layout: &ui_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&block_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&block_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ui_tile_buf.as_entire_binding(),
                },
            ],
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view-proj"),
            size: 512,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chunk-uniforms"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&block_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&block_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&trace_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&trace_sampler),
                },
            ],
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow-uniforms"),
            layout: &shadow_bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let loaded_size = cluster.size();
        let camera = Camera::drop_by_the_stairs();
        let air_peak = camera.eye.y;
        let spawn_focus = chunk_of(camera.eye);
        let spawn_light = crate::daylight::sample(crate::daylight::Clock::noon());
        let spawn_lookups = crate::daylight::shadow_lookups(spawn_light.direction);
        let chunk_bake = mesh::ChunkBake::new(&cluster, spawn_light.direction, spawn_focus);
        let frame = FramePass::load(&device, view_format);

        let terrain = rebuild_world::TerrainPump::new(&cluster);
        let terrain_lane =
            crate::jobs::Lane::new("terralite-terrain", |(epoch, x, z): (u64, i32, i32)| {
                (epoch, rebuild_world::generate_chunk(x, z))
            });
        let mesh_pool = crate::jobs::Pool::new(2, "terralite-mesh", mesh::MeshWork::bake);
        let hooks = load_player_hooks()?;
        let posed_eye = camera.eye;
        let (posed_yaw, posed_pitch) = camera.yaw_pitch();
        let mut hotbar = hotbar::Hotbar::new();
        hotbar.collect("coin");
        let pack = rebuild_world::Pack::builtin();
        hotbar.stock_blocks(&pack.script_block_ids());
        hotbar.stock_items(&pack.placeable_item_ids());
        let egui = crate::egui_ui::EguiHost::new(&window, &device, view_format);
        let mut state = Self {
            instance,
            window,
            device,
            queue,
            size: PhysicalSize::new(0, 0),
            surface,
            surface_format,
            view_format,
            configured: false,
            depth: None,
            pipeline,
            bind_group,
            uniform_buf,
            chunk_draws: Vec::new(),
            outline_pipeline,
            outline_buf,
            dig_pipeline,
            dig_buf,
            dig: crate::dig::Dig::new(),
            break_id: String::new(),
            break_factor: 1.0,
            ui_pipeline,
            ui_bind,
            ui_tile_buf,
            ui_vertex_buf,
            ui_index_buf,
            world: cluster,
            link,
            pending: VecDeque::new(),
            told_disconnect: false,
            camera,
            input: MoveInput {
                forward: false,
                back: false,
                left: false,
                right: false,
                jump: false,
                fast: false,
                crouch: false,
            },
            hotbar,
            bag: hotbar::Bag::new(),
            hotbar_dirty: true,
            bag_dirty: true,
            pose_dirty: true,
            pose_left: 0.0,
            posed_eye,
            posed_yaw,
            posed_pitch,
            port,
            gui: gui::Gui::new(),
            menu: gui::Menu::new(),
            binds: crate::keybinds::Keybinds::load(),
            egui,
            playing: false,
            loaded_world: crate::net::DEFAULT_WORLD.to_string(),
            audio: crate::audio::Audio::new(),
            local,
            quit: false,
            shot_path: None,
            shot_frames: 0,
            settings_shot_path: None,
            settings_shot_frames: 0,
            keybinds_shot_path: None,
            keybinds_shot_frames: 0,
            creative_shot_path: None,
            creative_shot_frames: 0,
            held_light_shot_path: None,
            held_light_shot_frames: 0,
            held_light_shot_phase: HeldLightShotPhase::Idle,
            held_light_shot_dark: None,
            held_light_shot_eye: glam::Vec3::ZERO,
            held_light_shot_yaw: 0.0,
            held_light_shot_pitch: 0.0,
            night_spawn_shot_path: None,
            night_spawn_shot_frames: 0,
            night_spawn_shot_eye: glam::Vec3::ZERO,
            night_spawn_shot_yaw: 0.0,
            night_spawn_shot_pitch: 0.0,
            bed_sleep_shot_path: None,
            bed_sleep_shot_frames: 0,
            bed_sleep_shot_eye: glam::Vec3::ZERO,
            bed_sleep_shot_yaw: 0.0,
            bed_sleep_shot_pitch: 0.0,
            crouch_shot_path: None,
            crouch_shot_frames: 0,
            crouch_shot_eye: glam::Vec3::ZERO,
            crouch_shot_yaw: 0.0,
            crouch_shot_pitch: 0.0,
            sprint_shot_path: None,
            sprint_shot_frames: 0,
            sprint_shot_phase: SprintShotPhase::Idle,
            sprint_shot_walk: None,
            sprint_shot_eye: glam::Vec3::ZERO,
            sprint_shot_yaw: 0.0,
            sprint_shot_pitch: 0.0,
            swim_shot_path: None,
            swim_shot_frames: 0,
            swim_shot_phase: SwimShotPhase::Idle,
            swim_shot_dry: None,
            swim_shot_dry_eye: glam::Vec3::ZERO,
            swim_shot_wet_eye: glam::Vec3::ZERO,
            swim_shot_yaw: 0.0,
            swim_shot_pitch: 0.0,
            boat_shot_path: None,
            boat_shot_frames: 0,
            boat_shot_phase: BoatShotPhase::Idle,
            boat_shot_idle: None,
            boat_shot_eye: glam::Vec3::ZERO,
            boat_shot_yaw: 0.0,
            boat_shot_pitch: 0.0,
            cart_shot_path: None,
            cart_shot_frames: 0,
            cart_shot_phase: CartShotPhase::Idle,
            cart_shot_idle: None,
            cart_shot_eye: glam::Vec3::ZERO,
            cart_shot_yaw: 0.0,
            cart_shot_pitch: 0.0,
            attack_shot_path: None,
            attack_shot_frames: 0,
            attack_shot_phase: AttackShotPhase::Idle,
            attack_shot_cool: None,
            attack_shot_eye: glam::Vec3::ZERO,
            attack_shot_yaw: 0.0,
            attack_shot_pitch: 0.0,
            hunger_shot_path: None,
            hunger_shot_frames: 0,
            hunger_shot_phase: HungerShotPhase::Idle,
            hunger_shot_fed: None,
            hunger_shot_eye: glam::Vec3::ZERO,
            hunger_shot_yaw: 0.0,
            hunger_shot_pitch: 0.0,
            eat_shot_path: None,
            eat_shot_frames: 0,
            eat_shot_phase: EatShotPhase::Idle,
            eat_shot_idle: None,
            eat_shot_eye: glam::Vec3::ZERO,
            eat_shot_yaw: 0.0,
            eat_shot_pitch: 0.0,
            jump_shot_path: None,
            jump_shot_frames: 0,
            jump_shot_phase: JumpShotPhase::Idle,
            jump_shot_ground: None,
            jump_shot_eye: glam::Vec3::ZERO,
            jump_shot_yaw: 0.0,
            jump_shot_pitch: 0.0,
            place_shot_path: None,
            place_shot_frames: 0,
            place_shot_phase: PlaceShotPhase::Idle,
            place_shot_idle: None,
            place_shot_eye: glam::Vec3::ZERO,
            place_shot_yaw: 0.0,
            place_shot_pitch: 0.0,
            swap_shot_path: None,
            swap_shot_frames: 0,
            swap_shot_phase: SwapShotPhase::Idle,
            swap_shot_idle: None,
            swap_shot_eye: glam::Vec3::ZERO,
            swap_shot_yaw: 0.0,
            swap_shot_pitch: 0.0,
            break_shot_path: None,
            break_shot_frames: 0,
            break_shot_phase: BreakShotPhase::Idle,
            break_shot_idle: None,
            break_shot_eye: glam::Vec3::ZERO,
            break_shot_yaw: 0.0,
            break_shot_pitch: 0.0,
            hooks,
            entity_time: 0.0,
            air_peak,
            airborne: false,
            jump_asked: false,
            jump_blocked: false,
            sprint_asked: false,
            sprint_blocked: false,
            slot_punch: 0.0,
            swap_kick: 0.0,
            break_kick: 0.0,
            bob_phase: 0.0,
            bob_amount: 0.0,
            crouch: 0.0,
            sprint_kick: 0.0,
            sprint_run: 0.0,
            sprint_show: 0.0,
            boat_kick: 0.0,
            boat_show: 0.0,
            boat_stroke: 0.0,
            cart_kick: 0.0,
            cart_show: 0.0,
            cart_stroke: 0.0,
            horse_kick: 0.0,
            horse_show: 0.0,
            horse_stroke: 0.0,
            horse_shot_path: None,
            horse_shot_frames: 0,
            horse_shot_phase: HorseShotPhase::Idle,
            horse_shot_idle: None,
            horse_shot_eye: glam::Vec3::ZERO,
            horse_shot_yaw: 0.0,
            horse_shot_pitch: 0.0,
            hit_kick: 0.0,
            hurt_flash: 0.0,
            sip: 0.0,
            sipping: false,
            chew_phase: 0.0,
            burp_due: false,
            dead: false,
            shape_pick: crate::shape_pick::ShapePick::new(),
            plate_held: None,
            plate_close: None,
            lever_follow: None,
            bell_return: None,
            lodestone: None,
            zoom_mouse: false,
            ctrl_mod: false,
            place_click_at: None,
            compound_flow_wait: 0.35,
            lamp_drain_wait: 0,
            gen_charge_wait: 0,
            compound_flow_told: false,
            map_open: false,
            zoom_key: false,
            horn_wait: 0.0,
            was_wet: false,
            water_sub: 0.0,
            splash_kick: 0.0,
            jump_kick: 0.0,
            rain_mix: 0.0,
            rain_told: false,
            found_camp: false,
            found_farm: false,
            found_scarecrow: false,
            found_shrine: false,
            found_mine: false,
            found_rails: false,
            found_clay: false,
            found_compound: false,
            found_fireflies: false,
            found_wolf: false,
            found_cat: false,
            found_parrot: false,
            found_horse: false,
            told_cook_heat: false,
            told_blast_heat: false,
            told_furnace_heat: false,
            told_shape_pot: false,
            told_pump: false,
            told_tank: false,
            told_battery: false,
            told_lamp: false,
            told_spout: false,
            told_drain: false,
            told_generator: false,
            drip_mix: 0.0,
            rustle_mix: 0.0,
            cricket_mix: 0.0,
            bird_mix: 0.0,
            crackle_mix: 0.0,
            crackle_at: None,
            smoke_wait: 0.0,
            firefly_wait: 0.0,
            campfire_smoke_shot_path: None,
            campfire_smoke_shot_frames: 0,
            campfire_smoke_shot_eye: glam::Vec3::ZERO,
            campfire_smoke_shot_yaw: 0.0,
            campfire_smoke_shot_pitch: 0.0,
            cook_shot_path: None,
            cook_shot_frames: 0,
            cook_shot_eye: glam::Vec3::ZERO,
            cook_shot_yaw: 0.0,
            cook_shot_pitch: 0.0,
            note_shot_at: None,
            note_shot_pitch: 0,
            note_shot_book: false,
            note_shot_brew: false,
            shrine_shot_path: None,
            shrine_shot_frames: 0,
            shrine_shot_eye: glam::Vec3::ZERO,
            shrine_shot_yaw: 0.0,
            shrine_shot_pitch: 0.0,
            mine_shot_path: None,
            mine_shot_frames: 0,
            mine_shot_eye: glam::Vec3::ZERO,
            mine_shot_yaw: 0.0,
            mine_shot_pitch: 0.0,
            wind_mix: 0.0,
            lap_mix: 0.0,
            lap_at: None,
            mace_wait: 0.0,
            omen: false,
            trial_at: None,
            trial_due: false,
            wave_breeze: false,
            trial_flip: false,
            breeze_due: false,
            slime_due: false,
            horn_due: false,
            golem_shut: 0.0,
            spawn_at: None,
            sleep_fade: 0.0,
            rod_strike: None,
            fish_bobber: None,
            target_hit: None,
            frame,
            drops: crate::drops::Drops::new(),
            drop_buf: None,
            drop_count: 0,
            sheep: crate::sheep::Sheep::new(),
            sheep_buf: None,
            sheep_count: 0,
            pig: crate::mobs::Pig::new(),
            pig_buf: None,
            pig_count: 0,
            horse: crate::mobs::Horse::new(),
            horse_buf: None,
            horse_count: 0,
            zombie: crate::mobs::Zombie::new(),
            night: crate::mobs::NightHorde::new(),
            zombie_buf: None,
            zombie_count: 0,
            breeze: crate::mobs::Breeze::new(),
            breeze_buf: None,
            breeze_count: 0,
            golem: crate::mobs::Golem::new(),
            golem_buf: None,
            golem_count: 0,
            wolf: crate::mobs::Wolf::new(),
            wolf_buf: None,
            wolf_count: 0,
            cat: crate::mobs::Cat::new(),
            cat_buf: None,
            cat_count: 0,
            parrot: crate::mobs::Parrot::new(),
            parrot_buf: None,
            parrot_count: 0,
            frog: crate::mobs::Frog::new(),
            frog_buf: None,
            frog_count: 0,
            axolotl: crate::mobs::Axolotl::new(),
            axolotl_buf: None,
            axolotl_count: 0,
            chicken: crate::mobs::Chicken::new(),
            chicken_buf: None,
            chicken_count: 0,
            bee: crate::mobs::Bee::new(),
            bee_buf: None,
            bee_count: 0,
            fox: crate::mobs::Fox::new(),
            fox_buf: None,
            fox_count: 0,
            goat: crate::mobs::Goat::new(),
            goat_buf: None,
            goat_count: 0,
            guests: crate::mobs::Guest::placed_defs(
                &rebuild_world::Pack::builtin().script_entity_defs(),
                &rebuild_world::Pack::builtin().script_spawns(),
            ),
            guest_buf: None,
            guest_count: 0,
            boat: crate::boat::Boat::new(),
            boat_buf: None,
            boat_count: 0,
            cart: crate::minecart::Minecart::new(),
            cart_buf: None,
            cart_count: 0,
            others: Vec::new(),
            peer_buf: None,
            peer_count: 0,
            player: crate::player::Pose::new(),
            player_buf: None,
            player_count: 0,
            third_person: false,
            hearts: body::HEARTS,
            cactus_left: 0.0,
            magma_left: 0.0,
            cake_eat: None,
            egg_hop: None,
            crystal_blast: None,
            hunger: body::Hunger::full(),
            armor: body::Armor::new(),
            chest_at: None,
            chest_dirty: false,
            captured: false,
            cursor_x: 0.0,
            cursor_y: 0.0,
            clock: crate::daylight::Clock::noon(),
            shadow_lookups: spawn_lookups.clone(),
            wanted_lookups: spawn_lookups,
            wanted_sun: spawn_light.direction,
            wanted_focus: spawn_focus,
            chunk_bake,
            terrain,
            terrain_dirty: false,
            terrain_quiet: 0,
            job_epoch: 1,
            terrain_lane,
            mesh_pool,
            sky_buf,
            sky_label: String::new(),
            shadow_tex,
            shadow_view,
            shadow_layers,
            shadow_pipelines,
            shadow_bind,
            trace: crate::trace::Volume::new(),
            trace_tex,
        };
        if !local {
            state.menu.lock_world_size(loaded_size);
        }
        state.bake_spawn();
        state.rebuild_sheep();
        state.rebuild_pig();
        state.rebuild_horse();
        state.rebuild_zombie();
        state.rebuild_breeze();
        state.rebuild_golem();
        state.rebuild_wolf();
        state.rebuild_cat();
        state.rebuild_parrot();
        state.rebuild_frog();
        state.rebuild_axolotl();
        state.rebuild_chicken();
        state.rebuild_bee();
        state.rebuild_fox();
        state.rebuild_goat();
        state.rebuild_guests();
        state.rebuild_boat();
        state.rebuild_minecart();
        state.note_sky();
        state.resize(state.window.inner_size());
        state.load_bag(&crate::net::world_file(&state.menu.world_name()));
        state.load_hotbar_and_place(&crate::net::world_file(&state.menu.world_name()));
        state.flush_hotbar();
        state.flush_bag();
        Ok(state)
    }

    /// Play on a local world opens the size and named save chosen in the menu.
    fn apply_chosen_size(&mut self) {
        let size = self.menu.world_size();
        if !self.local {
            if size == self.world.size() {
                return;
            }
            let mut cluster = rebuild_world::synthetic_cluster_sized(size);
            rebuild_world::seed_terrain_ring(&mut cluster);
            let path = crate::net::world_file(crate::net::DEFAULT_WORLD);
            if let Ok(changes) = rebuild_world::read_changes(&path) {
                apply_changes(&mut cluster, &changes);
            }
            let _ = self.link.send(rebuild_world::encode_world_size(size));
            self.install_cluster(cluster);
            return;
        }
        let name = self.menu.world_name().to_string();
        if size == self.world.size() && name == self.loaded_world {
            return;
        }
        let Ok(cluster) = crate::net::load_named(size, &name) else {
            return;
        };
        let _ = self.link.send(rebuild_world::encode_world_size(size));
        let _ = self.link.send(rebuild_world::encode_save(&name));
        self.loaded_world = name.clone();
        self.install_cluster(cluster);
        let path = crate::net::world_file(&name);
        self.load_bag(&path);
        self.load_hotbar_and_place(&path);
    }

    fn load_bag(&mut self, path: &Path) {
        self.bag = hotbar::Bag::new();
        match rebuild_world::read_inventory(path) {
            Ok(slots) => self.bag.install(&slots),
            Err(err) => eprintln!("inventory: {err}"),
        }
        self.bag_dirty = true;
    }

    fn load_hotbar_and_place(&mut self, path: &Path) {
        match rebuild_world::read_hotbar(path) {
            Ok(Some(saved)) => {
                self.hotbar.install_saved(saved.selected, &saved.slots);
                self.hotbar_dirty = true;
            }
            Ok(None) => {}
            Err(err) => eprintln!("hotbar: {err}"),
        }
        match rebuild_world::read_player(path) {
            Ok(Some(place)) => {
                self.camera.restore(
                    glam::Vec3::new(place.x, place.y, place.z),
                    place.yaw,
                    place.pitch,
                );
                let (min_x, max_x, min_z, max_z) = self.world.bounds();
                self.camera.eye.x = self
                    .camera
                    .eye
                    .x
                    .clamp(min_x as f32 + 1.0, max_x as f32 - 1.0);
                self.camera.eye.z = self
                    .camera
                    .eye
                    .z
                    .clamp(min_z as f32 + 1.0, max_z as f32 - 1.0);
                let (yaw, pitch) = self.camera.yaw_pitch();
                self.posed_eye = self.camera.eye;
                self.posed_yaw = yaw;
                self.posed_pitch = pitch;
                self.pose_dirty = true;
                self.pose_left = 0.0;
            }
            Ok(None) => {}
            Err(err) => eprintln!("player: {err}"),
        }
    }

    fn join_localhost(&mut self) {
        if !self.local {
            self.playing = true;
            self.menu.close_menu();
            return;
        }
        match crate::net::connect(self.port) {
            Ok((cluster, link)) => {
                self.link = link;
                self.local = false;
                self.menu.lock_world_size(cluster.size());
                self.menu.note_join("");
                self.loaded_world = crate::net::DEFAULT_WORLD.to_string();
                self.install_cluster(cluster);
                self.load_bag(&crate::net::world_file(crate::net::DEFAULT_WORLD));
                self.load_hotbar_and_place(&crate::net::world_file(crate::net::DEFAULT_WORLD));
                self.playing = true;
                self.menu.close_menu();
            }
            Err(_) => self.menu.note_join("no server"),
        }
    }

    fn install_cluster(&mut self, cluster: Cluster) {
        self.job_epoch = self.job_epoch.wrapping_add(1);
        self.world = cluster;
        self.terrain = rebuild_world::TerrainPump::new(&self.world);
        self.terrain_dirty = false;
        self.terrain_quiet = 0;
        self.chunk_draws.clear();
        let (min_x, max_x, min_z, max_z) = self.world.bounds();
        self.camera.eye.x = self
            .camera
            .eye
            .x
            .clamp(min_x as f32 + 1.0, max_x as f32 - 1.0);
        self.camera.eye.z = self
            .camera
            .eye
            .z
            .clamp(min_z as f32 + 1.0, max_z as f32 - 1.0);
        let focus = chunk_of(self.camera.eye);
        self.wanted_focus = focus;
        self.chunk_bake = mesh::ChunkBake::new(&self.world, self.wanted_sun, focus);
        self.bake_spawn();
    }

    /// The demo and the seed ring. Workers mesh them. This call does not bake.
    fn bake_spawn(&mut self) {
        self.chunk_bake.queue_unmeshed(&self.world);
        self.dispatch_meshes();
    }

    /// Frames to wait after the pump goes idle before new ground is meshed.
    const TERRAIN_MESH_WAIT: u8 = 6;

    /// Apply finished terrain, then start the next chunk on the worker.
    fn service_terrain(&mut self) -> bool {
        let mut wrote = false;
        while let Some((epoch, chunk)) = self.terrain_lane.poll() {
            if epoch != self.job_epoch {
                continue;
            }
            let x = chunk.x();
            let z = chunk.z();
            if self
                .world
                .get(x, z)
                .is_some_and(|have| have.non_air_count() == 0)
                && self.world.replace_chunk(chunk).is_ok()
            {
                wrote = true;
            }
        }
        if !self.terrain_lane.busy() {
            if let Some((x, z)) = self.terrain.pop() {
                let epoch = self.job_epoch;
                self.terrain_lane.submit((epoch, x, z));
            }
        }
        wrote
    }

    /// Upload finished meshes and hand the next snapshots to idle workers.
    /// New ground is queued only after terrain has been idle. An edit is
    /// already on the queue, so it still goes out while terrain is running.
    fn service_meshes(&mut self) {
        while let Some(done) = self.mesh_pool.poll() {
            if done.job_epoch != self.job_epoch {
                continue;
            }
            if let Some(chunk) = self.chunk_bake.accept(done.epoch, done.baked) {
                self.install_chunks(vec![chunk]);
            }
        }
        let settled =
            self.terrain.remaining() == 0 && !self.terrain_lane.busy() && self.terrain_quiet == 0;
        if settled {
            if self.terrain_dirty {
                self.terrain_dirty = false;
                self.chunk_bake.queue_unmeshed(&self.world);
            }
            let focus_moved = self.wanted_focus != self.chunk_bake.focus();
            let sun_moved = self.wanted_lookups != self.shadow_lookups;
            if focus_moved || sun_moved {
                self.chunk_bake
                    .retarget(&self.world, self.wanted_sun, self.wanted_focus);
                self.shadow_lookups = self.wanted_lookups.clone();
            }
        }
        self.dispatch_meshes();
    }

    fn dispatch_meshes(&mut self) {
        while self.mesh_pool.has_idle() {
            let ready = self.chunk_bake.drain_ready();
            self.install_chunks(ready);
            let Some(mut work) = self.chunk_bake.take_work(&self.world) else {
                break;
            };
            work.job_epoch = self.job_epoch;
            if !self.mesh_pool.submit(work) {
                break;
            }
        }
        let ready = self.chunk_bake.drain_ready();
        self.install_chunks(ready);
    }

    fn install_chunks(&mut self, baked: Vec<mesh::BakedChunk>) {
        for chunk in baked {
            self.chunk_draws
                .retain(|draw| draw.x != chunk.x || draw.z != chunk.z);
            if chunk.mesh.indices.is_empty() {
                continue;
            }
            self.chunk_draws.push(ChunkGpu {
                x: chunk.x,
                z: chunk.z,
                vertices: self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("chunk-vertices"),
                        contents: bytemuck::cast_slice(&chunk.mesh.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                indices: self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("chunk-indices"),
                        contents: bytemuck::cast_slice(&chunk.mesh.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                count: chunk.mesh.indices.len() as u32,
            });
        }
    }

    fn draw_chunks<'a>(chunks: &'a [ChunkGpu], pass: &mut wgpu::RenderPass<'a>) {
        for chunk in chunks {
            pass.set_vertex_buffer(0, chunk.vertices.slice(..));
            pass.set_index_buffer(chunk.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..chunk.count, 0, 0..1);
        }
    }

    fn sun_light(&self) -> crate::daylight::Light {
        crate::daylight::sample_at(self.clock, self.menu.hemisphere())
    }

    fn note_sky(&mut self) {
        let light = self.sun_light();
        let fog = if crate::daylight::rain_mood(self.clock.time_of_day()) > 0.5 {
            ", rain"
        } else {
            ""
        };
        let label = if self.menu.hemisphere() == crate::daylight::Hemisphere::North {
            format!(
                "Terralite — {}, {}, north{fog}",
                light.season.label(),
                light.phase
            )
        } else {
            format!("Terralite — {}, {}{fog}", light.season.label(), light.phase)
        };
        if label != self.sky_label {
            self.sky_label = label;
            self.window.set_title(&self.sky_label);
        }
    }

    pub fn is_captured(&self) -> bool {
        self.captured
    }

    pub fn set_breaking(&mut self, held: bool) {
        if !held {
            self.dig.set_held(false);
            return;
        }
        if self.try_script_attack() || self.hop_egg() || self.blast_crystal() {
            self.dig.set_held(false);
            return;
        }
        self.dig.set_held(true);
    }

    /// Route a mouse button through remaps / keybind listening.
    ///
    /// Place/use fires on the bound place button (default Right), on Ctrl+Attack
    /// (trackpad / one-button secondary click), and after a capture grab so the
    /// first secondary click still places.
    pub fn set_mouse(&mut self, button: winit::event::MouseButton, pressed: bool) {
        use crate::keybinds::Action;
        if pressed && self.binds.listening.is_some() && self.menu.is_keybinds() {
            let _ = self.binds.hear_mouse(button);
            return;
        }
        if self.binds.is_place_press(button, self.ctrl_mod) {
            self.press_place(pressed);
            return;
        }
        if self.binds.is_mouse(Action::Attack, button) {
            if !pressed {
                self.set_breaking(false);
                return;
            }
            if self.is_dead() {
                self.pointer_click();
            } else if self.screen_open() || self.menu_open() {
                self.pointer_click();
            } else if self.is_captured() {
                self.set_breaking(true);
            } else {
                self.set_capture(true);
            }
        }
    }

    /// Place / use from a mouse or raw device secondary click.
    fn press_place(&mut self, pressed: bool) {
        if !pressed {
            self.release_spyglass();
            return;
        }
        if self.is_dead() || self.screen_open() || self.menu_open() {
            return;
        }
        if !self.is_captured() {
            self.set_capture(true);
        }
        // Window right-click and DeviceEvent button 1 often arrive as a pair.
        let now = Instant::now();
        if self
            .place_click_at
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(80))
        {
            return;
        }
        self.place_click_at = Some(now);
        self.place_targeted();
    }

    /// A left click asks `events.attack` before mining, an egg hop, or a crystal blast.
    ///
    /// False consumes the click. `target` is the aimed mob, or the looked-at block
    /// when no mob is on the ray. A consumed click sends the first `world.set_block`.
    fn try_script_attack(&mut self) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let item = self
            .hotbar
            .slot(self.hotbar.selected())
            .unwrap_or("")
            .to_string();
        let target = self.attack_target();
        if self.hooks.attack(&mut self.world, &item, &target, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// The mob on the look ray, or the solid block behind it.
    fn attack_target(&self) -> String {
        if let Some(kind) = self.aimed_kind() {
            return kind.to_string();
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return String::new();
        };
        let id = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id;
        if id.is_empty() || id == "air" {
            String::new()
        } else {
            id.to_string()
        }
    }

    fn break_block_at(&mut self, cell: [i32; 3]) {
        let slot = self.hotbar.selected();
        self.send_edit(
            encode_break(cell[0], cell[1], cell[2]),
            TAG_BREAK,
            Some(slot),
        );
    }

    fn rebuild_drops(&mut self) {
        let eye = self.camera.eye.to_array();
        let vertices = self.drops.vertices_toward(eye);
        if vertices.is_empty() {
            self.drop_buf = None;
            self.drop_count = 0;
            return;
        }
        let bytes = bytemuck::cast_slice(&vertices);
        let count = vertices.len() as u32;
        if let Some(buf) = &self.drop_buf {
            if buf.size() >= bytes.len() as u64 {
                self.queue.write_buffer(buf, 0, bytes);
                self.drop_count = count;
                return;
            }
        }
        self.drop_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("drops"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
        );
        self.drop_count = count;
    }

    fn rebuild_sheep(&mut self) {
        let vertices = self.sheep.vertices();
        if vertices.is_empty() {
            self.sheep_buf = None;
            self.sheep_count = 0;
            return;
        }
        self.sheep_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("sheep"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.sheep_count = vertices.len() as u32;
    }

    fn rebuild_pig(&mut self) {
        let vertices = self.pig.vertices();
        if vertices.is_empty() {
            self.pig_buf = None;
            self.pig_count = 0;
            return;
        }
        self.pig_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("pig"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.pig_count = vertices.len() as u32;
    }

    fn rebuild_horse(&mut self) {
        let vertices = self.horse.vertices();
        if vertices.is_empty() {
            self.horse_buf = None;
            self.horse_count = 0;
            return;
        }
        self.horse_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("horse"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.horse_count = vertices.len() as u32;
    }

    fn rebuild_zombie(&mut self) {
        let mut vertices = self.zombie.vertices();
        vertices.extend(self.night.vertices());
        if vertices.is_empty() {
            self.zombie_buf = None;
            self.zombie_count = 0;
            return;
        }
        self.zombie_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("zombie"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.zombie_count = vertices.len() as u32;
    }

    fn held_emitter(&self) -> Option<Emitter> {
        let pack = Pack::builtin();
        self.hotbar.slot(self.hotbar.selected()).and_then(|id| {
            let light = held_light_of(&pack, id)?;
            let [x, y, z] = eye_cell(self.camera.eye);
            Some(Emitter { x, y, z, light })
        })
    }

    fn rebuild_breeze(&mut self) {
        let vertices = self.breeze.vertices();
        if vertices.is_empty() {
            self.breeze_buf = None;
            self.breeze_count = 0;
            return;
        }
        self.breeze_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("breeze"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.breeze_count = vertices.len() as u32;
    }

    fn rebuild_golem(&mut self) {
        let vertices = self.golem.vertices();
        if vertices.is_empty() {
            self.golem_buf = None;
            self.golem_count = 0;
            return;
        }
        self.golem_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("golem"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.golem_count = vertices.len() as u32;
    }

    fn rebuild_wolf(&mut self) {
        let vertices = self.wolf.vertices();
        if vertices.is_empty() {
            self.wolf_buf = None;
            self.wolf_count = 0;
            return;
        }
        self.wolf_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("wolf"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.wolf_count = vertices.len() as u32;
    }

    fn rebuild_cat(&mut self) {
        let vertices = self.cat.vertices();
        if vertices.is_empty() {
            self.cat_buf = None;
            self.cat_count = 0;
            return;
        }
        self.cat_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cat"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.cat_count = vertices.len() as u32;
    }

    fn rebuild_parrot(&mut self) {
        let vertices = self.parrot.vertices();
        if vertices.is_empty() {
            self.parrot_buf = None;
            self.parrot_count = 0;
            return;
        }
        self.parrot_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("parrot"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.parrot_count = vertices.len() as u32;
    }

    fn rebuild_frog(&mut self) {
        let vertices = self.frog.vertices();
        if vertices.is_empty() {
            self.frog_buf = None;
            self.frog_count = 0;
            return;
        }
        self.frog_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("frog"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.frog_count = vertices.len() as u32;
    }

    fn rebuild_axolotl(&mut self) {
        let vertices = self.axolotl.vertices();
        if vertices.is_empty() {
            self.axolotl_buf = None;
            self.axolotl_count = 0;
            return;
        }
        self.axolotl_buf = Some(self.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("axolotl"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        self.axolotl_count = vertices.len() as u32;
    }

    fn rebuild_chicken(&mut self) {
        let vertices = self.chicken.vertices();
        if vertices.is_empty() {
            self.chicken_buf = None;
            self.chicken_count = 0;
            return;
        }
        self.chicken_buf = Some(self.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("chicken"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        self.chicken_count = vertices.len() as u32;
    }

    fn rebuild_bee(&mut self) {
        let vertices = self.bee.vertices();
        if vertices.is_empty() {
            self.bee_buf = None;
            self.bee_count = 0;
            return;
        }
        self.bee_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bee"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.bee_count = vertices.len() as u32;
    }

    fn rebuild_fox(&mut self) {
        let vertices = self.fox.vertices();
        if vertices.is_empty() {
            self.fox_buf = None;
            self.fox_count = 0;
            return;
        }
        self.fox_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("fox"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.fox_count = vertices.len() as u32;
    }

    fn rebuild_guests(&mut self) {
        let vertices: Vec<crate::mesh::Vertex> = self
            .guests
            .iter()
            .flat_map(|guest| guest.vertices())
            .collect();
        if vertices.is_empty() {
            self.guest_buf = None;
            self.guest_count = 0;
            return;
        }
        self.guest_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("guests"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.guest_count = vertices.len() as u32;
    }

    fn rebuild_goat(&mut self) {
        let vertices = self.goat.vertices();
        if vertices.is_empty() {
            self.goat_buf = None;
            self.goat_count = 0;
            return;
        }
        self.goat_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("goat"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.goat_count = vertices.len() as u32;
    }

    fn rebuild_boat(&mut self) {
        let vertices = self.boat.vertices();
        if vertices.is_empty() {
            self.boat_buf = None;
            self.boat_count = 0;
            return;
        }
        self.boat_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("boat"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.boat_count = vertices.len() as u32;
    }

    fn rebuild_minecart(&mut self) {
        let vertices = self.cart.vertices();
        if vertices.is_empty() {
            self.cart_buf = None;
            self.cart_count = 0;
            return;
        }
        self.cart_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("minecart"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.cart_count = vertices.len() as u32;
    }

    fn rebuild_player(&mut self) {
        if !self.third_person || !self.playing || self.dead {
            self.player_buf = None;
            self.player_count = 0;
            return;
        }
        let eye = self.camera.eye.to_array();
        let vertices = self.player.vertices(eye, false);
        if vertices.is_empty() {
            self.player_buf = None;
            self.player_count = 0;
            return;
        }
        self.player_buf = Some(self.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("player"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        self.player_count = vertices.len() as u32;
    }

    fn rebuild_peers(&mut self) {
        let vertices = crate::peers::mesh(&self.others);
        if vertices.is_empty() {
            self.peer_buf = None;
            self.peer_count = 0;
            return;
        }
        self.peer_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("peers"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.peer_count = vertices.len() as u32;
    }

    fn hotbar_digit(code: KeyCode) -> Option<u8> {
        match code {
            KeyCode::Digit1 => Some(1),
            KeyCode::Digit2 => Some(2),
            KeyCode::Digit3 => Some(3),
            KeyCode::Digit4 => Some(4),
            KeyCode::Digit5 => Some(5),
            KeyCode::Digit6 => Some(6),
            KeyCode::Digit7 => Some(7),
            KeyCode::Digit8 => Some(8),
            KeyCode::Digit9 => Some(9),
            KeyCode::Digit0 => Some(10),
            _ => None,
        }
    }

    fn holding_mace(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_mace(id))
    }

    /// Swing the mace. True when the selected slot is one.
    fn try_mace(&mut self) -> bool {
        if !self.holding_mace() {
            return false;
        }
        self.swing_mace();
        true
    }

    fn swing_mace(&mut self) {
        let Some(wait) = body::mace_ready(self.mace_wait) else {
            self.gui.push_chat("wait");
            return;
        };
        let slot = self.hotbar.selected();
        if self.hotbar.count(slot) == 0 {
            self.gui.push_chat("empty");
            return;
        }
        self.mace_wait = wait;
        self.hit_kick = self.hit_kick.max(0.55);
        let falling = !self.camera.on_ground && self.camera.velocity_y < 0.0;
        let smash = Pack::builtin().mace_smash(self.hotbar.slot(slot).unwrap_or(""));
        let eye = self.camera.eye.to_array();
        let forward = self.camera.forward().to_array();
        let zombie_aimed = self.zombie.aimed(eye, forward);
        let night_index = if zombie_aimed {
            None
        } else {
            self.night.aimed(eye, forward)
        };
        let breeze_aimed =
            !zombie_aimed && night_index.is_none() && self.breeze.aimed(eye, forward);
        let guest_index = if zombie_aimed || night_index.is_some() || breeze_aimed {
            None
        } else {
            self.aimed_guest(eye, forward)
        };
        let amount = body::mace_hearts(falling, smash);
        let dropped_zombie = zombie_aimed && {
            let (min, max) = self.zombie.bounds();
            let origin = [
                (min[0] + max[0]) * 0.5,
                min[1],
                (min[2] + max[2]) * 0.5,
            ];
            self.hurt_mob("zombie", amount, origin, |s, n| s.zombie.hurt(n))
        };
        let dropped_night = if let Some(index) = night_index {
            let origin = self
                .night
                .iter()
                .nth(index)
                .map(crate::mobs::Zombie::origin)
                .unwrap_or([0.0; 3]);
            self.hurt_mob("zombie", amount, origin, |s, n| {
                s.night.get_mut(index).is_some_and(|z| z.hurt(n))
            })
        } else {
            false
        };
        let dropped_breeze = breeze_aimed && {
            let origin = self.breeze.origin();
            self.hurt_mob("breeze", amount, origin, |s, n| s.breeze.hurt(n))
        };
        let dropped_guest = if let Some(index) = guest_index {
            let id = self.guests[index].id().to_string();
            let origin = self.guests[index].origin();
            let dropped = self.hurt_mob(&id, amount, origin, |s, n| s.guests[index].hurt(n));
            if dropped {
                self.guests.remove(index);
            }
            self.rebuild_guests();
            dropped
        } else {
            false
        };
        let aimed = zombie_aimed || night_index.is_some() || breeze_aimed || guest_index.is_some();
        let dropped = dropped_zombie || dropped_night || dropped_breeze || dropped_guest;
        let word = if !aimed {
            "swing"
        } else if dropped {
            "down"
        } else if falling {
            "smash"
        } else {
            "hit"
        };
        if !self.hotbar.wear_use(slot) {
            self.gui.push_chat("empty");
            return;
        }
        self.hotbar_dirty = true;
        let source = self.camera.eye.to_array();
        self.play_world(crate::audio::Sound::Mace, source);
        self.gui.push_chat(word);
        if dropped_zombie && !self.wave_breeze {
            if let Some(at) = self.trial_at {
                let prize = self.pay_trial(at);
                self.gui.push_chat(prize);
            }
        }
        if dropped_breeze {
            self.grant_rod();
            if self.wave_breeze {
                if let Some(at) = self.trial_at {
                    let prize = self.pay_trial(at);
                    self.gui.push_chat(prize);
                }
            }
        }
        if self.hotbar.slot(slot).is_none() {
            self.gui.push_chat("the mace breaks");
        }
        self.rebuild_zombie();
        self.rebuild_breeze();
    }

    fn holding_horn(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_horn(id))
    }

    /// Play the horn call. True when the selected slot is a horn.
    fn try_horn(&mut self) -> bool {
        if !self.holding_horn() {
            return false;
        }
        self.blow_horn();
        true
    }

    fn blow_horn(&mut self) {
        let Some(wait) = crate::audio::horn_blow(self.horn_wait) else {
            self.gui.push_chat("wait");
            return;
        };
        self.horn_wait = wait;
        let source = self.camera.eye.to_array();
        self.play_world(crate::audio::Sound::Horn, source);
        self.tell_parrot(crate::mobs::Mimic::Horn, source);
        self.gui.push_chat("horn");
    }

    fn holding_brush(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_brush(id))
    }

    /// Sweep the looked-at soft block. True when a brush is aimed at one.
    fn try_brush(&mut self) -> bool {
        if !self.holding_brush() {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if !brushable(block.id) {
            return false;
        }
        if already_brushed(block.id, block.metadata) || !self.pending.is_empty() {
            self.gui.push_chat("empty");
            return true;
        }
        let find = brush_find(hit.block[0], hit.block[2]);
        let slot = self.hotbar.selected();
        let opens = self.hotbar.count(slot) <= 1;
        if !self.hotbar.room_for(find) && !opens && !self.bag.room_for(find) {
            self.gui.push_chat("full");
            return true;
        }
        if !self.hotbar.wear_use(slot) {
            self.gui.push_chat("empty");
            return true;
        }
        if !self.hotbar.collect(find) {
            self.bag.collect(find);
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.mark_brushed(hit.block, block.id, block.metadata);
        self.gui.push_chat(&find.replace('_', " "));
        if self.hotbar.slot(slot).is_none() {
            self.gui.push_chat("the brush breaks");
        }
        true
    }

    fn mark_brushed(&mut self, at: [i32; 3], id: &str, metadata: u8) {
        let want = brush_mark(id, metadata);
        if metadata == want || !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new(id, want),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    fn holding_spyglass(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_spyglass(id))
    }

    fn spyglass_zoomed(&self) -> bool {
        self.playing
            && !self.menu.is_open()
            && !self.gui.is_open()
            && self.holding_spyglass()
            && (self.zoom_mouse || self.zoom_key)
    }

    pub fn release_spyglass(&mut self) {
        self.zoom_mouse = false;
    }

    fn holding_compass(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_compass(id))
    }

    fn holding_clock(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_clock(id))
    }

    fn holding_map(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_map(id))
    }

    fn looking_at_use(&self) -> bool {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let id = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id;
        matches!(
            id,
            "bed"
                | "crafting_table"
                | "furnace"
                | "door"
                | "copper_door"
                | "trapdoor"
                | "copper_trapdoor"
                | "fence_gate"
                | "button"
                | "lever"
                | "note_block"
                | "jukebox"
                | "flower_pot"
                | "end_portal_frame"
                | "dragon_egg"
                | "cake"
                | "pumpkin"
                | "anvil"
                | "enchanting_table"
                | "brewing_stand"
                | "grindstone"
                | "smithing_table"
                | "loom"
                | "cartography_table"
                | "lectern"
                | "chiseled_bookshelf"
                | "hanging_sign"
                | "item_frame"
                | "composter"
                | "stonecutter"
                | "campfire"
                | "fletching_table"
                | "smoker"
                | "blast_furnace"
                | "beehive"
                | "cauldron"
                | "bell"
                | "candle"
                | "lodestone"
                | "respawn_anchor"
                | "lightning_rod"
                | "target"
                | "trial_spawner"
                | "copper_bulb"
                | "pump"
        ) || Pack::builtin().is_block_entity(id)
    }

    fn open_looked_block(&mut self) {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id == "bed" {
            self.sleep_in_bed(hit.block);
            return;
        }
        if block.id == "pump" {
            self.toggle_pump(hit.block, block.metadata);
            return;
        }
        if block.id == "spout" {
            self.toggle_spout(hit.block, block.metadata);
            return;
        }
        if block.id == "drain" {
            self.toggle_drain(hit.block, block.metadata);
            return;
        }
        if block.id == "generator" {
            self.toggle_generator(hit.block, block.metadata);
            return;
        }
        if hinge(block.id) {
            self.toggle_swing(block.id, hit.block, block.metadata);
            return;
        }
        if block.id == "button" {
            self.gui.push_chat("click");
            if let Some((at, other)) =
                press_target(&self.world, [hit.block[0], hit.block[1], hit.block[2]])
            {
                self.toggle_swing(other.id, at, other.metadata);
            }
            return;
        }
        if block.id == "lever" {
            self.flip_lever(hit.block, block.metadata);
            return;
        }
        if block.id == "note_block" {
            self.play_note_block(hit.block, block.metadata);
            return;
        }
        if block.id == "jukebox" {
            self.use_jukebox(hit.block, block.metadata);
            return;
        }
        if block.id == "item_frame" {
            let word = self.display_frame(hit.block);
            self.gui.push_chat(&word);
            return;
        }
        if block.id == "trial_spawner" {
            let word = self.start_wave(hit.block, block.metadata);
            self.gui.push_chat(word);
            return;
        }
        if block.id == "flower_pot" {
            self.plant_pot(hit.block, block.metadata);
            return;
        }
        if block.id == "end_portal_frame" {
            self.fill_frame(hit.block, block.metadata);
            return;
        }
        if block.id == "dragon_egg" {
            self.hop_egg();
            return;
        }
        if block.id == "cake" {
            self.eat_cake(hit.block, block.metadata);
            return;
        }
        if block.id == "pumpkin" {
            self.carve_pumpkin(hit.block, block.metadata);
            return;
        }
        let opened = match block.id {
            "crafting_table" => {
                if self.try_shape_pot() {
                    true
                } else {
                    self.gui.open_craft()
                }
            }
            "furnace" => {
                if self.try_furnace_heat() {
                    true
                } else {
                    self.gui.open_furnace()
                }
            }
            "anvil" => self.gui.open_anvil(hit.block),
            "enchanting_table" => self.gui.open_enchanting(hit.block),
            "brewing_stand" => self.gui.open_brewing(hit.block),
            "grindstone" => self.gui.open_grind(hit.block),
            "smithing_table" => self.gui.open_smith(hit.block),
            "loom" => self.gui.open_loom(),
            "cartography_table" => self.gui.open_chart(),
            "lectern" => self.gui.open_lectern(hit.block, block.metadata & 1 == 1),
            "chiseled_bookshelf" => self.gui.open_shelf(hit.block, block.metadata),
            "hanging_sign" => self.gui.open_hang(hit.block),
            "composter" => self.gui.open_compost(hit.block, block.metadata),
            "stonecutter" => self.gui.open_cut(),
            "campfire" => {
                if self.try_cook_heat() {
                    true
                } else {
                    self.gui.open_fire()
                }
            }
            "fletching_table" => self.gui.open_fletch(),
            "smoker" => {
                if self.try_cook_heat() {
                    true
                } else {
                    self.gui.open_smoker()
                }
            }
            "blast_furnace" => {
                if self.try_blast_heat() {
                    true
                } else {
                    self.gui.open_blast()
                }
            }
            "beehive" => self.gui.open_hive(hit.block, block.metadata),
            "cauldron" => self.gui.open_cauldron(hit.block, block.metadata),
            "bell" => self.gui.open_bell(hit.block),
            "candle" => self.gui.open_candle(hit.block, block.metadata & 1 == 1),
            "lodestone" => self.gui.open_lodestone(hit.block),
            "respawn_anchor" => self.gui.open_anchor(hit.block, block.metadata),
            "lightning_rod" => self.gui.open_rod(hit.block),
            "target" => self.gui.open_target(hit.block),
            "decorated_pot" => self.open_pot_at(hit.block),
            "shelf" => self.open_board_at(hit.block),
            "copper_bulb" => self.gui.open_bulb(hit.block, block.metadata & 1 == 1),
            _ => false,
        };
        if opened {
            if block.id == "bell" {
                let word = self.ring_bell();
                self.gui.note_bell(word);
                self.gui.push_chat(word);
            }
            if block.id == "candle" {
                let word = self.toggle_candle();
                self.gui.note_candle(word);
                self.gui.push_chat(word);
            }
            if block.id == "lodestone" {
                let word = self.link_lodestone();
                self.gui.note_lodestone(word);
                self.gui.push_chat(word);
            }
            if block.id == "respawn_anchor" {
                let word = self.charge_anchor();
                self.gui.note_anchor(word);
                self.gui.push_chat(word);
            }
            if block.id == "lightning_rod" {
                let word = self.strike_rod();
                self.gui.note_rod(word);
                self.gui.push_chat(word);
            }
            if block.id == "target" {
                let word = self.hit_target(hit.block);
                self.gui.note_target(word);
                self.gui.push_chat(word);
            }
            if block.id == "copper_bulb" {
                let word = self.toggle_bulb();
                self.gui.note_bulb(word);
                self.gui.push_chat(word);
            }
            if block.id == "decorated_pot" {
                let word = self.store_pot();
                self.gui.note_pot(word);
                self.gui.push_chat(word);
            }
            if block.id == "shelf" {
                let word = self.store_board();
                self.gui.note_board(word);
                self.gui.push_chat(word);
            }
            if block.id == "chiseled_bookshelf" {
                let word = self.store_shelf();
                self.gui.note_shelf(word);
                self.gui.push_chat(word);
                self.hotbar_dirty = true;
            }
            if block.id == "hanging_sign" {
                self.gui.note_hang(gui::HANGING_LINE);
                self.gui.push_chat(gui::HANGING_LINE);
            }
            self.clear_motion();
            self.set_capture(false);
            return;
        }
        self.open_looked_chest();
    }

    fn toggle_swing(&mut self, id: &str, at: [i32; 3], metadata: u8) {
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new(id, metadata ^ 1),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Power bit (8) on a pump — facing bits stay. Off is a closed valve.
    fn toggle_pump(&mut self, at: [i32; 3], metadata: u8) {
        let next = metadata ^ 8;
        self.gui.push_chat(if pump_on(next) { "pump on" } else { "pump off" });
        let (min, max) = body::player_box_bounds(self.camera.eye);
        let current = self.world.block_at(at[0], at[1], at[2]);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("pump", next).with_fill(current.fill),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Open bit (8) on a spout — facing bits stay. Off is a closed faucet.
    fn toggle_spout(&mut self, at: [i32; 3], metadata: u8) {
        let next = metadata ^ 8;
        self.gui
            .push_chat(if pump_on(next) { "spout open" } else { "spout closed" });
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("spout", next),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Open bit (8) on a drain — facing bits stay. Off is a closed intake.
    fn toggle_drain(&mut self, at: [i32; 3], metadata: u8) {
        let next = metadata ^ 8;
        self.gui
            .push_chat(if pump_on(next) { "drain open" } else { "drain closed" });
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("drain", next),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// On bit (0) on a generator — off stops charging.
    fn toggle_generator(&mut self, at: [i32; 3], metadata: u8) {
        let next = metadata ^ 1;
        self.gui.push_chat(if generator_on(next) {
            "generator on"
        } else {
            "generator off"
        });
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("generator", next),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Right-click a pump to toggle when not holding a block to place.
    fn try_use_pump(&mut self) -> bool {
        if self.hotbar.selected_block().is_some() {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id == "spout" {
            self.toggle_spout(hit.block, block.metadata);
            return true;
        }
        if block.id == "drain" {
            self.toggle_drain(hit.block, block.metadata);
            return true;
        }
        if block.id == "generator" {
            self.toggle_generator(hit.block, block.metadata);
            return true;
        }
        if block.id != "pump" {
            return false;
        }
        self.toggle_pump(hit.block, block.metadata);
        true
    }

    fn sleep_in_bed(&mut self, at: [i32; 3]) {
        if !self.note_respawn("bed", at) {
            self.gui.push_chat("awake");
            return;
        }
        let feet = [
            self.camera.eye.x,
            self.camera.eye.y - body::EYE_HEIGHT,
            self.camera.eye.z,
        ];
        let night = !(0.25..0.75).contains(&self.clock.time_of_day());
        if night && self.monsters_near_sleep(feet) {
            self.gui.push_chat("monsters nearby");
            return;
        }
        // Day click still claims the bed; night needs a clear ring first.
        self.spawn_at = Some(at);
        if !self.clock.sleep() {
            self.gui.push_chat("respawn point set");
            self.gui.push_chat("only sleep at night");
            return;
        }
        if self.night.clear_wild() {
            self.rebuild_zombie();
        }
        self.sleep_fade = 1.0;
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);
        self.note_dawn();
        self.gui.push_chat("slept until morning");
    }

    /// True when a living hostile sits inside the night-spawn min ring.
    fn monsters_near_sleep(&self, at: [f32; 3]) -> bool {
        let range = crate::mobs::SLEEP_MONSTER_RANGE;
        let r2 = range * range;
        let near = |origin: [f32; 3]| {
            let dx = origin[0] - at[0];
            let dy = origin[1] - at[1];
            let dz = origin[2] - at[2];
            dx * dx + dy * dy + dz * dz <= r2
        };
        if self.zombie.living() && near(self.zombie.origin()) {
            return true;
        }
        if self.night.any_living_within(at, range) {
            return true;
        }
        self.guests
            .iter()
            .any(|guest| guest.hostile() && guest.alive() && near(guest.origin()))
    }

    /// `events.dawn` sees the day of the four-day year. The first block it
    /// writes is sent as a place.
    fn note_dawn(&mut self) {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        self.hooks
            .dawn(&mut self.world, self.clock.year_day(), &view);
        self.flush_script_writes();
    }

    /// `events.dusk` sees the day of the four-day year. The first block it
    /// writes is sent as a place.
    fn note_dusk(&mut self) {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        self.hooks
            .dusk(&mut self.world, self.clock.year_day(), &view);
        self.flush_script_writes();
    }

    /// Soft chat tip when the sky crosses dawn or dusk while playing.
    fn tell_sky_edge(&mut self, word: &str) {
        if sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            self.gui.push_chat(word);
        }
    }

    /// Once per landmark, when the player first walks into range.
    fn tell_landmarks(&mut self) {
        if !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let eye = self.camera.eye;
        let near = |x: i32, z: i32| {
            let dx = eye.x - (x as f32 + 0.5);
            let dz = eye.z - (z as f32 + 0.5);
            dx * dx + dz * dz < 10.0 * 10.0
        };
        if !self.found_camp && near(rebuild_world::CAMP_FIRE.0, rebuild_world::CAMP_FIRE.1) {
            self.found_camp = true;
            self.gui.push_chat("a camp");
        }
        if !self.found_farm && near(rebuild_world::CAMP_FARM.0, rebuild_world::CAMP_FARM.1) {
            self.found_farm = true;
            self.gui.push_chat("a farm plot");
        }
        if !self.found_scarecrow
            && near(
                rebuild_world::CAMP_SCARECROW.0,
                rebuild_world::CAMP_SCARECROW.1,
            )
        {
            self.found_scarecrow = true;
            self.gui.push_chat("a scarecrow");
        }
        if !self.found_shrine && near(rebuild_world::SHRINE.0, rebuild_world::SHRINE.1) {
            self.found_shrine = true;
            self.gui.push_chat("a shrine");
        }
        if !self.found_mine && near(rebuild_world::SHAFT_MOUTH.0, rebuild_world::SHAFT_MOUTH.1) {
            self.found_mine = true;
            self.gui.push_chat("a mine — ladder into the pit");
        }
        if !self.found_rails && near(rebuild_world::SHAFT_RAIL.0, rebuild_world::SHAFT_RAIL.1) {
            self.found_rails = true;
            self.gui.push_chat("rails by the mine");
        }
        if !self.found_clay && near(rebuild_world::CLAY_BANK.0, rebuild_world::CLAY_BANK.1) {
            self.found_clay = true;
            self.gui.push_chat("clay under the water");
        }
        if !self.found_compound
            && near(rebuild_world::COMPOUND_DEMO.0, rebuild_world::COMPOUND_DEMO.1)
        {
            self.found_compound = true;
            self.gui.push_chat("compound loop by the pad");
        }
        if !self.found_wolf && near(crate::mobs::WOLF_PAD.0, crate::mobs::WOLF_PAD.1) {
            self.found_wolf = true;
            self.gui.push_chat("a wolf");
        }
        if !self.found_cat && near(crate::mobs::CAT_PAD.0, crate::mobs::CAT_PAD.1) {
            self.found_cat = true;
            self.gui.push_chat("a cat");
        }
        if !self.found_parrot && near(crate::mobs::PARROT_PAD.0, crate::mobs::PARROT_PAD.1) {
            self.found_parrot = true;
            self.gui.push_chat("a parrot");
        }
        if !self.found_horse && near(crate::mobs::HORSE_PAD.0, crate::mobs::HORSE_PAD.1) {
            self.found_horse = true;
            self.gui.push_chat("a horse");
        }
    }

    /// Step the demo zombie back when it wanders near the camp scarecrow.
    fn tick_scarecrow(&mut self) {
        if !self.zombie.living() {
            return;
        }
        let (sx, sz) = rebuild_world::CAMP_SCARECROW;
        let mut head_y = None;
        for y in 0..80 {
            let block = self.world.block_at(sx, y, sz);
            if block.id == "pumpkin" && block.metadata >= 1 {
                head_y = Some(y);
                break;
            }
        }
        let Some(hy) = head_y else {
            return;
        };
        if self.world.block_at(sx, hy - 1, sz).id != "fence" {
            return;
        }
        let feet = self.zombie.origin();
        let dx = feet[0] - (sx as f32 + 0.5);
        let dz = feet[2] - (sz as f32 + 0.5);
        if dx * dx + dz * dz > 7.0 * 7.0 {
            return;
        }
        self.zombie.spook([sx as f32 + 0.5, sz as f32 + 0.5]);
        self.rebuild_zombie();
    }

    /// Once: hold raw, look at a campfire or smoker — tip to use it.
    fn tell_cook_heat(&mut self) {
        if self.told_cook_heat || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead)
        {
            return;
        }
        if self.hotbar.slot(self.hotbar.selected()) != Some("raw") {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let heat = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id;
        if heat != "campfire" && heat != "smoker" {
            return;
        }
        self.told_cook_heat = true;
        self.gui.push_chat("use to cook");
    }

    /// Once: hold cobble/stone, look at a blast furnace — tip to use it.
    fn tell_blast_heat(&mut self) {
        if self.told_blast_heat || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead)
        {
            return;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return;
        };
        if !matches!(
            id,
            "cobblestone" | "stone" | "stone_bricks" | "netherrack"
        ) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "blast_furnace"
        {
            return;
        }
        self.told_blast_heat = true;
        self.gui.push_chat("use to blast");
    }

    /// Once: look at a pump — tip to toggle power / valve.
    fn tell_pump(&mut self) {
        if self.told_pump || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "pump"
        {
            return;
        }
        self.told_pump = true;
        self.gui.push_chat("use pump to toggle");
    }

    /// Once: look at a tank — tip that it stores multiple fill units.
    fn tell_tank(&mut self) {
        if self.told_tank || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "tank" {
            return;
        }
        self.told_tank = true;
        let level = tank_level(block);
        if level == 0 {
            self.gui.push_chat("tank stores fill");
        } else {
            self.gui.push_chat(&format!("tank {level}/{TANK_MAX}"));
        }
    }

    /// Once: look at a battery — tip that it stores ember charge.
    fn tell_battery(&mut self) {
        if self.told_battery || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "battery" {
            return;
        }
        self.told_battery = true;
        let level = battery_level(block);
        if level == 0 {
            self.gui.push_chat("battery stores ember");
        } else {
            self.gui.push_chat(&format!("battery {level}/{BATTERY_MAX}"));
        }
    }

    /// Once: look at a lamp — tip that wire power lights it.
    fn tell_lamp(&mut self) {
        if self.told_lamp || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "lamp" {
            return;
        }
        self.told_lamp = true;
        if lamp_on(block.metadata) {
            self.gui.push_chat("lamp on");
        } else {
            self.gui.push_chat("wire a battery to light");
        }
    }

    /// Once: look at a spout — tip that open dumps water.
    fn tell_spout(&mut self) {
        if self.told_spout || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "spout" {
            return;
        }
        self.told_spout = true;
        self.gui.push_chat("spout dumps tank water");
    }

    /// Once: look at a drain — tip that open pulls world water.
    fn tell_drain(&mut self) {
        if self.told_drain || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "drain" {
            return;
        }
        self.told_drain = true;
        self.gui.push_chat("drain pulls world water");
    }

    /// Once: look at a generator — tip that it charges the wire net.
    fn tell_generator(&mut self) {
        if self.told_generator || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead)
        {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "generator" {
            return;
        }
        self.told_generator = true;
        self.gui.push_chat("generator charges batteries");
    }

    /// True when a campfire sits on any face of `at` (optional fuel flavor).
    fn campfire_beside(&self, at: [i32; 3]) -> bool {
        const DIRS: [[i32; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        DIRS.iter().any(|[dx, dy, dz]| {
            self.world
                .block_at(at[0] + dx, at[1] + dy, at[2] + dz)
                .id
                == "campfire"
        })
    }

    /// Once: hold clay, look at a furnace — tip to use it.
    fn tell_furnace_heat(&mut self) {
        if self.told_furnace_heat
            || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead)
        {
            return;
        }
        if self.hotbar.slot(self.hotbar.selected()) != Some("clay") {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "furnace"
        {
            return;
        }
        self.told_furnace_heat = true;
        self.gui.push_chat("use to bake");
    }

    /// Once: hold bricks or clay with both on the bar at a crafting table.
    fn tell_shape_pot(&mut self) {
        if self.told_shape_pot || !sky_edge_tip_open(self.playing, self.menu.is_open(), self.dead)
        {
            return;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return;
        };
        if id != "bricks" && id != "clay" {
            return;
        }
        if !self.hotbar.holds("bricks", 1) || !self.hotbar.holds("clay", 1) {
            return;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "crafting_table"
        {
            return;
        }
        self.told_shape_pot = true;
        self.gui.push_chat("use to pot");
    }

    /// True when `events.jump` keeps the player on the ground.
    ///
    /// The cell is where the feet are. A consumed jump still sends the first
    /// `world.set_block`, and so does a jump that is allowed to launch.
    fn try_script_jump(&mut self, x: i32, y: i32, z: i32) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let allow = self.hooks.jump(&mut self.world, x, y, z, &view);
        self.flush_script_writes();
        !allow
    }

    /// True when `events.sprint` keeps the walk at normal speed.
    ///
    /// The cell is where the feet are. A refused sprint still sends the first
    /// `world.set_block`, and so does a sprint that is allowed to go fast.
    fn try_script_sprint(&mut self, x: i32, y: i32, z: i32) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let allow = self.hooks.sprint(&mut self.world, x, y, z, &view);
        self.flush_script_writes();
        !allow
    }

    /// `events.land` sees the feet cell and how many whole blocks the player fell.
    /// The first block it writes is sent as a place.
    fn note_land(&mut self, drop: i32) {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let feet = body::feet_y(self.camera.eye.y);
        self.hooks.land(
            &mut self.world,
            self.camera.eye.x.floor() as i32,
            (feet + 1.0e-3).floor() as i32,
            self.camera.eye.z.floor() as i32,
            drop,
            &view,
        );
        self.flush_script_writes();
    }

    fn refund_station(&mut self) {
        let mut moved = false;
        for id in self.gui.drain_craft() {
            moved = true;
            if !self.hotbar.collect(id) {
                self.bag.collect(id);
            }
        }
        for id in self.gui.drain_furnace() {
            moved = true;
            if !self.hotbar.collect(id) {
                self.bag.collect(id);
            }
        }
        for id in self.gui.drain_inv_craft() {
            moved = true;
            if !self.hotbar.collect(id) {
                self.bag.collect(id);
            }
        }
        if moved {
            self.hotbar_dirty = true;
            self.bag_dirty = true;
        }
    }

    fn stash_into_craft(&mut self, digit: u8) {
        let index = (digit - 1) as usize;
        self.hotbar.select_digit(digit);
        let Some(id) = self.hotbar.slot(index) else {
            return;
        };
        let id = id.to_string();
        if !self.gui.give_craft(&id) {
            return;
        }
        self.hotbar.consume_at(index);
        self.hotbar_dirty = true;
    }

    fn stash_into_furnace(&mut self, digit: u8) {
        let index = (digit - 1) as usize;
        self.hotbar.select_digit(digit);
        let Some(id) = self.hotbar.slot(index) else {
            return;
        };
        let id = id.to_string();
        if !self.gui.give_furnace(&id) {
            return;
        }
        self.hotbar.consume_at(index);
        self.hotbar_dirty = true;
    }

    fn take_craft_result(&mut self) {
        let Some(id) = self.gui.craft_preview() else {
            return;
        };
        if !self.hotbar.room_for(&id) && !self.bag.room_for(&id) {
            return;
        }
        if self.try_script_craft(&id) {
            return;
        }
        let Some(id) = self.gui.pull_craft() else {
            return;
        };
        if !self.hotbar.collect(&id) {
            self.bag.collect(&id);
        }
        self.hotbar_dirty = true;
        self.bag_dirty = true;
    }

    fn take_furnace_output(&mut self) {
        let Some(id) = self.gui.furnace_output() else {
            return;
        };
        if self.try_script_smelt(&id) {
            return;
        }
        let Some(id) = self.gui.take_furnace_output() else {
            return;
        };
        if self.hotbar.collect(id) || self.bag.collect(id) {
            self.hotbar_dirty = true;
            self.bag_dirty = true;
            return;
        }
        self.gui.give_furnace_back(id);
    }

    fn open_looked_chest(&mut self) {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if !Pack::builtin().is_container(block.id) {
            return;
        }
        let path = crate::net::world_file(&self.loaded_world);
        let (has_record, mut slots) = if block.id == "ender_chest" {
            (
                true,
                rebuild_world::read_ender_chest(&path).unwrap_or_default(),
            )
        } else {
            let found = rebuild_world::read_chests(&path)
                .unwrap_or_default()
                .into_iter()
                .find(|chest| {
                    chest.x == hit.block[0] && chest.y == hit.block[1] && chest.z == hit.block[2]
                });
            let has_record = found.is_some();
            let slots = found.map(|chest| chest.slots).unwrap_or_default();
            (has_record, slots)
        };
        let mut seeded = false;
        let fresh = fresh_camp(has_record, hit.block[0], hit.block[2], self.omen)
            .or_else(|| fresh_shrine(has_record, hit.block[0], hit.block[2], self.omen))
            .or_else(|| fresh_mine(has_record, hit.block[0], hit.block[2], self.omen));
        if let Some(mut find) = fresh {
            let bar = hotbar_slots(&self.hotbar);
            let view = client_view(&bar, &self.hotbar, self.camera.eye, self.clock);
            if let Some(next) = self.hooks.loot(
                &mut self.world,
                &find,
                hit.block[0],
                hit.block[2],
                self.omen,
                &view,
            ) {
                find = next;
            }
            slots = vec![rebuild_world::CarriedSlot {
                slot: 0,
                id: find,
                count: 1,
            }];
            seeded = true;
        }
        if self.gui.open_chest(&slots) {
            self.chest_at = Some((hit.block[0], hit.block[1], hit.block[2]));
            if seeded {
                self.chest_dirty = true;
            }
            self.clear_motion();
            self.set_capture(false);
        }
    }

    fn take_chest_slot(&mut self) {
        let Some(id) = self.gui.take_chest() else {
            return;
        };
        let id = id.to_string();
        if self.hotbar.collect(&id) {
            self.hotbar_dirty = true;
            self.chest_dirty = true;
            return;
        }
        self.gui.give_chest(&id);
    }

    fn stash_into_chest(&mut self, digit: u8) {
        let index = (digit - 1) as usize;
        self.hotbar.select_digit(digit);
        let Some(id) = self.hotbar.slot(index) else {
            return;
        };
        let id = id.to_string();
        if !self.gui.give_chest(&id) {
            return;
        }
        self.hotbar.consume_at(index);
        self.hotbar_dirty = true;
        self.chest_dirty = true;
    }

    fn flush_chest(&mut self) {
        if !self.chest_dirty || self.told_disconnect {
            return;
        }
        let Some((x, y, z)) = self.chest_at else {
            self.chest_dirty = false;
            return;
        };
        let slots = self.gui.chest_wire();
        if self.link.send(encode_chest(x, y, z, &slots)).is_err() {
            self.note_disconnect();
            return;
        }
        self.chest_dirty = false;
    }

    /// Hold bricks or clay and use a looked-at crafting table to shape a pot.
    fn try_shape_pot(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if id != "bricks" && id != "clay" {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "crafting_table"
        {
            return false;
        }
        let word = self.hotbar.shape_pot();
        self.gui.push_chat(word);
        if word == "potted" {
            self.hotbar_dirty = true;
            let at = [
                hit.block[0] as f32 + 0.5,
                hit.block[1] as f32 + 0.9,
                hit.block[2] as f32 + 0.5,
            ];
            self.play_world(crate::audio::Sound::Place, at);
        }
        true
    }

    /// Hold clay and use a looked-at furnace to bake bricks in one step.
    fn try_furnace_heat(&mut self) -> bool {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "furnace"
        {
            return false;
        }
        let word = self.hotbar.furnace_now();
        if word == "need clay" {
            return false;
        }
        self.gui.push_chat(word);
        if word != "no room" {
            self.hotbar_dirty = true;
            let at = [
                hit.block[0] as f32 + 0.5,
                hit.block[1] as f32 + 0.9,
                hit.block[2] as f32 + 0.5,
            ];
            self.play_world(crate::audio::Sound::Crackle, at);
            self.drops.smoke(at);
            self.drops
                .smoke([at[0] + 0.05, at[1] + 0.14, at[2] - 0.06]);
            self.rebuild_drops();
        }
        true
    }

    /// Hold cobble or stone and use a looked-at blast furnace in one step.
    fn try_blast_heat(&mut self) -> bool {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        if self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id
            != "blast_furnace"
        {
            return false;
        }
        let word = self.hotbar.blast_now();
        if word == "need blast" {
            return false;
        }
        self.gui.push_chat(word);
        if word != "no room" {
            self.hotbar_dirty = true;
            let at = [
                hit.block[0] as f32 + 0.5,
                hit.block[1] as f32 + 0.9,
                hit.block[2] as f32 + 0.5,
            ];
            self.play_world(crate::audio::Sound::Crackle, at);
            self.drops.smoke(at);
            self.drops
                .smoke([at[0] - 0.06, at[1] + 0.1, at[2] + 0.04]);
            self.rebuild_drops();
        }
        true
    }

    /// Hold raw food and use a looked-at campfire or smoker to cook in one step.
    fn try_cook_heat(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if id != "raw" {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let heat = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2])
            .id;
        if heat != "campfire" && heat != "smoker" {
            return false;
        }
        let word = self.hotbar.cook_raw_now();
        self.gui.push_chat(word);
        if word == "cooked" {
            self.hotbar_dirty = true;
            let at = [
                hit.block[0] as f32 + 0.5,
                hit.block[1] as f32 + 0.9,
                hit.block[2] as f32 + 0.5,
            ];
            self.play_world(crate::audio::Sound::Crackle, at);
            // A couple smoke motes so the cook reads in the world.
            self.drops.smoke(at);
            self.drops
                .smoke([at[0] + 0.08, at[1] + 0.12, at[2] - 0.05]);
            self.rebuild_drops();
        }
        true
    }

    /// Eat the held food. True when the selected slot is food, even if the bar is full.
    fn try_eat(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        let Some(amount) = Pack::builtin().food_of(id) else {
            return false;
        };
        let potion = id == "healing_potion";
        let fed = self.hunger.eat(amount);
        let before = self.hearts;
        if potion {
            self.hearts = body::potion_hearts(self.hearts);
        }
        if !fed && self.hearts == before {
            return true;
        }
        if potion {
            self.gui.push_chat("healed");
        }
        if id == "chorus_fruit" {
            if let Some(eye) = body::chorus_hop(self.camera.eye, &self.world) {
                self.camera.eye = eye;
                self.camera.velocity_y = 0.0;
                self.camera.on_ground = true;
                self.gui.push_chat("blink");
            }
        }
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.note_taste(potion);
        true
    }

    fn holding_omen(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_omen(id))
    }

    /// Drink the held ominous bottle. The next trial key pays richer loot.
    fn try_omen(&mut self) -> bool {
        if !self.holding_omen() {
            return false;
        }
        self.omen = true;
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.gui.push_chat("omen");
        self.note_taste(true);
        true
    }

    /// A bite or a sip. The view dips and a strip shows along the bottom.
    fn note_taste(&mut self, drink: bool) {
        self.sip = 1.0;
        self.sipping = drink;
        self.chew_phase = 0.0;
        self.burp_due = !drink;
        let sound = if drink {
            crate::audio::Sound::Sip
        } else {
            crate::audio::Sound::Bite
        };
        self.audio.play(sound);
    }

    fn holding_key(&self) -> bool {
        self.hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_key(id))
    }

    /// Unlock the looked-at copper chest once. True when a trial key is selected and a chest is in view.
    fn try_key(&mut self) -> bool {
        if !self.holding_key() {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "copper_chest" {
            return false;
        }
        if trial_open(block.metadata) {
            self.gui.push_chat("empty");
            return true;
        }
        let omen = self.omen;
        let Some(mut find) = trial_loot(hit.block[0], hit.block[2], omen) else {
            self.gui.push_chat("loot");
            return true;
        };
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if let Some(next) = self.hooks.loot(
            &mut self.world,
            &find,
            hit.block[0],
            hit.block[2],
            omen,
            &view,
        ) {
            find = next;
        }
        let slot = self.hotbar.selected();
        let opens = self.hotbar.count(slot) <= 1;
        if !self.hotbar.room_for(&find) && !opens && !self.bag.room_for(&find) {
            self.gui.push_chat("full");
            return true;
        }
        self.hotbar.consume_at(slot);
        if omen {
            self.omen = false;
        }
        if !self.hotbar.collect(&find) {
            self.bag.collect(&find);
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.mark_trial(hit.block, block.metadata);
        let source = [
            hit.block[0] as f32 + 0.5,
            hit.block[1] as f32 + 0.5,
            hit.block[2] as f32 + 0.5,
        ];
        self.play_world(crate::audio::Sound::Place, source);
        self.gui.push_chat(&find.replace('_', " "));
        true
    }

    /// Start a wave, or hand over a key a full bar could not take.
    fn start_wave(&mut self, at: [i32; 3], metadata: u8) -> &'static str {
        if self.trial_due {
            let owed = self.trial_at.unwrap_or(at);
            return self.pay_trial(owed);
        }
        if self.trial_at.is_some() || metadata & 1 != 0 {
            return "fight";
        }
        self.trial_at = Some(at);
        let feet = [at[0] as f32 + 1.5, at[1] as f32, at[2] as f32 + 0.5];
        let breeze = self.trial_flip;
        self.trial_flip = !breeze;
        self.wave_breeze = breeze;
        if breeze {
            self.breeze.rouse(feet);
            self.rebuild_breeze();
            self.mark_spawner(at, metadata | 1);
            "breeze"
        } else {
            self.zombie.rouse(feet);
            self.rebuild_zombie();
            self.mark_spawner(at, metadata | 1);
            "wave"
        }
    }

    /// Give the trial key and quiet the cage. `"full"` leaves the debt in place.
    fn pay_trial(&mut self, at: [i32; 3]) -> &'static str {
        if !self.hotbar.room_for("trial_key") && !self.bag.room_for("trial_key") {
            self.trial_due = true;
            return "full";
        }
        if !self.hotbar.collect("trial_key") {
            self.bag.collect("trial_key");
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.trial_due = false;
        self.trial_at = None;
        self.wave_breeze = false;
        self.mark_spawner(at, 0);
        "clear"
    }

    /// Hand over the breeze rod. A full bar leaves the debt for a later tick.
    fn grant_rod(&mut self) {
        if !self.hotbar.room_for("breeze_rod") && !self.bag.room_for("breeze_rod") {
            if !self.breeze_due {
                self.gui.push_chat("full");
            }
            self.breeze_due = true;
            return;
        }
        if !self.hotbar.collect("breeze_rod") {
            self.bag.collect("breeze_rod");
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.breeze_due = false;
        self.gui.push_chat("rod");
    }

    /// Hand over the slimeball. A full bar leaves the debt for a later tick.
    fn grant_slime(&mut self) {
        if !self.hotbar.room_for("slimeball") && !self.bag.room_for("slimeball") {
            if !self.slime_due {
                self.gui.push_chat("full");
            }
            self.slime_due = true;
            return;
        }
        if !self.hotbar.collect("slimeball") {
            self.bag.collect("slimeball");
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.slime_due = false;
        self.gui.push_chat("slime");
    }

    /// Hand over the goat horn. A full bar leaves the debt for a later tick.
    fn grant_horn(&mut self) {
        if !self.hotbar.room_for("goat_horn") && !self.bag.room_for("goat_horn") {
            if !self.horn_due {
                self.gui.push_chat("full");
            }
            self.horn_due = true;
            return;
        }
        if !self.hotbar.collect("goat_horn") {
            self.bag.collect("goat_horn");
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.horn_due = false;
        self.gui.push_chat("horn");
    }

    /// Knock the player and any body standing by the charge.
    fn goat_ram(&mut self, ram: crate::mobs::Ram) {
        let from = ram.from;
        if let Some(butt) = crate::mobs::goat_shove(from, self.camera.eye.to_array()) {
            self.camera.eye.x += butt.dx;
            self.camera.eye.z += butt.dz;
            if butt.lift > self.camera.velocity_y {
                self.camera.velocity_y = butt.lift;
            }
            self.camera.on_ground = false;
        }
        self.sheep.bump(from);
        self.pig.bump(from);
        self.horse.bump(from);
        self.zombie.bump(from);
        self.breeze.bump(from);
        self.golem.bump(from);
        self.wolf.bump(from);
        self.cat.bump(from);
        self.parrot.bump(from);
        self.frog.bump(from);
        self.axolotl.bump(from);
        self.chicken.bump(from);
        self.bee.bump(from);
        self.fox.bump(from);
        self.play_world(crate::audio::Sound::Step, self.goat.origin());
        self.gui.push_chat("ram");
        if ram.horn {
            self.grant_horn();
        }
    }

    fn mark_spawner(&mut self, at: [i32; 3], metadata: u8) {
        if !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("trial_spawner", metadata),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    fn mark_trial(&mut self, at: [i32; 3], metadata: u8) {
        let want = trial_mark(metadata);
        if metadata == want || !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("copper_chest", want),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Throw the held wind charge. It can still shove a mob. It never moves you.
    fn try_wind(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_wind(id) {
            return false;
        }
        let eye = self.camera.eye;
        let forward = self.camera.forward();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        let Some(burst) = body::wind_spot(eye, forward, &self.world) else {
            self.gui.push_chat("miss");
            return true;
        };
        let shoved = self.zombie.shove(burst)
            || self.breeze.shove(burst)
            || self.golem.shove(burst)
            || self.wolf.shove(burst)
            || self.cat.shove(burst)
            || self.parrot.shove(burst)
            || self.frog.shove(burst)
            || self.axolotl.shove(burst)
            || self.chicken.shove(burst)
            || self.bee.shove(burst)
            || self.fox.shove(burst)
            || self.goat.shove(burst)
            || self.pig.shove(burst)
            || self.horse.shove(burst)
            || self.sheep.shove(burst);
        if shoved {
            self.rebuild_zombie();
            self.rebuild_breeze();
            self.rebuild_golem();
            self.rebuild_wolf();
            self.rebuild_cat();
            self.rebuild_parrot();
            self.rebuild_frog();
            self.rebuild_axolotl();
            self.rebuild_chicken();
            self.rebuild_bee();
            self.rebuild_fox();
            self.rebuild_goat();
            self.rebuild_pig();
            self.rebuild_horse();
            self.rebuild_sheep();
        }
        self.play_world(crate::audio::Sound::Gust, burst);
        self.gui.push_chat("gust");
        true
    }

    /// Throw the held pearl along the look. True when the selected slot is a pearl.
    fn try_pearl(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_pearl(id) {
            return false;
        }
        let forward = self.camera.forward();
        if let Some(eye) = body::pearl_landing(self.camera.eye, forward, &self.world) {
            self.camera.eye = eye;
            self.camera.velocity_y = 0.0;
            self.camera.on_ground = true;
            self.hurt(1);
            self.gui.push_chat("pearl");
        }
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        true
    }

    /// An arrow aimed at a target scores a hit. True when the selected slot is an arrow.
    fn try_arrow(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_arrow(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if arrow_scores(id, block.id) {
            let word = self.hit_target(hit.block);
            if self.gui.target_open() {
                self.gui.note_target(word);
            }
            self.gui.push_chat(word);
        }
        true
    }

    /// Till dirt or grass into farmland with a hoe. True when holding a hoe.
    fn try_till(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_hoe(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        if !self.pending.is_empty() {
            return true;
        }
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "dirt" && block.id != "grass" {
            self.gui.push_chat("need dirt");
            return true;
        }
        let slot = self.hotbar.selected();
        if !self.hotbar.wear_use(slot) {
            self.gui.push_chat("broken");
            return true;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                hit.block[0],
                hit.block[1],
                hit.block[2],
                Block::new("farmland", 0),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
        let mote = [
            hit.block[0] as f32 + 0.5,
            hit.block[1] as f32 + 1.05,
            hit.block[2] as f32 + 0.5,
        ];
        self.drops.puff(mote, [0.55, 0.38, 0.22]);
        self.rebuild_drops();
        self.hotbar_dirty = true;
        self.gui.push_chat("till");
        if self.hotbar.slot(slot).is_none() {
            self.gui.push_chat("the hoe breaks");
        }
        true
    }

    /// Plant wheat seeds on dirt or grass. True when holding seeds.
    fn try_plant_seeds(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_seeds(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        if !self.pending.is_empty() {
            return true;
        }
        let soil = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        let (x, y, z) = if matches!(soil.id, "dirt" | "grass" | "farmland") {
            (hit.block[0], hit.block[1] + 1, hit.block[2])
        } else {
            let below = self
                .world
                .block_at(hit.place[0], hit.place[1] - 1, hit.place[2]);
            if matches!(below.id, "dirt" | "grass" | "farmland") {
                (hit.place[0], hit.place[1], hit.place[2])
            } else {
                self.gui.push_chat("need soil");
                return true;
            }
        };
        if !(0..256).contains(&y) || !self.world.block_at(x, y, z).is_air() {
            self.gui.push_chat("blocked");
            return true;
        }
        let slot = self.hotbar.selected();
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                x,
                y,
                z,
                Block::new("wheat_crop", 0),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        let mote = [x as f32 + 0.5, y as f32 + 0.25, z as f32 + 0.5];
        self.drops.puff(mote, [0.55, 0.72, 0.28]);
        self.rebuild_drops();
        self.gui.push_chat("plant");
        true
    }

    /// Spend bone meal on wheat or sugar cane. True when holding fertilizer.
    fn try_fertilize(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_fertilizer(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if !self.pending.is_empty() {
            return true;
        }
        if block.id == "wheat_crop" {
            if block.metadata >= 3 {
                self.gui.push_chat("ripe");
                return true;
            }
            let next = block.metadata.saturating_add(1).min(3);
            let slot = self.hotbar.selected();
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    hit.block[0],
                    hit.block[1],
                    hit.block[2],
                    Block::new("wheat_crop", next),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                Some(slot),
            );
            let mote = [
                hit.block[0] as f32 + 0.5,
                hit.block[1] as f32 + 0.35 + next as f32 * 0.2,
                hit.block[2] as f32 + 0.5,
            ];
            self.drops.puff(mote, [0.72, 0.82, 0.32]);
            self.rebuild_drops();
            self.gui.push_chat(if next >= 3 { "ripe" } else { "grow" });
            return true;
        }
        if block.id != "sugar_cane" {
            self.gui.push_chat("need crop");
            return true;
        }
        let (x, z) = (hit.block[0], hit.block[2]);
        let mut top = hit.block[1];
        while top < 255 && self.world.block_at(x, top + 1, z).id == "sugar_cane" {
            top += 1;
        }
        let mut base = hit.block[1];
        while base > 0 && self.world.block_at(x, base - 1, z).id == "sugar_cane" {
            base -= 1;
        }
        if top - base + 1 >= 3 {
            self.gui.push_chat("tall");
            return true;
        }
        let grow_y = top + 1;
        if !self.world.block_at(x, grow_y, z).is_air() {
            self.gui.push_chat("blocked");
            return true;
        }
        let slot = self.hotbar.selected();
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                x,
                grow_y,
                z,
                Block::new("sugar_cane", 0),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        let mote = [x as f32 + 0.5, grow_y as f32 + 0.35, z as f32 + 0.5];
        self.drops.puff(mote, [0.45, 0.85, 0.38]);
        self.rebuild_drops();
        self.gui.push_chat("grow");
        true
    }

    /// Cast a fishing bobber into looked-at water, or reel one in. True when holding a fishing rod.
    fn try_fish(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_fishing(id) {
            return false;
        }
        if self.fish_bobber.take().is_some() {
            self.gui.push_chat("reel");
            return true;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "water" {
            self.gui.push_chat("need water");
            return true;
        }
        let at = [
            hit.block[0] as f32 + 0.5,
            hit.block[1] as f32 + 0.92,
            hit.block[2] as f32 + 0.5,
        ];
        self.fish_bobber = Some((at, 2.2, 0.0));
        self.drops.brew_bubbles(at);
        self.drops.bobber(at);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Splash, at);
        self.gui.push_chat("cast");
        true
    }

    /// Wait-mote splash while a bobber sits, then catch a fish into the bar.
    fn tick_fish_bobber(&mut self, dt: f32) {
        let Some((at, wait, splash)) = self.fish_bobber else {
            return;
        };
        let holding = self
            .hotbar
            .slot(self.hotbar.selected())
            .is_some_and(|id| Pack::builtin().is_fishing(id));
        if !holding {
            self.fish_bobber = None;
            return;
        }
        let wait = wait - dt;
        let mut splash = splash - dt;
        if splash <= 0.0 {
            self.drops.brew_bubbles(at);
            self.drops.bobber(at);
            self.rebuild_drops();
            self.play_world(crate::audio::Sound::Lap, at);
            splash = 0.42;
        }
        if wait <= 0.0 {
            self.fish_bobber = None;
            if !self.hotbar.room_for("fish") && !self.bag.room_for("fish") {
                self.gui.push_chat("full");
                self.drops.puff(at, [0.45, 0.72, 0.88]);
                self.rebuild_drops();
                self.play_world(crate::audio::Sound::Splash, at);
                return;
            }
            if !self.hotbar.collect("fish") {
                self.bag.collect("fish");
                self.bag_dirty = true;
            }
            self.hotbar_dirty = true;
            self.drops.puff(at, [0.55, 0.82, 0.95]);
            self.drops.bobber(at);
            self.rebuild_drops();
            self.play_world(crate::audio::Sound::Splash, at);
            self.gui.push_chat("fish");
            return;
        }
        self.fish_bobber = Some((at, wait, splash));
    }

    /// Throw the held eye toward the stairs. True when the selected slot is an eye.
    fn try_eye(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_eye(id) {
            return false;
        }
        let id = id.to_string();
        let flight = crate::camera::eye_throw(self.camera.eye.x, self.camera.eye.z);
        let here =
            (self.camera.eye.x - flight.landing[0]).hypot(self.camera.eye.z - flight.landing[1]);
        self.hotbar.consume_selected();
        if here < 0.05 {
            self.gui.push_chat("at the stairs");
        } else {
            self.gui.push_chat("toward the stairs");
        }
        if flight.breaks {
            self.gui.push_chat("the eye breaks");
        } else if !self.hotbar.collect(&id) {
            self.bag.collect(&id);
            self.bag_dirty = true;
            self.gui.push_chat("the eye drops");
        } else {
            self.gui.push_chat("the eye drops");
        }
        self.hotbar_dirty = true;
        true
    }

    /// Wear the held armor. True when the selected slot is a piece.
    fn try_equip(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        let Some((slot, _)) = Pack::builtin().armor_of(id) else {
            return false;
        };
        let id = rebuild_world::Block::new(id, 0).id;
        let previous = self.armor.equip(slot as usize, id);
        self.hotbar.consume_selected();
        if let Some(old) = previous {
            if !self.hotbar.collect(old) {
                self.bag.collect(old);
                self.bag_dirty = true;
            }
        }
        self.hotbar_dirty = true;
        let piece = match slot {
            0 => "cap",
            1 => "tunic",
            _ => "boots",
        };
        self.gui.push_chat(&format!("{piece} on"));
        true
    }

    fn armor_soak(&self) -> i32 {
        self.armor
            .worn()
            .into_iter()
            .flatten()
            .filter_map(|id| Pack::builtin().armor_of(id).map(|(_, soak)| soak))
            .sum()
    }

    fn apply_damage(&mut self, amount: i32) {
        let amount = self.hooks.entity_hurt(&mut self.world, "player", amount);
        if amount <= 0 {
            return;
        }
        self.lose_hearts(amount);
    }

    /// Ask hurt hooks, then apply damage. `origin` is used for `entity_death` when it drops.
    fn hurt_mob(
        &mut self,
        kind: &str,
        amount: i32,
        origin: [f32; 3],
        apply: impl FnOnce(&mut Self, i32) -> bool,
    ) -> bool {
        let amount = self.hooks.entity_hurt(&mut self.world, kind, amount);
        if amount <= 0 {
            return false;
        }
        let dropped = apply(self, amount);
        if dropped {
            let _ = self.hooks.entity_death(
                &mut self.world,
                kind,
                origin[0].floor() as i32,
                origin[1].floor() as i32,
                origin[2].floor() as i32,
            );
        }
        dropped
    }

    /// Spend hearts. At zero the death screen waits instead of respawning now.
    fn lose_hearts(&mut self, amount: i32) {
        if self.dead || amount <= 0 {
            return;
        }
        self.hurt_flash = 1.0;
        self.hit_kick = 1.0;
        self.hearts -= amount;
        if self.hearts <= 0 {
            self.hearts = 0;
            self.dead = true;
            self.clear_motion();
            self.menu.close_menu();
            self.gui.close();
            self.map_open = false;
            self.set_capture(false);
        }
    }

    fn hurt(&mut self, amount: i32) {
        self.apply_damage(body::soaked(amount, self.armor_soak()));
    }

    /// Offer the fox a sweet food when the look ray hits it.
    fn try_fox(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.fox.aimed(eye, forward) {
            return false;
        }
        if self.fox.trusted() {
            self.fox.sit();
            self.rebuild_fox();
            self.gui
                .push_chat(if self.fox.sitting() { "sit" } else { "follow" });
            return true;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !crate::mobs::fox_food(id) {
            return false;
        }
        self.fox.trust();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.rebuild_fox();
        self.gui.push_chat("trust");
        true
    }

    /// Feed the frog honey when the look ray hits it. It pays with a slimeball.
    fn try_frog(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.frog.aimed(eye, forward) {
            return false;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !crate::mobs::frog_food(id) {
            return false;
        }
        self.frog.feed();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.rebuild_frog();
        self.grant_slime();
        true
    }

    /// Right-click the axolotl so it follows through the water, or swims again.
    fn try_axolotl(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.axolotl.aimed(eye, forward) {
            return false;
        }
        let following = self.axolotl.follow();
        self.rebuild_axolotl();
        self.gui
            .push_chat(if following { "companion" } else { "swim" });
        true
    }

    /// Right-click a ready egg and take the raw food. An empty nest only pecks.
    fn try_chicken(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.chicken.aimed(eye, forward) {
            return false;
        }
        if !self.chicken.egg() {
            self.gui.push_chat("pecking");
            return true;
        }
        if !self.hotbar.room_for("raw") && !self.bag.room_for("raw") {
            self.gui.push_chat("full");
            return true;
        }
        self.chicken.take_egg();
        if !self.hotbar.collect("raw") {
            self.bag.collect("raw");
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        self.rebuild_chicken();
        self.gui.push_chat("egg");
        true
    }

    /// Feed the parrot a hay bale, or empty-hand perch/hop-off a tamed one.
    fn try_parrot(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        let held = self.hotbar.slot(self.hotbar.selected());
        // Perched birds sit on the shoulder — empty hand hops off without a hard aim.
        let aimed = self.parrot.aimed(eye, forward)
            || (self.parrot.shoulder() && held.is_none() && self.looking_near_parrot());
        if !aimed {
            return false;
        }
        if self.parrot.tamed() {
            if held.is_some() {
                return false;
            }
            self.parrot.perch();
            if self.parrot.shoulder() {
                let feet = [
                    self.camera.eye.x,
                    self.camera.eye.y - body::EYE_HEIGHT,
                    self.camera.eye.z,
                ];
                let (yaw, _) = self.camera.yaw_pitch();
                self.parrot.ride_shoulder(feet, yaw);
            }
            self.rebuild_parrot();
            self.gui.push_chat(if self.parrot.shoulder() {
                "shoulder"
            } else {
                "follow"
            });
            return true;
        }
        let Some(id) = held else {
            return false;
        };
        if !crate::mobs::parrot_food(id) {
            return false;
        }
        self.parrot.feed();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.rebuild_parrot();
        self.gui.push_chat("tamed");
        true
    }

    /// Soft aim cone for hopping a perched parrot off the shoulder.
    fn looking_near_parrot(&self) -> bool {
        let eye = self.camera.eye;
        let forward = self.camera.forward();
        let at = glam::Vec3::from(self.parrot.origin());
        let to = at - eye;
        let along = to.dot(forward);
        if !(0.0..=4.0).contains(&along) {
            return false;
        }
        let closest = eye + forward * along;
        (closest - at).length_squared() < 1.2
    }

    /// Repeat a note or horn when the tamed parrot is close enough to hear it.
    fn tell_parrot(&mut self, sound: crate::mobs::Mimic, at: [f32; 3]) {
        if !self.parrot.tamed() {
            return;
        }
        let from = self.parrot.origin();
        let dx = from[0] - at[0];
        let dy = from[1] - at[1];
        let dz = from[2] - at[2];
        if self.parrot.shoulder() || dx * dx + dy * dy + dz * dz < 64.0 {
            self.parrot.hear(sound);
        }
    }

    /// Feed the cat raw meat, or empty-hand sit/follow a tamed cat.
    fn try_cat(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.cat.aimed(eye, forward) {
            return false;
        }
        let held = self.hotbar.slot(self.hotbar.selected());
        if self.cat.tamed() {
            if held.is_some() {
                return false;
            }
            self.cat.sit();
            self.rebuild_cat();
            self.gui
                .push_chat(if self.cat.sitting() { "sit" } else { "follow" });
            return true;
        }
        let Some(id) = held else {
            return false;
        };
        if !crate::mobs::cat_food(id) {
            return false;
        }
        self.cat.feed();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.rebuild_cat();
        self.gui.push_chat("tamed");
        true
    }

    /// Feed the wolf bone meal or meat, or empty-hand sit/follow a tamed wolf.
    fn try_wolf(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.wolf.aimed(eye, forward) {
            return false;
        }
        let held = self.hotbar.slot(self.hotbar.selected());
        if self.wolf.tamed() {
            // Empty hand toggles sit ↔ follow; a held item keeps place/use free.
            if held.is_some() {
                return false;
            }
            self.wolf.sit();
            self.rebuild_wolf();
            self.gui
                .push_chat(if self.wolf.sitting() { "sit" } else { "follow" });
            return true;
        }
        let Some(id) = held else {
            return false;
        };
        if !crate::mobs::wolf_food(id) {
            return false;
        }
        self.wolf.feed();
        self.hotbar.consume_selected();
        self.hotbar_dirty = true;
        self.rebuild_wolf();
        self.gui.push_chat("tamed");
        true
    }

    /// Feed the horse wheat to tame it.
    fn try_horse(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if !self.horse.aimed(eye, forward) {
            return false;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !crate::mobs::horse_food(id) {
            return false;
        }
        if self.horse.feed() {
            self.hotbar.consume_selected();
            self.hotbar_dirty = true;
            self.rebuild_horse();
            self.gui.push_chat("tamed");
        } else {
            self.gui.push_chat("already tame");
        }
        true
    }

    /// Put the held saddle on the horse or pig when the look ray hits it.
    fn try_saddle(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_saddle(id) {
            return false;
        }
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if self.horse.aimed(eye, forward) {
            if !self.horse.tamed() {
                self.gui.push_chat("tame first");
                return true;
            }
            if self.horse.saddle() {
                self.hotbar.consume_selected();
                self.hotbar_dirty = true;
                self.rebuild_horse();
                self.gui.push_chat("saddled");
            } else {
                self.gui.push_chat("already saddled");
            }
            return true;
        }
        if !self.pig.aimed(eye, forward) {
            return true;
        }
        if self.pig.saddle() {
            self.hotbar.consume_selected();
            self.hotbar_dirty = true;
            self.rebuild_pig();
            self.gui.push_chat("saddled");
        } else {
            self.gui.push_chat("already saddled");
        }
        true
    }

    fn bucket_fits(&self, index: usize, from: &str, to: &str) -> bool {
        if self.hotbar.slot(index) != Some(from) {
            return false;
        }
        self.hotbar.count(index) <= 1 || self.hotbar.room_for(to) || self.bag.room_for(to)
    }

    /// Turn one `from` into one `to`. A lone item stays in the same slot.
    fn exchange_bucket(&mut self, index: usize, from: &str, to: &str) -> bool {
        if self.hotbar.exchange_last(index, from, to) {
            self.hotbar_dirty = true;
            return true;
        }
        if !self.hotbar.shrink(index, from) {
            return false;
        }
        if !self.hotbar.collect(to) {
            self.bag.collect(to);
            self.bag_dirty = true;
        }
        self.hotbar_dirty = true;
        true
    }

    /// Slow pipe seep: copy fill into one nearby empty host each pulse.
    /// Powered pumps push first; pipe/wire/pump runs pulse faster.
    fn tick_compound_flow(&mut self, dt: f32) {
        if !self.playing || self.menu.is_open() || self.dead {
            return;
        }
        self.compound_flow_wait -= dt;
        if self.compound_flow_wait > 0.0 {
            return;
        }
        if self.told_disconnect || !self.pending.is_empty() {
            self.compound_flow_wait = 0.42;
            return;
        }
        let eye = self.camera.eye;
        let focus = [
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        ];
        self.lamp_drain_wait = self.lamp_drain_wait.saturating_add(1);
        let drain_lamps = self.lamp_drain_wait >= 8;
        if drain_lamps {
            self.lamp_drain_wait = 0;
        }
        self.gen_charge_wait = self.gen_charge_wait.saturating_add(1);
        let charge_gens = self.gen_charge_wait >= 6;
        if charge_gens {
            self.gen_charge_wait = 0;
        }
        let player = PlayerBox {
            min: [1.0e6, 1.0e6, 1.0e6],
            max: [1.000_000_6e6, 1.000_001_8e6, 1.000_000_6e6],
        };
        let note_flow = |gui: &mut crate::gui::Gui, told: &mut bool| {
            if !*told {
                *told = true;
                gui.push_chat("flow");
            }
        };
        if let Some(cell) = plan_pump_pushes(&self.world, focus, 20, 1).into_iter().next() {
            self.send_edit(
                encode_place(cell.x, cell.y, cell.z, cell.block, player),
                TAG_PLACE,
                None,
            );
            self.compound_flow_wait = 0.22;
            note_flow(&mut self.gui, &mut self.compound_flow_told);
            return;
        }
        let spout_plan = plan_water_spouts(&self.world, focus, 20, 1);
        if !spout_plan.is_empty() {
            for change in spout_plan.into_iter().take(3) {
                self.send_edit(
                    encode_place(change.x, change.y, change.z, change.block, player),
                    TAG_PLACE,
                    None,
                );
            }
            self.compound_flow_wait = 0.35;
            note_flow(&mut self.gui, &mut self.compound_flow_told);
            return;
        }
        let drain_plan = plan_water_drains(&self.world, focus, 20, 1);
        if !drain_plan.is_empty() {
            for change in drain_plan.into_iter().take(2) {
                self.send_edit(
                    encode_place(change.x, change.y, change.z, change.block, player),
                    TAG_PLACE,
                    None,
                );
            }
            self.compound_flow_wait = 0.35;
            note_flow(&mut self.gui, &mut self.compound_flow_told);
            return;
        }
        if let Some(cell) = plan_compound_seeps(&self.world, focus, 20, 1).into_iter().next() {
            let conduit =
                is_pipe(cell.block.id) || is_wire(cell.block.id) || is_pump(cell.block.id);
            self.compound_flow_wait = if conduit { 0.22 } else { 0.42 };
            self.send_edit(
                encode_place(cell.x, cell.y, cell.z, cell.block, player),
                TAG_PLACE,
                None,
            );
            note_flow(&mut self.gui, &mut self.compound_flow_told);
            return;
        }
        let lamp_plan = plan_ember_lamps(&self.world, focus, 20, 4, drain_lamps);
        if !lamp_plan.is_empty() {
            for change in lamp_plan.into_iter().take(2) {
                self.send_edit(
                    encode_place(change.x, change.y, change.z, change.block, player),
                    TAG_PLACE,
                    None,
                );
            }
            self.compound_flow_wait = 0.35;
            note_flow(&mut self.gui, &mut self.compound_flow_told);
            return;
        }
        if charge_gens {
            let gen_plan = plan_ember_charges(&self.world, focus, 20, 1);
            if !gen_plan.is_empty() {
                for change in gen_plan.into_iter().take(1) {
                    self.send_edit(
                        encode_place(change.x, change.y, change.z, change.block, player),
                        TAG_PLACE,
                        None,
                    );
                }
                self.compound_flow_wait = 0.45;
                note_flow(&mut self.gui, &mut self.compound_flow_told);
                return;
            }
        }
        self.compound_flow_wait = 0.42;
    }

    /// Place water or ember into a looked-at compound host (grate, fence, …).
    fn try_compound_fill(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        let Some(kind) = Pack::builtin().compound_fill_of(id) else {
            return false;
        };
        let Some(fill) = fill_kind(kind) else {
            return false;
        };
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let host = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        let next = if is_battery(host.id) {
            if fill != FILL_EMBER || !compound_accepts_meta(host.id, host.metadata, fill) {
                return false;
            }
            if host.fill == FILL_NONE {
                battery_at(BATTERY_CHARGE)
            } else if host.fill == FILL_EMBER && battery_level(host) < BATTERY_MAX {
                battery_at((battery_level(host) + BATTERY_CHARGE).min(BATTERY_MAX))
            } else {
                return false;
            }
        } else if is_tank(host.id) {
            if !compound_accepts_meta(host.id, host.metadata, fill) {
                return false;
            }
            if host.fill == FILL_NONE {
                tank_at(fill, TANK_BUCKET)
            } else if host.fill == fill && tank_level(host) < TANK_MAX {
                tank_at(fill, (tank_level(host) + TANK_BUCKET).min(TANK_MAX))
            } else {
                // Let water_bucket fall through to pour a full water cell.
                return false;
            }
        } else if host.fill != FILL_NONE || !compound_accepts_meta(host.id, host.metadata, fill)
        {
            // Let water_bucket fall through to pour a full water cell.
            return false;
        } else {
            host.with_fill(fill)
        };
        let slot = self.hotbar.selected();
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                hit.block[0],
                hit.block[1],
                hit.block[2],
                next,
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        true
    }

    /// Clear compound fill from a host with the drain sponge.
    fn try_compound_drain(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_drain(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let host = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if !compound_host(host.id) || host.fill == FILL_NONE {
            return true;
        }
        let next = if is_battery(host.id) {
            Block::new("battery", 0)
        } else if is_tank(host.id) {
            Block::new("tank", 0)
        } else {
            host.with_fill(FILL_NONE)
        };
        let slot = self.hotbar.selected();
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                hit.block[0],
                hit.block[1],
                hit.block[2],
                next,
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        true
    }

    /// Empty hand: move fill from the looked-at host into the host past that face.
    ///
    /// Buffers move units (all into another same-kind buffer, one into a conduit).
    /// Underfilled same-kind buffers accept a top-up transfer.
    fn try_compound_transfer(&mut self) -> bool {
        if self.hotbar.slot(self.hotbar.selected()).is_some() {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let src = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if !compound_host(src.id) || src.fill == FILL_NONE {
            return false;
        }
        let dest = self
            .world
            .block_at(hit.place[0], hit.place[1], hit.place[2]);
        if !compound_host(dest.id) {
            return false;
        }
        if dest.fill != FILL_NONE
            && !(is_buffer(dest.id)
                && dest.fill == src.fill
                && buffer_level(dest) < buffer_max(dest.id))
        {
            return false;
        }
        if !compound_accepts_meta(dest.id, dest.metadata, src.fill) {
            return false;
        }
        let fill = src.fill;
        let src_units = if is_buffer(src.id) {
            buffer_level(src).max(1)
        } else {
            1
        };
        let clear_buffer = |id: &str| {
            if is_battery(id) {
                Block::new("battery", 0)
            } else {
                Block::new("tank", 0)
            }
        };
        let with_level = |id: &str, fill: u8, level: u8| {
            if is_battery(id) {
                battery_at(level)
            } else {
                tank_at(fill, level)
            }
        };
        let (src_next, dest_next) = if is_buffer(dest.id) {
            let room = if dest.fill == FILL_NONE {
                buffer_max(dest.id)
            } else {
                buffer_max(dest.id).saturating_sub(buffer_level(dest))
            };
            let moved = src_units.min(room).max(1);
            let dest_level = if dest.fill == FILL_NONE {
                moved
            } else {
                buffer_level(dest) + moved
            };
            let src_left = src_units.saturating_sub(moved);
            let src_next = if !is_buffer(src.id) {
                src.with_fill(FILL_NONE)
            } else if src_left == 0 {
                clear_buffer(src.id)
            } else {
                with_level(src.id, fill, src_left)
            };
            (src_next, with_level(dest.id, fill, dest_level))
        } else if is_buffer(src.id) {
            // One unit into a conduit; leave the rest in the buffer.
            let src_next = if src_units <= 1 {
                clear_buffer(src.id)
            } else {
                with_level(src.id, fill, src_units - 1)
            };
            (src_next, dest.with_fill(fill))
        } else {
            (src.with_fill(FILL_NONE), dest.with_fill(fill))
        };
        let (min, max) = body::player_box_bounds(self.camera.eye);
        let player = PlayerBox { min, max };
        self.send_edit(
            encode_place(
                hit.block[0],
                hit.block[1],
                hit.block[2],
                src_next,
                player,
            ),
            TAG_PLACE,
            None,
        );
        self.send_edit(
            encode_place(
                hit.place[0],
                hit.place[1],
                hit.place[2],
                dest_next,
                player,
            ),
            TAG_PLACE,
            None,
        );
        self.gui.push_chat("moved");
        true
    }

    /// Scoop water into an empty bucket, or pour a full one into air.
    fn try_bucket(&mut self) -> bool {
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        let pack = Pack::builtin();
        let empty = pack.is_bucket(id);
        let full = pack.is_water_bucket(id);
        if !empty && !full {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let slot = self.hotbar.selected();
        if empty {
            let block = self
                .world
                .block_at(hit.block[0], hit.block[1], hit.block[2]);
            if !self.bucket_fits(slot, "bucket", "water_bucket") {
                return true;
            }
            // Scoop from a water tank: take up to a bucket of units.
            if is_tank(block.id) && block.fill == FILL_WATER {
                let level = tank_level(block);
                if level == 0 {
                    return true;
                }
                let take = level.min(TANK_BUCKET);
                let next = if level <= take {
                    Block::new("tank", 0)
                } else {
                    tank_at(FILL_WATER, level - take)
                };
                let (min, max) = body::player_box_bounds(self.camera.eye);
                self.send_edit(
                    encode_place(
                        hit.block[0],
                        hit.block[1],
                        hit.block[2],
                        next,
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                return true;
            }
            if block.id != "water" {
                return true;
            }
            self.send_edit(
                encode_break(hit.block[0], hit.block[1], hit.block[2]),
                TAG_BREAK,
                Some(slot),
            );
            return true;
        }
        let [x, y, z] = hit.place;
        if !self.world.block_at(x, y, z).is_air()
            || !self.bucket_fits(slot, "water_bucket", "bucket")
        {
            return true;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(x, y, z, Block::new("water", 0), PlayerBox { min, max }),
            TAG_PLACE,
            Some(slot),
        );
        true
    }

    /// Place the held boat on the looked-at water, or pick a hull back up.
    fn try_place_boat(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if self.boat.aimed(eye, forward) && self.boat.take() {
            if !self.hotbar.collect("boat") {
                self.bag.collect("boat");
                self.bag_dirty = true;
            }
            self.hotbar_dirty = true;
            self.rebuild_boat();
            self.gui.push_chat("picked up");
            return true;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_boat(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "water" {
            return true;
        }
        if self.boat.place(hit.block[0], hit.block[1], hit.block[2]) {
            self.hotbar.consume_selected();
            self.hotbar_dirty = true;
            self.rebuild_boat();
            self.gui.push_chat("boat");
        } else {
            self.gui.push_chat("boat is out");
        }
        true
    }

    fn steer_boat(&mut self, dt: f32) {
        if self.boat_shot_path.is_some() {
            if let Some(seat) = self.boat.seat() {
                self.camera.eye = glam::Vec3::from(seat);
                self.camera.velocity_y = 0.0;
                self.camera.on_ground = true;
            }
            return;
        }
        let (forward, right) = self.camera.walk_axes();
        let mut wish = glam::Vec3::ZERO;
        if self.input.forward {
            wish += forward;
        }
        if self.input.back {
            wish -= forward;
        }
        if self.input.right {
            wish += right;
        }
        if self.input.left {
            wish -= right;
        }
        let rowing = wish.length_squared() > 0.0;
        if rowing {
            wish = wish.normalize() * crate::boat::SPEED;
        }
        let moved = self.boat.steer(dt, wish.x, wish.z, &self.world);
        if moved {
            self.rebuild_boat();
        }
        let goal = if moved || rowing { 1.0 } else { 0.0 };
        self.boat_kick += (goal - self.boat_kick) * (dt * 8.0).min(1.0);
        self.boat_show += (goal - self.boat_show) * (dt * 10.0).min(1.0);
        if moved {
            self.boat_stroke -= dt;
            if self.boat_stroke <= 0.0 {
                self.boat_stroke = 0.48;
                if let Some(origin) = self.boat.origin() {
                    self.play_world(crate::audio::Sound::Paddle, origin);
                }
            }
            self.bob_amount = (self.bob_amount + dt * 3.5).min(0.55);
        } else {
            self.boat_stroke = 0.0;
        }
        if let Some(seat) = self.boat.seat() {
            self.camera.eye = glam::Vec3::from(seat);
            self.camera.velocity_y = 0.0;
            self.camera.on_ground = true;
        }
    }

    fn clear_boat_feel(&mut self) {
        self.boat_kick = 0.0;
        self.boat_show = 0.0;
        self.boat_stroke = 0.0;
    }

    /// Place the held minecart on the looked-at rail, or pick a hull back up.
    fn try_place_minecart(&mut self) -> bool {
        let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
        let forward = self.camera.forward();
        let forward = [forward.x, forward.y, forward.z];
        if self.cart.aimed(eye, forward) && self.cart.take() {
            if !self.hotbar.collect("minecart") {
                self.bag.collect("minecart");
                self.bag_dirty = true;
            }
            self.hotbar_dirty = true;
            self.rebuild_minecart();
            self.gui.push_chat("picked up");
            return true;
        }
        let Some(id) = self.hotbar.slot(self.hotbar.selected()) else {
            return false;
        };
        if !Pack::builtin().is_minecart(id) {
            return false;
        }
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return true;
        };
        let block = self
            .world
            .block_at(hit.block[0], hit.block[1], hit.block[2]);
        if block.id != "rail" && block.id != "powered_rail" {
            self.gui.push_chat("need rail");
            return true;
        }
        if self.cart.place(hit.block[0], hit.block[1], hit.block[2]) {
            self.hotbar.consume_selected();
            self.hotbar_dirty = true;
            self.rebuild_minecart();
            self.gui.push_chat("cart");
        } else {
            self.gui.push_chat("cart is out");
        }
        true
    }

    fn steer_minecart(&mut self, dt: f32) {
        if self.cart_shot_path.is_some() {
            if let Some(seat) = self.cart.seat() {
                self.camera.eye = glam::Vec3::from(seat);
                self.camera.velocity_y = 0.0;
                self.camera.on_ground = true;
            }
            return;
        }
        let (forward, right) = self.camera.walk_axes();
        let mut wish = glam::Vec3::ZERO;
        if self.input.forward {
            wish += forward;
        }
        if self.input.back {
            wish -= forward;
        }
        if self.input.right {
            wish += right;
        }
        if self.input.left {
            wish -= right;
        }
        let riding = wish.length_squared() > 0.0;
        if riding {
            wish = wish.normalize() * crate::minecart::SPEED;
        }
        let moved = self.cart.steer(dt, wish.x, wish.z, &self.world);
        if moved {
            self.rebuild_minecart();
        }
        let boosted = self.cart.boosted(&self.world);
        let goal = if moved || riding {
            if boosted {
                1.0
            } else {
                0.85
            }
        } else {
            0.0
        };
        self.cart_kick += (goal - self.cart_kick) * (dt * 8.0).min(1.0);
        self.cart_show += (goal - self.cart_show) * (dt * 10.0).min(1.0);
        if moved {
            self.cart_stroke -= dt;
            if self.cart_stroke <= 0.0 {
                self.cart_stroke = if boosted { 0.22 } else { 0.36 };
                if let Some(origin) = self.cart.origin() {
                    self.play_world(crate::audio::Sound::Step, origin);
                }
            }
            self.bob_amount = (self.bob_amount + dt * if boosted { 5.5 } else { 4.0 }).min(0.55);
        } else {
            self.cart_stroke = 0.0;
        }
        if let Some(seat) = self.cart.seat() {
            self.camera.eye = glam::Vec3::from(seat);
            self.camera.velocity_y = 0.0;
            self.camera.on_ground = true;
        }
    }

    fn clear_cart_feel(&mut self) {
        self.cart_kick = 0.0;
        self.cart_show = 0.0;
        self.cart_stroke = 0.0;
    }

    fn steer_pig(&mut self, dt: f32) {
        let (forward, right) = self.camera.walk_axes();
        let mut wish = glam::Vec3::ZERO;
        if self.input.forward {
            wish += forward;
        }
        if self.input.back {
            wish -= forward;
        }
        if self.input.right {
            wish += right;
        }
        if self.input.left {
            wish -= right;
        }
        if wish.length_squared() > 0.0 {
            wish = wish.normalize() * crate::mobs::RIDE_SPEED;
        }
        if self.pig.steer(dt, wish.x, wish.z, &self.world) {
            self.rebuild_pig();
        }
        if let Some(seat) = self.pig.seat() {
            self.camera.eye = glam::Vec3::from(seat);
            self.camera.velocity_y = 0.0;
            self.camera.on_ground = true;
        }
    }

    fn steer_horse(&mut self, dt: f32) {
        if self.horse_shot_path.is_some() {
            if let Some(seat) = self.horse.seat() {
                self.camera.eye = glam::Vec3::from(seat);
                self.camera.velocity_y = 0.0;
                self.camera.on_ground = true;
            }
            return;
        }
        let (forward, right) = self.camera.walk_axes();
        let mut wish = glam::Vec3::ZERO;
        if self.input.forward {
            wish += forward;
        }
        if self.input.back {
            wish -= forward;
        }
        if self.input.right {
            wish += right;
        }
        if self.input.left {
            wish -= right;
        }
        let galloping = wish.length_squared() > 0.0;
        if galloping {
            wish = wish.normalize() * crate::mobs::HORSE_SPEED;
        }
        let moved = self.horse.steer(dt, wish.x, wish.z, &self.world);
        if moved {
            self.rebuild_horse();
        }
        let goal = if moved || galloping { 1.0 } else { 0.0 };
        self.horse_kick += (goal - self.horse_kick) * (dt * 8.0).min(1.0);
        self.horse_show += (goal - self.horse_show) * (dt * 10.0).min(1.0);
        if moved {
            self.horse_stroke -= dt;
            if self.horse_stroke <= 0.0 {
                self.horse_stroke = 0.28;
                self.play_world(crate::audio::Sound::Step, self.horse.origin());
            }
            self.bob_amount = (self.bob_amount + dt * 4.5).min(0.6);
        } else {
            self.horse_stroke = 0.0;
        }
        if let Some(seat) = self.horse.seat() {
            self.camera.eye = glam::Vec3::from(seat);
            self.camera.velocity_y = 0.0;
            self.camera.on_ground = true;
        }
    }

    fn clear_horse_feel(&mut self) {
        self.horse_kick = 0.0;
        self.horse_show = 0.0;
        self.horse_stroke = 0.0;
    }

    /// A right click asks `events.use_entity`, then `events.use_block`, then `events.use_item`.
    ///
    /// False from any hook consumes the click, so the built-in eat, place, feed,
    /// saddle, and mace swing do not run. A consumed click sends the first
    /// `world.set_block` from those hooks as a place. Other held items fall through.
    fn try_script_use(&mut self) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let item = self
            .hotbar
            .slot(self.hotbar.selected())
            .unwrap_or("")
            .to_string();
        let mut cancel = false;
        if let Some(kind) = self.aimed_kind() {
            if !self.hooks.use_entity(&mut self.world, &item, kind, &view) {
                cancel = true;
            }
        }
        if cancel {
            self.flush_script_writes();
            self.flush_script_screens();
            return true;
        }
        if let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) {
            let id = self
                .world
                .block_at(hit.block[0], hit.block[1], hit.block[2])
                .id
                .to_string();
            if !id.is_empty()
                && id != "air"
                && !self.hooks.use_block(
                    &mut self.world,
                    &item,
                    &id,
                    hit.block[0],
                    hit.block[1],
                    hit.block[2],
                    &view,
                )
            {
                cancel = true;
            }
        }
        if !cancel && !item.is_empty() && !self.hooks.use_item(&mut self.world, &item, &view) {
            cancel = true;
        }
        if cancel {
            self.flush_script_writes();
        } else {
            let _ = self.hooks.drain_writes();
        }
        self.flush_script_screens();
        cancel
    }

    /// Open any GUIAPI screen a use hook asked for with `gui.open`.
    fn flush_script_screens(&mut self) {
        let names = self.hooks.drain_screen_opens();
        if names.is_empty() {
            return;
        }
        self.gui
            .sync_built(&rebuild_world::Pack::builtin().gui_screens());
        for name in names {
            if self.gui.open_built(&name) {
                self.clear_motion();
                self.set_capture(false);
            }
        }
    }

    /// The name under the cross: the closest entity, fluid, or block.
    fn inspect_id(&self) -> Option<String> {
        let eye = self.camera.eye;
        let forward = self.camera.forward();
        let block = look::block_along(eye, forward, &self.world);
        let fluid = look::fluid_along(eye, forward, &self.world);
        let entity = self.entity_along(eye, forward);
        let entity = entity
            .as_ref()
            .map(|(id, distance)| (id.as_str(), *distance));
        let id = look::nearest_label([entity, fluid, block])?;
        // Compound look tips: tank level, pump power, filled pipe/wire kind.
        if let Some(tip) = self.compound_look_tip(id, eye, forward) {
            return Some(tip);
        }
        Some(id.to_string())
    }

    /// HUD tip when looking at a compound host with extra state.
    fn compound_look_tip(
        &self,
        id: &str,
        eye: glam::Vec3,
        forward: glam::Vec3,
    ) -> Option<String> {
        let target = look::raycast(eye, forward, &self.world)?;
        let block = self
            .world
            .block_at(target.block[0], target.block[1], target.block[2]);
        if block.id != id {
            return None;
        }
        if is_battery(block.id) {
            let level = battery_level(block);
            let kind = if block.fill == FILL_EMBER {
                "ember"
            } else {
                "empty"
            };
            return Some(format!("Battery  {kind} {level}/{BATTERY_MAX}"));
        }
        if is_lamp(block.id) {
            let power = if lamp_on(block.metadata) { "on" } else { "off" };
            return Some(format!("Lamp  {power}"));
        }
        if is_generator(block.id) {
            let power = if generator_on(block.metadata) {
                "on"
            } else {
                "off"
            };
            let fueled = self.campfire_beside(target.block);
            return Some(if fueled && generator_on(block.metadata) {
                format!("Gen  {power}  fueled")
            } else {
                format!("Gen  {power}")
            });
        }
        if is_spout(block.id) {
            let power = if pump_on(block.metadata) { "open" } else { "closed" };
            let dir = pump_dir(block.metadata);
            let intake = self.world.block_at(
                target.block[0] - dir[0],
                target.block[1] - dir[1],
                target.block[2] - dir[2],
            );
            let spray = is_tank(intake.id) && intake.fill == FILL_WATER && tank_level(intake) > 0;
            return Some(if spray && pump_on(block.metadata) {
                format!("Spout  {power}  spray")
            } else {
                format!("Spout  {power}")
            });
        }
        if is_drain(block.id) {
            let power = if pump_on(block.metadata) { "open" } else { "closed" };
            let dir = pump_dir(block.metadata);
            let intake = self.world.block_at(
                target.block[0] + dir[0],
                target.block[1] + dir[1],
                target.block[2] + dir[2],
            );
            let wet = intake.id == "water";
            return Some(if wet && pump_on(block.metadata) {
                format!("Drain  {power}  pull")
            } else {
                format!("Drain  {power}")
            });
        }
        if is_tank(block.id) {
            let level = tank_level(block);
            let kind = match block.fill {
                FILL_WATER => "water",
                FILL_EMBER => "ember",
                _ => "empty",
            };
            return Some(format!("Tank  {kind} {level}/{TANK_MAX}"));
        }
        if is_pump(block.id) {
            let power = if pump_on(block.metadata) { "on" } else { "off" };
            return match block.fill {
                FILL_WATER => Some(format!("Pump  {power}  water")),
                FILL_EMBER => Some(format!("Pump  {power}  ember")),
                _ => Some(format!("Pump  {power}")),
            };
        }
        if (is_pipe(block.id) || is_wire(block.id)) && block.fill != FILL_NONE {
            let kind = fill_name(block.fill).unwrap_or("fill");
            let name = if is_pipe(block.id) { "Pipe" } else { "Wire" };
            return Some(format!("{name}  {kind}"));
        }
        None
    }

    /// The closest body the look ray meets, within the same reach as a block.
    fn entity_along(&self, eye: glam::Vec3, forward: glam::Vec3) -> Option<(String, f32)> {
        fn note<'a>(best: &mut Option<(&'a str, f32)>, id: &'a str, hit: Option<f32>) {
            let Some(distance) = hit else {
                return;
            };
            if best.is_none_or(|(_, so_far)| distance < so_far) {
                *best = Some((id, distance));
            }
        }
        let mut best: Option<(&str, f32)> = None;
        let bodies = [
            ("sheep", self.sheep.bounds()),
            ("pig", self.pig.bounds()),
            ("horse", self.horse.bounds()),
            ("zombie", self.zombie.bounds()),
            ("breeze", self.breeze.bounds()),
            ("copper_golem", self.golem.bounds()),
            ("wolf", self.wolf.bounds()),
            ("cat", self.cat.bounds()),
            ("parrot", self.parrot.bounds()),
            ("frog", self.frog.bounds()),
            ("axolotl", self.axolotl.bounds()),
            ("chicken", self.chicken.bounds()),
            ("bee", self.bee.bounds()),
            ("fox", self.fox.bounds()),
            ("goat", self.goat.bounds()),
        ];
        for (id, (min, max)) in bodies {
            note(&mut best, id, look::ray_aabb(eye, forward, min, max));
        }
        for zombie in self.night.iter() {
            if !zombie.living() {
                continue;
            }
            let (min, max) = zombie.bounds();
            note(&mut best, "zombie", look::ray_aabb(eye, forward, min, max));
        }
        if let Some([x, y, z]) = self.boat.origin() {
            note(
                &mut best,
                "boat",
                look::ray_aabb(
                    eye,
                    forward,
                    [x - 0.8, y, z - 0.8],
                    [x + 0.8, y + 0.7, z + 0.8],
                ),
            );
        }
        for guest in &self.guests {
            if !guest.alive() {
                continue;
            }
            let [x, y, z] = guest.origin();
            note(
                &mut best,
                guest.id(),
                look::ray_aabb(
                    eye,
                    forward,
                    [x - 0.28, y, z - 0.28],
                    [x + 0.28, y + 1.95, z + 0.28],
                ),
            );
        }
        for (id, min, max) in self.drops.boxes() {
            note(&mut best, id, look::ray_aabb(eye, forward, min, max));
        }
        best.map(|(id, distance)| (id.to_string(), distance))
    }

    /// The mob the look ray hits, in the order a right click would use one.
    /// Closest living script Guest under the crosshair, if any.
    fn aimed_guest(&self, eye: [f32; 3], forward: [f32; 3]) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, guest) in self.guests.iter().enumerate() {
            if !guest.alive() || !guest.aimed(eye, forward) {
                continue;
            }
            let [x, y, z] = guest.origin();
            let dx = x - eye[0];
            let dy = y + 1.0 - eye[1];
            let dz = z - eye[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((index, dist));
            }
        }
        best.map(|(index, _)| index)
    }

    fn aimed_kind(&self) -> Option<&'static str> {
        let eye = self.camera.eye.to_array();
        let forward = self.camera.forward().to_array();
        if self.zombie.aimed(eye, forward) || self.night.aimed(eye, forward).is_some() {
            return Some("zombie");
        }
        if self.breeze.aimed(eye, forward) {
            return Some("breeze");
        }
        if self.fox.aimed(eye, forward) {
            return Some("fox");
        }
        if self.frog.aimed(eye, forward) {
            return Some("frog");
        }
        if self.parrot.aimed(eye, forward) {
            return Some("parrot");
        }
        if self.cat.aimed(eye, forward) {
            return Some("cat");
        }
        if self.wolf.aimed(eye, forward) {
            return Some("wolf");
        }
        if self.pig.aimed(eye, forward) {
            return Some("pig");
        }
        if self.boat.aimed(eye, forward) {
            return Some("boat");
        }
        if self.cart.aimed(eye, forward) {
            return Some("minecart");
        }
        None
    }

    /// The place the use hook asked for. One cell, the first `world.set_block`.
    fn flush_script_writes(&mut self) {
        let Some(change) = self.hooks.drain_writes().into_iter().next() else {
            return;
        };
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                change.x,
                change.y,
                change.z,
                change.block,
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Apply hook-time `entity.spawn` / `entity.despawn` results to Guest bodies.
    fn flush_live_spawns(&mut self) {
        let events = self.hooks.drain_live_spawns();
        if events.is_empty() {
            return;
        }
        let pack = rebuild_world::Pack::builtin();
        let mut dirty = false;
        for event in events {
            match event {
                Ok((id, x, y, z)) => {
                    if !self.hooks.entity_spawn(&mut self.world, &id, x, y, z) {
                        let _ = pack.take_spawn(&id, x, y, z);
                        continue;
                    }
                    crate::mobs::Guest::push_live(
                        &mut self.guests,
                        id.clone(),
                        x,
                        y,
                        z,
                        pack.entity_model(&id),
                        pack.entity_hostile(&id),
                    );
                    dirty = true;
                }
                Err((id, x, y, z)) => {
                    let [ox, oy, oz] = [x as f32 + 0.5, y as f32, z as f32 + 0.5];
                    if self.hooks.entity_death(
                        &mut self.world,
                        &id,
                        ox.floor() as i32,
                        oy.floor() as i32,
                        oz.floor() as i32,
                    ) && crate::mobs::Guest::take_live(&mut self.guests, &id, x, y, z)
                    {
                        dirty = true;
                    }
                }
            }
        }
        if dirty {
            self.rebuild_guests();
        }
    }

    /// Right-click a script spawn egg to place that mob in front of the player.
    fn try_spawn_egg(&mut self) -> bool {
        let item = self
            .hotbar
            .slot(self.hotbar.selected())
            .unwrap_or("")
            .to_string();
        if item.is_empty() {
            return false;
        }
        let pack = rebuild_world::Pack::builtin();
        let Some(kind) = pack.entity_for_spawn_egg(&item) else {
            return false;
        };
        let forward = self.camera.forward();
        let x = (self.camera.eye.x + forward.x * 2.0).floor() as i32;
        let y = (self.camera.eye.y - 1.0).floor().max(1.0) as i32;
        let z = (self.camera.eye.z + forward.z * 2.0).floor() as i32;
        if !self.hooks.entity_spawn(&mut self.world, &kind, x, y, z) {
            return true;
        }
        if pack.note_spawn(&kind, x, y, z, "").is_err() {
            return true;
        }
        let model = pack.entity_model(&kind);
        let hostile = pack.entity_hostile(&kind);
        crate::mobs::Guest::push_live(&mut self.guests, kind, x, y, z, model, hostile);
        self.rebuild_guests();
        self.gui.push_chat("spawn");
        true
    }

    pub fn place_targeted(&mut self) {
        if self.try_script_use() {
            return;
        }
        if self.try_use_pump() {
            return;
        }
        if self.try_spawn_egg() {
            return;
        }
        if self.holding_map() {
            self.map_open = true;
            self.clear_motion();
            self.set_capture(false);
            return;
        }
        if self.try_mace() || self.try_horn() || self.try_brush() {
            return;
        }
        if self.holding_spyglass() {
            self.zoom_mouse = true;
            return;
        }
        if self.try_equip()
            || self.try_omen()
            || self.try_fox()
            || self.try_frog()
            || self.try_axolotl()
            || self.try_chicken()
            || self.try_parrot()
            || self.try_cat()
            || self.try_wolf()
            || self.try_horse()
            || self.try_cook_heat()
            || self.try_blast_heat()
            || self.try_furnace_heat()
            || self.try_shape_pot()
            || self.try_eat()
            || self.try_key()
            || self.try_wind()
            || self.try_pearl()
            || self.try_eye()
            || self.try_arrow()
            || self.try_fish()
            || self.try_plant_seeds()
            || self.try_till()
            || self.try_fertilize()
        {
            return;
        }
        if self.try_saddle()
            || self.try_compound_fill()
            || self.try_compound_drain()
            || self.try_compound_transfer()
            || self.try_bucket()
            || self.try_place_boat()
            || self.try_place_minecart()
        {
            return;
        }
        let Some(held) = self.hotbar.selected_block() else {
            return;
        };
        self.shape_pick.clamp_to(Some(held.id));
        let mut block = self
            .shape_pick
            .resolve(held.id)
            .unwrap_or(held);
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return;
        };
        if block.id == "slab" && hit.upper {
            block = Block::new("slab", 1);
        }
        if block.id == "torch"
            || block.id == "end_rod"
            || block.id == "chain"
            || block.id == "hanging_sign"
            || block.id == "lightning_rod"
        {
            block = Block::new(block.id, look::torch_facing(hit.axis, hit.normal));
        }
        if block.id == "pump" {
            // Face along the look — pushes away from the player. Starts off (valve).
            block = Block::new("pump", pump_meta(pump_facing_from_look(self.camera.forward()), false));
        }
        if block.id == "spout" {
            // Open by default; faces along look so water dumps away from you.
            block = Block::new(
                "spout",
                pump_meta(pump_facing_from_look(self.camera.forward()), true),
            );
        }
        if block.id == "drain" {
            // Open by default; faces along look so intake faces the water ahead.
            block = Block::new(
                "drain",
                pump_meta(pump_facing_from_look(self.camera.forward()), true),
            );
        }
        if block.id == "generator" {
            // Starts running so a fresh place begins charging.
            block = generator_at(true);
        }
        if block.id == "painting"
            || block.id == "vine"
            || block.id == "item_frame"
            || block.id == "shelf"
        {
            let Some(facing) = look::painting_facing(hit.axis, hit.normal) else {
                return;
            };
            let metadata = if block.id == "shelf" {
                rebuild_world::shelf_meta(facing, 0)
            } else {
                facing
            };
            block = Block::new(block.id, metadata);
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        let slot = self.hotbar.selected();
        self.send_edit(
            encode_place(
                hit.place[0],
                hit.place[1],
                hit.place[2],
                block,
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
    }

    /// Queue one request. The world changes only when the server's cell reply arrives.
    fn send_edit(&mut self, body: Vec<u8>, kind: u8, slot: Option<usize>) {
        if self.told_disconnect || !self.pending.is_empty() {
            return;
        }
        self.pending.push_back(PendingEdit { kind, slot });
        if self.link.send(body).is_err() {
            self.pending.pop_back();
            self.note_disconnect();
        }
    }

    fn poll_server(&mut self) {
        loop {
            while let Some(line) = self.link.try_chat() {
                self.gui.push_chat(&line);
            }
            if let Some(peers) = self.link.try_peers() {
                self.others = peers
                    .into_iter()
                    .map(|peer| crate::peers::Other {
                        eye: [peer.x, peer.y, peer.z],
                        yaw: peer.yaw,
                    })
                    .collect();
                self.rebuild_peers();
            }
            match self.link.try_recv() {
                Ok(Some(cell)) => self.apply_server_cell(cell),
                Ok(None) => break,
                Err(()) => {
                    self.note_disconnect();
                    break;
                }
            }
        }
    }

    fn apply_server_cell(&mut self, cell: rebuild_world::CellUpdate) {
        let pending = self.pending.pop_front();
        if !cell.changed {
            if pending.is_some() {
                self.egg_hop = None;
                self.crystal_blast = None;
            }
            return;
        }
        let before = self.world.block_at(cell.x, cell.y, cell.z);
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: cell.x,
                y: cell.y,
                z: cell.z,
                block: cell.block,
            }],
        );
        let kind = pending.as_ref().map(|edit| edit.kind).unwrap_or(cell.kind);
        if kind == TAG_BREAK && !before.is_air() {
            let scooped = before.id == "water"
                && pending
                    .as_ref()
                    .and_then(|edit| edit.slot)
                    .is_some_and(|slot| {
                        self.hotbar.slot(slot) == Some("bucket")
                            && self.exchange_bucket(slot, "bucket", "water_bucket")
                    });
            if scooped {
                self.gui.push_chat("filled");
            } else if let Some(slot) = pending.as_ref().and_then(|edit| edit.slot) {
                if self.hotbar.wear(slot, before.id) {
                    self.hotbar_dirty = true;
                }
            }
            let origin = [
                cell.x as f32 + 0.5,
                cell.y as f32 + 0.5,
                cell.z as f32 + 0.5,
            ];
            self.play_world(crate::audio::break_for(before.id), origin);
            self.break_kick = 1.0;
            self.drops.puff(origin, crate::drops::chip_color(before.id));
            self.rebuild_drops();
            let remains = shape_of(cell.block).is_full_cube();
            let (dropped, amount) = rebuild_world::harvest_of(before.id);
            let mut id = dropped.to_string();
            let mut count = u32::from(amount);
            if self.hooks.listens_break_drop() {
                let slots = hotbar_slots(&self.hotbar);
                let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                if let Some(chosen) =
                    self.hooks
                        .break_drop(&mut self.world, before.id, cell.block.id, &view)
                {
                    if chosen.is_empty() || chosen == "air" {
                        id.clear();
                        count = 0;
                    } else if Pack::builtin().contains_item(&chosen) {
                        if chosen != id {
                            count = 1;
                        }
                        id = chosen;
                    }
                }
            }
            let eaten = self
                .cake_eat
                .take()
                .is_some_and(|at| at == [cell.x, cell.y, cell.z]);
            let hop_dest = self.egg_hop.take().and_then(|(from, dest)| {
                if from == [cell.x, cell.y, cell.z] && before.id == "dragon_egg" {
                    Some(dest)
                } else {
                    None
                }
            });
            if before.id == "lectern" && before.metadata & 1 == 1 && self.hotbar.collect("book") {
                self.hotbar_dirty = true;
            }
            if before.id == "jukebox"
                && before.metadata & 1 == 1
                && self.hotbar.collect("music_disc")
            {
                self.hotbar_dirty = true;
            }
            if before.id == "chiseled_bookshelf" {
                let stored = before.metadata.min(6);
                for _ in 0..stored {
                    if !self.hotbar.collect("book") {
                        break;
                    }
                    self.hotbar_dirty = true;
                }
            }
            if before.id == "composter"
                && before.metadata >= hotbar::COMPOST_READY
                && self.hotbar.collect("bone_meal")
            {
                self.hotbar_dirty = true;
            }
            if before.id == "beehive"
                && before.metadata >= hotbar::HONEY_READY
                && self.hotbar.collect("honey")
            {
                self.hotbar_dirty = true;
            }
            if before.id == "wheat_crop" {
                id.clear();
                count = 0;
                let cell_at = [cell.x, cell.y, cell.z];
                if before.metadata >= 3 {
                    self.drops.spawn("wheat", cell_at, remains);
                    self.gui.push_chat("harvest");
                }
                self.drops.spawn("wheat_seeds", cell_at, remains);
                self.rebuild_drops();
            }
            if before.id == "beehive" && self.hotbar.slot(self.hotbar.selected()).is_none() {
                self.bee.anger();
                self.rebuild_bee();
                self.gui.push_chat("angry");
            }
            if eaten {
                self.taste_cake();
                self.gui.push_chat("gone");
            }
            if hop_dest.is_some() {
                self.gui.push_chat("the egg hops");
            }
            let blasted = self
                .crystal_blast
                .take()
                .is_some_and(|from| from == [cell.x, cell.y, cell.z] && before.id == "end_crystal");
            if blasted {
                self.gui.push_chat("boom");
                if body::crystal_reaches(self.camera.eye, [cell.x, cell.y, cell.z]) {
                    self.hurt(1);
                }
            }
            if !scooped && !eaten && hop_dest.is_none() && !blasted && !id.is_empty() {
                for _ in 0..count {
                    self.drops.spawn(&id, [cell.x, cell.y, cell.z], remains);
                }
                self.rebuild_drops();
            }
            if let Some(color) = particle_color(&self.hooks) {
                let origin = crate::drops::center_for([cell.x, cell.y, cell.z], remains);
                self.drops.burst(origin, color);
                self.rebuild_drops();
            }
            if let Some(dest) = hop_dest {
                let player = PlayerBox {
                    min: [40.0, 40.0, 40.0],
                    max: [40.6, 41.8, 40.6],
                };
                self.send_edit(
                    encode_place(
                        dest[0],
                        dest[1],
                        dest[2],
                        Block::new("dragon_egg", 0),
                        player,
                    ),
                    TAG_PLACE,
                    None,
                );
            }
        }
        if kind == TAG_PLACE {
            if hinge(before.id) && before.id == cell.block.id {
                let word = if cell.block.metadata & 1 == 1 {
                    "open"
                } else {
                    "closed"
                };
                self.gui.push_chat(word);
            }
            self.play_world(
                crate::audio::Sound::Place,
                [
                    cell.x as f32 + 0.5,
                    cell.y as f32 + 0.5,
                    cell.z as f32 + 0.5,
                ],
            );
            self.slot_punch = 1.0;
            if before.id == "chiseled_bookshelf"
                && cell.block.id == "chiseled_bookshelf"
                && cell.block.metadata < before.metadata
                && self.hotbar.collect("book")
            {
                self.hotbar_dirty = true;
            }
            if before.id == "composter"
                && before.metadata >= hotbar::COMPOST_READY
                && cell.block.id == "composter"
                && cell.block.metadata == 0
                && self.hotbar.collect("bone_meal")
            {
                self.hotbar_dirty = true;
            }
            if before.id == "beehive"
                && before.metadata >= hotbar::HONEY_READY
                && cell.block.id == "beehive"
                && cell.block.metadata == 0
                && self.hotbar.collect("honey")
            {
                self.hotbar_dirty = true;
            }
            if cell.block.id == "copper_bulb" && before.id != "copper_bulb" {
                let under = self.world.block_at(cell.x, cell.y - 1, cell.z);
                if crate::mobs::copper_base(under.id) {
                    self.golem
                        .summon(crate::mobs::summon_feet([cell.x, cell.y, cell.z]));
                    self.rebuild_golem();
                    self.gui.push_chat("golem");
                }
            }
            if let Some(slot) = pending.and_then(|edit| edit.slot) {
                let buffer_level_changed = is_buffer(cell.block.id)
                    && before.id == cell.block.id
                    && buffer_level(before) != buffer_level(cell.block);
                let fill_changed = before.id == cell.block.id
                    && before.fill != cell.block.fill
                    && compound_host(cell.block.id);
                if fill_changed || buffer_level_changed {
                    let rose = buffer_level(cell.block) > buffer_level(before)
                        || (before.fill == FILL_NONE && cell.block.fill != FILL_NONE);
                    let fell = buffer_level(cell.block) < buffer_level(before)
                        || (before.fill != FILL_NONE && cell.block.fill == FILL_NONE);
                    if rose {
                        // Filled / topped up a host (including buffer level++).
                        if self.hotbar.slot(slot) == Some("water_bucket") {
                            if self.exchange_bucket(slot, "water_bucket", "bucket") {
                                self.gui.push_chat(if is_tank(cell.block.id) {
                                    "tank"
                                } else {
                                    "waterlogged"
                                });
                            }
                        } else if self.hotbar.slot(slot) == Some("bucket") {
                            // Scoop path should not land here; ignore.
                        } else {
                            self.hotbar.consume_at(slot);
                            self.hotbar_dirty = true;
                            if is_battery(cell.block.id) {
                                self.gui.push_chat("battery");
                            } else if let Some(name) = fill_name(cell.block.fill) {
                                self.gui.push_chat(name);
                            }
                        }
                    } else if fell {
                        // Drained / scooped a host — refund when holding an empty bucket.
                        if before.fill == FILL_WATER
                            && self.hotbar.slot(slot) == Some("bucket")
                            && self.exchange_bucket(slot, "bucket", "water_bucket")
                        {
                            self.gui.push_chat(if is_tank(before.id) {
                                "scooped"
                            } else {
                                "drained"
                            });
                        } else {
                            let refund = match before.fill {
                                FILL_WATER => "water_bucket",
                                FILL_EMBER => "ember_charge",
                                _ => "",
                            };
                            if !refund.is_empty() && self.hotbar.collect(refund) {
                                self.hotbar_dirty = true;
                            }
                            self.gui.push_chat(if is_battery(before.id) {
                                "discharged"
                            } else {
                                "drained"
                            });
                        }
                    }
                } else if cell.block.id == "water" && self.hotbar.slot(slot) == Some("water_bucket") {
                    if self.exchange_bucket(slot, "water_bucket", "bucket") {
                        self.gui.push_chat("emptied");
                    }
                } else if before.id == "cauldron"
                    && cell.block.id == "cauldron"
                    && cell.block.metadata > before.metadata
                    && self.hotbar.slot(slot) == Some("water_bucket")
                {
                    if !self.exchange_bucket(slot, "water_bucket", "bucket") {
                        self.hotbar.consume_at(slot);
                        self.hotbar_dirty = true;
                    }
                } else if before.id == "cauldron"
                    && cell.block.id == "cauldron"
                    && cell.block.metadata < before.metadata
                    && self.hotbar.slot(slot) == Some("bucket")
                {
                    if !self.exchange_bucket(slot, "bucket", "water_bucket") {
                        self.hotbar.consume_at(slot);
                        self.hotbar_dirty = true;
                    }
                } else {
                    self.hotbar.consume_at(slot);
                    self.hotbar_dirty = true;
                }
            } else if before.id == cell.block.id
                && before.fill == FILL_NONE
                && cell.block.fill != FILL_NONE
                && compound_host(cell.block.id)
            {
                // Pipe seep — no held item. Soft mote so flow reads in the world.
                let origin = [
                    cell.x as f32 + 0.5,
                    cell.y as f32 + 0.55,
                    cell.z as f32 + 0.5,
                ];
                let color = match cell.block.fill {
                    FILL_WATER => [0.28, 0.48, 0.9],
                    FILL_EMBER => [1.0, 0.42, 0.12],
                    _ => [0.75, 0.75, 0.75],
                };
                self.drops.puff(origin, color);
                self.rebuild_drops();
            }
        }
        self.chunk_bake.touch(cell.x, cell.z);
    }

    /// Path-trace the sound to each ear, then play the stereo wave.
    fn play_world(&mut self, sound: crate::audio::Sound, source: [f32; 3]) {
        self.play_world_soft(sound, source, 1.0);
    }

    fn play_world_soft(&mut self, sound: crate::audio::Sound, source: [f32; 3], level: f32) {
        let eye = self.camera.eye.to_array();
        let forward = self.camera.forward().to_array();
        let right = self.camera.walk_axes().1.to_array();
        let heard = {
            let world = &self.world;
            crate::audio::hear(source, eye, forward, right, |x, y, z| {
                shape_of(world.block_at(x, y, z)).is_full_cube()
            })
        };
        self.audio.play_heard(sound, heard.attenuate(level));
    }

    fn note_disconnect(&mut self) {
        self.pending.clear();
        if self.told_disconnect {
            return;
        }
        self.told_disconnect = true;
        eprintln!("lost the server. Breaks and places stop here.");
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Auto-Play, freeze the zombie at home, and aim the camera for a proof shot.
    pub fn arm_zombie_shot(&mut self, path: std::path::PathBuf) {
        self.shot_path = Some(path);
        self.shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        // Proven clear stairs drop; zombie on the path under the crosshair.
        self.camera = crate::camera::Camera::drop_by_the_stairs();
        let at = [8.5_f32, 1.0, 13.2];
        self.zombie.rouse(at);
        self.zombie.hold();
        self.zombie.face(0.0);
        let eye = self.camera.eye;
        let target = glam::Vec3::new(at[0], at[1] + 1.0, at[2]);
        self.camera = crate::camera::Camera::looking_at(eye, target);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.posed_eye = self.camera.eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.rebuild_zombie();
        let _ = self.window.focus_window();
    }

    /// Open the Godot-style settings screen and capture a window PNG.
    pub fn arm_settings_shot(&mut self, path: std::path::PathBuf) {
        self.settings_shot_path = Some(path);
        self.settings_shot_frames = 0;
        self.playing = false;
        self.menu.show_settings();
        let _ = self.window.focus_window();
    }

    /// Open the keybinds screen and capture a window PNG.
    pub fn arm_keybinds_shot(&mut self, path: std::path::PathBuf) {
        self.keybinds_shot_path = Some(path);
        self.keybinds_shot_frames = 0;
        self.playing = false;
        self.binds.cancel_listen();
        self.menu.show_keybinds();
        let _ = self.window.focus_window();
    }

    /// Open creative inventory on the `flow` compound tab and capture a PNG.
    pub fn arm_creative_shot(&mut self, path: std::path::PathBuf) {
        self.creative_shot_path = Some(path);
        self.creative_shot_frames = 0;
        self.playing = false;
        self.menu.close_menu();
        self.gui.open_creative_tab("flow");
        self.gui.push_chat("flow tab: pipes tanks power");
        let _ = self.window.focus_window();
    }

    /// Midnight outdoor horde in view, then quit with a PNG.
    pub fn arm_night_spawn_shot(&mut self, path: std::path::PathBuf) {
        self.night_spawn_shot_path = Some(path);
        self.night_spawn_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.0);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        // Keep the trial demo zombie out of frame; night pack is the subject.
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.night.force_at([8.0, 1.0, 12.0]);
        self.night.force_at([9.2, 1.0, 11.4]);
        self.night.force_at([7.0, 1.0, 11.0]);
        for zombie in self.night.iter_mut() {
            zombie.hold();
            // Face the camera (looking north toward -Z from south of the stairs).
            zombie.face(0.0);
        }

        // Same proven stairs overlook as the Blockbench zombie shot, aimed at the pack.
        let eye = glam::Vec3::new(8.5, 3.5 + body::EYE_HEIGHT, 14.4);
        let target = glam::Vec3::new(8.2, 1.7, 11.6);
        self.camera = Camera::looking_at(eye, target);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.night_spawn_shot_eye = eye;
        self.night_spawn_shot_yaw = yaw;
        self.night_spawn_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.rebuild_zombie();
        let _ = self.window.focus_window();
    }

    fn pin_night_spawn_shot_camera(&mut self) {
        if self.night_spawn_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.night_spawn_shot_eye,
            self.night_spawn_shot_yaw,
            self.night_spawn_shot_pitch,
        );
        self.posed_eye = self.night_spawn_shot_eye;
        self.posed_yaw = self.night_spawn_shot_yaw;
        self.posed_pitch = self.night_spawn_shot_pitch;
        self.pose_dirty = true;
    }

    /// Night bed beside the stairs, sleep to dawn, then quit with a PNG.
    pub fn arm_bed_sleep_shot(&mut self, path: std::path::PathBuf) {
        self.bed_sleep_shot_path = Some(path);
        self.bed_sleep_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();

        // Keep hostiles out of the sleep ring so the proof reaches morning.
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();

        let bed = [9, 1, 11];
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: bed[0],
                y: bed[1],
                z: bed[2],
                block: Block::new("bed", 0),
            }],
        );
        self.chunk_bake.touch(bed[0], bed[2]);

        // Start after dusk so sleep advances to morning.
        self.clock.set_time_of_day(0.85);
        self.sleep_in_bed(bed);
        // Mods may re-seed guests on ticks; keep the ring clear for the shot.
        self.guests.clear();
        self.rebuild_guests();
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(
            bed[0] as f32 + 0.5,
            bed[1] as f32 + 0.7,
            bed[2] as f32 + 0.5,
        );
        self.camera = Camera::looking_at(eye, target);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.bed_sleep_shot_eye = eye;
        self.bed_sleep_shot_yaw = yaw;
        self.bed_sleep_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        // Let the wake veil settle so the morning bed reads clearly.
        self.sleep_fade = 0.35;
        let _ = self.window.focus_window();
    }

    fn pin_bed_sleep_shot_camera(&mut self) {
        if self.bed_sleep_shot_path.is_none() {
            return;
        }
        if !self.guests.is_empty() {
            self.guests.clear();
            self.rebuild_guests();
        }
        self.camera.restore(
            self.bed_sleep_shot_eye,
            self.bed_sleep_shot_yaw,
            self.bed_sleep_shot_pitch,
        );
        self.posed_eye = self.bed_sleep_shot_eye;
        self.posed_yaw = self.bed_sleep_shot_yaw;
        self.posed_pitch = self.bed_sleep_shot_pitch;
        self.pose_dirty = true;
    }

    /// Cliff edge under a crawlspace slab: sneak label, lowered view, cling.
    pub fn arm_crouch_shot(&mut self, path: std::path::PathBuf) {
        self.crouch_shot_path = Some(path);
        self.crouch_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.35);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        // Stone pier ending at z=12, with a 1.5-high crawl slab over the end.
        let mut changes = Vec::new();
        for z in 8..=12 {
            for x in 7..=9 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("stone", 0),
                });
            }
        }
        for x in 7..=9 {
            changes.push(CellChange {
                x,
                y: 3,
                z: 12,
                block: Block::new("slab", 1),
            });
        }
        // Drop beyond the pier so the edge reads.
        for z in 13..=16 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::AIR,
                });
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("dirt", 0),
                });
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let eye = glam::Vec3::new(8.5, 2.0 + body::EYE_HEIGHT, 11.55);
        let target = glam::Vec3::new(8.5, 1.2, 14.5);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        self.input.crouch = true;
        self.crouch = 1.0;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.crouch_shot_eye = eye;
        self.crouch_shot_yaw = yaw;
        self.crouch_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_crouch_shot_camera(&mut self) {
        if self.crouch_shot_path.is_none() {
            return;
        }
        self.input.crouch = true;
        self.crouch = 1.0;
        self.camera.restore(
            self.crouch_shot_eye,
            self.crouch_shot_yaw,
            self.crouch_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.crouch_shot_eye;
        self.posed_yaw = self.crouch_shot_yaw;
        self.posed_pitch = self.crouch_shot_pitch;
        self.pose_dirty = true;
    }

    /// Walk FOV then sprint FOV from the stairs overlook, stitched side by side.
    pub fn arm_sprint_shot(&mut self, path: std::path::PathBuf) {
        self.sprint_shot_path = Some(path);
        self.sprint_shot_frames = 0;
        self.sprint_shot_phase = SprintShotPhase::WaitingWalk;
        self.sprint_shot_walk = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        self.input.fast = false;
        self.input.forward = false;
        self.sprint_kick = 0.0;
        self.sprint_show = 0.0;
        self.sprint_asked = false;
        self.sprint_blocked = false;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.sprint_shot_eye = eye;
        self.sprint_shot_yaw = yaw;
        self.sprint_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_sprint_shot_camera(&mut self) {
        if self.sprint_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.sprint_shot_eye,
            self.sprint_shot_yaw,
            self.sprint_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.sprint_shot_eye;
        self.posed_yaw = self.sprint_shot_yaw;
        self.posed_pitch = self.sprint_shot_pitch;
        self.pose_dirty = true;
        match self.sprint_shot_phase {
            SprintShotPhase::WaitingWalk | SprintShotPhase::Idle => {
                self.input.fast = false;
                self.input.forward = false;
                self.sprint_kick = 0.0;
                self.sprint_show = 0.0;
                self.sprint_asked = false;
                self.sprint_blocked = false;
            }
            SprintShotPhase::WaitingSprint => {
                self.input.fast = true;
                self.input.forward = true;
                self.sprint_kick = 1.0;
                self.sprint_show = 1.0;
                self.sprint_asked = true;
                self.sprint_blocked = false;
                self.sprint_run = 1.0;
            }
        }
    }

    /// Dry shore FOV then underwater FOV/tint/veil from the pool edge, stitched side by side.
    pub fn arm_swim_shot(&mut self, path: std::path::PathBuf) {
        self.swim_shot_path = Some(path);
        self.swim_shot_frames = 0;
        self.swim_shot_phase = SwimShotPhase::WaitingDry;
        self.swim_shot_dry = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        // Stone pier overlooking a deep pool.
        for z in 8..=11 {
            for x in 7..=9 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("stone", 0),
                });
            }
        }
        for z in 12..=18 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("stone", 0),
                });
                for y in 1..=4 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::new("water", 0),
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let dry_eye = glam::Vec3::new(8.5, 2.0 + body::EYE_HEIGHT, 10.7);
        let wet_eye = glam::Vec3::new(8.5, 2.55, 15.2);
        let target = glam::Vec3::new(8.5, 2.2, 17.5);
        self.camera = Camera::looking_at(dry_eye, target);
        self.camera.on_ground = true;
        self.water_sub = 0.0;
        self.splash_kick = 0.0;
        self.was_wet = false;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.swim_shot_dry_eye = dry_eye;
        self.swim_shot_wet_eye = wet_eye;
        self.swim_shot_yaw = yaw;
        self.swim_shot_pitch = pitch;
        self.posed_eye = dry_eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_swim_shot_camera(&mut self) {
        if self.swim_shot_path.is_none() {
            return;
        }
        let eye = match self.swim_shot_phase {
            SwimShotPhase::WaitingWet => {
                self.water_sub = 1.0;
                self.splash_kick = 0.35;
                self.was_wet = true;
                self.camera.on_ground = false;
                self.swim_shot_wet_eye
            }
            SwimShotPhase::WaitingDry | SwimShotPhase::Idle => {
                self.water_sub = 0.0;
                self.splash_kick = 0.0;
                self.was_wet = false;
                self.camera.on_ground = true;
                self.swim_shot_dry_eye
            }
        };
        self.camera.restore(eye, self.swim_shot_yaw, self.swim_shot_pitch);
        self.posed_eye = eye;
        self.posed_yaw = self.swim_shot_yaw;
        self.posed_pitch = self.swim_shot_pitch;
        self.pose_dirty = true;
    }

    /// Cart mid-boost on the powered rail, then quit with a PNG.
    pub fn arm_boost_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = (
            rebuild_world::SHAFT_MOUTH.0 + 5,
            rebuild_world::SHAFT_MOUTH.1,
        );
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "powered_rail" {
                cy = y;
                break;
            }
        }
        let _ = self.cart.take();
        assert!(self.cart.place(cx, cy, cz));
        assert!(self.cart.mount());
        self.rebuild_minecart();
        let seat = self
            .cart
            .seat()
            .map(glam::Vec3::from)
            .expect("cart seat");
        let target = glam::Vec3::new(cx as f32 + 3.5, cy as f32 + 1.1, cz as f32 + 0.5);
        self.camera = Camera::looking_at(seat, target);
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = seat;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = seat;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.cart_kick = 1.0;
        self.cart_show = 1.0;
        self.bob_amount = 0.5;
        self.found_mine = true;
        self.found_rails = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("minecart");
        self.hotbar_dirty = true;
        self.gui.push_chat("boost");
        let mote = [cx as f32 + 0.5, cy as f32 + 0.9, cz as f32 + 0.5];
        self.drops.puff(mote, [0.92, 0.45, 0.22]);
        self.rebuild_drops();
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Idle aboard then gallop FOV on the east meadow horse, stitched side by side.
    pub fn arm_horse_shot(&mut self, path: std::path::PathBuf) {
        self.horse_shot_path = Some(path);
        self.horse_shot_frames = 0;
        self.horse_shot_phase = HorseShotPhase::WaitingIdle;
        self.horse_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        self.horse = crate::mobs::Horse::new();
        assert!(self.horse.feed());
        assert!(self.horse.saddle());
        assert!(self.horse.mount());
        self.rebuild_horse();
        let seat = self
            .horse
            .seat()
            .map(glam::Vec3::from)
            .expect("horse seat");
        let (hx, hz) = crate::mobs::HORSE_PAD;
        let target = glam::Vec3::new(hx as f32 + 3.0, 1.6, hz as f32 + 0.5);
        self.camera = Camera::looking_at(seat, target);
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.input.forward = false;
        self.input.fast = false;
        self.clear_horse_feel();
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.horse_shot_eye = seat;
        self.horse_shot_yaw = yaw;
        self.horse_shot_pitch = pitch;
        self.posed_eye = seat;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.found_horse = true;
        self.gui.push_chat("gallop");
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("wheat");
        let _ = self.hotbar.give_stack("saddle");
        self.hotbar_dirty = true;
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(hx + dx * 8, hz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    fn pin_horse_shot_camera(&mut self) {
        if self.horse_shot_path.is_none() {
            return;
        }
        if !self.horse.ridden() {
            let _ = self.horse.mount();
        }
        self.camera.restore(
            self.horse_shot_eye,
            self.horse_shot_yaw,
            self.horse_shot_pitch,
        );
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.horse_shot_eye;
        self.posed_yaw = self.horse_shot_yaw;
        self.posed_pitch = self.horse_shot_pitch;
        self.pose_dirty = true;
        match self.horse_shot_phase {
            HorseShotPhase::WaitingIdle | HorseShotPhase::Idle => {
                self.input.forward = false;
                self.input.back = false;
                self.input.left = false;
                self.input.right = false;
                self.horse_kick = 0.0;
                self.horse_show = 0.0;
                self.horse_stroke = 0.0;
            }
            HorseShotPhase::WaitingRide => {
                self.input.forward = true;
                self.horse_kick = 1.0;
                self.horse_show = 1.0;
                self.horse_stroke = 0.2;
                self.bob_amount = 0.5;
            }
        }
    }

    /// Idle aboard then riding FOV on the shaft rail spur, stitched side by side.
    pub fn arm_minecart_shot(&mut self, path: std::path::PathBuf) {
        self.cart_shot_path = Some(path);
        self.cart_shot_frames = 0;
        self.cart_shot_phase = CartShotPhase::WaitingIdle;
        self.cart_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::SHAFT_RAIL;
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "rail" {
                cy = y;
                break;
            }
        }
        let _ = self.cart.take();
        assert!(self.cart.place(cx, cy, cz));
        assert!(self.cart.mount());
        self.rebuild_minecart();
        let seat = self
            .cart
            .seat()
            .map(glam::Vec3::from)
            .expect("cart seat");
        let target = glam::Vec3::new(cx as f32 + 4.0, cy as f32 + 1.2, cz as f32 + 0.5);
        self.camera = Camera::looking_at(seat, target);
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.input.forward = false;
        self.input.fast = false;
        self.clear_cart_feel();
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cart_shot_eye = seat;
        self.cart_shot_yaw = yaw;
        self.cart_shot_pitch = pitch;
        self.posed_eye = seat;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.found_mine = true;
        self.found_rails = true;
        self.gui.push_chat("ride");
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("minecart");
        self.hotbar_dirty = true;
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    fn pin_cart_shot_camera(&mut self) {
        if self.cart_shot_path.is_none() {
            return;
        }
        if !self.cart.aboard() {
            let _ = self.cart.mount();
        }
        self.camera.restore(
            self.cart_shot_eye,
            self.cart_shot_yaw,
            self.cart_shot_pitch,
        );
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.cart_shot_eye;
        self.posed_yaw = self.cart_shot_yaw;
        self.posed_pitch = self.cart_shot_pitch;
        self.pose_dirty = true;
        match self.cart_shot_phase {
            CartShotPhase::WaitingIdle | CartShotPhase::Idle => {
                self.input.forward = false;
                self.input.back = false;
                self.input.left = false;
                self.input.right = false;
                self.cart_kick = 0.0;
                self.cart_show = 0.0;
                self.cart_stroke = 0.0;
            }
            CartShotPhase::WaitingRide => {
                self.input.forward = true;
                self.cart_kick = 1.0;
                self.cart_show = 1.0;
                self.cart_stroke = 0.2;
                self.bob_amount = 0.45;
            }
        }
    }

    /// Idle aboard then rowing FOV from a marsh pool, stitched side by side.
    pub fn arm_boat_shot(&mut self, path: std::path::PathBuf) {
        self.boat_shot_path = Some(path);
        self.boat_shot_frames = 0;
        self.boat_shot_phase = BoatShotPhase::WaitingIdle;
        self.boat_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 12..=20 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("stone", 0),
                });
                for y in 1..=2 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::new("water", 0),
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let _ = self.boat.take();
        assert!(self.boat.place(8, 1, 16));
        assert!(self.boat.mount());
        self.rebuild_boat();
        let seat = self
            .boat
            .seat()
            .map(glam::Vec3::from)
            .expect("boat seat");
        let target = glam::Vec3::new(8.5, 2.0, 20.0);
        self.camera = Camera::looking_at(seat, target);
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.input.forward = false;
        self.input.fast = false;
        self.clear_boat_feel();
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.boat_shot_eye = seat;
        self.boat_shot_yaw = yaw;
        self.boat_shot_pitch = pitch;
        self.posed_eye = seat;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_boat_shot_camera(&mut self) {
        if self.boat_shot_path.is_none() {
            return;
        }
        if !self.boat.aboard() {
            let _ = self.boat.mount();
        }
        self.camera.restore(
            self.boat_shot_eye,
            self.boat_shot_yaw,
            self.boat_shot_pitch,
        );
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.boat_shot_eye;
        self.posed_yaw = self.boat_shot_yaw;
        self.posed_pitch = self.boat_shot_pitch;
        self.pose_dirty = true;
        match self.boat_shot_phase {
            BoatShotPhase::WaitingIdle | BoatShotPhase::Idle => {
                self.input.forward = false;
                self.input.back = false;
                self.input.left = false;
                self.input.right = false;
                self.boat_kick = 0.0;
                self.boat_show = 0.0;
                self.boat_stroke = 0.0;
            }
            BoatShotPhase::WaitingRow => {
                self.input.forward = true;
                self.boat_kick = 1.0;
                self.boat_show = 1.0;
                self.boat_stroke = 0.2;
                self.bob_amount = 0.45;
            }
        }
    }

    /// Idle FOV then break FOV from the stairs overlook, stitched side by side.
    /// Look at the southeast stone-brick shrine, then quit with a PNG.
    pub fn arm_shrine_shot(&mut self, path: std::path::PathBuf) {
        self.shrine_shot_path = Some(path);
        self.shrine_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        // Force the shrine chunk so a stale save cannot hide it.
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (sx, sz) = rebuild_world::SHRINE;
        let mut sy = 4;
        for y in 0..80 {
            if self.world.block_at(sx, y, sz).id == "chiseled_stone_bricks"
                || self.world.block_at(sx, y, sz - 1).id == "chest"
            {
                sy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(sx as f32 + 0.5, sy as f32 + 1.2, sz as f32 + 0.5);
        let eye = glam::Vec3::new(
            sx as f32 + 0.5,
            sy as f32 + 1.4 + body::EYE_HEIGHT,
            sz as f32 + 5.5,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.shrine_shot_eye = eye;
        self.shrine_shot_yaw = yaw;
        self.shrine_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(sx + dx * 8, sz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    fn pin_shrine_shot_camera(&mut self) {
        if self.shrine_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.shrine_shot_eye,
            self.shrine_shot_yaw,
            self.shrine_shot_pitch,
        );
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.shrine_shot_eye;
        self.posed_yaw = self.shrine_shot_yaw;
        self.posed_pitch = self.shrine_shot_pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
    }

    fn poll_shrine_shot(&mut self) {
        let Some(path) = self.shrine_shot_path.clone() else {
            return;
        };
        self.pin_shrine_shot_camera();
        self.shrine_shot_frames = self.shrine_shot_frames.saturating_add(1);
        if self.shrine_shot_frames < 110 || !self.chunk_bake.is_idle() {
            if self.shrine_shot_frames > 600 {
                eprintln!("shrine shot timed out");
                self.shrine_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("shrine shot wrote {}", path.display());
                self.shrine_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("shrine shot failed: {err}");
                if self.shrine_shot_frames > 250 {
                    self.shrine_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    /// Look at the west mine-mouth chest, then quit with a PNG.
    pub fn arm_mine_shot(&mut self, path: std::path::PathBuf) {
        self.mine_shot_path = Some(path);
        self.mine_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::SHAFT_CHEST;
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "chest" {
                cy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(cx as f32 + 0.5, cy as f32 + 0.9, cz as f32 + 0.5);
        let eye = glam::Vec3::new(
            cx as f32 + 4.5,
            cy as f32 + 1.2 + body::EYE_HEIGHT,
            cz as f32 + 0.5,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.mine_shot_eye = eye;
        self.mine_shot_yaw = yaw;
        self.mine_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    fn pin_mine_shot_camera(&mut self) {
        if self.mine_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.mine_shot_eye,
            self.mine_shot_yaw,
            self.mine_shot_pitch,
        );
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.mine_shot_eye;
        self.posed_yaw = self.mine_shot_yaw;
        self.posed_pitch = self.mine_shot_pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
    }

    fn poll_mine_shot(&mut self) {
        let Some(path) = self.mine_shot_path.clone() else {
            return;
        };
        self.pin_mine_shot_camera();
        self.mine_shot_frames = self.mine_shot_frames.saturating_add(1);
        if self.mine_shot_frames < 110 || !self.chunk_bake.is_idle() {
            if self.mine_shot_frames > 600 {
                eprintln!("mine shot timed out");
                self.mine_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("mine shot wrote {}", path.display());
                self.mine_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("mine shot failed: {err}");
                if self.mine_shot_frames > 250 {
                    self.mine_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    /// Hold cooked food over a campfire after a one-step cook, then quit with a PNG.
    pub fn arm_cook_shot(&mut self, path: std::path::PathBuf) {
        self.arm_heat_cook_shot(path, "campfire");
    }

    /// Hold cooked food over a smoker after a one-step cook, then quit with a PNG.
    pub fn arm_smoker_cook_shot(&mut self, path: std::path::PathBuf) {
        self.arm_heat_cook_shot(path, "smoker");
    }

    /// Look at a waterlogged copper grate with cyan outline, then quit with a PNG.
    pub fn arm_compound_shot(&mut self, path: std::path::PathBuf) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        // Creative palette shots open the flow tab instead of a world vignette.
        if name.contains("creative") || name.contains("palette") {
            self.arm_creative_shot(path);
            return;
        }
        // Pad starter demo: photograph the stamped fixture loops in place.
        let demo_shot = name.contains("demo") || name.contains("starter");
        let ember = name.contains("ember");
        let flow = name.contains("flow");
        let pipes = name.contains("pipe") || name.contains("wire");
        let connected = name.contains("connected");
        let pump_shot = name.contains("pump") || name.contains("valve");
        let lamp_shot = name.contains("lamp");
        let gen_shot = name.contains("generator") || name.contains("dynamo");
        let spout_shot = name.contains("spout") || name.contains("faucet");
        let drain_shot = name.contains("drain") || name.contains("intake");
        let battery_shot = !lamp_shot
            && !gen_shot
            && (name.contains("battery") || name.contains("charge"));
        let tank_shot = !battery_shot
            && !lamp_shot
            && !gen_shot
            && !spout_shot
            && !drain_shot
            && !demo_shot
            && (name.contains("tank") || name.contains("buffer"));
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        // Ember / wire / pump / battery / lamp / generator shots at night so fill light reads.
        self.clock.set_time_of_day(
            if demo_shot || ember || pipes || pump_shot || battery_shot || lamp_shot || gen_shot
            {
                0.88
            } else {
                0.42
            },
        );
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        if demo_shot {
            // Stamp the pad loops even when a dirty save is loaded, then photograph.
            let (dx, dz) = rebuild_world::COMPOUND_DEMO;
            let mut changes = Vec::new();
            // Clear past the camera stand so generated terrain does not bury the shot.
            for z in 8..=17 {
                for x in 1..=16 {
                    changes.push(CellChange {
                        x,
                        y: 0,
                        z,
                        block: Block::new("grass", 0),
                    });
                    for y in 1..=6 {
                        changes.push(CellChange {
                            x,
                            y,
                            z,
                            block: Block::AIR,
                        });
                    }
                }
            }
            // West: tank → pipe → spout (+X). East: generator → wire → battery → lamp.
            changes.push(CellChange {
                x: 3,
                y: 1,
                z: 12,
                block: tank_at(FILL_WATER, 5),
            });
            changes.push(CellChange {
                x: 4,
                y: 1,
                z: 12,
                block: Block::new("pipe", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: 5,
                y: 1,
                z: 12,
                block: Block::new("spout", pump_meta(0, true)),
            });
            changes.push(CellChange {
                x: 11,
                y: 1,
                z: 10,
                block: generator_at(true),
            });
            changes.push(CellChange {
                x: 12,
                y: 1,
                z: 10,
                block: Block::new("wire", 0),
            });
            changes.push(CellChange {
                x: 13,
                y: 1,
                z: 10,
                block: battery_at(4),
            });
            changes.push(CellChange {
                x: 14,
                y: 1,
                z: 10,
                block: lamp_at(true),
            });
            // Stairs stay as the pad landmark south of the oak.
            changes.push(CellChange {
                x: 8,
                y: 1,
                z: 12,
                block: Block::new("oak_stairs", 0),
            });
            apply_changes(&mut self.world, &changes);
            for change in &changes {
                self.chunk_bake.touch(change.x, change.z);
            }
            let _ = apply_water_spouts(&mut self.world, [5, 1, 12], 8, 2);
            let _ = apply_ember_lamps(&mut self.world, [13, 1, 10], 8, 4, false);
            self.chunk_bake.touch(5, 12);
            self.chunk_bake.touch(6, 12);
            self.chunk_bake.touch(14, 10);
            self.found_compound = true;
            self.gui.push_chat("compound loop by the pad");
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            let eye = glam::Vec3::new(
                dx as f32 + 0.5,
                1.0 + 2.15 + body::EYE_HEIGHT,
                15.4,
            );
            let focus = glam::Vec3::new(dx as f32 + 0.5, 1.2, dz as f32 + 0.3);
            self.camera = Camera::looking_at(eye, focus);
            let (yaw, pitch) = self.camera.yaw_pitch();
            self.cook_shot_eye = eye;
            self.cook_shot_yaw = yaw;
            self.cook_shot_pitch = pitch;
            self.posed_eye = eye;
            self.posed_yaw = yaw;
            self.posed_pitch = pitch;
            self.pose_dirty = true;
            self.wanted_focus = chunk_of(self.camera.eye);
            let _ = self.window.focus_window();
            return;
        }

        let gx = 11;
        let gy = 1;
        let gz = 12;
        let mut changes = Vec::new();
        for z in 10..=14 {
            for x in 9..=14 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 1..=3 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        if gen_shot {
            // Running generator → wire → empty battery; optional campfire fuel beside.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: generator_at(true),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("wire", 0),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: Block::new("battery", 0),
            });
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz + 1,
                block: Block::new("campfire", 0),
            });
        } else if drain_shot {
            // World water ahead → open drain (+X) pulls into empty pipe behind.
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: Block::new("water", 0),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("drain", pump_meta(0, true)),
            });
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0),
            });
        } else if spout_shot {
            // Pressurized tank → open spout (+X) sprays water into air.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: tank_at(FILL_WATER, 6),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("spout", pump_meta(0, true)),
            });
        } else if lamp_shot {
            // Charged battery → wire → lit lamp; separate dark lamp with no power.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: battery_at(6),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("wire", 0),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: lamp_at(true),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 2,
                block: lamp_at(false),
            });
        } else if battery_shot {
            // Ember glass → wire → mid battery → outlet wire run.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("glass", 0).with_fill(FILL_EMBER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("wire", 0).with_fill(FILL_EMBER),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: battery_at(5),
            });
            changes.push(CellChange {
                x: gx + 3,
                y: gy,
                z: gz,
                block: Block::new("wire", 0),
            });
            changes.push(CellChange {
                x: gx + 4,
                y: gy,
                z: gz,
                block: Block::new("wire", 0),
            });
            // Side spur wire tee into a second battery.
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 1,
                block: Block::new("wire", 0).with_fill(FILL_EMBER),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 2,
                block: battery_at(3),
            });
        } else if tank_shot {
            // Source grate → pipe → mid tank → outlet pipe run.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: tank_at(FILL_WATER, 5),
            });
            changes.push(CellChange {
                x: gx + 3,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0),
            });
            changes.push(CellChange {
                x: gx + 4,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0),
            });
            // Ember tank on a side spur.
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 2,
                block: tank_at(FILL_EMBER, 3),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 1,
                block: Block::new("wire", 0),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz + 2,
                block: Block::new("glass", 0).with_fill(FILL_EMBER),
            });
        } else if pump_shot {
            // Intake grate → pipe → powered pump (+X) → empty pipe outlet.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: Block::new("pump", pump_meta(0, true)),
            });
            changes.push(CellChange {
                x: gx + 3,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0),
            });
            changes.push(CellChange {
                x: gx + 4,
                y: gy,
                z: gz,
                block: Block::new("pipe", 0),
            });
            // Closed pump on a parallel line — valve shut, no push.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz + 2,
                block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz + 2,
                block: Block::new("pipe", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz + 2,
                block: Block::new("pump", pump_meta(0, false)),
            });
            changes.push(CellChange {
                x: gx + 3,
                y: gy,
                z: gz + 2,
                block: Block::new("pipe", 0),
            });
        } else if pipes {
            if connected {
                // Water L-bend + T, ember wire corner — shows junction shapes.
                changes.push(CellChange {
                    x: gx,
                    y: gy,
                    z: gz,
                    block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
                });
                changes.push(CellChange {
                    x: gx + 1,
                    y: gy,
                    z: gz,
                    block: Block::new("pipe", 0),
                });
                changes.push(CellChange {
                    x: gx + 2,
                    y: gy,
                    z: gz,
                    block: Block::new("pipe", 0),
                });
                changes.push(CellChange {
                    x: gx + 3,
                    y: gy,
                    z: gz,
                    block: Block::new("pipe", 0),
                });
                // T stem south from mid pipe.
                changes.push(CellChange {
                    x: gx + 2,
                    y: gy,
                    z: gz + 1,
                    block: Block::new("pipe", 0),
                });
                changes.push(CellChange {
                    x: gx + 2,
                    y: gy,
                    z: gz + 2,
                    block: Block::new("pipe", 0),
                });
                // Ember wire L in front.
                changes.push(CellChange {
                    x: gx,
                    y: gy,
                    z: gz - 2,
                    block: Block::new("glass", 0).with_fill(FILL_EMBER),
                });
                changes.push(CellChange {
                    x: gx + 1,
                    y: gy,
                    z: gz - 2,
                    block: Block::new("wire", 0),
                });
                changes.push(CellChange {
                    x: gx + 2,
                    y: gy,
                    z: gz - 2,
                    block: Block::new("wire", 0),
                });
                changes.push(CellChange {
                    x: gx + 2,
                    y: gy,
                    z: gz - 1,
                    block: Block::new("wire", 0),
                });
            } else {
                // Water along pipes (north row) and ember along wires (south row).
                changes.push(CellChange {
                    x: gx,
                    y: gy,
                    z: gz,
                    block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
                });
                for step in 1..=4 {
                    changes.push(CellChange {
                        x: gx + step,
                        y: gy,
                        z: gz,
                        block: Block::new("pipe", 0),
                    });
                }
                changes.push(CellChange {
                    x: gx,
                    y: gy,
                    z: gz + 2,
                    block: Block::new("glass", 0).with_fill(FILL_EMBER),
                });
                for step in 1..=4 {
                    changes.push(CellChange {
                        x: gx + step,
                        y: gy,
                        z: gz + 2,
                        block: Block::new("wire", 0),
                    });
                }
            }
        } else if flow {
            // Pipe of fences: source fill at the west end, empty hosts east.
            let fill = if ember { FILL_EMBER } else { FILL_WATER };
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("copper_grate", 0).with_fill(fill),
            });
            for step in 1..=4 {
                changes.push(CellChange {
                    x: gx + step,
                    y: gy,
                    z: gz,
                    block: Block::new("fence", 0),
                });
            }
        } else {
            // Row of hosts: water grate, empty fence, ember glass, leaves.
            changes.push(CellChange {
                x: gx,
                y: gy,
                z: gz,
                block: Block::new("copper_grate", 0).with_fill(FILL_WATER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz,
                block: Block::new("fence", 0),
            });
            changes.push(CellChange {
                x: gx + 2,
                y: gy,
                z: gz,
                block: Block::new("glass", 0).with_fill(FILL_EMBER),
            });
            changes.push(CellChange {
                x: gx + 3,
                y: gy,
                z: gz,
                block: Block::new("leaves", 0).with_fill(FILL_EMBER),
            });
            changes.push(CellChange {
                x: gx + 1,
                y: gy,
                z: gz + 1,
                block: Block::new("oak_stairs", 0).with_fill(FILL_WATER),
            });
        }
        apply_changes(&mut self.world, &changes);
        if gen_shot {
            // Charge a few pulses so the battery shows ember.
            for _ in 0..3 {
                let _ = apply_ember_charges(&mut self.world, [gx + 1, gy, gz], 8, 2);
            }
        } else if spout_shot {
            let _ = apply_water_spouts(&mut self.world, [gx + 1, gy, gz], 8, 2);
        } else if lamp_shot {
            let _ = apply_ember_lamps(&mut self.world, [gx + 2, gy, gz], 8, 4, false);
        } else if battery_shot {
            for _ in 0..2 {
                let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz], 8, 1);
            }
        } else if tank_shot {
            for _ in 0..2 {
                let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz], 8, 1);
            }
        } else if pump_shot {
            for _ in 0..3 {
                let _ = apply_pump_pushes(&mut self.world, [gx + 2, gy, gz], 8, 1);
                let _ = apply_compound_seeps(&mut self.world, [gx + 3, gy, gz], 8, 1);
                // Closed line should stay dry past the valve.
                let _ = apply_pump_pushes(&mut self.world, [gx + 2, gy, gz + 2], 8, 1);
                let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz + 2], 8, 1);
            }
        } else if pipes {
            if connected {
                for _ in 0..4 {
                    let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz], 8, 1);
                    let _ = apply_compound_seeps(&mut self.world, [gx + 1, gy, gz - 2], 8, 1);
                }
            } else {
                for _ in 0..3 {
                    let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz], 8, 1);
                    let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz + 2], 8, 1);
                }
            }
        } else if flow {
            // Advance a few seeps so the PNG shows mid-pipe flow.
            for _ in 0..3 {
                let _ = apply_compound_seeps(&mut self.world, [gx + 2, gy, gz], 8, 1);
            }
        }
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }
        self.chunk_bake.touch(gx + 4, gz);
        self.chunk_bake.touch(gx + 4, gz + 2);
        self.chunk_bake.touch(gx + 2, gz - 2);
        let (eye, focus) = if gen_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("ember_charge");
            (
                glam::Vec3::new(
                    gx as f32 + 0.3,
                    gy as f32 + 2.05 + body::EYE_HEIGHT,
                    gz as f32 - 2.5,
                ),
                glam::Vec3::new(gx as f32 + 0.55, gy as f32 + 0.7, gz as f32 + 0.45),
            )
        } else if drain_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 1) as f32 + 0.5,
                    gy as f32 + 2.1 + body::EYE_HEIGHT,
                    gz as f32 - 2.6,
                ),
                glam::Vec3::new((gx + 1) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.5),
            )
        } else if spout_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 1) as f32 + 0.5,
                    gy as f32 + 2.1 + body::EYE_HEIGHT,
                    gz as f32 - 2.6,
                ),
                glam::Vec3::new((gx + 1) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.5),
            )
        } else if lamp_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("ember_charge");
            (
                glam::Vec3::new(
                    (gx + 1) as f32 + 0.2,
                    gy as f32 + 2.05 + body::EYE_HEIGHT,
                    gz as f32 - 2.5,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.7, gz as f32 + 0.45),
            )
        } else if battery_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("ember_charge");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 2.2 + body::EYE_HEIGHT,
                    gz as f32 - 2.8,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.7, gz as f32 + 0.5),
            )
        } else if tank_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 2.2 + body::EYE_HEIGHT,
                    gz as f32 - 2.8,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.7, gz as f32 + 0.5),
            )
        } else if pump_shot {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 2.1 + body::EYE_HEIGHT,
                    gz as f32 - 2.6,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.7),
            )
        } else if pipes && connected {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 2.15 + body::EYE_HEIGHT,
                    gz as f32 - 3.0,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.35),
            )
        } else if pipes {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 2.0 + body::EYE_HEIGHT,
                    gz as f32 - 2.4,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 1.0),
            )
        } else if flow {
            let held = if ember { "ember_charge" } else { "water_bucket" };
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack(held);
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 1.7 + body::EYE_HEIGHT,
                    gz as f32 - 3.2,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.5),
            )
        } else if ember {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("ember_charge");
            (
                glam::Vec3::new(
                    (gx + 2) as f32 + 0.5,
                    gy as f32 + 1.55 + body::EYE_HEIGHT,
                    gz as f32 - 2.8,
                ),
                glam::Vec3::new((gx + 2) as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.5),
            )
        } else {
            let _ = self.hotbar.select_digit(1);
            let _ = self.hotbar.give_stack("water_bucket");
            (
                glam::Vec3::new(
                    gx as f32 + 0.5,
                    gy as f32 + 1.55 + body::EYE_HEIGHT,
                    gz as f32 - 2.8,
                ),
                glam::Vec3::new(gx as f32 + 0.5, gy as f32 + 0.55, gz as f32 + 0.5),
            )
        };
        self.hotbar_dirty = true;
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Tamed parrot perched on the shoulder (third person), then quit with a PNG.
    pub fn arm_parrot_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 8..=14 {
            for x in 2..=8 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 1..=3 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        // Stand on the pad facing +Z so the right-shoulder bird reads clearly.
        // Keep the eye on the body; F5 third-person pulls the view back for the PNG.
        let feet = glam::Vec3::new(5.5, 1.0, 10.5);
        let eye = feet + glam::Vec3::new(0.0, body::EYE_HEIGHT, 0.0);
        let player_yaw = std::f32::consts::FRAC_PI_2; // forward +Z
        self.camera.restore(eye, player_yaw, -0.12);
        self.parrot.place_at([4.5, 1.0, 11.0]);
        let _ = self.parrot.feed();
        let _ = self.parrot.perch();
        self.parrot
            .ride_shoulder([feet.x, feet.y, feet.z], player_yaw);
        self.rebuild_parrot();
        self.found_parrot = true;
        self.gui.push_chat("shoulder");
        self.hotbar.install_saved(0, &[]);
        self.hotbar_dirty = true;
        self.third_person = true;
        self.gui.push_chat("third person");

        self.cook_shot_eye = eye;
        self.cook_shot_yaw = player_yaw;
        self.cook_shot_pitch = -0.12;
        self.posed_eye = eye;
        self.posed_yaw = player_yaw;
        self.posed_pitch = -0.12;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Tamed cat companion sitting by the player, then quit with a PNG.
    pub fn arm_cat_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 8..=14 {
            for x in 9..=15 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 1..=3 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        self.cat.place_at([12.5, 1.0, 11.2]);
        let _ = self.cat.feed();
        let _ = self.cat.sit();
        self.rebuild_cat();
        self.found_cat = true;
        self.gui.push_chat("sit");
        self.hotbar.install_saved(0, &[]);
        self.hotbar_dirty = true;

        let focus = glam::Vec3::from(self.cat.origin()) + glam::Vec3::new(0.0, 0.3, 0.0);
        let eye = glam::Vec3::new(focus.x - 0.3, focus.y + 1.25 + body::EYE_HEIGHT, focus.z - 2.4);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Tamed wolf companion sitting by the player, then quit with a PNG.
    pub fn arm_wolf_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 6..=12 {
            for x in 5..=12 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 1..=3 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        self.wolf.place_at([8.5, 1.0, 9.2]);
        let _ = self.wolf.feed();
        let _ = self.wolf.sit();
        self.rebuild_wolf();
        self.found_wolf = true;
        self.gui.push_chat("sit");
        // Empty hand so the companion tip matches the use rule.
        self.hotbar.install_saved(0, &[]);
        self.hotbar_dirty = true;

        let focus = glam::Vec3::from(self.wolf.origin()) + glam::Vec3::new(0.0, 0.35, 0.0);
        let eye = glam::Vec3::new(focus.x - 0.4, focus.y + 1.35 + body::EYE_HEIGHT, focus.z - 2.6);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Dusk fireflies over a shore pool, then quit with a PNG.
    pub fn arm_firefly_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        // Early night so fireflies read against a dark sky.
        self.clock.set_time_of_day(0.88);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 20..=28 {
            for x in 6..=12 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 1..=4 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        for z in 22..=26 {
            for x in 7..=11 {
                changes.push(CellChange {
                    x,
                    y: 0,
                    z,
                    block: Block::new("stone", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("water", 0),
                });
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let shore = [9.5, 2.2, 24.5];
        for offset in [
            [0.0, 0.4, 0.0],
            [1.2, 0.8, -0.5],
            [-1.0, 1.0, 0.9],
            [0.5, 1.3, 1.4],
            [-0.7, 0.6, -1.2],
            [1.5, 1.1, 0.3],
            [-1.4, 0.9, -0.4],
            [0.2, 1.6, -1.0],
            [0.9, 0.5, 1.1],
            [-0.3, 1.4, 0.2],
            [1.1, 0.7, -1.3],
            [-1.2, 1.2, 1.5],
        ] {
            self.drops.firefly([
                shore[0] + offset[0],
                shore[1] + offset[1],
                shore[2] + offset[2],
            ]);
        }
        self.rebuild_drops();
        self.found_fireflies = true;
        self.gui.push_chat("fireflies");

        let eye = glam::Vec3::new(9.5, 1.0 + 1.8 + body::EYE_HEIGHT, 20.2);
        let focus = glam::Vec3::new(9.5, 2.6, 24.5);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Axolotl companion swimming beside you in a pool, then quit with a PNG.
    pub fn arm_axolotl_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.zombie.rouse([2.0, 1.0, 2.0]);
        self.zombie.hold();
        self.night = crate::mobs::NightHorde::new();
        self.guests.clear();
        self.rebuild_zombie();
        self.rebuild_guests();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 28..=38 {
            for x in 5..=12 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("stone", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::new("water", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 3,
                    z,
                    block: Block::new("water", 0),
                });
                for y in 4..=6 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        // Dry elbow so the companion has to pathfind around it.
        changes.push(CellChange {
            x: 8,
            y: 2,
            z: 33,
            block: Block::new("stone", 0),
        });
        changes.push(CellChange {
            x: 8,
            y: 3,
            z: 33,
            block: Block::new("stone", 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        self.axolotl.place_at([6.5, 3.72, 30.5]);
        if !self.axolotl.following() {
            let _ = self.axolotl.follow();
        }
        // Swim toward the far side of the pool (around the dry cell).
        self.axolotl.tick(80, [10.5, 36.5], &self.world);
        self.rebuild_axolotl();
        self.gui.push_chat("companion");

        let focus = glam::Vec3::from(self.axolotl.origin()) + glam::Vec3::new(0.0, 0.35, 0.0);
        let eye = glam::Vec3::new(focus.x - 1.2, focus.y + 1.55 + body::EYE_HEIGHT, focus.z - 3.4);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Open the shrine lectern on page 2 of the guidebook, then quit with a PNG.
    pub fn arm_lectern_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (lx, lz) = rebuild_world::SHRINE_LECTERN;
        let mut ly = 4;
        for y in 0..80 {
            if self.world.block_at(lx, y, lz).id == "lectern" {
                ly = y;
                break;
            }
        }
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: lx,
                y: ly,
                z: lz,
                block: Block::new("lectern", 1),
            }],
        );
        self.chunk_bake.touch(lx, lz);
        let focus = glam::Vec3::new(lx as f32 + 0.5, ly as f32 + 0.85, lz as f32 + 0.5);
        let eye = glam::Vec3::new(
            lx as f32 + 0.5,
            ly as f32 + 1.3 + body::EYE_HEIGHT,
            lz as f32 + 3.2,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let _ = self.gui.open_lectern([lx, ly, lz], true);
        let _ = self.gui.turn_lectern(1);
        self.gui.push_chat(gui::LECTERN_PAGES[1]);
        self.found_shrine = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(lx + dx * 8, lz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Camp scarecrow south of the farm plot, then quit with a PNG.
    pub fn arm_scarecrow_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::CAMP_SCARECROW;
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "pumpkin"
                && self.world.block_at(cx, y, cz).metadata >= 1
            {
                cy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(cx as f32 + 0.5, cy as f32 + 0.35, cz as f32 + 0.5);
        let eye = glam::Vec3::new(
            cx as f32 - 2.6,
            cy as f32 + 0.55 + body::EYE_HEIGHT,
            cz as f32 + 3.0,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, cy as f32 + 0.85, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 1;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.92, 0.55, 0.18]);
        self.drops.note(mote, 1);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(1), mote);
        self.found_camp = true;
        self.found_farm = true;
        self.found_scarecrow = true;
        self.zombie.spook([cx as f32 + 0.5, cz as f32 + 0.5]);
        self.rebuild_zombie();
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("pumpkin");
        self.hotbar_dirty = true;
        self.gui.push_chat("scarecrow");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Camp east fence plot with wheat, then quit with a PNG.
    pub fn arm_farm_plot_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::CAMP_FARM;
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "wheat_crop" {
                cy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(cx as f32 + 0.5, cy as f32 + 0.55, cz as f32 + 0.5);
        let eye = glam::Vec3::new(
            cx as f32 - 3.4,
            cy as f32 + 1.15 + body::EYE_HEIGHT,
            cz as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, cy as f32 + 1.05, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 2;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.55, 0.72, 0.28]);
        self.drops.note(mote, 2);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(2), mote);
        self.found_camp = true;
        self.found_farm = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("wooden_hoe");
        let _ = self.hotbar.collect("wheat_seeds");
        self.hotbar_dirty = true;
        self.gui.push_chat("plot");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Fresh bread on a tilled wheat patch, then quit with a PNG.
    pub fn arm_bread_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let cx = 12;
        let cz = 17;
        let mut changes = Vec::new();
        for z in (cz - 1)..=(cz + 1) {
            for x in (cx - 1)..=(cx + 1) {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("farmland", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::AIR,
                });
            }
        }
        changes.push(CellChange {
            x: cx,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 3),
        });
        changes.push(CellChange {
            x: cx - 1,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 2),
        });
        changes.push(CellChange {
            x: cx + 1,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 1),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let focus = glam::Vec3::new(cx as f32 + 0.5, 2.4, cz as f32 + 0.5);
        let eye = glam::Vec3::new(cx as f32 - 2.3, 2.05 + body::EYE_HEIGHT, cz as f32 + 2.5);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, 2.85, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 3;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.82, 0.58, 0.28]);
        self.drops.note(mote, 3);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(3), mote);
        self.found_compound = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("bread");
        let _ = self.hotbar.collect("wheat");
        let _ = self.hotbar.collect("wheat");
        let _ = self.hotbar.collect("wheat");
        self.hotbar_dirty = true;
        self.gui.push_chat("bread");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Fresh farmland beside dirt with a till puff, then quit with a PNG.
    pub fn arm_farmland_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let cx = 11;
        let cz = 16;
        let mut changes = Vec::new();
        for z in (cz - 1)..=(cz + 1) {
            for x in (cx - 1)..=(cx + 1) {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("dirt", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::AIR,
                });
            }
        }
        changes.push(CellChange {
            x: cx,
            y: 1,
            z: cz,
            block: Block::new("farmland", 0),
        });
        changes.push(CellChange {
            x: cx,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let focus = glam::Vec3::new(cx as f32 + 0.5, 1.85, cz as f32 + 0.5);
        let eye = glam::Vec3::new(cx as f32 - 2.2, 2.1 + body::EYE_HEIGHT, cz as f32 + 2.5);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, 2.15, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 2;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.55, 0.38, 0.22]);
        self.drops.note(mote, 2);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(2), mote);
        self.found_compound = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("wooden_hoe");
        self.hotbar_dirty = true;
        self.gui.push_chat("till");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Ripe wheat on dirt mid-harvest with a mote, then quit with a PNG.
    pub fn arm_wheat_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let cx = 10;
        let cz = 18;
        let mut changes = Vec::new();
        for z in (cz - 1)..=(cz + 1) {
            for x in (cx - 1)..=(cx + 1) {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("dirt", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::AIR,
                });
                changes.push(CellChange {
                    x,
                    y: 3,
                    z,
                    block: Block::AIR,
                });
            }
        }
        changes.push(CellChange {
            x: cx,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 3),
        });
        changes.push(CellChange {
            x: cx - 1,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 2),
        });
        changes.push(CellChange {
            x: cx + 1,
            y: 2,
            z: cz,
            block: Block::new("wheat_crop", 1),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let focus = glam::Vec3::new(cx as f32 + 0.5, 2.7, cz as f32 + 0.5);
        let eye = glam::Vec3::new(cx as f32 - 2.4, 2.05 + body::EYE_HEIGHT, cz as f32 + 2.6);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, 3.05, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 4;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.78, 0.72, 0.28]);
        self.drops.note(mote, 4);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(4), mote);
        self.found_compound = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("wheat_seeds");
        let _ = self.hotbar.collect("bone_meal");
        self.hotbar_dirty = true;
        self.gui.push_chat("harvest");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Mid-grow a sugar cane stalk with bone meal puff, then quit with a PNG.
    pub fn arm_grow_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let cx = 12;
        let cz = 22;
        let mut changes = Vec::new();
        for z in (cz - 1)..=(cz + 1) {
            for x in (cx - 2)..=(cx + 1) {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("sand", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::AIR,
                });
            }
        }
        // Water against the sand so the cane is supported.
        for z in (cz - 1)..=(cz + 1) {
            changes.push(CellChange {
                x: cx + 2,
                y: 1,
                z,
                block: Block::new("water", 0),
            });
            changes.push(CellChange {
                x: cx + 2,
                y: 2,
                z,
                block: Block::AIR,
            });
        }
        changes.push(CellChange {
            x: cx,
            y: 2,
            z: cz,
            block: Block::new("sugar_cane", 0),
        });
        changes.push(CellChange {
            x: cx,
            y: 3,
            z: cz,
            block: Block::new("sugar_cane", 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let focus = glam::Vec3::new(cx as f32 + 0.5, 3.2, cz as f32 + 0.5);
        let eye = glam::Vec3::new(cx as f32 - 2.2, 2.15 + body::EYE_HEIGHT, cz as f32 + 2.8);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote = [cx as f32 + 0.5, 3.35, cz as f32 + 0.5];
        self.note_shot_at = Some(mote);
        self.note_shot_pitch = 5;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(mote, [0.45, 0.85, 0.38]);
        self.drops.note(mote, 5);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(5), mote);
        self.found_compound = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("bone_meal");
        self.hotbar_dirty = true;
        self.gui.push_chat("grow");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Cast a fishing bobber into a shore pool mid-wait with splash motes, then quit with a PNG.
    pub fn arm_fish_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let mut changes = Vec::new();
        for z in 18..=24 {
            for x in 14..=20 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("stone", 0),
                });
                changes.push(CellChange {
                    x,
                    y: 2,
                    z,
                    block: Block::new("water", 0),
                });
                for y in 3..=5 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        // Dry shore rim south of the pool for the cast stance.
        for x in 14..=20 {
            changes.push(CellChange {
                x,
                y: 2,
                z: 25,
                block: Block::new("grass", 0),
            });
            changes.push(CellChange {
                x,
                y: 3,
                z: 25,
                block: Block::AIR,
            });
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let bobber = [17.5, 2.92, 21.5];
        let focus = glam::Vec3::from(bobber);
        let eye = glam::Vec3::new(17.5, 2.2 + body::EYE_HEIGHT, 26.4);
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        self.fish_bobber = Some((bobber, 8.0, 0.15));
        self.note_shot_at = Some(bobber);
        self.note_shot_pitch = 8;
        self.note_shot_book = false;
        self.note_shot_brew = true;
        self.drops.brew_bubbles(bobber);
        self.drops.bobber(bobber);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Splash, bobber);
        self.gui.push_chat("cast");
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("fishing_rod");
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(17 + dx * 8, 21 + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    pub fn arm_ladder_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (lx, lz) = rebuild_world::SHAFT_LADDER;
        let mut top = 4;
        for y in (0..80).rev() {
            if self.world.block_at(lx, y, lz).id == "ladder" {
                top = y;
                break;
            }
        }
        let mid = top.saturating_sub(1).max(1);
        // Stand on the ladder mid-climb, looking down at coal.
        let eye = glam::Vec3::new(
            lx as f32 + 0.5,
            mid as f32 + 0.35 + body::EYE_HEIGHT,
            lz as f32 + 0.55,
        );
        let focus = glam::Vec3::new(lx as f32 - 1.2, 1.2, lz as f32 + 1.0);
        self.camera = Camera::looking_at(eye, focus);
        self.camera.on_ground = false;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.found_mine = true;
        self.gui.push_chat("climb");
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(lx + dx * 8, lz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shaft anvil panel mid-clang after a repair, then quit with a PNG.
    pub fn arm_anvil_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (ax, az) = rebuild_world::SHAFT_ANVIL;
        let mut ay = 4;
        for y in 0..80 {
            if self.world.block_at(ax, y, az).id == "anvil" {
                ay = y;
                break;
            }
        }
        self.chunk_bake.touch(ax, az);
        let focus = glam::Vec3::new(ax as f32 + 0.5, ay as f32 + 0.85, az as f32 + 0.5);
        let eye = glam::Vec3::new(
            ax as f32 + 2.6,
            ay as f32 + 1.35 + body::EYE_HEIGHT,
            az as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let clang_at = [ax as f32 + 0.5, ay as f32 + 0.95, az as f32 + 0.5];
        self.note_shot_at = Some(clang_at);
        self.note_shot_pitch = 1;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(clang_at, [0.72, 0.72, 0.78]);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(1), clang_at);
        let _ = self.gui.open_anvil([ax, ay, az]);
        self.gui.note_anvil("repaired");
        self.gui.push_chat("repaired");
        self.found_mine = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("iron_pickaxe");
        // Wear it, then refill so the tip reads as a freshly repaired pick.
        for _ in 0..8 {
            let _ = self.hotbar.wear_use(0);
        }
        let _ = self.hotbar.collect("iron_ore");
        let _ = self.hotbar.repair_pick();
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(ax + dx * 8, az + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shaft smith panel mid-spark after an iron upgrade, then quit with a PNG.
    pub fn arm_smith_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (sx, sz) = rebuild_world::SHAFT_SMITH;
        let mut sy = 4;
        for y in 0..80 {
            if self.world.block_at(sx, y, sz).id == "smithing_table" {
                sy = y;
                break;
            }
        }
        self.chunk_bake.touch(sx, sz);
        let focus = glam::Vec3::new(sx as f32 + 0.5, sy as f32 + 0.85, sz as f32 + 0.5);
        let eye = glam::Vec3::new(
            sx as f32 - 2.8,
            sy as f32 + 1.35 + body::EYE_HEIGHT,
            sz as f32 + 2.6,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let spark_at = [sx as f32 + 0.5, sy as f32 + 0.95, sz as f32 + 0.5];
        self.note_shot_at = Some(spark_at);
        self.note_shot_pitch = 4;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(spark_at, [0.86, 0.58, 0.36]);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(4), spark_at);
        let _ = self.gui.open_smith([sx, sy, sz]);
        self.gui.note_smith("iron");
        self.gui.push_chat("iron");
        self.found_mine = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("stone_pickaxe");
        let _ = self.hotbar.collect("iron_ore");
        let _ = self.hotbar.smith_pick();
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(sx + dx * 8, sz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shrine brew panel mid-bubble with a healing potion, then quit with a PNG.
    pub fn arm_brew_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (bx, bz) = rebuild_world::SHRINE_BREW;
        let mut by = 4;
        for y in 0..80 {
            if self.world.block_at(bx, y, bz).id == "brewing_stand" {
                by = y;
                break;
            }
        }
        self.chunk_bake.touch(bx, bz);
        let focus = glam::Vec3::new(bx as f32 + 0.5, by as f32 + 0.95, bz as f32 + 0.5);
        let eye = glam::Vec3::new(
            bx as f32 - 2.6,
            by as f32 + 1.4 + body::EYE_HEIGHT,
            bz as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let bubble_at = [bx as f32 + 0.5, by as f32 + 0.95, bz as f32 + 0.5];
        self.note_shot_at = Some(bubble_at);
        self.note_shot_pitch = 8;
        self.note_shot_book = false;
        self.note_shot_brew = true;
        self.drops.brew_bubbles(bubble_at);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(8), bubble_at);
        let _ = self.gui.open_brewing([bx, by, bz]);
        self.gui.note_brew("brewed");
        self.gui.push_chat("brewed");
        self.found_shrine = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("water_bucket");
        let _ = self.hotbar.collect("nether_wart_block");
        let _ = self.hotbar.brew_potion();
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(bx + dx * 8, bz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shaft grindstone panel mid-spark after clearing Efficiency, then quit with a PNG.
    pub fn arm_grind_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(-2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (gx, gz) = rebuild_world::SHAFT_GRIND;
        let mut gy = 4;
        for y in 0..80 {
            if self.world.block_at(gx, y, gz).id == "grindstone" {
                gy = y;
                break;
            }
        }
        self.chunk_bake.touch(gx, gz);
        let focus = glam::Vec3::new(gx as f32 + 0.5, gy as f32 + 0.85, gz as f32 + 0.5);
        let eye = glam::Vec3::new(
            gx as f32 + 2.8,
            gy as f32 + 1.35 + body::EYE_HEIGHT,
            gz as f32 - 2.4,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let spark_at = [gx as f32 + 0.5, gy as f32 + 0.95, gz as f32 + 0.5];
        self.note_shot_at = Some(spark_at);
        self.note_shot_pitch = 3;
        self.note_shot_book = false;
        self.note_shot_brew = false;
        self.drops.puff(spark_at, [0.78, 0.76, 0.70]);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(3), spark_at);
        let _ = self.gui.open_grind([gx, gy, gz]);
        self.gui.note_grind("ground");
        self.gui.push_chat("ground");
        self.found_mine = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("iron_pickaxe");
        // Keen then clear so the tip reads as a freshly ground pick.
        let _ = self.hotbar.collect("bookshelf");
        let _ = self.hotbar.enchant_pick();
        let _ = self.hotbar.grind_pick();
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(gx + dx * 8, gz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shrine enchant panel mid-swirl with Efficiency tip, then quit with a PNG.
    pub fn arm_enchant_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (ex, ez) = rebuild_world::SHRINE_ENCHANT;
        let mut ey = 4;
        for y in 0..80 {
            if self.world.block_at(ex, y, ez).id == "enchanting_table" {
                ey = y;
                break;
            }
        }
        self.chunk_bake.touch(ex, ez);
        let focus = glam::Vec3::new(ex as f32 + 0.5, ey as f32 + 0.95, ez as f32 + 0.5);
        let eye = glam::Vec3::new(
            ex as f32 + 2.6,
            ey as f32 + 1.4 + body::EYE_HEIGHT,
            ez as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let swirl_at = [ex as f32 + 0.5, ey as f32 + 0.95, ez as f32 + 0.5];
        self.note_shot_at = Some(swirl_at);
        self.note_shot_pitch = 10;
        self.note_shot_book = true;
        self.note_shot_brew = false;
        self.drops.book_swirl(swirl_at);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(10), swirl_at);
        let _ = self.gui.open_enchanting([ex, ey, ez]);
        self.gui.note_enchant("efficiency");
        self.gui.push_chat("efficiency");
        self.found_shrine = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("iron_pickaxe");
        let _ = self.hotbar.collect("bookshelf");
        let _ = self.hotbar.enchant_pick();
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(ex + dx * 8, ez + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the camp cauldron panel mid-pour with a mote, then quit with a PNG.
    pub fn arm_cauldron_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (kx, kz) = rebuild_world::CAMP_CAULDRON;
        let mut ky = 4;
        for y in 0..80 {
            if self.world.block_at(kx, y, kz).id == "cauldron" {
                ky = y;
                break;
            }
        }
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: kx,
                y: ky,
                z: kz,
                block: Block::new("cauldron", 1),
            }],
        );
        self.chunk_bake.touch(kx, kz);
        let focus = glam::Vec3::new(kx as f32 + 0.5, ky as f32 + 0.85, kz as f32 + 0.5);
        let eye = glam::Vec3::new(
            kx as f32 - 2.6,
            ky as f32 + 1.35 + body::EYE_HEIGHT,
            kz as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [kx as f32 + 0.5, ky as f32 + 1.05, kz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = 0;
        self.drops.note(mote_at, 0);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(0), mote_at);
        let _ = self.gui.open_cauldron([kx, ky, kz], 1);
        self.gui.note_cauldron("level 1");
        self.gui.push_chat("level 1");
        self.found_camp = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("water_bucket");
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(kx + dx * 8, kz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the camp beehive panel mid-fill with a mote, then quit with a PNG.
    pub fn arm_hive_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (hx, hz) = rebuild_world::CAMP_HIVE;
        let mut hy = 4;
        for y in 0..80 {
            if self.world.block_at(hx, y, hz).id == "beehive" {
                hy = y;
                break;
            }
        }
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: hx,
                y: hy,
                z: hz,
                block: Block::new("beehive", 1),
            }],
        );
        self.chunk_bake.touch(hx, hz);
        let focus = glam::Vec3::new(hx as f32 + 0.5, hy as f32 + 0.85, hz as f32 + 0.5);
        let eye = glam::Vec3::new(
            hx as f32 + 2.8,
            hy as f32 + 1.35 + body::EYE_HEIGHT,
            hz as f32 + 2.6,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [hx as f32 + 0.5, hy as f32 + 1.05, hz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = 7;
        self.drops.note(mote_at, 7);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(7), mote_at);
        let _ = self.gui.open_hive([hx, hy, hz], 1);
        self.gui.note_hive("level 1");
        self.gui.push_chat("level 1");
        self.found_camp = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("flower");
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(hx + dx * 8, hz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the camp compost panel mid-fill with a mote, then quit with a PNG.
    pub fn arm_compost_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::CAMP_COMPOST;
        let mut cy = 4;
        for y in 0..80 {
            if self.world.block_at(cx, y, cz).id == "composter" {
                cy = y;
                break;
            }
        }
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: cx,
                y: cy,
                z: cz,
                block: Block::new("composter", 1),
            }],
        );
        self.chunk_bake.touch(cx, cz);
        let focus = glam::Vec3::new(cx as f32 + 0.5, cy as f32 + 0.85, cz as f32 + 0.5);
        let eye = glam::Vec3::new(
            cx as f32 + 0.5,
            cy as f32 + 1.35 + body::EYE_HEIGHT,
            cz as f32 + 3.2,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [cx as f32 + 0.5, cy as f32 + 1.05, cz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = 2;
        self.drops.note(mote_at, 2);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(2), mote_at);
        let _ = self.gui.open_compost([cx, cy, cz], 1);
        self.gui.note_compost("level 1");
        self.gui.push_chat("level 1");
        self.found_camp = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("flower");
        self.hotbar_dirty = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the shrine bell panel mid-ring with a mote, then quit with a PNG.
    pub fn arm_bell_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (bx, bz) = rebuild_world::SHRINE_BELL;
        let mut by = 4;
        for y in 0..80 {
            if self.world.block_at(bx, y, bz).id == "bell" {
                by = y;
                break;
            }
        }
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: bx,
                y: by,
                z: bz,
                block: Block::new("bell", 1),
            }],
        );
        self.chunk_bake.touch(bx, bz);
        let focus = glam::Vec3::new(bx as f32 + 0.5, by as f32 + 0.9, bz as f32 + 0.5);
        let eye = glam::Vec3::new(
            bx as f32 - 2.4,
            by as f32 + 1.35 + body::EYE_HEIGHT,
            bz as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [bx as f32 + 0.5, by as f32 + 1.05, bz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = 12;
        self.drops.note(mote_at, 12);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(12), mote_at);
        let _ = self.gui.open_bell([bx, by, bz]);
        self.gui.note_bell("ring");
        self.gui.push_chat("ring");
        self.found_shrine = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(bx + dx * 8, bz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Insert a disc into the camp jukebox mid-melody mote, then quit with a PNG.
    pub fn arm_jukebox_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (jx, jz) = rebuild_world::CAMP_JUKEBOX;
        let mut jy = 4;
        for y in 0..80 {
            if self.world.block_at(jx, y, jz).id == "jukebox" {
                jy = y;
                break;
            }
        }
        let mut changes = vec![CellChange {
            x: jx,
            y: jy,
            z: jz,
            block: Block::new("jukebox", 1),
        }];
        // Clear a south look-corridor so the mote reads above the box.
        for z in (jz + 1)..=(jz + 4) {
            for y in jy..=(jy + 3) {
                changes.push(CellChange {
                    x: jx,
                    y,
                    z,
                    block: Block::AIR,
                });
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }
        let focus = glam::Vec3::new(jx as f32 + 0.5, jy as f32 + 1.15, jz as f32 + 0.5);
        let eye = glam::Vec3::new(
            jx as f32 + 0.5,
            jy as f32 + 1.4 + body::EYE_HEIGHT,
            jz as f32 + 3.6,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [jx as f32 + 0.5, jy as f32 + 1.05, jz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = 12;
        self.drops.note(mote_at, 12);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Disc, mote_at);
        self.gui.push_chat("playing");
        self.found_camp = true;
        self.hotbar.install_saved(0, &[]);
        let _ = self.hotbar.give_stack("music_disc");
        self.hotbar_dirty = true;
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(jx + dx * 8, jz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the camp hanging-sign panel (path tip), then quit with a PNG.
    pub fn arm_hang_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (sx, sz) = rebuild_world::CAMP_SIGN;
        let mut sy = 5;
        for y in 0..80 {
            if self.world.block_at(sx, y, sz).id == "hanging_sign" {
                sy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(sx as f32 + 0.5, sy as f32 + 0.55, sz as f32 + 0.5);
        let eye = glam::Vec3::new(
            sx as f32 + 0.5,
            sy as f32 + 0.4 + body::EYE_HEIGHT,
            sz as f32 + 2.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.gui.open_hang([sx, sy, sz]);
        self.gui.note_hang(gui::HANGING_LINE);
        self.gui.push_chat(gui::HANGING_LINE);
        self.found_camp = true;
        self.set_capture(false);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(sx + dx * 8, sz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Hit the camp note block mid-mote (pitch-tinted), then quit with a PNG.
    pub fn arm_note_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (nx, nz) = rebuild_world::CAMP_NOTE;
        let mut ny = 4;
        for y in 0..80 {
            if self.world.block_at(nx, y, nz).id == "note_block" {
                ny = y;
                break;
            }
        }
        // Mid-scale pitch so the mote reads warm-green in the PNG.
        let pitch = 7u8;
        apply_changes(
            &mut self.world,
            &[CellChange {
                x: nx,
                y: ny,
                z: nz,
                block: Block::new("note_block", pitch),
            }],
        );
        self.chunk_bake.touch(nx, nz);
        let focus = glam::Vec3::new(nx as f32 + 0.5, ny as f32 + 0.85, nz as f32 + 0.5);
        let eye = glam::Vec3::new(
            nx as f32 - 1.6,
            ny as f32 + 1.35 + body::EYE_HEIGHT,
            nz as f32 - 2.4,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch_look) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch_look;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch_look;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);

        let mote_at = [nx as f32 + 0.5, ny as f32 + 1.05, nz as f32 + 0.5];
        self.note_shot_at = Some(mote_at);
        self.note_shot_pitch = pitch;
        self.drops.note(mote_at, pitch);
        self.rebuild_drops();
        self.play_world(crate::audio::Sound::Note(pitch), mote_at);
        self.gui.push_chat(crate::audio::note_name(pitch));
        self.found_camp = true;
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(nx + dx * 8, nz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Open the map sheet by the marsh clay bank (teal mark), then quit with a PNG.
    pub fn arm_map_clay_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(0, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::CLAY_BANK;
        // Stand north of the bank so the teal mark sits below center on the sheet.
        let eye = glam::Vec3::new(
            cx as f32 + 0.5,
            3.0 + body::EYE_HEIGHT,
            cz as f32 - 6.0,
        );
        let yaw = std::f32::consts::FRAC_PI_2; // look +Z toward the bank
        self.camera.restore(eye, yaw, -0.2);
        let _ = self.hotbar.give_stack("map");
        self.hotbar_dirty = true;
        self.map_open = true;
        self.found_clay = true;
        self.gui.push_chat("clay teal");

        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = -0.2;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = -0.2;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Look at the marsh clay bank under water, then quit with a PNG.
    pub fn arm_clay_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(0, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (cx, cz) = rebuild_world::CLAY_BANK;
        let mut cy = 2;
        for y in 0..16 {
            if self.world.block_at(cx, y, cz).id == "clay" {
                cy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(cx as f32 + 0.5, cy as f32 + 0.6, cz as f32 + 0.5);
        let eye = glam::Vec3::new(
            cx as f32 + 0.5,
            cy as f32 + 1.6 + body::EYE_HEIGHT,
            cz as f32 - 3.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(cx + dx * 8, cz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Hold a decorated pot after shaping bricks+clay at the shrine table.
    pub fn arm_pot_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (tx, tz) = rebuild_world::SHRINE_TABLE;
        let mut ty = 4;
        for y in 0..80 {
            if self.world.block_at(tx, y, tz).id == "crafting_table" {
                ty = y;
                break;
            }
        }
        let _ = self.hotbar.collect("bricks");
        let _ = self.hotbar.collect("clay");
        let word = self.hotbar.shape_pot();
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        let focus = glam::Vec3::new(tx as f32 + 0.5, ty as f32 + 1.0, tz as f32 + 0.5);
        let eye = glam::Vec3::new(
            tx as f32 - 1.0,
            ty as f32 + 1.35 + body::EYE_HEIGHT,
            tz as f32 + 4.0,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(tx + dx * 8, tz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Look at the shrine furnace beside clay, then quit with a PNG.
    pub fn arm_furnace_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, 2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (fx, fz) = rebuild_world::SHRINE_FURNACE;
        let mut fy = 4;
        for y in 0..80 {
            if self.world.block_at(fx, y, fz).id == "furnace" {
                fy = y;
                break;
            }
        }
        let _ = self.hotbar.collect("clay");
        let _ = self.hotbar.select_digit(2);
        let word = self.hotbar.furnace_now();
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        let fire = glam::Vec3::new(fx as f32 + 0.5, fy as f32 + 0.85, fz as f32 + 0.5);
        self.drops.smoke([fire.x, fire.y + 0.2, fire.z]);
        self.drops.smoke([fire.x + 0.06, fire.y + 0.28, fire.z - 0.04]);
        self.rebuild_drops();
        let focus = glam::Vec3::new(fx as f32 + 0.5, fy as f32 + 1.0, fz as f32 + 0.5);
        let eye = glam::Vec3::new(
            fx as f32 - 1.2,
            fy as f32 + 1.35 + body::EYE_HEIGHT,
            fz as f32 + 4.2,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(fx + dx * 8, fz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    /// Hold stone over a blast furnace after a one-step blast, then quit with a PNG.
    pub fn arm_blast_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.45);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let fx = 8;
        let fy = 2;
        let fz = 10;
        let mut changes = Vec::new();
        for z in 8..=12 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 2..=6 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        changes.push(CellChange {
            x: fx,
            y: fy,
            z: fz,
            block: Block::new("blast_furnace", 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let _ = self.hotbar.select_digit(1);
        let word = self.hotbar.blast_now();
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        let fire = glam::Vec3::new(fx as f32 + 0.5, fy as f32 + 0.85, fz as f32 + 0.5);
        self.drops.smoke([fire.x, fire.y + 0.2, fire.z]);
        self.drops.smoke([fire.x - 0.08, fire.y + 0.3, fire.z + 0.05]);
        self.rebuild_drops();
        let eye = glam::Vec3::new(
            fx as f32 + 0.5,
            fy as f32 + 1.05 + body::EYE_HEIGHT,
            fz as f32 - 3.4,
        );
        let look = glam::Vec3::new(fire.x, fire.y + 0.55, fire.z);
        self.camera = Camera::looking_at(eye, look);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    /// Look at the camp smoker beside the fire, then quit with a PNG.
    pub fn arm_camp_smoker_shot(&mut self, path: std::path::PathBuf) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        rebuild_world::seed_terrain_ring(&mut self.world);
        let _ = self
            .world
            .replace_chunk(rebuild_world::generate_chunk(2, -2));
        self.clock.set_time_of_day(0.42);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let (fx, fz) = rebuild_world::CAMP_FIRE;
        let (sx, sz) = rebuild_world::CAMP_SMOKER;
        let mut fy = 4;
        for y in 0..80 {
            if self.world.block_at(fx, y, fz).id == "campfire"
                || self.world.block_at(sx, y, sz).id == "smoker"
            {
                fy = y;
                break;
            }
        }
        let focus = glam::Vec3::new(
            (fx as f32 + sx as f32) * 0.5 + 0.5,
            fy as f32 + 1.0,
            fz as f32 + 0.5,
        );
        let eye = glam::Vec3::new(
            fx as f32 + 0.5,
            fy as f32 + 1.35 + body::EYE_HEIGHT,
            fz as f32 + 4.8,
        );
        self.camera = Camera::looking_at(eye, focus);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        for dx in -1..=1 {
            for dz in -1..=1 {
                self.chunk_bake.touch(fx + dx * 8, fz + dz * 8);
            }
        }
        let _ = self.window.focus_window();
    }

    fn arm_heat_cook_shot(&mut self, path: std::path::PathBuf, heat: &str) {
        self.cook_shot_path = Some(path);
        self.cook_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.45);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let fx = 8;
        let fy = 2;
        let fz = 10;
        let mut changes = Vec::new();
        for z in 8..=12 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 2..=6 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        changes.push(CellChange {
            x: fx,
            y: fy,
            z: fz,
            block: Block::new(heat, 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let _ = self.hotbar.give_stack("raw");
        let _ = self.hotbar.select_digit(2);
        let word = self.hotbar.cook_raw_now();
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        let fire = glam::Vec3::new(fx as f32 + 0.5, fy as f32 + 0.85, fz as f32 + 0.5);
        self.drops.smoke([fire.x, fire.y + 0.2, fire.z]);
        self.drops.smoke([fire.x + 0.1, fire.y + 0.35, fire.z - 0.05]);
        self.rebuild_drops();
        let eye = glam::Vec3::new(
            fx as f32 + 0.5,
            fy as f32 + 1.05 + body::EYE_HEIGHT,
            fz as f32 - 3.4,
        );
        let look = glam::Vec3::new(fire.x, fire.y + 0.55, fire.z);
        self.camera = Camera::looking_at(eye, look);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.cook_shot_eye = eye;
        self.cook_shot_yaw = yaw;
        self.cook_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_cook_shot_camera(&mut self) {
        if self.cook_shot_path.is_none() {
            return;
        }
        self.camera.restore(self.cook_shot_eye, self.cook_shot_yaw, self.cook_shot_pitch);
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.cook_shot_eye;
        self.posed_yaw = self.cook_shot_yaw;
        self.posed_pitch = self.cook_shot_pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
    }

    fn poll_cook_shot(&mut self) {
        let Some(path) = self.cook_shot_path.clone() else {
            return;
        };
        self.pin_cook_shot_camera();
        self.cook_shot_frames = self.cook_shot_frames.saturating_add(1);
        if self.cook_shot_frames % 5 == 0 {
            if let Some(at) = self.note_shot_at {
                if self.note_shot_book {
                    self.drops.book_swirl(at);
                } else if self.note_shot_brew {
                    self.drops.brew_bubbles(at);
                    if self.fish_bobber.is_some() {
                        self.drops.bobber(at);
                    }
                } else {
                    self.drops.note(at, self.note_shot_pitch);
                }
            } else {
                self.drops.smoke([8.5, 2.95, 10.5]);
            }
            self.rebuild_drops();
        }
        if self.cook_shot_frames < 100 || !self.chunk_bake.is_idle() {
            if self.cook_shot_frames > 500 {
                eprintln!("cook shot timed out");
                self.cook_shot_path = None;
                self.note_shot_at = None;
                self.note_shot_book = false;
                self.note_shot_brew = false;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("cook shot wrote {}", path.display());
                self.cook_shot_path = None;
                self.note_shot_at = None;
                self.note_shot_book = false;
                self.note_shot_brew = false;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("cook shot failed: {err}");
                if self.cook_shot_frames > 220 {
                    self.cook_shot_path = None;
                    self.note_shot_at = None;
                    self.note_shot_book = false;
                    self.note_shot_brew = false;
                    self.quit = true;
                }
            }
        }
    }

    /// Look at a lit campfire with rising smoke puffs, then quit with a PNG.
    pub fn arm_campfire_smoke_shot(&mut self, path: std::path::PathBuf) {
        self.campfire_smoke_shot_path = Some(path);
        self.campfire_smoke_shot_frames = 0;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.45);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        // Clear pad + campfire on the demo stairs overlook so the shot is stable.
        let fx = 8;
        let fy = 2;
        let fz = 10;
        let mut changes = Vec::new();
        for z in 8..=12 {
            for x in 6..=10 {
                changes.push(CellChange {
                    x,
                    y: 1,
                    z,
                    block: Block::new("grass", 0),
                });
                for y in 2..=6 {
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: Block::AIR,
                    });
                }
            }
        }
        changes.push(CellChange {
            x: fx,
            y: fy,
            z: fz,
            block: Block::new("campfire", 0),
        });
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let fire = glam::Vec3::new(fx as f32 + 0.5, fy as f32 + 0.85, fz as f32 + 0.5);
        // Stand back and a bit low so the rising column reads against the sky.
        let eye = glam::Vec3::new(
            fx as f32 + 0.5,
            fy as f32 + 1.15 + body::EYE_HEIGHT,
            fz as f32 - 4.2,
        );
        let look = glam::Vec3::new(fire.x, fire.y + 1.4, fire.z);
        self.camera = Camera::looking_at(eye, look);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.campfire_smoke_shot_eye = eye;
        self.campfire_smoke_shot_yaw = yaw;
        self.campfire_smoke_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.crackle_at = Some([fire.x, fire.y, fire.z]);
        for _ in 0..8 {
            self.drops.smoke([fire.x, fire.y + 0.2, fire.z]);
        }
        self.rebuild_drops();
        let _ = self.window.focus_window();
    }

    fn pin_campfire_smoke_shot_camera(&mut self) {
        if self.campfire_smoke_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.campfire_smoke_shot_eye,
            self.campfire_smoke_shot_yaw,
            self.campfire_smoke_shot_pitch,
        );
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.campfire_smoke_shot_eye;
        self.posed_yaw = self.campfire_smoke_shot_yaw;
        self.posed_pitch = self.campfire_smoke_shot_pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
    }

    fn poll_campfire_smoke_shot(&mut self) {
        let Some(path) = self.campfire_smoke_shot_path.clone() else {
            return;
        };
        self.pin_campfire_smoke_shot_camera();
        self.campfire_smoke_shot_frames = self.campfire_smoke_shot_frames.saturating_add(1);
        if self.campfire_smoke_shot_frames % 4 == 0 {
            self.drops.smoke([8.5, 2.95, 10.5]);
            self.drops.smoke([8.55, 3.05, 10.45]);
            self.rebuild_drops();
        }
        if self.campfire_smoke_shot_frames < 120 || !self.chunk_bake.is_idle() {
            if self.campfire_smoke_shot_frames > 600 {
                eprintln!("campfire smoke shot timed out");
                self.campfire_smoke_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("campfire smoke shot wrote {}", path.display());
                self.campfire_smoke_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("campfire smoke shot failed: {err}");
                if self.campfire_smoke_shot_frames > 250 {
                    self.campfire_smoke_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    pub fn arm_break_shot(&mut self, path: std::path::PathBuf) {
        self.break_shot_path = Some(path);
        self.break_shot_frames = 0;
        self.break_shot_phase = BreakShotPhase::WaitingIdle;
        self.break_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        self.break_kick = 0.0;
        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.break_shot_eye = eye;
        self.break_shot_yaw = yaw;
        self.break_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_break_shot_camera(&mut self) {
        if self.break_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.break_shot_eye,
            self.break_shot_yaw,
            self.break_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.break_shot_eye;
        self.posed_yaw = self.break_shot_yaw;
        self.posed_pitch = self.break_shot_pitch;
        self.pose_dirty = true;
        match self.break_shot_phase {
            BreakShotPhase::WaitingIdle | BreakShotPhase::Idle => {
                self.break_kick = 0.0;
            }
            BreakShotPhase::WaitingBreak => {
                self.break_kick = 1.0;
            }
        }
    }

    /// Idle FOV then place-click FOV from the stairs overlook, stitched side by side.
    pub fn arm_place_shot(&mut self, path: std::path::PathBuf) {
        self.place_shot_path = Some(path);
        self.place_shot_frames = 0;
        self.place_shot_phase = PlaceShotPhase::WaitingIdle;
        self.place_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let _ = self.hotbar.select_digit(1);
        let _ = self.hotbar.give_stack("cobblestone");
        self.slot_punch = 0.0;
        self.swap_kick = 0.0;

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.place_shot_eye = eye;
        self.place_shot_yaw = yaw;
        self.place_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_place_shot_camera(&mut self) {
        if self.place_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.place_shot_eye,
            self.place_shot_yaw,
            self.place_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.place_shot_eye;
        self.posed_yaw = self.place_shot_yaw;
        self.posed_pitch = self.place_shot_pitch;
        self.pose_dirty = true;
        match self.place_shot_phase {
            PlaceShotPhase::WaitingIdle | PlaceShotPhase::Idle => {
                self.slot_punch = 0.0;
                self.swap_kick = 0.0;
            }
            PlaceShotPhase::WaitingPlace => {
                self.slot_punch = 1.0;
                self.swap_kick = 0.0;
            }
        }
    }

    /// Idle FOV then hotbar-swap FOV from the stairs overlook, stitched side by side.
    pub fn arm_swap_shot(&mut self, path: std::path::PathBuf) {
        self.swap_shot_path = Some(path);
        self.swap_shot_frames = 0;
        self.swap_shot_phase = SwapShotPhase::WaitingIdle;
        self.swap_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let _ = self.hotbar.select_digit(1);
        let _ = self.hotbar.give_stack("cobblestone");
        let _ = self.hotbar.select_digit(2);
        let _ = self.hotbar.give_stack("dirt");
        let _ = self.hotbar.select_digit(1);
        self.slot_punch = 0.0;
        self.swap_kick = 0.0;

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.swap_shot_eye = eye;
        self.swap_shot_yaw = yaw;
        self.swap_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_swap_shot_camera(&mut self) {
        if self.swap_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.swap_shot_eye,
            self.swap_shot_yaw,
            self.swap_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.swap_shot_eye;
        self.posed_yaw = self.swap_shot_yaw;
        self.posed_pitch = self.swap_shot_pitch;
        self.pose_dirty = true;
        match self.swap_shot_phase {
            SwapShotPhase::WaitingIdle | SwapShotPhase::Idle => {
                let _ = self.hotbar.select_digit(1);
                self.slot_punch = 0.0;
                self.swap_kick = 0.0;
            }
            SwapShotPhase::WaitingSwap => {
                let _ = self.hotbar.select_digit(2);
                self.slot_punch = 0.0;
                self.swap_kick = 1.0;
            }
        }
    }

    /// Grounded FOV then jump takeoff FOV from the stairs overlook, stitched side by side.
    pub fn arm_jump_shot(&mut self, path: std::path::PathBuf) {
        self.jump_shot_path = Some(path);
        self.jump_shot_frames = 0;
        self.jump_shot_phase = JumpShotPhase::WaitingGround;
        self.jump_shot_ground = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        self.camera.velocity_y = 0.0;
        self.input.jump = false;
        self.jump_kick = 0.0;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.jump_shot_eye = eye;
        self.jump_shot_yaw = yaw;
        self.jump_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_jump_shot_camera(&mut self) {
        if self.jump_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.jump_shot_eye,
            self.jump_shot_yaw,
            self.jump_shot_pitch,
        );
        self.posed_eye = self.jump_shot_eye;
        self.posed_yaw = self.jump_shot_yaw;
        self.posed_pitch = self.jump_shot_pitch;
        self.pose_dirty = true;
        match self.jump_shot_phase {
            JumpShotPhase::WaitingGround | JumpShotPhase::Idle => {
                self.camera.on_ground = true;
                self.camera.velocity_y = 0.0;
                self.input.jump = false;
                self.jump_kick = 0.0;
            }
            JumpShotPhase::WaitingAir => {
                self.camera.on_ground = false;
                self.camera.velocity_y = 4.0;
                self.input.jump = true;
                self.jump_kick = 1.0;
            }
        }
    }

    /// Idle view then mid-chew FOV/plate from the stairs overlook, stitched side by side.
    pub fn arm_eat_shot(&mut self, path: std::path::PathBuf) {
        self.eat_shot_path = Some(path);
        self.eat_shot_frames = 0;
        self.eat_shot_phase = EatShotPhase::WaitingIdle;
        self.eat_shot_idle = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let _ = self.hotbar.select_digit(1);
        let _ = self.hotbar.give_stack("apple");
        self.hunger.set_food(6);
        self.sip = 0.0;
        self.sipping = false;
        self.chew_phase = 0.0;
        self.burp_due = false;

        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.eat_shot_eye = eye;
        self.eat_shot_yaw = yaw;
        self.eat_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_eat_shot_camera(&mut self) {
        if self.eat_shot_path.is_none() {
            return;
        }
        let _ = self.hotbar.select_digit(1);
        if self.hotbar.slot(0) != Some("apple") {
            let _ = self.hotbar.give_stack("apple");
        }
        self.camera.restore(self.eat_shot_eye, self.eat_shot_yaw, self.eat_shot_pitch);
        self.camera.on_ground = true;
        self.posed_eye = self.eat_shot_eye;
        self.posed_yaw = self.eat_shot_yaw;
        self.posed_pitch = self.eat_shot_pitch;
        self.pose_dirty = true;
        match self.eat_shot_phase {
            EatShotPhase::WaitingIdle | EatShotPhase::Idle => {
                self.sip = 0.0;
                self.sipping = false;
                self.chew_phase = 0.0;
                self.burp_due = false;
            }
            EatShotPhase::WaitingChew => {
                self.sip = 1.0;
                self.sipping = false;
                self.chew_phase = std::f32::consts::FRAC_PI_2;
                self.burp_due = true;
            }
        }
    }

    /// Fed HUD then starving hunger veil from the stairs overlook, stitched side by side.
    pub fn arm_hunger_shot(&mut self, path: std::path::PathBuf) {
        self.hunger_shot_path = Some(path);
        self.hunger_shot_frames = 0;
        self.hunger_shot_phase = HungerShotPhase::WaitingFed;
        self.hunger_shot_fed = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.4);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        self.hunger = body::Hunger::full();
        let eye = glam::Vec3::new(8.5, 3.2 + body::EYE_HEIGHT, 14.2);
        let target = glam::Vec3::new(8.5, 2.0, 8.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.hunger_shot_eye = eye;
        self.hunger_shot_yaw = yaw;
        self.hunger_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_hunger_shot_camera(&mut self) {
        if self.hunger_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.hunger_shot_eye,
            self.hunger_shot_yaw,
            self.hunger_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.hunger_shot_eye;
        self.posed_yaw = self.hunger_shot_yaw;
        self.posed_pitch = self.hunger_shot_pitch;
        self.pose_dirty = true;
        match self.hunger_shot_phase {
            HungerShotPhase::WaitingFed | HungerShotPhase::Idle => {
                self.hunger = body::Hunger::full();
            }
            HungerShotPhase::WaitingStarved => {
                self.hunger.set_food(0);
            }
        }
    }

    /// Cooling crosshair then ready crosshair with a held mace, stitched side by side.
    pub fn arm_attack_shot(&mut self, path: std::path::PathBuf) {
        self.attack_shot_path = Some(path);
        self.attack_shot_frames = 0;
        self.attack_shot_phase = AttackShotPhase::WaitingCool;
        self.attack_shot_cool = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.35);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        let _ = self.hotbar.select_digit(1);
        let _ = self.hotbar.give_stack("mace");
        self.mace_wait = body::MACE_WAIT;
        self.hit_kick = 0.0;

        let eye = glam::Vec3::new(8.5, 2.0 + body::EYE_HEIGHT, 12.5);
        let target = glam::Vec3::new(11.0, 2.4, 13.0);
        self.camera = Camera::looking_at(eye, target);
        self.camera.on_ground = true;
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.attack_shot_eye = eye;
        self.attack_shot_yaw = yaw;
        self.attack_shot_pitch = pitch;
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        let _ = self.window.focus_window();
    }

    fn pin_attack_shot_camera(&mut self) {
        if self.attack_shot_path.is_none() {
            return;
        }
        let _ = self.hotbar.select_digit(1);
        if self.hotbar.slot(0) != Some("mace") {
            let _ = self.hotbar.give_stack("mace");
        }
        self.camera.restore(
            self.attack_shot_eye,
            self.attack_shot_yaw,
            self.attack_shot_pitch,
        );
        self.camera.on_ground = true;
        self.posed_eye = self.attack_shot_eye;
        self.posed_yaw = self.attack_shot_yaw;
        self.posed_pitch = self.attack_shot_pitch;
        self.pose_dirty = true;
        match self.attack_shot_phase {
            AttackShotPhase::WaitingCool | AttackShotPhase::Idle => {
                self.mace_wait = body::MACE_WAIT;
                self.hit_kick = 0.35;
            }
            AttackShotPhase::WaitingReady => {
                self.mace_wait = 0.0;
                self.hit_kick = 0.0;
            }
        }
    }

    /// Midnight stone corridor: dirt (dark) then torch (lit), stitched side by side.
    pub fn arm_held_light_shot(&mut self, path: std::path::PathBuf) {
        self.held_light_shot_path = Some(path);
        self.held_light_shot_frames = 0;
        self.held_light_shot_phase = HeldLightShotPhase::WaitingDark;
        self.held_light_shot_dark = None;
        self.playing = true;
        self.menu.close_menu();
        self.apply_chosen_size();
        self.clock.set_time_of_day(0.0);
        let light = self.sun_light();
        self.wanted_sun = light.direction;
        self.wanted_lookups = crate::daylight::shadow_lookups(light.direction);

        // Long sealed corridor so a held torch falloff reads (near bright → far black).
        let mut changes = Vec::new();
        for z in 16..=46 {
            for y in 2..=5 {
                for x in 20..=22 {
                    let wall = x != 21 || y == 2 || y == 5 || z == 16 || z == 46;
                    changes.push(CellChange {
                        x,
                        y,
                        z,
                        block: if wall {
                            Block::new("stone", 0)
                        } else {
                            Block::AIR
                        },
                    });
                }
            }
        }
        apply_changes(&mut self.world, &changes);
        for change in &changes {
            self.chunk_bake.touch(change.x, change.z);
        }

        let eye = glam::Vec3::new(21.5, 3.0 + body::EYE_HEIGHT, 18.5);
        let target = glam::Vec3::new(21.5, 3.6, 44.0);
        self.camera = Camera::looking_at(eye, target);
        let (yaw, pitch) = self.camera.yaw_pitch();
        self.posed_eye = self.camera.eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
        self.held_light_shot_eye = eye;
        self.held_light_shot_yaw = yaw;
        self.held_light_shot_pitch = pitch;

        // Start dark: non-light item so LightField has no held emitter.
        self.hotbar.select_digit(1);
        let _ = self.hotbar.give_stack("dirt");
        self.hotbar_dirty = true;
        self.sync_held_light();
        let _ = self.window.focus_window();
    }

    fn pin_held_light_shot_camera(&mut self) {
        if self.held_light_shot_path.is_none() {
            return;
        }
        self.camera.restore(
            self.held_light_shot_eye,
            self.held_light_shot_yaw,
            self.held_light_shot_pitch,
        );
        self.camera.velocity_y = 0.0;
        self.posed_eye = self.camera.eye;
        self.posed_yaw = self.held_light_shot_yaw;
        self.posed_pitch = self.held_light_shot_pitch;
        self.pose_dirty = true;
        self.wanted_focus = chunk_of(self.camera.eye);
    }

    /// Seed the mesh light field from the selected hotbar placeable, or clear it.
    fn sync_held_light(&mut self) {
        let held = self.held_emitter();
        self.chunk_bake.set_held(held);
    }

    fn poll_zombie_shot(&mut self) {
        let Some(path) = self.shot_path.clone() else {
            return;
        };
        self.shot_frames = self.shot_frames.saturating_add(1);
        if self.shot_frames < 150 || self.zombie_count < 3 {
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("shot wrote {}", path.display());
                self.shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("shot failed: {err}");
                if self.shot_frames > 200 {
                    self.shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_settings_shot(&mut self) {
        let Some(path) = self.settings_shot_path.clone() else {
            return;
        };
        self.settings_shot_frames = self.settings_shot_frames.saturating_add(1);
        // Wait for a few egui frames so the dock + inspector paint.
        if self.settings_shot_frames < 45 {
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("settings shot wrote {}", path.display());
                self.settings_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("settings shot failed: {err}");
                if self.settings_shot_frames > 120 {
                    self.settings_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_creative_shot(&mut self) {
        let Some(path) = self.creative_shot_path.clone() else {
            return;
        };
        self.creative_shot_frames = self.creative_shot_frames.saturating_add(1);
        if self.creative_shot_frames < 40 {
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("creative shot wrote {}", path.display());
                self.creative_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("creative shot failed: {err}");
                if self.creative_shot_frames > 120 {
                    self.creative_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_keybinds_shot(&mut self) {
        let Some(path) = self.keybinds_shot_path.clone() else {
            return;
        };
        self.keybinds_shot_frames = self.keybinds_shot_frames.saturating_add(1);
        if self.keybinds_shot_frames < 50 {
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("keybinds shot wrote {}", path.display());
                self.keybinds_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("keybinds shot failed: {err}");
                if self.keybinds_shot_frames > 140 {
                    self.keybinds_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_night_spawn_shot(&mut self) {
        let Some(path) = self.night_spawn_shot_path.clone() else {
            return;
        };
        self.pin_night_spawn_shot_camera();
        self.night_spawn_shot_frames = self.night_spawn_shot_frames.saturating_add(1);
        if self.night_spawn_shot_frames < 150
            || self.zombie_count < 3
            || !self.chunk_bake.is_idle()
        {
            if self.night_spawn_shot_frames > 600 {
                eprintln!("night-spawn shot timed out");
                self.night_spawn_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("night-spawn shot wrote {}", path.display());
                self.night_spawn_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("night-spawn shot failed: {err}");
                if self.night_spawn_shot_frames > 250 {
                    self.night_spawn_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_sprint_shot(&mut self) {
        let Some(path) = self.sprint_shot_path.clone() else {
            return;
        };
        self.pin_sprint_shot_camera();
        self.sprint_shot_frames = self.sprint_shot_frames.saturating_add(1);
        if self.sprint_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.sprint_shot_frames > 600 {
                eprintln!("sprint shot timed out");
                self.sprint_shot_path = None;
                self.sprint_shot_phase = SprintShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.sprint_shot_phase {
            SprintShotPhase::WaitingWalk => match capture_window_rgba(&self.window) {
                Ok(walk) => {
                    self.sprint_shot_walk = Some(walk);
                    self.sprint_shot_frames = 0;
                    self.sprint_shot_phase = SprintShotPhase::WaitingSprint;
                }
                Err(err) => {
                    eprintln!("sprint walk capture failed: {err}");
                    if self.sprint_shot_frames > 400 {
                        self.sprint_shot_path = None;
                        self.sprint_shot_phase = SprintShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            SprintShotPhase::WaitingSprint => {
                let Some(walk) = self.sprint_shot_walk.take() else {
                    self.sprint_shot_path = None;
                    self.sprint_shot_phase = SprintShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(sprint) => match stitch_held_light_proof(&walk, &sprint, &path) {
                        Ok(()) => {
                            eprintln!("sprint shot wrote {}", path.display());
                            self.sprint_shot_path = None;
                            self.sprint_shot_phase = SprintShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("sprint stitch failed: {err}");
                            self.sprint_shot_path = None;
                            self.sprint_shot_phase = SprintShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("sprint capture failed: {err}");
                        self.sprint_shot_walk = Some(walk);
                        if self.sprint_shot_frames > 400 {
                            self.sprint_shot_path = None;
                            self.sprint_shot_phase = SprintShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            SprintShotPhase::Idle => {}
        }
    }

    fn poll_swim_shot(&mut self) {
        let Some(path) = self.swim_shot_path.clone() else {
            return;
        };
        self.pin_swim_shot_camera();
        self.swim_shot_frames = self.swim_shot_frames.saturating_add(1);
        if self.swim_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.swim_shot_frames > 600 {
                eprintln!("swim shot timed out");
                self.swim_shot_path = None;
                self.swim_shot_phase = SwimShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.swim_shot_phase {
            SwimShotPhase::WaitingDry => match capture_window_rgba(&self.window) {
                Ok(dry) => {
                    self.swim_shot_dry = Some(dry);
                    self.swim_shot_frames = 0;
                    self.swim_shot_phase = SwimShotPhase::WaitingWet;
                }
                Err(err) => {
                    eprintln!("swim dry capture failed: {err}");
                    if self.swim_shot_frames > 400 {
                        self.swim_shot_path = None;
                        self.swim_shot_phase = SwimShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            SwimShotPhase::WaitingWet => {
                let Some(dry) = self.swim_shot_dry.take() else {
                    self.swim_shot_path = None;
                    self.swim_shot_phase = SwimShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(wet) => match stitch_held_light_proof(&dry, &wet, &path) {
                        Ok(()) => {
                            eprintln!("swim shot wrote {}", path.display());
                            self.swim_shot_path = None;
                            self.swim_shot_phase = SwimShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("swim stitch failed: {err}");
                            self.swim_shot_path = None;
                            self.swim_shot_phase = SwimShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("swim wet capture failed: {err}");
                        self.swim_shot_dry = Some(dry);
                        if self.swim_shot_frames > 400 {
                            self.swim_shot_path = None;
                            self.swim_shot_phase = SwimShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            SwimShotPhase::Idle => {}
        }
    }

    fn poll_boat_shot(&mut self) {
        let Some(path) = self.boat_shot_path.clone() else {
            return;
        };
        self.pin_boat_shot_camera();
        self.boat_shot_frames = self.boat_shot_frames.saturating_add(1);
        if self.boat_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.boat_shot_frames > 600 {
                eprintln!("boat shot timed out");
                self.boat_shot_path = None;
                self.boat_shot_phase = BoatShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.boat_shot_phase {
            BoatShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.boat_shot_idle = Some(idle);
                    self.boat_shot_frames = 0;
                    self.boat_shot_phase = BoatShotPhase::WaitingRow;
                }
                Err(err) => {
                    eprintln!("boat idle capture failed: {err}");
                    if self.boat_shot_frames > 400 {
                        self.boat_shot_path = None;
                        self.boat_shot_phase = BoatShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            BoatShotPhase::WaitingRow => {
                let Some(idle) = self.boat_shot_idle.take() else {
                    self.boat_shot_path = None;
                    self.boat_shot_phase = BoatShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(row) => match stitch_held_light_proof(&idle, &row, &path) {
                        Ok(()) => {
                            eprintln!("boat shot wrote {}", path.display());
                            self.boat_shot_path = None;
                            self.boat_shot_phase = BoatShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("boat stitch failed: {err}");
                            self.boat_shot_path = None;
                            self.boat_shot_phase = BoatShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("boat row capture failed: {err}");
                        self.boat_shot_idle = Some(idle);
                        if self.boat_shot_frames > 400 {
                            self.boat_shot_path = None;
                            self.boat_shot_phase = BoatShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            BoatShotPhase::Idle => {}
        }
    }

    fn poll_cart_shot(&mut self) {
        let Some(path) = self.cart_shot_path.clone() else {
            return;
        };
        self.pin_cart_shot_camera();
        self.cart_shot_frames = self.cart_shot_frames.saturating_add(1);
        if self.cart_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.cart_shot_frames > 600 {
                eprintln!("cart shot timed out");
                self.cart_shot_path = None;
                self.cart_shot_phase = CartShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.cart_shot_phase {
            CartShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.cart_shot_idle = Some(idle);
                    self.cart_shot_frames = 0;
                    self.cart_shot_phase = CartShotPhase::WaitingRide;
                }
                Err(err) => {
                    eprintln!("cart idle capture failed: {err}");
                    if self.cart_shot_frames > 400 {
                        self.cart_shot_path = None;
                        self.cart_shot_phase = CartShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            CartShotPhase::WaitingRide => {
                let Some(idle) = self.cart_shot_idle.take() else {
                    self.cart_shot_path = None;
                    self.cart_shot_phase = CartShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(ride) => match stitch_held_light_proof(&idle, &ride, &path) {
                        Ok(()) => {
                            eprintln!("cart shot wrote {}", path.display());
                            self.cart_shot_path = None;
                            self.cart_shot_phase = CartShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("cart stitch failed: {err}");
                            self.cart_shot_path = None;
                            self.cart_shot_phase = CartShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("cart ride capture failed: {err}");
                        self.cart_shot_idle = Some(idle);
                        if self.cart_shot_frames > 400 {
                            self.cart_shot_path = None;
                            self.cart_shot_phase = CartShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            CartShotPhase::Idle => {}
        }
    }

    fn poll_horse_shot(&mut self) {
        let Some(path) = self.horse_shot_path.clone() else {
            return;
        };
        self.pin_horse_shot_camera();
        self.horse_shot_frames = self.horse_shot_frames.saturating_add(1);
        if self.horse_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.horse_shot_frames > 600 {
                eprintln!("horse shot timed out");
                self.horse_shot_path = None;
                self.horse_shot_phase = HorseShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.horse_shot_phase {
            HorseShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.horse_shot_idle = Some(idle);
                    self.horse_shot_frames = 0;
                    self.horse_shot_phase = HorseShotPhase::WaitingRide;
                }
                Err(err) => {
                    eprintln!("horse idle capture failed: {err}");
                    if self.horse_shot_frames > 400 {
                        self.horse_shot_path = None;
                        self.horse_shot_phase = HorseShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            HorseShotPhase::WaitingRide => {
                let Some(idle) = self.horse_shot_idle.take() else {
                    self.horse_shot_path = None;
                    self.horse_shot_phase = HorseShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(ride) => match stitch_held_light_proof(&idle, &ride, &path) {
                        Ok(()) => {
                            eprintln!("horse shot wrote {}", path.display());
                            self.horse_shot_path = None;
                            self.horse_shot_phase = HorseShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("horse stitch failed: {err}");
                            self.horse_shot_path = None;
                            self.horse_shot_phase = HorseShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("horse ride capture failed: {err}");
                        self.horse_shot_idle = Some(idle);
                        if self.horse_shot_frames > 400 {
                            self.horse_shot_path = None;
                            self.horse_shot_phase = HorseShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            HorseShotPhase::Idle => {}
        }
    }

    fn poll_attack_shot(&mut self) {
        let Some(path) = self.attack_shot_path.clone() else {
            return;
        };
        self.pin_attack_shot_camera();
        self.attack_shot_frames = self.attack_shot_frames.saturating_add(1);
        if self.attack_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.attack_shot_frames > 600 {
                eprintln!("attack shot timed out");
                self.attack_shot_path = None;
                self.attack_shot_phase = AttackShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.attack_shot_phase {
            AttackShotPhase::WaitingCool => match capture_window_rgba(&self.window) {
                Ok(cool) => {
                    self.attack_shot_cool = Some(cool);
                    self.attack_shot_frames = 0;
                    self.attack_shot_phase = AttackShotPhase::WaitingReady;
                }
                Err(err) => {
                    eprintln!("attack cool capture failed: {err}");
                    if self.attack_shot_frames > 400 {
                        self.attack_shot_path = None;
                        self.attack_shot_phase = AttackShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            AttackShotPhase::WaitingReady => {
                let Some(cool) = self.attack_shot_cool.take() else {
                    self.attack_shot_path = None;
                    self.attack_shot_phase = AttackShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(ready) => match stitch_held_light_proof(&cool, &ready, &path) {
                        Ok(()) => {
                            eprintln!("attack shot wrote {}", path.display());
                            self.attack_shot_path = None;
                            self.attack_shot_phase = AttackShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("attack stitch failed: {err}");
                            self.attack_shot_path = None;
                            self.attack_shot_phase = AttackShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("attack ready capture failed: {err}");
                        self.attack_shot_cool = Some(cool);
                        if self.attack_shot_frames > 400 {
                            self.attack_shot_path = None;
                            self.attack_shot_phase = AttackShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            AttackShotPhase::Idle => {}
        }
    }

    fn poll_hunger_shot(&mut self) {
        let Some(path) = self.hunger_shot_path.clone() else {
            return;
        };
        self.pin_hunger_shot_camera();
        self.hunger_shot_frames = self.hunger_shot_frames.saturating_add(1);
        if self.hunger_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.hunger_shot_frames > 600 {
                eprintln!("hunger shot timed out");
                self.hunger_shot_path = None;
                self.hunger_shot_phase = HungerShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.hunger_shot_phase {
            HungerShotPhase::WaitingFed => match capture_window_rgba(&self.window) {
                Ok(fed) => {
                    self.hunger_shot_fed = Some(fed);
                    self.hunger_shot_frames = 0;
                    self.hunger_shot_phase = HungerShotPhase::WaitingStarved;
                }
                Err(err) => {
                    eprintln!("hunger fed capture failed: {err}");
                    if self.hunger_shot_frames > 400 {
                        self.hunger_shot_path = None;
                        self.hunger_shot_phase = HungerShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            HungerShotPhase::WaitingStarved => {
                let Some(fed) = self.hunger_shot_fed.take() else {
                    self.hunger_shot_path = None;
                    self.hunger_shot_phase = HungerShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(starved) => match stitch_held_light_proof(&fed, &starved, &path) {
                        Ok(()) => {
                            eprintln!("hunger shot wrote {}", path.display());
                            self.hunger_shot_path = None;
                            self.hunger_shot_phase = HungerShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("hunger stitch failed: {err}");
                            self.hunger_shot_path = None;
                            self.hunger_shot_phase = HungerShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("hunger starved capture failed: {err}");
                        self.hunger_shot_fed = Some(fed);
                        if self.hunger_shot_frames > 400 {
                            self.hunger_shot_path = None;
                            self.hunger_shot_phase = HungerShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            HungerShotPhase::Idle => {}
        }
    }

    fn poll_eat_shot(&mut self) {
        let Some(path) = self.eat_shot_path.clone() else {
            return;
        };
        self.pin_eat_shot_camera();
        self.eat_shot_frames = self.eat_shot_frames.saturating_add(1);
        if self.eat_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.eat_shot_frames > 600 {
                eprintln!("eat shot timed out");
                self.eat_shot_path = None;
                self.eat_shot_phase = EatShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.eat_shot_phase {
            EatShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.eat_shot_idle = Some(idle);
                    self.eat_shot_frames = 0;
                    self.eat_shot_phase = EatShotPhase::WaitingChew;
                }
                Err(err) => {
                    eprintln!("eat idle capture failed: {err}");
                    if self.eat_shot_frames > 400 {
                        self.eat_shot_path = None;
                        self.eat_shot_phase = EatShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            EatShotPhase::WaitingChew => {
                let Some(idle) = self.eat_shot_idle.take() else {
                    self.eat_shot_path = None;
                    self.eat_shot_phase = EatShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(chew) => match stitch_held_light_proof(&idle, &chew, &path) {
                        Ok(()) => {
                            eprintln!("eat shot wrote {}", path.display());
                            self.eat_shot_path = None;
                            self.eat_shot_phase = EatShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("eat stitch failed: {err}");
                            self.eat_shot_path = None;
                            self.eat_shot_phase = EatShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("eat chew capture failed: {err}");
                        self.eat_shot_idle = Some(idle);
                        if self.eat_shot_frames > 400 {
                            self.eat_shot_path = None;
                            self.eat_shot_phase = EatShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            EatShotPhase::Idle => {}
        }
    }

    fn poll_jump_shot(&mut self) {
        let Some(path) = self.jump_shot_path.clone() else {
            return;
        };
        self.pin_jump_shot_camera();
        self.jump_shot_frames = self.jump_shot_frames.saturating_add(1);
        if self.jump_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.jump_shot_frames > 600 {
                eprintln!("jump shot timed out");
                self.jump_shot_path = None;
                self.jump_shot_phase = JumpShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.jump_shot_phase {
            JumpShotPhase::WaitingGround => match capture_window_rgba(&self.window) {
                Ok(ground) => {
                    self.jump_shot_ground = Some(ground);
                    self.jump_shot_frames = 0;
                    self.jump_shot_phase = JumpShotPhase::WaitingAir;
                }
                Err(err) => {
                    eprintln!("jump ground capture failed: {err}");
                    if self.jump_shot_frames > 400 {
                        self.jump_shot_path = None;
                        self.jump_shot_phase = JumpShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            JumpShotPhase::WaitingAir => {
                let Some(ground) = self.jump_shot_ground.take() else {
                    self.jump_shot_path = None;
                    self.jump_shot_phase = JumpShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(air) => match stitch_held_light_proof(&ground, &air, &path) {
                        Ok(()) => {
                            eprintln!("jump shot wrote {}", path.display());
                            self.jump_shot_path = None;
                            self.jump_shot_phase = JumpShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("jump stitch failed: {err}");
                            self.jump_shot_path = None;
                            self.jump_shot_phase = JumpShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("jump air capture failed: {err}");
                        self.jump_shot_ground = Some(ground);
                        if self.jump_shot_frames > 400 {
                            self.jump_shot_path = None;
                            self.jump_shot_phase = JumpShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            JumpShotPhase::Idle => {}
        }
    }

    fn poll_place_shot(&mut self) {
        let Some(path) = self.place_shot_path.clone() else {
            return;
        };
        self.pin_place_shot_camera();
        self.place_shot_frames = self.place_shot_frames.saturating_add(1);
        if self.place_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.place_shot_frames > 600 {
                eprintln!("place shot timed out");
                self.place_shot_path = None;
                self.place_shot_phase = PlaceShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.place_shot_phase {
            PlaceShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.place_shot_idle = Some(idle);
                    self.place_shot_frames = 0;
                    self.place_shot_phase = PlaceShotPhase::WaitingPlace;
                }
                Err(err) => {
                    eprintln!("place idle capture failed: {err}");
                    if self.place_shot_frames > 400 {
                        self.place_shot_path = None;
                        self.place_shot_phase = PlaceShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            PlaceShotPhase::WaitingPlace => {
                let Some(idle) = self.place_shot_idle.take() else {
                    self.place_shot_path = None;
                    self.place_shot_phase = PlaceShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(placed) => match stitch_held_light_proof(&idle, &placed, &path) {
                        Ok(()) => {
                            eprintln!("place shot wrote {}", path.display());
                            self.place_shot_path = None;
                            self.place_shot_phase = PlaceShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("place stitch failed: {err}");
                            self.place_shot_path = None;
                            self.place_shot_phase = PlaceShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("place capture failed: {err}");
                        self.place_shot_idle = Some(idle);
                        if self.place_shot_frames > 400 {
                            self.place_shot_path = None;
                            self.place_shot_phase = PlaceShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            PlaceShotPhase::Idle => {}
        }
    }

    fn poll_swap_shot(&mut self) {
        let Some(path) = self.swap_shot_path.clone() else {
            return;
        };
        self.pin_swap_shot_camera();
        self.swap_shot_frames = self.swap_shot_frames.saturating_add(1);
        if self.swap_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.swap_shot_frames > 600 {
                eprintln!("swap shot timed out");
                self.swap_shot_path = None;
                self.swap_shot_phase = SwapShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.swap_shot_phase {
            SwapShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.swap_shot_idle = Some(idle);
                    self.swap_shot_frames = 0;
                    self.swap_shot_phase = SwapShotPhase::WaitingSwap;
                }
                Err(err) => {
                    eprintln!("swap idle capture failed: {err}");
                    if self.swap_shot_frames > 400 {
                        self.swap_shot_path = None;
                        self.swap_shot_phase = SwapShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            SwapShotPhase::WaitingSwap => {
                let Some(idle) = self.swap_shot_idle.take() else {
                    self.swap_shot_path = None;
                    self.swap_shot_phase = SwapShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(swapped) => match stitch_held_light_proof(&idle, &swapped, &path) {
                        Ok(()) => {
                            eprintln!("swap shot wrote {}", path.display());
                            self.swap_shot_path = None;
                            self.swap_shot_phase = SwapShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("swap stitch failed: {err}");
                            self.swap_shot_path = None;
                            self.swap_shot_phase = SwapShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("swap capture failed: {err}");
                        self.swap_shot_idle = Some(idle);
                        if self.swap_shot_frames > 400 {
                            self.swap_shot_path = None;
                            self.swap_shot_phase = SwapShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            SwapShotPhase::Idle => {}
        }
    }

    fn poll_break_shot(&mut self) {
        let Some(path) = self.break_shot_path.clone() else {
            return;
        };
        self.pin_break_shot_camera();
        self.break_shot_frames = self.break_shot_frames.saturating_add(1);
        if self.break_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.break_shot_frames > 600 {
                eprintln!("break shot timed out");
                self.break_shot_path = None;
                self.break_shot_phase = BreakShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.break_shot_phase {
            BreakShotPhase::WaitingIdle => match capture_window_rgba(&self.window) {
                Ok(idle) => {
                    self.break_shot_idle = Some(idle);
                    self.break_shot_frames = 0;
                    self.break_shot_phase = BreakShotPhase::WaitingBreak;
                }
                Err(err) => {
                    eprintln!("break idle capture failed: {err}");
                    if self.break_shot_frames > 400 {
                        self.break_shot_path = None;
                        self.break_shot_phase = BreakShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            BreakShotPhase::WaitingBreak => {
                let Some(idle) = self.break_shot_idle.take() else {
                    self.break_shot_path = None;
                    self.break_shot_phase = BreakShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(broken) => match stitch_held_light_proof(&idle, &broken, &path) {
                        Ok(()) => {
                            eprintln!("break shot wrote {}", path.display());
                            self.break_shot_path = None;
                            self.break_shot_phase = BreakShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("break stitch failed: {err}");
                            self.break_shot_path = None;
                            self.break_shot_phase = BreakShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("break capture failed: {err}");
                        self.break_shot_idle = Some(idle);
                        if self.break_shot_frames > 400 {
                            self.break_shot_path = None;
                            self.break_shot_phase = BreakShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            BreakShotPhase::Idle => {}
        }
    }

    fn poll_crouch_shot(&mut self) {
        let Some(path) = self.crouch_shot_path.clone() else {
            return;
        };
        self.pin_crouch_shot_camera();
        self.crouch_shot_frames = self.crouch_shot_frames.saturating_add(1);
        if self.crouch_shot_frames < 120 || !self.chunk_bake.is_idle() {
            if self.crouch_shot_frames > 600 {
                eprintln!("crouch shot timed out");
                self.crouch_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("crouch shot wrote {}", path.display());
                self.crouch_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("crouch shot failed: {err}");
                if self.crouch_shot_frames > 250 {
                    self.crouch_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_bed_sleep_shot(&mut self) {
        let Some(path) = self.bed_sleep_shot_path.clone() else {
            return;
        };
        self.pin_bed_sleep_shot_camera();
        self.bed_sleep_shot_frames = self.bed_sleep_shot_frames.saturating_add(1);
        if self.bed_sleep_shot_frames < 120 || !self.chunk_bake.is_idle() {
            if self.bed_sleep_shot_frames > 600 {
                eprintln!("bed-sleep shot timed out");
                self.bed_sleep_shot_path = None;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match capture_window_png(&self.window, &path) {
            Ok(()) => {
                eprintln!("bed-sleep shot wrote {}", path.display());
                self.bed_sleep_shot_path = None;
                self.quit = true;
            }
            Err(err) => {
                eprintln!("bed-sleep shot failed: {err}");
                if self.bed_sleep_shot_frames > 250 {
                    self.bed_sleep_shot_path = None;
                    self.quit = true;
                }
            }
        }
    }

    fn poll_held_light_shot(&mut self) {
        let Some(path) = self.held_light_shot_path.clone() else {
            return;
        };
        self.pin_held_light_shot_camera();
        self.held_light_shot_frames = self.held_light_shot_frames.saturating_add(1);
        // Wait for corridor remesh (and later, torch light remesh) to settle.
        if self.held_light_shot_frames < 90 || !self.chunk_bake.is_idle() {
            if self.held_light_shot_frames > 600 {
                eprintln!("held-light shot timed out waiting for mesh");
                self.held_light_shot_path = None;
                self.held_light_shot_phase = HeldLightShotPhase::Idle;
                self.quit = true;
            }
            return;
        }
        let _ = self.window.focus_window();
        match self.held_light_shot_phase {
            HeldLightShotPhase::WaitingDark => match capture_window_rgba(&self.window) {
                Ok(dark) => {
                    eprintln!(
                        "held-light dark frame {}x{}",
                        dark.width, dark.height
                    );
                    self.held_light_shot_dark = Some(dark);
                    self.hotbar.select_digit(1);
                    let _ = self.hotbar.give_stack("torch");
                    self.hotbar_dirty = true;
                    self.sync_held_light();
                    self.held_light_shot_frames = 0;
                    self.held_light_shot_phase = HeldLightShotPhase::WaitingLit;
                }
                Err(err) => {
                    eprintln!("held-light dark capture failed: {err}");
                    if self.held_light_shot_frames > 400 {
                        self.held_light_shot_path = None;
                        self.held_light_shot_phase = HeldLightShotPhase::Idle;
                        self.quit = true;
                    }
                }
            },
            HeldLightShotPhase::WaitingLit => {
                let Some(dark) = self.held_light_shot_dark.take() else {
                    self.held_light_shot_path = None;
                    self.held_light_shot_phase = HeldLightShotPhase::Idle;
                    self.quit = true;
                    return;
                };
                match capture_window_rgba(&self.window) {
                    Ok(lit) => match stitch_held_light_proof(&dark, &lit, &path) {
                        Ok(()) => {
                            eprintln!("held-light shot wrote {}", path.display());
                            self.held_light_shot_path = None;
                            self.held_light_shot_phase = HeldLightShotPhase::Idle;
                            self.quit = true;
                        }
                        Err(err) => {
                            eprintln!("held-light stitch failed: {err}");
                            self.held_light_shot_path = None;
                            self.held_light_shot_phase = HeldLightShotPhase::Idle;
                            self.quit = true;
                        }
                    },
                    Err(err) => {
                        eprintln!("held-light lit capture failed: {err}");
                        self.held_light_shot_dark = Some(dark);
                        if self.held_light_shot_frames > 400 {
                            self.held_light_shot_path = None;
                            self.held_light_shot_phase = HeldLightShotPhase::Idle;
                            self.quit = true;
                        }
                    }
                }
            }
            HeldLightShotPhase::Idle => {}
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.size = size;
        if size.width == 0 || size.height == 0 {
            self.configured = false;
            return;
        }
        self.configure_surface();
        self.depth = Some(DepthBuffer::new(&self.device, size.width, size.height));
        self.prepare_frame_target();
        self.configured = true;
    }

    fn prepare_frame_target(&mut self) {
        let Some(frame) = &self.frame else {
            return;
        };
        let width = self.size.width;
        let height = self.size.height;
        if width == 0 || height == 0 {
            return;
        }
        if frame.width == width && frame.height == height && frame.target.is_some() {
            return;
        }
        let format = self.view_format;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let layout = self.frame.as_ref().unwrap().layout.clone();
        let sampler = self.frame.as_ref().unwrap().sampler.clone();
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let frame = self.frame.as_mut().unwrap();
        frame.target = Some(FrameTarget {
            texture,
            view,
            bind_group,
        });
        frame.width = width;
        frame.height = height;
    }

    fn configure_surface(&self) {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![self.view_format],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.size.width,
            height: self.size.height,
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        self.surface.configure(&self.device, &config);
    }

    pub fn set_capture(&mut self, capture: bool) {
        self.captured = capture;
        if !capture {
            self.dig.set_held(false);
        }
        if capture {
            let _ = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            self.window.set_cursor_visible(false);
        } else {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
    }

    pub fn screen_open(&self) -> bool {
        self.gui.is_open() || self.map_open
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Forward a winit event to egui. True when the game should ignore it.
    /// Escape is never blocked so pause / creative / items can still close.
    /// While a keybind is listening, keyboard and mouse reach `set_key` / `set_mouse`.
    pub fn egui_on_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        if self.menu.is_keybinds() && self.binds.listening.is_some() {
            let _ = self.egui.on_window_event(&self.window, event);
            return !matches!(
                event,
                winit::event::WindowEvent::KeyboardInput { .. }
                    | winit::event::WindowEvent::MouseInput { .. }
            );
        }
        let consumed = self.egui.on_window_event(&self.window, event);
        if !consumed || !crate::egui_ui::EguiHost::wants_input(&self.menu, &self.gui) {
            return false;
        }
        !matches!(
            event,
            winit::event::WindowEvent::KeyboardInput {
                event: key,
                ..
            } if key.state == winit::event::ElementState::Pressed
                && matches!(key.physical_key, winit::keyboard::PhysicalKey::Code(KeyCode::Escape))
        )
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    pub fn wants_quit(&self) -> bool {
        self.quit
    }

    pub fn open_menu(&mut self) {
        self.refund_station();
        self.gui.close();
        if self.playing {
            self.menu.open_pause();
        } else {
            self.menu.open();
        }
        self.clear_motion();
        self.set_capture(false);
    }

    /// Escape leaves settings or worlds, and resumes from pause.
    pub fn menu_escape(&mut self) {
        if self.menu.is_keybinds() && self.binds.listening.is_some() {
            self.binds.cancel_listen();
            return;
        }
        let _ = self.menu.escape();
        self.sync_pointer();
    }

    pub fn close_screen(&mut self) {
        if self.map_open {
            self.map_open = false;
            self.clear_motion();
            self.sync_pointer();
            return;
        }
        if self.gui.is_open() {
            self.refund_station();
            self.gui.close();
            self.clear_motion();
            self.sync_pointer();
        }
    }

    fn clear_motion(&mut self) {
        self.input.forward = false;
        self.input.back = false;
        self.input.left = false;
        self.input.right = false;
        self.input.jump = false;
        self.input.fast = false;
        self.input.crouch = false;
        self.shape_pick.open = false;
        self.dig.set_held(false);
    }

    pub fn set_key(&mut self, code: KeyCode, pressed: bool) {
        if pressed && self.binds.listening.is_some() && self.menu.is_keybinds() {
            let _ = self.binds.hear_key(code);
            self.clear_motion();
            return;
        }
        let code = self.binds.canonicalize_key(code);
        if code == KeyCode::KeyF && !pressed {
            self.zoom_key = false;
        }
        if self.dead {
            if pressed && matches!(code, KeyCode::Enter | KeyCode::Space | KeyCode::Escape) {
                self.respawn();
            }
            self.clear_motion();
            return;
        }
        if self.menu.is_open() {
            if pressed {
                let action = self.menu.on_press(code);
                self.apply_menu(action);
                self.sync_pointer();
            }
            self.clear_motion();
            return;
        }
        if self.map_open {
            self.clear_motion();
            return;
        }
        if pressed {
            let was_open = self.gui.is_open();
            let closing_built = self
                .gui
                .built_open()
                .then(|| self.gui.screen_id().to_string());
            let chest_was = self.gui.chest_open();
            let station_was = self.gui.craft_open() || self.gui.furnace_open();
            let inv_was = self.gui.player_inv_open();
            if self.gui.on_press(code) {
                if chest_was && !self.gui.chest_open() {
                    self.flush_chest();
                    self.chest_at = None;
                }
                if station_was && !self.gui.craft_open() && !self.gui.furnace_open() {
                    self.refund_station();
                }
                if inv_was && !self.gui.player_inv_open() {
                    self.refund_station();
                }
                self.clear_motion();
                self.sync_pointer();
                if self.gui.bag_open() && code == KeyCode::Enter {
                    if let Some(id) = self.gui.creative_pick() {
                        if self.hotbar.give_stack(&id) {
                            self.hotbar_dirty = true;
                        }
                    } else if !self.gui.creative_focused() {
                        self.take_bag_slot();
                    }
                }
                if self.gui.command_open() && code == KeyCode::Enter {
                    if let Some(id) = self.gui.selected_command() {
                        self.run_command(&id);
                    }
                }
                if self.gui.chest_open() && code == KeyCode::Enter {
                    self.take_chest_slot();
                }
                if self.gui.craft_open() && code == KeyCode::Enter {
                    self.take_craft_result();
                }
                if self.gui.furnace_open() && code == KeyCode::Enter {
                    self.take_furnace_output();
                }
                if self.gui.anvil_open() && code == KeyCode::Enter {
                    self.try_repair();
                }
                if self.gui.enchant_open() && code == KeyCode::Enter {
                    self.try_enchant();
                }
                if self.gui.brew_open() && code == KeyCode::Enter {
                    self.try_brew();
                }
                if self.gui.grind_open() && code == KeyCode::Enter {
                    let word = self.hotbar.grind_pick();
                    self.gui.note_grind(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                    if matches!(word, "ground" | "sharpened") {
                        if let Some(at) = self.gui.grind_at() {
                            let source =
                                [at[0] as f32 + 0.5, at[1] as f32 + 0.85, at[2] as f32 + 0.5];
                            let pitch = if word == "ground" { 3 } else { 6 };
                            self.play_world(crate::audio::Sound::Note(pitch), source);
                            self.drops.puff(source, [0.78, 0.76, 0.70]);
                            self.rebuild_drops();
                        }
                    }
                }
                if self.gui.smith_open() && code == KeyCode::Enter {
                    self.try_smith();
                }
                if self.gui.loom_open() && code == KeyCode::Enter {
                    self.try_weave();
                }
                if self.gui.chart_open() && code == KeyCode::Enter {
                    let word = self.hotbar.chart_map();
                    self.gui.note_chart(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.lectern_open() && code == KeyCode::Enter {
                    let word = self.read_lectern();
                    self.gui.note_lectern(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.lectern_open()
                    && matches!(
                        code,
                        KeyCode::ArrowLeft
                            | KeyCode::ArrowRight
                            | KeyCode::KeyA
                            | KeyCode::KeyD
                    )
                {
                    let delta = match code {
                        KeyCode::ArrowLeft | KeyCode::KeyA => -1,
                        _ => 1,
                    };
                    if let Some(line) = self.gui.turn_lectern(delta) {
                        if let Some(at) = self.gui.lectern_at() {
                            let source =
                                [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
                            self.play_world(crate::audio::Sound::Note(5), source);
                        }
                        self.gui.push_chat(line);
                    }
                }
                if self.gui.shelf_open() && code == KeyCode::Enter {
                    let word = self.store_shelf();
                    self.gui.note_shelf(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.hang_open() && code == KeyCode::Enter {
                    self.gui.note_hang(gui::HANGING_LINE);
                    self.gui.push_chat(gui::HANGING_LINE);
                }
                if self.gui.compost_open() && code == KeyCode::Enter {
                    let word = self.fill_compost();
                    self.gui.note_compost(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                    if matches!(word, "level 1" | "level 2" | "level 3" | "bone meal") {
                        if let Some(at) = self.gui.compost_at() {
                            let source =
                                [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
                            let pitch = if word == "bone meal" { 4 } else { 2 };
                            self.play_world(crate::audio::Sound::Note(pitch), source);
                            self.drops.note(
                                [source[0], at[1] as f32 + 1.05, source[2]],
                                pitch,
                            );
                            self.rebuild_drops();
                        }
                    }
                }
                if self.gui.hive_open() && code == KeyCode::Enter {
                    let word = self.fill_hive();
                    self.gui.note_hive(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                    if matches!(word, "level 1" | "level 2" | "level 3" | "honey") {
                        if let Some(at) = self.gui.hive_at() {
                            let source =
                                [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
                            let pitch = if word == "honey" { 9 } else { 7 };
                            self.play_world(crate::audio::Sound::Note(pitch), source);
                            self.drops.note(
                                [source[0], at[1] as f32 + 1.05, source[2]],
                                pitch,
                            );
                            self.rebuild_drops();
                        }
                    }
                }
                if self.gui.cauldron_open() && code == KeyCode::Enter {
                    let word = self.fill_cauldron();
                    self.gui.note_cauldron(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                    if matches!(word, "level 1" | "level 2" | "level 3" | "scooped") {
                        if let Some(at) = self.gui.cauldron_at() {
                            let source =
                                [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
                            let pitch = if word == "scooped" { 1 } else { 0 };
                            self.play_world(crate::audio::Sound::Note(pitch), source);
                            self.drops.note(
                                [source[0], at[1] as f32 + 1.05, source[2]],
                                pitch,
                            );
                            self.rebuild_drops();
                        }
                    }
                }
                if self.gui.bell_open() && code == KeyCode::Enter {
                    let word = self.ring_bell();
                    self.gui.note_bell(word);
                    self.gui.push_chat(word);
                }
                if self.gui.candle_open() && code == KeyCode::Enter {
                    let word = self.toggle_candle();
                    self.gui.note_candle(word);
                    self.gui.push_chat(word);
                }
                if self.gui.lodestone_open() && code == KeyCode::Enter {
                    let word = self.link_lodestone();
                    self.gui.note_lodestone(word);
                    self.gui.push_chat(word);
                }
                if self.gui.anchor_open() && code == KeyCode::Enter {
                    let word = self.charge_anchor();
                    self.gui.note_anchor(word);
                    self.gui.push_chat(word);
                }
                if self.gui.rod_open() && code == KeyCode::Enter {
                    let word = self.strike_rod();
                    self.gui.note_rod(word);
                    self.gui.push_chat(word);
                }
                if self.gui.target_open() && code == KeyCode::Enter {
                    if let Some(at) = self.gui.target_at() {
                        let word = self.hit_target(at);
                        self.gui.note_target(word);
                        self.gui.push_chat(word);
                    }
                }
                if self.gui.bulb_open() && code == KeyCode::Enter {
                    let word = self.toggle_bulb();
                    self.gui.note_bulb(word);
                    self.gui.push_chat(word);
                }
                if self.gui.pot_open() && code == KeyCode::Enter {
                    let word = self.store_pot();
                    self.gui.note_pot(word);
                    self.gui.push_chat(word);
                }
                if self.gui.board_open() && code == KeyCode::Enter {
                    let word = self.store_board();
                    self.gui.note_board(word);
                    self.gui.push_chat(word);
                }
                if self.gui.cut_open() && code == KeyCode::Enter {
                    let word = self.hotbar.cut_stone();
                    self.gui.note_cut(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.fire_open() && code == KeyCode::Enter {
                    let word = self.hotbar.cook_raw();
                    self.gui.note_fire(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.fletch_open() && code == KeyCode::Enter {
                    let word = self.hotbar.fletch_arrow();
                    self.gui.note_fletch(word);
                    self.gui.push_chat(word);
                    self.hotbar_dirty = true;
                }
                if self.gui.chat_open() && code == KeyCode::Enter {
                    if let Some(line) = self.gui.take_chat() {
                        if !self.try_slash(&line) && !self.try_script_chat(&line) {
                            self.say(&line);
                        }
                    }
                }
                if self.gui.is_open() != was_open {
                    let open = self.gui.is_open();
                    let id = if open {
                        self.gui.screen_id().to_string()
                    } else {
                        closing_built.unwrap_or_else(|| self.gui.screen_id().to_string())
                    };
                    let slots = hotbar_slots(&self.hotbar);
                    let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                    self.hooks.screen(&mut self.world, &id, open, &view);
                }
                self.fire_script_row();
                return;
            }
        }
        if self.gui.craft_open() && pressed {
            if let Some(digit) = Self::hotbar_digit(code) {
                self.stash_into_craft(digit);
                return;
            }
        }
        if self.gui.furnace_open() && pressed {
            if let Some(digit) = Self::hotbar_digit(code) {
                self.stash_into_furnace(digit);
                return;
            }
        }
        if self.gui.chest_open() && pressed {
            if let Some(digit) = Self::hotbar_digit(code) {
                self.stash_into_chest(digit);
                return;
            }
        }
        if self.gui.bag_open() && pressed {
            if let Some(digit) = Self::hotbar_digit(code) {
                self.stash_from_hotbar(digit);
                return;
            }
        }
        if self.gui.is_open() {
            if pressed && code == KeyCode::Enter {
                self.try_craft();
            }
            return;
        }
        if self.playing
            && !self.dead
            && matches!(code, KeyCode::AltLeft | KeyCode::AltRight)
        {
            let held = self.hotbar.slot(self.hotbar.selected());
            self.shape_pick.clamp_to(held);
            let can_pick = held.is_some_and(|id| crate::shape_pick::ShapePick::options(id).len() > 1);
            if pressed && can_pick {
                self.shape_pick.open = true;
            } else if !pressed {
                self.shape_pick.open = false;
            }
            return;
        }
        if self.shape_pick.open && pressed {
            let held = self.hotbar.slot(self.hotbar.selected()).unwrap_or("");
            if let Some(digit) = Self::hotbar_digit(code) {
                let _ = self.shape_pick.pick_digit(held, digit);
                return;
            }
            match code {
                KeyCode::ArrowLeft | KeyCode::ArrowUp => {
                    self.shape_pick.cycle(held, false);
                    return;
                }
                KeyCode::ArrowRight | KeyCode::ArrowDown => {
                    self.shape_pick.cycle(held, true);
                    return;
                }
                KeyCode::Escape => {
                    self.shape_pick.open = false;
                    return;
                }
                _ => {}
            }
        }
        if pressed {
            if let Some(digit) = Self::hotbar_digit(code) {
                let prev = self.hotbar.selected();
                if self.hotbar.select_digit(digit) {
                    self.note_hotbar_swap(prev);
                    self.hotbar_dirty = true;
                    let index = (digit - 1) as usize;
                    let block = self.hotbar.slot(index).unwrap_or("").to_string();
                    self.shape_pick.clamp_to(Some(block.as_str()));
                    let slots = hotbar_slots(&self.hotbar);
                    let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                    self.hooks
                        .select(&mut self.world, i32::from(digit), &block, &view);
                }
                return;
            }
        }
        match code {
            KeyCode::KeyW => self.input.forward = pressed,
            KeyCode::KeyS => self.input.back = pressed,
            KeyCode::KeyA => self.input.left = pressed,
            KeyCode::KeyD => self.input.right = pressed,
            KeyCode::Space => self.input.jump = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => {
                self.ctrl_mod = pressed;
                self.input.fast = pressed;
            }
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.input.crouch = pressed,
            KeyCode::KeyF if pressed && self.playing && self.holding_spyglass() => {
                self.zoom_key = true;
            }
            KeyCode::KeyF if pressed && self.playing => {
                if self.boat.aboard() {
                    let seat = self.boat.seat();
                    self.boat.dismount();
                    self.clear_boat_feel();
                    if let Some(pos) = seat.or_else(|| self.boat.origin()) {
                        let wet = body::in_water(self.camera.eye, &self.world);
                        let sound = if wet {
                            crate::audio::Sound::Splash
                        } else {
                            crate::audio::Sound::SplashOut
                        };
                        self.play_world(sound, pos);
                    }
                    self.gui.push_chat("ashore");
                } else if self.cart.aboard() {
                    let seat = self.cart.seat();
                    self.cart.dismount();
                    self.clear_cart_feel();
                    if let Some(pos) = seat.or_else(|| self.cart.origin()) {
                        self.play_world(crate::audio::Sound::Step, pos);
                    }
                    self.gui.push_chat("off cart");
                } else if self.horse.ridden() {
                    self.horse.dismount();
                    self.clear_horse_feel();
                    let right = self.camera.walk_axes().1;
                    self.camera.eye += right * 1.0;
                    self.gui.push_chat("dismounted");
                } else if self.pig.ridden() {
                    self.pig.dismount();
                    let right = self.camera.walk_axes().1;
                    self.camera.eye += right * 1.0;
                    self.gui.push_chat("dismounted");
                } else if self.gui.chest_open() {
                    self.flush_chest();
                    self.gui.close_chest();
                    self.chest_at = None;
                } else if self.gui.craft_open() || self.gui.furnace_open() {
                    self.refund_station();
                    self.gui.close();
                } else if self
                    .boat
                    .near([self.camera.eye.x, self.camera.eye.y, self.camera.eye.z])
                    && !self.looking_at_use()
                {
                    self.boat.mount();
                    if let Some(seat) = self.boat.seat() {
                        self.camera.eye = glam::Vec3::from(seat);
                        self.camera.velocity_y = 0.0;
                        self.camera.on_ground = true;
                        self.play_world(crate::audio::Sound::SplashOut, seat);
                    }
                    self.boat_kick = 0.0;
                    self.boat_show = 0.0;
                    self.boat_stroke = 0.0;
                    self.gui.push_chat("aboard");
                } else if self
                    .cart
                    .near([self.camera.eye.x, self.camera.eye.y, self.camera.eye.z])
                    && !self.looking_at_use()
                {
                    self.cart.mount();
                    if let Some(seat) = self.cart.seat() {
                        self.camera.eye = glam::Vec3::from(seat);
                        self.camera.velocity_y = 0.0;
                        self.camera.on_ground = true;
                        self.play_world(crate::audio::Sound::Step, seat);
                    }
                    self.clear_cart_feel();
                    self.gui.push_chat("aboard cart");
                } else if self.horse.saddled()
                    && self
                        .horse
                        .near([self.camera.eye.x, self.camera.eye.y, self.camera.eye.z])
                    && !self.looking_at_use()
                {
                    self.horse.mount();
                    if let Some(seat) = self.horse.seat() {
                        self.camera.eye = glam::Vec3::from(seat);
                        self.camera.velocity_y = 0.0;
                        self.camera.on_ground = true;
                        self.play_world(crate::audio::Sound::Step, seat);
                    }
                    self.clear_horse_feel();
                    self.gui.push_chat("mounted");
                } else if self.pig.saddled()
                    && self
                        .pig
                        .near([self.camera.eye.x, self.camera.eye.y, self.camera.eye.z])
                    && !self.looking_at_use()
                {
                    self.pig.mount();
                    if let Some(seat) = self.pig.seat() {
                        self.camera.eye = glam::Vec3::from(seat);
                        self.camera.velocity_y = 0.0;
                        self.camera.on_ground = true;
                    }
                    self.gui.push_chat("mounted");
                } else if !self.try_mace()
                    && !self.try_horn()
                    && !self.try_brush()
                    && !self.try_omen()
                    && !self.try_key()
                    && !self.try_wind()
                {
                    self.open_looked_block();
                }
            }
            KeyCode::F5 if pressed && self.playing => {
                self.third_person = !self.third_person;
                self.rebuild_player();
                self.gui.push_chat(if self.third_person {
                    "third"
                } else {
                    "first"
                });
            }
            KeyCode::Enter if pressed && self.playing => {
                if let Some(hit) =
                    look::raycast(self.camera.eye, self.camera.forward(), &self.world)
                {
                    let block = self
                        .world
                        .block_at(hit.block[0], hit.block[1], hit.block[2]);
                    if block.id == "item_frame" {
                        let word = self.display_frame(hit.block);
                        self.gui.push_chat(&word);
                    }
                }
            }
            KeyCode::KeyT if pressed && self.playing => {
                if self.gui.open_chat() {
                    self.clear_motion();
                    self.set_capture(false);
                    let slots = hotbar_slots(&self.hotbar);
                    let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                    self.hooks.screen(&mut self.world, "chat", true, &view);
                }
            }
            KeyCode::KeyC if pressed && self.playing => {
                let rows = command_rows(&self.hooks);
                if self.gui.show_commands(rows) {
                    self.clear_motion();
                    self.set_capture(false);
                    let id = self.gui.screen_id().to_string();
                    let slots = hotbar_slots(&self.hotbar);
                    let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                    self.hooks.screen(&mut self.world, &id, true, &view);
                }
            }
            _ => {}
        }
    }

    fn run_command(&mut self, id: &str) {
        let _ = self.invoke_command(id, "");
        self.spark_command();
    }

    /// `/spark hi` in chat. True when a script or a built-in command ran.
    fn try_slash(&mut self, line: &str) -> bool {
        let Some(rest) = line.strip_prefix('/') else {
            return false;
        };
        let rest = rest.trim();
        if rest.is_empty() {
            return false;
        }
        let (name, args) = rest.split_once(' ').unwrap_or((rest, ""));
        if !self.invoke_command(name, args.trim()) {
            return false;
        }
        self.spark_command();
        true
    }

    /// Run a script callback, then day, night, or home when the name is one of those.
    fn invoke_command(&mut self, id: &str, args: &str) -> bool {
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let _ = self.hooks.drain_writes();
        let reply = self.hooks.command(&mut self.world, id, args, &view);
        if reply.is_some() {
            if let Some(text) = &reply {
                for line in text.lines() {
                    if !line.is_empty() {
                        self.gui.push_chat(line);
                    }
                }
            }
            self.flush_script_writes();
        } else {
            let _ = self.hooks.drain_writes();
        }
        let builtin = crate::commands::apply(id, &mut self.clock, &mut self.camera);
        reply.is_some() || builtin
    }

    fn spark_command(&mut self) {
        let color = particle_color(&self.hooks).unwrap_or([1.0, 0.82, 0.35]);
        let eye = self.camera.eye.to_array();
        self.drops.burst([eye[0], eye[1] - 0.4, eye[2]], color);
        self.rebuild_drops();
        self.note_sky();
    }

    /// An ordinary chat line asks `events.chat`. A slash line is a command.
    ///
    /// False swallows the line, so it is not drawn and not sent. A swallowed
    /// line sends the first `world.set_block` from the hook.
    fn try_script_chat(&mut self, line: &str) -> bool {
        if line.starts_with('/') {
            return false;
        }
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.chat(&mut self.world, line, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// A crafting-grid take asks `events.craft` before the ingredients are spent.
    ///
    /// False leaves the grid alone. A refused take sends the first
    /// `world.set_block` from the hook.
    fn try_script_craft(&mut self, result: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.craft(&mut self.world, result, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// A furnace, smoker, or blast-furnace take asks `events.smelt` first.
    ///
    /// False leaves the output in the slot. A refused take sends the first
    /// `world.set_block` from the hook.
    fn try_script_smelt(&mut self, result: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.smelt(&mut self.world, result, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// Enter on the anvil refills a worn pick. `events.repair` can refuse it.
    ///
    /// The hook sees the tool id only when iron ore would be spent. False
    /// leaves the ore and the worn pick alone.
    fn try_repair(&mut self) {
        let target = self.hotbar.repair_target().map(str::to_string);
        if let Some(id) = target {
            if self.try_script_repair(&id) {
                self.gui.note_anvil("refused");
                self.gui.push_chat("refused");
                return;
            }
        }
        let word = self.hotbar.repair_pick();
        self.gui.note_anvil(word);
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        if word == "repaired" {
            if let Some(at) = self.gui.anvil_at() {
                let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.85, at[2] as f32 + 0.5];
                self.play_world(crate::audio::Sound::Note(1), source);
                self.drops.puff(source, [0.72, 0.72, 0.78]);
                self.rebuild_drops();
            }
        }
    }

    /// False from `events.repair` keeps the iron ore. A refused repair sends
    /// the first `world.set_block` from the hook.
    fn try_script_repair(&mut self, item: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.repair(&mut self.world, item, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// Enter on the enchanting table puts Efficiency on a plain pick.
    /// `events.enchant` can refuse it.
    ///
    /// The hook sees the tool id only when a bookshelf would be spent. False
    /// leaves the book and the plain pick alone.
    fn try_enchant(&mut self) {
        let target = self.hotbar.enchant_target().map(str::to_string);
        if let Some(id) = target {
            if self.try_script_enchant(&id) {
                self.gui.note_enchant("refused");
                self.gui.push_chat("refused");
                return;
            }
        }
        let word = self.hotbar.enchant_pick();
        self.gui.note_enchant(word);
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        if word == "efficiency" {
            if let Some(at) = self.gui.enchant_at() {
                let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.85, at[2] as f32 + 0.5];
                self.play_world(crate::audio::Sound::Note(10), source);
                self.drops.book_swirl(source);
                self.rebuild_drops();
            }
        }
    }

    /// False from `events.enchant` keeps the bookshelf. A refused enchant
    /// sends the first `world.set_block` from the hook.
    fn try_script_enchant(&mut self, item: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.enchant(&mut self.world, item, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// Enter on the brewing stand mixes water with nether wart or a flower.
    /// `events.brew` can refuse it.
    ///
    /// The hook sees the ingredient only when both pieces are present and the
    /// potion fits. False leaves the water bucket and the ingredient alone.
    fn try_brew(&mut self) {
        let target = self.hotbar.brew_target().map(str::to_string);
        if let Some(id) = target {
            if self.try_script_brew(&id) {
                self.gui.note_brew("refused");
                self.gui.push_chat("refused");
                return;
            }
        }
        let word = self.hotbar.brew_potion();
        self.gui.note_brew(word);
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        if word == "brewed" {
            if let Some(at) = self.gui.brew_at() {
                let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.85, at[2] as f32 + 0.5];
                self.play_world(crate::audio::Sound::Note(8), source);
                self.drops.brew_bubbles(source);
                self.rebuild_drops();
            }
        }
    }

    /// False from `events.brew` keeps the water and the ingredient. A refused
    /// brew sends the first `world.set_block` from the hook.
    fn try_script_brew(&mut self, ingredient: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.brew(&mut self.world, ingredient, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// Enter on the smithing table upgrades a pick or a mace.
    /// `events.smith` can refuse it.
    ///
    /// The hook sees the held id only when iron ore or a heavy core would be
    /// spent. False leaves that payment and the held item alone.
    fn try_smith(&mut self) {
        let target = self.hotbar.smith_target().map(str::to_string);
        if let Some(id) = target {
            if self.try_script_smith(&id) {
                self.gui.note_smith("refused");
                self.gui.push_chat("refused");
                return;
            }
        }
        let word = self.hotbar.smith_pick();
        self.gui.note_smith(word);
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
        if matches!(word, "iron" | "diamond" | "heavy") {
            if let Some(at) = self.gui.smith_at() {
                let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.85, at[2] as f32 + 0.5];
                let pitch = if word == "diamond" { 11 } else { 4 };
                self.play_world(crate::audio::Sound::Note(pitch), source);
                self.drops.puff(source, [0.86, 0.58, 0.36]);
                self.rebuild_drops();
            }
        }
    }

    /// False from `events.smith` keeps the iron ore or the heavy core. A
    /// refused upgrade sends the first `world.set_block` from the hook.
    fn try_script_smith(&mut self, item: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.smith(&mut self.world, item, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    /// Enter on the loom spends one wool for a banner. `events.weave` can
    /// refuse it.
    ///
    /// The hook sees the wool id only when the banner fits. False leaves the
    /// wool in the bar.
    fn try_weave(&mut self) {
        let target = self.hotbar.weave_target().map(str::to_string);
        if let Some(id) = target {
            if self.try_script_weave(&id) {
                self.gui.note_loom("refused");
                self.gui.push_chat("refused");
                return;
            }
        }
        let word = self.hotbar.weave_banner();
        self.gui.note_loom(word);
        self.gui.push_chat(word);
        self.hotbar_dirty = true;
    }

    /// False from `events.weave` keeps the wool. A refused weave sends the
    /// first `world.set_block` from the hook.
    fn try_script_weave(&mut self, wool: &str) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        if self.hooks.weave(&mut self.world, wool, &view) {
            let _ = self.hooks.drain_writes();
            return false;
        }
        self.flush_script_writes();
        true
    }

    fn say(&mut self, text: &str) {
        let line = rebuild_world::chat_line(text);
        if line.is_empty() {
            return;
        }
        if self.local {
            self.gui.push_chat(&line);
            return;
        }
        if self.link.send(encode_chat(&line)).is_err() {
            self.note_disconnect();
        }
    }

    fn note_pose(&mut self) {
        let (yaw, pitch) = self.camera.yaw_pitch();
        let moved = (self.camera.eye - self.posed_eye).length() > 0.08
            || (yaw - self.posed_yaw).abs() > 0.03
            || (pitch - self.posed_pitch).abs() > 0.03;
        if moved {
            self.pose_dirty = true;
        }
    }

    fn flush_pose(&mut self, dt: f32) {
        self.pose_left = (self.pose_left - dt).max(0.0);
        let due = self.pose_dirty || !self.local;
        if !due || self.pose_left > 0.0 || self.told_disconnect {
            return;
        }
        let (yaw, pitch) = self.camera.yaw_pitch();
        let eye = self.camera.eye;
        if self
            .link
            .send(encode_pose(eye.x, eye.y, eye.z, yaw, pitch))
            .is_err()
        {
            self.note_disconnect();
            return;
        }
        self.posed_eye = eye;
        self.posed_yaw = yaw;
        self.posed_pitch = pitch;
        self.pose_dirty = false;
        self.pose_left = 0.4;
    }

    pub fn mouse_look(&mut self, dx: f64, dy: f64) {
        if self.captured {
            self.camera.apply_mouse(dx, dy);
        }
    }

    /// Remember the pointer and move the highlight while a screen is open.
    pub fn pointer_at(&mut self, x: f32, y: f32) {
        self.cursor_x = x;
        self.cursor_y = y;
        if !self.menu.is_open() && !self.gui.is_open() {
            return;
        }
        let hit = self.pointer_hit();
        if self.menu.is_open() {
            self.menu.pointer_move(hit);
        } else {
            self.gui.pointer_move(hit);
        }
    }

    fn fire_script_row(&mut self) {
        let Some(row) = self.gui.take_press() else {
            return;
        };
        let id = self.gui.screen_id().to_string();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let typed = if self.gui.is_field(&row) {
            self.gui.row_text(&row)
        } else {
            None
        };
        for line in self
            .hooks
            .screen_row_text(&mut self.world, &id, &row, typed.as_deref(), &view)
        {
            self.gui.push_chat(&line);
        }
        self.gui
            .sync_built(&rebuild_world::Pack::builtin().gui_screens());
    }

    /// Click the slot or button under the pointer.
    pub fn pointer_click(&mut self) {
        if self.dead {
            self.respawn();
            return;
        }
        if !self.menu.is_open() && !self.gui.is_open() {
            return;
        }
        let hit = self.pointer_hit();
        if self.menu.is_open() {
            let action = self.menu.pointer_click(hit);
            self.apply_menu(action);
            self.sync_pointer();
            return;
        }
        match self.gui.pointer_click(hit) {
            gui::Click::Give => {
                if let Some(id) = self.gui.creative_pick() {
                    if self.hotbar.give_stack(&id) {
                        self.hotbar_dirty = true;
                    }
                }
            }
            gui::Click::TakeBag => self.take_bag_slot(),
            gui::Click::TakeChest => self.take_chest_slot(),
            gui::Click::TakeCraft => self.take_craft_result(),
            gui::Click::TakeFurnace => self.take_furnace_output(),
            gui::Click::RunCommand => {
                if let Some(id) = self.gui.selected_command() {
                    self.run_command(&id);
                }
            }
            gui::Click::Hotbar(index) => {
                let prev = self.hotbar.selected();
                if self.hotbar.select_digit(index as u8 + 1) {
                    self.note_hotbar_swap(prev);
                    self.hotbar_dirty = true;
                }
            }
            gui::Click::Script => self.fire_script_row(),
            gui::Click::None => {}
        }
    }

    /// Scroll the open list. Down moves toward later rows.
    ///
    /// In play with the mouse captured, the wheel cycles the hotbar.
    pub fn pointer_scroll(&mut self, down: bool) {
        if self.map_open && self.holding_map() {
            self.hotbar.zoom_map(down);
            return;
        }
        if self.shape_pick.open {
            if let Some(held) = self.hotbar.slot(self.hotbar.selected()) {
                self.shape_pick.cycle(held, down);
            }
            return;
        }
        if self.menu.is_open() {
            self.menu.pointer_scroll(down);
        } else if self.gui.is_open() {
            self.gui.pointer_scroll(down);
        } else if self.playing && !self.dead {
            let prev = self.hotbar.selected();
            let next = if down {
                (prev + 1) % 10
            } else {
                (prev + 9) % 10
            };
            if self.hotbar.select_digit(next as u8 + 1) {
                self.note_hotbar_swap(prev);
                self.hotbar_dirty = true;
            }
        }
    }

    fn note_hotbar_swap(&mut self, prev: usize) {
        if self.hotbar.selected() != prev {
            self.swap_kick = 1.0;
        }
    }

    fn pointer_hit(&self) -> u16 {
        let quads = gui::feel_quads(
            self.size.width as f32,
            self.size.height as f32,
            &self.hotbar,
            &self.bag,
            &self.gui,
            &self.menu,
            self.hearts,
            self.hunger.food(),
            self.armor.worn(),
            self.slot_punch,
            self.hurt_flash,
            self.sip,
            self.sipping,
            self.dead,
        );
        gui::quad_hit(
            &quads,
            self.cursor_x,
            self.cursor_y,
            self.size.width as f32,
            self.size.height as f32,
        )
    }

    fn apply_menu(&mut self, action: gui::MenuAction) {
        match action {
            gui::MenuAction::Quit => self.quit = true,
            gui::MenuAction::Play => {
                self.playing = true;
                self.apply_chosen_size();
            }
            gui::MenuAction::Join => self.join_localhost(),
            gui::MenuAction::Title => self.playing = false,
            gui::MenuAction::Volume => {
                self.audio.set_muted(self.menu.muted());
                if !self.menu.muted() {
                    self.audio.play(crate::audio::Sound::Place);
                }
            }
            gui::MenuAction::Resize => {
                if self.playing {
                    self.apply_chosen_size();
                }
            }
            gui::MenuAction::Setting => {
                if let Some(name) = self.menu.take_changed() {
                    let slots = hotbar_slots(&self.hotbar);
                    let view = if self.playing {
                        client_view(&slots, &self.hotbar, self.camera.eye, self.clock)
                    } else {
                        ScriptView {
                            slots: &slots,
                            selected: 0,
                            eye: None,
                            time: None,
                            season: "",
                        }
                    };
                    for line in self.hooks.setting_changed(&mut self.world, &name, &view) {
                        self.gui.push_chat(&line);
                    }
                    if self.playing {
                        self.flush_script_writes();
                    } else {
                        let _ = self.hooks.drain_writes();
                    }
                }
            }
            gui::MenuAction::Resume | gui::MenuAction::None => {}
        }
    }

    /// A screen keeps the cursor free. Closing it while playing grabs the look again.
    fn sync_pointer(&mut self) {
        if self.menu.is_open() || self.gui.is_open() || self.map_open || self.dead {
            self.set_capture(false);
        } else if self.playing {
            self.set_capture(true);
        } else {
            self.set_capture(false);
        }
    }

    fn carve_pumpkin(&mut self, at: [i32; 3], metadata: u8) {
        let slot = self.hotbar.selected();
        let holding = self.hotbar.slot(slot) == Some("torch");
        match pumpkin_carve(metadata, holding) {
            PumpkinCarve::Say(word) => self.gui.push_chat(word),
            PumpkinCarve::Set {
                metadata: next,
                spend_torch,
                word,
            } => {
                if !self.pending.is_empty() {
                    return;
                }
                let (min, max) = body::player_box_bounds(self.camera.eye);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("pumpkin", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    spend_torch.then_some(slot),
                );
                self.gui.push_chat(word);
            }
        }
    }

    fn eat_cake(&mut self, at: [i32; 3], metadata: u8) {
        if !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        let player = PlayerBox { min, max };
        match body::cake_bite(metadata) {
            body::CakeBite::Slice(next) => {
                self.send_edit(
                    encode_place(at[0], at[1], at[2], Block::new("cake", next), player),
                    TAG_PLACE,
                    None,
                );
                self.taste_cake();
                self.gui.push_chat("yum");
            }
            body::CakeBite::Finish => {
                self.cake_eat = Some(at);
                self.send_edit(encode_break(at[0], at[1], at[2]), TAG_BREAK, None);
            }
        }
    }

    fn taste_cake(&mut self) {
        self.hunger.eat(body::CAKE_FOOD);
        if self.hearts < body::HEARTS {
            self.hearts += 1;
        }
        self.note_taste(false);
    }

    /// Punch. True when the look hits an end crystal.
    fn blast_crystal(&mut self) -> bool {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let at = hit.block;
        if self.world.block_at(at[0], at[1], at[2]).id != "end_crystal" {
            return false;
        }
        if !self.pending.is_empty() {
            return true;
        }
        self.crystal_blast = Some(at);
        self.send_edit(encode_break(at[0], at[1], at[2]), TAG_BREAK, None);
        if self.pending.is_empty() {
            self.crystal_blast = None;
        }
        true
    }

    /// Punch or F. True when the look hits a dragon egg, whether or not it moved.
    fn hop_egg(&mut self) -> bool {
        let Some(hit) = look::raycast(self.camera.eye, self.camera.forward(), &self.world) else {
            return false;
        };
        let at = hit.block;
        if self.world.block_at(at[0], at[1], at[2]).id != "dragon_egg" {
            return false;
        }
        let Some(dest) = body::egg_hop(at, &self.world) else {
            self.gui.push_chat("the egg stays");
            return true;
        };
        if !self.pending.is_empty() {
            return true;
        }
        self.egg_hop = Some((at, dest));
        self.send_edit(encode_break(at[0], at[1], at[2]), TAG_BREAK, None);
        if self.pending.is_empty() {
            self.egg_hop = None;
        }
        true
    }

    fn fill_frame(&mut self, at: [i32; 3], metadata: u8) {
        let holding = self.hotbar.slot(self.hotbar.selected()) == Some("eye_of_ender");
        match frame_eye(metadata, holding) {
            FrameEye::Full => self.gui.push_chat("filled"),
            FrameEye::NeedEye => self.gui.push_chat("need an eye"),
            FrameEye::Set => {
                if !self.pending.is_empty() {
                    return;
                }
                let slot = self.hotbar.selected();
                let beside = filled_beside(&self.world, at);
                let (min, max) = body::player_box_bounds(self.camera.eye);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("end_portal_frame", 1),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                self.gui.push_chat(ring_word(beside));
            }
        }
    }

    fn plant_pot(&mut self, at: [i32; 3], metadata: u8) {
        let holding = self.hotbar.slot(self.hotbar.selected()) == Some("flower");
        match pot_plant(metadata, holding) {
            PotPlant::Full => self.gui.push_chat("planted"),
            PotPlant::NeedFlower => self.gui.push_chat("need a flower"),
            PotPlant::Plant => {
                if !self.pending.is_empty() {
                    return;
                }
                let slot = self.hotbar.selected();
                let (min, max) = body::player_box_bounds(self.camera.eye);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("flower_pot", 1),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                self.gui.push_chat("planted");
            }
        }
    }

    /// Flash a target and pulse a door, trapdoor, or gate beside it.
    fn hit_target(&mut self, at: [i32; 3]) -> &'static str {
        let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
        self.play_world(crate::audio::Sound::Note(4), source);
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "target" || !self.pending.is_empty() {
            return "hit";
        }
        if block.metadata & 1 == 0 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("target", 1),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
        self.target_hit = Some((at, 0.35, false));
        "hit"
    }

    fn flush_target(&mut self, dt: f32) {
        let Some((at, left, pulsed)) = self.target_hit else {
            return;
        };
        let left = left - dt;
        if !self.pending.is_empty() {
            self.target_hit = Some((at, left, pulsed));
            return;
        }
        if !pulsed {
            self.target_hit = Some((at, left, true));
            if let Some((cell, other)) = press_target(&self.world, at) {
                self.toggle_swing(other.id, cell, other.metadata);
            }
            return;
        }
        if left > 0.0 {
            self.target_hit = Some((at, left, true));
            return;
        }
        self.target_hit = None;
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id == "target" && block.metadata & 1 != 0 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("target", 0),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
    }

    /// Flash the open rod and pulse a door, trapdoor, or gate beside it.
    fn strike_rod(&mut self) -> &'static str {
        let Some(at) = self.gui.rod_at() else {
            return "strike";
        };
        let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
        self.play_world(crate::audio::Sound::Note(0), source);
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "lightning_rod" || !self.pending.is_empty() {
            return "strike";
        }
        if block.metadata & 8 == 0 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("lightning_rod", block.metadata | 8),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
        self.rod_strike = Some((at, 0.35, false));
        "strike"
    }

    fn flush_rod(&mut self, dt: f32) {
        let Some((at, left, pulsed)) = self.rod_strike else {
            return;
        };
        let left = left - dt;
        if !self.pending.is_empty() {
            self.rod_strike = Some((at, left, pulsed));
            return;
        }
        if !pulsed {
            self.rod_strike = Some((at, left, true));
            if let Some((cell, other)) = press_target(&self.world, at) {
                self.toggle_swing(other.id, cell, other.metadata);
            }
            return;
        }
        if left > 0.0 {
            self.rod_strike = Some((at, left, true));
            return;
        }
        self.rod_strike = None;
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id == "lightning_rod" && block.metadata & 8 != 0 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("lightning_rod", block.metadata & 7),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
    }

    /// Charge the open anchor with glowstone, or set spawn once it holds a charge.
    fn charge_anchor(&mut self) -> &'static str {
        let Some(at) = self.gui.anchor_at() else {
            return "need glowstone";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "respawn_anchor" || !self.pending.is_empty() {
            return "need glowstone";
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        match self.hotbar.anchor_step(block.metadata) {
            hotbar::AnchorStep::NeedGlow => "need glowstone",
            hotbar::AnchorStep::SetSpawn => {
                if self.note_respawn("anchor", at) {
                    self.spawn_at = Some(at);
                    "spawn set"
                } else {
                    "spawn stays"
                }
            }
            hotbar::AnchorStep::Charge(slot) => {
                let next = block.metadata.saturating_add(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("respawn_anchor", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                if self.note_respawn("anchor", at) {
                    self.spawn_at = Some(at);
                }
                hotbar::anchor_word(next)
            }
        }
    }

    /// Point the held compass at the open lodestone.
    fn link_lodestone(&mut self) -> &'static str {
        let Some(at) = self.gui.lodestone_at() else {
            return "linked";
        };
        if !self.holding_compass() {
            return "need a compass";
        }
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "lodestone" {
            return "need a lodestone";
        }
        self.lodestone = Some(at);
        "linked"
    }

    /// Put the held item in the frame, or take the shown item back.
    fn display_frame(&mut self, at: [i32; 3]) -> String {
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "item_frame" {
            return "empty".to_string();
        }
        if !self.pending.is_empty() {
            return if block.metadata & 8 != 0 {
                "shown".to_string()
            } else {
                "empty".to_string()
            };
        }
        let path = crate::net::world_file(&self.loaded_world);
        let stored = rebuild_world::read_chests(&path)
            .unwrap_or_default()
            .into_iter()
            .find(|chest| chest.x == at[0] && chest.y == at[1] && chest.z == at[2])
            .and_then(|chest| {
                chest
                    .slots
                    .into_iter()
                    .find(|slot| slot.slot == 0 && slot.count > 0)
            })
            .map(|slot| slot.id)
            .unwrap_or_default();
        let held = self.hotbar.slot(self.hotbar.selected()).map(str::to_string);
        let room = if stored.is_empty() {
            true
        } else {
            self.hotbar.room_for(&stored)
        };
        match frame_move(held.as_deref(), &stored, room) {
            FrameMove::Store => {
                let item = held.unwrap_or_default();
                self.hotbar.consume_at(self.hotbar.selected());
                self.hotbar_dirty = true;
                self.save_frame(at, &item);
                self.mark_frame(at, true);
                item.replace('_', " ")
            }
            FrameMove::Take => {
                if !self.hotbar.collect(&stored) {
                    return "full".to_string();
                }
                self.hotbar_dirty = true;
                self.save_frame(at, "");
                self.mark_frame(at, false);
                "taken".to_string()
            }
            FrameMove::Full => "full".to_string(),
            FrameMove::Empty => "empty".to_string(),
        }
    }

    fn save_frame(&mut self, at: [i32; 3], id: &str) {
        let count = u16::from(!id.is_empty());
        if self
            .link
            .send(encode_chest(at[0], at[1], at[2], &pot_wire(id, count)))
            .is_err()
        {
            self.note_disconnect();
        }
    }

    fn mark_frame(&mut self, at: [i32; 3], filled: bool) {
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "item_frame" || !self.pending.is_empty() {
            return;
        }
        let facing = match block.metadata & 7 {
            2 => 2,
            3 => 3,
            4 => 4,
            _ => 1,
        };
        let want = if filled { facing | 8 } else { facing };
        if block.metadata == want {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("item_frame", want),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Open the wall shelf and show the three stacks saved on it.
    fn open_board_at(&mut self, at: [i32; 3]) -> bool {
        let path = crate::net::world_file(&self.loaded_world);
        let mut slots = [
            (String::new(), 0u16),
            (String::new(), 0),
            (String::new(), 0),
        ];
        for slot in rebuild_world::read_chests(&path)
            .unwrap_or_default()
            .into_iter()
            .find(|chest| chest.x == at[0] && chest.y == at[1] && chest.z == at[2])
            .map(|chest| chest.slots)
            .unwrap_or_default()
        {
            let index = usize::from(slot.slot);
            if index < 3 && slot.count > 0 {
                slots[index] = (slot.id, slot.count);
            }
        }
        self.gui.open_board(at, &slots)
    }

    /// Store one held item on the shelf, or take one back into the hotbar.
    fn store_board(&mut self) -> &'static str {
        let Some(at) = self.gui.board_at() else {
            return "empty";
        };
        let stored = self.gui.board_slots();
        let held = self.hotbar.slot(self.hotbar.selected()).map(str::to_string);
        let room = held
            .as_deref()
            .map(|id| self.hotbar.room_for(id))
            .unwrap_or_else(|| {
                stored
                    .iter()
                    .any(|(id, count)| *count > 0 && self.hotbar.room_for(id))
            });
        let view = [
            (stored[0].0.as_str(), stored[0].1),
            (stored[1].0.as_str(), stored[1].1),
            (stored[2].0.as_str(), stored[2].1),
        ];
        match board_move(held.as_deref(), &view, room) {
            BoardMove::Store { slot, count } => {
                let id = if stored[slot].1 == 0 {
                    held.unwrap_or_default()
                } else {
                    stored[slot].0.clone()
                };
                self.hotbar.consume_at(self.hotbar.selected());
                self.hotbar_dirty = true;
                self.gui.set_board_slot(slot, &id, count);
                self.save_board(at);
                self.mark_board(at);
                "stored"
            }
            BoardMove::Take { slot, count } => {
                let id = stored[slot].0.clone();
                if !self.hotbar.collect(&id) {
                    return "full";
                }
                self.hotbar_dirty = true;
                let next = if count == 0 { String::new() } else { id };
                self.gui.set_board_slot(slot, &next, count);
                self.save_board(at);
                self.mark_board(at);
                "taken"
            }
            BoardMove::Full => "full",
            BoardMove::Empty => "empty",
        }
    }

    fn save_board(&mut self, at: [i32; 3]) {
        if self
            .link
            .send(encode_chest(
                at[0],
                at[1],
                at[2],
                &board_wire(&self.gui.board_slots()),
            ))
            .is_err()
        {
            self.note_disconnect();
        }
    }

    fn mark_board(&mut self, at: [i32; 3]) {
        let block = self.world.block_at(at[0], at[1], at[2]);
        let filled = self
            .gui
            .board_slots()
            .iter()
            .filter(|(_, count)| *count > 0)
            .count() as u8;
        let want = rebuild_world::shelf_meta(rebuild_world::shelf_facing(block.metadata), filled);
        if block.id != "shelf" || block.metadata == want || !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("shelf", want),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Open the pot and show the one stack saved in it.
    fn open_pot_at(&mut self, at: [i32; 3]) -> bool {
        let path = crate::net::world_file(&self.loaded_world);
        let (id, count) = rebuild_world::read_chests(&path)
            .unwrap_or_default()
            .into_iter()
            .find(|chest| chest.x == at[0] && chest.y == at[1] && chest.z == at[2])
            .and_then(|chest| {
                chest
                    .slots
                    .into_iter()
                    .find(|slot| slot.slot == 0 && slot.count > 0)
            })
            .map(|slot| (slot.id, slot.count))
            .unwrap_or_default();
        self.gui.open_pot(at, &id, count)
    }

    /// Store one held item, or take one back into the hotbar.
    fn store_pot(&mut self) -> &'static str {
        let Some(at) = self.gui.pot_at() else {
            return "empty";
        };
        let (stored, count) = self.gui.pot_stack();
        let held = self.hotbar.slot(self.hotbar.selected()).map(str::to_string);
        let room = held
            .as_deref()
            .map(|id| self.hotbar.room_for(id))
            .unwrap_or_else(|| !stored.is_empty() && self.hotbar.room_for(&stored));
        match pot_move(held.as_deref(), &stored, count, room) {
            PotMove::Store => {
                let item = held.unwrap_or_default();
                let next = count.saturating_add(1);
                let id = if count == 0 { item } else { stored };
                self.hotbar.consume_at(self.hotbar.selected());
                self.hotbar_dirty = true;
                self.gui.set_pot(&id, next);
                self.save_pot(at, &id, next);
                self.mark_pot(at, true);
                "stored"
            }
            PotMove::Take => {
                let next = count.saturating_sub(1);
                if !self.hotbar.collect(&stored) {
                    return "full";
                }
                self.hotbar_dirty = true;
                let id = if next == 0 { String::new() } else { stored };
                self.gui.set_pot(&id, next);
                self.save_pot(at, &id, next);
                self.mark_pot(at, next > 0);
                "taken"
            }
            PotMove::Full => "full",
            PotMove::Empty => "empty",
        }
    }

    fn save_pot(&mut self, at: [i32; 3], id: &str, count: u16) {
        if self
            .link
            .send(encode_chest(at[0], at[1], at[2], &pot_wire(id, count)))
            .is_err()
        {
            self.note_disconnect();
        }
    }

    fn mark_pot(&mut self, at: [i32; 3], filled: bool) {
        let block = self.world.block_at(at[0], at[1], at[2]);
        let want = u8::from(filled);
        if block.id != "decorated_pot" || block.metadata == want || !self.pending.is_empty() {
            return;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("decorated_pot", want),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    /// Light the open bulb, or put it out.
    fn toggle_bulb(&mut self) -> &'static str {
        let Some(at) = self.gui.bulb_at() else {
            return "lit";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        let lighting = block.id == "copper_bulb" && block.metadata & 1 == 0;
        let word = if lighting { "lit" } else { "out" };
        if block.id != "copper_bulb" || !self.pending.is_empty() {
            return word;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("copper_bulb", if lighting { 1 } else { 0 }),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
        word
    }

    /// Light the open candle, or put it out.
    fn toggle_candle(&mut self) -> &'static str {
        let Some(at) = self.gui.candle_at() else {
            return "lit";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        let lighting = block.id == "candle" && block.metadata & 1 == 0;
        let word = if lighting { "lit" } else { "out" };
        if block.id != "candle" || !self.pending.is_empty() {
            return word;
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("candle", if lighting { 1 } else { 0 }),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
        word
    }

    /// Ring the open bell, tip it, mote, and swing it back after a short beat.
    fn ring_bell(&mut self) -> &'static str {
        let Some(at) = self.gui.bell_at() else {
            return "ring";
        };
        let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
        self.play_world(crate::audio::Sound::Note(12), source);
        self.drops.note([source[0], at[1] as f32 + 1.05, source[2]], 12);
        self.rebuild_drops();
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "bell" || !self.pending.is_empty() {
            return "ring";
        }
        if block.metadata & 1 == 0 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("bell", 1),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
        self.bell_return = Some((at, 0.45));
        "ring"
    }

    fn flush_bell(&mut self, dt: f32) {
        let Some((at, left)) = self.bell_return else {
            return;
        };
        let left = left - dt;
        if left > 0.0 {
            self.bell_return = Some((at, left));
            return;
        }
        if !self.pending.is_empty() {
            self.bell_return = Some((at, 0.0));
            return;
        }
        self.bell_return = None;
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id == "bell" && block.metadata & 1 == 1 {
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("bell", 0),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
        }
    }

    fn play_note_block(&mut self, at: [i32; 3], pitch: u8) {
        let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
        self.play_world(crate::audio::Sound::Note(pitch), source);
        self.tell_parrot(crate::mobs::Mimic::Note(pitch), source);
        self.gui.push_chat(crate::audio::note_name(pitch));
        // Rising mote above the block, tinted by the pitch just played.
        self.drops.note(
            [source[0], at[1] as f32 + 1.05, source[2]],
            pitch,
        );
        self.rebuild_drops();
        if self.pending.is_empty() {
            self.retune(at, crate::audio::next_pitch(pitch));
        }
    }

    /// Insert a music disc to play, empty-hand eject, or replay while filled.
    fn use_jukebox(&mut self, at: [i32; 3], metadata: u8) {
        let source = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5];
        let mote = [source[0], at[1] as f32 + 1.05, source[2]];
        let holding_disc = self.hotbar.slot(self.hotbar.selected()) == Some("music_disc");
        let filled = metadata & 1 == 1;
        if filled {
            if holding_disc {
                self.play_jukebox(source, mote);
                return;
            }
            if self.hotbar.slot(self.hotbar.selected()).is_some() {
                self.gui.push_chat("playing");
                return;
            }
            if !self.pending.is_empty() {
                return;
            }
            if !self.hotbar.room_for("music_disc") && !self.bag.room_for("music_disc") {
                self.gui.push_chat("no room");
                return;
            }
            let (min, max) = body::player_box_bounds(self.camera.eye);
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("jukebox", 0),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                None,
            );
            if !self.hotbar.collect("music_disc") {
                self.bag.collect("music_disc");
            }
            self.hotbar_dirty = true;
            self.gui.push_chat("ejected");
            return;
        }
        if !holding_disc {
            self.gui.push_chat("need a disc");
            return;
        }
        if !self.pending.is_empty() {
            return;
        }
        let slot = self.hotbar.selected();
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("jukebox", 1),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        self.play_jukebox(source, mote);
    }

    fn play_jukebox(&mut self, source: [f32; 3], mote: [f32; 3]) {
        self.play_world(crate::audio::Sound::Disc, source);
        self.drops.note(mote, 12);
        self.rebuild_drops();
        self.gui.push_chat("playing");
    }

    /// Put a book on the aimed lectern, or read the page it already holds.
    /// Add one plant, or take bone meal when the aimed composter is full.
    fn fill_compost(&mut self) -> &'static str {
        let Some(at) = self.gui.compost_at() else {
            return "need plants";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "composter" {
            return "need plants";
        }
        if !self.pending.is_empty() {
            return "need plants";
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        match self.hotbar.compost_step(block.metadata) {
            hotbar::CompostStep::NeedPlants => "need plants",
            hotbar::CompostStep::NoRoom => "no room",
            hotbar::CompostStep::Take => {
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("composter", 0),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    None,
                );
                "bone meal"
            }
            hotbar::CompostStep::Fill(slot) => {
                let next = block.metadata.saturating_add(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("composter", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                hotbar::compost_word(next)
            }
        }
    }

    /// Pour one honey level into a hive the bee just visited.
    fn bee_fill(&mut self, at: [i32; 3]) {
        if !self.pending.is_empty() {
            self.bee.keep_pollen();
            return;
        }
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "beehive" || block.metadata >= hotbar::HONEY_READY {
            return;
        }
        let next = block.metadata.saturating_add(1);
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("beehive", next),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
        self.gui.push_chat(hotbar::hive_word(next));
    }

    /// Add one flower, or take honey when the aimed beehive is full.
    fn fill_hive(&mut self) -> &'static str {
        let Some(at) = self.gui.hive_at() else {
            return "need a flower";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "beehive" {
            return "need a flower";
        }
        if !self.pending.is_empty() {
            return "need a flower";
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        match self.hotbar.hive_step(block.metadata) {
            hotbar::HiveStep::NeedFlower => "need a flower",
            hotbar::HiveStep::NoRoom => "no room",
            hotbar::HiveStep::Take => {
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("beehive", 0),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    None,
                );
                "honey"
            }
            hotbar::HiveStep::Fill(slot) => {
                let next = block.metadata.saturating_add(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("beehive", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                hotbar::hive_word(next)
            }
        }
    }

    fn beside_water(&self, at: [i32; 3]) -> bool {
        [
            [at[0] + 1, at[1], at[2]],
            [at[0] - 1, at[1], at[2]],
            [at[0], at[1], at[2] + 1],
            [at[0], at[1], at[2] - 1],
            [at[0], at[1] + 1, at[2]],
            [at[0], at[1] - 1, at[2]],
        ]
        .into_iter()
        .any(|cell| self.world.block_at(cell[0], cell[1], cell[2]).id == "water")
    }

    /// Pour one level from a water bucket, scoop one into an empty bucket, or
    /// take one level from marsh water beside the pot.
    fn fill_cauldron(&mut self) -> &'static str {
        let Some(at) = self.gui.cauldron_at() else {
            return "need a bucket";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "cauldron" {
            return "need a bucket";
        }
        if !self.pending.is_empty() {
            return "need a bucket";
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        let step = self.hotbar.cauldron_step(block.metadata);
        match step {
            hotbar::CauldronStep::NoRoom => "no room",
            hotbar::CauldronStep::Full => "full",
            hotbar::CauldronStep::NeedBucket if self.beside_water(at) && block.metadata < 3 => {
                let next = block.metadata.saturating_add(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("cauldron", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    None,
                );
                hotbar::cauldron_word(next)
            }
            hotbar::CauldronStep::NeedBucket => "need a bucket",
            hotbar::CauldronStep::Pour(slot) => {
                let next = block.metadata.saturating_add(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("cauldron", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                hotbar::cauldron_word(next)
            }
            hotbar::CauldronStep::Scoop(slot) => {
                let next = block.metadata.saturating_sub(1);
                self.send_edit(
                    encode_place(
                        at[0],
                        at[1],
                        at[2],
                        Block::new("cauldron", next),
                        PlayerBox { min, max },
                    ),
                    TAG_PLACE,
                    Some(slot),
                );
                "scooped"
            }
        }
    }

    fn store_shelf(&mut self) -> &'static str {
        let Some(at) = self.gui.shelf_at() else {
            return "need a book";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "chiseled_bookshelf" {
            return "need a book";
        }
        if !self.pending.is_empty() {
            return "need a book";
        }
        let count = block.metadata.min(6);
        let (min, max) = body::player_box_bounds(self.camera.eye);
        if let Some(slot) = self.hotbar.index_of("book") {
            if count >= 6 {
                return "full";
            }
            self.send_edit(
                encode_place(
                    at[0],
                    at[1],
                    at[2],
                    Block::new("chiseled_bookshelf", count + 1),
                    PlayerBox { min, max },
                ),
                TAG_PLACE,
                Some(slot),
            );
            return shelf_word(count + 1);
        }
        if count == 0 {
            return "need a book";
        }
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("chiseled_bookshelf", count - 1),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
        shelf_word(count - 1)
    }

    fn read_lectern(&mut self) -> &'static str {
        let Some(at) = self.gui.lectern_at() else {
            return "need a book";
        };
        let block = self.world.block_at(at[0], at[1], at[2]);
        if block.id != "lectern" {
            return "need a book";
        }
        if block.metadata & 1 == 1 {
            return gui::LECTERN_PAGE;
        }
        let Some(slot) = self.hotbar.index_of("book") else {
            return "need a book";
        };
        if !self.pending.is_empty() {
            return "need a book";
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("lectern", 1),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            Some(slot),
        );
        gui::LECTERN_PAGE
    }

    fn retune(&mut self, at: [i32; 3], pitch: u8) {
        let (min, max) = body::player_box_bounds(self.camera.eye);
        self.send_edit(
            encode_place(
                at[0],
                at[1],
                at[2],
                Block::new("note_block", pitch),
                PlayerBox { min, max },
            ),
            TAG_PLACE,
            None,
        );
    }

    fn flip_lever(&mut self, at: [i32; 3], metadata: u8) {
        if !self.pending.is_empty() {
            return;
        }
        let turning_on = metadata & 1 == 0;
        self.gui.push_chat(if turning_on { "on" } else { "off" });
        self.toggle_swing("lever", at, metadata);
        let Some((cell, other)) = press_target(&self.world, at) else {
            return;
        };
        if let Some(want_open) = lever_wants(turning_on, other.metadata & 1 == 1) {
            self.lever_follow = Some((cell, want_open));
        }
    }

    fn flush_lever(&mut self) {
        let Some((cell, want_open)) = self.lever_follow else {
            return;
        };
        if !self.pending.is_empty() {
            return;
        }
        let block = self.world.block_at(cell[0], cell[1], cell[2]);
        if !hinge(block.id) {
            self.lever_follow = None;
            return;
        }
        let is_open = block.metadata & 1 == 1;
        self.lever_follow = None;
        if is_open != want_open {
            self.toggle_swing(block.id, cell, block.metadata);
        }
    }

    fn note_cactus(&mut self, dt: f32) {
        let touching = body::touches_cactus(self.camera.eye, &self.world);
        let (left, poke) = body::cactus_poke(touching, self.cactus_left, dt);
        self.cactus_left = left;
        if poke {
            self.gui.push_chat("ouch");
            self.hurt(1);
        }
    }

    fn note_magma(&mut self, dt: f32) {
        let standing = body::stands_on_magma(self.camera.eye, &self.world);
        let (left, burn) = body::cactus_poke(standing, self.magma_left, dt);
        self.magma_left = left;
        if burn {
            self.gui.push_chat("burn");
            self.hurt(1);
        }
    }

    fn note_plate(&mut self) {
        self.flush_plate_close();
        if self.plate_close.is_some() {
            return;
        }
        let under = plate_under(&self.world, self.camera.eye);
        if under == self.plate_held.as_ref().map(|hold| hold.at) {
            return;
        }
        self.release_plate();
        if self.plate_close.is_some() {
            return;
        }
        let Some(at) = under else {
            return;
        };
        self.gui.push_chat("click");
        let mut opened = None;
        if let Some((cell, block)) = press_target(&self.world, at) {
            if block.metadata & 1 == 0 {
                self.toggle_swing(block.id, cell, block.metadata);
                opened = Some(cell);
            }
        }
        self.plate_held = Some(PlateHold { at, opened });
    }

    fn release_plate(&mut self) {
        let Some(hold) = self.plate_held.take() else {
            return;
        };
        if hold.opened.is_some() {
            self.plate_close = hold.opened;
            self.flush_plate_close();
        }
    }

    fn flush_plate_close(&mut self) {
        let Some(cell) = self.plate_close else {
            return;
        };
        if !self.pending.is_empty() {
            return;
        }
        self.plate_close = None;
        let block = self.world.block_at(cell[0], cell[1], cell[2]);
        if hinge(block.id) && block.metadata & 1 == 1 {
            self.toggle_swing(block.id, cell, block.metadata);
        }
    }

    fn note_fall(&mut self) {
        if !self.camera.on_ground {
            self.air_peak = self.air_peak.max(self.camera.eye.y);
            self.airborne = true;
            return;
        }
        if self.airborne {
            let fallen = self.air_peak - self.camera.eye.y;
            let drop = fallen.max(0.0).floor() as i32;
            self.note_land(drop);
            // Camera punch on any real landing; hurt still owns deadly falls.
            if fallen > 0.55 {
                let punch = (fallen / 3.5).clamp(0.3, 1.0);
                self.hit_kick = self.hit_kick.max(punch);
                if crate::body::fall_hurt(fallen) == 0 {
                    let feet = [
                        self.camera.eye.x,
                        self.camera.eye.y - body::EYE_HEIGHT,
                        self.camera.eye.z,
                    ];
                    let ground = body::ground_id(&self.world, self.camera.eye);
                    self.play_world_soft(crate::audio::step_for(ground), feet, 0.45);
                }
            }
            let amount = crate::body::fall_hurt(fallen);
            if amount > 0 {
                self.hurt(amount);
            }
        }
        self.airborne = false;
        self.air_peak = self.camera.eye.y;
    }

    fn respawn(&mut self) {
        let saved = self.spawn_at;
        let home = saved.filter(|&at| {
            let block = self.world.block_at(at[0], at[1], at[2]);
            block.id == "bed" || (block.id == "respawn_anchor" && block.metadata > 0)
        });
        let planned = if let Some(at) = home {
            Camera::anchor_eye(at)
        } else {
            self.spawn_at = None;
            Camera::drop_by_the_stairs()
        };
        let cell = eye_cell(planned.eye);
        let fresh = if self.note_respawn("death", cell) {
            planned
        } else {
            Camera::drop_by_the_stairs()
        };
        let (yaw, pitch) = fresh.yaw_pitch();
        self.camera.restore(fresh.eye, yaw, pitch);
        self.hearts = body::HEARTS;
        self.dead = false;
        self.hurt_flash = 0.0;
        self.sip = 0.0;
        self.sipping = false;
        self.chew_phase = 0.0;
        self.burp_due = false;
        self.cactus_left = 0.0;
        self.magma_left = 0.0;
        self.cake_eat = None;
        self.egg_hop = None;
        self.crystal_blast = None;
        self.hunger = body::Hunger::full();
        self.airborne = false;
        self.jump_asked = false;
        self.jump_blocked = false;
        self.sprint_asked = false;
        self.sprint_blocked = false;
        self.sprint_kick = 0.0;
        self.sprint_run = 0.0;
        self.sprint_show = 0.0;
        self.air_peak = self.camera.eye.y;
        self.boat.dismount();
        self.pig.dismount();
        self.horse.dismount();
        self.release_plate();
        self.pose_dirty = true;
        self.pose_left = 0.0;
        self.gui.push_chat("respawn");
        self.was_wet = false;
        self.water_sub = 0.0;
        self.splash_kick = 0.0;
        self.jump_kick = 0.0;
        self.slot_punch = 0.0;
        self.swap_kick = 0.0;
        self.break_kick = 0.0;
        self.clear_boat_feel();
        self.clear_cart_feel();
        self.clear_horse_feel();
        self.sync_pointer();
    }

    /// Ask `events.respawn` and send the first block it writes.
    ///
    /// False refuses the planned return. A death then uses the stairs, a bed
    /// stays awake, and an anchor does not become the saved spawn.
    fn note_respawn(&mut self, cause: &str, at: [i32; 3]) -> bool {
        let _ = self.hooks.drain_writes();
        let slots = hotbar_slots(&self.hotbar);
        let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
        let allow = self
            .hooks
            .respawn(&mut self.world, cause, at[0], at[1], at[2], &view);
        self.flush_script_writes();
        allow
    }

    pub fn tick(&mut self, dt: f32) {
        self.flush_hotbar();
        self.flush_bag();
        self.flush_pose(dt);
        self.flush_chest();
        self.gui.stoke(dt);
        self.gui.age_chat(dt);
        self.tick_compound_flow(dt);
        self.tell_landmarks();
        self.tell_cook_heat();
        self.tell_pump();
        self.tell_tank();
        self.tell_battery();
        self.tell_lamp();
        self.tell_spout();
        self.tell_drain();
        self.tell_generator();
        self.tell_blast_heat();
        self.tell_furnace_heat();
        self.tell_shape_pot();
        self.note_pose();
        if self.drops.tick_motes(dt) {
            self.rebuild_drops();
        }
        let generated = self.service_terrain();
        if generated {
            self.terrain_dirty = true;
            self.terrain_quiet = Self::TERRAIN_MESH_WAIT;
        } else if !self.terrain_lane.busy() && self.terrain_quiet > 0 {
            self.terrain_quiet -= 1;
        }
        let time = self.clock.time_of_day();
        let sky = self.clock.tick(dt);
        if time < 0.25 || time >= 0.75 {
            if sky.dawn {
                self.note_dawn();
                self.tell_sky_edge("dawn");
            }
            if sky.dusk {
                self.note_dusk();
                self.tell_sky_edge("dusk");
            }
        } else {
            if sky.dusk {
                self.note_dusk();
                self.tell_sky_edge("dusk");
            }
            if sky.dawn {
                self.note_dawn();
                self.tell_sky_edge("dawn");
            }
        }
        self.horn_wait = crate::audio::horn_cool(self.horn_wait, dt);
        // Night insects, dawn birds, rain, daytime wind, shore lapping, and campfire crackle.
        let time = self.clock.time_of_day();
        let night = crate::daylight::night_fog(time);
        let dawn = crate::daylight::dawn_chorus(time);
        let eye = self.camera.eye.to_array();
        let sky_open = open_sky(&self.world, eye);
        let wet = if sky_open {
            crate::daylight::rain_mood(time)
        } else {
            0.0
        };
        let open = if sky_open {
            crate::daylight::day_breeze(time) * (1.0 - wet * 0.65)
        } else {
            0.0
        };
        let was_dry = self.rain_mix < 0.35;
        self.cricket_mix += (night - self.cricket_mix) * (dt * 0.7).min(1.0);
        self.bird_mix += (dawn - self.bird_mix) * (dt * 0.8).min(1.0);
        self.rain_mix += (wet - self.rain_mix) * (dt * 1.1).min(1.0);
        self.wind_mix += (open - self.wind_mix) * (dt * 0.9).min(1.0);
        let cover = cave_drip_mix(&self.world, eye);
        self.drip_mix += (cover - self.drip_mix) * (dt * 1.4).min(1.0);
        let woods = if sky_open
            && rebuild_world::biome_at(eye[0].floor() as i32, eye[2].floor() as i32)
                == rebuild_world::Biome::Forest
        {
            (1.0 - wet * 0.45).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.rustle_mix += (woods - self.rustle_mix) * (dt * 1.0).min(1.0);
        if self.rain_mix < 0.15 {
            self.rain_told = false;
        }
        let (fire_mix, fire_at) = nearest_campfire(&self.world, eye);
        self.crackle_mix += (fire_mix - self.crackle_mix) * (dt * 2.4).min(1.0);
        self.crackle_at = fire_at;
        self.smoke_wait = (self.smoke_wait - dt).max(0.0);
        self.firefly_wait = (self.firefly_wait - dt).max(0.0);
        if self.playing && !self.menu.is_open() && !self.dead {
            if let Some(source) = self.crackle_at {
                if self.smoke_wait <= 0.0 {
                    self.smoke_wait = 0.32;
                    self.drops.smoke([source[0], source[1] + 0.35, source[2]]);
                    self.rebuild_drops();
                }
            }
        }
        let (shore_mix, shore_at) = nearest_water_edge(&self.world, eye);
        self.lap_mix += (shore_mix - self.lap_mix) * (dt * 2.2).min(1.0);
        self.lap_at = shore_at;
        // Dusk / night fireflies drift near open-sky water.
        let firefly_mood = if sky_open {
            (night * (1.0 - wet * 0.7) * shore_mix.max(0.35)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.playing && !self.menu.is_open() && !self.dead && firefly_mood > 0.25 {
            if self.firefly_wait <= 0.0 {
                self.firefly_wait = 0.22 + (1.0 - firefly_mood) * 0.55;
                let base = shore_at.unwrap_or([
                    eye[0] + 1.2,
                    eye[1] - 0.4,
                    eye[2] + 0.8,
                ]);
                // A few offsets so a quiet shore fills without one hot spot.
                let offsets = [
                    [0.0, 0.6, 0.0],
                    [1.4, 0.9, -0.6],
                    [-1.1, 1.1, 1.0],
                    [0.6, 1.4, 1.5],
                    [-0.8, 0.7, -1.3],
                ];
                let n = 1 + ((firefly_mood * 3.0) as usize).min(2);
                for offset in offsets.iter().take(n) {
                    self.drops.firefly([
                        base[0] + offset[0],
                        base[1] + offset[1],
                        base[2] + offset[2],
                    ]);
                }
                self.rebuild_drops();
                if !self.found_fireflies && self.drops.firefly_count() > 0 {
                    self.found_fireflies = true;
                    self.gui.push_chat("fireflies");
                }
            }
        }
        if self.playing && !self.menu.is_open() && !self.dead {
            if was_dry && self.rain_mix >= 0.35 && !self.rain_told {
                self.rain_told = true;
                self.gui.push_chat("rain");
            }
            if let Some(level) = self.audio.cricket_ready(dt, self.cricket_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Cricket,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.bird_ready(dt, self.bird_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Bird,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.rain_ready(dt, self.rain_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Rain,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.drip_ready(dt, self.drip_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Drip,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.rustle_ready(dt, self.rustle_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Rustle,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.wind_ready(dt, self.wind_mix) {
                self.audio.play_heard(
                    crate::audio::Sound::Wind,
                    crate::audio::Hearing::even(level),
                );
            }
            if let Some(level) = self.audio.lap_ready(dt, self.lap_mix) {
                if let Some(source) = self.lap_at {
                    self.play_world_soft(crate::audio::Sound::Lap, source, level);
                }
            }
            if let Some(level) = self.audio.crackle_ready(dt, self.crackle_mix) {
                if let Some(source) = self.crackle_at {
                    self.play_world_soft(crate::audio::Sound::Crackle, source, level);
                }
            }
        } else {
            let _ = self.audio.cricket_ready(dt, 0.0);
            let _ = self.audio.bird_ready(dt, 0.0);
            let _ = self.audio.rain_ready(dt, 0.0);
            let _ = self.audio.drip_ready(dt, 0.0);
            let _ = self.audio.rustle_ready(dt, 0.0);
            let _ = self.audio.wind_ready(dt, 0.0);
            let _ = self.audio.lap_ready(dt, 0.0);
            let _ = self.audio.crackle_ready(dt, 0.0);
        }
        self.mace_wait = (self.mace_wait - dt).max(0.0);
        self.slot_punch = (self.slot_punch - dt * 6.0).max(0.0);
        self.swap_kick = (self.swap_kick - dt * 10.0).max(0.0);
        self.break_kick = (self.break_kick - dt * 7.0).max(0.0);
        self.hurt_flash = (self.hurt_flash - dt * 2.2).max(0.0);
        self.sleep_fade = (self.sleep_fade - dt * 1.4).max(0.0);
        let sip_before = self.sip;
        self.sip = (self.sip - dt * 1.8).max(0.0);
        if self.sip > 0.02 {
            self.chew_phase += dt * 14.0;
        }
        if self.burp_due && !self.sipping && sip_before > 0.2 && self.sip <= 0.2 {
            self.burp_due = false;
            self.audio.play(crate::audio::Sound::Burp);
        }
        if self.sip <= 0.0 {
            self.burp_due = false;
            self.chew_phase = 0.0;
        }
        self.hit_kick = (self.hit_kick - dt * 4.5).max(0.0);
        self.splash_kick = (self.splash_kick - dt * 5.0).max(0.0);
        self.jump_kick = (self.jump_kick - dt * 5.5).max(0.0);
        if !self.boat.aboard() {
            self.boat_kick = (self.boat_kick - dt * 6.0).max(0.0);
            self.boat_show = (self.boat_show - dt * 5.0).max(0.0);
            self.boat_stroke = 0.0;
        }
        if !self.cart.aboard() {
            self.cart_kick = (self.cart_kick - dt * 6.0).max(0.0);
            self.cart_show = (self.cart_show - dt * 5.0).max(0.0);
            self.cart_stroke = 0.0;
        }
        if !self.horse.ridden() {
            self.horse_kick = (self.horse_kick - dt * 6.0).max(0.0);
            self.horse_show = (self.horse_show - dt * 5.0).max(0.0);
            self.horse_stroke = 0.0;
        }
        self.bob_amount = (self.bob_amount - dt * 2.5).max(0.0);
        let crouch_goal = if self.input.crouch { 1.0 } else { 0.0 };
        self.crouch += (crouch_goal - self.crouch) * (dt * 10.0).min(1.0);
        if !self.trial_due && self.zombie.rest(dt) {
            self.rebuild_zombie();
        }
        {
            let mut night_dirty = false;
            for zombie in self.night.iter_mut() {
                if zombie.rest(dt) {
                    night_dirty = true;
                }
            }
            let before = self.night.len();
            // spent() bodies are removed in NightHorde::tick; flash still rebuilds here.
            if night_dirty || self.night.len() != before {
                self.rebuild_zombie();
            }
        }
        if !self.trial_due && self.breeze.rest(dt) {
            self.rebuild_breeze();
        }
        self.audio.set_muted(self.menu.muted());
        let light = self.sun_light();
        let lookups = crate::daylight::shadow_lookups(light.direction);
        if lookups != self.wanted_lookups {
            self.wanted_lookups = lookups;
            self.wanted_sun = light.direction;
        }
        self.pin_held_light_shot_camera();
        self.pin_night_spawn_shot_camera();
        self.pin_crouch_shot_camera();
        self.pin_sprint_shot_camera();
        self.pin_swim_shot_camera();
        self.pin_boat_shot_camera();
        self.pin_cart_shot_camera();
        self.pin_horse_shot_camera();
        self.pin_attack_shot_camera();
        self.pin_hunger_shot_camera();
        self.pin_eat_shot_camera();
        self.pin_jump_shot_camera();
        self.pin_place_shot_camera();
        self.pin_swap_shot_camera();
        self.pin_break_shot_camera();
        self.sync_held_light();
        self.service_meshes();
        self.note_sky();
        self.poll_server();
        self.flush_lever();
        self.flush_bell(dt);
        self.flush_rod(dt);
        self.tick_fish_bobber(dt);
        self.flush_target(dt);
        if self.gui.is_open() || self.menu.is_open() || self.dead {
            return;
        }
        let mut obstacles = vec![
            self.sheep.bounds(),
            self.pig.bounds(),
            self.horse.bounds(),
            self.zombie.bounds(),
            self.breeze.bounds(),
            self.golem.bounds(),
            self.wolf.bounds(),
            self.cat.bounds(),
            self.parrot.bounds(),
            self.frog.bounds(),
            self.axolotl.bounds(),
            self.chicken.bounds(),
            self.bee.bounds(),
            self.fox.bounds(),
            self.goat.bounds(),
        ];
        for zombie in self.night.iter() {
            if zombie.living() {
                obstacles.push(zombie.bounds());
            }
        }
        if self.boat.aboard() {
            self.steer_boat(dt);
            self.sprint_asked = false;
            self.sprint_blocked = false;
            self.sprint_run = 0.0;
            self.sprint_kick = (self.sprint_kick - dt * 6.0).max(0.0);
        } else if self.cart.aboard() {
            self.steer_minecart(dt);
            self.sprint_asked = false;
            self.sprint_blocked = false;
            self.sprint_run = 0.0;
            self.sprint_kick = (self.sprint_kick - dt * 6.0).max(0.0);
        } else if self.horse.ridden() {
            self.steer_horse(dt);
            self.sprint_asked = false;
            self.sprint_blocked = false;
            self.sprint_run = 0.0;
            self.sprint_kick = (self.sprint_kick - dt * 6.0).max(0.0);
        } else if self.pig.ridden() {
            self.steer_pig(dt);
            self.sprint_asked = false;
            self.sprint_blocked = false;
            self.sprint_run = 0.0;
            self.sprint_kick = (self.sprint_kick - dt * 6.0).max(0.0);
        } else {
            let mut motion = self.input;
            if motion.crouch {
                motion.fast = false;
            }
            let feet = body::feet_y(self.camera.eye.y);
            let leaving = motion.jump && body::ground_jump(&self.camera, &self.world);
            if leaving {
                if !self.jump_asked {
                    self.jump_asked = true;
                    let cell = [
                        self.camera.eye.x.floor() as i32,
                        (feet + 1.0e-3).floor() as i32,
                        self.camera.eye.z.floor() as i32,
                    ];
                    self.jump_blocked = self.try_script_jump(cell[0], cell[1], cell[2]);
                    if !self.jump_blocked {
                        self.jump_kick = 1.0;
                    }
                }
                if self.jump_blocked {
                    motion.jump = false;
                }
            } else {
                self.jump_asked = false;
                self.jump_blocked = false;
            }
            let dashing =
                motion.fast && (motion.forward || motion.back || motion.left || motion.right);
            let mut sprinting = false;
            if dashing {
                if !self.sprint_asked {
                    self.sprint_asked = true;
                    let cell = [
                        self.camera.eye.x.floor() as i32,
                        (feet + 1.0e-3).floor() as i32,
                        self.camera.eye.z.floor() as i32,
                    ];
                    self.sprint_blocked = self.try_script_sprint(cell[0], cell[1], cell[2]);
                    if !self.sprint_blocked {
                        // Snap open for a clear sprint start.
                        self.sprint_kick = 1.0;
                        self.sprint_run = 0.0;
                    }
                }
                if self.sprint_blocked {
                    motion.fast = false;
                } else {
                    sprinting = true;
                    self.sprint_kick = 1.0;
                    self.sprint_run += dt;
                }
            } else {
                if self.sprint_asked && !self.sprint_blocked && self.sprint_run > 0.35 {
                    self.audio.play(crate::audio::Sound::Breath);
                    if self.sprint_run > 2.5 {
                        self.gui.push_chat("winded");
                    }
                }
                self.sprint_asked = false;
                self.sprint_blocked = false;
                self.sprint_run = 0.0;
            }
            if !sprinting {
                self.sprint_kick = (self.sprint_kick - dt * 6.0).max(0.0);
            }
            self.camera
                .step_paced(dt, &motion, &self.world, &obstacles, self.hunger.pace());
            let stepping = motion.forward || motion.back || motion.left || motion.right;
            if stepping && self.camera.on_ground {
                let pace = if motion.fast { 11.0 } else { 8.0 };
                self.bob_phase += dt * pace;
                self.bob_amount = (self.bob_amount + dt * 5.0).min(1.0);
            }
            self.note_plate();
            self.note_cactus(dt);
            self.note_magma(dt);
        }
        let sprinting_now = self.sprint_asked && !self.sprint_blocked;
        let show_goal = if sprinting_now { 1.0 } else { 0.0 };
        self.sprint_show += (show_goal - self.sprint_show) * (dt * 12.0).min(1.0);
        let moving = self.input.forward || self.input.back || self.input.left || self.input.right;
        {
            let look = self.camera.walk_axes().0;
            self.player.tick(
                dt,
                look.x,
                look.z,
                moving && self.camera.on_ground,
                self.dig.is_held(),
            );
        }
        match self.hunger.tick(
            dt,
            moving,
            self.dig.is_held() || sprinting_now,
            self.hearts < body::HEARTS,
        ) {
            crate::body::HungerTick::Hurt => self.apply_damage(1),
            crate::body::HungerTick::Heal => {
                if self.hearts < body::HEARTS {
                    self.hearts += 1;
                }
            }
            crate::body::HungerTick::None => {}
        }
        let wet = body::in_water(self.camera.eye, &self.world);
        let submerged = body::eye_in_water(self.camera.eye, &self.world);
        let water_goal = if submerged { 1.0 } else { 0.0 };
        self.water_sub += (water_goal - self.water_sub) * (dt * 8.0).min(1.0);
        let aboard = self.boat.aboard()
            || self.cart.aboard()
            || self.horse.ridden()
            || self.pig.ridden();
        let feet = [
            self.camera.eye.x,
            self.camera.eye.y - body::EYE_HEIGHT,
            self.camera.eye.z,
        ];
        let edge = if aboard {
            None
        } else {
            crate::audio::water_edge(self.was_wet, wet)
        };
        let splashed = edge.is_some();
        if let Some(edge) = edge {
            self.play_world(edge.sound(), feet);
            if matches!(edge, crate::audio::WaterEdge::Enter) {
                self.splash_kick = 1.0;
            }
        }
        self.was_wet = wet;
        let sneaking = self.input.crouch && self.camera.on_ground;
        if self.audio.footstep(
            moving && self.camera.on_ground && !aboard && !wet && !splashed,
            dt,
            sneaking,
        ) {
            let ground = body::ground_id(&self.world, self.camera.eye);
            if sneaking {
                self.play_world_soft(crate::audio::step_for(ground), feet, 0.35);
            } else {
                self.play_world(crate::audio::step_for(ground), feet);
            }
        }
        if self.audio.stroke(wet && moving && !aboard && !splashed, dt) {
            self.play_world(crate::audio::Sound::Swim, feet);
        }
        self.note_fall();
        if self.golem_shut > 0.0 {
            self.golem_shut -= dt;
            if self.golem_shut <= 0.0 {
                self.golem_shut = 0.0;
                self.gui.push_chat("shut");
            }
        }
        self.entity_time += dt;
        let steps = (self.entity_time / 0.05) as u32;
        if steps > 0 {
            self.entity_time -= steps as f32 * 0.05;
            self.hooks.drive_entities(&mut self.world, steps);
            self.flush_script_writes();
            let feet = [self.camera.eye.x, self.camera.eye.z];
            self.sheep.tick_near(steps, feet, &self.world);
            self.rebuild_sheep();
            self.pig.tick_near(steps, feet, &self.world);
            self.rebuild_pig();
            self.horse.tick_near(steps, feet, &self.world);
            self.rebuild_horse();
            let (zmin, zmax) = self.zombie.bounds();
            let from = [
                (zmin[0] + zmax[0]) * 0.5,
                zmin[1] + 1.4,
                (zmin[2] + zmax[2]) * 0.5,
            ];
            let to = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
            let clear = look::clear_between(from, to, &self.world);
            if self.zombie.tick_near(steps, feet, clear, &self.world) {
                let blocked = self.hotbar.block_bite();
                if blocked {
                    self.hotbar_dirty = true;
                    self.gui.push_chat("blocked");
                }
                let amount = body::bite_hearts(blocked, 1, self.armor_soak());
                self.apply_damage(amount);
            }
            {
                let night = crate::daylight::night_fog(self.clock.time_of_day()) > 0.35;
                let held = self.held_emitter();
                let player = [
                    self.camera.eye.x,
                    self.camera.eye.y - body::EYE_HEIGHT,
                    self.camera.eye.z,
                ];
                let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
                let (night_bite, _night_dirty) = {
                    let world = &self.world;
                    let horde = &mut self.night;
                    let mut clear_of =
                        |from: [f32; 3]| look::clear_between(from, eye, world);
                    horde.tick(dt, steps, player, night, held, world, &mut clear_of)
                };
                if night_bite {
                    let blocked = self.hotbar.block_bite();
                    if blocked {
                        self.hotbar_dirty = true;
                        self.gui.push_chat("blocked");
                    }
                    let amount = body::bite_hearts(blocked, 1, self.armor_soak());
                    self.apply_damage(amount);
                }
            }
            self.rebuild_zombie();
            let mut guest_bite = false;
            let mut guests_moved = false;
            for guest in &mut self.guests {
                let (gmin, gmax) = {
                    let [x, y, z] = guest.origin();
                    ([x - 0.28, y, z - 0.28], [x + 0.28, y + 1.95, z + 0.28])
                };
                let from = [
                    (gmin[0] + gmax[0]) * 0.5,
                    gmin[1] + 1.4,
                    (gmin[2] + gmax[2]) * 0.5,
                ];
                let to = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
                let guest_clear = look::clear_between(from, to, &self.world);
                if guest.tick_near(steps, feet, guest_clear, &self.world) {
                    guest_bite = true;
                }
                if guest.hostile() {
                    guests_moved = true;
                }
            }
            if guest_bite {
                let blocked = self.hotbar.block_bite();
                if blocked {
                    self.hotbar_dirty = true;
                    self.gui.push_chat("blocked");
                }
                let amount = body::bite_hearts(blocked, 1, self.armor_soak());
                self.apply_damage(amount);
            }
            if guests_moved || guest_bite {
                self.rebuild_guests();
            }
            self.flush_live_spawns();
            self.breeze.tick_near(steps, feet);
            if self.breeze_due {
                self.grant_rod();
            }
            if self.slime_due {
                self.grant_slime();
            }
            if self.horn_due {
                self.grant_horn();
            }
            self.rebuild_breeze();
            if let Some(chore) = self.golem.tick(steps, &self.world) {
                match chore {
                    crate::mobs::Chore::Press(at) => {
                        self.gui.push_chat("click");
                        if let Some((cell, other)) = press_target(&self.world, at) {
                            self.toggle_swing(other.id, cell, other.metadata);
                        }
                    }
                    crate::mobs::Chore::Open(at) => {
                        self.gui.push_chat("opened");
                        self.golem_shut = 1.2;
                        self.play_world(
                            crate::audio::Sound::Place,
                            [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 0.5],
                        );
                    }
                }
            }
            self.rebuild_golem();
            if self.wolf.tick(steps, feet, self.zombie.feet(), &self.world) {
                self.play_world(crate::audio::Sound::Step, self.wolf.origin());
                let (min, max) = self.zombie.bounds();
                let origin = [
                    (min[0] + max[0]) * 0.5,
                    min[1],
                    (min[2] + max[2]) * 0.5,
                ];
                let dropped =
                    self.hurt_mob("zombie", 1, origin, |s, n| s.zombie.hurt(n));
                self.gui.push_chat(if dropped { "down" } else { "bite" });
                if dropped && !self.wave_breeze {
                    if let Some(at) = self.trial_at {
                        let prize = self.pay_trial(at);
                        self.gui.push_chat(prize);
                    }
                }
                self.rebuild_zombie();
            }
            self.rebuild_wolf();
            if self.cat.tick(steps, feet, self.zombie.feet(), &self.world) {
                let spot = self.cat.origin();
                self.zombie.spook([spot[0], spot[2]]);
                self.play_world(crate::audio::Sound::Step, spot);
                self.gui.push_chat("hiss");
                self.rebuild_zombie();
            }
            self.rebuild_cat();
            self.tick_scarecrow();
            let body = [
                self.camera.eye.x,
                self.camera.eye.y - body::EYE_HEIGHT,
                self.camera.eye.z,
            ];
            let (look_yaw, _) = self.camera.yaw_pitch();
            if let Some(sound) = self.parrot.tick(steps, body, look_yaw, &self.world) {
                let spot = self.parrot.origin();
                match sound {
                    crate::mobs::Mimic::Note(pitch) => {
                        self.play_world(crate::audio::Sound::Note(pitch), spot);
                    }
                    crate::mobs::Mimic::Horn => {
                        self.play_world(crate::audio::Sound::Horn, spot);
                    }
                }
                self.gui.push_chat("echo");
            }
            self.rebuild_parrot();
            if self.frog.tick(steps, &self.world) {
                let spot = self.frog.origin();
                self.play_world(crate::audio::Sound::Step, spot);
                self.grant_slime();
            }
            self.rebuild_frog();
            self.axolotl.tick(steps, feet, &self.world);
            self.rebuild_axolotl();
            if self.chicken.tick(steps, feet, &self.world) {
                self.gui.push_chat("egg");
            }
            self.rebuild_chicken();
            let feet_at = [
                self.camera.eye.x,
                self.camera.eye.y - body::EYE_HEIGHT,
                self.camera.eye.z,
            ];
            if let Some(work) = self.bee.tick(steps, feet_at, &self.world) {
                match work {
                    crate::mobs::BeeWork::Pollen => {
                        if self.bee.pollen() {
                            self.gui.push_chat("pollen");
                        }
                    }
                    crate::mobs::BeeWork::Fill(at, _) => self.bee_fill(at),
                    crate::mobs::BeeWork::Sting => {
                        let spot = self.bee.origin();
                        self.play_world(crate::audio::Sound::Step, spot);
                        self.hurt(1);
                        self.gui.push_chat("sting");
                    }
                }
            }
            self.rebuild_bee();
            let treat = if self.fox.holding().is_none() {
                self.drops.food_near(self.fox.origin(), 6.0)
            } else {
                None
            };
            if self.fox.tick(steps, feet, treat, &self.world) {
                if let Some(id) = self.drops.take_food(self.fox.origin(), 1.6) {
                    self.fox.hold(id);
                    if self.fox.holding() == Some(id) {
                        self.gui.push_chat("held");
                        self.rebuild_drops();
                    }
                }
            }
            self.rebuild_fox();
            let feet_at = [
                self.camera.eye.x,
                self.camera.eye.y - body::EYE_HEIGHT,
                self.camera.eye.z,
            ];
            if let Some(ram) = self.goat.tick(steps, feet_at, &self.world) {
                self.goat_ram(ram);
            }
            self.rebuild_goat();
        }
        self.wanted_focus = chunk_of(self.camera.eye);
        let look = look::raycast(self.camera.eye, self.camera.forward(), &self.world);
        let (cell, hardness) = match look {
            Some(hit) => {
                let id = self
                    .world
                    .block_at(hit.block[0], hit.block[1], hit.block[2])
                    .id
                    .to_string();
                if id != self.break_id {
                    let slots = hotbar_slots(&self.hotbar);
                    let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
                    self.break_factor = self.hooks.break_scale(&mut self.world, &id, &view);
                    self.break_id = id;
                }
                let block = self
                    .world
                    .block_at(hit.block[0], hit.block[1], hit.block[2]);
                let tool = self.hotbar.dig_factor(block.id);
                let hardness = Pack::builtin()
                    .hardness_of(block)
                    .map(|seconds| seconds / (self.break_factor * tool));
                (Some(hit.block), hardness)
            }
            None => (None, None),
        };
        if let Some(cell) = self.dig.tick(dt, cell, hardness) {
            self.break_block_at(cell);
        }
        let (min, max) = body::player_box_bounds(self.camera.eye);
        if self.hooks.listens_pickup() {
            let slots = hotbar_slots(&self.hotbar);
            let view = client_view(&slots, &self.hotbar, self.camera.eye, self.clock);
            let overlapping = self.drops.pending_pickups(min, max);
            let mut refuse = Vec::new();
            for id in overlapping {
                if !self.hooks.allow_pickup(&mut self.world, id, &view) {
                    refuse.push(id);
                }
            }
            if self
                .drops
                .pickup_carried(min, max, &mut self.hotbar, &mut self.bag, &refuse)
            {
                self.hotbar_dirty = true;
                self.bag_dirty = true;
                self.rebuild_drops();
            }
        } else if self
            .drops
            .pickup_carried(min, max, &mut self.hotbar, &mut self.bag, &[])
        {
            self.hotbar_dirty = true;
            self.bag_dirty = true;
            self.rebuild_drops();
        }
    }

    fn try_craft(&mut self) {
        let Some((result, inputs)) = self.gui.craft_job() else {
            return;
        };
        if hotbar::craft_carried(&mut self.hotbar, &mut self.bag, &inputs, &result) {
            self.hotbar_dirty = true;
            self.bag_dirty = true;
        }
    }

    fn take_bag_slot(&mut self) {
        let Some(id) = self.bag.take_one(self.gui.bag_selected()) else {
            return;
        };
        self.bag_dirty = true;
        if self.hotbar.collect(id) {
            self.hotbar_dirty = true;
            return;
        }
        self.bag.collect(id);
    }

    fn stash_from_hotbar(&mut self, digit: u8) {
        let index = (digit - 1) as usize;
        self.hotbar.select_digit(digit);
        let Some(id) = self.hotbar.slot(index) else {
            return;
        };
        let id = id.to_string();
        if !self.bag.room_for(&id) {
            return;
        }
        self.hotbar.consume_at(index);
        self.bag.collect(&id);
        self.hotbar_dirty = true;
        self.bag_dirty = true;
    }

    fn flush_hotbar(&mut self) {
        if !self.hotbar_dirty || self.told_disconnect {
            return;
        }
        let slots = hotbar_slots(&self.hotbar);
        let selected = self.hotbar.selected() as i32 + 1;
        if self.link.send(encode_hotbar(selected, &slots)).is_err() {
            self.note_disconnect();
            return;
        }
        self.hotbar_dirty = false;
    }

    fn flush_bag(&mut self) {
        if !self.bag_dirty || self.told_disconnect {
            return;
        }
        let slots = self.bag.wire();
        if self.link.send(encode_bag(&slots)).is_err() {
            self.note_disconnect();
            return;
        }
        self.bag_dirty = false;
    }

    pub fn render(&mut self) {
        if !self.configured {
            return;
        }
        let Some(depth_view) = self.depth.as_ref().map(|depth| depth.view.clone()) else {
            return;
        };
        // Item quads point at the eye. The last break's mesh is edge-on once the player moves.
        self.rebuild_drops();

        let aspect = self.size.width as f32 / self.size.height as f32;
        let base = crate::camera::look_fov(self.spyglass_zoomed());
        let bob_scale = self.menu.bob().scale();
        let water_fov = (self.water_sub + self.splash_kick * 0.35).min(1.0);
        let chew = if self.sipping {
            0.0
        } else {
            crate::camera::chew_fov(self.sip, self.chew_phase) * bob_scale
        };
        let jump = self.jump_kick * crate::camera::JUMP_FOV * bob_scale;
        let place = self.slot_punch * crate::camera::PLACE_FOV * bob_scale;
        let swap = self.swap_kick * crate::camera::SWAP_FOV * bob_scale;
        let broken = self.break_kick * crate::camera::BREAK_FOV * bob_scale;
        let fov = crate::camera::punched_fov(
            base,
            self.sprint_kick * bob_scale,
            self.hit_kick * bob_scale,
            water_fov,
            self.boat_kick
                .max(self.cart_kick)
                .max(self.horse_kick)
                * bob_scale,
        ) + chew
            + jump
            + swap
            - place
            - broken;
        let (side, rise) =
            crate::camera::head_bob(self.bob_phase, self.bob_amount * bob_scale);
        let chew_bob = if self.sipping {
            0.0
        } else {
            crate::camera::chew_rise(self.sip, self.chew_phase) * bob_scale
        };
        let mut view_eye = self.camera.bobbed_eye(
            side,
            rise + chew_bob
                - crate::camera::nibble(self.sip)
                - crate::camera::crouch_drop(self.crouch),
        );
        if self.third_person {
            view_eye -= self.camera.forward() * 3.4;
        }
        self.rebuild_player();
        let matrix = self.camera.view_from(aspect, fov, view_eye).to_cols_array();
        let light = self.sun_light();
        let tiles = crate::texture::atlas_tile_count();
        let params = [tiles, light.brightness, light.shadow, 0.0];
        let sun = [
            light.direction[0],
            light.direction[1],
            light.direction[2],
            0.0,
        ];
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytes_of(&matrix));
        self.queue
            .write_buffer(&self.uniform_buf, 64, bytes_of(&params));
        self.queue
            .write_buffer(&self.uniform_buf, 80, bytes_of(&sun));
        let eye = self.camera.eye.to_array();
        let trace_on = self.menu.trace();
        if trace_on {
            self.trace.advance(&self.world, eye, light.direction);
            let bytes = self.trace.upload();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.trace_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(crate::trace::GRID_Y as u32),
                },
                wgpu::Extent3d {
                    width: crate::trace::GRID_X as u32,
                    height: crate::trace::GRID_Y as u32,
                    depth_or_array_layers: crate::trace::GRID_Z as u32,
                },
            );
        }
        let sample = self.trace.sample_origin();
        let trace_box = [sample[0], sample[1], sample[2], crate::trace::SPACING];
        self.queue
            .write_buffer(&self.uniform_buf, 384, bytes_of(&trace_box));
        let blend = if trace_on { 1.0 } else { 0.0 };
        let shadow = [CASCADE_MAP as f32, CASCADE_COUNT as f32, 0.75, blend];
        self.queue
            .write_buffer(&self.uniform_buf, 96, bytes_of(&shadow));
        for index in 0..CASCADE_COUNT {
            let matrix = cascade_view_proj(eye, light.direction, index);
            let info = [
                cascade_texel(index),
                cascade_span(index),
                CASCADE_HALF[index],
                0.0,
            ];
            self.queue.write_buffer(
                &self.uniform_buf,
                112 + index as u64 * 64,
                bytes_of(&matrix),
            );
            self.queue
                .write_buffer(&self.uniform_buf, 304 + index as u64 * 16, bytes_of(&info));
        }
        let time = self.clock.time_of_day();
        let mood = crate::daylight::fog_mood(time);
        let night = crate::daylight::night_fog(time);
        let (mut fog_start, mut fog_end) = crate::daylight::fog_span(light.brightness, time);
        let tint = rebuild_world::sky_tint(rebuild_world::biome_at(
            self.camera.eye.x.floor() as i32,
            self.camera.eye.z.floor() as i32,
        ));
        let mut sky_rgb = crate::daylight::foggy(
            crate::daylight::foggy(
                [
                    light.sky[0] * tint[0],
                    light.sky[1] * tint[1],
                    light.sky[2] * tint[2],
                ],
                [0.55, 0.58, 0.62],
                mood,
            ),
            [0.012, 0.016, 0.04],
            night * 0.75,
        );
        let mut horizon = crate::daylight::foggy(
            crate::daylight::foggy(
                [
                    light.horizon[0] * tint[0],
                    light.horizon[1] * tint[1],
                    light.horizon[2] * tint[2],
                ],
                [0.68, 0.70, 0.72],
                mood,
            ),
            [0.012, 0.016, 0.04],
            night * 0.85,
        );
        // Underwater: blue-green tint and a short fog so the pool reads as submerged.
        let sub = self.water_sub.clamp(0.0, 1.0);
        if sub > 0.0 {
            let water_sky = [0.04, 0.22, 0.32];
            let water_fog = [0.06, 0.28, 0.38];
            for i in 0..3 {
                sky_rgb[i] = sky_rgb[i] * (1.0 - sub) + water_sky[i] * sub;
                horizon[i] = horizon[i] * (1.0 - sub) + water_fog[i] * sub;
            }
            fog_start = fog_start * (1.0 - sub) + 1.5 * sub;
            fog_end = fog_end * (1.0 - sub) + 14.0 * sub;
        }
        let fog_eye = [eye[0], eye[1], eye[2], fog_start];
        let fog_color = [horizon[0], horizon[1], horizon[2], fog_end];
        self.queue
            .write_buffer(&self.uniform_buf, 352, bytes_of(&fog_eye));
        self.queue
            .write_buffer(&self.uniform_buf, 368, bytes_of(&fog_color));

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                drop(texture);
                self.configure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => return,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                match self.instance.create_surface(self.window.clone()) {
                    Ok(surface) => {
                        self.surface = surface;
                        self.configure_surface();
                    }
                    Err(err) => eprintln!("surface lost: {err}"),
                }
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let color_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.view_format),
                ..Default::default()
            });
        if self.frame.is_some() {
            self.prepare_frame_target();
        }
        let world_view = self
            .frame
            .as_ref()
            .and_then(|frame| frame.target.as_ref())
            .map(|target| target.view.clone())
            .unwrap_or_else(|| color_view.clone());

        let held = self.hotbar.slot(self.hotbar.selected());
        let outline = crate::outline::selection_lines(
            self.camera.eye,
            self.camera.forward(),
            &self.world,
            held,
        );
        let outline_count = outline.len().min(OUTLINE_VERTEX_CAP as usize) & !1usize;
        if outline_count >= 2 {
            self.queue.write_buffer(
                &self.outline_buf,
                0,
                bytemuck::cast_slice(&outline[..outline_count]),
            );
        }
        let dig_progress = self.dig.progress();
        let dig_faces = match self.dig.target() {
            Some(cell) if dig_progress > 0.0 => crate::dig::break_faces(cell, &self.world),
            _ => Vec::new(),
        };
        let dig_count = dig_faces.len().min(DIG_VERTEX_CAP as usize);
        if dig_count >= 3 {
            self.queue.write_buffer(
                &self.dig_buf,
                0,
                bytemuck::cast_slice(&dig_faces[..dig_count]),
            );
        }
        let sky = sky_discs(
            self.camera.eye,
            self.clock.time_of_day(),
            self.clock.season(),
            self.menu.hemisphere(),
        );
        let sky_count = sky.len().min(SKY_VERTEX_CAP as usize);
        if sky_count >= 3 {
            self.queue
                .write_buffer(&self.sky_buf, 0, bytemuck::cast_slice(&sky[..sky_count]));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        for (index, layer) in self.shadow_layers.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-map"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: layer,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipelines[index]);
            pass.set_bind_group(0, &self.shadow_bind, &[]);
            Self::draw_chunks(&self.chunk_draws, &mut pass);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("chunk"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &world_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_rgb[0] as f64,
                            g: sky_rgb[1] as f64,
                            b: sky_rgb[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if sky_count >= 3 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.sky_buf.slice(..));
                pass.draw(0..sky_count as u32, 0..1);
            }
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            Self::draw_chunks(&self.chunk_draws, &mut pass);
            if let Some(drops) = &self.drop_buf {
                if self.drop_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, drops.slice(..));
                    pass.draw(0..self.drop_count, 0..1);
                }
            }
            if let Some(peers) = &self.peer_buf {
                if self.peer_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, peers.slice(..));
                    pass.draw(0..self.peer_count, 0..1);
                }
            }
            if let Some(player) = &self.player_buf {
                if self.player_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, player.slice(..));
                    pass.draw(0..self.player_count, 0..1);
                }
            }
            if let Some(sheep) = &self.sheep_buf {
                if self.sheep_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, sheep.slice(..));
                    pass.draw(0..self.sheep_count, 0..1);
                }
            }
            if let Some(pig) = &self.pig_buf {
                if self.pig_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, pig.slice(..));
                    pass.draw(0..self.pig_count, 0..1);
                }
            }
            if let Some(horse) = &self.horse_buf {
                if self.horse_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, horse.slice(..));
                    pass.draw(0..self.horse_count, 0..1);
                }
            }
            if let Some(zombie) = &self.zombie_buf {
                if self.zombie_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, zombie.slice(..));
                    pass.draw(0..self.zombie_count, 0..1);
                }
            }
            if let Some(breeze) = &self.breeze_buf {
                if self.breeze_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, breeze.slice(..));
                    pass.draw(0..self.breeze_count, 0..1);
                }
            }
            if let Some(golem) = &self.golem_buf {
                if self.golem_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, golem.slice(..));
                    pass.draw(0..self.golem_count, 0..1);
                }
            }
            if let Some(wolf) = &self.wolf_buf {
                if self.wolf_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, wolf.slice(..));
                    pass.draw(0..self.wolf_count, 0..1);
                }
            }
            if let Some(cat) = &self.cat_buf {
                if self.cat_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, cat.slice(..));
                    pass.draw(0..self.cat_count, 0..1);
                }
            }
            if let Some(parrot) = &self.parrot_buf {
                if self.parrot_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, parrot.slice(..));
                    pass.draw(0..self.parrot_count, 0..1);
                }
            }
            if let Some(frog) = &self.frog_buf {
                if self.frog_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, frog.slice(..));
                    pass.draw(0..self.frog_count, 0..1);
                }
            }
            if let Some(axolotl) = &self.axolotl_buf {
                if self.axolotl_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, axolotl.slice(..));
                    pass.draw(0..self.axolotl_count, 0..1);
                }
            }
            if let Some(chicken) = &self.chicken_buf {
                if self.chicken_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, chicken.slice(..));
                    pass.draw(0..self.chicken_count, 0..1);
                }
            }
            if let Some(bee) = &self.bee_buf {
                if self.bee_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, bee.slice(..));
                    pass.draw(0..self.bee_count, 0..1);
                }
            }
            if let Some(fox) = &self.fox_buf {
                if self.fox_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, fox.slice(..));
                    pass.draw(0..self.fox_count, 0..1);
                }
            }
            if let Some(goat) = &self.goat_buf {
                if self.goat_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, goat.slice(..));
                    pass.draw(0..self.goat_count, 0..1);
                }
            }
            if let Some(guests) = &self.guest_buf {
                if self.guest_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, guests.slice(..));
                    pass.draw(0..self.guest_count, 0..1);
                }
            }
            if let Some(boat) = &self.boat_buf {
                if self.boat_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, boat.slice(..));
                    pass.draw(0..self.boat_count, 0..1);
                }
            }
            if let Some(cart) = &self.cart_buf {
                if self.cart_count >= 3 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, cart.slice(..));
                    pass.draw(0..self.cart_count, 0..1);
                }
            }
            if dig_count >= 3 {
                let kept = f64::from(crate::dig::crack_light(dig_progress));
                pass.set_pipeline(&self.dig_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_blend_constant(wgpu::Color {
                    r: kept,
                    g: kept,
                    b: kept,
                    a: 1.0,
                });
                pass.set_vertex_buffer(0, self.dig_buf.slice(..));
                pass.draw(0..dig_count as u32, 0..1);
            }
            if outline_count >= 2 {
                pass.set_pipeline(&self.outline_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.outline_buf.slice(..));
                pass.draw(0..outline_count as u32, 0..1);
            }
        }
        if let Some(frame) = &self.frame {
            if let Some(target) = &frame.target {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame-shader"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&frame.pipeline);
                pass.set_bind_group(0, &target.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        let screen_w = self.size.width as f32;
        let screen_h = self.size.height as f32;
        let (gui_w, gui_h) = gui::gui_space(screen_w, screen_h);
        let mut quads = gui::feel_quads(
            screen_w,
            screen_h,
            &self.hotbar,
            &self.bag,
            &self.gui,
            &self.menu,
            self.hearts,
            self.hunger.food(),
            self.armor.worn(),
            self.slot_punch,
            self.hurt_flash,
            self.sip,
            self.sipping,
            self.dead,
        );
        if self.crouch > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::crouch_quads(gui_w, gui_h, self.crouch));
        }
        if self.sprint_show > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::sprint_quads(gui_w, gui_h, self.sprint_show));
        }
        if self.boat_show > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::boat_quads(gui_w, gui_h, self.boat_show));
        }
        if self.cart_show > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::cart_quads(gui_w, gui_h, self.cart_show));
        }
        if self.horse_show > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::horse_quads(gui_w, gui_h, self.horse_show));
        }
        if self.sip > 0.02 && !self.sipping && !self.menu.is_open() && !self.dead {
            quads.extend(gui::chew_quads(gui_w, gui_h, self.sip));
        }
        if self.water_sub > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::water_quads(gui_w, gui_h, self.water_sub));
        }
        let hunger_veil = self.hunger.hunger_veil();
        if hunger_veil > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::starve_quads(gui_w, gui_h, hunger_veil));
        }
        if self.sleep_fade > 0.02 && !self.menu.is_open() && !self.dead {
            quads.extend(gui::sleep_quads(gui_w, gui_h, self.sleep_fade));
        }
        if !self.menu.is_open() && !self.dead && !self.screen_open() {
            let looked = self.inspect_id();
            let held = self.hotbar.slot(self.hotbar.selected());
            self.shape_pick.clamp_to(held);
            let held_override = held.and_then(|id| self.shape_pick.held_label(id));
            let attack_ready = if self.holding_mace() {
                (1.0 - self.mace_wait / body::MACE_WAIT).clamp(0.0, 1.0)
            } else {
                1.0
            };
            quads.extend(gui::look_quads(
                gui_w,
                gui_h,
                looked.as_deref(),
                held,
                held_override.as_deref(),
                attack_ready,
            ));
            if self.shape_pick.open {
                if let Some(held) = held {
                    let options = crate::shape_pick::ShapePick::options(held);
                    if options.len() > 1 {
                        let labels: Vec<&str> =
                            options.iter().map(|shape| shape.label()).collect();
                        let selected = options
                            .iter()
                            .position(|shape| *shape == self.shape_pick.shape)
                            .unwrap_or(0);
                        quads.extend(gui::shape_picker_quads(gui_w, gui_h, &labels, selected));
                    }
                }
            }
        }
        if self.holding_compass() && !self.menu.is_open() {
            let (yaw, _) = self.camera.yaw_pitch();
            let mark = if let Some(at) = self.lodestone {
                crate::camera::compass_mark_at(
                    self.camera.eye.x,
                    self.camera.eye.z,
                    yaw,
                    [at[0] as f32 + 0.5, at[2] as f32 + 0.5],
                )
            } else {
                crate::camera::compass_mark(self.camera.eye.x, self.camera.eye.z, yaw)
            };
            quads.extend(gui::compass_quads(gui_w, gui_h, mark));
        }
        if self.holding_clock() && !self.menu.is_open() {
            let mark = crate::daylight::clock_mark(self.clock.time_of_day());
            quads.extend(gui::clock_quads(gui_w, gui_h, mark));
        }
        if self.map_open && !self.holding_map() {
            self.map_open = false;
            self.sync_pointer();
        }
        if self.map_open && !self.menu.is_open() {
            let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
            let zoom = self.hotbar.map_zoom();
            let colors = crate::map::sample_span(&self.world, eye, zoom, crate::map::SHEET);
            let (yaw, _) = self.camera.yaw_pitch();
            let camp = crate::map::cell_of(
                eye[0],
                eye[2],
                zoom,
                crate::map::SHEET,
                rebuild_world::CAMP_FIRE.0,
                rebuild_world::CAMP_FIRE.1,
            );
            let mine = crate::map::cell_of(
                eye[0],
                eye[2],
                zoom,
                crate::map::SHEET,
                rebuild_world::SHAFT_MOUTH.0,
                rebuild_world::SHAFT_MOUTH.1,
            );
            let stairs = crate::map::cell_of(
                eye[0],
                eye[2],
                zoom,
                crate::map::SHEET,
                crate::camera::STAIRS_XZ[0].floor() as i32,
                crate::camera::STAIRS_XZ[1].floor() as i32,
            );
            let shrine = crate::map::cell_of(
                eye[0],
                eye[2],
                zoom,
                crate::map::SHEET,
                rebuild_world::SHRINE.0,
                rebuild_world::SHRINE.1,
            );
            let clay = crate::map::cell_of(
                eye[0],
                eye[2],
                zoom,
                crate::map::SHEET,
                rebuild_world::CLAY_BANK.0,
                rebuild_world::CLAY_BANK.1,
            );
            quads.extend(crate::map::sheet_quads(
                gui_w, gui_h, &colors, yaw, camp, mine, stairs, shrine, clay,
            ));
        } else if self.holding_map() && !self.menu.is_open() {
            let eye = [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z];
            let colors = crate::map::sample_at(&self.world, eye, self.hotbar.map_zoom());
            let (yaw, _) = self.camera.yaw_pitch();
            quads.extend(crate::map::quads(gui_w, gui_h, &colors, yaw));
        }
        if self.gui.is_open() && !self.menu.is_open() && !self.dead {
            let hit = self.pointer_hit();
            if let Some(tip) = gui::hover_tip(&self.gui, &self.hotbar, &self.bag, hit) {
                quads.extend(gui::tooltip_quads(
                    gui_w,
                    gui_h,
                    self.cursor_x / gui::GUI_SCALE,
                    self.cursor_y / gui::GUI_SCALE,
                    &tip.name,
                    &tip.line,
                ));
            }
        }
        if self.menu.is_open() || self.gui.is_open() || self.dead {
            quads.extend(gui::cursor_quads(
                gui_w,
                gui_h,
                self.cursor_x / gui::GUI_SCALE,
                self.cursor_y / gui::GUI_SCALE,
            ));
        }
        let max_quads = (UI_VERTEX_CAP as usize / 4).min(UI_INDEX_CAP as usize / 6);
        if quads.len() > max_quads {
            quads.truncate(max_quads);
        }
        let (ui_vertices, ui_indices) = ui_mesh(&quads);
        if !ui_indices.is_empty() {
            self.queue
                .write_buffer(&self.ui_vertex_buf, 0, bytemuck::cast_slice(&ui_vertices));
            self.queue
                .write_buffer(&self.ui_index_buf, 0, bytemuck::cast_slice(&ui_indices));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hotbar"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_bind_group(0, &self.ui_bind, &[]);
            pass.set_vertex_buffer(0, self.ui_vertex_buf.slice(..));
            pass.set_index_buffer(self.ui_index_buf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..ui_indices.len() as u32, 0, 0..1);
        }

        if crate::egui_ui::EguiHost::wants_input(&self.menu, &self.gui) {
            let full = self.egui.run(
                &self.window,
                &mut self.menu,
                &mut self.gui,
                &mut self.hotbar,
                &mut self.bag,
                &mut self.armor,
                &mut self.binds,
            );
            let action = self.egui.take_action();
            let touched = self.egui.take_touched();
            self.egui.paint(
                &self.device,
                &self.queue,
                &mut encoder,
                &color_view,
                self.size.width,
                self.size.height,
                full,
            );
            if action != gui::MenuAction::None {
                self.hotbar_dirty = true;
                self.bag_dirty = true;
                self.apply_menu(action);
            } else if touched {
                self.hotbar_dirty = true;
                self.bag_dirty = true;
            }
        }

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
        self.poll_zombie_shot();
        self.poll_settings_shot();
        self.poll_keybinds_shot();
        self.poll_creative_shot();
        self.poll_held_light_shot();
        self.poll_night_spawn_shot();
        self.poll_bed_sleep_shot();
        self.poll_crouch_shot();
        self.poll_sprint_shot();
        self.poll_swim_shot();
        self.poll_boat_shot();
        self.poll_cart_shot();
        self.poll_horse_shot();
        self.poll_attack_shot();
        self.poll_hunger_shot();
        self.poll_eat_shot();
        self.poll_jump_shot();
        self.poll_place_shot();
        self.poll_campfire_smoke_shot();
        self.poll_cook_shot();
        self.poll_shrine_shot();
        self.poll_mine_shot();
        self.poll_swap_shot();
        self.poll_break_shot();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeldLightShotPhase {
    Idle,
    WaitingDark,
    WaitingLit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SprintShotPhase {
    Idle,
    WaitingWalk,
    WaitingSprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwimShotPhase {
    Idle,
    WaitingDry,
    WaitingWet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoatShotPhase {
    Idle,
    WaitingIdle,
    WaitingRow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CartShotPhase {
    Idle,
    WaitingIdle,
    WaitingRide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HorseShotPhase {
    Idle,
    WaitingIdle,
    WaitingRide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttackShotPhase {
    Idle,
    WaitingCool,
    WaitingReady,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HungerShotPhase {
    Idle,
    WaitingFed,
    WaitingStarved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EatShotPhase {
    Idle,
    WaitingIdle,
    WaitingChew,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JumpShotPhase {
    Idle,
    WaitingGround,
    WaitingAir,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaceShotPhase {
    Idle,
    WaitingIdle,
    WaitingPlace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwapShotPhase {
    Idle,
    WaitingIdle,
    WaitingSwap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreakShotPhase {
    Idle,
    WaitingIdle,
    WaitingBreak,
}

struct CapturedRgba {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn write_rgba_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Dirt (left) vs torch (right) so the LightField difference is obvious.
fn stitch_held_light_proof(
    dark: &CapturedRgba,
    lit: &CapturedRgba,
    path: &std::path::Path,
) -> Result<(), String> {
    if dark.width != lit.width || dark.height != lit.height {
        return Err(format!(
            "size mismatch dark {}x{} lit {}x{}",
            dark.width, dark.height, lit.width, lit.height
        ));
    }
    let gap = 8u32;
    let out_w = dark.width * 2 + gap;
    let out_h = dark.height;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..dark.height {
        let src_row = (y * dark.width * 4) as usize;
        let dark_dst = (y * out_w * 4) as usize;
        let lit_dst = dark_dst + ((dark.width + gap) * 4) as usize;
        let row = (dark.width * 4) as usize;
        out[dark_dst..dark_dst + row].copy_from_slice(&dark.rgba[src_row..src_row + row]);
        out[lit_dst..lit_dst + row].copy_from_slice(&lit.rgba[src_row..src_row + row]);
        // Warm gap so the seam does not read as world light.
        for x in 0..gap {
            let i = dark_dst + ((dark.width + x) * 4) as usize;
            out[i] = 40;
            out[i + 1] = 28;
            out[i + 2] = 18;
            out[i + 3] = 255;
        }
    }
    write_rgba_png(path, out_w, out_h, &out)
}

#[cfg(windows)]
fn capture_window_rgba(window: &Window) -> Result<CapturedRgba, String> {
    capture_window_rgba_impl(window)
}

#[cfg(windows)]
fn capture_window_png(window: &Window, path: &std::path::Path) -> Result<(), String> {
    let shot = capture_window_rgba(window)?;
    write_rgba_png(path, shot.width, shot.height, &shot.rgba)
}

#[cfg(windows)]
fn capture_window_rgba_impl(window: &Window) -> Result<CapturedRgba, String> {
    use std::ptr;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(hwnd: *mut core::ffi::c_void, rect: *mut [i32; 4]) -> i32;
        fn ClientToScreen(hwnd: *mut core::ffi::c_void, point: *mut [i32; 2]) -> i32;
        fn GetDC(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn ReleaseDC(hwnd: *mut core::ffi::c_void, hdc: *mut core::ffi::c_void) -> i32;
        fn SetForegroundWindow(hwnd: *mut core::ffi::c_void) -> i32;
        fn ShowWindow(hwnd: *mut core::ffi::c_void, cmd: i32) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn CreateCompatibleBitmap(
            hdc: *mut core::ffi::c_void,
            width: i32,
            height: i32,
        ) -> *mut core::ffi::c_void;
        fn SelectObject(
            hdc: *mut core::ffi::c_void,
            obj: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn BitBlt(
            dst: *mut core::ffi::c_void,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            src: *mut core::ffi::c_void,
            sx: i32,
            sy: i32,
            rop: u32,
        ) -> i32;
        fn GetDIBits(
            hdc: *mut core::ffi::c_void,
            bitmap: *mut core::ffi::c_void,
            start: u32,
            lines: u32,
            bits: *mut u8,
            info: *mut BITMAPINFO,
            usage: u32,
        ) -> i32;
        fn DeleteObject(obj: *mut core::ffi::c_void) -> i32;
        fn DeleteDC(hdc: *mut core::ffi::c_void) -> i32;
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_pels: i32,
        y_pels: i32,
        clr_used: u32,
        clr_important: u32,
    }
    #[repr(C)]
    struct BITMAPINFO {
        header: BITMAPINFOHEADER,
        colors: [u32; 1],
    }

    let handle = window
        .window_handle()
        .map_err(|err| format!("window handle: {err}"))?;
    let hwnd = match handle.as_raw() {
        RawWindowHandle::Win32(win) => win.hwnd.get() as *mut core::ffi::c_void,
        _ => return Err("not a Win32 window".into()),
    };
    if hwnd.is_null() {
        return Err("null hwnd".into());
    }
    unsafe {
        ShowWindow(hwnd, 9); // SW_RESTORE
        SetForegroundWindow(hwnd);
    }
    // Let the DXGI present land on screen before we blit.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let mut rect = [0i32; 4];
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return Err("GetClientRect failed".into());
    }
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    if width <= 8 || height <= 8 {
        return Err(format!("client too small {width}x{height}"));
    }
    let mut origin = [0i32; 2];
    if unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
        return Err("ClientToScreen failed".into());
    }
    // Blit the presented swapchain from the screen DC (PrintWindow often misses wgpu).
    let screen = unsafe { GetDC(ptr::null_mut()) };
    if screen.is_null() {
        return Err("GetDC failed".into());
    }
    let mem = unsafe { CreateCompatibleDC(screen) };
    let bmp = unsafe { CreateCompatibleBitmap(screen, width, height) };
    if mem.is_null() || bmp.is_null() {
        unsafe {
            if !bmp.is_null() {
                DeleteObject(bmp);
            }
            if !mem.is_null() {
                DeleteDC(mem);
            }
            ReleaseDC(ptr::null_mut(), screen);
        }
        return Err("GDI alloc failed".into());
    }
    unsafe { SelectObject(mem, bmp) };
    const SRCCOPY: u32 = 0x00CC_0020;
    if unsafe { BitBlt(mem, 0, 0, width, height, screen, origin[0], origin[1], SRCCOPY) } == 0 {
        unsafe {
            DeleteObject(bmp);
            DeleteDC(mem);
            ReleaseDC(ptr::null_mut(), screen);
        }
        return Err("BitBlt failed".into());
    }
    let mut info = BITMAPINFO {
        header: BITMAPINFOHEADER {
            size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            width,
            height: -height,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_pels: 0,
            y_pels: 0,
            clr_used: 0,
            clr_important: 0,
        },
        colors: [0],
    };
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    let got = unsafe {
        GetDIBits(
            mem,
            bmp,
            0,
            height as u32,
            bgra.as_mut_ptr(),
            &mut info,
            0,
        )
    };
    unsafe {
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(ptr::null_mut(), screen);
    }
    if got == 0 {
        return Err("GetDIBits failed".into());
    }
    let mut rgba = vec![0u8; bgra.len()];
    for (dst, src) in rgba.chunks_exact_mut(4).zip(bgra.chunks_exact(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
    // Reject empty/black captures so we keep retrying.
    let lit = rgba
        .chunks(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count();
    if lit < 200 {
        return Err(format!("capture too dark ({lit} lit px)"));
    }
    let _ = ptr::null::<()>();
    Ok(CapturedRgba {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

#[cfg(not(windows))]
fn capture_window_rgba(_window: &Window) -> Result<CapturedRgba, String> {
    Err("window shot is Windows-only".into())
}

#[cfg(not(windows))]
fn capture_window_png(_window: &Window, _path: &std::path::Path) -> Result<(), String> {
    Err("window shot is Windows-only".into())
}

fn eye_cell(eye: glam::Vec3) -> [i32; 3] {
    [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ]
}

fn client_view<'a>(
    slots: &'a [(String, i32)],
    hotbar: &hotbar::Hotbar,
    eye: glam::Vec3,
    clock: crate::daylight::Clock,
) -> ScriptView<'a> {
    ScriptView {
        slots,
        selected: hotbar.selected() as i32 + 1,
        eye: Some(eye.to_array()),
        time: Some(clock.time_of_day()),
        season: clock.season().label(),
    }
}

fn hotbar_slots(hotbar: &hotbar::Hotbar) -> Vec<(String, i32)> {
    (0..hotbar::SLOTS)
        .map(|index| {
            (
                hotbar.slot(index).unwrap_or("").to_string(),
                i32::from(hotbar.count(index)),
            )
        })
        .collect()
}

fn ui_mesh(quads: &[hotbar::Quad]) -> (Vec<UiVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut indices = Vec::with_capacity(quads.len() * 6);
    for quad in quads {
        let base = vertices.len() as u32;
        let [x0, y0] = quad.min;
        let [x1, y1] = quad.max;
        let color = quad.color;
        let [su, sv] = quad.span;
        let tex = quad.tex;
        vertices.extend([
            UiVertex {
                position: [x0, y0],
                color,
                uv: [0.0, 0.0],
                tex,
            },
            UiVertex {
                position: [x1, y0],
                color,
                uv: [su, 0.0],
                tex,
            },
            UiVertex {
                position: [x1, y1],
                color,
                uv: [su, sv],
                tex,
            },
            UiVertex {
                position: [x0, y1],
                color,
                uv: [0.0, sv],
                tex,
            },
        ]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// Sun and moon discs. A halo sits just behind a smaller core. Below the
/// horizon the body is left out, so it does not poke through the ground.
fn load_player_hooks() -> Result<Hooks, String> {
    let mut hooks = Hooks::new();
    let dir = Path::new(r"C:\CODE\TerraLite\mods");
    for (path, found) in load_dir(dir, &mut hooks)? {
        for hook in found {
            println!(
                "rosegold: {} {} \"{}\"",
                path.display(),
                hook.kind(),
                hook.id()
            );
        }
    }
    Ok(hooks)
}

struct PlateHold {
    at: [i32; 3],
    opened: Option<[i32; 3]>,
}

enum PotPlant {
    /// The pot already holds a flower.
    Full,
    /// A held flower can be planted.
    Plant,
    /// The pot is empty and the hand is not a flower.
    NeedFlower,
}

enum PumpkinCarve {
    /// Change the pumpkin. A torch is spent when `spend_torch` is set.
    Set {
        metadata: u8,
        spend_torch: bool,
        word: &'static str,
    },
    /// The face is already in this state.
    Say(&'static str),
}

/// A bare hand carves a face. A held torch lights that face.
fn pumpkin_carve(metadata: u8, holding_torch: bool) -> PumpkinCarve {
    if metadata >= 2 {
        return PumpkinCarve::Say("lit");
    }
    if holding_torch {
        return PumpkinCarve::Set {
            metadata: 2,
            spend_torch: true,
            word: "lit",
        };
    }
    if metadata == 0 {
        return PumpkinCarve::Set {
            metadata: 1,
            spend_torch: false,
            word: "carved",
        };
    }
    PumpkinCarve::Say("carved")
}

/// Setting an eye spends a held eye of ender. A filled frame stays filled.
enum FrameEye {
    Full,
    NeedEye,
    Set,
}

fn frame_eye(metadata: u8, holding_eye: bool) -> FrameEye {
    if metadata & 1 == 1 {
        FrameEye::Full
    } else if holding_eye {
        FrameEye::Set
    } else {
        FrameEye::NeedEye
    }
}

/// Filled frames beside this one. Two or more is the start of a ring, not a portal.
fn filled_beside(world: &Cluster, at: [i32; 3]) -> u8 {
    let mut count = 0u8;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let block = world.block_at(at[0] + dx, at[1], at[2] + dz);
        if block.id == "end_portal_frame" && block.metadata & 1 == 1 {
            count += 1;
        }
    }
    count
}

fn ring_word(filled_beside: u8) -> &'static str {
    if filled_beside >= 2 {
        "the ring stirs"
    } else {
        "the eye sets"
    }
}

/// Planting spends a held flower. A full pot stays full.
fn pot_plant(metadata: u8, holding_flower: bool) -> PotPlant {
    if metadata & 1 == 1 {
        PotPlant::Full
    } else if holding_flower {
        PotPlant::Plant
    } else {
        PotPlant::NeedFlower
    }
}

/// Dominant look axis → pump facing bits (`0..=5`).
fn pump_facing_from_look(forward: glam::Vec3) -> u8 {
    let ax = forward.x.abs();
    let ay = forward.y.abs();
    let az = forward.z.abs();
    if ax >= ay && ax >= az {
        if forward.x >= 0.0 {
            0
        } else {
            1
        }
    } else if ay >= ax && ay >= az {
        if forward.y >= 0.0 {
            2
        } else {
            3
        }
    } else if forward.z >= 0.0 {
        4
    } else {
        5
    }
}

/// Open the neighbor when the lever turns on, and close it when the lever turns off.
fn lever_wants(turning_on: bool, neighbor_open: bool) -> Option<bool> {
    if turning_on && !neighbor_open {
        Some(true)
    } else if !turning_on && neighbor_open {
        Some(false)
    } else {
        None
    }
}

/// The plate or tripwire the body is in. Standing on a plate or walking
/// through a string both count, so the same pulse can open a neighbor.
fn plate_under(world: &Cluster, eye: glam::Vec3) -> Option<[i32; 3]> {
    let feet = eye.y - crate::body::EYE_HEIGHT;
    let x = eye.x.floor() as i32;
    let z = eye.z.floor() as i32;
    let y0 = feet.floor() as i32;
    let y1 = (feet + crate::body::HEIGHT - 1.0e-3).floor() as i32;
    let mut y = y0;
    while y <= y1 {
        if (0..256).contains(&y) {
            let id = world.block_at(x, y, z).id;
            if id == "plate" || id == "tripwire" {
                return Some([x, y, z]);
            }
        }
        y += 1;
    }
    None
}

/// A block that swings open the way a door does.
fn shelf_word(count: u8) -> &'static str {
    match count {
        0 => "empty",
        1 => "1 book",
        2 => "2 books",
        3 => "3 books",
        4 => "4 books",
        5 => "5 books",
        _ => "6 books",
    }
}

fn hinge(id: &str) -> bool {
    matches!(
        id,
        "door" | "copper_door" | "trapdoor" | "copper_trapdoor" | "fence_gate"
    )
}

/// Sand, gravel, and the other soft blocks a brush can sweep.
fn brushable(id: &str) -> bool {
    matches!(
        id,
        "sand" | "gravel" | "soul_sand" | "dirt" | "clay" | "grass" | "decorated_pot"
    )
}

fn already_brushed(id: &str, metadata: u8) -> bool {
    if id == "decorated_pot" {
        metadata & 4 != 0
    } else {
        metadata & 1 != 0
    }
}

fn brush_mark(id: &str, metadata: u8) -> u8 {
    if id == "decorated_pot" {
        metadata | 4
    } else {
        metadata | 1
    }
}

/// One find from this column. The same cell always yields the same item.
fn brush_find(x: i32, z: i32) -> &'static str {
    const FINDS: [&str; 4] = ["pottery_sherd", "stick", "clay", "bone_meal"];
    let n = x
        .unsigned_abs()
        .wrapping_add(z.unsigned_abs().wrapping_mul(3));
    FINDS[(n % 4) as usize]
}

/// A copper chest a trial key has already opened.
fn trial_open(metadata: u8) -> bool {
    metadata & 1 != 0
}

fn trial_mark(metadata: u8) -> u8 {
    metadata | 1
}

/// The camp chest, two blocks south of the campfire.
fn camp_cell(x: i32, z: i32) -> bool {
    (x, z) == rebuild_world::CAMP_CHEST
}

/// The southeast shrine chest.
fn shrine_cell(x: i32, z: i32) -> bool {
    (x, z) == rebuild_world::SHRINE_CHEST
}

/// The west mine-mouth chest on the gravel apron.
fn mine_cell(x: i32, z: i32) -> bool {
    (x, z) == rebuild_world::SHAFT_CHEST
}

/// Dawn/dusk chat tips only while the world is live (not menu / death).
fn sky_edge_tip_open(playing: bool, menu_open: bool, dead: bool) -> bool {
    playing && !menu_open && !dead
}

/// True when the column above the eye is clear of full cubes (~open sky).
fn open_sky(world: &Cluster, eye: [f32; 3]) -> bool {
    let x = eye[0].floor() as i32;
    let z = eye[2].floor() as i32;
    let start = eye[1].floor() as i32 + 1;
    for y in start..(start + 16) {
        if shape_of(world.block_at(x, y, z)).is_full_cube() {
            return false;
        }
    }
    true
}

/// Cave-like cover above the eye. Needs a few solids so thin roofs stay quiet.
fn cave_drip_mix(world: &Cluster, eye: [f32; 3]) -> f32 {
    let x = eye[0].floor() as i32;
    let z = eye[2].floor() as i32;
    let start = eye[1].floor() as i32 + 1;
    let mut solids = 0u32;
    for y in start..(start + 16) {
        if shape_of(world.block_at(x, y, z)).is_full_cube() {
            solids += 1;
        }
    }
    if solids < 3 {
        return 0.0;
    }
    ((solids as f32 - 2.0) / 8.0).clamp(0.35, 1.0)
}

/// Soft proximity to a water surface (air or non-water above). Mix is `0`
/// outside ~10 blocks, rising to `1` at the shore. Returns the surface center.
fn nearest_water_edge(world: &Cluster, eye: [f32; 3]) -> (f32, Option<[f32; 3]>) {
    const REACH: i32 = 8;
    let cx = eye[0].floor() as i32;
    let cy = eye[1].floor() as i32;
    let cz = eye[2].floor() as i32;
    let mut best: Option<(f32, [f32; 3])> = None;
    for z in (cz - REACH)..=(cz + REACH) {
        for x in (cx - REACH)..=(cx + REACH) {
            for y in (cy - 3)..=(cy + 2) {
                if world.block_at(x, y, z).id != "water" {
                    continue;
                }
                // Surface only: something that is not water above the cell.
                if world.block_at(x, y + 1, z).id == "water" {
                    continue;
                }
                let src = [x as f32 + 0.5, y as f32 + 0.85, z as f32 + 0.5];
                let dx = eye[0] - src[0];
                let dy = eye[1] - src[1];
                let dz = eye[2] - src[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                if best.is_none_or(|(so_far, _)| dist < so_far) {
                    best = Some((dist, src));
                }
            }
        }
    }
    match best {
        Some((dist, src)) if dist < 10.0 => {
            let t = ((10.0 - dist) / 10.0).clamp(0.0, 1.0);
            (t * t, Some(src))
        }
        _ => (0.0, None),
    }
}

/// Soft proximity to the nearest campfire. Mix is `0` outside ~12 blocks,
/// rising to `1` when standing over the flame. Returns the fire's center too.
fn nearest_campfire(world: &Cluster, eye: [f32; 3]) -> (f32, Option<[f32; 3]>) {
    const REACH: i32 = 10;
    let cx = eye[0].floor() as i32;
    let cy = eye[1].floor() as i32;
    let cz = eye[2].floor() as i32;
    let mut best: Option<(f32, [f32; 3])> = None;
    for z in (cz - REACH)..=(cz + REACH) {
        for x in (cx - REACH)..=(cx + REACH) {
            for y in (cy - 4)..=(cy + 2) {
                if world.block_at(x, y, z).id != "campfire" {
                    continue;
                }
                let src = [x as f32 + 0.5, y as f32 + 0.35, z as f32 + 0.5];
                let dx = eye[0] - src[0];
                let dy = eye[1] - src[1];
                let dz = eye[2] - src[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                if best.is_none_or(|(so_far, _)| dist < so_far) {
                    best = Some((dist, src));
                }
            }
        }
    }
    match best {
        Some((dist, src)) if dist < 12.0 => {
            let t = ((12.0 - dist) / 12.0).clamp(0.0, 1.0);
            (t * t, Some(src))
        }
        _ => (0.0, None),
    }
}

/// One drop from the camp chest, read from the data pack. Any other cell is
/// `None`. The same cell always yields the same item. `omen` uses the richer
/// list. `None` also means the file could not be read.
fn camp_loot(x: i32, z: i32, omen: bool) -> Option<String> {
    if !camp_cell(x, z) {
        return None;
    }
    match crate::loot::load_camp_chest() {
        Ok(table) => Some(crate::loot::pick(&table, x, z, omen).to_string()),
        Err(err) => {
            eprintln!("loot: {err}");
            None
        }
    }
}

/// The first open of the camp chest. A chest the world file already lists,
/// even an empty one, is left alone.
fn fresh_camp(has_record: bool, x: i32, z: i32, omen: bool) -> Option<String> {
    if has_record {
        return None;
    }
    camp_loot(x, z, omen)
}

/// One drop from the shrine chest, read from the data pack.
fn shrine_loot(x: i32, z: i32, omen: bool) -> Option<String> {
    if !shrine_cell(x, z) {
        return None;
    }
    match crate::loot::load_shrine_chest() {
        Ok(table) => Some(crate::loot::pick(&table, x, z, omen).to_string()),
        Err(err) => {
            eprintln!("loot: {err}");
            None
        }
    }
}

/// The first open of the shrine chest.
fn fresh_shrine(has_record: bool, x: i32, z: i32, omen: bool) -> Option<String> {
    if has_record {
        return None;
    }
    shrine_loot(x, z, omen)
}

/// One drop from the mine chest, read from the data pack.
fn mine_loot(x: i32, z: i32, omen: bool) -> Option<String> {
    if !mine_cell(x, z) {
        return None;
    }
    match crate::loot::load_mine_chest() {
        Ok(table) => Some(crate::loot::pick(&table, x, z, omen).to_string()),
        Err(err) => {
            eprintln!("loot: {err}");
            None
        }
    }
}

/// The first open of the mine-mouth chest.
fn fresh_mine(has_record: bool, x: i32, z: i32, omen: bool) -> Option<String> {
    if has_record {
        return None;
    }
    mine_loot(x, z, omen)
}

/// One drop from this chest, read from the data pack. The same cell always
/// yields the same item. `omen` uses the richer list. `None` means the file
/// could not be read.
fn trial_loot(x: i32, z: i32, omen: bool) -> Option<String> {
    match crate::loot::load_copper_chest() {
        Ok(table) => Some(crate::loot::pick(&table, x, z, omen).to_string()),
        Err(err) => {
            eprintln!("loot: {err}");
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameMove {
    Store,
    Take,
    Full,
    Empty,
}

/// One item in an empty frame, or the shown item back into an empty hand.
fn frame_move(held: Option<&str>, stored: &str, room: bool) -> FrameMove {
    if stored.is_empty() {
        return if held.is_some() {
            FrameMove::Store
        } else {
            FrameMove::Empty
        };
    }
    if held.is_some() {
        return FrameMove::Full;
    }
    if room {
        FrameMove::Take
    } else {
        FrameMove::Full
    }
}

/// What Enter does with one pot stack and the held hotbar slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PotMove {
    Store,
    Take,
    Full,
    Empty,
}

/// What Enter does with the three shelf stacks and the held hotbar slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoardMove {
    Store { slot: usize, count: u16 },
    Take { slot: usize, count: u16 },
    Full,
    Empty,
}

fn board_move(held: Option<&str>, slots: &[(&str, u16); 3], room: bool) -> BoardMove {
    if let Some(item) = held {
        if let Some(slot) = slots
            .iter()
            .position(|(id, count)| *id == item && *count > 0 && *count < 64)
        {
            return BoardMove::Store {
                slot,
                count: slots[slot].1 + 1,
            };
        }
        if let Some(slot) = slots
            .iter()
            .position(|(id, count)| id.is_empty() || *count == 0)
        {
            return BoardMove::Store { slot, count: 1 };
        }
        return BoardMove::Full;
    }
    if let Some(slot) = slots
        .iter()
        .rposition(|(id, count)| !id.is_empty() && *count > 0)
    {
        if !room {
            return BoardMove::Full;
        }
        return BoardMove::Take {
            slot,
            count: slots[slot].1 - 1,
        };
    }
    BoardMove::Empty
}

fn board_wire(slots: &[(String, u16); 3]) -> Vec<(String, i32)> {
    let mut out = vec![(String::new(), 0); BAG_SLOTS];
    for (index, (id, count)) in slots.iter().enumerate() {
        if *count > 0 && !id.is_empty() {
            out[index] = (id.clone(), i32::from(*count));
        }
    }
    out
}

fn pot_move(held: Option<&str>, stored: &str, count: u16, room: bool) -> PotMove {
    if let Some(item) = held {
        if count == 0 || (stored == item && count < 64) {
            return PotMove::Store;
        }
        return PotMove::Full;
    }
    if count == 0 {
        return PotMove::Empty;
    }
    if room { PotMove::Take } else { PotMove::Full }
}

fn pot_wire(id: &str, count: u16) -> Vec<(String, i32)> {
    let mut slots = vec![(String::new(), 0); BAG_SLOTS];
    if count > 0 && !id.is_empty() {
        slots[0] = (id.to_string(), i32::from(count));
    }
    slots
}

/// True when a held arrow is aimed at a target block.
fn arrow_scores(held: &str, looked: &str) -> bool {
    Pack::builtin().is_arrow(held) && looked == "target"
}

/// The door, trapdoor, or fence gate on a face of this cell, if one is there.
fn press_target(world: &Cluster, at: [i32; 3]) -> Option<([i32; 3], Block)> {
    const STEPS: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
    for step in STEPS {
        let cell = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
        let block = world.block_at(cell[0], cell[1], cell[2]);
        if hinge(block.id) {
            return Some((cell, block));
        }
    }
    None
}

fn chunk_of(eye: glam::Vec3) -> [i32; 2] {
    [eye.x.div_euclid(16.0) as i32, eye.z.div_euclid(16.0) as i32]
}

fn command_rows(hooks: &Hooks) -> Vec<(String, String)> {
    hooks
        .script_content()
        .iter()
        .filter(|(kind, _)| kind == "COMMAND")
        .map(|(_, id)| {
            let label: String = id
                .rsplit(':')
                .next()
                .unwrap_or(id)
                .chars()
                .filter(|ch| ch.is_ascii_lowercase())
                .collect();
            let label = if label.is_empty() {
                "command".to_string()
            } else {
                label
            };
            (id.clone(), label)
        })
        .collect()
}

fn particle_color(hooks: &Hooks) -> Option<[f32; 3]> {
    hooks
        .script_content()
        .iter()
        .find(|(kind, _)| kind == "PARTICLE")
        .map(|(_, id)| crate::palette::color(id, 0))
}

fn sky_discs(
    eye: glam::Vec3,
    time: f32,
    season: crate::daylight::Season,
    hemisphere: crate::daylight::Hemisphere,
) -> Vec<mesh::Vertex> {
    let mut out = Vec::new();
    let sun = crate::daylight::sun_on(time, season, hemisphere);
    let moon = crate::daylight::moon_on(time, season, hemisphere);
    push_disc(&mut out, eye, sun, 190.0, 16.0, [1.0, 0.55, 0.18]);
    push_disc(&mut out, eye, sun, 180.0, 8.0, [1.0, 0.95, 0.70]);
    push_disc(&mut out, eye, moon, 190.0, 13.0, [0.42, 0.50, 0.78]);
    push_disc(&mut out, eye, moon, 180.0, 6.5, [0.88, 0.92, 1.0]);
    if sun[1] < 0.0 {
        const STARS: [[f32; 3]; 4] = [
            [0.15, 0.85, 0.35],
            [-0.45, 0.70, 0.25],
            [0.40, 0.75, -0.40],
            [-0.25, 0.90, -0.15],
        ];
        for dir in STARS {
            push_disc(&mut out, eye, dir, 200.0, 1.4, [0.62, 0.74, 0.98]);
        }
    }
    out
}

fn push_disc(
    out: &mut Vec<mesh::Vertex>,
    eye: glam::Vec3,
    dir: [f32; 3],
    distance: f32,
    radius: f32,
    color: [f32; 3],
) {
    if dir[1] < -0.02 {
        return;
    }
    let forward = glam::Vec3::from(dir).normalize();
    let center = eye + forward * distance;
    let mut right = glam::Vec3::Y.cross(forward);
    if right.length_squared() < 1.0e-4 {
        right = glam::Vec3::X;
    }
    right = right.normalize();
    let up = forward.cross(right).normalize();
    let corners = [
        (center - right * radius - up * radius, [0.0, 1.0]),
        (center + right * radius - up * radius, [1.0, 1.0]),
        (center + right * radius + up * radius, [1.0, 0.0]),
        (center - right * radius + up * radius, [0.0, 0.0]),
    ];
    let normal = forward.to_array();
    let mut quad = Vec::with_capacity(4);
    for (point, uv) in corners {
        quad.push(mesh::Vertex {
            position: point.to_array(),
            color,
            normal,
            uv,
            textured: -1.0,
            block: color,
            bounce: [0.0, 0.0, 0.0],
            ao: 1.0,
        });
    }
    for index in [0, 1, 2, 0, 2, 3] {
        out.push(quad[index]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pot_stores_one_stack_and_gives_it_back() {
        assert_eq!(
            super::pot_move(Some("clay"), "", 0, true),
            super::PotMove::Store
        );
        assert_eq!(
            super::pot_move(Some("clay"), "clay", 3, true),
            super::PotMove::Store
        );
        assert_eq!(
            super::pot_move(Some("clay"), "clay", 64, true),
            super::PotMove::Full
        );
        assert_eq!(
            super::pot_move(Some("dirt"), "clay", 1, true),
            super::PotMove::Full
        );
        assert_eq!(super::pot_move(None, "clay", 1, true), super::PotMove::Take);
        assert_eq!(
            super::pot_move(None, "clay", 1, false),
            super::PotMove::Full
        );
        assert_eq!(super::pot_move(None, "", 0, false), super::PotMove::Empty);
        let empty = [("", 0), ("", 0), ("", 0)];
        assert_eq!(
            super::board_move(Some("apple"), &empty, true),
            super::BoardMove::Store { slot: 0, count: 1 }
        );
        let some = [("apple", 2), ("", 0), ("stick", 1)];
        assert_eq!(
            super::board_move(Some("apple"), &some, true),
            super::BoardMove::Store { slot: 0, count: 3 }
        );
        assert_eq!(
            super::board_move(Some("clay"), &some, true),
            super::BoardMove::Store { slot: 1, count: 1 }
        );
        let full = [("apple", 64), ("clay", 1), ("stick", 1)];
        assert_eq!(
            super::board_move(Some("apple"), &full, true),
            super::BoardMove::Full
        );
        assert_eq!(
            super::board_move(None, &some, true),
            super::BoardMove::Take { slot: 2, count: 0 }
        );
        assert_eq!(
            super::board_move(None, &some, false),
            super::BoardMove::Full
        );
        assert_eq!(
            super::board_move(None, &empty, true),
            super::BoardMove::Empty
        );
        assert_eq!(
            rebuild_world::shelf_meta(1, 0) & 0x0F,
            rebuild_world::shelf_meta(1, 0)
        );
        assert_eq!(
            rebuild_world::shelf_filled(rebuild_world::shelf_meta(2, 3)),
            3
        );
        let wire = super::pot_wire("clay", 2);
        assert_eq!(wire[0], ("clay".to_string(), 2));
        assert!(wire[1].0.is_empty());
    }

    #[test]
    fn an_arrow_scores_on_a_target() {
        assert!(super::arrow_scores("arrow", "target"));
        assert!(!super::arrow_scores("arrow", "hay_bale"));
        assert!(!super::arrow_scores("stick", "target"));
    }

    #[test]
    fn a_brush_finds_one_item_and_a_swept_block_stays_swept() {
        assert!(super::brushable("sand"));
        assert!(super::brushable("gravel"));
        assert!(super::brushable("decorated_pot"));
        assert!(!super::brushable("stone"));
        assert!(!super::already_brushed("sand", 0));
        assert_eq!(super::brush_mark("sand", 0), 1);
        assert!(super::already_brushed("sand", 1));
        assert_eq!(super::brush_mark("decorated_pot", 1), 5);
        assert!(super::already_brushed("decorated_pot", 5));
        assert!(super::already_brushed("decorated_pot", 5) && (5 & 1) == 1);
        let find = super::brush_find(2, 4);
        assert_eq!(super::brush_find(2, 4), find);
        assert!(matches!(
            find,
            "pottery_sherd" | "stick" | "clay" | "bone_meal"
        ));
    }

    #[test]
    fn a_trial_key_opens_a_copper_chest_once() {
        assert!(!super::trial_open(0));
        assert!(super::trial_open(1));
        assert_eq!(super::trial_mark(0), 1);
        assert_eq!(super::trial_mark(1), 1);
        let paid = super::trial_loot(0, 0, false).unwrap();
        let rich = super::trial_loot(0, 0, true).unwrap();
        let table = crate::loot::load_copper_chest().unwrap();
        assert_eq!(paid, table.drops[0]);
        assert_eq!(rich, table.omen[0]);
        assert_eq!(
            super::trial_loot(1, 2, false).as_deref(),
            super::trial_loot(1, 2, false).as_deref()
        );
        assert_ne!(
            super::trial_loot(1, 2, false).as_deref(),
            super::trial_loot(1, 2, true).as_deref()
        );
        let ordinary = super::trial_loot(3, 5, false).unwrap();
        let ominous = super::trial_loot(3, 5, true).unwrap();
        assert!(table.drops.iter().any(|id| id == &ordinary));
        assert!(table.omen.iter().any(|id| id == &ominous));
    }

    #[test]
    fn the_camp_chest_pays_once_from_the_pack() {
        let (x, z) = rebuild_world::CAMP_CHEST;
        assert!(super::camp_cell(x, z));
        assert!(!super::camp_cell(
            rebuild_world::CAMP_FIRE.0,
            rebuild_world::CAMP_FIRE.1
        ));
        let table = crate::loot::load_camp_chest().unwrap();
        let paid = super::fresh_camp(false, x, z, false).unwrap();
        let rich = super::fresh_camp(false, x, z, true).unwrap();
        assert_eq!(paid, crate::loot::pick(&table, x, z, false));
        assert_eq!(rich, crate::loot::pick(&table, x, z, true));
        assert!(super::fresh_camp(true, x, z, false).is_none());
        assert!(super::fresh_camp(false, 0, 0, false).is_none());
    }

    #[test]
    fn the_shrine_chest_pays_once_from_the_pack() {
        let (x, z) = rebuild_world::SHRINE_CHEST;
        assert!(super::shrine_cell(x, z));
        assert!(!super::shrine_cell(
            rebuild_world::SHRINE.0,
            rebuild_world::SHRINE.1
        ));
        assert!(!super::camp_cell(x, z));
        let table = crate::loot::load_shrine_chest().unwrap();
        let paid = super::fresh_shrine(false, x, z, false).unwrap();
        let rich = super::fresh_shrine(false, x, z, true).unwrap();
        assert_eq!(paid, crate::loot::pick(&table, x, z, false));
        assert_eq!(rich, crate::loot::pick(&table, x, z, true));
        assert!(super::fresh_shrine(true, x, z, false).is_none());
        assert!(super::fresh_shrine(false, 0, 0, false).is_none());
    }

    #[test]
    fn the_mine_chest_pays_once_from_the_pack() {
        let (x, z) = rebuild_world::SHAFT_CHEST;
        assert!(super::mine_cell(x, z));
        assert!(!super::mine_cell(
            rebuild_world::SHAFT_MOUTH.0,
            rebuild_world::SHAFT_MOUTH.1
        ));
        assert!(!super::camp_cell(x, z));
        assert!(!super::shrine_cell(x, z));
        let table = crate::loot::load_mine_chest().unwrap();
        let paid = super::fresh_mine(false, x, z, false).unwrap();
        let rich = super::fresh_mine(false, x, z, true).unwrap();
        assert_eq!(paid, crate::loot::pick(&table, x, z, false));
        assert_eq!(rich, crate::loot::pick(&table, x, z, true));
        assert!(super::fresh_mine(true, x, z, false).is_none());
        assert!(super::fresh_mine(false, 0, 0, false).is_none());
    }

    #[test]
    fn sky_edge_tips_only_while_playing_alive() {
        assert!(super::sky_edge_tip_open(true, false, false));
        assert!(!super::sky_edge_tip_open(false, false, false));
        assert!(!super::sky_edge_tip_open(true, true, false));
        assert!(!super::sky_edge_tip_open(true, false, true));
    }

    #[test]
    fn landmark_tips_fire_once_when_near() {
        let near = |ex: f32, ez: f32, x: i32, z: i32| {
            let dx = ex - (x as f32 + 0.5);
            let dz = ez - (z as f32 + 0.5);
            dx * dx + dz * dz < 10.0 * 10.0
        };
        let (sx, sz) = rebuild_world::SHRINE;
        assert!(near(sx as f32 + 0.5, sz as f32 + 0.5, sx, sz));
        assert!(!near(0.5, 0.5, sx, sz));
        let (mx, mz) = rebuild_world::SHAFT_MOUTH;
        assert!(near(mx as f32 + 3.0, mz as f32 + 0.5, mx, mz));
        assert!(!near(0.5, 0.5, mx, mz));
    }

    #[test]
    fn water_edge_mix_rises_beside_a_surface_and_fades_away() {
        let mut cluster = rebuild_world::synthetic_cluster();
        rebuild_world::apply_changes(
            &mut cluster,
            &[rebuild_world::CellChange {
                x: 4,
                y: 1,
                z: 4,
                block: rebuild_world::Block::new("water", 0),
            }],
        );
        let shore = [4.5, 2.4, 5.5];
        let (close, src) = super::nearest_water_edge(&cluster, shore);
        assert!(close > 0.4, "beside the surface: {close}");
        assert!(src.is_some());
        let far = [24.5, 2.4, 24.5];
        let (away, none) = super::nearest_water_edge(&cluster, far);
        assert_eq!(away, 0.0);
        assert!(none.is_none());
    }

    #[test]
    fn open_sky_is_clear_above_spawn_and_blocked_by_a_roof() {
        let mut cluster = rebuild_world::synthetic_cluster();
        rebuild_world::seed_terrain_ring(&mut cluster);
        let eye = [0.5, 70.0, 0.5];
        assert!(super::open_sky(&cluster, eye));
        rebuild_world::apply_changes(
            &mut cluster,
            &[rebuild_world::CellChange {
                x: 0,
                y: 72,
                z: 0,
                block: rebuild_world::Block::new("stone", 0),
            }],
        );
        assert!(!super::open_sky(&cluster, eye));
    }

    #[test]
    fn cave_drips_need_deep_cover_not_a_thin_roof() {
        let mut cluster = rebuild_world::synthetic_cluster();
        let eye = [0.5, 40.0, 0.5];
        assert_eq!(super::cave_drip_mix(&cluster, eye), 0.0);
        rebuild_world::apply_changes(
            &mut cluster,
            &[
                rebuild_world::CellChange {
                    x: 0,
                    y: 42,
                    z: 0,
                    block: rebuild_world::Block::new("stone", 0),
                },
                rebuild_world::CellChange {
                    x: 0,
                    y: 43,
                    z: 0,
                    block: rebuild_world::Block::new("stone", 0),
                },
            ],
        );
        assert_eq!(super::cave_drip_mix(&cluster, eye), 0.0);
        rebuild_world::apply_changes(
            &mut cluster,
            &(44..50)
                .map(|y| rebuild_world::CellChange {
                    x: 0,
                    y,
                    z: 0,
                    block: rebuild_world::Block::new("stone", 0),
                })
                .collect::<Vec<_>>(),
        );
        let deep = super::cave_drip_mix(&cluster, eye);
        assert!(deep > 0.5, "deep cover drips: {deep}");
    }

    #[test]
    fn campfire_crackle_mix_rises_near_the_spawn_fire() {
        let mut cluster = rebuild_world::synthetic_cluster();
        rebuild_world::seed_terrain_ring(&mut cluster);
        let (fx, fz) = rebuild_world::CAMP_FIRE;
        let mut fy = None;
        for y in 0..80 {
            if cluster.block_at(fx, y, fz).id == "campfire" {
                fy = Some(y);
                break;
            }
        }
        let fy = fy.expect("spawn campfire");
        let at = [fx as f32 + 0.5, fy as f32 + 1.6, fz as f32 + 0.5];
        let (close, src) = super::nearest_campfire(&cluster, at);
        assert!(close > 0.7, "standing over the fire: {close}");
        assert!(src.is_some());
        let far = [fx as f32 + 20.0, fy as f32 + 1.6, fz as f32 + 20.0];
        let (away, none) = super::nearest_campfire(&cluster, far);
        assert_eq!(away, 0.0);
        assert!(none.is_none());
    }

    #[test]
    fn a_frame_stores_one_item_and_an_empty_hand_takes_it() {
        assert!(matches!(
            super::frame_move(Some("stick"), "", true),
            super::FrameMove::Store
        ));
        assert!(matches!(
            super::frame_move(None, "", true),
            super::FrameMove::Empty
        ));
        assert!(matches!(
            super::frame_move(Some("stick"), "apple", true),
            super::FrameMove::Full
        ));
        assert!(matches!(
            super::frame_move(None, "apple", true),
            super::FrameMove::Take
        ));
        assert!(matches!(
            super::frame_move(None, "apple", false),
            super::FrameMove::Full
        ));
    }

    #[test]
    fn a_neighbor_pulse_finds_a_copper_door() {
        use rebuild_world::{Block, Chunk, Cluster};
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 1, 1, Block::new("button", 0)),
                (2, 1, 1, Block::new("copper_door", 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let (at, block) = super::press_target(&world, [1, 1, 1]).expect("door beside the button");
        assert_eq!(at, [2, 1, 1]);
        assert_eq!(block.id, "copper_door");
        assert!(super::hinge("copper_door"));
        assert!(super::hinge("door"));
        assert!(super::hinge("copper_trapdoor"));
        assert!(super::hinge("trapdoor"));
    }

    #[test]
    fn a_button_press_opens_the_neighbor_gate() {
        use rebuild_world::{Block, Chunk, Cluster};
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 1, 1, Block::new("button", 0)),
                (2, 1, 1, Block::new("fence_gate", 0)),
                (0, 1, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let (at, block) = super::press_target(&world, [1, 1, 1]).expect("gate beside the button");
        assert_eq!(at, [2, 1, 1]);
        assert_eq!(block.id, "fence_gate");
        assert!(super::press_target(&world, [0, 1, 1]).is_none());
    }

    #[test]
    fn a_lever_latches_the_neighbor_until_it_is_flipped_off() {
        assert_eq!(super::lever_wants(true, false), Some(true));
        assert_eq!(super::lever_wants(true, true), None);
        assert_eq!(super::lever_wants(false, true), Some(false));
        assert_eq!(super::lever_wants(false, false), None);
    }

    #[test]
    fn a_held_flower_plants_an_empty_pot() {
        assert!(matches!(super::pot_plant(0, true), super::PotPlant::Plant));
        assert!(matches!(
            super::pot_plant(0, false),
            super::PotPlant::NeedFlower
        ));
        assert!(matches!(super::pot_plant(1, true), super::PotPlant::Full));
        assert!(matches!(super::pot_plant(1, false), super::PotPlant::Full));
    }

    #[test]
    fn an_eye_sets_in_an_empty_frame_and_a_pair_of_neighbors_stirs() {
        use rebuild_world::{Block, Chunk, Cluster};
        assert!(matches!(super::frame_eye(0, true), super::FrameEye::Set));
        assert!(matches!(
            super::frame_eye(0, false),
            super::FrameEye::NeedEye
        ));
        assert!(matches!(super::frame_eye(1, true), super::FrameEye::Full));
        assert_eq!(super::ring_word(0), "the eye sets");
        assert_eq!(super::ring_word(1), "the eye sets");
        assert_eq!(super::ring_word(2), "the ring stirs");
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (2, 1, 2, Block::new("end_portal_frame", 0)),
                (1, 1, 2, Block::new("end_portal_frame", 1)),
                (3, 1, 2, Block::new("end_portal_frame", 1)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        assert_eq!(super::filled_beside(&world, [2, 1, 2]), 2);
        assert_eq!(super::filled_beside(&world, [1, 1, 2]), 0);
    }

    #[test]
    fn a_torch_lights_a_pumpkin_and_a_bare_hand_only_carves() {
        match super::pumpkin_carve(0, false) {
            super::PumpkinCarve::Set {
                metadata,
                spend_torch,
                word,
            } => {
                assert_eq!(metadata, 1);
                assert!(!spend_torch);
                assert_eq!(word, "carved");
            }
            super::PumpkinCarve::Say(_) => panic!("a plain pumpkin should carve"),
        }
        match super::pumpkin_carve(0, true) {
            super::PumpkinCarve::Set {
                metadata,
                spend_torch,
                word,
            } => {
                assert_eq!(metadata, 2);
                assert!(spend_torch);
                assert_eq!(word, "lit");
            }
            super::PumpkinCarve::Say(_) => panic!("a torch should light the pumpkin"),
        }
        match super::pumpkin_carve(1, true) {
            super::PumpkinCarve::Set {
                metadata,
                spend_torch,
                ..
            } => {
                assert_eq!(metadata, 2);
                assert!(spend_torch);
            }
            super::PumpkinCarve::Say(_) => panic!("a carved pumpkin should still light"),
        }
        assert!(matches!(
            super::pumpkin_carve(1, false),
            super::PumpkinCarve::Say("carved")
        ));
        assert!(matches!(
            super::pumpkin_carve(2, true),
            super::PumpkinCarve::Say("lit")
        ));
    }

    #[test]
    fn standing_on_a_plate_is_the_cell_under_the_feet() {
        use rebuild_world::{Block, Chunk, Cluster};
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 0, Block::new("plate", 0)),
                (1, 1, 0, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let on_it = glam::Vec3::new(0.5, 1.125 + crate::body::EYE_HEIGHT, 0.5);
        assert_eq!(super::plate_under(&world, on_it), Some([0, 1, 0]));
        let beside = glam::Vec3::new(1.5, 1.0 + crate::body::EYE_HEIGHT, 0.5);
        assert_eq!(super::plate_under(&world, beside), None);
    }

    #[test]
    fn walking_through_a_tripwire_is_the_cell_under_the_feet() {
        use rebuild_world::{Block, Chunk, Cluster};
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 0, Block::new("tripwire", 0)),
                (1, 1, 0, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let through = glam::Vec3::new(0.5, 1.0 + crate::body::EYE_HEIGHT, 0.5);
        assert_eq!(super::plate_under(&world, through), Some([0, 1, 0]));
        let beside = glam::Vec3::new(2.5, 1.0 + crate::body::EYE_HEIGHT, 0.5);
        assert_eq!(super::plate_under(&world, beside), None);
    }

    #[test]
    fn world_light_stays_in_the_vertex_color() {
        let src = super::SHADER;
        assert!(src.contains("input.block"));
        assert!(src.contains("input.bounce"));
        assert!(src.contains("input.ao"));
        assert!(src.contains("textureSampleCompare"));
        assert!(src.contains("u.cascades"));
        assert!(src.contains("texture_depth_2d_array"));
        assert!(src.contains("override CASCADE"));
        assert!(!src.contains("input.shadow"));
        assert!(src.contains("lit * albedo"));
        assert!(src.contains("u.fog_eye"));
        assert!(src.contains("u.fog_color"));
        assert!(src.contains("@interpolate(flat) textured"));
        assert!(src.contains("cell = 18.0"));
        assert!(src.contains("floor(tile * 16.0)"));
        assert!(src.contains("texel + vec2<f32>(1.5)"));
        assert!(src.contains("u.sun_dir"));
        assert!(src.contains("0.34 + 0.66 * ndotl"));
        assert!(src.contains("0.86 + 0.14 * ndotl"));
        assert!(src.contains("nrm * 0.42"));
        assert!(src.contains("let reach = 3.6"));
        assert!(src.contains("array<f32, 3>(0.8, 1.8, 3.0)"));
        assert!(src.contains("mix(0.84, 1.0, sqrt(clamp(sky_open, 0.0, 1.0)))"));
        assert!(src.contains("mix(0.90, 1.0, sqrt(clamp(contact, 0.0, 1.0)))"));
        assert!(src.contains("sqrt(clamp(sky_open, 0.0, 1.0))"));
        assert!(src.contains("sqrt(clamp(contact, 0.0, 1.0))"));
        assert!(src.contains("trace_margin"));
        assert!(src.contains("(1.0 - use_trace)"));
        assert!(src.contains("texture_3d"));
        assert!(src.contains("traced_sun"));
        assert!(src.contains("1.0 - use_trace"));
        assert!(src.contains("sky_open"));
        assert!(src.contains("contact"));
        assert!(src.contains("mix(sky, sky_open, use_trace)"));
        assert!(!src.to_lowercase().contains("pathtrace"));
        assert!(!src.to_lowercase().contains("path trace"));
        assert!(src.contains("discard"));
    }

    #[test]
    fn the_light_frustum_puts_the_sun_in_front_of_the_ground() {
        let focus = [8.0, 1.0, 8.0];
        let matrix =
            glam::Mat4::from_cols_array(&super::light_view_proj(focus, crate::daylight::NOON_SUN));
        let dir = glam::Vec3::from(crate::daylight::NOON_SUN).normalize();
        let depth = |point: glam::Vec3| {
            let clip = matrix * point.extend(1.0);
            clip.z / clip.w
        };
        let ground = glam::Vec3::from(focus);
        let closer = ground + dir * 6.0;
        assert!(
            depth(closer) < depth(ground),
            "a point toward the sun is nearer in the shadow map"
        );
        assert!((0.0..1.0).contains(&depth(ground)));
    }

    /// Bias stays a centimeter plus a capped slope term. Three cascades keep
    /// the near texel small and still cover ground past the old single box.
    #[test]
    fn cast_shadow_starts_at_the_blocker() {
        let src = super::SHADER;
        assert!(src.contains("2.0 + 2.0 * tan_theta"));
        assert!(src.contains("texel_world * 4.0"));
        assert!(src.contains("miss.edge = 2.0"));
        assert!(src.contains("i < 9"));
        assert!(src.contains("smoothstep(0.85, 1.0, primary.edge)"));
        assert!(src.contains("input.ao"));
        assert!(!src.contains("input.shadow"));
        assert!(!src.contains("shadow_kernel_angle"));
        assert!(!src.contains("0.0012"));

        assert!(
            super::shadow_texel_world() <= 64.0 / 4096.0 + 1.0e-6,
            "the near cascade is at least as fine as the previous map"
        );
        assert!(super::cascade_texel(0) < super::cascade_texel(1));
        assert!(super::cascade_texel(1) < super::cascade_texel(2));
        assert!(
            super::CASCADE_HALF[2] > 48.0,
            "the far cascade outruns the old box"
        );
        assert_eq!(super::CASCADE_COUNT, 3);
        let noon = super::shadow_receiver_bias(0.8);
        assert!(
            noon < 0.06,
            "noon ground bias stays on the blocker, {noon} blocks"
        );
        let texel = super::shadow_texel_world();
        assert!((super::shadow_receiver_bias(1.0) - 2.0 * texel).abs() < 1.0e-6);
        assert!(super::shadow_receiver_bias(0.0) <= 4.0 * texel + 1.0e-6);
        assert!(
            noon > 2.0 * texel,
            "a sloped noon face needs more than the flat bias"
        );

        let focus = [8.0, 2.0, 8.0];
        let sun = crate::daylight::NOON_SUN;
        let matrix = glam::Mat4::from_cols_array(&super::cascade_view_proj(focus, sun, 0));
        let dir = glam::Vec3::from(sun).normalize();
        let depth = |point: glam::Vec3| {
            let clip = matrix * point.extend(1.0);
            clip.z / clip.w
        };
        let post = glam::Vec3::from(focus);
        // 0.08 blocks down-sun: past the capped bias, inside the old 0.29-block gap.
        let ground = post - dir * 0.08;
        let gap_world = (depth(ground) - depth(post)) * super::cascade_span(0);
        assert!(
            gap_world > super::shadow_receiver_bias(0.0),
            "a short step past the post is in shadow, world gap {gap_world}"
        );
        assert!(
            gap_world < 0.2,
            "that step used to be eaten by a 0.29-block bias, world gap {gap_world}"
        );

        let clip = matrix * post.extend(1.0);
        let uv_x = clip.x / clip.w * 0.5 + 0.5;
        let uv_y = -(clip.y / clip.w) * 0.5 + 0.5;
        let on_center = |p: f32| {
            let texel = p * super::CASCADE_MAP as f32;
            (texel - (texel.floor() + 0.5)).abs() < 1.0e-2
        };
        assert!(on_center(uv_x), "shadow origin sits on a texel center");
        assert!(on_center(uv_y), "shadow origin sits on a texel center");

        let inside = |cascade: usize, point: glam::Vec3| {
            let matrix =
                glam::Mat4::from_cols_array(&super::cascade_view_proj(focus, sun, cascade));
            let clip = matrix * point.extend(1.0);
            clip.x.abs() < clip.w && clip.y.abs() < clip.w
        };
        let near = post + glam::Vec3::new(4.0, 0.0, 0.0);
        let mid = post + glam::Vec3::new(30.0, 0.0, 0.0);
        let far = post + glam::Vec3::new(80.0, 0.0, 0.0);
        let past = post + glam::Vec3::new(200.0, 0.0, 0.0);
        assert!(inside(0, near));
        assert!(!inside(0, mid));
        assert!(inside(1, mid));
        assert!(!inside(1, far));
        assert!(inside(2, far));
        assert!(!inside(2, past));
    }

    #[test]
    fn noon_draws_the_sun_and_midnight_draws_the_moon() {
        let eye = glam::Vec3::new(8.0, 5.0, 8.0);
        let noon = super::sky_discs(
            eye,
            0.5,
            crate::daylight::Season::Spring,
            crate::daylight::Hemisphere::South,
        );
        let night = super::sky_discs(
            eye,
            0.0,
            crate::daylight::Season::Spring,
            crate::daylight::Hemisphere::South,
        );
        assert_eq!(noon.len(), 12, "sun halo and core");
        assert_eq!(night.len(), 36, "moon halo, moon core, and four stars");
        assert!(noon.iter().all(|vertex| vertex.color[0] > vertex.color[2]));
        assert!(night.iter().all(|vertex| vertex.color[2] > vertex.color[0]));
        assert!(
            noon.iter()
                .all(|vertex| vertex.textured < 0.0 && vertex.block == vertex.color)
        );
        let noon_height = noon
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0, f32::max);
        assert!(noon_height > eye.y + 40.0, "the noon sun sits high");
    }

    /// Chunk borders store UV 0 or 16. Perspective correction nudges the 0 edge
    /// slightly negative, and `fract` wraps that onto the far side of the tile.
    /// That sample used to read the dirt column.
    #[test]
    fn chunk_edge_uv_samples_grass_not_the_dirt_column() {
        let grass =
            crate::texture::texture_slot("assets/terralite/textures/grass.png").unwrap() as i32;
        let dirt = crate::texture::texture_slot("assets/terralite/textures/block/dirt.png").unwrap()
            as i32;
        let atlas = crate::texture::load_pack_texture();
        let grass_png = crate::texture::decode_png(
            &rebuild_world::asset_png("assets/terralite/textures/grass.png").unwrap(),
        )
        .unwrap();
        let dirt_png = crate::texture::decode_png(
            &rebuild_world::asset_png("assets/terralite/textures/block/dirt.png").unwrap(),
        )
        .unwrap();

        fn fract(v: f32) -> f32 {
            v - v.floor()
        }
        fn pixel(uv: f32, slot: i32) -> i32 {
            let tile = fract(uv);
            let texel = (tile * 16.0).floor().clamp(0.0, 15.0) as i32;
            texel + 1 + slot * 18
        }
        fn at(image: &crate::texture::Image, x: i32, y: i32) -> [u8; 4] {
            let i = ((y as u32 * image.width + x as u32) * 4) as usize;
            [
                image.rgba[i],
                image.rgba[i + 1],
                image.rgba[i + 2],
                image.rgba[i + 3],
            ]
        }

        let edges = [0.0, 16.0, -1.0e-3, 16.001, 15.999, -16.02, 32.0];
        for &u in &edges {
            for &v in &edges {
                let x = pixel(u, grass);
                let y = pixel(v, 0);
                let local_x = (fract(u) * 16.0).floor().clamp(0.0, 15.0) as i32;
                let local_y = (fract(v) * 16.0).floor().clamp(0.0, 15.0) as i32;
                assert!(
                    (grass * 18..grass * 18 + 18).contains(&x),
                    "uv {u} left the grass column at pixel {x}"
                );
                assert!(
                    !(dirt * 18..dirt * 18 + 18).contains(&x),
                    "uv {u} sampled the dirt column at pixel {x}"
                );
                assert_eq!(
                    at(&atlas, x, y),
                    at(&grass_png, local_x, local_y),
                    "uv ({u}, {v}) is not the grass texel ({local_x}, {local_y})"
                );
            }
        }
        let wrapped = pixel(-1.0e-3, grass);
        // Source row 8 lives on atlas row 9 (row 0 is the gutter).
        assert_eq!(at(&atlas, wrapped, 9), at(&grass_png, 15, 8));
        assert_ne!(
            at(&atlas, wrapped, 9),
            at(&dirt_png, 0, 8),
            "negative chunk-edge uv reads dirt"
        );
    }

    #[test]
    fn the_shipped_frame_shader_compiles() {
        let hook = crate::shade::load_frame_hook().unwrap();
        assert!(hook.enabled);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        let Ok(adapter) = adapter else {
            return;
        };
        let Ok((device, _queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        else {
            return;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let world = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world-test"),
            source: wgpu::ShaderSource::Wgsl(super::SHADER.into()),
        });
        let ui = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui-test"),
            source: wgpu::ShaderSource::Wgsl(super::UI_SHADER.into()),
        });
        let frame = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame-test"),
            source: wgpu::ShaderSource::Wgsl(hook.source.into()),
        });
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "shader: {err:?}");
        let _ = (world, frame, ui);
    }
}
