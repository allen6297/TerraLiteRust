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
    stamp_dungeon(&mut blocks, chunk_x, chunk_z);
    stamp_mineshaft(&mut blocks, chunk_x, chunk_z);
    stamp_shrine(&mut blocks, chunk_x, chunk_z);
    stamp_ruined_portal(&mut blocks, chunk_x, chunk_z);
    stamp_nether_pocket(&mut blocks, chunk_x, chunk_z);
    stamp_nether_fortress(&mut blocks, chunk_x, chunk_z);
    stamp_desert_temple(&mut blocks, chunk_x, chunk_z);
    stamp_shipwreck(&mut blocks, chunk_x, chunk_z);
    stamp_ocean_monument(&mut blocks, chunk_x, chunk_z);
    stamp_stronghold(&mut blocks, chunk_x, chunk_z);
    stamp_end_pocket(&mut blocks, chunk_x, chunk_z);
    stamp_end_city(&mut blocks, chunk_x, chunk_z);
    stamp_end_chorus(&mut blocks, chunk_x, chunk_z);
    stamp_village_remnant(&mut blocks, chunk_x, chunk_z);
    stamp_clay_bank(&mut blocks, chunk_x, chunk_z);
    stamp_frog_pads(&mut blocks, chunk_x, chunk_z);
    Chunk::from_blocks(chunk_x, chunk_z, blocks).expect("terrain coordinates stay in the chunk")
}

/// Lily pads on marsh water under the frog's hop square.
fn stamp_frog_pads(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != FROG_CHUNK {
        return;
    }
    for &(wx, wz) in &FROG_PADS {
        let lx = (wx - chunk_x * 16) as u8;
        let lz = (wz - chunk_z * 16) as u8;
        let height = column_height(wx, wz, Biome::Marsh);
        let pad_y = height + 1;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == pad_y));
        // Keep a water cell under the leaf.
        if !blocks
            .iter()
            .any(|(x, y, z, b)| *x == lx && *z == lz && *y == height && b.id == "water")
        {
            blocks.push((lx, height, lz, Block::new("water", 0)));
        }
        blocks.push((lx, pad_y, lz, Block::new("lily_pad", 0)));
    }
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

/// Cartography table east of the fire (between the oak log and the note).
pub const CAMP_CHART: (i32, i32) = (CAMP_FIRE.0 + 2, CAMP_FIRE.1 + 1);

/// Hanging sign in the south gate, pointing the path north of camp.
pub const CAMP_SIGN: (i32, i32) = (CAMP_FIRE.0, CAMP_FIRE.1 + 3);

/// Jukebox east of the fire (between the log and the chest side).
pub const CAMP_JUKEBOX: (i32, i32) = (CAMP_FIRE.0 + 2, CAMP_FIRE.1 - 1);

/// Composter southwest of the fire (south of the smoker).
pub const CAMP_COMPOST: (i32, i32) = (CAMP_FIRE.0 - 2, CAMP_FIRE.1 - 2);

/// Beehive northwest of the fire (north of the smoker).
pub const CAMP_HIVE: (i32, i32) = (CAMP_FIRE.0 - 2, CAMP_FIRE.1 + 2);

/// Cauldron southeast of the fire (south of the jukebox).
pub const CAMP_CAULDRON: (i32, i32) = (CAMP_FIRE.0 + 2, CAMP_FIRE.1 - 2);

/// Fenced wheat plot east of the camp ring.
pub const CAMP_FARM: (i32, i32) = (CAMP_FIRE.0 + 5, CAMP_FIRE.1);

/// West-facing fence gate into the camp farm plot.
pub const CAMP_FARM_GATE: (i32, i32) = (CAMP_FARM.0 - 1, CAMP_FARM.1);

/// Scarecrow south of the farm plot (fence post + carved pumpkin).
pub const CAMP_SCARECROW: (i32, i32) = (CAMP_FARM.0, CAMP_FARM.1 - 3);

/// Fence hitch west of the east meadow horse pad (demo chunk `(1, 0)`).
pub const HORSE_HITCH: (i32, i32) = (18, 6);

/// West gate of the meadow horse stable ring. Walk east from the stairs.
pub const HORSE_STABLE_GATE: (i32, i32) = (17, 6);

/// Inclusive world bounds of the meadow stable fence ring `[x0, x1, z0, z1]`.
pub const HORSE_STABLE: (i32, i32, i32, i32) = (17, 22, 4, 8);

/// Marsh lily pads under the frog hop square. Chunk `(0, 2)`.
pub const FROG_PADS: [(i32, i32); 4] = [(8, 33), (9, 33), (9, 34), (8, 34)];

const FROG_CHUNK: (i32, i32) = (0, 2);

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
    // East of the fire: widen a held map at the chart table.
    blocks.push((local_x + 2, floor, local_z + 1, Block::new("cartography_table", 0)));
    // South gate lintel height: hanging sign with the north-path tip.
    blocks.push((local_x, floor + 1, local_z + 3, Block::new("hanging_sign", 0)));
    // East of the fire: insert a music disc to play.
    blocks.push((local_x + 2, floor, local_z - 1, Block::new("jukebox", 0)));
    // Southwest of the fire: fill plants for bone meal.
    blocks.push((local_x - 2, floor, local_z - 2, Block::new("composter", 0)));
    // A flower beside it so the fill loop is findable without creative give.
    blocks.push((local_x - 1, floor, local_z - 2, Block::new("flower", 0)));
    // Northwest of the fire: fill flowers for honey.
    blocks.push((local_x - 2, floor, local_z + 2, Block::new("beehive", 0)));
    // Southeast of the fire: pour or scoop water.
    blocks.push((local_x + 2, floor, local_z - 2, Block::new("cauldron", 0)));
    // East of the ring: a fenced wheat plot with a west-facing gate.
    let fx0 = 11u8;
    let fx1 = 14u8;
    let fz0 = 5u8;
    let fz1 = 8u8;
    blocks.retain(|(x, y, z, _)| {
        !(*x >= fx0 && *x <= fx1 && *z >= fz0 && *z <= fz1 && *y + 1 >= floor)
    });
    for lz in fz0..=fz1 {
        for lx in fx0..=fx1 {
            let top = blocks
                .iter()
                .filter(|(x, _, z, _)| *x == lx && *z == lz)
                .map(|(_, y, _, _)| *y)
                .max()
                .unwrap_or(0);
            for y in (top + 1)..floor.saturating_sub(1) {
                blocks.push((lx, y, lz, Block::new("dirt", 0)));
            }
            let edge = lx == fx0 || lx == fx1 || lz == fz0 || lz == fz1;
            let gate = lx == fx0 && lz == local_z;
            if edge && !gate {
                blocks.push((lx, floor - 1, lz, Block::new("grass", 0)));
                blocks.push((lx, floor, lz, Block::new("fence", 0)));
            } else if gate {
                blocks.push((lx, floor - 1, lz, Block::new("grass", 0)));
                blocks.push((lx, floor, lz, Block::new("fence_gate", 0)));
            } else {
                blocks.push((lx, floor - 1, lz, Block::new("farmland", 0)));
                let stage = (lx + lz) % 4;
                blocks.push((lx, floor, lz, Block::new("wheat_crop", stage)));
            }
        }
    }
    // South of the plot: fence post with a carved pumpkin head.
    let sx = local_x + 5;
    let sz = local_z - 3;
    blocks.retain(|(x, y, z, _)| !(*x == sx && *z == sz && *y + 1 >= floor));
    let top = blocks
        .iter()
        .filter(|(x, _, z, _)| *x == sx && *z == sz)
        .map(|(_, y, _, _)| *y)
        .max()
        .unwrap_or(0);
    for y in (top + 1)..floor.saturating_sub(1) {
        blocks.push((sx, y, sz, Block::new("dirt", 0)));
    }
    blocks.push((sx, floor - 1, sz, Block::new("grass", 0)));
    blocks.push((sx, floor, sz, Block::new("fence", 0)));
    blocks.push((sx, floor + 1, sz, Block::new("pumpkin", 1)));
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

/// Grindstone on the apron, one block north of the chest.
pub const SHAFT_GRIND: (i32, i32) = (SHAFT_CHEST.0, SHAFT_CHEST.1 - 1);

/// Smithing table on the apron, one block east of the chest.
pub const SHAFT_SMITH: (i32, i32) = (SHAFT_CHEST.0 + 1, SHAFT_CHEST.1);

/// Anvil on the apron, one block west of the chest (toward the pit).
pub const SHAFT_ANVIL: (i32, i32) = (SHAFT_CHEST.0 - 1, SHAFT_CHEST.1);

/// Ladder on the pit's south wall, one block south of the mouth center.
pub const SHAFT_LADDER: (i32, i32) = (SHAFT_MOUTH.0, SHAFT_MOUTH.1 - 1);

/// Midpoint of the short rail spur east of the mine apron.
pub const SHAFT_RAIL: (i32, i32) = (SHAFT_MOUTH.0 + 7, SHAFT_MOUTH.1);

/// Lodestone northwest of the mouth — link a compass here.
pub const SHAFT_LODESTONE: (i32, i32) = (SHAFT_MOUTH.0 - 1, SHAFT_MOUTH.1 + 2);

/// Demo emerald ore west of the lodestone — rare hills ore for village trades.
pub const EMERALD_ORE: (i32, i32) = (SHAFT_LODESTONE.0 - 1, SHAFT_LODESTONE.1);

/// Underground cobble dungeon west of the mine pit — chest + trial spawner.
pub const DUNGEON: (i32, i32) = (SHAFT_MOUTH.0 - 5, SHAFT_MOUTH.1 - 4);

/// Loot chest on the west wall of the dungeon.
pub const DUNGEON_CHEST: (i32, i32) = (DUNGEON.0 - 1, DUNGEON.1);

/// Trial spawner in the dungeon center.
pub const DUNGEON_SPAWNER: (i32, i32) = DUNGEON;

/// Standing floor of the demo dungeon (cobble underfoot is one below).
pub const DUNGEON_FLOOR: u16 = 2;

/// Abandoned mineshaft spur east of the pit — timber supports + rails.
pub const MINESHAFT: (i32, i32) = (SHAFT_MOUTH.0 + 3, SHAFT_MOUTH.1);

/// Loot chest at the east end of the mineshaft spur (north of the rail).
pub const MINESHAFT_CHEST: (i32, i32) = (SHAFT_MOUTH.0 + 6, SHAFT_MOUTH.1 + 1);

/// Standing floor of the demo mineshaft (gravel underfoot is one below).
pub const MINESHAFT_FLOOR: u16 = 2;

const SHAFT_CHUNK: (i32, i32) = (-2, 2);

/// Ruined nether-portal frame, northwest of the demo pad.
///
/// The chunk is `(-2, -2)`. Walk west past the pad, then north. Incomplete
/// obsidian / crying-obsidian upright with a loot chest on the apron.
pub const RUINED_PORTAL: (i32, i32) = (-24, -24);

/// Loot chest on the ruined portal apron, two blocks east of the SE foot.
pub const RUINED_PORTAL_CHEST: (i32, i32) = (RUINED_PORTAL.0 + 2, RUINED_PORTAL.1 + 1);

/// Hellish sister pad south of the ruined portal — return gate for a lit nether portal.
pub const NETHER_PAD: (i32, i32) = (RUINED_PORTAL.0, RUINED_PORTAL.1 - 16);

/// Interior standing cell of the nether pocket portal (one block inside the frame).
pub const NETHER_PORTAL_IN: (i32, i32) = (NETHER_PAD.0 + 1, NETHER_PAD.1);

/// Interior standing cell of the overworld ruined portal once lit.
pub const RUINED_PORTAL_IN: (i32, i32) = (RUINED_PORTAL.0 + 1, RUINED_PORTAL.1);

/// Nether-brick fortress spur west of the hellish pocket chamber.
///
/// Same chunk as [`NETHER_PAD`] (`(-2, -3)`). Short corridor + loot chest +
/// blaze-cage stub (trial spawner on soul sand).
pub const NETHER_FORTRESS: (i32, i32) = (NETHER_PAD.0 - 6, NETHER_PAD.1);

