//! Alt place-shape picker.
//!
//! Holding Alt while a family block is selected opens a small menu of place
//! variants (block, slab, stairs, plate). Each option places the matching
//! per-block model and collision for that family. The selection sticks after
//! Alt is released.

use rebuild_world::{Block, Pack};
#[cfg(test)]
use rebuild_world::shape_of;

/// A place variant for the held block's family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceShape {
    Block,
    Slab,
    Stairs,
    Plate,
}

impl PlaceShape {
    pub fn label(self) -> &'static str {
        match self {
            PlaceShape::Block => "block",
            PlaceShape::Slab => "slab",
            PlaceShape::Stairs => "stairs",
            PlaceShape::Plate => "plate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Stone,
    Oak,
}

/// Sticky place-shape choice and whether Alt currently shows the menu.
#[derive(Debug)]
pub struct ShapePick {
    pub shape: PlaceShape,
    pub open: bool,
}

impl ShapePick {
    pub fn new() -> Self {
        Self {
            shape: PlaceShape::Block,
            open: false,
        }
    }

    /// Options for the held hotbar id. Empty when the id is not a placeable
    /// family block (or only has the default block itself).
    pub fn options(held: &str) -> Vec<PlaceShape> {
        match family_of(held) {
            Some(Family::Stone) => vec![
                PlaceShape::Block,
                PlaceShape::Slab,
                PlaceShape::Stairs,
                PlaceShape::Plate,
            ],
            Some(Family::Oak) => vec![PlaceShape::Block, PlaceShape::Stairs],
            None => Vec::new(),
        }
    }

    /// Keep the sticky shape on an option that exists for `held`.
    pub fn clamp_to(&mut self, held: Option<&str>) {
        let Some(held) = held else {
            self.shape = PlaceShape::Block;
            return;
        };
        let options = Self::options(held);
        if options.is_empty() {
            self.shape = PlaceShape::Block;
            return;
        }
        if !options.contains(&self.shape) {
            self.shape = options[0];
        }
    }

    pub fn cycle(&mut self, held: &str, forward: bool) {
        let options = Self::options(held);
        if options.len() < 2 {
            return;
        }
        let at = options
            .iter()
            .position(|shape| *shape == self.shape)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % options.len()
        } else {
            (at + options.len() - 1) % options.len()
        };
        self.shape = options[next];
    }

    /// Digit `1..=n` picks that row. True when a row changed.
    pub fn pick_digit(&mut self, held: &str, digit: u8) -> bool {
        let options = Self::options(held);
        let index = digit.saturating_sub(1) as usize;
        let Some(shape) = options.get(index).copied() else {
            return false;
        };
        if self.shape == shape {
            return false;
        }
        self.shape = shape;
        true
    }

    /// Block id + metadata to place for the held selection.
    pub fn resolve(&self, held: &str) -> Option<Block> {
        let id = place_id(held, self.shape)?;
        if !Pack::builtin().contains_block(id) {
            return None;
        }
        Some(Block::new(id, 0))
    }

    /// Readable line for the held plate while a non-default shape is armed.
    pub fn held_label(&self, held: &str) -> Option<String> {
        if Self::options(held).len() < 2 {
            return None;
        }
        if self.shape == PlaceShape::Block && !self.open {
            return None;
        }
        let id = place_id(held, self.shape).unwrap_or(held);
        let name = crate::recipes::display_name(id);
        Some(format!("{} · {}", title_case(&name), self.shape.label()))
    }
}

fn is_shape_variant(id: &str) -> bool {
    matches!(id, "slab" | "stone_stairs" | "plate" | "oak_stairs")
}

fn family_block(family: Family) -> &'static str {
    match family {
        Family::Stone => "stone",
        Family::Oak => "oak_log",
    }
}

fn family_of(id: &str) -> Option<Family> {
    match id {
        "stone" | "stone_stairs" | "slab" | "plate" | "cobblestone" | "stone_bricks"
        | "smooth_stone" | "mossy_cobblestone" | "mossy_stone_bricks"
        | "chiseled_stone_bricks" | "cracked_stone_bricks" => Some(Family::Stone),
        "oak_log" | "oak_stairs" => Some(Family::Oak),
        _ => None,
    }
}

