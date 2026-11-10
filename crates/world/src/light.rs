//! Colored voxel light.
//!
//! A pack block can emit a color at a level from 1 to 15. Each channel
//! spreads through air and through shapes that are not full cubes, dropping
//! together so the color stays the same hue until the cell is dark. A full
//! cube stops it. The sun and its shadows are applied when meshing. There is
//! no path trace. The number in a cell is how much of that channel is left,
//! from 0 to 15.

use std::collections::VecDeque;

use crate::chunk::Block;
use crate::cluster::Cluster;
use crate::pack::{Light, light_of};
use crate::shape_of;

/// A level of 15 is the brightest pack light, and each step drops one.
pub const LIGHT_REACH: i32 = 15;

/// A light that is not stored as a block — a held torch, lantern, or similar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub light: Light,
}
/// Fixed point for one level. Dim channels stay in ratio with the peak
/// instead of rounding to zero and leaving a pure red fringe.
const UNIT: u16 = 256;

const NEIGHBORS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Attenuated light for every cell in a box. Each channel is `0..=15`.
#[derive(Clone)]
pub struct LightField {
    min_x: i32,
    min_z: i32,
    width: i32,
    depth: i32,
    cells: Vec<[u16; 3]>,
}

impl LightField {
    /// Light for `world`.
    ///
    /// The field covers chunks that emit, plus 15 blocks, which is as far as
    /// a level-15 light can travel. Terrain that does not emit stays outside
    /// the field, so filling the small square does not allocate a 256-tall
    /// box over every chunk.
    pub fn solve(world: &Cluster) -> Self {
        Self::solve_with(world, &[])
    }

    /// Like [`solve`](Self::solve), and also seeds `extras` (a held torch, etc.).
    ///
    /// Extra emitters still stop at full cubes and fall off like pack lights.
    /// A world with no block lights still gets a field when something is held.
    pub fn solve_with(world: &Cluster, extras: &[Emitter]) -> Self {
        let (world_min_x, world_max_x, world_min_z, world_max_z) = world.bounds();
        let mut min_x = world_max_x;
        let mut min_z = world_max_z;
        let mut max_x = world_min_x;
        let mut max_z = world_min_z;
        let mut emitters = Vec::new();
        for (index, chunk) in world.chunks().iter().enumerate() {
            if !chunk.any_block(|block| light_of(block).is_some()) {
                continue;
            }
            emitters.push(index);
            let x0 = chunk.x() * 16;
            let z0 = chunk.z() * 16;
            min_x = min_x.min(x0);
            min_z = min_z.min(z0);
            max_x = max_x.max(x0 + 16);
            max_z = max_z.max(z0 + 16);
        }
        for extra in extras {
            min_x = min_x.min(extra.x);
            min_z = min_z.min(extra.z);
            max_x = max_x.max(extra.x + 1);
            max_z = max_z.max(extra.z + 1);
        }
        if emitters.is_empty() && extras.is_empty() {
            return Self::vacant();
        }
        let min_x = (min_x - LIGHT_REACH).max(world_min_x);
        let min_z = (min_z - LIGHT_REACH).max(world_min_z);
        let max_x = (max_x + LIGHT_REACH).min(world_max_x);
        let max_z = (max_z + LIGHT_REACH).min(world_max_z);
        let mut field = Self::allocate(min_x, max_x, min_z, max_z);
        let mut queue = VecDeque::new();
        for index in emitters {
            world.chunks()[index].for_each_block(|x, y, z, block| {
                let Some(light) = light_of(block) else {
                    return;
                };
                if field.raise(x, y, z, emission_fp(light.channels())) {
                    queue.push_back([x, y, z]);
                }
            });
        }
        for extra in extras {
            if field.raise(
                extra.x,
                extra.y,
                extra.z,
                emission_fp(extra.light.channels()),
            ) {
                queue.push_back([extra.x, extra.y, extra.z]);
            }
        }
        field.spread(&mut queue, |x, y, z| world.block_at(x, y, z));
        field
    }