/// Loot chest at the west stub of the fortress corridor.
pub const NETHER_FORTRESS_CHEST: (i32, i32) = (NETHER_PAD.0 - 7, NETHER_PAD.1 + 1);

/// Blaze-cage stub on the south flank of the corridor (trial spawner on soul sand).
pub const NETHER_FORTRESS_BLAZE: (i32, i32) = (NETHER_PAD.0 - 5, NETHER_PAD.1 - 1);

/// Floor under the fortress spur (matches the nether pocket floor).
pub const NETHER_FORTRESS_FLOOR: u16 = 4;

/// Rare ancient debris embedded in the nether pocket floor (east of the pad).
///
/// Blast it for `netherite_scrap`, craft an ingot, then smith diamond gear up.
pub const NETHER_DEBRIS: (i32, i32) = (NETHER_PAD.0 + 2, NETHER_PAD.1 + 2);

/// Second debris vein west of the pad for a rare read.
pub const NETHER_DEBRIS_B: (i32, i32) = (NETHER_PAD.0 - 2, NETHER_PAD.1 - 1);

const PORTAL_CHUNK: (i32, i32) = (-2, -2);
const NETHER_CHUNK: (i32, i32) = (-2, -3);

/// Desert temple remnant, east of the demo pad.
///
/// The chunk is `(2, 0)`. Walk east from the stairs onto the sand. Hollow
/// sandstone square with a chest inside and a pressure plate at the south gate.
pub const DESERT_TEMPLE: (i32, i32) = (40, 8);

/// Loot chest in the temple interior, one block north of center.
pub const DESERT_TEMPLE_CHEST: (i32, i32) = (DESERT_TEMPLE.0, DESERT_TEMPLE.1 + 1);

/// Stone plate on the south gate — a quiet trap remnant.
pub const DESERT_TEMPLE_PLATE: (i32, i32) = (DESERT_TEMPLE.0, DESERT_TEMPLE.1 - 3);

const TEMPLE_CHUNK: (i32, i32) = (2, 0);

/// Half-buried shipwreck on a sand beach, west of the demo pad.
///
/// The chunk is `(-2, 0)`. Walk west from the stairs onto the shore. Oak hull
/// with a loot chest in the hold; the south side sits in shallow water.
pub const SHIPWRECK: (i32, i32) = (-24, 8);

/// Loot chest in the shipwreck hold, one block east of center.
pub const SHIPWRECK_CHEST: (i32, i32) = (SHIPWRECK.0 + 1, SHIPWRECK.1);

const WRECK_CHUNK: (i32, i32) = (-2, 0);

/// Small prismarine ocean-monument remnant south of the shipwreck shallows.
///
/// The chunk is `(-2, 1)`. Swim south from the wreck into deeper water. Hollow
/// prismarine shell with a gold nook, sponge pads, and a loot chest.
pub const OCEAN_MONUMENT: (i32, i32) = (-24, 22);

/// Loot chest in the monument hall center.
pub const OCEAN_MONUMENT_CHEST: (i32, i32) = (OCEAN_MONUMENT.0, OCEAN_MONUMENT.1);

/// Northwest gold-block corner of the monument treasure nook.
pub const OCEAN_MONUMENT_GOLD: (i32, i32) = (OCEAN_MONUMENT.0 - 2, OCEAN_MONUMENT.1 - 1);

const MONUMENT_CHUNK: (i32, i32) = (-2, 1);

/// Underground stronghold end-portal room, southeast of the demo pad.
///
/// The chunk is `(1, -1)`. Dig or eye-of-ender toward `(24, -8)`. Stone-brick
/// hall with an incomplete end-portal frame ring (some eyes already set) and
/// a loot chest in the corner.
pub const STRONGHOLD: (i32, i32) = (24, -8);

/// Loot chest in the southwest corner of the portal room.
pub const STRONGHOLD_CHEST: (i32, i32) = (STRONGHOLD.0 - 3, STRONGHOLD.1 - 3);

/// Standing floor of the demo stronghold portal room.
pub const STRONGHOLD_FLOOR: u16 = 3;

/// Frame offsets around [`STRONGHOLD`] for the 12-slot end-portal ring.
pub const STRONGHOLD_FRAMES: [(i32, i32); 12] = [
    (-1, -2),
    (0, -2),
    (1, -2),
    (2, -1),
    (2, 0),
    (2, 1),
    (1, 2),
    (0, 2),
    (-1, 2),
    (-2, 1),
    (-2, 0),
    (-2, -1),
];

/// Standing cell in the stronghold portal pit once the ring opens.
pub const STRONGHOLD_PORTAL_IN: (i32, i32) = STRONGHOLD;

const STRONGHOLD_CHUNK: (i32, i32) = (1, -1);

/// End pocket south of the stronghold — obsidian platform over the void.
///
/// The chunk is `(1, -3)`. Reached by walking into a completed end portal.
/// Small return portal on the pad brings you back to the stronghold.
pub const END_PAD: (i32, i32) = (24, -40);

/// Standing cell on the End platform (beside the return portal).
pub const END_PORTAL_IN: (i32, i32) = (END_PAD.0, END_PAD.1 + 1);

/// Return end-portal block on the End platform.
pub const END_RETURN: (i32, i32) = END_PAD;

/// Obsidian platform floor in the End pocket.
pub const END_FLOOR: u16 = 16;

/// End city remnant tower just west of the End pad (same chunk).
///
/// Tall purpur / end-stone ruin with a chest near the top. Reach it by stepping
/// stones off the pad, or flying after the dragon drops an elytra.
pub const END_CITY: (i32, i32) = (20, -36);

/// Loot chest inside the End city tower.
pub const END_CITY_CHEST: (i32, i32) = END_CITY;

/// Standing floor of the End city tower base.
pub const END_CITY_FLOOR: u16 = END_FLOOR;

/// Chorus grove on a small end-stone islet east of the End pad (same chunk).
///
/// Stalks of `chorus_plant` topped with `chorus_flower`. Break either for
/// `chorus_fruit` — End food that blinks you a short hop.
pub const END_CHORUS: (i32, i32) = (28, -44);

/// Floor of the chorus islet (same height as the End pad).
pub const END_CHORUS_FLOOR: u16 = END_FLOOR;

const END_CHUNK: (i32, i32) = (1, -3);

/// Ruined village plaza north of the demo pad.
///
/// The chunk is `(0, -2)`. Walk north from the stairs. Cobble well, gravel
/// paths, three house shells. Open green around the well is reserved for
/// future villager NPCs ([`VILLAGE_GREEN`]).
pub const VILLAGE: (i32, i32) = (8, -24);

/// Loot chest inside the west house shell.
pub const VILLAGE_CHEST: (i32, i32) = (3, -23);

/// Open plaza south of the well — leave clear for future villager NPCs.
pub const VILLAGE_GREEN: (i32, i32) = (8, -22);

const VILLAGE_CHUNK: (i32, i32) = (0, -2);

/// Broken stone-brick shrine, southeast of the demo pad.
///
/// The chunk is `(2, 2)`. Walk east past the pad, then south. A chest sits on
/// the altar under the broken walls.
pub const SHRINE: (i32, i32) = (40, 40);
/// Decorated pot on the shrine altar pedestal (same column as [`SHRINE`]).
pub const SHRINE_POT: (i32, i32) = SHRINE;

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

/// Enchanting table northwest of the altar (west of the lectern).
pub const SHRINE_ENCHANT: (i32, i32) = (SHRINE.0 - 2, SHRINE.1 + 2);

/// Brewing stand northeast of the altar (east of the lectern).
pub const SHRINE_BREW: (i32, i32) = (SHRINE.0 + 2, SHRINE.1 + 2);

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
    // Northwest of the altar: spend a bookshelf for Efficiency on a pick.
    let enchant_x = (SHRINE_ENCHANT.0 - chunk_x * 16) as u8;
    let enchant_z = (SHRINE_ENCHANT.1 - chunk_z * 16) as u8;
    blocks.push((enchant_x, floor, enchant_z, Block::new("enchanting_table", 0)));
    // A bookshelf beside it so the spend loop is findable without creative give.
    blocks.push((enchant_x, floor, enchant_z - 1, Block::new("bookshelf", 0)));
    // Northeast of the altar: brew a healing potion from water + wart.
    let brew_x = (SHRINE_BREW.0 - chunk_x * 16) as u8;
    let brew_z = (SHRINE_BREW.1 - chunk_z * 16) as u8;
    blocks.push((brew_x, floor, brew_z, Block::new("brewing_stand", 0)));
    // Nether wart beside it so the brew loop is findable without creative give.
    blocks.push((brew_x - 1, floor, brew_z, Block::new("nether_wart_block", 0)));
}

/// Complete upright obsidian ring (4×5) at `(cx, cz)` standing on `floor`.
fn push_portal_frame(
    blocks: &mut Vec<(u8, u16, u8, Block)>,
    chunk_x: i32,
    chunk_z: i32,
    cx: i32,
    cz: i32,
    floor: u16,
    lit: bool,
) {
    let obsidian = Block::new("obsidian", 0);
    let portal = Block::new("nether_portal", 0);
    // Outer ring.
    for dy in 0..=4u16 {
        for dx in 0..=3i32 {
            let edge = dx == 0 || dx == 3 || dy == 0 || dy == 4;
            if !edge {
                continue;
            }
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor + dy));
            blocks.push((lx, floor + dy, lz, obsidian));
        }
    }
    if lit {
        for dy in 1..=3u16 {
            for dx in 1..=2i32 {
                let lx = (cx + dx - chunk_x * 16) as u8;
                let lz = (cz - chunk_z * 16) as u8;
                blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor + dy));
                blocks.push((lx, floor + dy, lz, portal));
            }
        }
    }
}

/// Hellish sister pocket south of the ruined portal — netherrack room + return gate.
fn stamp_nether_pocket(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != NETHER_CHUNK {
        return;
    }
    let (cx, cz) = NETHER_PAD;
    let floor: u16 = 4;
    let netherrack = Block::new("netherrack", 0);
    let magma = Block::new("magma", 0);
    let blackstone = Block::new("blackstone", 0);
    let lava = Block::new("lava", 0);
    let glow = Block::new("glowstone", 0);
    // Hollow 9×9×5 chamber.
    for dz in -4i32..=4 {
        for dx in -4i32..=4 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor.saturating_sub(1) && *y <= floor + 5)
            });
            let edge = dx.abs() == 4 || dz.abs() == 4;
            blocks.push((
                lx,
                floor - 1,
                lz,
                if (dx + dz).rem_euclid(3) == 0 {
                    magma
                } else {
                    netherrack
                },
            ));
            if edge {
                for dy in 0..4u16 {
                    blocks.push((
                        lx,
                        floor + dy,
                        lz,
                        if dy == 0 { blackstone } else { netherrack },
                    ));
                }
                blocks.push((lx, floor + 4, lz, netherrack));
            } else {
                blocks.push((lx, floor + 4, lz, netherrack));
            }
        }
    }
    // Lava pools in the corners.
    for &(dx, dz) in &[(-3i32, -3), (3, -3), (-3, 3), (3, 3)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor - 1));
        blocks.push((lx, floor - 1, lz, lava));
    }
    // Glowstone accents on the ceiling.
    for &(dx, dz) in &[(0i32, 0), (-2, 2), (2, -2)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor + 4));
        blocks.push((lx, floor + 4, lz, glow));
    }
    // Rare ancient debris in the floor — blast for netherite scrap.
    let debris = Block::new("ancient_debris", 0);
    for &(wx, wz) in &[NETHER_DEBRIS, NETHER_DEBRIS_B] {
        let lx = (wx - chunk_x * 16) as u8;
        let lz = (wz - chunk_z * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor - 1));
        blocks.push((lx, floor - 1, lz, debris));
    }
    // Return portal — already lit so the trip home works.
    push_portal_frame(blocks, chunk_x, chunk_z, cx, cz, floor, true);
}