fn place_id<'a>(held: &'a str, shape: PlaceShape) -> Option<&'a str> {
    let family = family_of(held)?;
    match (family, shape) {
        (_, PlaceShape::Block) if !is_shape_variant(held) => Some(held),
        (_, PlaceShape::Block) => Some(family_block(family)),
        (Family::Stone, PlaceShape::Slab) => Some("slab"),
        (Family::Stone, PlaceShape::Stairs) => Some("stone_stairs"),
        (Family::Stone, PlaceShape::Plate) => Some("plate"),
        (Family::Oak, PlaceShape::Stairs) => Some("oak_stairs"),
        (Family::Oak, PlaceShape::Slab | PlaceShape::Plate) => None,
    }
}

fn title_case(name: &str) -> String {
    let mut out = String::new();
    for (i, word) in name.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            for ch in first.to_uppercase() {
                out.push(ch);
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

/// True when the held id meshes as a full cube (used by tests / authoring notes).
#[cfg(test)]
fn is_full_cube_block(id: &str) -> bool {
    shape_of(Block::new(id, 0)).is_full_cube()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_family_maps_each_shape_to_its_block_model() {
        let mut pick = ShapePick::new();
        assert_eq!(
            ShapePick::options("stone"),
            vec![
                PlaceShape::Block,
                PlaceShape::Slab,
                PlaceShape::Stairs,
                PlaceShape::Plate
            ]
        );
        assert_eq!(pick.resolve("stone").unwrap().id, "stone");
        assert_eq!(pick.resolve("cobblestone").unwrap().id, "cobblestone");
        pick.shape = PlaceShape::Slab;
        let slab = pick.resolve("stone").unwrap();
        assert_eq!(slab.id, "slab");
        assert!(!shape_of(slab).is_full_cube());
        assert_eq!(pick.resolve("cobblestone").unwrap().id, "slab");
        pick.shape = PlaceShape::Stairs;
        let stairs = pick.resolve("cobblestone").unwrap();
        assert_eq!(stairs.id, "stone_stairs");
        assert_eq!(shape_of(stairs).boxes().len(), 2);
        pick.shape = PlaceShape::Plate;
        let plate = pick.resolve("stone_bricks").unwrap();
        assert_eq!(plate.id, "plate");
        assert!(!is_full_cube_block(plate.id));
    }

    #[test]
    fn oak_family_only_offers_block_and_stairs() {
        assert_eq!(
            ShapePick::options("oak_log"),
            vec![PlaceShape::Block, PlaceShape::Stairs]
        );
        let mut pick = ShapePick::new();
        pick.shape = PlaceShape::Stairs;
        assert_eq!(pick.resolve("oak_log").unwrap().id, "oak_stairs");
        pick.shape = PlaceShape::Slab;
        pick.clamp_to(Some("oak_log"));
        assert_eq!(pick.shape, PlaceShape::Block);
        assert!(ShapePick::options("dirt").is_empty());
        assert!(pick.resolve("dirt").is_none());
    }

    #[test]
    fn digits_and_cycle_step_through_options() {
        let mut pick = ShapePick::new();
        assert!(pick.pick_digit("stone", 3));
        assert_eq!(pick.shape, PlaceShape::Stairs);
        pick.cycle("stone", true);
        assert_eq!(pick.shape, PlaceShape::Plate);
        pick.cycle("stone", true);
        assert_eq!(pick.shape, PlaceShape::Block);
        pick.cycle("stone", false);
        assert_eq!(pick.shape, PlaceShape::Plate);
    }

    #[test]
    fn holding_a_variant_still_resolves_the_family_base() {
        let mut pick = ShapePick::new();
        assert_eq!(pick.resolve("slab").unwrap().id, "stone");
        assert_eq!(pick.resolve("stone_stairs").unwrap().id, "stone");
        pick.shape = PlaceShape::Stairs;
        assert_eq!(pick.resolve("plate").unwrap().id, "stone_stairs");
        assert_eq!(pick.resolve("oak_stairs").unwrap().id, "oak_stairs");
    }
}
