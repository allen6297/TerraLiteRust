//! Piston / sticky piston push and pull.
//!
//! A powered neighbor (lever on, or button) extends the piston and pushes one
//! block ahead when the far cell is empty. Power falling (lever off, or a short
//! button pulse) retracts; sticky pistons pull that block back.

use rebuild_world::Block;

/// Facing bits `0..=5` (+X -X +Y -Y +Z -Z), bit 8 = extended.
pub fn facing_dir(metadata: u8) -> [i32; 3] {
    rebuild_world::pump_dir(metadata)
}

pub fn is_extended(metadata: u8) -> bool {
    metadata & 8 != 0
}

pub fn with_extended(metadata: u8, extended: bool) -> u8 {
    (metadata & 7) | if extended { 8 } else { 0 }
}

pub fn facing_from_look(forward: [f32; 3]) -> u8 {
    let ax = forward[0].abs();
    let ay = forward[1].abs();
    let az = forward[2].abs();
    if ax >= ay && ax >= az {
        if forward[0] >= 0.0 {
            0
        } else {
            1
        }
    } else if ay >= ax && ay >= az {
        if forward[1] >= 0.0 {
            2
        } else {
            3
        }
    } else if forward[2] >= 0.0 {
        4
    } else {
        5
    }
}

pub fn is_piston(id: &str) -> bool {
    id == "piston" || id == "sticky_piston"
}

pub fn is_sticky(id: &str) -> bool {
    id == "sticky_piston"
}

/// Blocks pistons refuse to push or pull.
pub fn immovable(id: &str) -> bool {
    matches!(
        id,
        "obsidian"
            | "crying_obsidian"
            | "bedrock"
            | "piston"
            | "sticky_piston"
            | "chest"
            | "copper_chest"
            | "ender_chest"
            | "barrel"
            | "hopper"
            | "dispenser"
            | "dropper"
            | "furnace"
            | "blast_furnace"
            | "smoker"
            | "beacon"
            | "enchanting_table"
    )
}

/// Face and beyond cells along `dir` from piston `at`.
pub fn push_cells(at: [i32; 3], dir: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    let face = [at[0] + dir[0], at[1] + dir[1], at[2] + dir[2]];
    let beyond = [
        at[0] + dir[0] * 2,
        at[1] + dir[1] * 2,
        at[2] + dir[2] * 2,
    ];
    (face, beyond)
}

/// True when this face/beyond pair can be pushed one step.
pub fn can_push(face: Block, beyond: Block) -> bool {
    if face.id == "air" {
        return true;
    }
    !immovable(face.id) && beyond.id == "air"
}

/// Sticky pull: face empty, beyond holds a movable block.
pub fn can_pull(face: Block, beyond: Block) -> bool {
    face.id == "air" && beyond.id != "air" && !immovable(beyond.id)
}

/// Seconds a button-powered piston stays extended before retracting.
pub const BUTTON_PULSE: f32 = 0.45;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piston_pushes_one_block_into_air() {
        assert!(can_push(Block::new("dirt", 0), Block::AIR));
        assert!(can_push(Block::AIR, Block::AIR));
        assert!(!can_push(Block::new("obsidian", 0), Block::AIR));
        assert!(!can_push(Block::new("dirt", 0), Block::new("stone", 0)));
    }

    #[test]
    fn a_sticky_piston_pulls_on_retract() {
        assert!(can_pull(Block::AIR, Block::new("cobblestone", 0)));
        assert!(!can_pull(Block::new("dirt", 0), Block::new("cobblestone", 0)));
        assert!(!can_pull(Block::AIR, Block::new("obsidian", 0)));
    }

    #[test]
    fn facing_and_extended_bits() {
        assert_eq!(facing_from_look([0.0, -1.0, 0.0]), 3);
        assert_eq!(facing_dir(0), [1, 0, 0]);
        assert_eq!(facing_dir(5), [0, 0, -1]);
        assert_eq!(with_extended(3, true), 11);
        assert!(is_extended(11));
        assert!(!is_extended(3));
        assert!(is_piston("sticky_piston"));
        assert!(is_sticky("sticky_piston"));
    }
}
