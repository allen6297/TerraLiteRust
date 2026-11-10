//! Inventory / creative slot icons from real block & item models.
//!
//! Minecraft projects `models/block` / `models/item` (and Blockbench `.bbmodel`)
//! through `display.gui`. Terralite does the same: resolve
//! [`Pack::slot_model`], apply that transform, and soft-raster the Shape the
//! world already meshes — not a separate house-shaped stub.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::ColorImage;
use rebuild_world::{asset_png, ItemDisplay, MeshFace, Pack, Shape};

use crate::palette;
use crate::texture::{self, decode_png, Image};

/// Soft-render a slot icon for `id` at `size`×`size` RGBA.
pub fn render(id: &str, size: usize) -> ColorImage {
    let size = size.max(8);
    let pack = Pack::builtin();
    if let Some((shape, gui)) = pack.slot_model(id) {
        if let Some(image) = raster_shape(id, shape, gui, size) {
            return image;
        }
    }
    flat_fallback(id, size)
}

fn raster_shape(
    id: &str,
    shape: Shape<'_>,
    gui: ItemDisplay,
    size: usize,
) -> Option<ColorImage> {
    let mut quads = Vec::new();
    if !shape.faces().is_empty() {
        for face in shape.faces() {
            push_mesh_face(&mut quads, id, shape, *face);
        }
    } else {
        synthesize_boxes(&mut quads, id, shape);
        synthesize_planes(&mut quads, id, shape);
    }
    if quads.is_empty() {
        return None;
    }
    Some(soft_project(&quads, gui, size))
}

struct Quad {
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    /// Sampled texture, or `None` for a flat tinted face.
    tile: Option<&'static Image>,
    /// Multiplied into the sample (grass biome tint, etc.).
    tint: [f32; 3],
}

fn push_mesh_face(quads: &mut Vec<Quad>, id: &str, shape: Shape<'_>, face: MeshFace) {
    let corners = face.corners();
    let uvs = face.uvs();
    let path = shape.texture_of(face);
    let normal = quad_normal(corners);
    let tint = icon_tint(id, path, normal);
    let tile = path.and_then(cached_tile);
    quads.push(Quad {
        corners,
        uvs,
        tile,
        tint,
    });
}

fn synthesize_boxes(quads: &mut Vec<Quad>, id: &str, shape: Shape<'_>) {
    let tile = pack_face_tile(id);
    let path = texture::face_path(id);
    for box_ in shape.boxes() {
        let min = [
            f32::from(box_.min[0]) / 16.0,
            f32::from(box_.min[1]) / 16.0,
            f32::from(box_.min[2]) / 16.0,
        ];
        let max = [
            f32::from(box_.max[0]) / 16.0,
            f32::from(box_.max[1]) / 16.0,
            f32::from(box_.max[2]) / 16.0,
        ];
        // West / east / down / up / north / south — same order MC emits.
        let faces = [
            (
                [[min[0], min[1], max[2]], [min[0], min[1], min[2]], [min[0], max[1], min[2]], [min[0], max[1], max[2]]],
                [-1.0, 0.0, 0.0],
            ),
            (
                [[max[0], min[1], min[2]], [max[0], min[1], max[2]], [max[0], max[1], max[2]], [max[0], max[1], min[2]]],
                [1.0, 0.0, 0.0],
            ),
            (
                [[min[0], min[1], min[2]], [max[0], min[1], min[2]], [max[0], min[1], max[2]], [min[0], min[1], max[2]]],
                [0.0, -1.0, 0.0],
            ),
            (
                [[min[0], max[1], max[2]], [max[0], max[1], max[2]], [max[0], max[1], min[2]], [min[0], max[1], min[2]]],
                [0.0, 1.0, 0.0],
            ),
            (
                [[max[0], min[1], min[2]], [min[0], min[1], min[2]], [min[0], max[1], min[2]], [max[0], max[1], min[2]]],
                [0.0, 0.0, -1.0],
            ),
            (
                [[min[0], min[1], max[2]], [max[0], min[1], max[2]], [max[0], max[1], max[2]], [min[0], max[1], max[2]]],
                [0.0, 0.0, 1.0],
            ),
        ];
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for (corners, normal) in faces {
            let tint = icon_tint(id, path.as_deref(), normal);
            quads.push(Quad {
                corners,
                uvs,
                tile,
                tint,
            });
        }
    }
}

