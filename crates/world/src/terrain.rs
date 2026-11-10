//! Biome terrain outside the demo pad.
//!
//! The center 3×3 chunks stay the synthetic scene. Every other chunk in the
//! finite square is plains, forest, hills, or marsh. The height comes from a
//! value noise of the world coordinates, so the same chunk is the same ground
//! on the client and on the authority. Stone under the crust opens into caves
//! you can walk into. On short ground a mouth dips a block and rings itself
//! in deepslate, so the dark hole reads from the surface. The bottom layer stays stone, so a cave never drops
//! through the finite square. One camp sits in the chunk just east and north
//! of the demo pad: ruined cobblestone, a campfire, and a chest. A timbered
//! mine mouth sits west and south of the pad. A ruined stone-brick shrine sits
//! southeast of the pad with a loot chest. Marsh columns hold a water block on top of the
//! dirt. The surface stays in the bottom section. A pump fills one chunk per
//! call, so a large square does not hitch a frame.

use std::collections::VecDeque;
use std::time::Duration;

/// One generated chunk per call, even when the frame still has time left.
const CHUNKS_PER_PUMP: usize = 1;

use crate::chunk::{Block, Chunk};
use crate::cluster::Cluster;

/// Chunks this close to the cluster center, in chunk coordinates, are the demo.
const DEMO_RADIUS: i32 = 1;

/// True for the center 3×3. Those chunks keep the synthetic scene.
pub fn is_demo_pad(cluster: &Cluster, x: i32, z: i32) -> bool {
    let (center_x, center_z) = cluster.center();
    (x - center_x).abs() <= DEMO_RADIUS && (z - center_z).abs() <= DEMO_RADIUS
}

/// The ring just outside the demo pad, so the first frame already has ground.
pub fn seed_terrain_ring(cluster: &mut Cluster) {
    let (center_x, center_z) = cluster.center();
    let radius = cluster.size().chunk_radius();
    for dz in -2i32..=2 {
        for dx in -2i32..=2 {
            if dx.abs().max(dz.abs()) != 2 {
                continue;
            }
            if dx.abs() > radius || dz.abs() > radius {
                continue;
            }
            let _ = cluster.replace_chunk(generate_chunk(center_x + dx, center_z + dz));
        }
    }
}

/// If `(x, z)` is an empty chunk outside the demo pad, fill it.
/// A chunk that already holds blocks is left alone, so a saved edit stays.
pub fn ensure_terrain(cluster: &mut Cluster, x: i32, z: i32) {
    if is_demo_pad(cluster, x, z) {
        return;
    }
    let Some(existing) = cluster.get(x, z) else {
        return;
    };
    if existing.non_air_count() > 0 {
        return;
    }
    let _ = cluster.replace_chunk(generate_chunk(x, z));
}

/// Empty chunks outside the demo pad, nearest to the center first.
pub struct TerrainPump {
    pending: VecDeque<(i32, i32)>,
}

