//! Dispenser / dropper inventory spit.
//!
//! A powered neighbor (lever rising edge or button) makes the block spit one
//! item from its inventory. Dispensers shoot arrows and fire charges;
//! everything else (and all dropper output) becomes a dropped item in front.

use rebuild_world::CarriedSlot;

/// Inventory slots (same 3×3 chest grid).
pub const SLOTS: u8 = 9;

/// Facing bits shared with pumps: +X -X +Y -Y +Z -Z.
pub fn facing_dir(metadata: u8) -> [i32; 3] {
    rebuild_world::pump_dir(metadata)
}

/// Look → dispenser / dropper facing (all six directions allowed).
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

pub fn is_dispenser(id: &str) -> bool {
    id == "dispenser"
}

pub fn is_dropper(id: &str) -> bool {
    id == "dropper"
}

pub fn is_spitter(id: &str) -> bool {
    is_dispenser(id) || is_dropper(id)
}

/// What happens when this inventory id leaves a dispenser or dropper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpitKind {
    Arrow,
    FireCharge,
    Drop,
}

pub fn spit_kind(block_id: &str, item: &str) -> SpitKind {
    if is_dropper(block_id) {
        return SpitKind::Drop;
    }
    match item {
        "arrow" => SpitKind::Arrow,
        "fire_charge" => SpitKind::FireCharge,
        _ => SpitKind::Drop,
    }
}

/// Remove one item from the lowest occupied slot. Returns the id.
pub fn take_one(slots: &mut Vec<CarriedSlot>) -> Option<String> {
    slots.sort_by_key(|slot| slot.slot);
    let index = slots.iter().position(|slot| slot.count > 0)?;
    let id = slots[index].id.clone();
    if slots[index].count <= 1 {
        slots.remove(index);
    } else {
        slots[index].count -= 1;
    }
    Some(id)
}

/// World point just in front of the spitter face.
pub fn muzzle(at: [i32; 3], dir: [i32; 3]) -> [f32; 3] {
    [
        at[0] as f32 + 0.5 + dir[0] as f32 * 0.55,
        at[1] as f32 + 0.5 + dir[1] as f32 * 0.55,
        at[2] as f32 + 0.5 + dir[2] as f32 * 0.55,
    ]
}

/// Aim point farther along the facing ray.
pub fn toward(at: [i32; 3], dir: [i32; 3]) -> [f32; 3] {
    [
        at[0] as f32 + 0.5 + dir[0] as f32 * 4.0,
        at[1] as f32 + 0.5 + dir[1] as f32 * 4.0,
        at[2] as f32 + 0.5 + dir[2] as f32 * 4.0,
    ]
}

/// Drop cell one step in front of the face.
pub fn drop_cell(at: [i32; 3], dir: [i32; 3]) -> [i32; 3] {
    [at[0] + dir[0], at[1] + dir[1], at[2] + dir[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispenser_shoots_arrows_and_charges_but_drops_stone() {
        assert_eq!(spit_kind("dispenser", "arrow"), SpitKind::Arrow);
        assert_eq!(spit_kind("dispenser", "fire_charge"), SpitKind::FireCharge);
        assert_eq!(spit_kind("dispenser", "stone"), SpitKind::Drop);
        assert_eq!(spit_kind("dropper", "arrow"), SpitKind::Drop);
        assert_eq!(spit_kind("dropper", "fire_charge"), SpitKind::Drop);
    }

    #[test]
    fn take_one_spends_the_lowest_slot() {
        let mut slots = vec![
            CarriedSlot {
                slot: 2,
                id: "dirt".into(),
                count: 2,
            },
            CarriedSlot {
                slot: 0,
                id: "arrow".into(),
                count: 1,
            },
        ];
        assert_eq!(take_one(&mut slots).as_deref(), Some("arrow"));
        assert_eq!(slots.len(), 1);
        assert_eq!(take_one(&mut slots).as_deref(), Some("dirt"));
        assert_eq!(slots[0].count, 1);
    }

    #[test]
    fn facing_follows_look_on_all_axes() {
        assert_eq!(facing_from_look([1.0, 0.0, 0.0]), 0);
        assert_eq!(facing_from_look([0.0, 1.0, 0.0]), 2);
        assert_eq!(facing_from_look([0.0, -1.0, 0.0]), 3);
        assert_eq!(facing_from_look([0.0, 0.0, -1.0]), 5);
    }
}