fn synthesize_planes(quads: &mut Vec<Quad>, id: &str, shape: Shape<'_>) {
    let tile = pack_face_tile(id);
    let path = texture::face_path(id);
    for plane in shape.planes() {
        let at = f32::from(plane.at) / 16.0;
        let (u0, v0, u1, v1) = (
            f32::from(plane.min[0]) / 16.0,
            f32::from(plane.min[1]) / 16.0,
            f32::from(plane.max[0]) / 16.0,
            f32::from(plane.max[1]) / 16.0,
        );
        let (corners, normal) = match plane.axis {
            0 => (
                [
                    [at, v0, u1],
                    [at, v0, u0],
                    [at, v1, u0],
                    [at, v1, u1],
                ],
                [1.0, 0.0, 0.0],
            ),
            1 => (
                [
                    [u0, at, v0],
                    [u1, at, v0],
                    [u1, at, v1],
                    [u0, at, v1],
                ],
                [0.0, 1.0, 0.0],
            ),
            _ => (
                [
                    [u0, v0, at],
                    [u1, v0, at],
                    [u1, v1, at],
                    [u0, v1, at],
                ],
                [0.0, 0.0, 1.0],
            ),
        };
        let tint = icon_tint(id, path.as_deref(), normal);
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        quads.push(Quad {
            corners,
            uvs,
            tile,
            tint,
        });
    }
}

fn icon_tint(id: &str, path: Option<&str>, normal: [f32; 3]) -> [f32; 3] {
    if path.is_some_and(|p| p.ends_with("dirt.png") || p.contains("dirt_alt_")) {
        return [1.0, 1.0, 1.0];
    }
    if id == "grass" {
        let (top, side) = ([0.45, 0.70, 0.30], [0.50, 0.58, 0.28]);
        // Overlay fringe + top tile always take the biome greens (normals on
        // rotated Blockbench planes are not reliable side/up tests).
        if path.is_some_and(|p| p.contains("grass_block_top") || p.contains("overlay")) {
            return if path.is_some_and(|p| p.contains("top")) {
                top
            } else {
                side
            };
        }
        if normal[1] > 0.5 {
            return top;
        }
        if normal[1] < -0.5 {
            return [0.58, 0.41, 0.27];
        }
        return side;
    }
    if id == "oak_log" && normal[1].abs() > 0.5 {
        return [0.72, 0.58, 0.32];
    }
    // Textured faces keep albedo; shade comes from the soft wrap.
    if path.is_some() {
        return [1.0, 1.0, 1.0];
    }
    palette::color(id, 0)
}

fn pack_face_tile(id: &str) -> Option<&'static Image> {
    if let Some(path) = texture::face_path(id) {
        if let Some(tile) = cached_tile(&path) {
            return Some(tile);
        }
    }
    static TILES: OnceLock<std::sync::Mutex<HashMap<String, &'static Image>>> = OnceLock::new();
    let cache = TILES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("face tile cache");
    if let Some(tile) = cache.get(id) {
        return Some(*tile);
    }
    let owned = texture::face_tile(id)?;
    let leaked: &'static Image = Box::leak(Box::new(owned));
    cache.insert(id.to_string(), leaked);
    Some(leaked)
}

fn cached_tile(path: &str) -> Option<&'static Image> {
    static TILES: OnceLock<std::sync::Mutex<HashMap<String, Option<&'static Image>>>> =
        OnceLock::new();
    let cache = TILES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("model tile cache");
    if let Some(hit) = cache.get(path) {
        return *hit;
    }
    let loaded = asset_png(path)
        .and_then(|bytes| decode_png(&bytes))
        .map(|image| &*Box::leak(Box::new(image)));
    cache.insert(path.to_string(), loaded);
    loaded
}