impl TerrainPump {
    pub fn new(cluster: &Cluster) -> Self {
        let (center_x, center_z) = cluster.center();
        let mut pending: Vec<(i32, i32)> = cluster
            .chunks()
            .iter()
            .filter(|chunk| {
                chunk.non_air_count() == 0 && !is_demo_pad(cluster, chunk.x(), chunk.z())
            })
            .map(|chunk| (chunk.x(), chunk.z()))
            .collect();
        pending.sort_by_key(|(x, z)| (x - center_x).abs().max((z - center_z).abs()));
        Self {
            pending: pending.into(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.pending.len()
    }

    /// The next empty chunk, nearest the center. The caller generates it.
    /// The frame thread uses this so `generate_chunk` can run on a worker.
    pub fn pop(&mut self) -> Option<(i32, i32)> {
        self.pending.pop_front()
    }

    /// Fill one chunk. A spent `budget` still finishes that chunk, and a long
    /// budget does not start a second one. Returns whether a chunk was written.
    pub fn pump(&mut self, cluster: &mut Cluster, budget: Duration) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let started = std::time::Instant::now();
        let mut wrote = false;
        let mut steps = 0usize;
        while let Some((x, z)) = self.pending.pop_front() {
            if cluster
                .get(x, z)
                .is_some_and(|chunk| chunk.non_air_count() == 0)
            {
                let _ = cluster.replace_chunk(generate_chunk(x, z));
                wrote = true;
            }
            steps += 1;
            if steps >= CHUNKS_PER_PUMP || started.elapsed() >= budget {
                break;
            }
        }
        wrote
    }
}

/// One chunk of terrain. The surface stays in the bottom section.
pub fn generate_chunk(chunk_x: i32, chunk_z: i32) -> Chunk {
    let mut blocks = Vec::with_capacity(16 * 16 * 8);
    for local_z in 0..16u8 {
        for local_x in 0..16u8 {
            let x = chunk_x * 16 + i32::from(local_x);
            let z = chunk_z * 16 + i32::from(local_z);
            let biome = biome_at(x, z);
            let height = column_height(x, z, biome);
            let mouth = is_mouth_column(x, z);
            let rim = !mouth && biome != Biome::Marsh && beside_mouth(x, z);
            for y in 0..height {
                let block = column_block(x, z, y, height, biome, mouth, rim);
                if block.is_air() {
                    continue;
                }
                blocks.push((local_x, y, local_z, block));
            }
            if biome == Biome::Marsh {
                blocks.push((local_x, height, local_z, Block::new("water", 0)));
            }
            if !mouth && !rim && biome == Biome::Plains && hash2(x, z) % 37 == 0 {
                blocks.push((local_x, height, local_z, Block::new("flower", 0)));
            }
            if !mouth && !rim && biome == Biome::Forest && tree_at(x, z) {
                plant_tree(&mut blocks, local_x, local_z, height);
            }
        }
    }
    stamp_camp(&mut blocks, chunk_x, chunk_z);
    stamp_shaft(&mut blocks, chunk_x, chunk_z);
    stamp_shrine(&mut blocks, chunk_x, chunk_z);
    stamp_clay_bank(&mut blocks, chunk_x, chunk_z);
    Chunk::from_blocks(chunk_x, chunk_z, blocks).expect("terrain coordinates stay in the chunk")
}

/// Campfire block, just east and north of the demo pad.
///
/// The chunk is `(2, -2)`. Walk east past the pad, then north. The south wall
/// has a gap facing the stairs.
pub const CAMP_FIRE: (i32, i32) = (39, -25);

/// The chest two blocks south of the campfire.
pub const CAMP_CHEST: (i32, i32) = (CAMP_FIRE.0, CAMP_FIRE.1 - 2);

/// The smoker two blocks west of the campfire.
pub const CAMP_SMOKER: (i32, i32) = (CAMP_FIRE.0 - 2, CAMP_FIRE.1);

/// Note block east of the fire, for pitch cycling with a rising mote.
pub const CAMP_NOTE: (i32, i32) = (CAMP_FIRE.0 + 2, CAMP_FIRE.1 + 2);

/// Hanging sign in the south gate, pointing the path north of camp.
pub const CAMP_SIGN: (i32, i32) = (CAMP_FIRE.0, CAMP_FIRE.1 + 3);

/// Jukebox east of the fire (between the log and the chest side).
pub const CAMP_JUKEBOX: (i32, i32) = (CAMP_FIRE.0 + 2, CAMP_FIRE.1 - 1);

/// Composter southwest of the fire (south of the smoker).
pub const CAMP_COMPOST: (i32, i32) = (CAMP_FIRE.0 - 2, CAMP_FIRE.1 - 2);

const CAMP_CHUNK: (i32, i32) = (2, -2);

/// A ruined ring, a campfire, and a chest. Only the camp chunk gets one.
fn stamp_camp(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != CAMP_CHUNK {
        return;
    }
    let (fire_x, fire_z) = CAMP_FIRE;
    let floor = column_height(fire_x, fire_z, biome_at(fire_x, fire_z));
    let x0 = 4u8;
    let x1 = 10u8;
    let z0 = 4u8;
    let z1 = 10u8;
    blocks
        .retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    for lz in z0..=z1 {
        for lx in x0..=x1 {
            let top = blocks
                .iter()
                .filter(|(x, _, z, _)| *x == lx && *z == lz)
                .map(|(_, y, _, _)| *y)
                .max()
                .unwrap_or(0);
            for y in (top + 1)..floor.saturating_sub(1) {
                blocks.push((lx, y, lz, Block::new("dirt", 0)));
            }
            blocks.push((lx, floor - 1, lz, Block::new("grass", 0)));
        }
    }
    let cobble = Block::new("cobblestone", 0);
    for lz in z0..=z1 {
        for lx in x0..=x1 {
            let edge = lx == x0 || lx == x1 || lz == z0 || lz == z1;
            let gate = lz == z1 && lx == 7;
            let missing = (u16::from(lx) + u16::from(lz)) % 5 == 0;
            if !edge || gate || missing {
                continue;
            }
            blocks.push((lx, floor, lz, cobble));
            let corner = (lx == x0 || lx == x1) && (lz == z0 || lz == z1);
            if corner {
                blocks.push((lx, floor + 1, lz, cobble));
            }
        }
    }
    let local_x = (fire_x - chunk_x * 16) as u8;
    let local_z = (fire_z - chunk_z * 16) as u8;
    blocks.push((local_x, floor, local_z, Block::new("campfire", 0)));
    blocks.push((local_x, floor, local_z - 2, Block::new("chest", 0)));
    blocks.push((local_x + 2, floor, local_z, Block::new("oak_log", 0)));
    // West of the fire: hold raw and use it for a one-step cook.
    blocks.push((local_x - 2, floor, local_z, Block::new("smoker", 0)));
    // Southeast of the fire: use to cycle pitch and see a mote.
    blocks.push((local_x + 2, floor, local_z + 2, Block::new("note_block", 0)));
    // South gate lintel height: hanging sign with the north-path tip.
    blocks.push((local_x, floor + 1, local_z + 3, Block::new("hanging_sign", 0)));
    // East of the fire: insert a music disc to play.
    blocks.push((local_x + 2, floor, local_z - 1, Block::new("jukebox", 0)));
    // Southwest of the fire: fill plants for bone meal.
    blocks.push((local_x - 2, floor, local_z - 2, Block::new("composter", 0)));
    // A flower beside it so the fill loop is findable without creative give.
    blocks.push((local_x - 1, floor, local_z - 2, Block::new("flower", 0)));
}

/// The open pit of the mine mouth, west and south of the demo pad.
///
/// The chunk is `(-2, 2)`. Walk west past the pad, then south. Oak posts and a
/// lintel face east, toward the stairs. Coal sits in the hole. A chest sits on
/// the gravel apron just east of the mouth.
pub const SHAFT_MOUTH: (i32, i32) = (-24, 40);

/// The mine-mouth chest on the gravel apron, two blocks east of the pit center.
pub const SHAFT_CHEST: (i32, i32) = (SHAFT_MOUTH.0 + 2, SHAFT_MOUTH.1);

/// The blast furnace on the apron, one block south of the chest.
pub const SHAFT_BLAST: (i32, i32) = (SHAFT_CHEST.0, SHAFT_CHEST.1 + 1);

const SHAFT_CHUNK: (i32, i32) = (-2, 2);

/// Broken stone-brick shrine, southeast of the demo pad.
///
/// The chunk is `(2, 2)`. Walk east past the pad, then south. A chest sits on
/// the altar under the broken walls.
pub const SHRINE: (i32, i32) = (40, 40);

/// The shrine chest one block south of the altar center.
pub const SHRINE_CHEST: (i32, i32) = (SHRINE.0, SHRINE.1 - 1);

/// The furnace two blocks east of the altar center.
pub const SHRINE_FURNACE: (i32, i32) = (SHRINE.0 + 2, SHRINE.1);

/// The crafting table one block north of the shrine furnace.
pub const SHRINE_TABLE: (i32, i32) = (SHRINE_FURNACE.0, SHRINE_FURNACE.1 - 1);

/// Bell west of the altar pedestal.
pub const SHRINE_BELL: (i32, i32) = (SHRINE.0 - 2, SHRINE.1);

/// Lectern with a guidebook, north of the altar.
pub const SHRINE_LECTERN: (i32, i32) = (SHRINE.0, SHRINE.1 + 2);

const SHRINE_CHUNK: (i32, i32) = (2, 2);

/// Marsh clay bank south of the demo pad, under the shallow water.
///
/// The chunk is `(0, 2)`. Walk south from the stairs into the marsh. Break
/// clay under the water, then bake it at the shrine furnace.
pub const CLAY_BANK: (i32, i32) = (2, 41);

const CLAY_CHUNK: (i32, i32) = (0, 2);

/// A ruined stone-brick square with a chest. Only the shrine chunk gets one.
fn stamp_shrine(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != SHRINE_CHUNK {
        return;
    }
    let (cx, cz) = SHRINE;
    let floor = column_height(cx, cz, biome_at(cx, cz));
    let x0 = 5u8;
    let x1 = 11u8;
    let z0 = 5u8;
    let z1 = 11u8;
    blocks
        .retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    for lz in z0..=z1 {
        for lx in x0..=x1 {
            let top = blocks
                .iter()
                .filter(|(x, _, z, _)| *x == lx && *z == lz)
                .map(|(_, y, _, _)| *y)
                .max()
                .unwrap_or(0);
            for y in (top + 1)..floor.saturating_sub(1) {
                blocks.push((lx, y, lz, Block::new("dirt", 0)));
            }
            blocks.push((lx, floor - 1, lz, Block::new("grass", 0)));
        }
    }
    let brick = Block::new("stone_bricks", 0);
    let mossy = Block::new("mossy_stone_bricks", 0);
    let cracked = Block::new("cracked_stone_bricks", 0);
    let chisel = Block::new("chiseled_stone_bricks", 0);
    // Paved floor.
    for lz in (z0 + 1)..=(z1 - 1) {
        for lx in (x0 + 1)..=(x1 - 1) {
            let tile = if (u16::from(lx) + u16::from(lz)) % 3 == 0 {
                mossy
            } else {
                brick
            };
            blocks.push((lx, floor - 1, lz, tile));
        }
    }
    // Broken ring walls, taller at the corners.
    for lz in z0..=z1 {
        for lx in x0..=x1 {
            let edge = lx == x0 || lx == x1 || lz == z0 || lz == z1;
            if !edge {
                continue;
            }
            let gate = lz == z1 && (lx == 7 || lx == 8);
            let missing = (u16::from(lx) * 3 + u16::from(lz)) % 7 == 0;
            if gate || missing {
                continue;
            }
            let corner = (lx == x0 || lx == x1) && (lz == z0 || lz == z1);
            let tall = if corner { 3u16 } else { 2 };
            for dy in 0..tall {
                let block = if dy == 0 && corner {
                    chisel
                } else if (u16::from(lx) + dy) % 2 == 0 {
                    cracked
                } else {
                    brick
                };
                blocks.push((lx, floor + dy, lz, block));
            }
        }
    }
    // Altar pedestal and chest.
    let local_x = (cx - chunk_x * 16) as u8;
    let local_z = (cz - chunk_z * 16) as u8;
    blocks.push((local_x, floor, local_z, chisel));
    blocks.push((local_x, floor, local_z - 1, Block::new("chest", 0)));
    blocks.push((local_x + 1, floor, local_z, Block::new("torch", 0)));
    blocks.push((local_x - 1, floor, local_z, mossy));
    // Offering on the altar: what bricks + clay make at the table.
    blocks.push((local_x, floor + 1, local_z, Block::new("decorated_pot", 0)));
    // East of the altar: hold clay and bake bricks in one use.
    let furnace_x = (SHRINE_FURNACE.0 - chunk_x * 16) as u8;
    let furnace_z = (SHRINE_FURNACE.1 - chunk_z * 16) as u8;
    blocks.push((furnace_x, floor, furnace_z, Block::new("furnace", 0)));
    // A clay block beside it so the bake loop is findable without creative give.
    blocks.push((furnace_x, floor, furnace_z + 1, Block::new("clay", 0)));
    // North of the furnace: bricks + clay shape a decorated pot in one use.
    let table_x = (SHRINE_TABLE.0 - chunk_x * 16) as u8;
    let table_z = (SHRINE_TABLE.1 - chunk_z * 16) as u8;
    blocks.push((table_x, floor, table_z, Block::new("crafting_table", 0)));
    // West of the altar: ring for a high note + mote.
    let bell_x = (SHRINE_BELL.0 - chunk_x * 16) as u8;
    let bell_z = (SHRINE_BELL.1 - chunk_z * 16) as u8;
    blocks.push((bell_x, floor, bell_z, Block::new("bell", 0)));
    // North of the altar: open the guidebook and turn pages.
    let lectern_x = (SHRINE_LECTERN.0 - chunk_x * 16) as u8;
    let lectern_z = (SHRINE_LECTERN.1 - chunk_z * 16) as u8;
    blocks.push((lectern_x, floor, lectern_z, Block::new("lectern", 1)));
}

/// Clay under marsh water south of the pad. Only the clay chunk gets it.
fn stamp_clay_bank(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != CLAY_CHUNK {
        return;
    }
    let (cx, cz) = CLAY_BANK;
    let clay = Block::new("clay", 0);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let x = cx + dx;
            let z = cz + dz;
            if biome_at(x, z) != Biome::Marsh {
                continue;
            }
            let height = column_height(x, z, Biome::Marsh);
            if height == 0 {
                continue;
            }
            let lx = (x - chunk_x * 16) as u8;
            let lz = (z - chunk_z * 16) as u8;
            let floor = height - 1;
            blocks.retain(|(bx, by, bz, _)| !(*bx == lx && *by == floor && *bz == lz));
            blocks.push((lx, floor, lz, clay));
        }
    }
}

