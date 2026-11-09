//! A few rays around the sun or moon.
//!
//! The world is still drawn by the raster pass. This pass asks the blocks
//! whether a short cone of rays stays clear, and keeps the average on a coarse
//! grid around the player. The fragment shader reads that grid per texel.
//! The answer is not stored on block corners. A crease eases darker than a
//! shape a block away, and the shader reads the sample in front of the face.
//!
//! Each frame traces one batch, not the whole grid. The rays stop at 8 blocks,
//! and the shadow map still answers anything farther away. The same probes
//! store how open the sky is and how near a shape sits. Those two are filtered
//! across the grid. They are not written onto block corners.

use rebuild_world::{Cluster, shape_of};

pub const GRID_X: usize = 12;
pub const GRID_Y: usize = 6;
pub const GRID_Z: usize = 12;
pub const SPACING: f32 = 2.0;
const BATCH: usize = 48;
const REACH: f32 = 8.0;
const STEPS: usize = 16;
/// How far a sky ray looks, and how near a shape has to be to count as contact.
const SKY_REACH: f32 = 5.0;
const CONTACT: f32 = 1.6;
const SKY_DIRS: [[f32; 3]; 5] = [
    [0.0, 1.0, 0.0],
    [0.65, 0.7, 0.0],
    [-0.65, 0.7, 0.0],
    [0.0, 0.7, 0.65],
    [0.0, 0.7, -0.65],
];
/// Sideways lean of each ray, in blocks per block of travel.
const CONE: f32 = 0.45;

const COUNT: usize = GRID_X * GRID_Y * GRID_Z;

pub struct Volume {
    origin: [i32; 3],
    /// Sun visibility, sky openness, and contact. Each is 0..=255.
    values: Vec<[u8; 3]>,
    scratch: Vec<[u8; 3]>,
    pad: Vec<u8>,
    cursor: usize,
}

impl Volume {
    pub fn new() -> Self {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        Self {
            origin: [0, 0, 0],
            values: vec![[255, 255, 255]; COUNT],
            scratch: vec![[255, 255, 255]; COUNT],
            pad: vec![0; align * GRID_Y * GRID_Z],
            cursor: 0,
        }
    }

    /// World position the shader treats as probe `(0, 0, 0)`.
    pub fn sample_origin(&self) -> [f32; 3] {
        probe_point(self.origin, 0, 0, 0)
    }

    /// Rows padded so a 3D upload accepts them. Visibility sits in the red byte.
    /// The buffer is reused, so a frame does not allocate it again.
    pub fn upload(&mut self) -> &[u8] {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        for z in 0..GRID_Z {
            for y in 0..GRID_Y {
                let src = index(0, y, z);
                let dst = (z * GRID_Y + y) * align;
                for x in 0..GRID_X {
                    let sample = self.values[src + x];
                    self.pad[dst + x * 4] = sample[0];
                    self.pad[dst + x * 4 + 1] = sample[1];
                    self.pad[dst + x * 4 + 2] = sample[2];
                }
            }
        }
        &self.pad
    }

    /// Trace the next batch toward `light`. A step of the player keeps the
    /// probes that still cover the same cells.
    pub fn advance(&mut self, world: &Cluster, eye: [f32; 3], light: [f32; 3]) {
        let next = snap_origin(eye);
        if next != self.origin {
            self.shift(next);
        }
        for _ in 0..BATCH {
            let cursor = self.cursor;
            self.cursor = (self.cursor + 1) % COUNT;
            let (ix, iy, iz) = unravel(cursor);
            let point = nearest_air(world, probe_point(self.origin, ix, iy, iz));
            let vis = soft_visibility(world, point, light);
            let shade = sky_shade(world, point);
            self.values[cursor] = [quant(vis), quant(shade.sky), quant(shade.contact)];
        }
    }