/// Short nether-brick corridor west of the pocket — chest + blaze-cage stub.
fn stamp_nether_fortress(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != NETHER_CHUNK {
        return;
    }
    let floor = NETHER_FORTRESS_FLOOR;
    let brick = Block::new("nether_bricks", 0);
    let cracked = Block::new("cracked_nether_bricks", 0);
    let chiseled = Block::new("chiseled_nether_bricks", 0);
    let red = Block::new("red_nether_bricks", 0);
    let soul = Block::new("soul_sand", 0);
    let (pad_x, pad_z) = NETHER_PAD;
    // Corridor from the west chamber wall out to the chest stub.
    let (start_x, end_x) = (pad_x - 7, pad_x - 4);
    let cz = pad_z;
    for wx in start_x..=end_x {
        for wz in (cz - 1)..=(cz + 1) {
            let lx = (wx - chunk_x * 16) as u8;
            let lz = (wz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor.saturating_sub(1) && *y <= floor + 3)
            });
            let bed = if (wx + wz).rem_euclid(3) == 0 {
                cracked
            } else {
                brick
            };
            blocks.push((lx, floor - 1, lz, bed));
            // Side walls + ceiling; leave the mid-row hollow as the walkway.
            if wz != cz {
                for dy in 0..3u16 {
                    let wall = if dy == 1 && (wx - start_x).rem_euclid(2) == 0 {
                        chiseled
                    } else if (wx + dy as i32).rem_euclid(4) == 0 {
                        red
                    } else {
                        brick
                    };
                    blocks.push((lx, floor + dy, lz, wall));
                }
            }
            blocks.push((lx, floor + 3, lz, brick));
        }
        // Center walkway stays air above the brick bed (feet at `floor`).
    }
    // Punch a doorway through the pocket's west wall into the corridor.
    let door_x = ((pad_x - 4) - chunk_x * 16) as u8;
    let door_z = (cz - chunk_z * 16) as u8;
    for dy in 1..=2u16 {
        blocks.retain(|(x, y, z, _)| !(*x == door_x && *z == door_z && *y == floor + dy));
    }
    // Blaze-cage stub on the south flank — soul sand + trial spawner (walkway stays clear).
    let (bx, bz) = NETHER_FORTRESS_BLAZE;
    let blaze_x = (bx - chunk_x * 16) as u8;
    let blaze_z = (bz - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| {
        !(*x == blaze_x && *z == blaze_z && *y >= floor && *y <= floor + 2)
    });
    blocks.push((blaze_x, floor, blaze_z, soul));
    blocks.push((blaze_x, floor + 1, blaze_z, Block::new("trial_spawner", 0)));
    blocks.push((blaze_x, floor + 2, blaze_z, chiseled));
    // Chest on the north flank of the west stub.
    let (chest_wx, chest_wz) = NETHER_FORTRESS_CHEST;
    let chest_x = (chest_wx - chunk_x * 16) as u8;
    let chest_z = (chest_wz - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == chest_x && *z == chest_z && *y == floor));
    blocks.push((chest_x, floor, chest_z, Block::new("chest", 0)));
    // Glowstone keystone over the doorway so the spur reads from the chamber.
    let key_x = door_x;
    let key_z = door_z;
    blocks.retain(|(x, y, z, _)| !(*x == key_x && *z == key_z && *y == floor + 3));
    blocks.push((key_x, floor + 3, key_z, Block::new("glowstone", 0)));
}

/// Incomplete portal frame + apron + chest. Guaranteed in [`PORTAL_CHUNK`];
/// rare copies in other non-reserved chunks.
fn stamp_ruined_portal(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    let guaranteed = (chunk_x, chunk_z) == PORTAL_CHUNK;
    let reserved = matches!(
        (chunk_x, chunk_z),
        CAMP_CHUNK
            | SHAFT_CHUNK
            | SHRINE_CHUNK
            | CLAY_CHUNK
            | TEMPLE_CHUNK
            | WRECK_CHUNK
            | MONUMENT_CHUNK
            | STRONGHOLD_CHUNK
            | END_CHUNK
            | VILLAGE_CHUNK
            | NETHER_CHUNK
            | (0, 0)
    );
    if !guaranteed {
        if reserved || hash2(chunk_x.wrapping_mul(31), chunk_z.wrapping_mul(17)) % 113 != 0 {
            return;
        }
    }
    let (cx, cz) = if guaranteed {
        RUINED_PORTAL
    } else {
        (chunk_x * 16 + 8, chunk_z * 16 + 8)
    };
    let floor = column_height(cx, cz, biome_at(cx, cz)).max(2);
    // Clear a small pad so the frame stands free, then lay the apron.
    let x0 = ((cx - 2) - chunk_x * 16) as u8;
    let x1 = ((cx + 4) - chunk_x * 16) as u8;
    let z0 = ((cz - 2) - chunk_z * 16) as u8;
    let z1 = ((cz + 3) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    for dz in -2..=3 {
        for dx in -2..=4 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            // Netherrack / magma apron under the ruin.
            let apron = if (dx + dz).rem_euclid(3) == 0 {
                Block::new("magma", 0)
            } else if (dx * 2 + dz).rem_euclid(4) == 0 {
                Block::new("netherrack", 0)
            } else {
                Block::new("blackstone", 0)
            };
            blocks.push((lx, floor - 1, lz, apron));
        }
    }
    let obsidian = Block::new("obsidian", 0);
    let crying = Block::new("crying_obsidian", 0);
    let cracked = Block::new("cracked_blackstone_bricks", 0);
    // Outer frame 4 wide × 5 tall, incomplete (classic ruined portal silhouette).
    // Columns at dx=0 and dx=3; bottom lintel dy=0; broken top.
    let frame = [
        // West column (mostly intact).
        (0, 0, false),
        (0, 1, false),
        (0, 2, true),
        (0, 3, false),
        // East column (gappy).
        (3, 0, false),
        (3, 1, true),
        (3, 3, false),
        // Bottom lintel (one missing).
        (1, 0, false),
        // Top lintel fragments.
        (1, 4, true),
        (2, 4, false),
        // Fallen / cracked rubble beside the east foot.
        (4, 0, false),
    ];
    for &(dx, dy, tear) in &frame {
        let wx = cx + dx;
        let wz = cz;
        let lx = (wx - chunk_x * 16) as u8;
        let lz = (wz - chunk_z * 16) as u8;
        let block = if tear {
            crying
        } else if dx == 4 {
            cracked
        } else {
            obsidian
        };
        blocks.push((lx, floor + dy as u16, lz, block));
    }
    // Extra cracked rubble on the apron.
    let rubble = [(1, 1), (2, -1), (-1, 0), (3, 2)];
    for &(dx, dz) in &rubble {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.push((lx, floor, lz, cracked));
    }
    // Loot chest east of the SE foot.
    let (chest_x, chest_z) = if guaranteed {
        RUINED_PORTAL_CHEST
    } else {
        (cx + 2, cz + 1)
    };
    let chest_lx = (chest_x - chunk_x * 16) as u8;
    let chest_lz = (chest_z - chunk_z * 16) as u8;
    blocks.push((chest_lx, floor, chest_lz, Block::new("chest", 0)));
    // Gold fleck so the ruin reads "nether" even from a distance.
    let gold_lx = (cx + 1 - chunk_x * 16) as u8;
    let gold_lz = (cz + 2 - chunk_z * 16) as u8;
    blocks.push((gold_lx, floor, gold_lz, Block::new("gilded_blackstone", 0)));
}

/// Hollow sandstone square + sand apron + chest. Guaranteed in [`TEMPLE_CHUNK`];
/// rare scatter only in desert biomes.
fn stamp_desert_temple(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    let guaranteed = (chunk_x, chunk_z) == TEMPLE_CHUNK;
    let reserved = matches!(
        (chunk_x, chunk_z),
        CAMP_CHUNK
            | SHAFT_CHUNK
            | SHRINE_CHUNK
            | CLAY_CHUNK
            | PORTAL_CHUNK
            | NETHER_CHUNK
            | WRECK_CHUNK
            | MONUMENT_CHUNK
            | STRONGHOLD_CHUNK
            | VILLAGE_CHUNK
            | (0, 0)
    );
    if !guaranteed {
        if reserved {
            return;
        }
        let cx = chunk_x * 16 + 8;
        let cz = chunk_z * 16 + 8;
        if biome_at(cx, cz) != Biome::Desert {
            return;
        }
        if hash2(chunk_x.wrapping_mul(41), chunk_z.wrapping_mul(19)) % 97 != 0 {
            return;
        }
    }
    let (cx, cz) = if guaranteed {
        DESERT_TEMPLE
    } else {
        (chunk_x * 16 + 8, chunk_z * 16 + 8)
    };
    let floor = column_height(cx, cz, Biome::Desert).max(2);
    // Clear a 9×9 pad and lay sand so the remnant reads as desert.
    let x0 = ((cx - 4) - chunk_x * 16) as u8;
    let x1 = ((cx + 4) - chunk_x * 16) as u8;
    let z0 = ((cz - 4) - chunk_z * 16) as u8;
    let z1 = ((cz + 4) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    let sand = Block::new("sand", 0);
    let sandstone = Block::new("sandstone", 0);
    let red = Block::new("red_sandstone", 0);
    for dz in -4..=4 {
        for dx in -4..=4 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.push((lx, floor - 1, lz, sand));
        }
    }
    // Hollow 7×7 square walls (relative −3..+3), broken gaps + taller corners.
    for dz in -3i32..=3 {
        for dx in -3i32..=3 {
            let edge = dx.abs() == 3 || dz.abs() == 3;
            if !edge {
                continue;
            }
            // South gate opening.
            if dz == -3 && dx.abs() <= 1 {
                continue;
            }
            // Weathered gaps.
            if (dx + dz * 2).rem_euclid(5) == 0 {
                continue;
            }
            let corner = dx.abs() == 3 && dz.abs() == 3;
            let tall = if corner { 3u16 } else { 2 };
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            for dy in 0..tall {
                let block = if corner && dy == tall - 1 {
                    red
                } else if (dx + dy as i32).rem_euclid(3) == 0 {
                    red
                } else {
                    sandstone
                };
                blocks.push((lx, floor + dy, lz, block));
            }
        }
    }
    // Low step pyramid nub on the north wall (reads "temple", not just a box).
    for &(dx, dy) in &[(0, 2), (-1, 2), (1, 2), (0, 3)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + 3 - chunk_z * 16) as u8;
        blocks.push((lx, floor + dy, lz, if dy == 3 { red } else { sandstone }));
    }
    // Interior chest + south-gate pressure plate remnant.
    let (chest_x, chest_z) = if guaranteed {
        DESERT_TEMPLE_CHEST
    } else {
        (cx, cz + 1)
    };
    let chest_lx = (chest_x - chunk_x * 16) as u8;
    let chest_lz = (chest_z - chunk_z * 16) as u8;
    blocks.push((chest_lx, floor, chest_lz, Block::new("chest", 0)));
    let (plate_x, plate_z) = if guaranteed {
        DESERT_TEMPLE_PLATE
    } else {
        (cx, cz - 3)
    };
    let plate_lx = (plate_x - chunk_x * 16) as u8;
    let plate_lz = (plate_z - chunk_z * 16) as u8;
    blocks.push((plate_lx, floor, plate_lz, Block::new("plate", 0)));
}