/// A timbered hole. Only the shaft chunk gets one.
fn stamp_shaft(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != SHAFT_CHUNK {
        return;
    }
    let (mouth_x, mouth_z) = SHAFT_MOUTH;
    let floor = column_height(mouth_x, mouth_z, biome_at(mouth_x, mouth_z));
    let x0 = 5u8;
    let x1 = 11u8;
    let z0 = 5u8;
    let z1 = 11u8;
    blocks
        .retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    for lz in z0..=z1 {
        for lx in x0..=x1 {
            let top = blocks
                .iter()
                .filter(|(x, _, z, _)| *x == lx && *z == lz)
                .map(|(_, y, _, _)| *y)
                .max()
                .unwrap_or(0);
            for y in (top + 1)..floor.saturating_sub(1) {
                blocks.push((lx, y, lz, Block::new("dirt", 0)));
            }
            blocks.push((lx, floor - 1, lz, Block::new("grass", 0)));
        }
    }
    blocks.retain(|(x, y, z, _)| {
        let hole = (6..=8).contains(x) && (7..=8).contains(z);
        !(hole && *y > 0)
    });
    let coal = Block::new("coal_ore", 0);
    blocks.push((6, 1, 7, coal));
    blocks.push((6, 1, 8, coal));
    let gravel = Block::new("gravel", 0);
    blocks.retain(|(x, y, z, _)| {
        let apron = (10..=11).contains(x) && (7..=8).contains(z) && *y + 1 == floor;
        !apron
    });
    for lz in 7..=8u8 {
        for lx in 10..=11u8 {
            blocks.push((lx, floor - 1, lz, gravel));
        }
    }
    let log = Block::new("oak_log", 0);
    for lz in [6u8, 9] {
        blocks.push((9, floor, lz, log));
        blocks.push((9, floor + 1, lz, log));
    }
    blocks.push((9, floor + 1, 7, log));
    blocks.push((9, floor + 1, 8, log));
    let chest_x = (SHAFT_CHEST.0 - chunk_x * 16) as u8;
    let chest_z = (SHAFT_CHEST.1 - chunk_z * 16) as u8;
    blocks.push((chest_x, floor, chest_z, Block::new("chest", 0)));
    // South of the chest: hold cobble and use it for a one-step blast.
    let blast_x = (SHAFT_BLAST.0 - chunk_x * 16) as u8;
    let blast_z = (SHAFT_BLAST.1 - chunk_z * 16) as u8;
    blocks.push((blast_x, floor, blast_z, Block::new("blast_furnace", 0)));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Hills,
    Marsh,
}