    pub fn solve_bounds(
        min_x: i32,
        max_x: i32,
        min_z: i32,
        max_z: i32,
        sample: impl Fn(i32, i32, i32) -> Block,
    ) -> Self {
        let mut field = Self::allocate(min_x, max_x, min_z, max_z);
        let mut queue = VecDeque::new();
        for y in 0..256 {
            for z in min_z..max_z {
                for x in min_x..max_x {
                    let block = sample(x, y, z);
                    if block.is_air() {
                        continue;
                    }
                    let Some(light) = light_of(block) else {
                        continue;
                    };
                    if field.raise(x, y, z, emission_fp(light.channels())) {
                        queue.push_back([x, y, z]);
                    }
                }
            }
        }
        field.spread(&mut queue, sample);
        field
    }

    fn vacant() -> Self {
        Self {
            min_x: 0,
            min_z: 0,
            width: 0,
            depth: 0,
            cells: Vec::new(),
        }
    }

    fn allocate(min_x: i32, max_x: i32, min_z: i32, max_z: i32) -> Self {
        let width = (max_x - min_x).max(0);
        let depth = (max_z - min_z).max(0);
        let count = (width as usize) * (depth as usize) * 256;
        Self {
            min_x,
            min_z,
            width,
            depth,
            cells: vec![[0, 0, 0]; count],
        }
    }

    fn spread(&mut self, queue: &mut VecDeque<[i32; 3]>, sample: impl Fn(i32, i32, i32) -> Block) {
        while let Some([x, y, z]) = queue.pop_front() {
            let here = self.sample_fp(x, y, z);
            let next = attenuate(here);
            if next == [0, 0, 0] {
                continue;
            }
            for offset in NEIGHBORS {
                let nx = x + offset[0];
                let ny = y + offset[1];
                let nz = z + offset[2];
                if self.index(nx, ny, nz).is_none() {
                    continue;
                }
                if blocks_light(sample(nx, ny, nz)) {
                    continue;
                }
                if self.raise(nx, ny, nz, next) {
                    queue.push_back([nx, ny, nz]);
                }
            }
        }
    }

    /// Channels left at this cell, `0..=15`. Outside the field, all zero.
    pub fn channels(&self, x: i32, y: i32, z: i32) -> [u8; 3] {
        quantize(self.sample_fp(x, y, z))
    }

    fn sample_fp(&self, x: i32, y: i32, z: i32) -> [u16; 3] {
        match self.index(x, y, z) {
            Some(index) => self.cells[index],
            None => [0, 0, 0],
        }
    }

    fn raise(&mut self, x: i32, y: i32, z: i32, value: [u16; 3]) -> bool {
        let Some(index) = self.index(x, y, z) else {
            return false;
        };
        let cell = &mut self.cells[index];
        let mut improved = false;
        for channel in 0..3 {
            if value[channel] > cell[channel] {
                cell[channel] = value[channel];
                improved = true;
            }
        }
        improved
    }

    #[cfg(test)]
    fn column_count(&self) -> i64 {
        i64::from(self.width) * i64::from(self.depth)
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if !(0..256).contains(&y) {
            return None;
        }
        let lx = x - self.min_x;
        let lz = z - self.min_z;
        if lx < 0 || lz < 0 || lx >= self.width || lz >= self.depth {
            return None;
        }
        Some(((y as usize * self.depth as usize) + lz as usize) * self.width as usize + lx as usize)
    }
}

fn blocks_light(block: Block) -> bool {
    !block.is_air() && shape_of(block).is_full_cube()
}

fn emission_fp(channels: [u8; 3]) -> [u16; 3] {
    channels.map(|channel| u16::from(channel) * UNIT)
}

/// Drop one level, scaling every channel by the same fraction of the peak.
/// Independent subtraction made the lantern's green hit zero while red was
/// still high, which painted a red fringe on grass at the chunk edge.
fn attenuate(here: [u16; 3]) -> [u16; 3] {
    let peak = here[0].max(here[1]).max(here[2]);
    if peak <= UNIT {
        return [0, 0, 0];
    }
    let next_peak = u32::from(peak - UNIT);
    let peak = u32::from(peak);
    here.map(|channel| ((u32::from(channel) * next_peak) / peak) as u16)
}