    fn shift(&mut self, next: [i32; 3]) {
        let spacing = SPACING as i32;
        let delta = [
            (self.origin[0] - next[0]) / spacing,
            (self.origin[1] - next[1]) / spacing,
            (self.origin[2] - next[2]) / spacing,
        ];
        self.scratch.fill([255, 255, 255]);
        for iz in 0..GRID_Z {
            for iy in 0..GRID_Y {
                for ix in 0..GRID_X {
                    let ox = ix as i32 + delta[0];
                    let oy = iy as i32 + delta[1];
                    let oz = iz as i32 + delta[2];
                    if (0..GRID_X as i32).contains(&ox)
                        && (0..GRID_Y as i32).contains(&oy)
                        && (0..GRID_Z as i32).contains(&oz)
                    {
                        self.scratch[index(ix, iy, iz)] =
                            self.values[index(ox as usize, oy as usize, oz as usize)];
                    }
                }
            }
        }
        std::mem::swap(&mut self.values, &mut self.scratch);
        self.origin = next;
    }
}

/// Average of four rays leaned off `dir`. `1` is open, `0` is blocked.
/// A sample that starts inside a shape stays open, so a buried probe does not
/// darken the air beside it.
pub fn soft_visibility(world: &Cluster, origin: [f32; 3], dir: [f32; 3]) -> f32 {
    if solid(world, origin[0], origin[1], origin[2]) {
        return 1.0;
    }
    let dir = unit(dir);
    let (tangent, bitangent) = basis(dir);
    let taps = [[CONE, 0.0], [-CONE, 0.0], [0.0, CONE], [0.0, -CONE]];
    let mut sum = 0.0;
    for [u, v] in taps {
        let ray = unit([
            dir[0] + tangent[0] * u + bitangent[0] * v,
            dir[1] + tangent[1] * u + bitangent[1] * v,
            dir[2] + tangent[2] * u + bitangent[2] * v,
        ]);
        if ray_clear(world, origin, ray) {
            sum += 1.0;
        }
    }
    sum / taps.len() as f32
}

/// One ray. True when it clears `REACH` blocks without entering a shape.
pub fn ray_clear(world: &Cluster, origin: [f32; 3], dir: [f32; 3]) -> bool {
    ray_distance(world, origin, dir, REACH).is_none()
}

/// Sky openness and how near a shape sits. A sample inside a shape stays open
/// so a buried probe does not darken the air beside it.
pub struct Shade {
    pub sky: f32,
    pub contact: f32,
}

pub fn sky_shade(world: &Cluster, origin: [f32; 3]) -> Shade {
    if solid(world, origin[0], origin[1], origin[2]) {
        return Shade {
            sky: 1.0,
            contact: 1.0,
        };
    }
    let mut sky = 0.0;
    let mut nearest = SKY_REACH;
    for dir in SKY_DIRS {
        match ray_distance(world, origin, dir, SKY_REACH) {
            None => sky += 1.0,
            Some(dist) => {
                sky += (dist / SKY_REACH).clamp(0.0, 1.0);
                if dist < nearest {
                    nearest = dist;
                }
            }
        }
    }
    Shade {
        sky: sky / SKY_DIRS.len() as f32,
        contact: ease_contact(nearest),
    }
}