/// Plains, forest, hills, or marsh. The noise cell is 32 blocks, the same on every world size.
pub fn biome_at(x: i32, z: i32) -> Biome {
    let n = value_noise(x, z, 32);
    if n < 0.32 {
        Biome::Marsh
    } else if n < 0.55 {
        Biome::Plains
    } else if n < 0.78 {
        Biome::Forest
    } else {
        Biome::Hills
    }
}

fn column_height(x: i32, z: i32, biome: Biome) -> u16 {
    let n = value_noise(x, z, 8);
    let height = match biome {
        Biome::Marsh => 3,
        Biome::Plains => 4 + (n > 0.72) as u16,
        Biome::Forest => 4 + (n > 0.4) as u16,
        Biome::Hills => 5 + (n * 7.0) as u16,
    };
    height.clamp(3, 12)
}

fn column_block(
    x: i32,
    z: i32,
    y: u16,
    height: u16,
    biome: Biome,
    mouth: bool,
    rim: bool,
) -> Block {
    if y == 0 {
        return Block::new("stone", 0);
    }
    if is_open_cave(x, y, z, height) || (mouth && y < height) {
        return Block::AIR;
    }
    if rim && y + 1 == height {
        return Block::AIR;
    }
    if rim && y + 2 == height {
        return Block::new("deepslate", 0);
    }
    if y + 1 == height {
        return match biome {
            Biome::Marsh => Block::new("dirt", 1),
            Biome::Hills if height >= 9 => Block::new("stone", 0),
            Biome::Hills if hash2(x, z) % 5 == 0 => Block::new("gravel", 0),
            Biome::Hills => Block::new("grass", 2),
            Biome::Forest => Block::new("grass", 1),
            Biome::Plains => Block::new("grass", 0),
        };
    }
    if y + 3 >= height {
        return Block::new("dirt", 0);
    }
    if let Some(id) = vein(x, y, z) {
        return Block::new(id, 0);
    }
    Block::new("stone", 0)
}

fn tree_at(x: i32, z: i32) -> bool {
    hash2(x, z) % 23 == 0
}