fn quantize(fp: [u16; 3]) -> [u8; 3] {
    let peak = fp[0].max(fp[1]).max(fp[2]);
    if peak == 0 {
        return [0, 0, 0];
    }
    let mut out = [0u8; 3];
    for i in 0..3 {
        if fp[i] == 0 {
            continue;
        }
        let rounded = (u32::from(fp[i]) + u32::from(UNIT) / 2) / u32::from(UNIT);
        let mut level = u8::try_from(rounded).unwrap_or(15).min(15);
        // A channel that is still a real share of this cell stays visible.
        // Rounding it to zero is what turned the lantern red at the fringe.
        if level == 0 && u32::from(fp[i]) * 8 >= u32::from(peak) {
            level = 1;
        }
        out[i] = level;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::Chunk;
    use crate::fixture::synthetic_cluster;
    use crate::shape::{LANTERN, SIGN};

    fn world(blocks: impl IntoIterator<Item = (u8, u16, u8, Block)>) -> Cluster {
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn a_torch_lights_the_air_beside_it_and_stops_at_stone() {
        let field = LightField::solve(&world([
            (1, 1, 1, Block::new("torch", 0)),
            (1, 1, 2, Block::new("stone", 0)),
        ]));
        assert_eq!(field.channels(1, 1, 1), [14, 9, 2]);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[0] > 0 && beside[0] < 14,
            "air beside the torch: {beside:?}"
        );
        assert!(
            beside[1] > 0,
            "the torch stays yellow, not a red fringe: {beside:?}"
        );
        assert_eq!(field.channels(1, 1, 2), [0, 0, 0]);
    }

    #[test]
    fn glowstone_lights_the_air_beside_it_and_stops_at_stone() {
        let field = LightField::solve(&world([
            (1, 1, 1, Block::new("glowstone", 0)),
            (1, 1, 2, Block::new("stone", 0)),
        ]));
        assert_eq!(field.channels(1, 1, 1), [15, 12, 3]);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[0] > 0 && beside[0] < 15,
            "air beside the glowstone: {beside:?}"
        );
        assert!(beside[1] > 0, "the glow stays yellow: {beside:?}");
        assert_eq!(field.channels(1, 1, 2), [0, 0, 0]);
    }

    #[test]
    fn an_end_rod_lights_the_air_with_a_cool_white_glow() {
        let field = LightField::solve(&world([
            (1, 1, 1, Block::new("end_rod", 0)),
            (1, 1, 2, Block::new("stone", 0)),
        ]));
        assert_eq!(field.channels(1, 1, 1), [10, 12, 14]);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[2] > beside[0] && beside[2] < 14,
            "air beside the end rod stays cool: {beside:?}"
        );
        assert_eq!(field.channels(1, 1, 2), [0, 0, 0]);
        let hung = LightField::solve(&world([(1, 1, 1, Block::new("end_rod", 1))]));
        assert_eq!(hung.channels(1, 1, 1), [10, 12, 14]);
    }

    #[test]
    fn an_end_crystal_lights_the_air_with_a_purple_glow() {
        let field = LightField::solve(&world([
            (1, 1, 1, Block::new("end_crystal", 0)),
            (1, 1, 2, Block::new("stone", 0)),
        ]));
        assert_eq!(field.channels(1, 1, 1), [12, 6, 15]);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[2] > beside[1] && beside[2] < 15,
            "air beside the crystal stays purple: {beside:?}"
        );
        assert_eq!(field.channels(1, 1, 2), [0, 0, 0]);
    }

    #[test]
    fn a_sea_lantern_lights_the_air_with_a_cool_glow() {
        let field = LightField::solve(&world([
            (1, 1, 1, Block::new("sea_lantern", 0)),
            (1, 1, 2, Block::new("stone", 0)),
        ]));
        assert_eq!(field.channels(1, 1, 1), [4, 11, 15]);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[2] > beside[0] && beside[2] < 15,
            "air beside the sea lantern stays cyan: {beside:?}"
        );
        assert_eq!(field.channels(1, 1, 2), [0, 0, 0]);
    }

    #[test]
    fn a_lit_pumpkin_lights_the_air_and_a_plain_one_does_not() {
        let dark = LightField::solve(&world([(1, 1, 1, Block::new("pumpkin", 1))]));
        assert_eq!(dark.channels(2, 1, 1), [0, 0, 0]);
        let field = LightField::solve(&world([(1, 1, 1, Block::new("pumpkin", 2))]));
        assert!(field.channels(1, 1, 1)[0] > 0);
        let beside = field.channels(2, 1, 1);
        assert!(
            beside[0] > 0 && beside[0] < 14,
            "air beside the lantern: {beside:?}"
        );
    }

    #[test]
    fn light_falls_off_with_distance() {
        let field = LightField::solve(&world([(1, 1, 1, Block::new(LANTERN, 0))]));
        let at = field.channels(1, 1, 1);
        let step1 = field.channels(1, 1, 2);
        let step4 = field.channels(1, 1, 5);
        let beyond = field.channels(1, 1, 14);
        assert!(at[0] > step1[0]);
        assert!(step1[0] > step4[0]);
        assert!(step4[0] > 0);
        assert_eq!(beyond, [0, 0, 0]);
        assert!(at[0] > at[1]);
        assert_eq!(at[2], 0);
    }

    #[test]
    fn dark_terrain_does_not_widen_the_light_field() {
        let mut cluster = synthetic_cluster();
        let before = LightField::solve(&cluster).column_count();
        assert!(before > 0);
        let grass = Chunk::from_blocks(14, 14, [(1, 4, 1, Block::new("grass", 0))]).unwrap();
        cluster.replace_chunk(grass).unwrap();
        let after = LightField::solve(&cluster).column_count();
        assert_eq!(after, before);
    }

    #[test]
    fn a_full_cube_blocks_light_and_a_sign_does_not() {
        let open = LightField::solve(&world([(1, 1, 1, Block::new(LANTERN, 0))]));
        let walled = LightField::solve(&world([
            (1, 1, 1, Block::new(LANTERN, 0)),
            (2, 1, 1, Block::new("stone", 0)),
        ]));
        assert_eq!(walled.channels(2, 1, 1), [0, 0, 0]);
        assert!(open.channels(3, 1, 1)[0] > walled.channels(3, 1, 1)[0]);

        let through = LightField::solve(&world([
            (1, 1, 1, Block::new(LANTERN, 0)),
            (2, 1, 1, Block::new(SIGN, 0)),
        ]));
        assert!(through.channels(2, 1, 1)[0] > 0);
        assert_eq!(through.channels(3, 1, 1)[0], open.channels(3, 1, 1)[0]);
    }

    #[test]
    fn a_copper_grate_lets_light_through() {
        let open = LightField::solve(&world([(1, 1, 1, Block::new(LANTERN, 0))]));
        let through = LightField::solve(&world([
            (1, 1, 1, Block::new(LANTERN, 0)),
            (2, 1, 1, Block::new("copper_grate", 0)),
        ]));
        assert!(through.channels(2, 1, 1)[0] > 0);
        assert_eq!(through.channels(3, 1, 1)[0], open.channels(3, 1, 1)[0]);
        let walled = LightField::solve(&world([
            (1, 1, 1, Block::new(LANTERN, 0)),
            (2, 1, 1, Block::new("stone", 0)),
        ]));
        assert!(through.channels(3, 1, 1)[0] > walled.channels(3, 1, 1)[0]);
    }

    #[test]
    fn ember_fill_in_a_grate_emits_warm_light() {
        use crate::chunk::FILL_EMBER;
        let dark = LightField::solve(&world([(2, 1, 2, Block::new("copper_grate", 0))]));
        assert_eq!(dark.channels(2, 1, 2)[0], 0);
        let lit = LightField::solve(&world([(
            2,
            1,
            2,
            Block::new("copper_grate", 0).with_fill(FILL_EMBER),
        )]));
        assert!(
            lit.channels(2, 1, 2)[0] > 0,
            "ember fill should light its own cell"
        );
        assert!(
            lit.channels(3, 1, 2)[0] > 0,
            "ember fill should reach a neighbor"
        );
        assert!(lit.channels(2, 1, 2)[0] > lit.channels(2, 1, 2)[2]);
    }

    #[test]
    fn light_does_not_cross_a_chunk_onto_unlit_cells() {
        let grass = Block::new("grass", 0);
        let mut near = Vec::new();
        let mut far = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                near.push((x, 0, z, grass));
                far.push((x, 0, z, grass));
            }
        }
        near.push((0, 1, 8, Block::new(LANTERN, 0)));
        let cluster = Cluster::try_from_chunks(
            0,
            0,
            [
                Chunk::from_blocks(0, 0, near).unwrap(),
                Chunk::from_blocks(1, 0, far).unwrap(),
            ],
        )
        .unwrap();
        let field = LightField::solve(&cluster);
        let mut leaked = Vec::new();
        for z in 0..32 {
            for y in 0..6 {
                for x in 12..32 {
                    let got = field.channels(x, y, z);
                    if got != [0, 0, 0] {
                        leaked.push((x, y, z, got));
                    }
                }
            }
        }
        assert!(
            leaked.is_empty(),
            "light past reach: {leaked:?} lantern {:?}",
            field.channels(0, 1, 8)
        );

        let mut walled_near = Vec::new();
        let mut walled_far = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                walled_near.push((x, 0, z, grass));
                walled_far.push((x, 0, z, grass));
            }
            walled_near.push((15, 1, z, Block::new("stone", 0)));
        }
        walled_near.push((13, 1, 8, Block::new(LANTERN, 0)));
        let walled = LightField::solve(
            &Cluster::try_from_chunks(
                0,
                0,
                [
                    Chunk::from_blocks(0, 0, walled_near).unwrap(),
                    Chunk::from_blocks(1, 0, walled_far).unwrap(),
                ],
            )
            .unwrap(),
        );
        let mut open_near = Vec::new();
        let mut open_far = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                open_near.push((x, 0, z, grass));
                open_far.push((x, 0, z, grass));
            }
        }
        open_near.push((13, 1, 8, Block::new(LANTERN, 0)));
        let open = LightField::solve(
            &Cluster::try_from_chunks(
                0,
                0,
                [
                    Chunk::from_blocks(0, 0, open_near).unwrap(),
                    Chunk::from_blocks(1, 0, open_far).unwrap(),
                ],
            )
            .unwrap(),
        );
        assert!(
            open.channels(16, 1, 8)[0] > walled.channels(16, 1, 8)[0],
            "wall on the chunk edge did not block: open {:?} walled {:?}",
            open.channels(16, 1, 8),
            walled.channels(16, 1, 8)
        );
    }

    #[test]
    fn colored_light_keeps_its_hue_until_it_ends() {
        let field = LightField::solve(&world([(1, 1, 1, Block::new(LANTERN, 0))]));
        let mut saw_lit = false;
        for step in 0..16 {
            let got = field.channels(1, 1, 1 + step);
            if got == [0, 0, 0] {
                assert!(saw_lit, "lantern cell was dark");
                for farther in step..16 {
                    assert_eq!(
                        field.channels(1, 1, 1 + farther),
                        [0, 0, 0],
                        "light came back at step {farther}"
                    );
                }
                break;
            }
            saw_lit = true;
            assert!(
                got[1] > 0 && got[0] >= got[1] && got[2] == 0,
                "step {step} lost the lantern hue: {got:?}"
            );
        }
    }

    #[test]
    fn the_lantern_by_the_sign_lights_the_sign_and_fades() {
        let field = LightField::solve(&synthetic_cluster());
        let lamp = field.channels(6, 1, 14);
        let beside = field.channels(7, 1, 14);
        let sign = field.channels(7, 1, 13);
        let far = field.channels(6, 1, 1);
        assert!(lamp[0] > beside[0]);
        assert!(beside[0] > sign[0]);
        assert!(sign[0] > 0);
        assert!(sign[0] > far[0]);
        assert_eq!(far, [0, 0, 0]);
    }

    #[test]
    fn a_held_torch_lights_air_with_no_placed_emitter() {
        let cluster = world([(1, 1, 1, Block::new("stone", 0))]);
        let dark = LightField::solve(&cluster);
        assert_eq!(dark.channels(4, 2, 4), [0, 0, 0]);
        let torch = light_of(Block::new("torch", 0)).expect("torch");
        let held = LightField::solve_with(
            &cluster,
            &[Emitter {
                x: 4,
                y: 2,
                z: 4,
                light: torch,
            }],
        );
        assert_eq!(held.channels(4, 2, 4), torch.channels());
        let beside = held.channels(5, 2, 4);
        assert!(
            beside[0] > 0 && beside[0] < torch.channels()[0],
            "air beside a held torch: {beside:?}"
        );
    }

    #[test]
    fn clearing_a_held_light_leaves_the_air_dark() {
        let cluster = world([(8, 1, 8, Block::new("stone", 0))]);
        let lantern = light_of(Block::new("lantern", 0)).expect("lantern");
        let lit = LightField::solve_with(
            &cluster,
            &[Emitter {
                x: 3,
                y: 2,
                z: 3,
                light: lantern,
            }],
        );
        assert!(lit.channels(3, 2, 3)[0] > 0);
        let dark = LightField::solve_with(&cluster, &[]);
        assert_eq!(dark.channels(3, 2, 3), [0, 0, 0]);
    }
}