/// A crease is dark. A shape a block away only eases the light. Open air stays open.
fn ease_contact(nearest: f32) -> f32 {
    let t = ((nearest - 0.35) / (CONTACT - 0.35)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ray_distance(world: &Cluster, origin: [f32; 3], dir: [f32; 3], reach: f32) -> Option<f32> {
    let dir = unit(dir);
    if solid(world, origin[0], origin[1], origin[2]) {
        return Some(0.0);
    }
    let mut t = 0.08;
    for _ in 0..STEPS {
        if t > reach {
            return None;
        }
        let point = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        if solid(world, point[0], point[1], point[2]) {
            return Some(t);
        }
        let step = next_boundary(point[0], dir[0])
            .min(next_boundary(point[1], dir[1]))
            .min(next_boundary(point[2], dir[2]));
        t += step.max(1.0e-3) + 1.0e-3;
    }
    None
}

fn quant(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

fn snap_origin(eye: [f32; 3]) -> [i32; 3] {
    let spacing = SPACING as i32;
    let half = [
        (GRID_X as i32 / 2) * spacing,
        (GRID_Y as i32 / 2) * spacing,
        (GRID_Z as i32 / 2) * spacing,
    ];
    let snap = |value: f32| (value / SPACING).floor() as i32 * spacing;
    [
        snap(eye[0]) - half[0],
        snap(eye[1]) - half[1],
        snap(eye[2]) - half[2],
    ]
}

fn probe_point(origin: [i32; 3], ix: usize, iy: usize, iz: usize) -> [f32; 3] {
    [
        origin[0] as f32 + ix as f32 * SPACING + 0.5,
        origin[1] as f32 + iy as f32 * SPACING + 0.5,
        origin[2] as f32 + iz as f32 * SPACING + 0.5,
    ]
}

fn index(ix: usize, iy: usize, iz: usize) -> usize {
    (iz * GRID_Y + iy) * GRID_X + ix
}

fn unravel(cursor: usize) -> (usize, usize, usize) {
    let ix = cursor % GRID_X;
    let rest = cursor / GRID_X;
    let iy = rest % GRID_Y;
    let iz = rest / GRID_Y;
    (ix, iy, iz)
}

/// A probe that starts inside a block reads the nearest air instead, so a
/// wide blur does not pull full sun out of the inside of a cobble or a hill.
fn nearest_air(world: &Cluster, point: [f32; 3]) -> [f32; 3] {
    if !solid(world, point[0], point[1], point[2]) {
        return point;
    }
    for dy in 1..4 {
        let y = point[1] + dy as f32;
        if !solid(world, point[0], y, point[2]) {
            return [point[0], y, point[2]];
        }
    }
    for [dx, dz] in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
        let p = [point[0] + dx, point[1], point[2] + dz];
        if !solid(world, p[0], p[1], p[2]) {
            return p;
        }
    }
    point
}

fn solid(world: &Cluster, x: f32, y: f32, z: f32) -> bool {
    let iy = y.floor() as i32;
    if !(0..256).contains(&iy) {
        return false;
    }
    let block = world.block_at(x.floor() as i32, iy, z.floor() as i32);
    if block.is_air() {
        return false;
    }
    !shape_of(block).is_empty()
}

fn next_boundary(pos: f32, dir: f32) -> f32 {
    if dir.abs() < 1.0e-8 {
        return f32::INFINITY;
    }
    let frac = pos - pos.floor();
    if dir > 0.0 {
        (1.0 - frac).max(1.0e-4) / dir
    } else {
        frac.max(1.0e-4) / -dir
    }
}

fn basis(dir: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let helper = if dir[1].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let tangent = unit(cross(helper, dir));
    let bitangent = unit(cross(dir, tangent));
    (tangent, bitangent)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1.0e-6 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk};

    fn world(blocks: Vec<(u8, u16, u8, Block)>) -> Cluster {
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap()
    }

    #[test]
    fn a_cone_softens_a_wall_the_center_ray_hits() {
        let open = world(vec![]);
        let at = [0.5, 2.5, 0.5];
        assert!(ray_clear(&open, at, [0.0, 1.0, 0.0]));
        assert!((soft_visibility(&open, at, [0.0, 1.0, 0.0]) - 1.0).abs() < 1.0e-4);

        let mut roof = Vec::new();
        for x in 1..4u8 {
            for z in 1..4u8 {
                roof.push((x, 4, z, Block::new("stone", 0)));
                roof.push((x, 5, z, Block::new("stone", 0)));
            }
        }
        let roof = world(roof);
        let under = [2.5, 2.5, 2.5];
        assert!(!ray_clear(&roof, under, [0.0, 1.0, 0.0]));
        assert!(soft_visibility(&roof, under, [0.0, 1.0, 0.0]) < 0.01);
        assert!(
            (soft_visibility(&roof, [2.5, 4.5, 2.5], [0.0, 1.0, 0.0]) - 1.0).abs() < 1.0e-4,
            "a probe inside stone does not darken its neighbors"
        );

        // A wall along z blocks the center and the sideways rays, and the
        // rays leaned up and down walk past it.
        let mut wall = Vec::new();
        for z in 0..6u8 {
            wall.push((5, 2, z, Block::new("stone", 0)));
        }
        let wall = world(wall);
        assert!(!ray_clear(&wall, at, [1.0, 0.0, 0.0]));
        let soft = soft_visibility(&wall, at, [1.0, 0.0, 0.0]);
        assert!(
            soft > 0.2 && soft < 0.8,
            "the cone is partly open, visibility {soft}"
        );
    }

    #[test]
    fn sky_and_contact_darken_without_a_corner() {
        let open = world(vec![]);
        let at = [2.5, 2.5, 2.5];
        let clear = sky_shade(&open, at);
        assert!((clear.sky - 1.0).abs() < 1.0e-4);
        assert!((clear.contact - 1.0).abs() < 1.0e-4);

        let mut roof = Vec::new();
        for x in 1..5u8 {
            for z in 1..5u8 {
                roof.push((x, 3, z, Block::new("stone", 0)));
            }
        }
        let roof = world(roof);
        let under = sky_shade(&roof, at);
        assert!(under.sky < 0.45, "a low roof closes the sky, {}", under.sky);
        assert!(
            under.contact < 0.7,
            "the roof is close enough to count, {}",
            under.contact
        );
        let buried = sky_shade(&roof, [2.5, 3.5, 2.5]);
        assert!((buried.sky - 1.0).abs() < 1.0e-4);
        assert!((buried.contact - 1.0).abs() < 1.0e-4);

        let mut wall = Vec::new();
        for y in 1..5u16 {
            wall.push((3, y, 2, Block::new("stone", 0)));
        }
        let wall = world(wall);
        let beside = sky_shade(&wall, at);
        assert!(
            beside.sky > 0.55,
            "the sky above a side wall stays open, {}",
            beside.sky
        );
        assert!(
            beside.contact < 0.75,
            "the wall softens the ground beside it, {}",
            beside.contact
        );
        assert!(ease_contact(0.2) < ease_contact(1.2));
        assert!((ease_contact(SKY_REACH) - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn a_frame_traces_a_short_batch_on_a_near_grid() {
        assert!(
            GRID_X as f32 * SPACING >= 20.0 && GRID_Z as f32 * SPACING >= 20.0,
            "the soft light still covers the ground around you"
        );
        assert!(GRID_Y as f32 * SPACING >= 10.0);
        assert!(COUNT <= 1000);
        assert!(BATCH <= 64);
        assert!(REACH <= 8.0);
        let mut chunks = Vec::new();
        for z in -2..=2 {
            for x in -2..=2 {
                chunks.push(rebuild_world::generate_chunk(x, z));
            }
        }
        let world = Cluster::try_from_chunks(0, 0, chunks).unwrap();
        let light = [0.25_f32, 1.0, -0.72];
        let mut volume = Volume::new();
        volume.advance(&world, [8.0, 5.0, 14.0], light);
        let start = std::time::Instant::now();
        for step in 0..30 {
            let x = 8.0 + step as f32 * 0.08;
            volume.advance(&world, [x, 5.0, 14.0], light);
            let _ = volume.upload();
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / 30.0;
        assert!(
            ms < 1.0,
            "a debug frame of tracing stays interactive, {ms:.3} ms"
        );
    }

    #[test]
    fn the_grid_follows_the_player_and_keeps_old_probes() {
        let open = world(vec![]);
        let mut volume = Volume::new();
        volume.advance(&open, [8.0, 4.0, 8.0], [0.0, 1.0, 0.0]);
        assert!(volume.values.iter().any(|value| value[0] == 255));
        volume.values[0] = [10, 40, 80];
        let before = volume.origin;
        volume.advance(&open, [10.0, 4.0, 8.0], [0.0, 1.0, 0.0]);
        assert_eq!(volume.origin[0], before[0] + SPACING as i32);
        assert_eq!(volume.values[index(1, 0, 0)], [10, 40, 80]);
        let padded = volume.upload();
        assert_eq!(padded.len(), 256 * GRID_Y * GRID_Z);
        assert_eq!(padded[4], 10);
        assert_eq!(padded[5], 40);
        assert_eq!(padded[6], 80);
    }
}