fn soft_project(quads: &[Quad], gui: ItemDisplay, size: usize) -> ColorImage {
    let mut rgba = vec![0u8; size * size * 4];
    let mut depth = vec![f32::INFINITY; size * size];
    let cx = (size as f32 - 1.0) * 0.5;
    let cy = (size as f32 - 1.0) * 0.52;
    // MC gui scale 0.625 → fill most of the slot with a soft pad.
    let s = gui.scale[0].max(gui.scale[1]).max(gui.scale[2]).max(0.1);
    // Keep a little pad so the MC gui cube doesn't clip the slot edge.
    let fit = size as f32 * 0.82 / s;
    let project = |p: [f32; 3]| -> (f32, f32, f32) {
        let t = apply_gui(p, gui);
        (cx + t[0] * fit, cy - t[1] * fit, -t[2])
    };
    // Opaque cube faces first, then translucent overlays (grass fringe) on top.
    let mut order: Vec<usize> = (0..quads.len()).collect();
    order.sort_by(|&a, &b| {
        let ta = quad_opacity_hint(&quads[a]);
        let tb = quad_opacity_hint(&quads[b]);
        ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
    });
    for index in order {
        let quad = &quads[index];
        let pts = [
            project(quad.corners[0]),
            project(quad.corners[1]),
            project(quad.corners[2]),
            project(quad.corners[3]),
        ];
        let uvs = quad.uvs;
        fill_tri(
            &mut rgba,
            &mut depth,
            size,
            [pts[0], pts[1], pts[2]],
            [uvs[0], uvs[1], uvs[2]],
            quad,
        );
        fill_tri(
            &mut rgba,
            &mut depth,
            size,
            [pts[0], pts[2], pts[3]],
            [uvs[0], uvs[2], uvs[3]],
            quad,
        );
    }
    ColorImage::from_rgba_unmultiplied([size, size], &rgba)
}

fn quad_opacity_hint(quad: &Quad) -> f32 {
    // Prefer drawing known overlays last so they composite over dirt sides.
    let Some(tile) = quad.tile else {
        return 1.0;
    };
    let mut alpha = 0.0_f32;
    let mut n = 0.0;
    for i in (3..tile.rgba.len()).step_by(4) {
        alpha += tile.rgba[i] as f32;
        n += 1.0;
    }
    if n <= 0.0 {
        return 1.0;
    }
    alpha / (n * 255.0)
}

/// Minecraft item `display.gui`: center the block, scale, rotate X/Y/Z, translate.
fn apply_gui(p: [f32; 3], gui: ItemDisplay) -> [f32; 3] {
    let mut v = [
        (p[0] - 0.5) * gui.scale[0],
        (p[1] - 0.5) * gui.scale[1],
        (p[2] - 0.5) * gui.scale[2],
    ];
    v = rot_x(v, gui.rotation[0].to_radians());
    v = rot_y(v, gui.rotation[1].to_radians());
    v = rot_z(v, gui.rotation[2].to_radians());
    // Translation is in sixteenths of a block in MC JSON.
    v[0] += gui.translation[0] / 16.0;
    v[1] += gui.translation[1] / 16.0;
    v[2] += gui.translation[2] / 16.0;
    v
}

fn rot_x(p: [f32; 3], rad: f32) -> [f32; 3] {
    let (s, c) = rad.sin_cos();
    [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]
}

fn rot_y(p: [f32; 3], rad: f32) -> [f32; 3] {
    let (s, c) = rad.sin_cos();
    [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]
}

fn rot_z(p: [f32; 3], rad: f32) -> [f32; 3] {
    let (s, c) = rad.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}