/// Half-buried oak hull on a sand beach with shallow water south.
/// Guaranteed in [`WRECK_CHUNK`]; rare scatter near marsh shores.
fn stamp_shipwreck(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    let guaranteed = (chunk_x, chunk_z) == WRECK_CHUNK;
    let reserved = matches!(
        (chunk_x, chunk_z),
        CAMP_CHUNK
            | SHAFT_CHUNK
            | SHRINE_CHUNK
            | CLAY_CHUNK
            | PORTAL_CHUNK
            | NETHER_CHUNK
            | TEMPLE_CHUNK
            | MONUMENT_CHUNK
            | STRONGHOLD_CHUNK
            | VILLAGE_CHUNK
            | (0, 0)
    );
    if !guaranteed {
        if reserved {
            return;
        }
        let cx = chunk_x * 16 + 8;
        let cz = chunk_z * 16 + 8;
        // Prefer marsh / marsh-adjacent shores.
        let shore = biome_at(cx, cz) == Biome::Marsh
            || biome_at(cx, cz + 8) == Biome::Marsh
            || biome_at(cx, cz - 8) == Biome::Marsh;
        if !shore || hash2(chunk_x.wrapping_mul(53), chunk_z.wrapping_mul(23)) % 101 != 0 {
            return;
        }
    }
    let (cx, cz) = if guaranteed {
        SHIPWRECK
    } else {
        (chunk_x * 16 + 8, chunk_z * 16 + 8)
    };
    let floor = column_height(cx, cz, biome_at(cx, cz)).max(3);
    // Clear a beach pad; sand north, shallow water south.
    let x0 = ((cx - 5) - chunk_x * 16) as u8;
    let x1 = ((cx + 5) - chunk_x * 16) as u8;
    let z0 = ((cz - 3) - chunk_z * 16) as u8;
    let z1 = ((cz + 4) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor.saturating_sub(1)));
    let sand = Block::new("sand", 0);
    let water = Block::new("water", 0);
    let oak = Block::new("oak_log", 0);
    let stair = Block::new("oak_stairs", 0);
    for dz in -3i32..=4 {
        for dx in -5i32..=5 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            if dz >= 2 {
                // Shallow water lapping the south flank.
                blocks.push((lx, floor - 2, lz, sand));
                blocks.push((lx, floor - 1, lz, water));
            } else {
                blocks.push((lx, floor - 1, lz, sand));
            }
        }
    }
    // Hull keel along X (−4..+4), half-sunk: floor−1 under sand, walls at floor.
    for dx in -4i32..=4 {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz0 = (cz - chunk_z * 16) as u8;
        let lz1 = (cz + 1 - chunk_z * 16) as u8;
        // Buried keel.
        blocks.push((lx, floor - 1, lz0, oak));
        blocks.push((lx, floor - 1, lz1, oak));
        // Deck floor (gaps for ruin).
        if dx.abs() != 2 {
            blocks.push((lx, floor, lz0, oak));
        }
        if dx.abs() != 3 {
            blocks.push((lx, floor, lz1, oak));
        }
        // Port / starboard ribs (broken).
        if dx.abs() != 1 && dx.abs() != 4 {
            blocks.push((lx, floor + 1, lz0, oak));
        }
        if dx != 0 && dx.abs() != 2 {
            blocks.push((lx, floor + 1, lz1, oak));
        }
    }
    // Broken bow (west) and stern steps (east).
    let bow_lx = (cx - 5 - chunk_x * 16) as u8;
    let stern_lx = (cx + 5 - chunk_x * 16) as u8;
    let mid_lz = (cz - chunk_z * 16) as u8;
    blocks.push((bow_lx, floor, mid_lz, oak));
    blocks.push((bow_lx, floor + 1, mid_lz, oak));
    blocks.push((stern_lx, floor, mid_lz, stair));
    blocks.push((stern_lx, floor - 1, mid_lz, oak));
    // Mast stub amidships.
    let mast_lx = (cx - chunk_x * 16) as u8;
    let mast_lz = (cz - chunk_z * 16) as u8;
    blocks.push((mast_lx, floor + 2, mast_lz, oak));
    blocks.push((mast_lx, floor + 3, mast_lz, oak));
    // Hold chest.
    let (chest_x, chest_z) = if guaranteed {
        SHIPWRECK_CHEST
    } else {
        (cx + 1, cz)
    };
    let chest_lx = (chest_x - chunk_x * 16) as u8;
    let chest_lz = (chest_z - chunk_z * 16) as u8;
    blocks.push((chest_lx, floor, chest_lz, Block::new("chest", 0)));
}

/// Small underwater prismarine remnant with a gold nook + sponge + chest.
/// Guaranteed in [`MONUMENT_CHUNK`] south of the shipwreck shallows.
fn stamp_ocean_monument(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    let guaranteed = (chunk_x, chunk_z) == MONUMENT_CHUNK;
    let reserved = matches!(
        (chunk_x, chunk_z),
        CAMP_CHUNK
            | SHAFT_CHUNK
            | SHRINE_CHUNK
            | CLAY_CHUNK
            | PORTAL_CHUNK
            | NETHER_CHUNK
            | TEMPLE_CHUNK
            | WRECK_CHUNK
            | STRONGHOLD_CHUNK
            | VILLAGE_CHUNK
            | (0, 0)
    );
    if !guaranteed {
        if reserved {
            return;
        }
        let cx = chunk_x * 16 + 8;
        let cz = chunk_z * 16 + 8;
        let wet = biome_at(cx, cz) == Biome::Marsh
            || biome_at(cx, cz + 8) == Biome::Marsh
            || biome_at(cx + 8, cz) == Biome::Marsh;
        if !wet || hash2(chunk_x.wrapping_mul(67), chunk_z.wrapping_mul(31)) % 109 != 0 {
            return;
        }
    }
    let (cx, cz) = if guaranteed {
        OCEAN_MONUMENT
    } else {
        (chunk_x * 16 + 8, chunk_z * 16 + 8)
    };
    let surface = column_height(cx, cz, biome_at(cx, cz)).max(5);
    let floor = surface.saturating_sub(2).max(2);
    let x0 = ((cx - 3) - chunk_x * 16) as u8;
    let x1 = ((cx + 3) - chunk_x * 16) as u8;
    let z0 = ((cz - 3) - chunk_z * 16) as u8;
    let z1 = ((cz + 3) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| {
        !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor.saturating_sub(1))
    });
    let sand = Block::new("sand", 0);
    let water = Block::new("water", 0);
    let prism = Block::new("prismarine", 0);
    let bricks = Block::new("prismarine_bricks", 0);
    let dark = Block::new("dark_prismarine", 0);
    let lantern = Block::new("sea_lantern", 0);
    let gold = Block::new("gold_block", 0);
    let sponge = Block::new("sponge", 0);
    // Wet basin: sand underfloor, water column, prismarine shell.
    for dz in -3i32..=3 {
        for dx in -3i32..=3 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.push((lx, floor - 1, lz, sand));
            let edge = dx.abs() == 3 || dz.abs() == 3;
            let corner = dx.abs() == 3 && dz.abs() == 3;
            if edge {
                let wall = if corner { dark } else { bricks };
                blocks.push((lx, floor, lz, wall));
                // Broken upper walls — skip a few for ruin silhouette.
                if !(dx == 0 || dz == 0 || (dx == 2 && dz == 3)) {
                    blocks.push((lx, floor + 1, lz, wall));
                }
                if corner || dx.abs() + dz.abs() == 4 {
                    blocks.push((lx, floor + 2, lz, dark));
                }
            } else {
                blocks.push((lx, floor, lz, prism));
            }
            // Water fills the hall and rises to the surface.
            for y in (floor + 1)..=surface {
                if edge && y <= floor + 2 {
                    continue;
                }
                blocks.push((lx, y, lz, water));
            }
        }
    }
    // Sea lantern pillars at the four floor corners.
    for &(dx, dz) in &[(-2i32, -2), (2, -2), (-2, 2), (2, 2)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.push((lx, floor, lz, lantern));
        blocks.push((lx, floor + 1, lz, lantern));
    }
    // Gold treasure nook (NW interior).
    let (gx, gz) = if guaranteed {
        OCEAN_MONUMENT_GOLD
    } else {
        (cx - 2, cz - 1)
    };
    for &(dx, dz) in &[(0i32, 0), (1, 0), (0, 1), (1, 1)] {
        let lx = (gx + dx - chunk_x * 16) as u8;
        let lz = (gz + dz - chunk_z * 16) as u8;
        blocks.push((lx, floor, lz, gold));
    }
    // Sponge pads beside the gold.
    let sponge_spots = [(gx + 2, gz), (gx + 2, gz + 1), (gx, gz + 2)];
    for &(sx, sz) in &sponge_spots {
        let lx = (sx - chunk_x * 16) as u8;
        let lz = (sz - chunk_z * 16) as u8;
        blocks.push((lx, floor, lz, sponge));
    }
    // Loot chest in the hall center (on prismarine, under water).
    let (chest_x, chest_z) = if guaranteed {
        OCEAN_MONUMENT_CHEST
    } else {
        (cx, cz)
    };
    let chest_lx = (chest_x - chunk_x * 16) as u8;
    let chest_lz = (chest_z - chunk_z * 16) as u8;
    blocks.push((chest_lx, floor + 1, chest_lz, Block::new("chest", 0)));
}

/// Stone-brick end-portal room with a partially eyed frame ring + chest.
/// Guaranteed in [`STRONGHOLD_CHUNK`].
fn stamp_stronghold(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != STRONGHOLD_CHUNK {
        return;
    }
    let (cx, cz) = STRONGHOLD;
    let floor = STRONGHOLD_FLOOR;
    let brick = Block::new("stone_bricks", 0);
    let mossy = Block::new("mossy_stone_bricks", 0);
    let cracked = Block::new("cracked_stone_bricks", 0);
    let chiseled = Block::new("chiseled_stone_bricks", 0);
    let lava = Block::new("lava", 0);
    let empty = Block::new("end_portal_frame", 0);
    let eyed = Block::new("end_portal_frame", 1);
    // Hollow 9×9×4 stone-brick hall centered on STRONGHOLD.
    for dz in -4i32..=4 {
        for dx in -4i32..=4 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            let edge = dx.abs() == 4 || dz.abs() == 4;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor.saturating_sub(1) && *y <= floor + 3)
            });
            let tile = match (dx + dz * 3).rem_euclid(5) {
                0 => mossy,
                1 => cracked,
                2 if edge => chiseled,
                _ => brick,
            };
            blocks.push((lx, floor - 1, lz, tile));
            if edge {
                for dy in 0..4u16 {
                    let wall = if (dx + dy as i32 + dz).rem_euclid(4) == 0 {
                        mossy
                    } else if (dx * dz).rem_euclid(3) == 0 {
                        cracked
                    } else {
                        brick
                    };
                    blocks.push((lx, floor + dy, lz, wall));
                }
            } else {
                blocks.push((lx, floor + 3, lz, brick));
            }
        }
    }
    // Portal pit: 3×3 lava under the ring, air above.
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y >= floor - 1 && *y <= floor + 1));
            blocks.push((lx, floor - 1, lz, lava));
        }
    }
    // Incomplete 12-frame ring. North row + one east slot already hold eyes.
    for (index, &(dx, dz)) in STRONGHOLD_FRAMES.iter().enumerate() {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor));
        // First four slots start eyed for the demo.
        let has_eye = index < 4;
        blocks.push((lx, floor, lz, if has_eye { eyed } else { empty }));
    }
    // Corner chest + torch so the hall reads in a still.
    let chest_lx = (STRONGHOLD_CHEST.0 - chunk_x * 16) as u8;
    let chest_lz = (STRONGHOLD_CHEST.1 - chunk_z * 16) as u8;
    blocks.push((chest_lx, floor, chest_lz, Block::new("chest", 0)));
    let torch_lx = (cx + 3 - chunk_x * 16) as u8;
    let torch_lz = (cz - 3 - chunk_z * 16) as u8;
    blocks.push((torch_lx, floor, torch_lz, Block::new("torch", 0)));
}

