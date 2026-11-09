//! Hold left click to break the outlined block.
//!
//! Progress follows the pack hardness, in seconds. Letting go or looking away
//! clears it. The block's faces are drawn darker by `kept_light` until the
//! break lands.

use glam::Vec3;
use rebuild_world::{Cluster, for_each_collider, shape_of};

use crate::mesh::Vertex;

/// How much of the block color remains. `1` is untouched, `0` is black.
#[cfg_attr(not(test), allow(dead_code))]
pub fn kept_light(progress: f32) -> f32 {
    1.0 - progress.clamp(0.0, 1.0)
}

/// Four crack stages. The face stays put, then jumps darker, instead of fading.
///
/// `1` is untouched. A full break is `0`.
pub fn crack_light(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return 1.0;
    }
    let stage = (progress * 4.0).ceil().min(4.0);
    1.0 - stage / 4.0
}

#[derive(Clone, Copy, Debug)]
pub struct Dig {
    held: bool,
    target: Option<[i32; 3]>,
    progress: f32,
}

impl Dig {
    pub fn new() -> Self {
        Self {
            held: false,
            target: None,
            progress: 0.0,
        }
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    pub fn set_held(&mut self, held: bool) {
        self.held = held;
        if !held {
            self.clear();
        }
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn target(&self) -> Option<[i32; 3]> {
        self.target
    }

    /// Advance the hold. `hardness` is the looked-at block's seconds, when it has one.
    ///
    /// Returns the cell that just finished breaking.
    pub fn tick(
        &mut self,
        dt: f32,
        looked_at: Option<[i32; 3]>,
        hardness: Option<f32>,
    ) -> Option<[i32; 3]> {
        let Some(cell) = looked_at else {
            self.clear();
            return None;
        };
        if !self.held {
            self.clear();
            return None;
        }
        let Some(seconds) = hardness.filter(|seconds| seconds.is_finite() && *seconds > 0.0) else {
            self.clear();
            return None;
        };
        if self.target != Some(cell) {
            self.target = Some(cell);
            self.progress = 0.0;
        }
        self.progress = (self.progress + dt.max(0.0) / seconds).min(1.0);
        if self.progress >= 1.0 {
            self.clear();
            return Some(cell);
        }
        None
    }

    fn clear(&mut self) {
        self.target = None;
        self.progress = 0.0;
    }
}

impl Default for Dig {
    fn default() -> Self {
        Self::new()
    }
}

/// Triangles just off the looked-at shape, so a darken blend covers that block.
pub fn break_faces(cell: [i32; 3], world: &Cluster) -> Vec<Vertex> {
    let block = world.block_at(cell[0], cell[1], cell[2]);
    let shape = shape_of(block);
    if shape.is_empty() {
        return Vec::new();
    }
    let origin = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
    let mut faces = Vec::new();
    for_each_collider(block, |min, max| {
        let min = origin + Vec3::new(min[0], min[1], min[2]) / 16.0 - Vec3::splat(FACE_PAD);
        let max = origin + Vec3::new(max[0], max[1], max[2]) / 16.0 + Vec3::splat(FACE_PAD);
        push_box(&mut faces, min, max);
    });
    faces
}

const FACE_PAD: f32 = 0.002;

fn push_box(out: &mut Vec<Vertex>, min: Vec3, max: Vec3) {
    let corner = [
        [min.x, min.y, min.z],
        [max.x, min.y, min.z],
        [max.x, max.y, min.z],
        [min.x, max.y, min.z],
        [min.x, min.y, max.z],
        [max.x, min.y, max.z],
        [max.x, max.y, max.z],
        [min.x, max.y, max.z],
    ];
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [3, 2, 6, 7],
        [0, 3, 7, 4],
        [1, 2, 6, 5],
    ];
    for face in FACES {
        let tri = [face[0], face[1], face[2], face[0], face[2], face[3]];
        for index in tri {
            out.push(Vertex {
                position: corner[index],
                color: [1.0, 1.0, 1.0],
                normal: [0.0, 1.0, 0.0],
                uv: [0.0, 0.0],
                textured: 0.0,
                block: [0.0, 0.0, 0.0],
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk, Cluster, Pack};

    fn world(blocks: &[(u8, u16, u8, Block)]) -> Cluster {
        let chunk = Chunk::from_blocks(0, 0, blocks.iter().copied()).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn grass_is_faster_than_stone_and_ores() {
        let pack = Pack::builtin();
        let grass = pack.hardness_of(Block::new("grass", 0)).unwrap();
        let stone = pack.hardness_of(Block::new("stone", 0)).unwrap();
        let coal = pack.hardness_of(Block::new("coal_ore", 0)).unwrap();
        let iron = pack.hardness_of(Block::new("iron_ore", 0)).unwrap();
        assert_eq!(grass, 0.25);
        assert_eq!(stone, 1.5);
        assert_eq!(coal, 2.0);
        assert_eq!(iron, 2.5);
        assert!(grass < stone && grass < coal && grass < iron);

        let mut dig = Dig::new();
        dig.set_held(true);
        assert_eq!(
            dig.tick(grass, Some([8, 0, 14]), Some(grass)),
            Some([8, 0, 14])
        );
        assert_eq!(dig.progress(), 0.0);

        let mut dig = Dig::new();
        dig.set_held(true);
        assert!(dig.tick(grass, Some([0, 1, 0]), Some(stone)).is_none());
        assert!(dig.progress() < 0.2);
        assert!(dig.tick(stone, Some([0, 1, 0]), Some(stone)).is_some());

        let iron = pack.mine_factor("iron_pickaxe", "stone");
        assert_eq!(iron, 6.0);
        let mut fast = Dig::new();
        fast.set_held(true);
        let sped = stone / iron;
        assert!(fast.tick(sped, Some([0, 1, 0]), Some(sped)).is_some());
    }

    #[test]
    fn release_or_looking_away_resets_progress() {
        let mut dig = Dig::new();
        dig.set_held(true);
        assert!(dig.tick(0.5, Some([1, 2, 3]), Some(1.5)).is_none());
        assert!((dig.progress() - 0.5 / 1.5).abs() < 1.0e-5);
        dig.set_held(false);
        assert_eq!(dig.progress(), 0.0);
        assert!(dig.target().is_none());
        assert!(dig.tick(1.5, Some([1, 2, 3]), Some(1.5)).is_none());

        dig.set_held(true);
        dig.tick(0.5, Some([1, 2, 3]), Some(1.5));
        assert!(dig.tick(0.1, None, None).is_none());
        assert_eq!(dig.progress(), 0.0);
        assert!(dig.tick(0.2, Some([4, 2, 3]), Some(1.5)).is_none());
        assert!((dig.progress() - 0.2 / 1.5).abs() < 1.0e-5);
        assert_eq!(dig.target(), Some([4, 2, 3]));
    }

    #[test]
    fn the_held_block_darkens_until_it_breaks() {
        let mut dig = Dig::new();
        dig.set_held(true);
        assert!((kept_light(0.0) - 1.0).abs() < 1.0e-5);
        dig.tick(0.5, Some([0, 0, 0]), Some(1.0));
        assert!((dig.progress() - 0.5).abs() < 1.0e-5);
        assert!((kept_light(dig.progress()) - 0.5).abs() < 1.0e-5);
        assert_eq!(dig.tick(0.5, Some([0, 0, 0]), Some(1.0)), Some([0, 0, 0]));
        assert_eq!(kept_light(dig.progress()), 1.0);

        let grass = world(&[(0, 0, 0, Block::new("grass", 0))]);
        let faces = break_faces([0, 0, 0], &grass);
        assert_eq!(faces.len(), 36, "six faces of two triangles");
        assert!(faces.iter().all(|vertex| vertex.textured == 0.0));

        let stairs = world(&[(0, 1, 0, Block::new("oak_stairs", 0))]);
        assert_eq!(break_faces([0, 1, 0], &stairs).len(), 72);
        let sign = world(&[(7, 1, 13, Block::new("sign", 0))]);
        let sign_faces = break_faces([7, 1, 13], &sign);
        assert_eq!(sign_faces.len(), 72);
        let min_y = sign_faces
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        assert!(min_y < 1.05, "the sign post darkens down to the grass");
        assert!(break_faces([1, 1, 1], &grass).is_empty());
    }

    #[test]
    fn cracks_step_darker_instead_of_fading() {
        assert!((crack_light(0.0) - 1.0).abs() < 1.0e-5);
        assert!((crack_light(0.1) - 0.75).abs() < 1.0e-5);
        assert!((crack_light(0.5) - 0.5).abs() < 1.0e-5);
        assert!((crack_light(0.6) - 0.25).abs() < 1.0e-5);
        assert!(crack_light(0.9).abs() < 1.0e-5);
        assert!(crack_light(1.0).abs() < 1.0e-5);
    }
}
