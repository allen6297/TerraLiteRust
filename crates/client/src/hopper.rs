//! Hopper pull / push between inventory containers.
//!
//! A hopper pulls one item from the container above, then pushes one item into
//! the container it faces (down or a side). Transfers run on a client tick.

use rebuild_world::CarriedSlot;

/// Seconds between hopper transfer attempts.
pub const HOPPER_PERIOD: f32 = 0.4;

/// How many slots a hopper inventory holds (same 3×3 chest grid).
pub const HOPPER_SLOTS: u8 = 9;

/// Facing bits shared with pumps: +X -X +Y -Y +Z -Z.
pub fn hopper_dir(metadata: u8) -> [i32; 3] {
    rebuild_world::pump_dir(metadata)
}

/// Look → hopper facing. Hoppers never face up (Minecraft rule).
pub fn hopper_facing_from_look(forward: [f32; 3]) -> u8 {
    let ax = forward[0].abs();
    let ay = forward[1].abs();
    let az = forward[2].abs();
    let dir = if ax >= ay && ax >= az {
        if forward[0] >= 0.0 {
            0
        } else {
            1
        }
    } else if ay >= ax && ay >= az {
        if forward[1] >= 0.0 {
            3
        } else {
            3
        }
    } else if forward[2] >= 0.0 {
        4
    } else {
        5
    };
    dir
}

/// True when hoppers may treat this block as an inventory endpoint.
pub fn hopper_inventory(id: &str) -> bool {
    matches!(
        id,
        "chest"
            | "copper_chest"
            | "barrel"
            | "shulker_box"
            | "hopper"
            | "decorated_pot"
    )
}

/// Move one item from `from` into `to`. Returns the transferred id.
pub fn move_one(
    from: &mut Vec<CarriedSlot>,
    to: &mut Vec<CarriedSlot>,
    to_slots: u8,
    stack_cap: impl Fn(&str) -> u16,
) -> Option<String> {
    from.sort_by_key(|slot| slot.slot);
    let index = from.iter().position(|slot| slot.count > 0)?;
    let id = from[index].id.clone();
    let cap = stack_cap(&id).max(1);

    if let Some(dest) = to.iter_mut().find(|slot| slot.id == id && slot.count < cap) {
        dest.count = dest.count.saturating_add(1);
    } else {
        let used: Vec<u8> = to.iter().map(|slot| slot.slot).collect();
        let free = (0..to_slots).find(|slot| !used.contains(slot))?;
        to.push(CarriedSlot {
            slot: free,
            id: id.clone(),
            count: 1,
        });
    }

    if from[index].count <= 1 {
        from.remove(index);
    } else {
        from[index].count -= 1;
    }
    Some(id)
}

/// Wire form for [`encode_chest`].
pub fn wire_slots(slots: &[CarriedSlot]) -> Vec<(String, i32)> {
    let mut ordered = slots.to_vec();
    ordered.sort_by_key(|slot| slot.slot);
    ordered
        .into_iter()
        .filter(|slot| slot.count > 0)
        .map(|slot| (slot.id, i32::from(slot.count)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(_: &str) -> u16 {
        64
    }

    #[test]
    fn a_hopper_pulls_one_from_above_then_pushes_below() {
        let mut above = vec![CarriedSlot {
            slot: 0,
            id: "dirt".into(),
            count: 3,
        }];
        let mut hopper = Vec::new();
        let mut below = Vec::new();

        assert_eq!(
            move_one(&mut above, &mut hopper, HOPPER_SLOTS, cap).as_deref(),
            Some("dirt")
        );
        assert_eq!(above[0].count, 2);
        assert_eq!(hopper[0].id, "dirt");
        assert_eq!(hopper[0].count, 1);

        assert_eq!(
            move_one(&mut hopper, &mut below, HOPPER_SLOTS, cap).as_deref(),
            Some("dirt")
        );
        assert!(hopper.is_empty());
        assert_eq!(below[0].id, "dirt");
        assert_eq!(below[0].count, 1);
    }

    #[test]
    fn a_hopper_facing_look_never_points_up() {
        assert_eq!(hopper_facing_from_look([0.0, 1.0, 0.0]), 3);
        assert_eq!(hopper_facing_from_look([0.0, -1.0, 0.0]), 3);
        assert_eq!(hopper_facing_from_look([1.0, 0.1, 0.0]), 0);
        assert_eq!(hopper_facing_from_look([-1.0, 0.1, 0.0]), 1);
    }

    #[test]
    fn stacks_merge_until_the_cap() {
        let mut from = vec![CarriedSlot {
            slot: 0,
            id: "cobblestone".into(),
            count: 2,
        }];
        let mut to = vec![CarriedSlot {
            slot: 1,
            id: "cobblestone".into(),
            count: 63,
        }];
        assert_eq!(
            move_one(&mut from, &mut to, HOPPER_SLOTS, cap).as_deref(),
            Some("cobblestone")
        );
        assert_eq!(to[0].count, 64);
        assert_eq!(from[0].count, 1);
        assert_eq!(
            move_one(&mut from, &mut to, HOPPER_SLOTS, |_| 64).as_deref(),
            Some("cobblestone")
        );
        assert_eq!(to.len(), 2);
        assert!(from.is_empty());
    }
}