/// Obsidian platform over empty void + return end portal. Guaranteed in [`END_CHUNK`].
fn stamp_end_pocket(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != END_CHUNK {
        return;
    }
    let (cx, cz) = END_PAD;
    let floor = END_FLOOR;
    let obsidian = Block::new("obsidian", 0);
    let end_stone = Block::new("end_stone", 0);
    let portal = Block::new("end_portal", 0);
    // Clear the whole column so the pocket reads as void around the pad.
    for lz in 0..16u8 {
        for lx in 0..16u8 {
            blocks.retain(|(x, _y, z, _)| !(*x == lx && *z == lz));
        }
    }
    // 5×5 obsidian platform with end-stone rim accents.
    for dz in -2i32..=2 {
        for dx in -2i32..=2 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            let rim = dx.abs() == 2 || dz.abs() == 2;
            blocks.push((lx, floor, lz, if rim { end_stone } else { obsidian }));
        }
    }
    // Return portal in the platform center — walk through to leave the End.
    let ret_lx = (END_RETURN.0 - chunk_x * 16) as u8;
    let ret_lz = (END_RETURN.1 - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == ret_lx && *z == ret_lz && *y == floor + 1));
    blocks.push((ret_lx, floor + 1, ret_lz, portal));
    // End rod so the pad has a light anchor in the void.
    let rod_lx = (cx + 2 - chunk_x * 16) as u8;
    let rod_lz = (cz - chunk_z * 16) as u8;
    blocks.push((rod_lx, floor + 1, rod_lz, Block::new("end_rod", 0)));
}

/// Tall purpur / end-stone tower west of the End pad. Guaranteed in [`END_CHUNK`].
fn stamp_end_city(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != END_CHUNK {
        return;
    }
    let (cx, cz) = END_CITY;
    let floor = END_CITY_FLOOR;
    let end_stone = Block::new("end_stone", 0);
    let purpur = Block::new("purpur", 0);
    let pillar = Block::new("purpur_pillar", 0);
    let rod = Block::new("end_rod", 0);
    // Stepping stones from the pad rim toward the tower base.
    for &(wx, wz) in &[(22i32, -38), (21, -37)] {
        let lx = (wx - chunk_x * 16) as u8;
        let lz = (wz - chunk_z * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y == floor));
        blocks.push((lx, floor, lz, end_stone));
    }
    // 3×3 floating foundation.
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| !(*x == lx && *z == lz && *y >= floor && *y <= floor + 12));
            blocks.push((lx, floor, lz, end_stone));
        }
    }
    // Hollow purpur shell with pillar corners; skip a few cells for ruin gaps.
    for y in 1u16..=10 {
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                let edge = dx.abs() == 1 || dz.abs() == 1;
                if !edge {
                    continue;
                }
                // Broken openings on the south face and mid east wall.
                if (dx == 0 && dz == 1 && (y == 2 || y == 6))
                    || (dx == 1 && dz == 0 && y == 4)
                {
                    continue;
                }
                let lx = (cx + dx - chunk_x * 16) as u8;
                let lz = (cz + dz - chunk_z * 16) as u8;
                let corner = dx.abs() == 1 && dz.abs() == 1;
                blocks.push((lx, floor + y, lz, if corner { pillar } else { purpur }));
            }
        }
    }
    // Cap + spire rod.
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.push((lx, floor + 11, lz, if dx == 0 && dz == 0 { end_stone } else { purpur }));
        }
    }
    let tip_lx = (cx - chunk_x * 16) as u8;
    let tip_lz = (cz - chunk_z * 16) as u8;
    blocks.push((tip_lx, floor + 12, tip_lz, rod));
    // Chest on an interior shelf near the top.
    let chest_lx = (END_CITY_CHEST.0 - chunk_x * 16) as u8;
    let chest_lz = (END_CITY_CHEST.1 - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == chest_lx && *z == chest_lz && *y == floor + 8));
    blocks.push((chest_lx, floor + 8, chest_lz, Block::new("chest", 0)));
    // Inner floor under the chest so it does not hang in air.
    blocks.retain(|(x, y, z, _)| !(*x == chest_lx && *z == chest_lz && *y == floor + 7));
    blocks.push((chest_lx, floor + 7, chest_lz, purpur));
}

/// Small end-stone islet with chorus stalks east of the End pad.
fn stamp_end_chorus(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != END_CHUNK {
        return;
    }
    let (cx, cz) = END_CHORUS;
    let floor = END_CHORUS_FLOOR;
    let end_stone = Block::new("end_stone", 0);
    let plant = Block::new("chorus_plant", 0);
    let flower = Block::new("chorus_flower", 0);
    // 3×3 floating end-stone pad.
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor && *y <= floor + 5)
            });
            blocks.push((lx, floor, lz, end_stone));
        }
    }
    // Center stalk: plant / plant / flower.
    let mid_lx = (cx - chunk_x * 16) as u8;
    let mid_lz = (cz - chunk_z * 16) as u8;
    blocks.push((mid_lx, floor + 1, mid_lz, plant));
    blocks.push((mid_lx, floor + 2, mid_lz, plant));
    blocks.push((mid_lx, floor + 3, mid_lz, flower));
    // Side stalks for a small grove read.
    for &(dx, dz, height) in &[(1i32, 0i32, 2u16), (-1, 1, 1), (0, -1, 2)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        for y in 1..=height {
            blocks.push((lx, floor + y, lz, plant));
        }
        blocks.push((lx, floor + height + 1, lz, flower));
    }
}

/// Ruined plaza + three house shells + well. Guaranteed in [`VILLAGE_CHUNK`].
/// Open green around the well stays clear for future villager NPCs.
fn stamp_village_remnant(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    let guaranteed = (chunk_x, chunk_z) == VILLAGE_CHUNK;
    let reserved = matches!(
        (chunk_x, chunk_z),
        CAMP_CHUNK
            | SHAFT_CHUNK
            | SHRINE_CHUNK
            | CLAY_CHUNK
            | PORTAL_CHUNK
            | NETHER_CHUNK
            | TEMPLE_CHUNK
            | WRECK_CHUNK
            | MONUMENT_CHUNK
            | STRONGHOLD_CHUNK
            | END_CHUNK
            | (0, 0)
    );
    if !guaranteed {
        if reserved {
            return;
        }
        let cx = chunk_x * 16 + 8;
        let cz = chunk_z * 16 + 8;
        if biome_at(cx, cz) != Biome::Plains {
            return;
        }
        if hash2(chunk_x.wrapping_mul(59), chunk_z.wrapping_mul(29)) % 107 != 0 {
            return;
        }
    }
    let (cx, cz) = if guaranteed {
        VILLAGE
    } else {
        (chunk_x * 16 + 8, chunk_z * 16 + 8)
    };
    let floor = column_height(cx, cz, biome_at(cx, cz)).max(3);
    // Clear the plaza pad (keeps room for villagers on the green).
    let x0 = ((cx - 7) - chunk_x * 16) as u8;
    let x1 = ((cx + 7) - chunk_x * 16) as u8;
    let z0 = ((cz - 6) - chunk_z * 16) as u8;
    let z1 = ((cz + 4) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x >= x0 && *x <= x1 && *z >= z0 && *z <= z1 && *y + 1 >= floor));
    let grass = Block::new("grass", 0);
    let gravel = Block::new("gravel", 0);
    let cobble = Block::new("cobblestone", 0);
    let mossy = Block::new("mossy_cobblestone", 0);
    let oak = Block::new("oak_log", 0);
    let water = Block::new("water", 0);
    for dz in -6i32..=4 {
        for dx in -7i32..=7 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            let path = dx == 0 || dz == 0 || (dx.abs() <= 1 && dz.abs() <= 1);
            blocks.push((lx, floor - 1, lz, if path { gravel } else { grass }));
        }
    }
    // Cobble well at plaza center — rim + water, open above.
    for &(dx, dz) in &[(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
        let lx = (cx + dx - chunk_x * 16) as u8;
        let lz = (cz + dz - chunk_z * 16) as u8;
        blocks.push((lx, floor, lz, if (dx + dz).rem_euclid(2) == 0 { mossy } else { cobble }));
    }
    let well_lx = (cx - chunk_x * 16) as u8;
    let well_lz = (cz - chunk_z * 16) as u8;
    blocks.push((well_lx, floor - 1, well_lz, cobble));
    blocks.push((well_lx, floor, well_lz, water));

    // Three ruined house shells: west (with chest), east, north.
    let houses: [((i32, i32), bool); 3] = if guaranteed {
        [
            ((VILLAGE_CHEST.0 - 1, VILLAGE_CHEST.1 - 1), true),
            ((cx + 4, cz - 1), false),
            ((cx - 1, cz - 5), false),
        ]
    } else {
        [
            ((cx - 6, cz - 1), true),
            ((cx + 4, cz - 1), false),
            ((cx - 1, cz - 5), false),
        ]
    };
    for &((hx, hz), with_chest) in &houses {
        // 4×4 footprint, south door gap, broken walls, oak corners.
        for dz in 0i32..=3 {
            for dx in 0i32..=3 {
                let edge = dx == 0 || dx == 3 || dz == 0 || dz == 3;
                if !edge {
                    continue;
                }
                // South door.
                if dz == 0 && (dx == 1 || dx == 2) {
                    continue;
                }
                // Weathered gaps.
                if (dx * 3 + dz * 5 + hx).rem_euclid(7) == 0 {
                    continue;
                }
                let corner = (dx == 0 || dx == 3) && (dz == 0 || dz == 3);
                let tall = if corner { 3u16 } else { 2 };
                let lx = (hx + dx - chunk_x * 16) as u8;
                let lz = (hz + dz - chunk_z * 16) as u8;
                for dy in 0..tall {
                    let block = if corner {
                        oak
                    } else if (dx + dy as i32).rem_euclid(2) == 0 {
                        mossy
                    } else {
                        cobble
                    };
                    blocks.push((lx, floor + dy, lz, block));
                }
            }
        }
        // Partial roof beams.
        for &(dx, dz) in &[(1, 1), (2, 2)] {
            let lx = (hx + dx - chunk_x * 16) as u8;
            let lz = (hz + dz - chunk_z * 16) as u8;
            blocks.push((lx, floor + 2, lz, oak));
        }
        if with_chest {
            let (chest_x, chest_z) = if guaranteed {
                VILLAGE_CHEST
            } else {
                (hx + 1, hz + 1)
            };
            let chest_lx = (chest_x - chunk_x * 16) as u8;
            let chest_lz = (chest_z - chunk_z * 16) as u8;
            blocks.push((chest_lx, floor, chest_lz, Block::new("chest", 0)));
        }
    }
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

/// Cobble dungeon under the mine apron — mossy room, chest, trial spawner.
/// Only the shaft chunk gets the guaranteed demo.
fn stamp_dungeon(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != SHAFT_CHUNK {
        return;
    }
    let (cx, cz) = DUNGEON;
    let floor = DUNGEON_FLOOR;
    let cobble = Block::new("cobblestone", 0);
    let mossy = Block::new("mossy_cobblestone", 0);
    // Hollow 5×5×3 room centered on DUNGEON, floor underfoot at `floor`.
    for dz in -2i32..=2 {
        for dx in -2i32..=2 {
            let lx = (cx + dx - chunk_x * 16) as u8;
            let lz = (cz + dz - chunk_z * 16) as u8;
            let edge = dx.abs() == 2 || dz.abs() == 2;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor.saturating_sub(1) && *y <= floor + 2)
            });
            // Floor slab underfoot.
            blocks.push((
                lx,
                floor - 1,
                lz,
                if (dx + dz).rem_euclid(2) == 0 {
                    mossy
                } else {
                    cobble
                },
            ));
            if edge {
                for dy in 0..3u16 {
                    let wall = if (dx + dy as i32 + dz).rem_euclid(3) == 0 {
                        mossy
                    } else {
                        cobble
                    };
                    blocks.push((lx, floor + dy, lz, wall));
                }
            } else {
                // Clear interior + ceiling so the room reads underground.
                blocks.push((lx, floor + 2, lz, cobble));
            }
        }
    }
    // East door into the mine pit corridor.
    let door_x = (cx + 2 - chunk_x * 16) as u8;
    let door_z = (cz - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == door_x && *z == door_z && *y >= floor && *y <= floor + 1));
    // Short hall toward the pit (local x grows toward the mouth).
    for step in 1..=2 {
        let hx = (cx + 2 + step - chunk_x * 16) as u8;
        let hz = door_z;
        blocks.retain(|(x, y, z, _)| {
            !(*x == hx && *z == hz && *y >= floor.saturating_sub(1) && *y <= floor + 2)
        });
        blocks.push((hx, floor - 1, hz, cobble));
        blocks.push((hx, floor + 2, hz, cobble));
    }
    // Chest on the west wall, spawner in the center.
    let chest_x = (DUNGEON_CHEST.0 - chunk_x * 16) as u8;
    let chest_z = (DUNGEON_CHEST.1 - chunk_z * 16) as u8;
    blocks.push((chest_x, floor, chest_z, Block::new("chest", 0)));
    let spawn_x = (DUNGEON_SPAWNER.0 - chunk_x * 16) as u8;
    let spawn_z = (DUNGEON_SPAWNER.1 - chunk_z * 16) as u8;
    blocks.push((spawn_x, floor, spawn_z, Block::new("trial_spawner", 0)));
    // Mossy torch corner so the room isn't pitch black in a still.
    let torch_x = (cx + 1 - chunk_x * 16) as u8;
    let torch_z = (cz - 1 - chunk_z * 16) as u8;
    blocks.push((torch_x, floor, torch_z, Block::new("torch", 0)));
}