fn fill_tri(
    rgba: &mut [u8],
    depth: &mut [f32],
    size: usize,
    pts: [(f32, f32, f32); 3],
    uvs: [[f32; 2]; 3],
    quad: &Quad,
) {
    let (ax, ay, az) = pts[0];
    let (bx, by, bz) = pts[1];
    let (cx, cy, cz) = pts[2];
    let e1 = [
        quad.corners[1][0] - quad.corners[0][0],
        quad.corners[1][1] - quad.corners[0][1],
        quad.corners[1][2] - quad.corners[0][2],
    ];
    let e2 = [
        quad.corners[2][0] - quad.corners[0][0],
        quad.corners[2][1] - quad.corners[0][1],
        quad.corners[2][2] - quad.corners[0][2],
    ];
    let nx = e1[1] * e2[2] - e1[2] * e2[1];
    let ny = e1[2] * e2[0] - e1[0] * e2[2];
    let nz = e1[0] * e2[1] - e1[1] * e2[0];
    let nlen = (nx * nx + ny * ny + nz * nz).sqrt().max(1.0e-5);
    let key = (nx / nlen) * 0.25 + (ny / nlen) * 0.75 + (nz / nlen) * 0.35;
    let wrap = (key * 0.5 + 0.5).clamp(0.0, 1.0);
    let face_shade = 0.72 + 0.28 * wrap * wrap;

    let area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
    if area.abs() < 1.0e-5 {
        return;
    }
    let min_x = ax.min(bx).min(cx).floor().max(0.0) as usize;
    let max_x = ax.max(bx).max(cx).ceil().min(size as f32 - 1.0) as usize;
    let min_y = ay.min(by).min(cy).floor().max(0.0) as usize;
    let max_y = ay.max(by).max(cy).ceil().min(size as f32 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = ((bx - px) * (cy - py) - (cx - px) * (by - py)) / area;
            let w1 = ((cx - px) * (ay - py) - (ax - px) * (cy - py)) / area;
            let w2 = 1.0 - w0 - w1;
            let edge = w0.min(w1).min(w2);
            if edge < -0.02 {
                continue;
            }
            let cover = ((edge + 0.02) / 0.06).clamp(0.0, 1.0);
            let z = az * w0 + bz * w1 + cz * w2;
            let i = y * size + x;
            let u = uvs[0][0] * w0 + uvs[1][0] * w1 + uvs[2][0] * w2;
            let v = uvs[0][1] * w0 + uvs[1][1] * w1 + uvs[2][1] * w2;
            let (sr, sg, sb, sa) = sample_quad(quad, u, v);
            if sa < 8.0 {
                continue;
            }
            let r = sr * quad.tint[0] * face_shade;
            let g = sg * quad.tint[1] * face_shade;
            let b = sb * quad.tint[2] * face_shade;
            let a = (sa / 255.0) * cover;
            if a <= 0.01 {
                continue;
            }
            let o = i * 4;
            let dst_a = rgba[o + 3] as f32 / 255.0;
            // Translucent model faces (grass overlay) composite; solids depth-test.
            if a < 0.95 {
                if z > depth[i] + 0.002 {
                    continue;
                }
                let out_a = a + dst_a * (1.0 - a);
                if out_a <= 0.01 {
                    continue;
                }
                let blend = |src: f32, dst: u8| {
                    (src * a + dst as f32 * dst_a * (1.0 - a)) / out_a
                };
                rgba[o] = blend(r, rgba[o]).clamp(0.0, 255.0) as u8;
                rgba[o + 1] = blend(g, rgba[o + 1]).clamp(0.0, 255.0) as u8;
                rgba[o + 2] = blend(b, rgba[o + 2]).clamp(0.0, 255.0) as u8;
                rgba[o + 3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
                continue;
            }
            if z >= depth[i] {
                continue;
            }
            depth[i] = z;
            rgba[o] = r.clamp(0.0, 255.0) as u8;
            rgba[o + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[o + 2] = b.clamp(0.0, 255.0) as u8;
            rgba[o + 3] = (a * 255.0).clamp(0.0, 255.0) as u8;
        }
    }
}

fn sample_quad(quad: &Quad, u: f32, v: f32) -> (f32, f32, f32, f32) {
    if let Some(tile) = quad.tile {
        return sample_bilinear(tile, u, v);
    }
    // White base — `tint` and face shade multiply in `fill_tri`.
    (255.0, 255.0, 255.0, 255.0)
}

fn sample_bilinear(tile: &Image, u: f32, v: f32) -> (f32, f32, f32, f32) {
    let w = tile.width.max(1) as f32;
    let h = tile.height.max(1) as f32;
    let x = (u.clamp(0.0, 1.0) * (w - 1.0)).clamp(0.0, w - 1.0);
    let y = (v.clamp(0.0, 1.0) * (h - 1.0)).clamp(0.0, h - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(tile.width.saturating_sub(1));
    let y1 = (y0 + 1).min(tile.height.saturating_sub(1));
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let texel = |tx: u32, ty: u32| {
        let i = ((ty * tile.width + tx) * 4) as usize;
        [
            tile.rgba[i] as f32,
            tile.rgba[i + 1] as f32,
            tile.rgba[i + 2] as f32,
            tile.rgba[i + 3] as f32,
        ]
    };
    let c00 = texel(x0, y0);
    let c10 = texel(x1, y0);
    let c01 = texel(x0, y1);
    let c11 = texel(x1, y1);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = [
        lerp(c00[0], c10[0], fx),
        lerp(c00[1], c10[1], fx),
        lerp(c00[2], c10[2], fx),
        lerp(c00[3], c10[3], fx),
    ];
    let bot = [
        lerp(c01[0], c11[0], fx),
        lerp(c01[1], c11[1], fx),
        lerp(c01[2], c11[2], fx),
        lerp(c01[3], c11[3], fx),
    ];
    (
        lerp(top[0], bot[0], fy),
        lerp(top[1], bot[1], fy),
        lerp(top[2], bot[2], fy),
        lerp(top[3], bot[3], fy),
    )
}

fn quad_normal(corners: [[f32; 3]; 4]) -> [f32; 3] {
    let e1 = [
        corners[1][0] - corners[0][0],
        corners[1][1] - corners[0][1],
        corners[1][2] - corners[0][2],
    ];
    let e2 = [
        corners[2][0] - corners[0][0],
        corners[2][1] - corners[0][1],
        corners[2][2] - corners[0][2],
    ];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1.0e-5);
    [n[0] / len, n[1] / len, n[2] / len]
}

fn flat_fallback(id: &str, size: usize) -> ColorImage {
    let tile = texture::face_tile(id);
    let base = palette::color(id, 0);
    let mut rgba = vec![0u8; size * size * 4];
    let pad = 2.0;
    let inner = (size as f32 - pad * 2.0).max(1.0);
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let u = ((fx - pad) / inner).clamp(0.0, 1.0);
            let v = ((fy - pad) / inner).clamp(0.0, 1.0);
            let edge = (fx - pad)
                .min(fy - pad)
                .min((pad + inner) - fx)
                .min((pad + inner) - fy);
            let cover = (edge / 1.25).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let (r, g, b, a) = if let Some(tile) = tile.as_ref() {
                sample_bilinear(tile, u, v)
            } else {
                (base[0] * 255.0, base[1] * 255.0, base[2] * 255.0, 255.0)
            };
            let i = (y * size + x) * 4;
            rgba[i] = r.clamp(0.0, 255.0) as u8;
            rgba[i + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[i + 2] = b.clamp(0.0, 255.0) as u8;
            rgba[i + 3] = (a * cover).clamp(0.0, 255.0) as u8;
        }
    }
    ColorImage::from_rgba_unmultiplied([size, size], &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_slot_uses_blockbench_faces() {
        let pack = Pack::builtin();
        let (shape, gui) = pack.slot_model("grass").expect("grass slot model");
        assert!(
            !shape.faces().is_empty(),
            "grass must expose Blockbench faces for UI"
        );
        assert_eq!(gui.rotation, ItemDisplay::GUI_BLOCK.rotation);
        let img = render("grass", 64);
        assert_eq!(img.width(), 64);
        let opaque = img.pixels.iter().filter(|c| c.a() > 200).count();
        assert!(opaque > 64 * 64 / 8, "grass icon should be solid, got {opaque}");
        // Real grass textures are green on top / sides — not a flat single hue stub.
        let greenish = img
            .pixels
            .iter()
            .filter(|c| c.a() > 200 && c.g() > c.r() && c.g() > 80)
            .count();
        assert!(
            greenish > 40,
            "expected grass-green texels from the model, got {greenish}"
        );
        let brownish = img
            .pixels
            .iter()
            .filter(|c| c.a() > 200 && c.r() > c.g() && c.r() > 60)
            .count();
        assert!(
            brownish > 20,
            "grass model sides should show dirt brown, got {brownish}"
        );
    }

    #[test]
    fn cobble_slot_uses_gui_block_display() {
        let (_, gui) = Pack::builtin()
            .slot_model("cobblestone")
            .expect("cobble");
        assert_eq!(gui, ItemDisplay::GUI_BLOCK);
        let img = render("cobblestone", 64);
        let opaque = img.pixels.iter().filter(|c| c.a() > 200).count();
        assert!(opaque > 64 * 64 / 6);
    }

    #[test]
    fn coin_item_model_still_resolves() {
        let (shape, _) = Pack::builtin().slot_model("coin").expect("coin");
        assert!(!shape.boxes().is_empty());
        let img = render("coin", 64);
        assert!(img.pixels.iter().any(|c| c.a() > 200));
    }
}