/// A short trunk and two leaves on top. The leaves stay in this column.
fn plant_tree(blocks: &mut Vec<(u8, u16, u8, Block)>, x: u8, z: u8, height: u16) {
    for dy in 0..3u16 {
        blocks.push((x, height + dy, z, Block::new("oak_log", 0)));
    }
    let leaf = Block::new("leaves", 0);
    blocks.push((x, height + 3, z, leaf));
    blocks.push((x, height + 4, z, leaf));
}

/// Sky and horizon multiplier. Plains leave the daylight color alone.
pub fn sky_tint(biome: Biome) -> [f32; 3] {
    match biome {
        Biome::Plains => [1.0, 1.0, 1.0],
        Biome::Forest => [0.72, 0.92, 0.78],
        Biome::Hills => [0.82, 0.88, 1.0],
        Biome::Marsh => [0.78, 0.90, 0.68],
    }
}

/// A roofed room under the top two blocks. `y == 0` is never a cave.
fn is_cave(x: i32, y: i32, z: i32) -> bool {
    y > 0 && hash3(x.div_euclid(4), y.div_euclid(2), z.div_euclid(4)) % 5 == 0
}

/// Cave air under the top two blocks. A surface mouth is handled beside this.
fn is_open_cave(x: i32, y: u16, z: i32, height: u16) -> bool {
    y + 2 < height && is_cave(x, i32::from(y), z)
}

/// Short ground over a cave. The column is open from the floor up to the sky.
fn is_mouth_column(x: i32, z: i32) -> bool {
    let height = column_height(x, z, biome_at(x, z));
    (4..=5).contains(&height) && hash2(x, z) % 9 == 0 && is_cave(x, 1, z)
}

/// A column touching a mouth. Its top block is missing and the new top is deepslate.
fn beside_mouth(x: i32, z: i32) -> bool {
    for dz in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            if is_mouth_column(x + dx, z + dz) {
                return true;
            }
        }
    }
    false
}

/// Coal in pairs through the stone. Iron in pairs in the bottom three layers.
fn vein(x: i32, y: u16, z: i32) -> Option<&'static str> {
    let h = hash3(x.div_euclid(2), i32::from(y).div_euclid(2), z.div_euclid(2));
    if y <= 3 && h % 31 == 0 {
        return Some("iron_ore");
    }
    if h % 23 == 0 {
        return Some("coal_ore");
    }
    None
}

fn hash3(x: i32, y: i32, z: i32) -> u32 {
    let mut n = hash2(x, z);
    n = n.wrapping_mul(2_246_822_519) ^ (y as u32).wrapping_mul(3_266_489_917);
    n ^ (n >> 13)
}

fn value_noise(x: i32, z: i32, period: i32) -> f32 {
    let x0 = x.div_euclid(period);
    let z0 = z.div_euclid(period);
    let fx = x.rem_euclid(period) as f32 / period as f32;
    let fz = z.rem_euclid(period) as f32 / period as f32;
    let fade = |t: f32| t * t * (3.0 - 2.0 * t);
    let u = fade(fx);
    let v = fade(fz);
    let sample = |ix: i32, iz: i32| (hash2(ix, iz) & 255) as f32 / 255.0;
    let n00 = sample(x0, z0);
    let n10 = sample(x0 + 1, z0);
    let n01 = sample(x0, z0 + 1);
    let n11 = sample(x0 + 1, z0 + 1);
    let nx0 = n00 + (n10 - n00) * u;
    let nx1 = n01 + (n11 - n01) * u;
    nx0 + (nx1 - nx0) * v
}