/// Abandoned timber spur east of the pit — oak posts, fence braces, rails, chest.
/// Only the shaft chunk gets the guaranteed demo.
fn stamp_mineshaft(blocks: &mut Vec<(u8, u16, u8, Block)>, chunk_x: i32, chunk_z: i32) {
    if (chunk_x, chunk_z) != SHAFT_CHUNK {
        return;
    }
    let floor = MINESHAFT_FLOOR;
    let log = Block::new("oak_log", 0);
    let fence = Block::new("fence", 0);
    let gravel = Block::new("gravel", 0);
    let cobble = Block::new("cobblestone", 0);
    let rail = Block::new("rail", 0);
    // Start at the east pit wall so the spur opens from the ladder hole.
    let (start_x, end_x) = (SHAFT_MOUTH.0 + 1, MINESHAFT_CHEST.0);
    let cz = SHAFT_MOUTH.1;
    for wx in start_x..=end_x {
        for wz in (cz - 1)..=(cz + 1) {
            let lx = (wx - chunk_x * 16) as u8;
            let lz = (wz - chunk_z * 16) as u8;
            blocks.retain(|(x, y, z, _)| {
                !(*x == lx && *z == lz && *y >= floor.saturating_sub(1) && *y <= floor + 2)
            });
            // Gravel bed under the rails; cobble flanks.
            let bed = if wz == cz { gravel } else { cobble };
            blocks.push((lx, floor - 1, lz, bed));
            // Timber ceiling beam.
            blocks.push((lx, floor + 2, lz, log));
        }
        let mid_x = (wx - chunk_x * 16) as u8;
        let mid_z = (cz - chunk_z * 16) as u8;
        // Center rail run.
        blocks.push((mid_x, floor, mid_z, rail));
        // Side posts + fence braces every other column.
        if (wx - start_x).rem_euclid(2) == 0 {
            for &side in &[-1i32, 1] {
                let px = mid_x;
                let pz = ((cz + side) - chunk_z * 16) as u8;
                blocks.push((px, floor, pz, log));
                blocks.push((px, floor + 1, pz, fence));
            }
        }
    }
    // Chest at the east stub, north of the rail so the track stays clear.
    let chest_x = (MINESHAFT_CHEST.0 - chunk_x * 16) as u8;
    let chest_z = (MINESHAFT_CHEST.1 - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == chest_x && *z == chest_z && *y == floor));
    blocks.push((chest_x, floor, chest_z, Block::new("chest", 0)));
    // Torch on a south brace post so the still isn't black.
    let torch_x = ((SHAFT_MOUTH.0 + 3) - chunk_x * 16) as u8;
    let torch_z = ((SHAFT_MOUTH.1 - 1) - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == torch_x && *z == torch_z && *y == floor + 1));
    blocks.push((torch_x, floor + 1, torch_z, Block::new("torch", 0)));
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
    // South wall of the pit: climb out without a jump.
    let ladder_x = (SHAFT_LADDER.0 - chunk_x * 16) as u8;
    let ladder_z = (SHAFT_LADDER.1 - chunk_z * 16) as u8;
    for y in 1..floor {
        blocks.push((ladder_x, y, ladder_z, Block::new("ladder", 0)));
    }
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
    // North of the chest: clear Efficiency or sharpen a worn pick.
    let grind_x = (SHAFT_GRIND.0 - chunk_x * 16) as u8;
    let grind_z = (SHAFT_GRIND.1 - chunk_z * 16) as u8;
    blocks.push((grind_x, floor, grind_z, Block::new("grindstone", 0)));
    // East of the chest: spend iron ore to upgrade a pick.
    let smith_x = (SHAFT_SMITH.0 - chunk_x * 16) as u8;
    let smith_z = (SHAFT_SMITH.1 - chunk_z * 16) as u8;
    blocks.push((smith_x, floor, smith_z, Block::new("smithing_table", 0)));
    // Iron ore beside it so the upgrade loop is findable without creative give.
    blocks.push((smith_x, floor, smith_z + 1, Block::new("iron_ore", 0)));
    // West of the chest: repair a worn pick for one iron ore.
    let anvil_x = (SHAFT_ANVIL.0 - chunk_x * 16) as u8;
    let anvil_z = (SHAFT_ANVIL.1 - chunk_z * 16) as u8;
    blocks.push((anvil_x, floor, anvil_z, Block::new("anvil", 0)));
    // Northwest of the mouth: link a compass for the way back.
    let stone_x = (SHAFT_LODESTONE.0 - chunk_x * 16) as u8;
    let stone_z = (SHAFT_LODESTONE.1 - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| !(*x == stone_x && *z == stone_z && *y >= floor));
    blocks.push((stone_x, floor, stone_z, Block::new("lodestone", 0)));
    // West of the lodestone: a short emerald vein for the trader loop.
    let ore_x = (EMERALD_ORE.0 - chunk_x * 16) as u8;
    let ore_z = (EMERALD_ORE.1 - chunk_z * 16) as u8;
    blocks.retain(|(x, y, z, _)| {
        !((*x == ore_x && *z == ore_z && *y >= floor.saturating_sub(1))
            || (*x == ore_x && *z == ore_z.saturating_sub(1) && *y == floor))
    });
    blocks.push((ore_x, floor.saturating_sub(1), ore_z, Block::new("emerald_ore", 0)));
    blocks.push((ore_x, floor, ore_z, Block::new("emerald_ore", 0)));
    if ore_z > 0 {
        blocks.push((ore_x, floor, ore_z - 1, Block::new("emerald_ore", 0)));
    }
    // East of the apron: a short rail spur for a minecart ride.
    let rail_z = (SHAFT_MOUTH.1 - chunk_z * 16) as u8;
    for dx in 4..=7 {
        let lx = (SHAFT_MOUTH.0 + dx - chunk_x * 16) as u8;
        blocks.retain(|(x, y, z, _)| !(*x == lx && *z == rail_z && *y + 1 >= floor));
        let top = blocks
            .iter()
            .filter(|(x, _, z, _)| *x == lx && *z == rail_z)
            .map(|(_, y, _, _)| *y)
            .max()
            .unwrap_or(0);
        for y in (top + 1)..floor.saturating_sub(1) {
            blocks.push((lx, y, rail_z, Block::new("dirt", 0)));
        }
        blocks.push((lx, floor - 1, rail_z, Block::new("gravel", 0)));
        // Mid spur is powered so a ride gets a kick.
        let id = if dx == 5 { "powered_rail" } else { "rail" };
        blocks.push((lx, floor, rail_z, Block::new(id, 0)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Hills,
    Marsh,
    Desert,
}

/// Guaranteed sand pocket around the demo desert temple.
fn desert_pocket(x: i32, z: i32) -> bool {
    (x - DESERT_TEMPLE.0).abs() <= 14 && (z - DESERT_TEMPLE.1).abs() <= 14
}

/// Plains, forest, hills, marsh, or desert. The noise cell is 32 blocks, the same on every world size.
pub fn biome_at(x: i32, z: i32) -> Biome {
    if desert_pocket(x, z) {
        return Biome::Desert;
    }
    let n = value_noise(x, z, 32);
    if n < 0.32 {
        Biome::Marsh
    } else if n < 0.55 {
        Biome::Plains
    } else if n < 0.78 {
        Biome::Forest
    } else if n < 0.90 {
        Biome::Hills
    } else {
        Biome::Desert
    }
}

fn column_height(x: i32, z: i32, biome: Biome) -> u16 {
    let n = value_noise(x, z, 8);
    let height = match biome {
        Biome::Marsh => 3,
        Biome::Plains | Biome::Desert => 4 + (n > 0.72) as u16,
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
            Biome::Desert => Block::new("sand", 0),
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
        Biome::Desert => [1.08, 0.96, 0.72],
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
/// Emerald is rare in hills, mid-height stone.
fn vein(x: i32, y: u16, z: i32) -> Option<&'static str> {
    let h = hash3(x.div_euclid(2), i32::from(y).div_euclid(2), z.div_euclid(2));
    if biome_at(x, z) == Biome::Hills && (4..=8).contains(&y) && h % 47 == 0 {
        return Some("emerald_ore");
    }
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
        assert!(sky_tint(Biome::Desert)[0] > sky_tint(Biome::Desert)[2]);
        assert_eq!(biome_at(40, 24), biome_at(40, 24));
        assert_eq!(biome_at(DESERT_TEMPLE.0, DESERT_TEMPLE.1), Biome::Desert);
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
        assert_eq!(camp.block(x + 2, y, z + 1).id, "cartography_table");
        assert_eq!(
            (2 * 16 + i32::from(x + 2), -2 * 16 + i32::from(z + 1)),
            CAMP_CHART
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
        assert_eq!(camp.block(x - 2, y, z + 2).id, "beehive");
        assert_eq!(
            (2 * 16 + i32::from(x - 2), -2 * 16 + i32::from(z + 2)),
            CAMP_HIVE
        );
        assert_eq!(camp.block(x + 2, y, z - 2).id, "cauldron");
        assert_eq!(
            (2 * 16 + i32::from(x + 2), -2 * 16 + i32::from(z - 2)),
            CAMP_CAULDRON
        );
        assert_eq!(camp.block(x + 5, y - 1, z).id, "farmland");
        assert_eq!(camp.block(x + 5, y, z).id, "wheat_crop");
        assert_eq!(
            (2 * 16 + i32::from(x + 5), -2 * 16 + i32::from(z)),
            CAMP_FARM
        );
        assert_eq!(camp.block(x + 4, y, z).id, "fence_gate");
        assert_eq!(
            (2 * 16 + i32::from(x + 4), -2 * 16 + i32::from(z)),
            CAMP_FARM_GATE
        );
        assert_eq!(camp.block(x + 4, y, z - 1).id, "fence");
        assert_eq!(camp.block(x + 5, y, z - 3).id, "fence");
        assert_eq!(camp.block(x + 5, y + 1, z - 3).id, "pumpkin");
        assert_eq!(camp.block(x + 5, y + 1, z - 3).metadata, 1);
        assert_eq!(
            (2 * 16 + i32::from(x + 5), -2 * 16 + i32::from(z - 3)),
            CAMP_SCARECROW
        );
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
        assert_eq!(shrine.block(local_x - 2, floor, local_z + 2).id, "enchanting_table");
        assert_eq!(
            (2 * 16 + i32::from(local_x - 2), 2 * 16 + i32::from(local_z + 2)),
            SHRINE_ENCHANT
        );
        assert_eq!(shrine.block(local_x - 2, floor, local_z + 1).id, "bookshelf");
        assert_eq!(shrine.block(local_x + 2, floor, local_z + 2).id, "brewing_stand");
        assert_eq!(
            (2 * 16 + i32::from(local_x + 2), 2 * 16 + i32::from(local_z + 2)),
            SHRINE_BREW
        );
        assert_eq!(shrine.block(local_x + 1, floor, local_z + 2).id, "nether_wart_block");
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
        assert_ne!(SHRINE_CHUNK, PORTAL_CHUNK);
    }

    #[test]
    fn a_ruined_portal_stands_northwest_of_the_pad() {
        let portal = generate_chunk(-2, -2);
        let (cx, cz) = RUINED_PORTAL;
        let lx = (cx - (-2) * 16) as u8;
        let lz = (cz - (-2) * 16) as u8;
        let floor = column_height(cx, cz, biome_at(cx, cz)).max(2);
        let mut obsidian = false;
        let mut crying = false;
        for y in floor..(floor + 5) {
            let id = portal.block(lx, y, lz).id;
            if id == "obsidian" {
                obsidian = true;
            }
            if id == "crying_obsidian" {
                crying = true;
            }
        }
        assert!(obsidian, "west column should include obsidian");
        assert!(crying, "frame should include crying obsidian");
        let chest_lx = (RUINED_PORTAL_CHEST.0 - (-2) * 16) as u8;
        let chest_lz = (RUINED_PORTAL_CHEST.1 - (-2) * 16) as u8;
        assert_eq!(
            portal.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest on the apron"
        );
        assert_ne!(PORTAL_CHUNK, CAMP_CHUNK);
        assert_ne!(PORTAL_CHUNK, SHAFT_CHUNK);
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(RUINED_PORTAL_CHEST.0, i32::from(floor), RUINED_PORTAL_CHEST.1)
                .id,
            "chest"
        );
    }

    #[test]
    fn a_nether_pocket_embeds_ancient_debris() {
        let pocket = generate_chunk(NETHER_CHUNK.0, NETHER_CHUNK.1);
        let floor = 4u16;
        for &(wx, wz) in &[NETHER_DEBRIS, NETHER_DEBRIS_B] {
            let lx = (wx - NETHER_CHUNK.0 * 16) as u8;
            let lz = (wz - NETHER_CHUNK.1 * 16) as u8;
            assert_eq!(
                pocket.block(lx, floor - 1, lz).id,
                "ancient_debris",
                "debris at ({wx},{wz})"
            );
        }
        assert_ne!(NETHER_DEBRIS, NETHER_PAD);
        assert_ne!(NETHER_DEBRIS_B, NETHER_DEBRIS);
    }

    #[test]
    fn a_nether_pocket_holds_a_lit_return_portal() {
        let pocket = generate_chunk(NETHER_CHUNK.0, NETHER_CHUNK.1);
        let (cx, cz) = NETHER_PAD;
        let lx = (cx - NETHER_CHUNK.0 * 16) as u8;
        let lz = (cz - NETHER_CHUNK.1 * 16) as u8;
        let floor = 4u16;
        assert_eq!(
            pocket.block(lx, floor, lz).id,
            "obsidian",
            "west foot of the return frame"
        );
        assert_eq!(
            pocket.block(lx + 1, floor + 1, lz).id,
            "nether_portal",
            "lit portal plane inside the frame"
        );
        assert_eq!(
            (cx + 1, cz),
            NETHER_PORTAL_IN,
            "interior standing cell"
        );
        assert_ne!(NETHER_CHUNK, PORTAL_CHUNK);
        assert_ne!(NETHER_CHUNK, CAMP_CHUNK);
    }

    #[test]
    fn a_nether_fortress_spur_runs_west_of_the_pocket() {
        let pocket = generate_chunk(NETHER_CHUNK.0, NETHER_CHUNK.1);
        let floor = NETHER_FORTRESS_FLOOR;
        let (fx, fz) = NETHER_FORTRESS;
        let lx = (fx - NETHER_CHUNK.0 * 16) as u8;
        let lz = (fz - NETHER_CHUNK.1 * 16) as u8;
        assert!(
            pocket.block(lx, floor - 1, lz).id == "nether_bricks"
                || pocket.block(lx, floor - 1, lz).id == "chiseled_nether_bricks"
                || pocket.block(lx, floor - 1, lz).id == "cracked_nether_bricks",
            "fortress bed should be nether brick"
        );
        assert_eq!(
            pocket.block(lx, floor + 1, lz).id,
            "air",
            "fortress walkway should stay clear"
        );
        let (cx, cz) = NETHER_FORTRESS_CHEST;
        let chest_lx = (cx - NETHER_CHUNK.0 * 16) as u8;
        let chest_lz = (cz - NETHER_CHUNK.1 * 16) as u8;
        assert_eq!(
            pocket.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest at the west stub"
        );
        let (bx, bz) = NETHER_FORTRESS_BLAZE;
        let blaze_lx = (bx - NETHER_CHUNK.0 * 16) as u8;
        let blaze_lz = (bz - NETHER_CHUNK.1 * 16) as u8;
        assert_eq!(
            pocket.block(blaze_lx, floor, blaze_lz).id,
            "soul_sand",
            "blaze-cage soul sand"
        );
        assert_eq!(
            pocket.block(blaze_lx, floor + 1, blaze_lz).id,
            "trial_spawner",
            "blaze-cage trial spawner stub"
        );
        assert_eq!(NETHER_FORTRESS.1, NETHER_PAD.1);
    }

    #[test]
    fn a_desert_temple_stands_east_of_the_pad() {
        assert_eq!(biome_at(DESERT_TEMPLE.0, DESERT_TEMPLE.1), Biome::Desert);
        let temple = generate_chunk(TEMPLE_CHUNK.0, TEMPLE_CHUNK.1);
        let (cx, cz) = DESERT_TEMPLE;
        let lx = (cx - TEMPLE_CHUNK.0 * 16) as u8;
        let lz = (cz - TEMPLE_CHUNK.1 * 16) as u8;
        let floor = column_height(cx, cz, Biome::Desert).max(2);
        let mut sandstone = false;
        for y in floor..(floor + 4) {
            if temple.block(lx + 3, y, lz).id == "sandstone"
                || temple.block(lx + 3, y, lz).id == "red_sandstone"
            {
                sandstone = true;
            }
        }
        assert!(sandstone, "east wall should include sandstone");
        let chest_lx = (DESERT_TEMPLE_CHEST.0 - TEMPLE_CHUNK.0 * 16) as u8;
        let chest_lz = (DESERT_TEMPLE_CHEST.1 - TEMPLE_CHUNK.1 * 16) as u8;
        assert_eq!(
            temple.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest inside the square"
        );
        let plate_lx = (DESERT_TEMPLE_PLATE.0 - TEMPLE_CHUNK.0 * 16) as u8;
        let plate_lz = (DESERT_TEMPLE_PLATE.1 - TEMPLE_CHUNK.1 * 16) as u8;
        assert_eq!(
            temple.block(plate_lx, floor, plate_lz).id,
            "plate",
            "pressure plate at the south gate"
        );
        assert_ne!(TEMPLE_CHUNK, CAMP_CHUNK);
        assert_ne!(TEMPLE_CHUNK, PORTAL_CHUNK);
        assert_ne!(TEMPLE_CHUNK, SHRINE_CHUNK);
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(DESERT_TEMPLE_CHEST.0, i32::from(floor), DESERT_TEMPLE_CHEST.1)
                .id,
            "chest"
        );
    }

    #[test]
    fn a_shipwreck_lies_west_of_the_pad() {
        let wreck = generate_chunk(WRECK_CHUNK.0, WRECK_CHUNK.1);
        let (cx, cz) = SHIPWRECK;
        let lx = (cx - WRECK_CHUNK.0 * 16) as u8;
        let lz = (cz - WRECK_CHUNK.1 * 16) as u8;
        let floor = column_height(cx, cz, biome_at(cx, cz)).max(3);
        let mut oak = false;
        for y in (floor - 1)..(floor + 4) {
            if wreck.block(lx, y, lz).id == "oak_log" {
                oak = true;
            }
        }
        assert!(oak, "hull should include oak_log");
        let chest_lx = (SHIPWRECK_CHEST.0 - WRECK_CHUNK.0 * 16) as u8;
        let chest_lz = (SHIPWRECK_CHEST.1 - WRECK_CHUNK.1 * 16) as u8;
        assert_eq!(
            wreck.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest in the hold"
        );
        // South flank should hold shallow water.
        let water_lz = (cz + 3 - WRECK_CHUNK.1 * 16) as u8;
        assert_eq!(
            wreck.block(lx, floor - 1, water_lz).id,
            "water",
            "beach water on the south side"
        );
        assert_ne!(WRECK_CHUNK, CAMP_CHUNK);
        assert_ne!(WRECK_CHUNK, PORTAL_CHUNK);
        assert_ne!(WRECK_CHUNK, TEMPLE_CHUNK);
        assert_ne!(WRECK_CHUNK, MONUMENT_CHUNK);
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(SHIPWRECK_CHEST.0, i32::from(floor), SHIPWRECK_CHEST.1)
                .id,
            "chest"
        );
    }

    #[test]
    fn an_ocean_monument_remnant_lies_south_of_the_wreck() {
        let monument = generate_chunk(MONUMENT_CHUNK.0, MONUMENT_CHUNK.1);
        let (cx, cz) = OCEAN_MONUMENT;
        let lx = (cx - MONUMENT_CHUNK.0 * 16) as u8;
        let lz = (cz - MONUMENT_CHUNK.1 * 16) as u8;
        let surface = column_height(cx, cz, biome_at(cx, cz)).max(5);
        let floor = surface.saturating_sub(2).max(2);
        let mut prism = false;
        for y in floor..(floor + 3) {
            if matches!(
                monument.block(lx, y, lz).id,
                "prismarine" | "prismarine_bricks" | "dark_prismarine" | "chest" | "water"
            ) {
                prism = true;
            }
        }
        assert!(prism, "monument hall should include prismarine or water");
        let chest_lx = (OCEAN_MONUMENT_CHEST.0 - MONUMENT_CHUNK.0 * 16) as u8;
        let chest_lz = (OCEAN_MONUMENT_CHEST.1 - MONUMENT_CHUNK.1 * 16) as u8;
        assert_eq!(
            monument.block(chest_lx, floor + 1, chest_lz).id,
            "chest",
            "loot chest in the monument hall"
        );
        let gold_lx = (OCEAN_MONUMENT_GOLD.0 - MONUMENT_CHUNK.0 * 16) as u8;
        let gold_lz = (OCEAN_MONUMENT_GOLD.1 - MONUMENT_CHUNK.1 * 16) as u8;
        assert_eq!(
            monument.block(gold_lx, floor, gold_lz).id,
            "gold_block",
            "gold treasure nook"
        );
        let sponge_lx = ((OCEAN_MONUMENT_GOLD.0 + 2) - MONUMENT_CHUNK.0 * 16) as u8;
        assert_eq!(
            monument.block(sponge_lx, floor, gold_lz).id,
            "sponge",
            "sponge pad beside the gold"
        );
        assert_ne!(MONUMENT_CHUNK, WRECK_CHUNK);
        assert_ne!(MONUMENT_CHUNK, CAMP_CHUNK);
        assert_ne!(MONUMENT_CHUNK, STRONGHOLD_CHUNK);
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(
                    OCEAN_MONUMENT_CHEST.0,
                    i32::from(floor + 1),
                    OCEAN_MONUMENT_CHEST.1
                )
                .id,
            "chest"
        );
    }

    #[test]
    fn a_stronghold_portal_room_lies_southeast_of_the_pad() {
        let hold = generate_chunk(STRONGHOLD_CHUNK.0, STRONGHOLD_CHUNK.1);
        let (cx, cz) = STRONGHOLD;
        let floor = STRONGHOLD_FLOOR;
        let chest_lx = (STRONGHOLD_CHEST.0 - STRONGHOLD_CHUNK.0 * 16) as u8;
        let chest_lz = (STRONGHOLD_CHEST.1 - STRONGHOLD_CHUNK.1 * 16) as u8;
        assert_eq!(
            hold.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest in the portal room"
        );
        let north_lx = (cx - STRONGHOLD_CHUNK.0 * 16) as u8;
        let north_lz = (cz - 2 - STRONGHOLD_CHUNK.1 * 16) as u8;
        let north = hold.block(north_lx, floor, north_lz);
        assert_eq!(north.id, "end_portal_frame");
        assert_eq!(north.metadata, 1, "north frames start with eyes set");
        let south_lz = (cz + 2 - STRONGHOLD_CHUNK.1 * 16) as u8;
        let south = hold.block(north_lx, floor, south_lz);
        assert_eq!(south.id, "end_portal_frame");
        assert_eq!(south.metadata, 0, "south frames wait for eyes");
        let pit_lx = (cx - STRONGHOLD_CHUNK.0 * 16) as u8;
        let pit_lz = (cz - STRONGHOLD_CHUNK.1 * 16) as u8;
        assert_eq!(
            hold.block(pit_lx, floor - 1, pit_lz).id,
            "lava",
            "lava under the portal pit"
        );
        let wall_lx = (cx + 4 - STRONGHOLD_CHUNK.0 * 16) as u8;
        assert!(
            matches!(
                hold.block(wall_lx, floor + 1, pit_lz).id,
                "stone_bricks" | "mossy_stone_bricks" | "cracked_stone_bricks" | "chiseled_stone_bricks"
            ),
            "stone-brick walls"
        );
        assert_ne!(STRONGHOLD_CHUNK, CAMP_CHUNK);
        assert_ne!(STRONGHOLD_CHUNK, SHAFT_CHUNK);
        assert_ne!(STRONGHOLD_CHUNK, MONUMENT_CHUNK);
        assert_ne!(STRONGHOLD_CHUNK, END_CHUNK);
    }

    #[test]
    fn an_end_pocket_holds_a_return_portal_south_of_the_stronghold() {
        let pocket = generate_chunk(END_CHUNK.0, END_CHUNK.1);
        let (cx, cz) = END_PAD;
        let lx = (cx - END_CHUNK.0 * 16) as u8;
        let lz = (cz - END_CHUNK.1 * 16) as u8;
        let floor = END_FLOOR;
        assert_eq!(
            pocket.block(lx, floor, lz).id,
            "obsidian",
            "pad center is obsidian"
        );
        assert_eq!(
            pocket.block(lx + 2, floor, lz).id,
            "end_stone",
            "rim accents are end stone"
        );
        assert_eq!(
            pocket.block(lx, floor + 1, lz).id,
            "end_portal",
            "return portal on the pad"
        );
        assert_eq!((cx, cz + 1), END_PORTAL_IN, "standing cell beside return");
        assert_eq!((cx, cz), END_RETURN, "return portal cell");
        assert_eq!(
            pocket.block(lx + 2, floor + 1, lz).id,
            "end_rod",
            "rod lights the void pad"
        );
        assert_ne!(END_CHUNK, STRONGHOLD_CHUNK);
        assert_ne!(END_CHUNK, NETHER_CHUNK);
        assert_ne!(END_CHUNK, PORTAL_CHUNK);
    }

    #[test]
    fn an_end_city_tower_stands_west_of_the_pad() {
        let pocket = generate_chunk(END_CHUNK.0, END_CHUNK.1);
        let (cx, cz) = END_CITY;
        let lx = (cx - END_CHUNK.0 * 16) as u8;
        let lz = (cz - END_CHUNK.1 * 16) as u8;
        let floor = END_CITY_FLOOR;
        assert_eq!(
            pocket.block(lx, floor, lz).id,
            "end_stone",
            "tower foundation"
        );
        assert!(
            matches!(
                pocket.block(lx + 1, floor + 3, lz + 1).id,
                "purpur" | "purpur_pillar"
            ),
            "purpur shell"
        );
        let chest_lx = (END_CITY_CHEST.0 - END_CHUNK.0 * 16) as u8;
        let chest_lz = (END_CITY_CHEST.1 - END_CHUNK.1 * 16) as u8;
        assert_eq!(
            pocket.block(chest_lx, floor + 8, chest_lz).id,
            "chest",
            "loot chest near the top"
        );
        assert_eq!(
            pocket.block(lx, floor + 12, lz).id,
            "end_rod",
            "spire rod"
        );
        assert_eq!(END_CITY_FLOOR, END_FLOOR);
        assert_ne!((cx, cz), END_PAD);
    }

    #[test]
    fn a_chorus_grove_stands_east_of_the_pad() {
        let pocket = generate_chunk(END_CHUNK.0, END_CHUNK.1);
        let (cx, cz) = END_CHORUS;
        let lx = (cx - END_CHUNK.0 * 16) as u8;
        let lz = (cz - END_CHUNK.1 * 16) as u8;
        let floor = END_CHORUS_FLOOR;
        assert_eq!(
            pocket.block(lx, floor, lz).id,
            "end_stone",
            "chorus islet floor"
        );
        assert_eq!(
            pocket.block(lx, floor + 1, lz).id,
            "chorus_plant",
            "center stalk"
        );
        assert_eq!(
            pocket.block(lx, floor + 3, lz).id,
            "chorus_flower",
            "center bloom"
        );
        assert_eq!(
            pocket.block(lx + 1, floor + 3, lz).id,
            "chorus_flower",
            "side bloom"
        );
        assert_eq!(END_CHORUS_FLOOR, END_FLOOR);
        assert_ne!((cx, cz), END_PAD);
        assert_ne!((cx, cz), END_CITY);
    }

    #[test]
    fn a_village_remnant_stands_north_of_the_pad() {
        let village = generate_chunk(VILLAGE_CHUNK.0, VILLAGE_CHUNK.1);
        let (cx, cz) = VILLAGE;
        let lx = (cx - VILLAGE_CHUNK.0 * 16) as u8;
        let lz = (cz - VILLAGE_CHUNK.1 * 16) as u8;
        let floor = column_height(cx, cz, biome_at(cx, cz)).max(3);
        assert_eq!(
            village.block(lx, floor, lz).id,
            "water",
            "well water at plaza center"
        );
        let chest_lx = (VILLAGE_CHEST.0 - VILLAGE_CHUNK.0 * 16) as u8;
        let chest_lz = (VILLAGE_CHEST.1 - VILLAGE_CHUNK.1 * 16) as u8;
        assert_eq!(
            village.block(chest_lx, floor, chest_lz).id,
            "chest",
            "loot chest in the west house"
        );
        // Green south of the well stays open (no house walls on the NPC pad).
        let green_lx = (VILLAGE_GREEN.0 - VILLAGE_CHUNK.0 * 16) as u8;
        let green_lz = (VILLAGE_GREEN.1 - VILLAGE_CHUNK.1 * 16) as u8;
        assert!(
            village.block(green_lx, floor, green_lz).is_air(),
            "village green should stay open for future NPCs"
        );
        assert_ne!(VILLAGE_CHUNK, CAMP_CHUNK);
        assert_ne!(VILLAGE_CHUNK, WRECK_CHUNK);
        assert_ne!(VILLAGE_CHUNK, PORTAL_CHUNK);
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(
            cluster
                .block_at(VILLAGE_CHEST.0, i32::from(floor), VILLAGE_CHEST.1)
                .id,
            "chest"
        );
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
        assert_eq!(shaft.block(10, post, local_z - 1).id, "grindstone");
        assert_eq!(
            (-2 * 16 + 10, 2 * 16 + i32::from(local_z - 1)),
            SHAFT_GRIND
        );
        assert_eq!(shaft.block(11, post, local_z).id, "smithing_table");
        assert_eq!(
            (-2 * 16 + 11, 2 * 16 + i32::from(local_z)),
            SHAFT_SMITH
        );
        assert_eq!(shaft.block(11, post, local_z + 1).id, "iron_ore");
        assert_eq!(shaft.block(9, post, local_z).id, "anvil");
        assert_eq!(
            (-2 * 16 + 9, 2 * 16 + i32::from(local_z)),
            SHAFT_ANVIL
        );
        assert_eq!(shaft.block(local_x, 1, local_z - 1).id, "ladder");
        assert_eq!(shaft.block(local_x, post - 1, local_z - 1).id, "ladder");
        assert_eq!(
            (-2 * 16 + i32::from(local_x), 2 * 16 + i32::from(local_z - 1)),
            SHAFT_LADDER
        );
        assert_eq!(shaft.block(local_x + 7, post, local_z).id, "rail");
        assert_eq!(shaft.block(local_x + 4, post, local_z).id, "rail");
        assert_eq!(shaft.block(local_x + 5, post, local_z).id, "powered_rail");
        assert_eq!(
            (-2 * 16 + i32::from(local_x + 7), 2 * 16 + i32::from(local_z)),
            SHAFT_RAIL
        );
        assert_eq!(
            shaft.block(
                (SHAFT_LODESTONE.0 - (-2 * 16)) as u8,
                post,
                (SHAFT_LODESTONE.1 - 2 * 16) as u8
            )
            .id,
            "lodestone"
        );
        assert_eq!(
            (
                -2 * 16 + (SHAFT_LODESTONE.0 - (-2 * 16)),
                2 * 16 + (SHAFT_LODESTONE.1 - 2 * 16)
            ),
            SHAFT_LODESTONE
        );
        assert_eq!(
            shaft
                .block(
                    (EMERALD_ORE.0 - (-2 * 16)) as u8,
                    post,
                    (EMERALD_ORE.1 - 2 * 16) as u8
                )
                .id,
            "emerald_ore"
        );
        assert_eq!(
            (
                -2 * 16 + (EMERALD_ORE.0 - (-2 * 16)),
                2 * 16 + (EMERALD_ORE.1 - 2 * 16)
            ),
            EMERALD_ORE
        );
        let dungeon_lx = (DUNGEON.0 - (-2 * 16)) as u8;
        let dungeon_lz = (DUNGEON.1 - 2 * 16) as u8;
        assert_eq!(
            shaft.block(dungeon_lx, DUNGEON_FLOOR, dungeon_lz).id,
            "trial_spawner",
            "dungeon spawner at room center"
        );
        let chest_lx = (DUNGEON_CHEST.0 - (-2 * 16)) as u8;
        let chest_lz = (DUNGEON_CHEST.1 - 2 * 16) as u8;
        assert_eq!(
            shaft.block(chest_lx, DUNGEON_FLOOR, chest_lz).id,
            "chest",
            "dungeon chest on the west wall"
        );
        let floor_id = shaft
            .block(dungeon_lx, DUNGEON_FLOOR - 1, dungeon_lz)
            .id;
        assert!(
            floor_id == "cobblestone" || floor_id == "mossy_cobblestone",
            "dungeon floor is cobble/mossy, got {floor_id}"
        );
        let shaft_lx = (MINESHAFT.0 - (-2 * 16)) as u8;
        let shaft_lz = (MINESHAFT.1 - 2 * 16) as u8;
        assert_eq!(
            shaft.block(shaft_lx, MINESHAFT_FLOOR, shaft_lz).id,
            "rail",
            "mineshaft spur has a center rail"
        );
        let mchest_lx = (MINESHAFT_CHEST.0 - (-2 * 16)) as u8;
        let mchest_lz = (MINESHAFT_CHEST.1 - 2 * 16) as u8;
        assert_eq!(
            shaft.block(mchest_lx, MINESHAFT_FLOOR, mchest_lz).id,
            "chest",
            "mineshaft chest at the east stub"
        );
        let post_lx = ((SHAFT_MOUTH.0 + 3) - (-2 * 16)) as u8;
        assert!(
            shaft.block(post_lx, MINESHAFT_FLOOR, shaft_lz - 1).id == "oak_log"
                || shaft.block(post_lx, MINESHAFT_FLOOR, shaft_lz + 1).id == "oak_log",
            "timber posts brace the spur"
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
    fn lily_pads_sit_under_the_frog_hop_square() {
        let chunk = generate_chunk(FROG_CHUNK.0, FROG_CHUNK.1);
        for &(wx, wz) in &FROG_PADS {
            let lx = (wx - FROG_CHUNK.0 * 16) as u8;
            let lz = (wz - FROG_CHUNK.1 * 16) as u8;
            let height = column_height(wx, wz, Biome::Marsh);
            assert_eq!(chunk.block(lx, height, lz).id, "water");
            assert_eq!(
                chunk.block(lx, height + 1, lz).id,
                "lily_pad",
                "pad at ({wx}, {wz})"
            );
        }
        let mut cluster = synthetic_cluster();
        seed_terrain_ring(&mut cluster);
        assert_eq!(cluster.block_at(8, 4, 33).id, "lily_pad");
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
