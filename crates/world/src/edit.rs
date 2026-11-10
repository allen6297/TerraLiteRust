//! The only way a block changes.
//!
//! Break and place both go through [`apply`]. The server calls
//! [`apply_authoritative`](crate::apply_authoritative), which runs the mod
//! hooks around this function. The caller picks the cell. This function
//! decides what is left behind: air, or the stone named in the pack when the
//! block is an ore. A place that is not a registered block, or that overlaps
//! the player, does not change the world. A block from `block.register`
//! places. A script item does not.

use crate::chunk::{Block, compound_host};
use crate::cluster::Cluster;
use crate::pack::{Pack, for_each_collider, shape_of};

/// Axis-aligned player box, in world blocks.
#[derive(Clone, Copy, Debug)]
pub struct PlayerBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// One requested world change.
#[derive(Clone, Copy, Debug)]
pub enum Edit {
    Break {
        x: i32,
        y: i32,
        z: i32,
    },
    Place {
        x: i32,
        y: i32,
        z: i32,
        block: Block,
        player: PlayerBox,
    },
}

/// Apply `edit` to `world`. Returns whether a cell changed.
pub fn apply(world: &mut Cluster, edit: Edit) -> bool {
    match edit {
        Edit::Break { x, y, z } => break_block(world, x, y, z),
        Edit::Place {
            x,
            y,
            z,
            block,
            player,
        } => place_block(world, x, y, z, block, player),
    }
}

fn break_block(world: &mut Cluster, x: i32, y: i32, z: i32) -> bool {
    let current = world.block_at(x, y, z);
    if current.is_air() || !world.in_bounds(x, y, z) {
        return false;
    }
    let next = Pack::builtin().break_result(current);
    world.set_block(x, y, z, next)
}

fn place_block(
    world: &mut Cluster,
    x: i32,
    y: i32,
    z: i32,
    block: Block,
    player: PlayerBox,
) -> bool {
    if !place_fits(world, x, y, z, block, player) {
        return false;
    }
    world.set_block(x, y, z, block)
}

/// The wall is one cell west, east, north, or south of a painting or a vine.
fn wall_support(metadata: u8) -> Option<(i32, i32)> {
    match metadata {
        1 => Some((-1, 0)),
        2 => Some((1, 0)),
        3 => Some((0, -1)),
        4 => Some((0, 1)),
        _ => None,
    }
}

/// Sand with water beside it, or another cane so a stalk can stack.
fn sugar_cane_supported(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    if y <= 0 {
        return false;
    }
    let below = world.block_at(x, y - 1, z);
    if below.id == "sugar_cane" {
        return true;
    }
    if below.id != "sand" {
        return false;
    }
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .any(|(dx, dz)| world.block_at(x + dx, y - 1, z + dz).id == "water")
}

/// End stone, or another chorus plant so a stalk can stack.
fn chorus_supported(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    if y <= 0 {
        return false;
    }
    let below = world.block_at(x, y - 1, z);
    below.id == "end_stone" || below.id == "chorus_plant"
}

fn swings(id: &str) -> bool {
    matches!(
        id,
        "door"
            | "copper_door"
            | "trapdoor"
            | "copper_trapdoor"
            | "fence_gate"
            | "lever"
            | "note_block"
            | "flower_pot"
            | "end_portal_frame"
            | "cake"
            | "pumpkin"
            | "lectern"
            | "chiseled_bookshelf"
            | "composter"
            | "beehive"
            | "cauldron"
            | "bell"
            | "candle"
            | "respawn_anchor"
            | "lightning_rod"
            | "target"
            | "trial_spawner"
            | "copper_bulb"
            | "lamp"
            | "spout"
            | "drain"
            | "generator"
            | "pump"
            | "decorated_pot"
            | "shelf"
            | "item_frame"
            | "copper_chest"
            | "sand"
            | "gravel"
            | "soul_sand"
            | "dirt"
            | "clay"
            | "grass"
            | "wheat_crop"
    )
}

fn place_fits(world: &Cluster, x: i32, y: i32, z: i32, block: Block, player: PlayerBox) -> bool {
    if !world.in_bounds(x, y, z) || block.is_air() || !Pack::builtin().contains_block(block.id) {
        return false;
    }
    let current = world.block_at(x, y, z);
    let swing = swings(current.id) && current.id == block.id && current.metadata != block.metadata;
    // Same host: empty↔fill only (drain or place; no kind swap without drain).
    // Pipes take water; wires take ember; other hosts take either.
    // Tanks may also change metadata when the fill kind stays (level up/down).
    let buffer = crate::chunk::is_buffer(current.id);
    let fill_update = compound_host(current.id)
        && current.id == block.id
        && (current.metadata == block.metadata || buffer)
        && current.fill != block.fill
        && (current.fill == 0 || block.fill == 0)
        && (block.fill == 0
            || crate::chunk::compound_accepts_meta(current.id, current.metadata, block.fill));
    let buffer_level = buffer
        && current.id == block.id
        && current.fill != 0
        && current.fill == block.fill
        && current.metadata != block.metadata;
    let till = block.id == "farmland" && (current.id == "dirt" || current.id == "grass");
    if !current.is_air() && !swing && !fill_update && !buffer_level && !till {
        return false;
    }
    if fill_update || till {
        return true;
    }
    if block.id == "rail" {
        let below = world.block_at(x, y - 1, z);
        return !below.is_air() && below.id != "rail" && below.id != "water";
    }
    let shape = shape_of(block);
    if shape.is_empty() {
        return false;
    }
    if block.id == "flower" {
        return y > 0 && world.block_at(x, y - 1, z).id == "grass";
    }
    if block.id == "wheat_crop" {
        if y <= 0 {
            return false;
        }
        let soil = world.block_at(x, y - 1, z).id;
        return soil == "dirt" || soil == "grass" || soil == "farmland";
    }
    if block.id == "flower_pot" {
        return true;
    }
    if block.id == "cactus" && (y <= 0 || world.block_at(x, y - 1, z).id != "sand") {
        return false;
    }
    if block.id == "sugar_cane" {
        return sugar_cane_supported(world, x, y, z);
    }
    if block.id == "chorus_plant" {
        return chorus_supported(world, x, y, z);
    }
    if block.id == "chorus_flower" {
        return y > 0 && world.block_at(x, y - 1, z).id == "chorus_plant";
    }
    if block.id == "lily_pad" && (y <= 0 || world.block_at(x, y - 1, z).id != "water") {
        return false;
    }
    if block.id == "snow"
        && (y <= 0
            || world.block_at(x, y - 1, z).id == "water"
            || !shape_of(world.block_at(x, y - 1, z)).is_full_cube())
    {
        return false;
    }
    if block.id == "painting"
        || block.id == "vine"
        || block.id == "item_frame"
        || block.id == "shelf"
    {
        let facing = if block.id == "shelf" {
            crate::pack::shelf_facing(block.metadata)
        } else if block.id == "item_frame" {
            block.metadata & 7
        } else {
            block.metadata
        };
        let Some((dx, dz)) = wall_support(facing) else {
            return false;
        };
        return shape_of(world.block_at(x + dx, y, z + dz)).is_full_cube();
    }
    let mut blocked = false;
    for_each_collider(block, |box_min, box_max| {
        if blocked {
            return;
        }
        let min = [
            x as f32 + box_min[0] / 16.0,
            y as f32 + box_min[1] / 16.0,
            z as f32 + box_min[2] / 16.0,
        ];
        let max = [
            x as f32 + box_max[0] / 16.0,
            y as f32 + box_max[1] / 16.0,
            z as f32 + box_max[2] / 16.0,
        ];
        if overlaps(min, max, player.min, player.max) {
            blocked = true;
        }
    });
    !blocked
}