fn hash2(x: i32, z: i32) -> u32 {
    let mut n = (x as u32).wrapping_mul(374761393) ^ (z as u32).wrapping_mul(668265263);
    n = n.wrapping_mul(1274126177);
    n ^ (n >> 13)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::synthetic_cluster;
    use crate::size::WorldSize;

    #[test]
    fn the_demo_pad_stays_and_a_short_pump_does_not_fill_the_square() {
        let mut cluster = synthetic_cluster();
        let demo = cluster.get(0, 0).unwrap().non_air_count();
        seed_terrain_ring(&mut cluster);
        assert!(cluster.get(2, 0).unwrap().non_air_count() > 0);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
        let mut pump = TerrainPump::new(&cluster);
        let pending = pump.remaining();
        assert!(pending > 100, "most of the small square is still empty");
        assert!(pump.pump(&mut cluster, Duration::from_millis(50)));
        assert_eq!(pump.remaining(), pending - 1);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
        assert!(cluster.size() == WorldSize::Small);
    }

    #[test]
    fn a_worker_generates_the_same_chunk_the_pump_would() {
        let mut cluster = synthetic_cluster();
        let mut pump = TerrainPump::new(&cluster);
        let before = pump.remaining();
        let (x, z) = pump.pop().expect("an empty chunk");
        assert_eq!(pump.remaining(), before - 1);
        let generated = std::thread::spawn(move || generate_chunk(x, z))
            .join()
            .expect("worker");
        let again = generate_chunk(x, z);
        assert_eq!(generated.non_air_count(), again.non_air_count());
        assert!(generated.non_air_count() > 100);
        assert_eq!(generated.block(4, 2, 4), again.block(4, 2, 4));
        cluster.replace_chunk(generated).unwrap();
        assert!(cluster.get(x, z).unwrap().non_air_count() > 100);
        let demo = cluster.get(0, 0).unwrap().non_air_count();
        ensure_terrain(&mut cluster, 0, 0);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
    }

    #[test]
    fn biomes_change_the_surface_past_the_pad() {
        let mut grass = false;
        let mut dirt = false;
        let mut stone = false;
        let mut logs = false;
        for z in (-12i32..12).step_by(3) {
            for x in (-12i32..12).step_by(3) {
                if x.abs() <= 1 && z.abs() <= 1 {
                    continue;
                }
                let chunk = generate_chunk(x, z);
                for local_z in 0..16u8 {
                    for local_x in 0..16u8 {
                        for y in 0..16u16 {
                            match chunk.block(local_x, y, local_z).id {
                                "grass" => grass = true,
                                "dirt" => dirt = true,
                                "stone" => stone = true,
                                "oak_log" => logs = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(grass, "plains or forest keep a grass top");
        assert!(dirt, "marsh tops are dirt");
        assert!(stone, "hills expose stone");
        assert!(logs, "forest chunks plant oak logs");
        let mut leaves = false;
        let mut flowers = false;
        let mut gravel = false;
        let mut forest_grass = false;
        let mut hill_grass = false;
        for z in (-12i32..12).step_by(3) {
            for x in (-12i32..12).step_by(3) {
                if x.abs() <= 1 && z.abs() <= 1 {
                    continue;
                }
                let chunk = generate_chunk(x, z);
                for local_z in 0..16u8 {
                    for local_x in 0..16u8 {
                        for y in 0..16u16 {
                            let block = chunk.block(local_x, y, local_z);
                            match (block.id, block.metadata) {
                                ("leaves", _) => leaves = true,
                                ("flower", _) => flowers = true,
                                ("gravel", _) => gravel = true,
                                ("grass", 1) => forest_grass = true,
                                ("grass", 2) => hill_grass = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(leaves, "forest trees carry leaves");
        assert!(flowers, "plains scatter flowers");
        assert!(gravel, "hills scatter gravel");
        assert!(forest_grass, "forest grass is its own tint");
        assert!(hill_grass, "hill grass is its own tint");
        assert_eq!(sky_tint(Biome::Plains), [1.0, 1.0, 1.0]);
        assert!(sky_tint(Biome::Forest)[1] > sky_tint(Biome::Forest)[0]);
        assert!(sky_tint(Biome::Hills)[2] > sky_tint(Biome::Hills)[0]);
        assert!(sky_tint(Biome::Marsh)[1] > sky_tint(Biome::Marsh)[2]);
        assert_eq!(biome_at(40, 24), biome_at(40, 24));
    }

    #[test]
    fn a_camp_stands_just_outside_the_demo_pad() {
        let camp = generate_chunk(2, -2);
        let mut fire = None;
        let mut chest = false;
        let mut wall = false;
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                for y in 0..16u16 {
                    match camp.block(local_x, y, local_z).id {
                        "campfire" => fire = Some((local_x, y, local_z)),
                        "chest" => chest = true,
                        "cobblestone" => wall = true,
                        _ => {}
                    }
                }
            }
        }
        let (x, y, z) = fire.expect("campfire");
        assert_eq!((2 * 16 + i32::from(x), -2 * 16 + i32::from(z)), CAMP_FIRE);
        assert_eq!(camp.block(x, y - 1, z).id, "grass");
        assert!(chest, "a chest sits in the camp");
        assert!(wall, "ruined cobblestone rings the camp");
        assert_eq!(camp.block(x, y, z - 2).id, "chest");
        assert_eq!(
            (2 * 16 + i32::from(x), -2 * 16 + i32::from(z - 2)),
            CAMP_CHEST
        );
        assert_eq!(camp.block(x - 2, y, z).id, "smoker");
        assert_eq!(
            (2 * 16 + i32::from(x - 2), -2 * 16 + i32::from(z)),
            CAMP_SMOKER
        );
        assert_eq!(camp.block(x + 2, y, z + 2).id, "note_block");
        assert_eq!(
            (2 * 16 + i32::from(x + 2), -2 * 16 + i32::from(z + 2)),
            CAMP_NOTE
        );
        assert_eq!(camp.block(x, y + 1, z + 3).id, "hanging_sign");
        assert_eq!(
            (2 * 16 + i32::from(x), -2 * 16 + i32::from(z + 3)),
            CAMP_SIGN
        );
        assert_eq!(camp.block(x + 2, y, z - 1).id, "jukebox");
        assert_eq!(
            (2 * 16 + i32::from(x + 2), -2 * 16 + i32::from(z - 1)),
            CAMP_JUKEBOX
        );
        assert_eq!(camp.block(x - 2, y, z - 2).id, "composter");
        assert_eq!(
            (2 * 16 + i32::from(x - 2), -2 * 16 + i32::from(z - 2)),
            CAMP_COMPOST
        );
        assert_eq!(camp.block(x - 1, y, z - 2).id, "flower");
        let elsewhere = generate_chunk(3, 0);
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                for y in 0..16u16 {
                    assert_ne!(elsewhere.block(local_x, y, local_z).id, "campfire");
                }
            }
        }
        let mut cluster = synthetic_cluster();
        let demo = cluster.get(0, 0).unwrap().non_air_count();
        seed_terrain_ring(&mut cluster);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
        assert_eq!(
            cluster.block_at(CAMP_FIRE.0, i32::from(y), CAMP_FIRE.1).id,
            "campfire"
        );
    }

    #[test]
    fn a_shrine_stands_southeast_of_the_pad() {
        let shrine = generate_chunk(2, 2);
        let mut chest = false;
        let mut brick = false;
        let mut chisel = false;
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                for y in 0..16u16 {
                    match shrine.block(local_x, y, local_z).id {
                        "chest" => chest = true,
                        "stone_bricks" | "mossy_stone_bricks" | "cracked_stone_bricks" => {
                            brick = true
                        }
                        "chiseled_stone_bricks" => chisel = true,
                        "campfire" => panic!("shrine is not the camp"),
                        _ => {}
                    }
                }
            }
        }
        assert!(chest, "a chest sits in the shrine");
        assert!(brick, "stone bricks form the ruin");
        assert!(chisel, "chiseled brick marks the altar");
        let (cx, cz) = SHRINE;
        let local_x = (cx - 2 * 16) as u8;
        let local_z = (cz - 2 * 16) as u8;
        assert_eq!((local_x, local_z), (8, 8));
        let floor = (0..16u16)
            .find(|y| shrine.block(local_x, *y, local_z).id == "chiseled_stone_bricks")
            .expect("altar");
        assert_eq!(shrine.block(local_x, floor + 1, local_z).id, "decorated_pot");
        assert_eq!(shrine.block(local_x, floor, local_z - 1).id, "chest");
        assert_eq!(
            (2 * 16 + i32::from(local_x), 2 * 16 + i32::from(local_z - 1)),
            SHRINE_CHEST
        );
        assert_eq!(shrine.block(local_x + 2, floor, local_z).id, "furnace");
        assert_eq!(
            (2 * 16 + i32::from(local_x + 2), 2 * 16 + i32::from(local_z)),
            SHRINE_FURNACE
        );
        assert_eq!(shrine.block(local_x + 2, floor, local_z + 1).id, "clay");
        assert_eq!(shrine.block(local_x + 2, floor, local_z - 1).id, "crafting_table");
        assert_eq!(
            (2 * 16 + i32::from(local_x + 2), 2 * 16 + i32::from(local_z - 1)),
            SHRINE_TABLE
        );
        assert_eq!(shrine.block(local_x - 2, floor, local_z).id, "bell");
        assert_eq!(
            (2 * 16 + i32::from(local_x - 2), 2 * 16 + i32::from(local_z)),
            SHRINE_BELL
        );
        assert_eq!(shrine.block(local_x, floor, local_z + 2).id, "lectern");
        assert_eq!(shrine.block(local_x, floor, local_z + 2).metadata, 1);
        assert_eq!(
            (2 * 16 + i32::from(local_x), 2 * 16 + i32::from(local_z + 2)),
            SHRINE_LECTERN
        );
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(SHRINE_CHEST.0, i32::from(floor), SHRINE_CHEST.1)
                .id,
            "chest"
        );
        assert_ne!(SHRINE_CHUNK, CAMP_CHUNK);
        assert_ne!(SHRINE_CHUNK, SHAFT_CHUNK);
    }

    #[test]
    fn a_mine_mouth_stands_west_and_south_of_the_pad() {
        let shaft = generate_chunk(-2, 2);
        let mut logs = false;
        let mut coal = false;
        let mut hole = false;
        let mut floor = true;
        for local_z in 0..16u8 {
            for local_x in 0..16u8 {
                if shaft.block(local_x, 0, local_z).is_air() {
                    floor = false;
                }
                for y in 0..16u16 {
                    match shaft.block(local_x, y, local_z).id {
                        "oak_log" => logs = true,
                        "coal_ore" => coal = true,
                        "air"
                            if (6..=8).contains(&local_x)
                                && (7..=8).contains(&local_z)
                                && y > 0 =>
                        {
                            hole = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(logs, "oak posts frame the mouth");
        assert!(coal, "coal sits in the hole");
        assert!(hole, "the mouth is open");
        assert!(floor, "the bottom layer stays solid");
        let local_x = (SHAFT_MOUTH.0 - (-2 * 16)) as u8;
        let local_z = (SHAFT_MOUTH.1 - 2 * 16) as u8;
        assert_eq!((local_x, local_z), (8, 8));
        assert!(shaft.block(local_x, 1, local_z).is_air());
        assert!(!shaft.block(local_x, 0, local_z).is_air());
        let post = (0..16u16)
            .find(|y| shaft.block(9, *y, 6).id == "oak_log")
            .expect("east post");
        assert_eq!(shaft.block(9, post + 1, local_z).id, "oak_log");
        assert_eq!(shaft.block(10, post - 1, local_z).id, "gravel");
        assert_eq!(shaft.block(10, post, local_z).id, "chest");
        assert_eq!(
            (-2 * 16 + 10, 2 * 16 + i32::from(local_z)),
            SHAFT_CHEST
        );
        assert_eq!(shaft.block(10, post, local_z + 1).id, "blast_furnace");
        assert_eq!(
            (-2 * 16 + 10, 2 * 16 + i32::from(local_z + 1)),
            SHAFT_BLAST
        );
        let camp = generate_chunk(2, -2);
        let mut campfire = false;
        for lz in 0..16u8 {
            for lx in 0..16u8 {
                for y in 0..16u16 {
                    if camp.block(lx, y, lz).id == "campfire" {
                        campfire = true;
                    }
                    assert_ne!(shaft.block(lx, y, lz).id, "campfire");
                }
            }
        }
        assert!(campfire, "the camp stays in its own chunk");
        let mut cluster = synthetic_cluster();
        let demo = cluster.get(0, 0).unwrap().non_air_count();
        seed_terrain_ring(&mut cluster);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
        assert!(cluster.block_at(SHAFT_MOUTH.0, 1, SHAFT_MOUTH.1).is_air());
        assert_eq!(cluster.block_at(-23, i32::from(post), 38).id, "oak_log");
        assert_eq!(
            cluster.block_at(-22, i32::from(post - 1), SHAFT_MOUTH.1).id,
            "gravel"
        );
    }

    #[test]
    fn a_clay_bank_sits_in_the_marsh_south_of_the_pad() {
        let chunk = generate_chunk(CLAY_CHUNK.0, CLAY_CHUNK.1);
        let (cx, cz) = CLAY_BANK;
        let lx = (cx - CLAY_CHUNK.0 * 16) as u8;
        let lz = (cz - CLAY_CHUNK.1 * 16) as u8;
        let height = column_height(cx, cz, Biome::Marsh);
        assert!(height > 0);
        assert_eq!(chunk.block(lx, height - 1, lz).id, "clay");
        assert_eq!(chunk.block(lx, height, lz).id, "water");
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster.block_at(cx, i32::from(height - 1), cz).id,
            "clay"
        );
        assert_ne!(
            generate_chunk(2, 2).block(lx, height - 1, lz).id,
            "clay"
        );
    }

    #[test]
    fn caves_open_under_the_crust_and_marsh_holds_water() {
        let mut cave = false;
        let mut water_on_dirt = false;
        for z in (-24i32..24).step_by(2) {
            for x in (-24i32..24).step_by(2) {
                if x.abs() <= 1 && z.abs() <= 1 {
                    continue;
                }
                let chunk = generate_chunk(x, z);
                for local_z in 0..16u8 {
                    for local_x in 0..16u8 {
                        let wx = x * 16 + i32::from(local_x);
                        let wz = z * 16 + i32::from(local_z);
                        if chunk.block(local_x, 3, local_z).id == "water"
                            && chunk.block(local_x, 2, local_z).id == "dirt"
                        {
                            water_on_dirt = true;
                        }
                        for y in 2..8u16 {
                            if chunk.block(local_x, y, local_z).is_air()
                                && is_cave(wx, i32::from(y), wz)
                            {
                                cave = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(cave, "stone under the crust opens into caves");
        assert!(water_on_dirt, "marsh water sits on dirt");
        let mut coal = false;
        let mut iron = false;
        let mut mouth = false;
        let mut floor = true;
        for z in (-16i32..16).step_by(4) {
            for x in (-16i32..16).step_by(4) {
                if x.abs() <= 1 && z.abs() <= 1 {
                    continue;
                }
                let chunk = generate_chunk(x, z);
                for local_z in 0..16u8 {
                    for local_x in 0..16u8 {
                        if !chunk.block(local_x, 0, local_z).id.eq("stone")
                            && !chunk.block(local_x, 0, local_z).id.eq("iron_ore")
                            && !chunk.block(local_x, 0, local_z).id.eq("coal_ore")
                        {
                            floor = false;
                        }
                        let wx = x * 16 + i32::from(local_x);
                        let wz = z * 16 + i32::from(local_z);
                        for y in 1..12u16 {
                            match chunk.block(local_x, y, local_z).id {
                                "coal_ore" => coal = true,
                                "iron_ore" => iron = true,
                                "air" if hash2(wx, wz) % 9 == 0 && is_cave(wx, 1, wz) => {
                                    mouth = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(coal, "coal veins sit in the stone");
        assert!(iron, "iron veins sit in the bottom stone");
        assert!(mouth, "short ground opens a mouth into a cave");
        assert!(floor, "the bottom layer stays solid");
    }

    #[test]
    fn a_cave_mouth_dips_and_rings_itself_in_deepslate() {
        let mut found = false;
        for z in -64..64 {
            for x in -64..64 {
                if !is_mouth_column(x, z) {
                    continue;
                }
                let cx = x.div_euclid(16);
                let cz = z.div_euclid(16);
                if (cx, cz) == CAMP_CHUNK || (cx, cz) == SHAFT_CHUNK {
                    continue;
                }
                let chunk = generate_chunk(cx, cz);
                let lx = (x - cx * 16) as u8;
                let lz = (z - cz * 16) as u8;
                let height = column_height(x, z, biome_at(x, z));
                assert!(
                    chunk.block(lx, height - 1, lz).is_air(),
                    "the mouth is open"
                );
                assert!(chunk.block(lx, 1, lz).is_air(), "the hole stays dark");
                assert!(!chunk.block(lx, 0, lz).is_air(), "the floor stays");
                assert_ne!(chunk.block(lx, height, lz).id, "oak_log");
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        let nx = x + dx;
                        let nz = z + dz;
                        if is_mouth_column(nx, nz) || biome_at(nx, nz) == Biome::Marsh {
                            continue;
                        }
                        let ncx = nx.div_euclid(16);
                        let ncz = nz.div_euclid(16);
                        if (ncx, ncz) == CAMP_CHUNK || (ncx, ncz) == SHAFT_CHUNK {
                            continue;
                        }
                        let nchunk = generate_chunk(ncx, ncz);
                        let nlx = (nx - ncx * 16) as u8;
                        let nlz = (nz - ncz * 16) as u8;
                        let nh = column_height(nx, nz, biome_at(nx, nz));
                        assert!(nchunk.block(nlx, nh - 1, nlz).is_air(), "the lip dips");
                        assert_eq!(nchunk.block(nlx, nh - 2, nlz).id, "deepslate");
                        found = true;
                    }
                }
                if found {
                    break;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "a surface cave wears a deepslate rim");
    }

    #[test]
    fn ensure_fills_an_empty_chunk_and_leaves_a_saved_block() {
        let mut cluster = synthetic_cluster();
        assert_eq!(cluster.get(4, 0).unwrap().non_air_count(), 0);
        ensure_terrain(&mut cluster, 4, 0);
        let filled = cluster.get(4, 0).unwrap().non_air_count();
        assert!(filled > 100);
        cluster
            .replace_chunk(Chunk::from_blocks(5, 0, [(1, 1, 1, Block::new("wool", 0))]).unwrap())
            .unwrap();
        ensure_terrain(&mut cluster, 5, 0);
        assert_eq!(cluster.block_at(5 * 16 + 1, 1, 1).id, "wool");
        let demo = cluster.get(0, 0).unwrap().non_air_count();
        ensure_terrain(&mut cluster, 0, 0);
        assert_eq!(cluster.get(0, 0).unwrap().non_air_count(), demo);
    }
}