/// True when [`apply`] would change a cell. Hooks use this so a refused edit
/// does not call them.
pub(crate) fn would_apply(world: &Cluster, edit: Edit) -> bool {
    match edit {
        Edit::Break { x, y, z } => !world.block_at(x, y, z).is_air() && world.in_bounds(x, y, z),
        Edit::Place {
            x,
            y,
            z,
            block,
            player,
        } => place_fits(world, x, y, z, block, player),
    }
}

fn overlaps(min: [f32; 3], max: [f32; 3], other_min: [f32; 3], other_max: [f32; 3]) -> bool {
    (0..3).all(|axis| max[axis] > other_min[axis] && min[axis] < other_max[axis])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk, FILL_EMBER, FILL_WATER};
    use crate::fixture::synthetic_cluster;
    use crate::pack::{Pack, light_of};

    fn far_player() -> PlayerBox {
        PlayerBox {
            min: [40.0, 40.0, 40.0],
            max: [40.6, 41.8, 40.6],
        }
    }

    #[test]
    fn a_tank_can_change_level_without_swapping_fill_kind() {
        use crate::chunk::{TANK_BUCKET, TANK_MAX, tank_at, tank_level};
        let chunk = Chunk::from_blocks(0, 0, [(2, 1, 2, tank_at(FILL_WATER, 3))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: tank_at(FILL_WATER, 7),
                player: far_player(),
            }
        ));
        assert_eq!(tank_level(world.block_at(2, 1, 2)), 7);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: tank_at(FILL_WATER, 1),
                player: far_player(),
            }
        ));
        assert_eq!(tank_level(world.block_at(2, 1, 2)), 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("tank", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2).fill, 0);
        assert_eq!(tank_level(world.block_at(2, 1, 2)), 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: tank_at(FILL_WATER, TANK_BUCKET.min(TANK_MAX)),
                player: far_player(),
            }
        ));
        assert_eq!(tank_level(world.block_at(2, 1, 2)), TANK_BUCKET.min(TANK_MAX));
    }

    #[test]
    fn a_grate_takes_water_or_ember_without_replacing_the_host() {
        let chunk =
            Chunk::from_blocks(0, 0, [(2, 1, 2, Block::new("copper_grate", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let wet = Block::new("copper_grate", 0).with_fill(FILL_WATER);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: wet,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), wet);
        let hot = wet.with_fill(FILL_EMBER);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: hot,
                player: far_player(),
            }
        ), "occupied fill does not swap kinds without drain");
        let clear = wet.with_fill(0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: clear,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2).fill, 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("copper_grate", 0).with_fill(FILL_EMBER),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2).fill, FILL_EMBER);
    }

    #[test]
    fn fence_glass_and_leaves_take_compound_fills() {
        use crate::chunk::{compound_accepts, compound_host};
        for id in ["fence", "glass", "leaves", "oak_stairs", "trapdoor", "ladder"] {
            assert!(compound_host(id), "{id} should host fills");
            let chunk = Chunk::from_blocks(0, 0, [(2, 1, 2, Block::new(id, 0))]).unwrap();
            let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
            let wet = Block::new(id, 0).with_fill(FILL_WATER);
            assert!(
                apply(
                    &mut world,
                    Edit::Place {
                        x: 2,
                        y: 1,
                        z: 2,
                        block: wet,
                        player: far_player(),
                    }
                ),
                "{id} accepts water fill"
            );
            assert_eq!(world.block_at(2, 1, 2).fill, FILL_WATER);
            assert!(
                light_of(Block::new(id, 0).with_fill(FILL_EMBER)).is_some(),
                "{id} ember fill should glow"
            );
        }
        assert!(!compound_host("stone"));
        assert!(!compound_host("cobblestone"));
        assert!(compound_host("pipe") && compound_accepts("pipe", FILL_WATER));
        assert!(!compound_accepts("pipe", FILL_EMBER));
        assert!(compound_host("wire") && compound_accepts("wire", FILL_EMBER));
        assert!(!compound_accepts("wire", FILL_WATER));
        let pipe = Chunk::from_blocks(0, 0, [(2, 1, 2, Block::new("pipe", 0))]).unwrap();
        let mut wet = Cluster::try_from_chunks(0, 0, [pipe.clone()]).unwrap();
        assert!(apply(
            &mut wet,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("pipe", 0).with_fill(FILL_WATER),
                player: far_player(),
            }
        ));
        let mut rejected = Cluster::try_from_chunks(0, 0, [pipe]).unwrap();
        assert!(
            !apply(
                &mut rejected,
                Edit::Place {
                    x: 2,
                    y: 1,
                    z: 2,
                    block: Block::new("pipe", 0).with_fill(FILL_EMBER),
                    player: far_player(),
                }
            ),
            "pipe rejects ember fill"
        );
    }

    #[test]
    fn a_normal_block_breaks_to_air() {
        let chunk = Chunk::from_blocks(0, 0, [(4, 1, 4, Block::new("grass", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        assert!(
            Pack::builtin()
                .break_result(Block::new("grass", 0))
                .is_air()
        );
        assert!(apply(&mut world, Edit::Break { x: 4, y: 1, z: 4 }));
        assert!(world.block_at(4, 1, 4).is_air());
        assert!(!apply(&mut world, Edit::Break { x: 4, y: 1, z: 4 }));
    }

    #[test]
    fn each_ore_breaks_to_its_own_stone() {
        let pack = Pack::builtin();
        let mut ores = Vec::new();
        for (id, _) in pack.blocks() {
            let left = pack.break_result(Block::new(id, 0));
            if !left.is_air() {
                ores.push((id, left));
            }
        }
        assert!(ores.contains(&("coal_ore", Block::new("stone", 0))));
        assert!(ores.contains(&("iron_ore", Block::new("deepslate", 0))));
        assert_eq!(ores.len(), 2, "each ore names its own stone");
        assert_ne!(ores[0].1, ores[1].1);

        let mut world = synthetic_cluster();
        assert_eq!(world.block_at(9, 1, 13), Block::new("coal_ore", 0));
        assert_eq!(world.block_at(10, 1, 13), Block::new("iron_ore", 0));
        assert!(apply(&mut world, Edit::Break { x: 9, y: 1, z: 13 }));
        assert_eq!(world.block_at(9, 1, 13), Block::new("stone", 0));
        assert_eq!(world.block_at(10, 1, 13), Block::new("iron_ore", 0));
        assert!(apply(&mut world, Edit::Break { x: 10, y: 1, z: 13 }));
        assert_eq!(world.block_at(10, 1, 13), Block::new("deepslate", 0));
        assert_eq!(world.block_at(9, 1, 13), Block::new("stone", 0));

        assert_eq!(world.block_at(8, 0, 14), Block::new("grass", 0));
        assert!(apply(&mut world, Edit::Break { x: 8, y: 0, z: 14 }));
        assert!(world.block_at(8, 0, 14).is_air());
    }

    #[test]
    fn a_door_place_swings_it_open_and_shut() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let door = Block::new("door", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: door,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), door);
        let open = Block::new("door", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
        let copper = Block::new("copper_door", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 2,
                z: 2,
                block: copper,
                player: far_player(),
            }
        ));
        let copper_open = Block::new("copper_door", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 2,
                z: 2,
                block: copper_open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(3, 2, 2), copper_open);
        assert_eq!(shape_of(copper).boxes()[0].max[2], 3);
        assert_eq!(shape_of(copper_open).boxes()[0].max[0], 3);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: open,
                player: far_player(),
            }
        ));
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
        let inside = PlayerBox {
            min: [2.0, 2.0, 2.0],
            max: [2.4, 3.6, 2.4],
        };
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: door,
                player: inside,
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
    }

    #[test]
    fn a_trapdoor_place_swings_it_open_and_shut() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let shut = Block::new("trapdoor", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: shut,
                player: far_player(),
            }
        ));
        let open = Block::new("trapdoor", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: shut,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), shut);
        let copper = Block::new("copper_trapdoor", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 2,
                z: 2,
                block: copper,
                player: far_player(),
            }
        ));
        let copper_open = Block::new("copper_trapdoor", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 2,
                z: 2,
                block: copper_open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(3, 2, 2), copper_open);
        assert_eq!(shape_of(copper).boxes()[0].min[1], 13);
        assert_eq!(shape_of(copper_open).boxes()[0].max[0], 3);
    }

    #[test]
    fn a_fence_gate_place_swings_it_open_and_shut() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let shut = Block::new("fence_gate", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: shut,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), shut);
        let open = Block::new("fence_gate", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), open);
    }

    #[test]
    fn a_lever_place_flips_it_on_and_off() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let off = Block::new("lever", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: off,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), off);
        let on = Block::new("lever", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: on,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), on);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: off,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), off);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), off);
    }

    #[test]
    fn a_note_block_place_steps_its_pitch() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let first = Block::new("note_block", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: first,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), first);
        let next = Block::new("note_block", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: next,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), next);
        let top = Block::new("note_block", 15);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: top,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), top);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: first,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), first);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 2, 2), first);
    }

    #[test]
    fn place_fills_an_empty_cell_and_refuses_the_player() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let inside = PlayerBox {
            min: [0.2, 1.0, 0.2],
            max: [0.8, 2.8, 0.8],
        };
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 1,
                z: 0,
                block: Block::new("cobblestone", 0),
                player: inside,
            }
        ));
        assert!(world.block_at(0, 1, 0).is_air());

        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 0,
                block: Block::new("cobblestone", 0),
                player: inside,
            }
        ));
        assert_eq!(world.block_at(2, 1, 0), Block::new("cobblestone", 0));
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 0,
                z: 0,
                block: Block::new("cobblestone", 0),
                player: far_player(),
            }
        ));
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 1,
                z: 0,
                block: Block::new("missing", 0),
                player: far_player(),
            }
        ));
        assert!(world.block_at(3, 1, 0).is_air());
    }

    #[test]
    fn a_flower_places_on_grass_and_not_on_stone() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 0, 1, Block::new("grass", 0)),
                (2, 0, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let flower = Block::new("flower", 0);
        let standing = PlayerBox {
            min: [0.7, 1.0, 0.7],
            max: [1.3, 2.8, 1.3],
        };
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: flower,
                player: standing,
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), flower);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: flower,
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: flower,
                player: far_player(),
            }
        ));
        assert!(world.block_at(1, 2, 1).is_air());
        let shape = shape_of(flower);
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
    }

    #[test]
    fn a_painting_hangs_on_a_full_wall_and_not_on_air() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let hung = Block::new("painting", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 1,
                z: 1,
                block: hung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(0, 1, 1), hung);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 2,
                z: 0,
                block: Block::new("painting", 0),
                player: far_player(),
            }
        ));
        assert!(world.block_at(0, 2, 0).is_air());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: Block::new("painting", 3),
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        let shape = shape_of(hung);
        assert_eq!(shape.boxes()[0].min[2], 0);
        assert_eq!(shape.boxes()[0].max[2], 1);
        let frame = Block::new("item_frame", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 0,
                block: frame,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 0), frame);
        let shown = Block::new("item_frame", 1 | 8);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 0,
                block: shown,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 0), shown);
        assert_eq!(shape_of(shown).boxes().len(), 2);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 2,
                block: Block::new("item_frame", 3),
                player: far_player(),
            }
        ));
    }

    #[test]
    fn a_shelf_hangs_on_a_wall_and_keeps_its_stacks() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 1, 1, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let shelf = Block::new("shelf", crate::pack::shelf_meta(1, 0));
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: shelf,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 1), shelf);
        let loaded = Block::new("shelf", crate::pack::shelf_meta(1, 3));
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: loaded,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 1), loaded);
        assert_eq!(shape_of(loaded).boxes().len(), 4);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 1,
                block: Block::new("shelf", 1),
                player: far_player(),
            }
        ));
    }

    #[test]
    fn a_vine_hangs_on_a_wall_and_not_on_air() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let hung = Block::new("vine", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 1,
                z: 1,
                block: hung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(0, 1, 1), hung);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 2,
                z: 0,
                block: Block::new("vine", 0),
                player: far_player(),
            }
        ));
        assert!(world.block_at(0, 2, 0).is_air());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: Block::new("vine", 3),
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        let shape = shape_of(hung);
        assert_eq!(shape.boxes()[0].min[2], 0);
        assert_eq!(shape.boxes()[0].max[2], 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_lily_pad_places_on_water_and_not_on_stone() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 0, Block::new("water", 0)),
                (2, 1, 0, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let pad = Block::new("lily_pad", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 2,
                z: 0,
                block: pad,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(0, 2, 0), pad);
        assert_eq!(world.block_at(0, 1, 0).id, "water");
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 0,
                block: pad,
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 2, 0).is_air());
        let shape = shape_of(pad);
        assert_eq!(shape.boxes()[0].max[1], 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_flower_pot_place_can_be_planted() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let empty = Block::new("flower_pot", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: empty,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), empty);
        let planted = Block::new("flower_pot", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: planted,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), planted);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), planted);
        assert_eq!(shape_of(empty).boxes()[0].max[1], 6);
        assert_eq!(shape_of(planted).boxes().len(), 2);
        assert_eq!(shape_of(planted).boxes()[1].max[1], 14);
    }

    #[test]
    fn an_end_portal_frame_place_can_take_an_eye() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let empty = Block::new("end_portal_frame", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: empty,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), empty);
        let filled = Block::new("end_portal_frame", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: filled,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), filled);
        let shape = shape_of(empty);
        assert_eq!(shape.boxes()[0].max[1], 13);
        assert!(!shape.is_full_cube());
        assert_eq!(shape_of(filled).boxes().len(), 2);
        assert_eq!(shape_of(filled).boxes()[1].max[1], 16);
    }

    #[test]
    fn a_dragon_egg_places_as_an_egg() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let egg = Block::new("dragon_egg", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: egg,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), egg);
        let shape = shape_of(egg);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[2].max[1], 15);
    }

    #[test]
    fn an_end_crystal_places_as_a_glowing_shard() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let crystal = Block::new("end_crystal", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: crystal,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), crystal);
        let shape = shape_of(crystal);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[2].max[1], 16);
    }

    #[test]
    fn a_chest_an_ender_chest_and_a_shulker_box_place_as_solid_cubes() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        for (index, id) in ["chest", "copper_chest", "ender_chest", "shulker_box"]
            .into_iter()
            .enumerate()
        {
            let block = Block::new(id, 0);
            let x = 2 + index as i32;
            assert!(apply(
                &mut world,
                Edit::Place {
                    x,
                    y: 1,
                    z: 2,
                    block,
                    player: far_player(),
                }
            ));
            assert_eq!(world.block_at(x, 1, 2), block);
            assert!(shape_of(block).is_full_cube());
        }
    }

    #[test]
    fn a_barrel_places_as_a_stave_with_two_hoops() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let barrel = Block::new("barrel", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: barrel,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), barrel);
        let shape = shape_of(barrel);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[1], 14);
        assert_eq!(shape.boxes()[1].max[1], 5);
        assert_eq!(shape.boxes()[2].min[1], 10);
    }

    #[test]
    fn a_stonecutter_places_with_a_saw_and_cuts_stone_stairs() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let saw = Block::new("stonecutter", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: saw,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), saw);
        let shape = shape_of(saw);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 6);
        assert_eq!(shape.boxes()[1].max[1], 14);
        let stairs = Block::new("stone_stairs", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 2,
                block: stairs,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(4, 1, 2), stairs);
        let steps = shape_of(stairs);
        assert!(!steps.is_full_cube());
        assert_eq!(steps.boxes().len(), 2);
        assert_eq!(steps.boxes()[1].max[1], 16);
    }

    #[test]
    fn a_campfire_places_as_logs_and_a_flame() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let fire = Block::new("campfire", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: fire,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), fire);
        let shape = shape_of(fire);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[1], 3);
        assert_eq!(shape.boxes()[2].max[1], 12);
    }

    #[test]
    fn a_fletching_table_places_with_a_tool_strip() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let table = Block::new("fletching_table", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: table,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), table);
        let shape = shape_of(table);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[1], 14);
    }

    #[test]
    fn a_smoker_places_with_a_hood_and_a_stack() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let smoker = Block::new("smoker", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: smoker,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), smoker);
        let shape = shape_of(smoker);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[1], 10);
        assert_eq!(shape.boxes()[1].max[1], 13);
        assert_eq!(shape.boxes()[2].max[1], 16);
    }

    #[test]
    fn a_blast_furnace_places_with_a_brow() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let blast = Block::new("blast_furnace", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: blast,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), blast);
        let shape = shape_of(blast);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[1], 4);
        assert_eq!(shape.boxes()[1].max[1], 14);
        assert_eq!(shape.boxes()[2].max[1], 16);
    }

    #[test]
    fn an_anvil_places_as_a_stepped_block() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let anvil = Block::new("anvil", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: anvil,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), anvil);
        let shape = shape_of(anvil);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[2].max[0], 16);
        assert!(shape.boxes()[2].max[0] > shape.boxes()[1].max[0]);
    }

    #[test]
    fn an_enchanting_table_places_with_a_book_on_top() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let table = Block::new("enchanting_table", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: table,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), table);
        let shape = shape_of(table);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[1], 16);
    }

    #[test]
    fn a_brewing_stand_places_as_a_stand() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let stand = Block::new("brewing_stand", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: stand,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), stand);
        let shape = shape_of(stand);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[1].max[0], 9);
        assert_eq!(shape.boxes()[2].max[1], 16);
    }

    #[test]
    fn a_grindstone_places_as_a_wheel_on_legs() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let stone = Block::new("grindstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: stone,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), stone);
        let shape = shape_of(stone);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[0], 4);
        assert_eq!(shape.boxes()[2].max[1], 14);
    }

    #[test]
    fn a_smithing_table_places_with_a_raised_back() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let table = Block::new("smithing_table", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: table,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), table);
        let shape = shape_of(table);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[2], 4);
    }

    #[test]
    fn a_loom_places_as_a_frame_and_a_banner_stands_on_a_pole() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let loom = Block::new("loom", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: loom,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), loom);
        let shape = shape_of(loom);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[0], 3);
        assert_eq!(shape.boxes()[2].max[1], 12);

        let banner = Block::new("banner", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 2,
                block: banner,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(4, 1, 2), banner);
        let cloth = shape_of(banner);
        assert!(!cloth.is_full_cube());
        assert_eq!(cloth.boxes().len(), 2);
        assert_eq!(cloth.boxes()[0].max[1], 16);
        assert_eq!(cloth.boxes()[1].max[0], 15);
    }

    #[test]
    fn a_cartography_table_places_with_a_map_on_top() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let table = Block::new("cartography_table", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: table,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), table);
        let shape = shape_of(table);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[1], 14);
    }

    #[test]
    fn a_lectern_places_and_then_holds_a_book() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let stand = Block::new("lectern", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: stand,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), stand);
        let shape = shape_of(stand);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[1], 15);
        let held = Block::new("lectern", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: held,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), held);
    }

    #[test]
    fn a_chiseled_bookshelf_places_and_then_holds_books() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let empty = Block::new("chiseled_bookshelf", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: empty,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), empty);
        assert_eq!(shape_of(empty).boxes().len(), 1);
        let three = Block::new("chiseled_bookshelf", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: three,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), three);
        assert_eq!(shape_of(three).boxes().len(), 4);
        let full = Block::new("chiseled_bookshelf", 6);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: full,
                player: far_player(),
            }
        ));
        assert_eq!(shape_of(full).boxes().len(), 7);
    }

    #[test]
    fn a_composter_places_and_then_fills() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bin = Block::new("composter", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bin,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bin);
        let shape = shape_of(bin);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 5);
        assert_eq!(shape.boxes()[0].max[1], 2);
        assert_eq!(shape.boxes()[1].max[1], 13);
        let ready = Block::new("composter", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: ready,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), ready);
    }

    #[test]
    fn a_beehive_places_with_a_slit_and_then_fills() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let hive = Block::new("beehive", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: hive,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), hive);
        let shape = shape_of(hive);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[1], 12);
        assert_eq!(shape.boxes()[1].max[1], 15);
        assert_eq!(shape.boxes()[2].max[1], 8);
        let ready = Block::new("beehive", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: ready,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), ready);
    }

    #[test]
    fn a_cauldron_places_and_the_water_line_rises() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let pot = Block::new("cauldron", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: pot,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), pot);
        let shape = shape_of(pot);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 5);
        assert_eq!(shape.boxes()[1].max[1], 12);
        let full = Block::new("cauldron", 3);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: full,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), full);
        let filled = shape_of(full);
        assert_eq!(filled.boxes().len(), 6);
        assert_eq!(filled.boxes()[5].max[1], 10);
        assert_eq!(shape_of(Block::new("cauldron", 1)).boxes()[5].max[1], 5);
        assert_eq!(shape_of(Block::new("cauldron", 2)).boxes()[5].max[1], 8);
    }

    #[test]
    fn a_bell_places_and_a_ring_swings_it() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bell = Block::new("bell", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bell,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bell);
        let shape = shape_of(bell);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 4);
        assert_eq!(shape.boxes()[0].min[1], 13);
        assert_eq!(shape.boxes()[1].min[0], 6);
        let swung = Block::new("bell", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: swung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), swung);
        assert_eq!(shape_of(swung).boxes()[1].min[0], 8);
    }

    #[test]
    fn a_candle_places_thin_and_a_lit_one_grows_a_flame() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let candle = Block::new("candle", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: candle,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), candle);
        let shape = shape_of(candle);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[0] - shape.boxes()[0].min[0], 2);
        let lit = Block::new("candle", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: lit,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), lit);
        assert_eq!(shape_of(lit).boxes().len(), 3);
        assert_eq!(shape_of(lit).boxes()[2].min[1], 8);
        assert!(light_of(candle).is_none());
        assert_eq!(light_of(lit).unwrap().level, 10);
    }

    #[test]
    fn a_chain_stands_thin_and_lies_along_a_wall() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let standing = Block::new("chain", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: standing,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), standing);
        let shape = shape_of(standing);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[0].max[0] - shape.boxes()[0].min[0], 2);
        assert_eq!(shape.boxes()[0].max[1], 16);
        let hung = Block::new("chain", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 1,
                z: 2,
                block: hung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(3, 1, 2), hung);
        assert_eq!(shape_of(hung).boxes()[0].min[0], 0);
        assert_eq!(shape_of(hung).boxes()[0].max[0], 16);
        let north = Block::new("chain", 3);
        assert_eq!(shape_of(north).boxes()[0].max[2], 16);
        let hung = Block::new("hanging_sign", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 2,
                block: hung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(4, 1, 2), hung);
        let board = shape_of(hung).boxes()[1];
        assert_eq!(board.max[2] - board.min[2], 2);
        assert_eq!(shape_of(Block::new("hanging_sign", 1)).boxes()[0].min[0], 0);
        assert_eq!(
            shape_of(Block::new("hanging_sign", 3)).boxes()[0].max[2],
            16
        );
    }

    #[test]
    fn a_lodestone_places_with_a_raised_face() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let stone = Block::new("lodestone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: stone,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), stone);
        let shape = shape_of(stone);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 3);
        assert_eq!(shape.boxes()[2].max[1], 16);
    }

    #[test]
    fn a_respawn_anchor_places_and_a_charge_fills_the_cradle() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let anchor = Block::new("respawn_anchor", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: anchor,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), anchor);
        let shape = shape_of(anchor);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        let charged = Block::new("respawn_anchor", 4);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: charged,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), charged);
        assert_eq!(shape_of(charged).boxes().len(), 3);
        assert_eq!(shape_of(charged).boxes()[2].max[1], 12);
        assert!(light_of(anchor).is_none());
        assert_eq!(light_of(charged).unwrap().level, 15);
    }

    #[test]
    fn a_lightning_rod_stands_thin_and_a_strike_flashes() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let rod = Block::new("lightning_rod", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: rod,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), rod);
        let shape = shape_of(rod);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 2);
        assert_eq!(shape.boxes()[0].max[0] - shape.boxes()[0].min[0], 2);
        assert_eq!(shape.boxes()[1].max[1], 16);
        let hung = Block::new("lightning_rod", 1);
        assert_eq!(shape_of(hung).boxes()[0].min[0], 0);
        let flash = Block::new("lightning_rod", 8);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: flash,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), flash);
        assert_eq!(shape_of(flash).boxes().len(), 3);
        assert!(light_of(rod).is_none());
        assert_eq!(light_of(flash).unwrap().level, 15);
    }

    #[test]
    fn a_target_places_as_a_bullseye_and_a_hit_pushes_the_eye() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let target = Block::new("target", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: target,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), target);
        let shape = shape_of(target);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 4);
        assert_eq!(shape.boxes()[3].max[2], 5);
        let hit = Block::new("target", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: hit,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), hit);
        assert_eq!(shape_of(hit).boxes().len(), 5);
        assert_eq!(shape_of(hit).boxes()[4].max[2], 8);
        assert!(light_of(target).is_none());
        assert_eq!(light_of(hit).unwrap().level, 10);
    }

    #[test]
    fn a_trial_spawner_places_as_a_cage_and_a_wave_lights_the_core() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cage = Block::new("trial_spawner", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cage,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cage);
        let shape = shape_of(cage);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 6);
        assert_eq!(shape.boxes()[0].max[1], 2);
        let lit = Block::new("trial_spawner", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: lit,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), lit);
        assert_eq!(shape_of(lit).boxes().len(), 7);
        assert_eq!(shape_of(lit).boxes()[6].max[1], 10);
        assert!(light_of(cage).is_none());
        assert_eq!(light_of(lit).unwrap().level, 10);
    }

    #[test]
    fn a_copper_bulb_places_blocky_and_a_light_fills_the_core() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bulb = Block::new("copper_bulb", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bulb,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bulb);
        let shape = shape_of(bulb);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 4);
        assert_eq!(shape.boxes()[1].max[0] - shape.boxes()[1].min[0], 12);
        assert_eq!(shape.boxes()[3].max[1], 16);
        let lit = Block::new("copper_bulb", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: lit,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), lit);
        assert_eq!(shape_of(lit).boxes().len(), 5);
        assert!(light_of(bulb).is_none());
        assert_eq!(light_of(lit).unwrap().level, 15);
    }

    #[test]
    fn a_decorated_pot_places_as_a_vessel_and_a_stack_shows_in_the_mouth() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let pot = Block::new("decorated_pot", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: pot,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), pot);
        let shape = shape_of(pot);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 4);
        assert_eq!(shape.boxes()[1].max[0] - shape.boxes()[1].min[0], 12);
        assert_eq!(shape.boxes()[3].max[1], 15);
        let held = Block::new("decorated_pot", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: held,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), held);
        assert_eq!(shape_of(held).boxes().len(), 5);
        assert_eq!(shape_of(held).boxes()[4].max[1], 16);
    }

    #[test]
    fn a_copper_grate_places_as_a_lattice() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let grate = Block::new("copper_grate", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: grate,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), grate);
        let shape = shape_of(grate);
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 6);
        assert_eq!(shape.boxes()[0].max[2] - shape.boxes()[0].min[2], 1);
    }

    #[test]
    fn a_cactus_places_on_sand_and_not_on_stone() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (1, 0, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let sand = Block::new("sand", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: sand,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), sand);
        let cactus = Block::new("cactus", 0);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 1,
                z: 0,
                block: cactus,
                player: far_player(),
            }
        ));
        assert!(world.block_at(0, 1, 0).is_air());
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: cactus,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 2, 1), cactus);
        let shape = shape_of(cactus);
        assert_eq!(shape.boxes()[0].min[0], 1);
        assert_eq!(shape.boxes()[0].max[0], 15);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_sugar_cane_places_on_sand_beside_water_and_stacks() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 0, 1, Block::new("sand", 0)),
                (2, 0, 1, Block::new("water", 0)),
                (4, 0, 1, Block::new("sand", 0)),
                (5, 0, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cane = Block::new("sugar_cane", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: cane,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), cane);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: cane,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 2, 1), cane);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 1,
                block: cane,
                player: far_player(),
            }
        ));
        assert!(world.block_at(4, 1, 1).is_air());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 5,
                y: 1,
                z: 1,
                block: cane,
                player: far_player(),
            }
        ));
        assert!(world.block_at(5, 1, 1).is_air());
        let shape = shape_of(cane);
        assert_eq!(shape.planes().len(), 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_chorus_plant_places_on_end_stone_and_stacks() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (1, 0, 1, Block::new("end_stone", 0)),
                (4, 0, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let stalk = Block::new("chorus_plant", 0);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 1,
                z: 1,
                block: stalk,
                player: far_player(),
            }
        ));
        assert!(world.block_at(4, 1, 1).is_air());
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: stalk,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), stalk);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: stalk,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 2, 1), stalk);
        let shape = shape_of(stalk);
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_chorus_flower_places_on_a_chorus_plant_and_not_on_end_stone() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 0, 1, Block::new("end_stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bloom = Block::new("chorus_flower", 0);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: bloom,
                player: far_player(),
            }
        ));
        assert!(world.block_at(1, 1, 1).is_air());
        let stalk = Block::new("chorus_plant", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: stalk,
                player: far_player(),
            }
        ));
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 2,
                z: 1,
                block: bloom,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 2, 1), bloom);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 3,
                z: 1,
                block: bloom,
                player: far_player(),
            }
        ));
        assert!(world.block_at(1, 3, 1).is_air());
        let shape = shape_of(bloom);
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn an_ice_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let ice = Block::new("ice", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: ice,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), ice);
        assert!(shape_of(ice).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_clay_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let clay = Block::new("clay", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: clay,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), clay);
        assert!(shape_of(clay).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_brushed_sand_keeps_the_cell_and_a_second_sweep_mark() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 1, 1, Block::new("sand", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let swept = Block::new("sand", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: swept,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), swept);
        let pot = Block::new("decorated_pot", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: pot,
                player: far_player(),
            }
        ));
        let swept_pot = Block::new("decorated_pot", 5);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: swept_pot,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 1), swept_pot);
    }

    #[test]
    fn a_trial_key_marks_a_copper_chest_once() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 1, 1, Block::new("copper_chest", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let open = Block::new("copper_chest", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block: open,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), open);
    }

    #[test]
    fn a_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bricks = Block::new("bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bricks,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bricks);
        assert!(shape_of(bricks).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_gravel_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let gravel = Block::new("gravel", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: gravel,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), gravel);
        assert!(shape_of(gravel).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_mossy_cobblestone_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let mossy = Block::new("mossy_cobblestone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: mossy,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), mossy);
        assert!(shape_of(mossy).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_stone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bricks = Block::new("stone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bricks,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bricks);
        assert!(shape_of(bricks).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_sandstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let sandstone = Block::new("sandstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: sandstone,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), sandstone);
        assert!(shape_of(sandstone).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_red_sandstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let red = Block::new("red_sandstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: red,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), red);
        assert!(shape_of(red).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_smooth_stone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let smooth = Block::new("smooth_stone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: smooth,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), smooth);
        assert!(shape_of(smooth).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_quartz_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let quartz = Block::new("quartz", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: quartz,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), quartz);
        assert!(shape_of(quartz).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_glowstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let glow = Block::new("glowstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: glow,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), glow);
        assert!(shape_of(glow).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_sea_lantern_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let lantern = Block::new("sea_lantern", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: lantern,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), lantern);
        assert!(shape_of(lantern).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_prismarine_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let prismarine = Block::new("prismarine", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: prismarine,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), prismarine);
        assert!(shape_of(prismarine).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_dark_prismarine_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let dark = Block::new("dark_prismarine", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: dark,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), dark);
        assert!(shape_of(dark).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_prismarine_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bricks = Block::new("prismarine_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bricks,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bricks);
        assert!(shape_of(bricks).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cracked_stone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cracked = Block::new("cracked_stone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cracked,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cracked);
        assert!(shape_of(cracked).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_mossy_stone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let mossy = Block::new("mossy_stone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: mossy,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), mossy);
        assert!(shape_of(mossy).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_chiseled_stone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let chiseled = Block::new("chiseled_stone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: chiseled,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), chiseled);
        assert!(shape_of(chiseled).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_deepslate_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bricks = Block::new("deepslate_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bricks,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bricks);
        assert!(shape_of(bricks).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cobbled_deepslate_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cobbled = Block::new("cobbled_deepslate", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cobbled,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cobbled);
        assert!(shape_of(cobbled).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_polished_deepslate_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let polished = Block::new("polished_deepslate", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: polished,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), polished);
        assert!(shape_of(polished).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_deepslate_tiles_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let tiles = Block::new("deepslate_tiles", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: tiles,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), tiles);
        assert!(shape_of(tiles).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cracked_deepslate_tiles_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cracked = Block::new("cracked_deepslate_tiles", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cracked,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cracked);
        assert!(shape_of(cracked).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_blackstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let black = Block::new("blackstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: black,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), black);
        assert!(shape_of(black).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_polished_blackstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let polished = Block::new("polished_blackstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: polished,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), polished);
        assert!(shape_of(polished).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_blackstone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bricks = Block::new("blackstone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bricks,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bricks);
        assert!(shape_of(bricks).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cracked_blackstone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cracked = Block::new("cracked_blackstone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cracked,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cracked);
        assert!(shape_of(cracked).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_chiseled_blackstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let carved = Block::new("chiseled_blackstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: carved,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), carved);
        assert!(shape_of(carved).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_gilded_blackstone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let gilded = Block::new("gilded_blackstone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: gilded,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), gilded);
        assert!(shape_of(gilded).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_basalt_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let column = Block::new("basalt", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: column,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), column);
        assert!(shape_of(column).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_polished_basalt_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let smooth = Block::new("polished_basalt", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: smooth,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), smooth);
        assert!(shape_of(smooth).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_smooth_basalt_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let plain = Block::new("smooth_basalt", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: plain,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), plain);
        assert!(shape_of(plain).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_nether_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let nether = Block::new("nether_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: nether,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), nether);
        assert!(shape_of(nether).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cracked_nether_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let cracked = Block::new("cracked_nether_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: cracked,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), cracked);
        assert!(shape_of(cracked).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_red_nether_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let red = Block::new("red_nether_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: red,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), red);
        assert!(shape_of(red).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_chiseled_nether_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let carved = Block::new("chiseled_nether_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: carved,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), carved);
        assert!(shape_of(carved).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_nether_wart_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let wart = Block::new("nether_wart_block", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: wart,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), wart);
        assert!(shape_of(wart).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_soul_sand_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let soul = Block::new("soul_sand", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: soul,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), soul);
        assert!(shape_of(soul).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_magma_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let hot = Block::new("magma", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: hot,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), hot);
        assert!(shape_of(hot).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_netherrack_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let rack = Block::new("netherrack", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: rack,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), rack);
        assert!(shape_of(rack).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn an_end_stone_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let end = Block::new("end_stone", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: end,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), end);
        assert!(shape_of(end).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn an_end_stone_bricks_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let courses = Block::new("end_stone_bricks", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: courses,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), courses);
        assert!(shape_of(courses).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_purpur_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let tile = Block::new("purpur", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: tile,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), tile);
        assert!(shape_of(tile).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_purpur_pillar_block_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let pillar = Block::new("purpur_pillar", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: pillar,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), pillar);
        assert!(shape_of(pillar).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn an_end_rod_stands_on_the_floor_and_hangs_on_a_wall() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let standing = Block::new("end_rod", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: standing,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), standing);
        assert!(!shape_of(standing).is_full_cube());
        let hung = Block::new("end_rod", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 3,
                y: 1,
                z: 2,
                block: hung,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(3, 1, 2), hung);
        assert_eq!(shape_of(hung).boxes()[0].min[0], 0);
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
        assert!(apply(&mut world, Edit::Break { x: 3, y: 1, z: 2 }));
        assert!(world.block_at(3, 1, 2).is_air());
    }

    #[test]
    fn a_cake_loses_a_slice_until_the_last_bite_clears_it() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let whole = Block::new("cake", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: whole,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), whole);
        let bitten = Block::new("cake", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bitten,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bitten);
        let last = Block::new("cake", 5);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: last,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), last);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
        assert_eq!(shape_of(whole).boxes()[0].min[0], 1);
        assert_eq!(shape_of(whole).boxes()[0].max[1], 8);
        assert_eq!(shape_of(bitten).boxes()[0].min[0], 3);
        assert_eq!(shape_of(last).boxes()[0].min[0], 11);
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_pumpkin_can_be_carved_and_then_lit() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let plain = Block::new("pumpkin", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: plain,
                player: far_player(),
            }
        ));
        assert!(shape_of(plain).is_full_cube());
        let carved = Block::new("pumpkin", 1);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: carved,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), carved);
        let lit = Block::new("pumpkin", 2);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: lit,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), lit);
        assert!(shape_of(lit).is_full_cube());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: Block::new("stone", 0),
                player: far_player(),
            }
        ));
    }

    #[test]
    fn a_melon_places_and_breaks_to_air() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let melon = Block::new("melon", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: melon,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), melon);
        assert!(shape_of(melon).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
        assert_eq!(crate::pack::harvest_of("melon"), ("melon_slice", 4));
    }

    #[test]
    fn a_hay_bale_places_as_a_solid_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let bale = Block::new("hay_bale", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: bale,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), bale);
        assert!(shape_of(bale).is_full_cube());
        assert!(apply(&mut world, Edit::Break { x: 2, y: 1, z: 2 }));
        assert!(world.block_at(2, 1, 2).is_air());
    }

    #[test]
    fn a_cobweb_places_as_a_cross() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let web = Block::new("cobweb", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 2,
                block: web,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(2, 1, 2), web);
        let shape = shape_of(web);
        assert_eq!(shape.planes().len(), 2);
        assert!(!shape.is_full_cube());
    }

    #[test]
    fn a_snow_layer_places_on_stone_and_not_on_water() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 0, Block::new("stone", 0)),
                (2, 1, 0, Block::new("water", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let snow = Block::new("snow", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 2,
                z: 0,
                block: snow,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(0, 2, 0), snow);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 2,
                z: 0,
                block: snow,
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 2, 0).is_air());
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 4,
                y: 2,
                z: 0,
                block: snow,
                player: far_player(),
            }
        ));
        assert!(world.block_at(4, 2, 0).is_air());
        assert_eq!(shape_of(snow).boxes()[0].max[1], 1);
    }

    #[test]
    fn a_script_block_places_and_a_script_item_does_not() {
        let pack = Pack::builtin();
        pack.register_block_from("editkit", "cube", "edit-place-test")
            .unwrap();
        pack.register_item_from("editkit", "gem", "edit-place-test")
            .unwrap();
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let block = Block::new("editkit:cube", 0);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 1,
                y: 1,
                z: 1,
                block,
                player: far_player(),
            }
        ));
        assert_eq!(world.block_at(1, 1, 1), block);
        assert!(!apply(
            &mut world,
            Edit::Place {
                x: 2,
                y: 1,
                z: 1,
                block: Block::new("editkit:gem", 0),
                player: far_player(),
            }
        ));
        assert!(world.block_at(2, 1, 1).is_air());
        assert!(apply(&mut world, Edit::Break { x: 1, y: 1, z: 1 }));
        assert!(world.block_at(1, 1, 1).is_air());
    }
}
