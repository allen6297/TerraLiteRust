//! Ten-slot hotbar.
//!
//! Keys 1 through 9 and 0 select a slot. Each slot holds a block id and a count.
//! Right click places one registered block and the count drops. A script item
//! can sit in a slot, and it does not place. At 0 the slot is empty.
//! Picking up a drop adds one to a stack of that id, or the first empty slot.

use rebuild_world::{Block, Pack};

use crate::palette;

pub const SLOTS: usize = 10;

/// A composter at this level is ready to give bone meal.
pub const COMPOST_READY: u8 = 3;

/// The line on an empty or ready composter.
pub fn compost_prompt(level: u8) -> &'static str {
    if level >= COMPOST_READY {
        "enter takes bone meal"
    } else {
        "enter adds a plant"
    }
}

/// A cauldron at this level is full.
pub const CAULDRON_FULL: u8 = 3;

/// The line on an empty, partial, or full cauldron.
pub fn cauldron_prompt(level: u8) -> &'static str {
    if level == 0 {
        "enter pours a water bucket"
    } else if level >= CAULDRON_FULL {
        "enter scoops a bucket"
    } else {
        "enter pours or scoops"
    }
}

/// The chat word after a pour. `next` is the new level.
pub fn cauldron_word(next: u8) -> &'static str {
    match next {
        1 => "level 1",
        2 => "level 2",
        _ => "level 3",
    }
}

/// A beehive at this level is ready to give honey.
pub const HONEY_READY: u8 = 3;

/// The line on an empty or ready beehive.
pub fn hive_prompt(level: u8) -> &'static str {
    if level >= HONEY_READY {
        "enter takes honey"
    } else {
        "enter adds a flower"
    }
}

/// The chat word after one flower goes in. `next` is the new level.
pub fn hive_word(next: u8) -> &'static str {
    match next {
        1 => "level 1",
        2 => "level 2",
        _ => "level 3",
    }
}

/// The chat word after one plant goes in. `next` is the new level.
pub fn compost_word(next: u8) -> &'static str {
    match next {
        1 => "level 1",
        2 => "level 2",
        _ => "level 3",
    }
}

/// Blast-furnace one-step lines. Food stays on the smoker; clay stays on the furnace.
fn blast_result(id: &str) -> Option<&'static str> {
    match id {
        "cobblestone" => Some("stone"),
        "stone" => Some("smooth_stone"),
        "stone_bricks" => Some("cracked_stone_bricks"),
        "netherrack" => Some("nether_bricks"),
        "ancient_debris" => Some("netherite_scrap"),
        _ => None,
    }
}

/// Diamond gear a smithing table turns into netherite for one ingot.
pub fn netherite_upgrade(id: &str) -> Option<&'static str> {
    match id {
        "diamond_pickaxe" => Some("netherite_pickaxe"),
        "diamond_sword" => Some("netherite_sword"),
        "diamond_axe" => Some("netherite_axe"),
        "diamond_shovel" => Some("netherite_shovel"),
        "diamond_hoe" => Some("netherite_hoe"),
        "diamond_helmet" => Some("netherite_helmet"),
        "diamond_chestplate" => Some("netherite_chestplate"),
        "diamond_boots" => Some("netherite_boots"),
        _ => None,
    }
}

/// Cosmetic trim stubs shown on the smithing UI (no gameplay effect yet).
pub const SMITH_TRIMS: &[&str] = &["None", "Coast", "Dune", "Eye", "Rib", "Sentry"];

/// Furnace one-step lines that the blast furnace refuses (clay → bricks).
fn furnace_result(id: &str) -> Option<&'static str> {
    match id {
        "clay" => Some("bricks"),
        _ => None,
    }
}

/// Plants and food a composter accepts. Bone meal itself does not go back in.
pub fn is_compost(id: &str) -> bool {
    matches!(
        id,
        "flower"
            | "cactus"
            | "sugar_cane"
            | "wheat_seeds"
            | "pumpkin"
            | "melon"
            | "hay_bale"
            | "vine"
            | "lily_pad"
            | "chorus_plant"
            | "chorus_flower"
            | "nether_wart_block"
    ) || Pack::builtin().food_of(id).is_some()
}

/// What Enter does at a composter of `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompostStep {
    /// Spend this slot and raise the level by one.
    Fill(usize),
    /// The bin is full and the bar can take bone meal.
    Take,
    /// Nothing on the bar will compost.
    NeedPlants,
    /// The bin is full and the bar cannot take bone meal.
    NoRoom,
}

/// What Enter does at a beehive of `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiveStep {
    /// Spend this flower slot and raise the level by one.
    Fill(usize),
    /// The hive is full and the bar can take honey.
    Take,
    /// No flower is on the bar.
    NeedFlower,
    /// The hive is full and the bar cannot take honey.
    NoRoom,
}

/// What Enter does at a cauldron of `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CauldronStep {
    /// Spend this water bucket and raise the level by one.
    Pour(usize),
    /// Turn this empty bucket into a water bucket and lower the level by one.
    Scoop(usize),
    /// No bucket can pour or scoop.
    NeedBucket,
    /// The pot is full and there is no empty bucket to scoop with.
    Full,
    /// The bar cannot hold the bucket the pour or scoop would leave.
    NoRoom,
}

/// A respawn anchor at this charge is full.
pub const ANCHOR_FULL: u8 = 4;

/// The line on an empty, partial, or full respawn anchor.
pub fn anchor_prompt(level: u8) -> &'static str {
    if level == 0 {
        "enter adds glowstone"
    } else if level >= ANCHOR_FULL {
        "enter sets your spawn"
    } else {
        "enter charges or sets spawn"
    }
}

/// The chat word after a charge. `next` is the new level.
pub fn anchor_word(next: u8) -> &'static str {
    match next {
        1 => "charge 1",
        2 => "charge 2",
        3 => "charge 3",
        _ => "charge 4",
    }
}

/// What Enter does at a respawn anchor of `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorStep {
    /// Spend this glowstone and raise the charge by one.
    Charge(usize),
    /// The anchor holds a charge, so it can set spawn.
    SetSpawn,
    /// No glowstone is on the bar and the anchor is empty.
    NeedGlow,
}

const SLOT_PX: f32 = 40.0;
const GAP_PX: f32 = 8.0;
const MARGIN_PX: f32 = 20.0;
const BORDER_PX: f32 = 2.0;
const SELECT_BORDER_PX: f32 = 6.0;
const PAD_PX: f32 = 6.0;
const FILL_INSET_PX: f32 = 4.0;
/// Icon inset inside the slot well (raster is already AABB-fitted).
const ICON_INSET_PX: f32 = 3.0;

/// GUI-space Y of the top lip of the hotbar chrome. HUD plates clear above this.
pub fn bar_chrome_top() -> f32 {
    MARGIN_PX + SLOT_PX + PAD_PX
}

const BAR: [f32; 3] = [0.08, 0.08, 0.10];
const EMPTY: [f32; 3] = [0.12, 0.12, 0.14];
const BORDER: [f32; 3] = [0.35, 0.35, 0.38];
const SELECTED: [f32; 3] = [0.95, 0.82, 0.25];
/// A light lip along the top of the bar.
const LIP: [f32; 3] = [0.30, 0.31, 0.36];
const COUNT: [f32; 3] = [1.0, 1.0, 1.0];
const COUNT_SHADOW: [f32; 3] = [0.05, 0.05, 0.06];

const STACK: u16 = 64;
const DIGIT_CELL: f32 = 2.0;
const DIGIT_GAP: f32 = 2.0;

/// Per-item hotbar limit. Script `item.stack` can lower it; default is 64.
fn stack_cap(id: &str) -> u16 {
    let cap = Pack::builtin().max_stack(id);
    if cap == 0 { STACK } else { cap.min(STACK) }
}

/// Three-wide rows, top to bottom. The high bit is the left column.
const GLYPHS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

#[derive(Clone, Copy)]
struct Item {
    id: &'static str,
    count: u16,
    /// Remaining uses of a tool. `0` means this slot is a stack, not a tool.
    uses: u16,
    /// Efficiency. A keen pick mines twice as fast.
    keen: bool,
    /// Unbreaking — half of wear attempts are shrugged off.
    sturdy: bool,
    /// Fortune — ore breaks may yield an extra drop.
    lucky: bool,
}

/// A new stack. A tool is one item at full durability. `count` is ignored for tools.
fn fresh(id: &'static str, count: u16) -> Item {
    let uses = Pack::builtin().tool_uses(id);
    if uses > 0 {
        Item {
            id,
            count: 1,
            uses,
            keen: false,
            sturdy: false,
            lucky: false,
        }
    } else {
        Item {
            id,
            count: count.min(stack_cap(id)),
            uses: 0,
            keen: false,
            sturdy: false,
            lucky: false,
        }
    }
}

/// A saved slot. For a tool, `saved` is the uses it had left.
fn restored(id: &'static str, saved: u16) -> Item {
    let uses = Pack::builtin().tool_uses(id);
    if uses > 0 {
        Item {
            id,
            count: 1,
            uses: saved.min(uses).max(1),
            keen: false,
            sturdy: false,
            lucky: false,
        }
    } else {
        Item {
            id,
            count: saved.min(stack_cap(id)),
            uses: 0,
            keen: false,
            sturdy: false,
            lucky: false,
        }
    }
}

/// One row on the enchanting table choice UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnchantOffer {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub xp: i32,
    pub lapis: u16,
}

/// Bookshelf power → how many offers (1..=3).
pub fn enchant_offer_count(books: u8) -> usize {
    if books >= 8 {
        3
    } else if books >= 2 {
        2
    } else {
        1
    }
}

/// Build 1–3 offers for the held tool from nearby bookshelf power.
pub fn enchant_offers_for(tool: &str, books: u8, keen: bool, sturdy: bool, lucky: bool) -> Vec<EnchantOffer> {
    let max = Pack::builtin().tool_uses(tool);
    if max == 0 {
        return Vec::new();
    }
    let pick = tool.ends_with("pickaxe");
    let dig =
        pick || tool.ends_with("axe") || tool.ends_with("shovel") || tool.ends_with("hoe");
    let mut pool: Vec<EnchantOffer> = Vec::new();
    if dig && !keen {
        let (xp, lapis, title, detail) = if books >= 8 {
            (3, 3, "Efficiency III", "Mines much faster")
        } else if books >= 2 {
            (2, 2, "Efficiency II", "Mines twice as fast")
        } else {
            (1, 1, "Efficiency I", "Mines twice as fast")
        };
        pool.push(EnchantOffer {
            id: "efficiency",
            title,
            detail,
            xp,
            lapis,
        });
    }
    if !sturdy {
        let (xp, lapis, title) = if books >= 8 {
            (2, 2, "Unbreaking III")
        } else if books >= 2 {
            (2, 1, "Unbreaking II")
        } else {
            (1, 1, "Unbreaking I")
        };
        pool.push(EnchantOffer {
            id: "unbreaking",
            title,
            detail: "Halves wear chance",
            xp,
            lapis,
        });
    }
    if pick && !lucky {
        let (xp, lapis, title) = if books >= 8 {
            (3, 3, "Fortune III")
        } else if books >= 2 {
            (2, 2, "Fortune II")
        } else {
            (1, 1, "Fortune I")
        };
        pool.push(EnchantOffer {
            id: "fortune",
            title,
            detail: "Extra ore chance",
            xp,
            lapis,
        });
    }
    let n = enchant_offer_count(books).min(pool.len());
    pool.truncate(n);
    pool
}

fn is_tool(id: &str) -> bool {
    Pack::builtin().tool_uses(id) > 0
}

/// Save / wire index for the offhand stack (`0..10` are the hotbar).
pub const OFFHAND_SAVE_SLOT: u8 = 10;

pub struct Hotbar {
    slots: [Option<Item>; SLOTS],
    selected: usize,
    /// Separate equipment hand — shield, totem, or any stack.
    offhand: Option<Item>,
    /// Per-slot crossbow cocked state (arrow already spent).
    crossbow_loaded: [bool; SLOTS],
    /// How far the held map reaches. `0` is one block per cell.
    map_zoom: u8,
    /// `1` after the first campfire Enter. The next Enter finishes the cook.
    camp_heat: u8,
    /// Anvil custom names keyed by hotbar slot (cleared when the slot empties).
    labels: [Option<String>; SLOTS],
}

impl Hotbar {
    /// Slot 1 holds a stack of cobblestone. Slots 2 through 10 are empty. Slot 1 is selected.
    pub fn new() -> Self {
        Self {
            slots: [
                Some(fresh(Block::new("cobblestone", 0).id, STACK)),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            selected: 0,
            offhand: None,
            crossbow_loaded: [false; SLOTS],
            map_zoom: 0,
            camp_heat: 0,
            labels: [const { None }; SLOTS],
        }
    }

    /// How far the held map reaches. `0` is one block per cell.
    pub fn map_zoom(&self) -> u8 {
        self.map_zoom
    }

    /// `1` through `9` and `10` (key 0) select a slot. Other digits do nothing.
    pub fn select_digit(&mut self, digit: u8) -> bool {
        if (1..=SLOTS as u8).contains(&digit) {
            self.selected = (digit - 1) as usize;
            true
        } else {
            false
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Replace the bar from a save. `selected` is `0..10`. Slot `10` is offhand.
    pub fn install_saved(&mut self, selected: u8, slots: &[rebuild_world::CarriedSlot]) {
        self.slots = [None; SLOTS];
        self.offhand = None;
        self.crossbow_loaded = [false; SLOTS];
        self.labels = [const { None }; SLOTS];
        self.selected = (selected as usize).min(SLOTS - 1);
        for carried in slots {
            if carried.id.is_empty() || carried.count == 0 {
                continue;
            }
            let id = Block::new(&carried.id, 0).id;
            if carried.slot == OFFHAND_SAVE_SLOT {
                self.offhand = Some(restored(id, carried.count));
                continue;
            }
            let index = carried.slot as usize;
            if index >= SLOTS {
                continue;
            }
            self.slots[index] = Some(restored(id, carried.count));
        }
    }

    /// Id in the offhand slot, if any.
    pub fn offhand(&self) -> Option<&str> {
        self.offhand.map(|item| item.id)
    }

    /// Count or remaining uses shown for the offhand slot.
    pub fn offhand_count(&self) -> u16 {
        match self.offhand {
            Some(item) if item.uses > 0 => item.uses,
            Some(item) => item.count,
            None => 0,
        }
    }

    /// Put `id` in the offhand (full stack / full uses). Creative equip uses this.
    pub fn set_offhand(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        self.offhand = Some(fresh(id, STACK));
        true
    }

    /// Clear the offhand and return what was there.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn take_offhand(&mut self) -> Option<&'static str> {
        self.offhand.take().map(|item| item.id)
    }

    /// Swap the selected hotbar stack with the offhand.
    pub fn swap_offhand(&mut self) {
        let selected = self.selected;
        let main = self.slots.get_mut(selected).map(Option::take).flatten();
        let off = self.offhand.take();
        self.offhand = main;
        if let Some(slot) = self.slots.get_mut(selected) {
            *slot = off;
        }
        // Cocked state stays with the bar cell; clear so a swapped-in item is not ready.
        if let Some(flag) = self.crossbow_loaded.get_mut(selected) {
            *flag = false;
        }
    }

    /// True when the offhand holds a usable shield.
    fn offhand_shield(&self) -> bool {
        self.offhand
            .is_some_and(|item| Pack::builtin().is_shield(item.id) && item.uses > 0)
    }

    /// Spend one use from the offhand shield. True when that bite is blocked.
    fn block_offhand_bite(&mut self) -> bool {
        let Some(item) = self.offhand.as_mut() else {
            return false;
        };
        if !Pack::builtin().is_shield(item.id) || item.uses == 0 {
            return false;
        }
        if item.uses <= 1 {
            self.offhand = None;
        } else {
            item.uses -= 1;
        }
        true
    }

    /// Consume one from the offhand stack (totem, etc.).
    pub fn consume_offhand(&mut self) {
        let Some(item) = self.offhand else {
            return;
        };
        if item.count <= 1 {
            self.offhand = None;
        } else {
            self.offhand = Some(Item {
                count: item.count - 1,
                ..item
            });
        }
    }

    /// Equip click: swap selected ↔ offhand (any item). True when either side held something.
    pub fn click_offhand(&mut self) -> bool {
        if self.item(self.selected).is_none() && self.offhand.is_none() {
            return false;
        }
        self.swap_offhand();
        true
    }

    /// True when the selected slot is a cocked crossbow.
    pub fn crossbow_loaded(&self) -> bool {
        self.crossbow_loaded
            .get(self.selected)
            .copied()
            .unwrap_or(false)
            && self
                .slot(self.selected)
                .is_some_and(|id| Pack::builtin().is_crossbow(id))
    }

    /// Mark the selected crossbow as loaded (arrow already spent).
    pub fn set_crossbow_loaded(&mut self, loaded: bool) {
        if let Some(flag) = self.crossbow_loaded.get_mut(self.selected) {
            *flag = loaded;
        }
    }

    pub fn slot(&self, index: usize) -> Option<&str> {
        self.item(index).map(|item| item.id)
    }

    pub fn count(&self, index: usize) -> u16 {
        match self.item(index) {
            Some(item) if item.uses > 0 => item.uses,
            Some(item) => item.count,
            None => 0,
        }
    }

    /// True when the slot holds an Efficiency (keen) pick.
    pub fn is_keen(&self, index: usize) -> bool {
        self.item(index).is_some_and(|item| item.keen)
    }

    /// True when the slot holds Unbreaking.
    pub fn is_sturdy(&self, index: usize) -> bool {
        self.item(index).is_some_and(|item| item.sturdy)
    }

    /// True when the slot holds Fortune.
    pub fn is_lucky(&self, index: usize) -> bool {
        self.item(index).is_some_and(|item| item.lucky)
    }

    /// Current enchant offers for the selected tool given nearby bookshelf power.
    pub fn enchant_offers(&self, books: u8) -> Vec<EnchantOffer> {
        let Some(item) = self.item(self.selected) else {
            return Vec::new();
        };
        enchant_offers_for(item.id, books, item.keen, item.sturdy, item.lucky)
    }

    fn item(&self, index: usize) -> Option<Item> {
        self.slots.get(index).copied().flatten()
    }

    /// The block a right click places.
    ///
    /// Empty slots and script items place nothing. A pack block or a
    /// `block.register` id does.
    pub fn selected_block(&self) -> Option<Block> {
        let id = self.slot(self.selected)?;
        let pack = Pack::builtin();
        if pack.contains_block(id) {
            return Some(Block::new(id, 0));
        }
        pack.placed_block(id)
            .filter(|block| pack.contains_block(block))
            .map(|block| Block::new(&block, 0))
    }

    /// Fill empty slots with script items that place a block.
    pub fn stock_items(&mut self, ids: &[String]) -> usize {
        let mut added = 0;
        for id in ids {
            if Pack::builtin().placed_block(id).is_none() {
                continue;
            }
            let id = Block::new(id, 0).id;
            if self.slots.iter().flatten().any(|slot| slot.id == id) {
                continue;
            }
            let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
                break;
            };
            *slot = Some(fresh(id, STACK));
            added += 1;
        }
        added
    }

    /// Put a full stack of `id` in the selected slot. Creative picking uses this.
    pub fn give_stack(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        let Some(slot) = self.slots.get_mut(self.selected) else {
            return false;
        };
        *slot = Some(fresh(id, STACK));
        if let Some(flag) = self.crossbow_loaded.get_mut(self.selected) {
            *flag = false;
        }
        true
    }

    /// Fill empty slots with a full stack of each registered block id.
    ///
    /// An id that is not a block is skipped. An id already in the bar is skipped.
    pub fn stock_blocks(&mut self, ids: &[String]) -> usize {
        let mut added = 0;
        for id in ids {
            if !Pack::builtin().contains_block(id) {
                continue;
            }
            let id = Block::new(id, 0).id;
            if self.slots.iter().flatten().any(|slot| slot.id == id) {
                continue;
            }
            let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
                break;
            };
            *slot = Some(fresh(id, STACK));
            added += 1;
        }
        added
    }

    /// Spend one use when `block` is something this tool mines. At zero uses the tool is gone.
    ///
    /// Grass, dirt, and wood do not wear a pickaxe. Returns true when a use was spent.
    pub fn wear(&mut self, index: usize, block: &str) -> bool {
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        let Some(item) = slot.as_mut() else {
            return false;
        };
        if item.uses == 0 || Pack::builtin().mine_factor(item.id, block) <= 1.0 {
            return false;
        }
        // Unbreaking shrugs off every other wear tick.
        if item.sturdy && item.uses % 2 == 0 {
            return true;
        }
        let broke = item.uses <= 1;
        if broke {
            *slot = None;
        } else {
            item.uses -= 1;
        }
        let _ = slot;
        if broke {
            self.labels[index] = None;
        }
        true
    }

    /// Spend one use of a tool, shield, or brush. At zero uses the item is gone.
    ///
    /// Returns false when that slot has no uses to spend.
    pub fn wear_use(&mut self, index: usize) -> bool {
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        let Some(item) = slot.as_mut() else {
            return false;
        };
        if item.uses == 0 {
            return false;
        }
        if item.sturdy && item.uses % 2 == 0 {
            return true;
        }
        let broke = item.uses <= 1;
        if broke {
            *slot = None;
        } else {
            item.uses -= 1;
        }
        let _ = slot;
        if broke {
            self.labels[index] = None;
        }
        true
    }

    /// Material id that repairs this tool tier (first match wins).
    pub fn repair_material_for(id: &str) -> Option<&'static [&'static str]> {
        if id.starts_with("netherite_") {
            Some(&["netherite_ingot"])
        } else if id.starts_with("diamond_") {
            Some(&["diamond"])
        } else if id.starts_with("golden_") || id.starts_with("gold_") {
            Some(&["gold_ingot"])
        } else if id.starts_with("iron_") {
            Some(&["iron_ingot", "iron_ore"])
        } else if id.starts_with("stone_") {
            Some(&["cobblestone"])
        } else if id.starts_with("wooden_") || id.starts_with("wood_") {
            Some(&["oak_planks", "planks"])
        } else {
            None
        }
    }

    /// The worn tool Enter / Repair would refill, when a matching material is in the bar.
    pub fn repair_target(&self) -> Option<&'static str> {
        let item = self.item(self.selected)?;
        let max = Pack::builtin().tool_uses(item.id);
        if max == 0 || item.uses >= max {
            return None;
        }
        let mats = Self::repair_material_for(item.id)?;
        let has = self.slots.iter().any(|slot| {
            slot.is_some_and(|item| mats.contains(&item.id) && item.count > 0)
        });
        if !has {
            return None;
        }
        Some(item.id)
    }

    /// Refill the selected tool. One matching material pays for it.
    ///
    /// `"repaired"` spends the material. `"full"` leaves a fresh tool alone.
    /// `"need a tool"` is anything that is not a worn or fresh tool.
    /// `"need material"` is a worn tool with no matching material in the bar.
    pub fn repair_pick(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return "need a tool";
        };
        let max = Pack::builtin().tool_uses(item.id);
        let Some(mats) = Self::repair_material_for(item.id) else {
            return "need a tool";
        };
        if max == 0 {
            return "need a tool";
        }
        if item.uses >= max {
            return "full";
        }
        let Some(mat) = self.slots.iter().position(|slot| {
            slot.is_some_and(|item| mats.contains(&item.id) && item.count > 0)
        }) else {
            return "need material";
        };
        self.consume_at(mat);
        if let Some(item) = self
            .slots
            .get_mut(self.selected)
            .and_then(|slot| slot.as_mut())
        {
            item.uses = max;
        }
        "repaired"
    }

    /// Slot of another worn tool of the same id (combine donor), if any.
    pub fn combine_donor(&self) -> Option<usize> {
        let item = self.item(self.selected)?;
        let max = Pack::builtin().tool_uses(item.id);
        if max == 0 || item.uses >= max {
            return None;
        }
        self.slots.iter().enumerate().find_map(|(index, slot)| {
            if index == self.selected {
                return None;
            }
            let other = slot.as_ref()?;
            if other.id == item.id && other.uses > 0 && other.uses < max {
                Some(index)
            } else {
                None
            }
        })
    }

    /// Merge a second worn copy into the selected tool. Consumes the donor.
    ///
    /// `"combined"` on success. Caps at max uses; keeps Efficiency if either had it.
    pub fn combine_tool(&mut self) -> &'static str {
        let Some(donor) = self.combine_donor() else {
            return "need a pair";
        };
        let Some(selected) = self.item(self.selected) else {
            return "need a tool";
        };
        let max = Pack::builtin().tool_uses(selected.id);
        if max == 0 {
            return "need a tool";
        }
        let (add, keen, sturdy, lucky) = {
            let other = self.slots[donor].as_ref().expect("donor");
            (other.uses, other.keen, other.sturdy, other.lucky)
        };
        if let Some(item) = self
            .slots
            .get_mut(self.selected)
            .and_then(|slot| slot.as_mut())
        {
            item.uses = (item.uses.saturating_add(add)).min(max);
            item.keen = item.keen || keen;
            item.sturdy = item.sturdy || sturdy;
            item.lucky = item.lucky || lucky;
        }
        self.slots[donor] = None;
        self.labels[donor] = None;
        "combined"
    }

    /// Custom anvil name on a slot, when set.
    pub fn label_of(&self, index: usize) -> Option<&str> {
        self.labels.get(index)?.as_deref()
    }

    /// Display name for a slot — custom label or the pack display id.
    pub fn display_of(&self, index: usize) -> String {
        if let Some(label) = self.label_of(index) {
            if !label.trim().is_empty() {
                return label.to_string();
            }
        }
        self.slot(index)
            .map(|id| crate::recipes::display_name(id))
            .unwrap_or_default()
    }

    /// Set or clear the anvil name on the selected slot.
    pub fn rename_selected(&mut self, name: &str) -> bool {
        if self.item(self.selected).is_none() {
            return false;
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.labels[self.selected] = None;
        } else {
            self.labels[self.selected] = Some(trimmed.to_string());
        }
        true
    }

    /// The plain tool Enter / first offer would enchant, when lapis is in the bar.
    pub fn enchant_target(&self) -> Option<&'static str> {
        let item = self.item(self.selected)?;
        let max = Pack::builtin().tool_uses(item.id);
        if max == 0 || item.keen {
            return None;
        }
        let dig = item.id.ends_with("pickaxe")
            || item.id.ends_with("axe")
            || item.id.ends_with("shovel")
            || item.id.ends_with("hoe");
        if !dig {
            return None;
        }
        if !self.holds("lapis_lazuli", 1) {
            return None;
        }
        Some(item.id)
    }

    /// Put Efficiency on the selected dig tool. One lapis pays for it (Enter path).
    ///
    /// `"efficiency"` spends lapis. `"already enchanted"` leaves a keen tool alone.
    /// `"need a tool"` is anything that is not a dig tool. `"need lapis"` is a
    /// plain tool with no lapis in the bar.
    pub fn enchant_pick(&mut self) -> &'static str {
        self.apply_enchant("efficiency", 1)
    }

    /// Apply an enchant offer. Spends `lapis` lapis; caller spends XP.
    ///
    /// Returns the offer id on success, or a need/already word.
    pub fn apply_enchant(&mut self, id: &str, lapis: u16) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return "need a tool";
        };
        let max = Pack::builtin().tool_uses(item.id);
        if max == 0 {
            return "need a tool";
        }
        match id {
            "efficiency" => {
                let dig = item.id.ends_with("pickaxe")
                    || item.id.ends_with("axe")
                    || item.id.ends_with("shovel")
                    || item.id.ends_with("hoe");
                if !dig {
                    return "need a tool";
                }
                if item.keen {
                    return "already enchanted";
                }
            }
            "unbreaking" => {
                if item.sturdy {
                    return "already enchanted";
                }
            }
            "fortune" => {
                if !item.id.ends_with("pickaxe") {
                    return "need a pick";
                }
                if item.lucky {
                    return "already enchanted";
                }
            }
            _ => return "need a tool",
        }
        if !self.holds("lapis_lazuli", lapis.max(1)) {
            return "need lapis";
        }
        for _ in 0..lapis.max(1) {
            if !self.consume_one("lapis_lazuli") {
                return "need lapis";
            }
        }
        if let Some(item) = self
            .slots
            .get_mut(self.selected)
            .and_then(|slot| slot.as_mut())
        {
            match id {
                "efficiency" => item.keen = true,
                "unbreaking" => item.sturdy = true,
                "fortune" => item.lucky = true,
                _ => {}
            }
        }
        match id {
            "efficiency" => "efficiency",
            "unbreaking" => "unbreaking",
            "fortune" => "fortune",
            _ => "enchanted",
        }
    }

    /// The ingredient Enter would brew, when the mix fits on the bar.
    ///
    /// Prefer splash conversion (`healing_potion` + `gravel`). Then awkward +
    /// blaze powder → strength, awkward + magma cream / ghast tear → fire
    /// resistance, awkward + wart/flower → healing, water+wart fueled by powder
    /// → healing, or water + powder → awkward. Missing pieces return `None`.
    pub fn brew_target(&self) -> Option<&'static str> {
        if self.can_brew_splash() {
            return Some("gravel");
        }
        if self.can_brew_strength() {
            return Some("blaze_powder");
        }
        if self.can_brew_fire_resistance() {
            return self.fire_res_reagent();
        }
        if self.can_finish_awkward() {
            return self.wart_or_flower();
        }
        let has_water = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "water_bucket" && item.count > 0));
        if !has_water {
            return None;
        }
        let has_powder = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "blaze_powder" && item.count > 0));
        if !has_powder {
            return None;
        }
        if let Some(id) = self.wart_or_flower() {
            let frees = self.slots.iter().any(|slot| {
                slot.as_ref().is_some_and(|item| {
                    (item.id == "nether_wart_block" || item.id == "flower") && item.count == 1
                })
            }) || self.slots.iter().any(|slot| {
                slot.as_ref()
                    .is_some_and(|item| item.id == "blaze_powder" && item.count == 1)
            });
            if !self.room_for("healing_potion") && !frees {
                return None;
            }
            return Some(id);
        }
        let frees = self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "blaze_powder" && item.count == 1)
        });
        if !self.room_for("awkward_potion") && !frees {
            return None;
        }
        Some("blaze_powder")
    }

    fn wart_or_flower(&self) -> Option<&'static str> {
        self.slots.iter().find_map(|slot| {
            let item = slot.as_ref()?;
            if (item.id == "nether_wart_block" || item.id == "flower") && item.count > 0 {
                Some(item.id)
            } else {
                None
            }
        })
    }

    fn can_brew_splash(&self) -> bool {
        let has_heal = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "healing_potion" && item.count > 0));
        let has_gravel = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "gravel" && item.count > 0));
        if !has_heal || !has_gravel {
            return false;
        }
        let frees = self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "healing_potion" && item.count == 1)
        });
        self.room_for("splash_healing_potion") || frees
    }

    fn can_brew_strength(&self) -> bool {
        let has_awkward = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0));
        let has_powder = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "blaze_powder" && item.count > 0));
        if !has_awkward || !has_powder {
            return false;
        }
        let frees = self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "awkward_potion" && item.count == 1)
        }) || self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "blaze_powder" && item.count == 1)
        });
        self.room_for("strength_potion") || frees
    }

    fn fire_res_reagent(&self) -> Option<&'static str> {
        self.slots.iter().find_map(|slot| {
            let item = slot.as_ref()?;
            if (item.id == "magma_cream" || item.id == "ghast_tear") && item.count > 0 {
                Some(item.id)
            } else {
                None
            }
        })
    }

    fn can_brew_fire_resistance(&self) -> bool {
        let has_awkward = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0));
        if !has_awkward || self.fire_res_reagent().is_none() {
            return false;
        }
        let frees = self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "awkward_potion" && item.count == 1)
        }) || self.slots.iter().any(|slot| {
            slot.as_ref().is_some_and(|item| {
                (item.id == "magma_cream" || item.id == "ghast_tear") && item.count == 1
            })
        });
        self.room_for("fire_resistance_potion") || frees
    }

    fn can_finish_awkward(&self) -> bool {
        let has_awkward = self
            .slots
            .iter()
            .any(|slot| slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0));
        if !has_awkward || self.wart_or_flower().is_none() {
            return false;
        }
        let frees = self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|item| item.id == "awkward_potion" && item.count == 1)
        }) || self.slots.iter().any(|slot| {
            slot.as_ref().is_some_and(|item| {
                (item.id == "nether_wart_block" || item.id == "flower") && item.count == 1
            })
        });
        self.room_for("healing_potion") || frees
    }

    /// Brew healing, strength, fire resistance, awkward, or splash from the stand mix.
    ///
    /// Paths: healing + gravel → splash; awkward + powder → strength; awkward +
    /// magma cream / ghast tear → fire resistance; awkward + wart/flower →
    /// healing; water + wart/flower + blaze powder (fuel) → healing; water +
    /// powder → awkward. `"brewed"` spends the mix. `"need water"` / `"need
    /// nether wart"` / `"need blaze powder"` name the missing piece. `"no room"`
    /// means the bar cannot hold the result.
    pub fn brew_potion(&mut self) -> &'static str {
        if self.can_brew_splash() {
            let Some(heal) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "healing_potion" && item.count > 0)
            }) else {
                return "need nether wart";
            };
            let Some(gravel) = self
                .slots
                .iter()
                .position(|slot| slot.is_some_and(|item| item.id == "gravel" && item.count > 0))
            else {
                return "need nether wart";
            };
            let frees = self.item(heal).is_some_and(|item| item.count == 1);
            if !self.room_for("splash_healing_potion") && !frees {
                return "no room";
            }
            self.consume_at(heal);
            self.consume_at(gravel);
            if !self.collect("splash_healing_potion") {
                return "no room";
            }
            return "brewed";
        }
        if self.can_brew_strength() {
            let Some(awkward) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0)
            }) else {
                return "need water";
            };
            let Some(powder) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "blaze_powder" && item.count > 0)
            }) else {
                return "need blaze powder";
            };
            let frees = self.item(awkward).is_some_and(|item| item.count == 1)
                || self.item(powder).is_some_and(|item| item.count == 1);
            if !self.room_for("strength_potion") && !frees {
                return "no room";
            }
            self.consume_at(awkward);
            self.consume_at(powder);
            if !self.collect("strength_potion") {
                return "no room";
            }
            return "brewed";
        }
        if self.can_brew_fire_resistance() {
            let Some(awkward) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0)
            }) else {
                return "need water";
            };
            let Some(reagent) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| {
                    (item.id == "magma_cream" || item.id == "ghast_tear") && item.count > 0
                })
            }) else {
                return "need blaze powder";
            };
            let frees = self.item(awkward).is_some_and(|item| item.count == 1)
                || self.item(reagent).is_some_and(|item| item.count == 1);
            if !self.room_for("fire_resistance_potion") && !frees {
                return "no room";
            }
            self.consume_at(awkward);
            self.consume_at(reagent);
            if !self.collect("fire_resistance_potion") {
                return "no room";
            }
            return "brewed";
        }
        if self.can_finish_awkward() {
            let Some(awkward) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "awkward_potion" && item.count > 0)
            }) else {
                return "need water";
            };
            let Some(wart) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| {
                    (item.id == "nether_wart_block" || item.id == "flower") && item.count > 0
                })
            }) else {
                return "need nether wart";
            };
            let frees = self.item(awkward).is_some_and(|item| item.count == 1)
                || self.item(wart).is_some_and(|item| item.count == 1);
            if !self.room_for("healing_potion") && !frees {
                return "no room";
            }
            self.consume_at(awkward);
            self.consume_at(wart);
            if !self.collect("healing_potion") {
                return "no room";
            }
            return "brewed";
        }
        let Some(water) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == "water_bucket" && item.count > 0))
        else {
            return "need water";
        };
        let Some(powder) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == "blaze_powder" && item.count > 0))
        else {
            return "need blaze powder";
        };
        let wart = self.slots.iter().position(|slot| {
            slot.is_some_and(|item| {
                (item.id == "nether_wart_block" || item.id == "flower") && item.count > 0
            })
        });
        if let Some(wart) = wart {
            let frees = self.item(wart).is_some_and(|item| item.count == 1)
                || self.item(powder).is_some_and(|item| item.count == 1);
            if !self.room_for("healing_potion") && !frees {
                return "no room";
            }
            let single = self.item(water).is_some_and(|item| item.count == 1);
            self.consume_at(water);
            if single {
                self.slots[water] = Some(fresh(Block::new("bucket", 0).id, 1));
            }
            self.consume_at(wart);
            self.consume_at(powder);
            if !self.collect("healing_potion") {
                return "no room";
            }
            return "brewed";
        }
        let frees = self.item(powder).is_some_and(|item| item.count == 1);
        if !self.room_for("awkward_potion") && !frees {
            return "no room";
        }
        let single = self.item(water).is_some_and(|item| item.count == 1);
        self.consume_at(water);
        if single {
            self.slots[water] = Some(fresh(Block::new("bucket", 0).id, 1));
        }
        self.consume_at(powder);
        if !self.collect("awkward_potion") {
            return "no room";
        }
        "brewed"
    }

    /// Take enchants off the selected pick and give it four uses back.
    ///
    /// `"ground"` clears an enchanted pick. `"sharpened"` is a plain worn pick.
    /// `"plain"` is a fresh unenchanted pick. `"need a pick"` is anything else.
    pub fn grind_pick(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return "need a pick";
        };
        let max = Pack::builtin().tool_uses(item.id);
        if max == 0 || !item.id.ends_with("pickaxe") {
            return "need a pick";
        }
        let enchanted = item.keen || item.sturdy || item.lucky;
        if !enchanted && item.uses >= max {
            return "plain";
        }
        let uses = item.uses.saturating_add(4).min(max);
        if let Some(item) = self
            .slots
            .get_mut(self.selected)
            .and_then(|slot| slot.as_mut())
        {
            item.keen = false;
            item.sturdy = false;
            item.lucky = false;
            item.uses = uses;
        }
        if enchanted { "ground" } else { "sharpened" }
    }

    /// The held item Enter would upgrade, when its payment is in the bar.
    ///
    /// A stone or iron pick needs iron ore. Diamond gear needs a netherite
    /// ingot. A mace needs a heavy core. Netherite gear, a heavy mace, or a
    /// missing payment returns `None`.
    pub fn smith_target(&self) -> Option<&'static str> {
        let item = self.item(self.selected)?;
        let paid = match item.id {
            "stone_pickaxe" | "iron_pickaxe" => self
                .slots
                .iter()
                .any(|slot| slot.is_some_and(|item| item.id == "iron_ore" && item.count > 0)),
            id if netherite_upgrade(id).is_some() => self.slots.iter().any(|slot| {
                slot.is_some_and(|item| item.id == "netherite_ingot" && item.count > 0)
            }),
            "mace" => self
                .slots
                .iter()
                .any(|slot| slot.is_some_and(|item| item.id == "heavy_core" && item.count > 0)),
            _ => false,
        };
        if paid { Some(item.id) } else { None }
    }

    /// Upgrade the selected gear. Iron ore pays stone→iron→diamond picks; a
    /// netherite ingot pays any diamond piece into netherite.
    ///
    /// `"iron"` / `"diamond"` / `"netherite"` name the upgrade. `"best"` leaves
    /// netherite gear alone. `"need iron ore"` / `"need ingot"` name the
    /// missing payment. `"need a pick"` is anything else.
    pub fn smith_pick(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return "need a pick";
        };
        if item.id == "heavy_mace" {
            return "heavy";
        }
        if item.id == "mace" {
            return self.smith_mace();
        }
        if item.id.starts_with("netherite_") {
            return "best";
        }
        if let Some(next) = netherite_upgrade(item.id) {
            let Some(cost) = self.slots.iter().position(|slot| {
                slot.is_some_and(|item| item.id == "netherite_ingot" && item.count > 0)
            }) else {
                return "need ingot";
            };
            let keep = (item.keen, item.sturdy, item.lucky, item.uses);
            self.consume_at(cost);
            let next = Block::new(next, 0).id;
            let max = Pack::builtin().tool_uses(next);
            if let Some(slot) = self.slots.get_mut(self.selected) {
                let mut upgraded = fresh(next, 1);
                upgraded.keen = keep.0;
                upgraded.sturdy = keep.1;
                upgraded.lucky = keep.2;
                if max > 0 {
                    // Keep remaining uses ratio when upgrading a worn tool.
                    let old_max = Pack::builtin().tool_uses(item.id).max(1);
                    let ratio = keep.3 as f32 / old_max as f32;
                    upgraded.uses = ((max as f32) * ratio).round().max(1.0) as u16;
                }
                *slot = Some(upgraded);
            }
            return "netherite";
        }
        let (next, word, pay, need) = match item.id {
            "stone_pickaxe" => ("iron_pickaxe", "iron", "iron_ore", "need iron ore"),
            "iron_pickaxe" => ("diamond_pickaxe", "diamond", "iron_ore", "need iron ore"),
            _ => return "need a pick",
        };
        let Some(cost) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == pay && item.count > 0))
        else {
            return need;
        };
        self.consume_at(cost);
        let next = Block::new(next, 0).id;
        if let Some(slot) = self.slots.get_mut(self.selected) {
            *slot = Some(fresh(next, 1));
        }
        word
    }

    /// Set a heavy core into the selected mace. The core is spent.
    ///
    /// `"heavy"` is the upgraded mace. `"need a core"` means the bar has none.
    fn smith_mace(&mut self) -> &'static str {
        let Some(core) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == "heavy_core" && item.count > 0))
        else {
            return "need a core";
        };
        self.consume_at(core);
        let next = Block::new("heavy_mace", 0).id;
        if let Some(slot) = self.slots.get_mut(self.selected) {
            *slot = Some(fresh(next, 1));
        }
        "heavy"
    }

    /// The wool Enter would spend, when the banner fits in the bar.
    pub fn weave_target(&self) -> Option<&'static str> {
        let (id, frees) = self.slots.iter().find_map(|slot| {
            let item = slot.as_ref()?;
            if item.id == "wool" && item.count > 0 {
                Some((item.id, item.count == 1))
            } else {
                None
            }
        })?;
        if !self.room_for("banner") && !frees {
            return None;
        }
        Some(id)
    }

    /// Weave one banner from wool.
    ///
    /// `"woven"` spends one wool. `"need wool"` means the bar has none.
    /// `"no room"` means the bar cannot hold the banner.
    pub fn weave_banner(&mut self) -> &'static str {
        let Some(wool) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == "wool" && item.count > 0))
        else {
            return "need wool";
        };
        let frees = self.item(wool).is_some_and(|item| item.count == 1);
        if !self.room_for("banner") && !frees {
            return "no room";
        }
        self.consume_at(wool);
        if !self.collect("banner") {
            return "no room";
        }
        "woven"
    }

    /// Widen the held map. Each step doubles the land one cell covers.
    ///
    /// `"wider"` steps out once. `"widest"` is the far chart, left as it is.
    /// `"need a map"` is anything that is not the selected map.
    pub fn chart_map(&mut self) -> &'static str {
        let Some(id) = self.slot(self.selected) else {
            return "need a map";
        };
        if !Pack::builtin().is_map(id) {
            return "need a map";
        }
        if self.map_zoom >= crate::map::ZOOM_MAX {
            return "widest";
        }
        self.map_zoom += 1;
        "wider"
    }

    /// Step the held chart in or out. `wider` covers more land per cell.
    pub fn zoom_map(&mut self, wider: bool) {
        if wider {
            self.map_zoom = self.map_zoom.saturating_add(1).min(crate::map::ZOOM_MAX);
        } else {
            self.map_zoom = self.map_zoom.saturating_sub(1);
        }
    }

    /// How fast the held tool breaks `block`. Efficiency doubles a real pick factor.
    pub fn dig_factor(&self, block: &str) -> f32 {
        let held = self.slot(self.selected).unwrap_or("");
        let factor = Pack::builtin().mine_factor(held, block);
        if factor > 1.0 && self.item(self.selected).is_some_and(|item| item.keen) {
            factor * 2.0
        } else {
            factor
        }
    }

    /// True when the selected slot or offhand holds a usable shield.
    pub fn holding_shield(&self) -> bool {
        self.item(self.selected)
            .is_some_and(|item| Pack::builtin().is_shield(item.id) && item.uses > 0)
            || self.offhand_shield()
    }

    /// Spend one use of a raised shield (selected first, else offhand). True when blocked.
    ///
    /// Call only while the shield is raised. At zero uses the slot clears.
    /// Mining does not spend these uses.
    pub fn block_bite(&mut self) -> bool {
        let index = self.selected;
        if let Some(slot) = self.slots.get_mut(index) {
            if let Some(item) = slot.as_mut() {
                if Pack::builtin().is_shield(item.id) && item.uses > 0 {
                    if item.uses <= 1 {
                        *slot = None;
                    } else {
                        item.uses -= 1;
                    }
                    return true;
                }
            }
        }
        self.block_offhand_bite()
    }

    /// Turn the last `from` in `index` into one `to`. A bigger stack is left alone.
    pub fn exchange_last(&mut self, index: usize, from: &str, to: &str) -> bool {
        let Some(item) = self.item(index) else {
            return false;
        };
        if item.id != from || item.count != 1 {
            return false;
        }
        self.slots[index] = Some(fresh(Block::new(to, 0).id, 1));
        true
    }

    /// Take one `from` off a stack of two or more.
    pub fn shrink(&mut self, index: usize, from: &str) -> bool {
        let Some(item) = self.item(index) else {
            return false;
        };
        if item.id != from || item.count <= 1 {
            return false;
        }
        self.consume_at(index);
        true
    }

    /// Use one from the selected slot. At 0 the slot is empty.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn consume_selected(&mut self) {
        self.consume_at(self.selected);
    }

    /// Use one from `index`. A place reply consumes the slot that was selected when it was sent.
    pub fn consume_at(&mut self, index: usize) {
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(item) = *slot else {
            return;
        };
        if item.count <= 1 {
            *slot = None;
        } else {
            *slot = Some(Item {
                count: item.count - 1,
                ..item
            });
        }
    }

    /// Add one of `id` to a stack that has room, or the first empty slot.
    ///
    /// Returns false when the bar cannot take it.
    pub fn collect(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        let cap = stack_cap(id);
        if !is_tool(id) {
            if let Some(slot) = self
                .slots
                .iter_mut()
                .flatten()
                .find(|slot| slot.id == id && slot.count < cap)
            {
                slot.count += 1;
                return true;
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(fresh(id, 1));
            return true;
        }
        false
    }

    /// True when one more of `id` fits in a stack or an empty slot.
    pub fn room_for(&self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        if is_tool(id) {
            return self.slots.iter().any(|slot| slot.is_none());
        }
        let cap = stack_cap(id);
        self.slots
            .iter()
            .flatten()
            .any(|slot| slot.id == id && slot.count < cap)
            || self.slots.iter().any(|slot| slot.is_none())
    }

    /// True when the bar holds at least `need` of `id`.
    pub fn holds(&self, id: &str, need: u16) -> bool {
        if need == 0 {
            return true;
        }
        let id = Block::new(id, 0).id;
        let have: u16 = self
            .slots
            .iter()
            .flatten()
            .filter(|slot| slot.id == id)
            .map(|slot| if slot.uses > 0 { 1 } else { slot.count })
            .fold(0, u16::saturating_add);
        have >= need
    }

    /// The first slot that holds `id`, when its count is at least one.
    /// Make one arrow from the data-pack recipe.
    ///
    /// Wool stands in for a feather and gravel for flint. `"fletched"` spends
    /// one of each. `"need a stick"`, `"need wool"`, and `"need gravel"` name
    /// the first missing piece. `"no room"` means the bar cannot hold the arrow.
    pub fn fletch_arrow(&mut self) -> &'static str {
        let Some(ingredients) = rebuild_world::Pack::builtin().pack_recipe("arrow") else {
            return "need a stick";
        };
        let ingredients = ingredients.to_vec();
        let mut slots = Vec::with_capacity(ingredients.len());
        for ingredient in &ingredients {
            let Some(slot) = self.index_of(ingredient) else {
                return match ingredient.as_str() {
                    "stick" => "need a stick",
                    "wool" => "need wool",
                    "gravel" => "need gravel",
                    _ => "need a stick",
                };
            };
            slots.push(slot);
        }
        let frees = slots
            .iter()
            .any(|slot| self.item(*slot).is_some_and(|item| item.count == 1));
        if !self.room_for("arrow") && !frees {
            return "no room";
        }
        for slot in slots {
            self.consume_at(slot);
        }
        if !self.collect("arrow") {
            return "no room";
        }
        "fletched"
    }

    /// Make one decorated pot from bricks and clay on the bar.
    ///
    /// `"potted"` spends one of each. `"need bricks"` / `"need clay"` name the
    /// first missing piece. `"no room"` means the bar cannot hold the pot.
    pub fn shape_pot(&mut self) -> &'static str {
        let Some(ingredients) = rebuild_world::Pack::builtin().pack_recipe("decorated_pot") else {
            return "need bricks";
        };
        let ingredients = ingredients.to_vec();
        let mut slots = Vec::with_capacity(ingredients.len());
        for ingredient in &ingredients {
            let Some(slot) = self.index_of(ingredient) else {
                return match ingredient.as_str() {
                    "bricks" => "need bricks",
                    "clay" => "need clay",
                    _ => "need bricks",
                };
            };
            slots.push(slot);
        }
        let frees = slots
            .iter()
            .any(|slot| self.item(*slot).is_some_and(|item| item.count == 1));
        if !self.room_for("decorated_pot") && !frees {
            return "no room";
        }
        for slot in slots {
            self.consume_at(slot);
        }
        if !self.collect("decorated_pot") {
            return "no room";
        }
        "potted"
    }

    /// Make one shulker box from a chest and a shulker shell on the bar.
    ///
    /// `"boxed"` spends one of each. `"need chest"` / `"need shell"` name the
    /// first missing piece. `"no room"` means the bar cannot hold the box.
    pub fn shape_box(&mut self) -> &'static str {
        let Some(ingredients) = rebuild_world::Pack::builtin().pack_recipe("shulker_box") else {
            return "need shell";
        };
        let ingredients = ingredients.to_vec();
        let mut slots = Vec::with_capacity(ingredients.len());
        for ingredient in &ingredients {
            let Some(slot) = self.index_of(ingredient) else {
                return match ingredient.as_str() {
                    "chest" => "need chest",
                    "shulker_shell" => "need shell",
                    _ => "need shell",
                };
            };
            slots.push(slot);
        }
        let frees = slots
            .iter()
            .any(|slot| self.item(*slot).is_some_and(|item| item.count == 1));
        if !self.room_for("shulker_box") && !frees {
            return "no room";
        }
        for slot in slots {
            self.consume_at(slot);
        }
        if !self.collect("shulker_box") {
            return "no room";
        }
        "boxed"
    }

    /// Cook the selected raw food. The first Enter only heats it.
    ///
    /// `"cooking"` spends nothing. `"cooked"` spends one raw and gives one
    /// cooked meal. `"need raw"` is anything else. `"no room"` means the bar
    /// cannot hold the meal. A furnace does this in one second and spends fuel.
    pub fn cook_raw(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            self.camp_heat = 0;
            return "need raw";
        };
        if item.id != "raw" {
            self.camp_heat = 0;
            return "need raw";
        }
        if self.camp_heat == 0 {
            self.camp_heat = 1;
            return "cooking";
        }
        self.finish_cook_raw()
    }

    /// One-step heat cook: hold raw, use a campfire or smoker, get cooked food.
    pub fn cook_raw_now(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            self.camp_heat = 0;
            return "need raw";
        };
        if item.id != "raw" {
            self.camp_heat = 0;
            return "need raw";
        }
        self.camp_heat = 1;
        self.finish_cook_raw()
    }

    /// One-step blast: hold cobble or stone, use a blast furnace, get the cook.
    pub fn blast_now(&mut self) -> &'static str {
        self.heat_now(blast_result, "need blast")
    }

    /// One-step furnace: hold clay, use a furnace, get bricks.
    pub fn furnace_now(&mut self) -> &'static str {
        self.heat_now(furnace_result, "need clay")
    }

    fn heat_now(
        &mut self,
        result: fn(&str) -> Option<&'static str>,
        need: &'static str,
    ) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return need;
        };
        let Some(out) = result(item.id) else {
            return need;
        };
        let count = item.count;
        if count > 1 && !self.room_for(out) {
            return "no room";
        }
        if count == 1 {
            if let Some(slot) = self.slots.get_mut(self.selected) {
                *slot = Some(fresh(out, 1));
            }
            return out;
        }
        self.consume_at(self.selected);
        if !self.collect(out) {
            return "no room";
        }
        out
    }

    fn finish_cook_raw(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            self.camp_heat = 0;
            return "need raw";
        };
        if item.id != "raw" {
            self.camp_heat = 0;
            return "need raw";
        }
        let count = item.count;
        if count > 1 && !self.room_for("cooked") {
            return "no room";
        }
        self.camp_heat = 0;
        if count == 1 {
            if let Some(slot) = self.slots.get_mut(self.selected) {
                *slot = Some(fresh("cooked", 1));
            }
            return "cooked";
        }
        self.consume_at(self.selected);
        if !self.collect("cooked") {
            return "no room";
        }
        "cooked"
    }

    /// Cut the selected stone. One stone becomes two slabs. A slab becomes
    /// stone stairs. Stone bricks become chiseled bricks.
    ///
    /// `"slab"`, `"stairs"`, and `"chiseled"` are the cuts. `"need stone"` is
    /// anything else. `"no room"` means the bar cannot hold the pieces.
    pub fn cut_stone(&mut self) -> &'static str {
        let Some(item) = self.item(self.selected) else {
            return "need stone";
        };
        match item.id {
            "stone" => self.cut_slabs(),
            "slab" => self.swap_cut("stone_stairs", "stairs"),
            "stone_bricks" => self.swap_cut("chiseled_stone_bricks", "chiseled"),
            _ => "need stone",
        }
    }

    fn cut_slabs(&mut self) -> &'static str {
        if self.room_after_one_spent("slab") < 2 {
            return "no room";
        }
        self.consume_at(self.selected);
        let _ = self.collect("slab");
        let _ = self.collect("slab");
        "slab"
    }

    /// Free stack space for `id` after spending one from the selected slot.
    fn room_after_one_spent(&self, id: &str) -> u16 {
        let cap = stack_cap(id);
        let mut room = 0u16;
        for (index, slot) in self.slots.iter().enumerate() {
            match slot {
                Some(item) if index == self.selected && item.count <= 1 => room += cap,
                Some(item) if item.id == id && item.count < cap => room += cap - item.count,
                None => room += cap,
                _ => {}
            }
        }
        room
    }

    fn swap_cut(&mut self, into: &'static str, word: &'static str) -> &'static str {
        let count = self.item(self.selected).map(|item| item.count).unwrap_or(0);
        if count == 0 {
            return "need stone";
        }
        if count == 1 {
            if let Some(slot) = self.slots.get_mut(self.selected) {
                *slot = Some(fresh(into, 1));
            }
            return word;
        }
        if !self.room_for(into) {
            return "no room";
        }
        self.consume_at(self.selected);
        if !self.collect(into) {
            return "no room";
        }
        word
    }

    /// Fill one level, or take bone meal when the bin is ready.
    pub fn compost_step(&self, level: u8) -> CompostStep {
        if level >= COMPOST_READY {
            if self.room_for("bone_meal") {
                CompostStep::Take
            } else {
                CompostStep::NoRoom
            }
        } else if let Some(slot) = self.compost_slot() {
            CompostStep::Fill(slot)
        } else {
            CompostStep::NeedPlants
        }
    }

    /// Pour one level from a water bucket, or scoop one level into an empty bucket.
    pub fn cauldron_step(&self, level: u8) -> CauldronStep {
        if level < CAULDRON_FULL {
            if let Some(slot) = self.index_of("water_bucket") {
                return if self.exchange_fits(slot, "bucket") {
                    CauldronStep::Pour(slot)
                } else {
                    CauldronStep::NoRoom
                };
            }
        }
        if level > 0 {
            if let Some(slot) = self.index_of("bucket") {
                return if self.exchange_fits(slot, "water_bucket") {
                    CauldronStep::Scoop(slot)
                } else {
                    CauldronStep::NoRoom
                };
            }
        }
        if level >= CAULDRON_FULL {
            CauldronStep::Full
        } else {
            CauldronStep::NeedBucket
        }
    }

    fn exchange_fits(&self, index: usize, to: &str) -> bool {
        self.count(index) <= 1 || self.room_for(to)
    }

    /// Add one glowstone, or set spawn once the anchor holds a charge.
    pub fn anchor_step(&self, level: u8) -> AnchorStep {
        if level >= ANCHOR_FULL {
            return AnchorStep::SetSpawn;
        }
        if let Some(slot) = self.index_of("glowstone") {
            return AnchorStep::Charge(slot);
        }
        if level > 0 {
            AnchorStep::SetSpawn
        } else {
            AnchorStep::NeedGlow
        }
    }

    /// Fill one level from a flower, or take honey when the hive is ready.
    pub fn hive_step(&self, level: u8) -> HiveStep {
        if level >= HONEY_READY {
            if self.room_for("honey") {
                HiveStep::Take
            } else {
                HiveStep::NoRoom
            }
        } else if let Some(slot) = self.index_of("flower") {
            HiveStep::Fill(slot)
        } else {
            HiveStep::NeedFlower
        }
    }

    fn compost_slot(&self) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.count > 0 && is_compost(item.id)))
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        let id = Block::new(id, 0).id;
        self.slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == id && item.count > 0))
    }

    /// Use one of `id` from any slot. At 0 that slot is empty.
    pub fn consume_one(&mut self, id: &str) -> bool {
        let id = Block::new(id, 0).id;
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|item| item.id == id && item.count > 0))
        else {
            return false;
        };
        self.consume_at(index);
        true
    }
}

/// Nine slots past the hotbar.
pub const PACK: usize = 9;

pub struct Bag {
    slots: [Option<Item>; PACK],
}

impl Bag {
    pub fn new() -> Self {
        Self {
            slots: [None; PACK],
        }
    }

    pub fn slot(&self, index: usize) -> Option<&str> {
        self.slots.get(index).copied().flatten().map(|item| item.id)
    }

    pub fn count(&self, index: usize) -> u16 {
        self.slots
            .get(index)
            .copied()
            .flatten()
            .map(|item| if item.uses > 0 { item.uses } else { item.count })
            .unwrap_or(0)
    }

    pub fn room_for(&self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        if is_tool(id) {
            return self.slots.iter().any(|slot| slot.is_none());
        }
        let cap = stack_cap(id);
        self.slots
            .iter()
            .flatten()
            .any(|slot| slot.id == id && slot.count < cap)
            || self.slots.iter().any(|slot| slot.is_none())
    }

    /// Add one of `id` to a stack that has room, or the first empty slot.
    pub fn collect(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        let cap = stack_cap(id);
        if !is_tool(id) {
            if let Some(slot) = self
                .slots
                .iter_mut()
                .flatten()
                .find(|slot| slot.id == id && slot.count < cap)
            {
                slot.count += 1;
                return true;
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(fresh(id, 1));
            return true;
        }
        false
    }

    /// Replace every slot. Ids are interned. A count above 64 is clamped.
    pub fn install(&mut self, slots: &[rebuild_world::CarriedSlot]) {
        self.slots = [None; PACK];
        for carried in slots {
            let index = carried.slot as usize;
            if index >= PACK || carried.id.is_empty() || carried.count == 0 {
                continue;
            }
            let id = Block::new(&carried.id, 0).id;
            self.slots[index] = Some(restored(id, carried.count));
        }
    }

    /// Nine wire pairs. An empty slot is an empty id and a count of 0.
    pub fn wire(&self) -> Vec<(String, i32)> {
        (0..PACK)
            .map(|index| match self.slot(index) {
                Some(id) => (id.to_string(), i32::from(self.count(index))),
                None => (String::new(), 0),
            })
            .collect()
    }

    /// Put one of `id` in that slot. A different id already there refuses.
    pub fn insert_at(&mut self, index: usize, id: &str) -> bool {
        if id.is_empty() || id == "air" {
            return false;
        }
        let id = Block::new(id, 0).id;
        let Some(slot) = self.slots.get_mut(index) else {
            return false;
        };
        match slot {
            Some(item) if item.id == id && item.uses == 0 && item.count < stack_cap(id) => {
                item.count += 1;
                true
            }
            None => {
                *slot = Some(fresh(id, 1));
                true
            }
            _ => false,
        }
    }

    /// Take one from `index`. The id stays interned for the rest of the process.
    pub fn take_one(&mut self, index: usize) -> Option<&'static str> {
        let slot = self.slots.get_mut(index)?;
        let item = (*slot)?;
        if item.count <= 1 {
            *slot = None;
        } else {
            *slot = Some(Item {
                count: item.count - 1,
                ..item
            });
        }
        Some(item.id)
    }

    /// Empty every slot, one item at a time.
    pub fn drain(&mut self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for index in 0..PACK {
            while let Some(id) = self.take_one(index) {
                out.push(id);
            }
        }
        out
    }
}

/// Spend one of each ingredient and put `result` in the hotbar, or the bag when the bar is full.
pub fn craft_carried(hotbar: &mut Hotbar, bag: &mut Bag, inputs: &[String], result: &str) -> bool {
    if result.is_empty() || inputs.is_empty() {
        return false;
    }
    if !hotbar.room_for(result) && !bag.room_for(result) {
        return false;
    }
    let mut need: Vec<(&str, u16)> = Vec::new();
    for id in inputs {
        if let Some((_, count)) = need.iter_mut().find(|(have, _)| *have == id.as_str()) {
            *count = count.saturating_add(1);
        } else {
            need.push((id.as_str(), 1));
        }
    }
    if need.iter().any(|(id, count)| !hotbar.holds(id, *count)) {
        return false;
    }
    for id in inputs {
        if !hotbar.consume_one(id) {
            return false;
        }
    }
    if !hotbar.collect(result) {
        bag.collect(result);
    }
    true
}

/// One screen-space rectangle. `min` is the lower-left corner in NDC, `max` the upper-right.
///
/// `tex` is an atlas column plus one. `0` keeps `color` flat. `span` is how
/// many 16px tiles the rectangle covers, so a panel tiles and an icon shows one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub color: [f32; 3],
    pub tex: f32,
    pub span: [f32; 2],
    /// Non-zero when a click on this rectangle means something. `0` is decoration.
    pub hit: u16,
}

impl Quad {
    /// Sample `id`'s pack texture across this rectangle. A missing PNG stays flat.
    pub fn face(mut self, id: &str, tiles_x: f32, tiles_y: f32) -> Self {
        let tex = crate::texture::block_face(id);
        if tex > 0.0 {
            self.tex = tex;
            self.span = [tiles_x.max(1.0), tiles_y.max(1.0)];
        }
        self
    }

    /// Bind an already-resolved atlas column (`slot + 1`).
    pub fn with_tex(mut self, tex: f32, tiles_x: f32, tiles_y: f32) -> Self {
        if tex > 0.0 {
            self.tex = tex;
            self.span = [tiles_x.max(1.0), tiles_y.max(1.0)];
        }
        self
    }

    pub fn tag(mut self, hit: u16) -> Self {
        self.hit = hit;
        self
    }
}

/// Draw a Blockbench item as its boxes. False when `id` is not one.
pub fn push_model_icon(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    size: f32,
    id: &str,
) -> bool {
    let Some(shape) = Pack::builtin().item_model(id) else {
        return false;
    };
    if shape.boxes().is_empty() {
        return false;
    }
    let tint = palette::color(id, 0);
    // Slight isometric skew so custom models read as 3D in a slot.
    for box_ in shape.boxes() {
        let z = (f32::from(box_.min[2]) + f32::from(box_.max[2])) * 0.5 / 16.0;
        let skew = (z - 0.5) * size * 0.22;
        let x0 = x + f32::from(box_.min[0]) / 16.0 * size + skew;
        let x1 = x + f32::from(box_.max[0]) / 16.0 * size + skew;
        let y0 = y + f32::from(box_.min[1]) / 16.0 * size + skew * 0.5;
        let y1 = y + f32::from(box_.max[1]) / 16.0 * size + skew * 0.5;
        quads.push(pixel_quad(width, height, x0, y0, x1, y1, tint));
    }
    true
}

fn push_flat_icon(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    size: f32,
    id: &str,
) {
    quads.push(
        pixel_quad(width, height, x, y, x + size, y + size, palette::color(id, 0)).face(id, 1.0, 1.0),
    );
}

/// Slot icon: soft-rendered model tile when present, else Blockbench boxes,
/// else a flat atlas face.
pub fn push_slot_icon(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    size: f32,
    id: &str,
) {
    if id.is_empty() || id == "air" || id == "empty" {
        return;
    }
    // Prefer the same display.gui raster egui uses (baked into the atlas).
    let model_tex = crate::texture::slot_icon_face(id);
    if model_tex > 0.0 {
        // Tint stays near-white for the atlas sample, but not exact COUNT white
        // so digit overlays (tex 0 + COUNT) stay distinct in tests/hit logic.
        quads.push(
            pixel_quad(width, height, x, y, x + size, y + size, [0.98, 0.98, 0.98])
                .with_tex(model_tex, 1.0, 1.0),
        );
        return;
    }
    if push_model_icon(quads, width, height, x, y, size, id) {
        return;
    }
    push_flat_icon(quads, width, height, x, y, size, id);
}

/// Background, then each slot's border and fill. The selected border is yellow and thicker.
#[cfg_attr(not(test), allow(dead_code))]
pub fn bar_quads(width: f32, height: f32, hotbar: &Hotbar) -> Vec<Quad> {
    bar_lifted(width, height, hotbar, 0.0)
}

/// Like [`bar_quads`], and `lift` raises the selected slot by that many pixels.
pub fn bar_lifted(width: f32, height: f32, hotbar: &Hotbar, lift: f32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let span = SLOTS as f32 * SLOT_PX + (SLOTS as f32 - 1.0) * GAP_PX;
    let left = (width - span) * 0.5;
    let bottom = MARGIN_PX;
    let mut quads = Vec::with_capacity(4 + SLOTS * 2);
    let bar_w = span + PAD_PX * 2.0;
    let bar_h = SLOT_PX + PAD_PX * 2.0;
    // Offhand sits left of the bar with a clear gap so it reads as equipment.
    let off_gap = GAP_PX * 2.0;
    let off_x = left - PAD_PX - off_gap - SLOT_PX;
    quads.push(
        pixel_quad(
            width,
            height,
            off_x - PAD_PX,
            bottom - PAD_PX,
            off_x + SLOT_PX + PAD_PX,
            bottom + SLOT_PX + PAD_PX,
            BAR,
        )
        .face("deepslate", (SLOT_PX + PAD_PX * 2.0) / 16.0, bar_h / 16.0),
    );
    quads.push(pixel_quad(
        width,
        height,
        off_x - PAD_PX,
        bottom + SLOT_PX + PAD_PX - 3.0,
        off_x + SLOT_PX + PAD_PX,
        bottom + SLOT_PX + PAD_PX,
        LIP,
    ));
    quads.push(
        pixel_quad(
            width,
            height,
            off_x - BORDER_PX,
            bottom - BORDER_PX,
            off_x + SLOT_PX + BORDER_PX,
            bottom + SLOT_PX + BORDER_PX,
            BORDER,
        )
        .tag(599),
    );
    let off_icon_x = off_x + ICON_INSET_PX;
    let off_icon_y = bottom + ICON_INSET_PX;
    let off_icon_size = SLOT_PX - ICON_INSET_PX * 2.0;
    match hotbar.offhand() {
        Some(id) => push_slot_icon(
            &mut quads,
            width,
            height,
            off_icon_x,
            off_icon_y,
            off_icon_size,
            id,
        ),
        None => quads.push(pixel_quad(
            width,
            height,
            off_icon_x,
            off_icon_y,
            off_icon_x + off_icon_size,
            off_icon_y + off_icon_size,
            EMPTY,
        )),
    }
    draw_count(
        &mut quads,
        width,
        height,
        off_x,
        bottom,
        hotbar.offhand_count(),
    );
    draw_wear(
        &mut quads,
        width,
        height,
        off_x,
        bottom,
        hotbar.offhand(),
        hotbar.offhand_count(),
    );
    quads.push(
        pixel_quad(
            width,
            height,
            left - PAD_PX,
            bottom - PAD_PX,
            left + span + PAD_PX,
            bottom + SLOT_PX + PAD_PX,
            BAR,
        )
        .face("deepslate", bar_w / 16.0, bar_h / 16.0),
    );
    quads.push(pixel_quad(
        width,
        height,
        left - PAD_PX,
        bottom + SLOT_PX + PAD_PX - 3.0,
        left + span + PAD_PX,
        bottom + SLOT_PX + PAD_PX,
        LIP,
    ));
    for index in 0..SLOTS {
        let x = left + index as f32 * (SLOT_PX + GAP_PX);
        let selected = index == hotbar.selected();
        let bottom = if selected {
            bottom + lift.max(0.0)
        } else {
            bottom
        };
        let border = if selected {
            SELECT_BORDER_PX
        } else {
            BORDER_PX
        };
        let border_color = if selected { SELECTED } else { BORDER };
        let well_tex = crate::texture::hud_face(crate::texture::HUD_HOTBAR_WELL);
        let sel_tex = crate::texture::hud_face(crate::texture::HUD_HOTBAR_SELECTION);
        let border_tex = if selected { sel_tex } else { well_tex };
        if border_tex > 0.0 {
            quads.push(
                pixel_quad(
                    width,
                    height,
                    x - border,
                    bottom - border,
                    x + SLOT_PX + border,
                    bottom + SLOT_PX + border,
                    [1.0, 1.0, 1.0],
                )
                .with_tex(border_tex, 1.0, 1.0)
                .tag(600 + index as u16),
            );
        } else {
            quads.push(
                pixel_quad(
                    width,
                    height,
                    x - border,
                    bottom - border,
                    x + SLOT_PX + border,
                    bottom + SLOT_PX + border,
                    border_color,
                )
                .tag(600 + index as u16),
            );
        }
        let icon_x = x + ICON_INSET_PX;
        let icon_y = bottom + ICON_INSET_PX;
        let icon_size = SLOT_PX - ICON_INSET_PX * 2.0;
        match hotbar.slot(index) {
            Some(id) => push_slot_icon(&mut quads, width, height, icon_x, icon_y, icon_size, id),
            None if well_tex > 0.0 => quads.push(
                pixel_quad(
                    width,
                    height,
                    icon_x,
                    icon_y,
                    icon_x + icon_size,
                    icon_y + icon_size,
                    [1.0, 1.0, 1.0],
                )
                .with_tex(well_tex, 1.0, 1.0),
            ),
            None => quads.push(pixel_quad(
                width,
                height,
                icon_x,
                icon_y,
                icon_x + icon_size,
                icon_y + icon_size,
                EMPTY,
            )),
        }
        draw_count(&mut quads, width, height, x, bottom, hotbar.count(index));
        draw_wear(
            &mut quads,
            width,
            height,
            x,
            bottom,
            hotbar.slot(index),
            hotbar.count(index),
        );
    }
    quads
}

const WEAR: [f32; 3] = [0.35, 0.78, 0.32];

fn draw_wear(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    x: f32,
    bottom: f32,
    id: Option<&str>,
    left: u16,
) {
    let Some(id) = id else {
        return;
    };
    let max = Pack::builtin().tool_uses(id);
    if max == 0 || left == 0 {
        return;
    }
    let span = SLOT_PX - FILL_INSET_PX * 2.0;
    let filled = span * (left.min(max) as f32 / max as f32);
    quads.push(pixel_quad(
        width,
        height,
        x + FILL_INSET_PX,
        bottom + FILL_INSET_PX,
        x + FILL_INSET_PX + filled,
        bottom + FILL_INSET_PX + 4.0,
        WEAR,
    ));
}

fn draw_count(quads: &mut Vec<Quad>, width: f32, height: f32, x: f32, bottom: f32, count: u16) {
    if count == 0 {
        return;
    }
    let text = count.to_string();
    let digits = text.len() as f32;
    let digit_w = 3.0 * DIGIT_CELL;
    let text_w = digits * digit_w + (digits - 1.0) * DIGIT_GAP;
    let right = x + SLOT_PX - FILL_INSET_PX - 2.0;
    let base = bottom + FILL_INSET_PX + 2.0;
    let mut cursor = right - text_w;
    for byte in text.bytes() {
        let digit = (byte - b'0') as usize;
        draw_glyph(
            quads,
            width,
            height,
            cursor + 1.0,
            base - 1.0,
            digit,
            COUNT_SHADOW,
        );
        draw_glyph(quads, width, height, cursor, base, digit, COUNT);
        cursor += digit_w + DIGIT_GAP;
    }
}

fn draw_glyph(
    quads: &mut Vec<Quad>,
    width: f32,
    height: f32,
    origin_x: f32,
    origin_y: f32,
    digit: usize,
    color: [f32; 3],
) {
    for (row, bits) in GLYPHS[digit].iter().enumerate() {
        let y = origin_y + (4 - row) as f32 * DIGIT_CELL;
        for col in 0..3 {
            if bits & (1 << (2 - col)) == 0 {
                continue;
            }
            let px = origin_x + col as f32 * DIGIT_CELL;
            quads.push(pixel_quad(
                width,
                height,
                px,
                y,
                px + DIGIT_CELL,
                y + DIGIT_CELL,
                color,
            ));
        }
    }
}

fn pixel_quad(
    width: f32,
    height: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 3],
) -> Quad {
    Quad {
        min: [x0 / width * 2.0 - 1.0, y0 / height * 2.0 - 1.0],
        max: [x1 / width * 2.0 - 1.0, y1 / height * 2.0 - 1.0],
        color,
        tex: 0.0,
        span: [0.0, 0.0],
        hit: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_cobblestone_in_slot_one() {
        let bar = Hotbar::new();
        assert_eq!(bar.selected(), 0);
        assert_eq!(bar.slot(0), Some("cobblestone"));
        assert_eq!(bar.count(0), 64);
        assert!(bar.slot(1).is_none());
        assert_eq!(bar.count(1), 0);
        assert!(bar.slot(2).is_none());
        assert!(bar.slot(3).is_none());
        assert!(bar.slot(4).is_none());
        assert_eq!(bar.selected_block(), Some(Block::new("cobblestone", 0)));
    }

    #[test]
    fn digits_one_through_ten_select_slots() {
        let mut bar = Hotbar::new();
        assert!(!bar.select_digit(0));
        assert!(!bar.select_digit(11));
        assert_eq!(bar.selected(), 0);
        for digit in 1..=10 {
            assert!(bar.select_digit(digit));
            assert_eq!(bar.selected(), (digit - 1) as usize);
        }
        assert!(bar.selected_block().is_none());
        assert!(bar.select_digit(1));
        assert_eq!(bar.selected_block(), Some(Block::new("cobblestone", 0)));
    }

    #[test]
    fn a_brush_wears_one_use_and_then_breaks() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("brush"));
        assert_eq!(bar.count(1), 8);
        assert!(bar.wear_use(1));
        assert_eq!(bar.slot(1), Some("brush"));
        assert_eq!(bar.count(1), 7);
        for _ in 0..7 {
            assert!(bar.wear_use(1));
        }
        assert!(bar.slot(1).is_none());
        assert!(!bar.wear_use(1));
    }

    #[test]
    fn collect_adds_to_a_stack_or_the_first_empty_slot() {
        let mut bar = Hotbar::new();
        bar.collect("air");
        bar.collect("");
        bar.consume_selected();
        assert_eq!(bar.count(0), 63);
        bar.collect("cobblestone");
        assert_eq!(bar.count(0), 64);
        assert!(bar.slot(1).is_none());
        bar.collect("cobblestone");
        assert_eq!(bar.count(0), 64);
        assert_eq!(bar.slot(1), Some("cobblestone"));
        assert_eq!(bar.count(1), 1);
        bar.collect("grass");
        assert_eq!(bar.slot(2), Some("grass"));
        assert_eq!(bar.count(2), 1);
        bar.collect("grass");
        assert_eq!(bar.count(2), 2);
        assert!(bar.slot(3).is_none());
        bar.collect("stone");
        bar.collect("dirt");
        assert_eq!(bar.slot(3), Some("stone"));
        assert_eq!(bar.slot(4), Some("dirt"));
        bar.collect("oak_log");
        assert_eq!(bar.slot(4), Some("dirt"));
        assert_eq!(bar.count(4), 1);
    }

    #[test]
    fn placing_one_drops_the_count_and_zero_is_empty() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.selected_block(), Some(Block::new("cobblestone", 0)));
        bar.consume_selected();
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.selected_block(), Some(Block::new("cobblestone", 0)));
        for _ in 0..63 {
            bar.consume_selected();
        }
        assert_eq!(bar.count(0), 0);
        assert!(bar.slot(0).is_none());
        assert!(bar.selected_block().is_none());
        bar.consume_selected();
        assert_eq!(bar.count(0), 0);
    }

    #[test]
    fn the_selected_slot_is_the_yellow_outline() {
        let mut bar = Hotbar::new();
        let quads = bar_quads(800.0, 600.0, &bar);
        let highlights: Vec<_> = quads.iter().filter(|quad| quad.color == SELECTED).collect();
        assert_eq!(highlights.len(), 1);
        let cobble = crate::texture::slot_icon_face("cobblestone");
        let cobble = if cobble > 0.0 {
            cobble
        } else {
            crate::texture::block_face("cobblestone")
        };
        let fill = quads
            .iter()
            .find(|quad| quad.tex == cobble && cobble > 0.0)
            .expect("cobblestone slot icon");
        let highlight = highlights[0];
        assert!(highlight.min[0] < fill.min[0] && highlight.min[1] < fill.min[1]);
        assert!(highlight.max[0] > fill.max[0] && highlight.max[1] > fill.max[1]);
        assert_eq!(quads.iter().filter(|quad| quad.color == EMPTY).count(), 9);

        let before = highlight.min[0];
        assert!(bar.select_digit(4));
        let moved = bar_quads(800.0, 600.0, &bar);
        let after = moved
            .iter()
            .find(|quad| quad.color == SELECTED)
            .expect("selected outline")
            .min[0];
        assert!(after > before);

        let resting = bar_quads(800.0, 600.0, &bar);
        let punched = bar_lifted(800.0, 600.0, &bar, 8.0);
        let top = |quads: &[Quad]| {
            quads
                .iter()
                .find(|quad| quad.color == SELECTED)
                .expect("selected outline")
                .max[1]
        };
        assert!(top(&punched) > top(&resting));
        assert!(resting.iter().any(|quad| quad.color == LIP));
    }

    #[test]
    fn the_count_is_drawn_on_the_slot() {
        let bar = Hotbar::new();
        let quads = bar_quads(800.0, 600.0, &bar);
        let ink: Vec<_> = quads.iter().filter(|quad| quad.color == COUNT).collect();
        let shadow = quads
            .iter()
            .filter(|quad| quad.color == COUNT_SHADOW)
            .count();
        // Digit glyphs are small white quads; exact count can shift slightly with
        // icon padding, but both digits must be present.
        assert_eq!(ink.len(), 21, "64 is two white digits");
        assert_eq!(shadow, 21);
        let outline = quads
            .iter()
            .find(|quad| quad.color == SELECTED)
            .expect("selected outline");
        assert!(ink.iter().all(|quad| {
            quad.min[0] >= outline.min[0]
                && quad.max[0] <= outline.max[0]
                && quad.min[1] >= outline.min[1]
                && quad.max[1] <= outline.max[1]
        }));
        let min_x = ink.iter().map(|quad| quad.min[0]).fold(f32::MAX, f32::min);
        let max_x = ink.iter().map(|quad| quad.max[0]).fold(f32::MIN, f32::max);
        assert!(max_x - min_x > 0.02, "the two digits sit side by side");

        let mut empty = Hotbar::new();
        for _ in 0..64 {
            empty.consume_selected();
        }
        let cleared = bar_quads(800.0, 600.0, &empty);
        assert!(cleared.iter().all(|quad| quad.color != COUNT));
    }

    #[test]
    fn a_coin_icon_uses_the_model_slot_tile() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("coin"));
        assert_eq!(bar.slot(1), Some("coin"));
        let quads = bar_quads(800.0, 600.0, &bar);
        let coin_tex = crate::texture::slot_icon_face("coin");
        assert!(coin_tex > 0.0, "coin should bake a slot-icon atlas tile");
        assert!(
            quads.iter().any(|quad| quad.tex == coin_tex),
            "coin draws via the model slot tile"
        );
    }

    #[test]
    fn give_stack_fills_the_selected_slot() {
        let mut bar = Hotbar::new();
        assert!(bar.give_stack("dirt"));
        assert_eq!(bar.slot(0), Some("dirt"));
        assert_eq!(bar.count(0), 64);
        bar.select_digit(2);
        assert!(bar.give_stack("stone"));
        assert_eq!(bar.slot(1), Some("stone"));
        assert_eq!(bar.count(1), 64);
        assert!(bar.slot(0) == Some("dirt"));
        assert!(!bar.give_stack("air"));
        assert!(!bar.give_stack(""));
        bar.select_digit(3);
        assert!(bar.give_stack("wooden_pickaxe"));
        assert_eq!(bar.count(2), 8);
        assert!(!bar.wear(2, "grass"));
        assert_eq!(bar.count(2), 8);
        assert!(bar.wear(2, "stone"));
        assert_eq!(bar.count(2), 7);
        let worn = bar_quads(800.0, 600.0, &bar);
        for _ in 0..6 {
            assert!(bar.wear(2, "cobblestone"));
        }
        assert!(bar.wear(2, "furnace"));
        assert!(bar.slot(2).is_none());
        let fresh = bar_quads(800.0, 600.0, &{
            let mut full = Hotbar::new();
            full.select_digit(3);
            full.give_stack("wooden_pickaxe");
            full
        });
        let wear_width = |quads: &[Quad]| {
            quads
                .iter()
                .filter(|quad| quad.color == WEAR)
                .map(|quad| quad.max[0] - quad.min[0])
                .fold(0.0f32, f32::max)
        };
        assert!(wear_width(&worn) < wear_width(&fresh));
        bar.select_digit(4);
        assert!(bar.give_stack("shield"));
        assert_eq!(bar.count(3), 4);
        assert_eq!(bar.selected_block(), None);
        assert!(bar.holding_shield());
        assert!(!bar.wear(3, "stone"));
        assert!(bar.block_bite());
        assert_eq!(bar.count(3), 3);
        assert!(bar.holding_shield());
        assert!(bar.block_bite());
        assert!(bar.block_bite());
        assert!(bar.block_bite());
        assert!(bar.slot(3).is_none());
        assert!(!bar.holding_shield());
        assert!(!bar.block_bite());
        assert!(bar.collect("bucket"));
        let bucket = (0..SLOTS)
            .find(|&index| bar.slot(index) == Some("bucket"))
            .expect("bucket");
        assert_eq!(bar.count(bucket), 1);
        assert!(bar.exchange_last(bucket, "bucket", "water_bucket"));
        assert_eq!(bar.slot(bucket), Some("water_bucket"));
        assert_eq!(bar.count(bucket), 1);
        assert!(bar.collect("bucket"));
        assert!(bar.collect("bucket"));
        let stack = (0..SLOTS)
            .find(|&index| bar.slot(index) == Some("bucket"))
            .expect("stack");
        assert_eq!(bar.count(stack), 2);
        assert!(bar.shrink(stack, "bucket"));
        assert_eq!(bar.count(stack), 1);
        let quads = bar_quads(800.0, 600.0, &bar);
        let dirt = crate::texture::slot_icon_face("dirt");
        let dirt = if dirt > 0.0 {
            dirt
        } else {
            crate::texture::block_face("dirt")
        };
        assert!(
            dirt > 0.0 && quads.iter().any(|quad| quad.tex == dirt),
            "dirt draws as a model slot icon"
        );
        assert!(
            quads
                .iter()
                .any(|quad| quad.tex > 0.0 && quad.span[0] > 1.0),
            "the bar chrome still tiles its atlas face"
        );
    }

    #[test]
    fn a_script_block_stocks_and_places_and_a_script_item_does_not() {
        let pack = Pack::builtin();
        pack.register_block_from("hotbar", "place_me", "hotbar-place-test")
            .unwrap();
        pack.register_item_from("hotbar", "not_a_block", "hotbar-place-test")
            .unwrap();
        let mut bar = Hotbar::new();
        assert_eq!(
            bar.stock_blocks(&[
                "hotbar:not_a_block".into(),
                "hotbar:place_me".into(),
                "missing".into(),
            ]),
            1
        );
        assert_eq!(bar.slot(1), Some("hotbar:place_me"));
        assert_eq!(bar.count(1), 64);
        assert!(bar.slot(2).is_none());
        assert!(bar.select_digit(2));
        assert_eq!(bar.selected_block(), Some(Block::new("hotbar:place_me", 0)));
        assert!(bar.collect("hotbar:not_a_block"));
        assert_eq!(bar.slot(2), Some("hotbar:not_a_block"));
        assert!(bar.select_digit(3));
        assert!(bar.selected_block().is_none());
        pack.set_item_places("hotbar:not_a_block", "hotbar:place_me", "hotbar-place-test")
            .unwrap();
        assert_eq!(bar.selected_block(), Some(Block::new("hotbar:place_me", 0)));
    }

    #[test]
    fn offhand_shield_blocks_and_swaps_with_selected() {
        let mut bar = Hotbar::new();
        assert!(bar.set_offhand("shield"));
        assert_eq!(bar.offhand(), Some("shield"));
        assert_eq!(bar.offhand_count(), 4);
        assert!(bar.holding_shield());
        assert!(bar.block_bite());
        assert_eq!(bar.offhand_count(), 3);
        bar.select_digit(1);
        assert!(bar.give_stack("iron_pickaxe"));
        bar.swap_offhand();
        assert_eq!(bar.offhand(), Some("iron_pickaxe"));
        assert_eq!(bar.slot(0), Some("shield"));
        assert!(bar.holding_shield());
        assert!(bar.block_bite());
        assert_eq!(bar.count(0), 2);
        let quads = bar_quads(800.0, 600.0, &bar);
        assert!(
            quads.iter().any(|quad| quad.hit == 599),
            "offhand HUD well is tagged"
        );
        // Persist through save slot 10.
        assert!(bar.set_offhand("totem_of_undying"));
        let slots = [
            rebuild_world::CarriedSlot {
                slot: 0,
                id: "shield".into(),
                count: 2,
            },
            rebuild_world::CarriedSlot {
                slot: OFFHAND_SAVE_SLOT,
                id: "totem_of_undying".into(),
                count: 1,
            },
        ];
        bar.install_saved(0, &slots);
        assert_eq!(bar.slot(0), Some("shield"));
        assert_eq!(bar.offhand(), Some("totem_of_undying"));
    }

    #[test]
    fn crafting_spends_cobblestone_and_a_full_bar_fills_the_bag() {
        let mut bar = Hotbar::new();
        let mut bag = Bag::new();
        assert!(craft_carried(
            &mut bar,
            &mut bag,
            &["cobblestone".to_string()],
            "stone"
        ));
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.slot(1), Some("stone"));
        assert_eq!(bar.count(1), 1);
        assert!(bag.slot(0).is_none());

        let mut full = Hotbar::new();
        for id in [
            "grass", "stone", "dirt", "oak_log", "wool", "sand", "gravel", "planks", "glass",
        ] {
            for _ in 0..64 {
                assert!(full.collect(id));
            }
        }
        assert!(!full.room_for("brick"));
        assert!(craft_carried(
            &mut full,
            &mut bag,
            &["cobblestone".to_string()],
            "stone"
        ));
        assert_eq!(full.count(0), 63);
        assert_eq!(bag.slot(0), Some("stone"));
        assert_eq!(bag.count(0), 1);
        assert_eq!(bag.take_one(0), Some("stone"));
        assert!(bag.slot(0).is_none());
        assert!(bag.collect("brick"));
        assert_eq!(bag.slot(0), Some("brick"));
    }

    #[test]
    fn a_creative_pick_survives_the_hotbar_save() {
        let mut hotbar = Hotbar::new();
        assert!(hotbar.give_stack("wool"));
        assert!(hotbar.select_digit(2));
        assert!(hotbar.give_stack("lantern"));
        assert!(hotbar.select_digit(3));
        assert!(hotbar.give_stack("coal_ore"));
        let slots: Vec<(String, i32)> = (0..SLOTS)
            .map(|index| match hotbar.slot(index) {
                Some(id) => (id.to_string(), i32::from(hotbar.count(index))),
                None => (String::new(), 0),
            })
            .collect();
        let saved = rebuild_world::bar_from_wire(3, &slots).unwrap();
        let path =
            std::env::temp_dir().join(format!("rebuild-creative-bar-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        rebuild_world::write_hotbar(&path, &saved).unwrap();
        let loaded = rebuild_world::read_hotbar(&path).unwrap().unwrap();
        let mut again = Hotbar::new();
        again.install_saved(loaded.selected, &loaded.slots);
        assert_eq!(again.slot(0), Some("wool"));
        assert_eq!(again.count(0), 64);
        assert_eq!(again.slot(1), Some("lantern"));
        assert_eq!(again.count(1), 64);
        assert_eq!(again.slot(2), Some("coal_ore"));
        assert_eq!(again.count(2), 64);
        assert_eq!(again.selected(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_anvil_repairs_a_worn_pick_for_one_iron_ore() {
        let mut bar = Hotbar::new();
        bar.select_digit(2);
        assert!(bar.give_stack("iron_pickaxe"));
        assert_eq!(bar.count(1), 32);
        assert!(bar.wear(1, "stone"));
        assert_eq!(bar.count(1), 31);
        assert_eq!(bar.repair_pick(), "need material");
        assert_eq!(bar.repair_target(), None);
        bar.select_digit(1);
        assert!(bar.give_stack("iron_ore"));
        bar.select_digit(2);
        assert_eq!(bar.repair_target(), Some("iron_pickaxe"));
        assert_eq!(bar.repair_pick(), "repaired");
        assert_eq!(bar.repair_target(), None);
        assert_eq!(bar.count(1), 32);
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.repair_pick(), "full");
        assert_eq!(bar.count(0), 63);
        bar.select_digit(3);
        assert!(bar.give_stack("shield"));
        assert_eq!(bar.repair_pick(), "need a tool");
    }

    #[test]
    fn an_enchanting_table_puts_efficiency_on_a_pick() {
        let mut bar = Hotbar::new();
        bar.select_digit(2);
        assert!(bar.give_stack("iron_pickaxe"));
        assert_eq!(bar.dig_factor("stone"), 6.0);
        assert_eq!(bar.dig_factor("dirt"), 1.0);
        assert_eq!(bar.enchant_pick(), "need lapis");
        assert_eq!(bar.enchant_target(), None);
        bar.select_digit(1);
        assert!(bar.give_stack("lapis_lazuli"));
        bar.select_digit(2);
        assert_eq!(bar.enchant_target(), Some("iron_pickaxe"));
        assert_eq!(bar.enchant_pick(), "efficiency");
        assert_eq!(bar.enchant_target(), None);
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.dig_factor("stone"), 12.0);
        assert_eq!(bar.dig_factor("dirt"), 1.0);
        assert_eq!(bar.enchant_pick(), "already enchanted");
        assert_eq!(bar.count(0), 63);
        bar.select_digit(3);
        assert!(bar.give_stack("shield"));
        assert_eq!(bar.enchant_pick(), "need a tool");
        let offers = enchant_offers_for("iron_pickaxe", 8, false, false, false);
        assert_eq!(offers.len(), 3);
        assert_eq!(offers[0].id, "efficiency");
    }

    #[test]
    fn a_brewing_stand_turns_water_and_wart_into_a_healing_potion() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.brew_potion(), "need water");
        assert!(bar.collect("water_bucket"));
        assert_eq!(bar.brew_potion(), "need blaze powder");
        assert_eq!(bar.brew_target(), None);
        assert!(bar.collect("nether_wart_block"));
        assert_eq!(bar.brew_potion(), "need blaze powder");
        assert_eq!(bar.brew_target(), None);
        assert!(bar.collect("blaze_powder"));
        assert_eq!(bar.brew_target(), Some("nether_wart_block"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert_eq!(bar.brew_target(), None);
        assert_eq!(bar.slot(1), Some("bucket"));
        assert!(bar.index_of("healing_potion").is_some());
        assert!(bar.index_of("blaze_powder").is_none());

        let mut flower = Hotbar::new();
        assert!(flower.collect("water_bucket"));
        assert!(flower.collect("flower"));
        assert!(flower.collect("blaze_powder"));
        assert_eq!(flower.brew_target(), Some("flower"));
        assert_eq!(flower.brew_potion(), "brewed");
        assert_eq!(flower.slot(1), Some("bucket"));
        assert!(flower.index_of("healing_potion").is_some());
    }

    #[test]
    fn a_brewing_stand_turns_water_and_powder_into_awkward_then_healing() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("water_bucket"));
        assert!(bar.collect("blaze_powder"));
        assert_eq!(bar.brew_target(), Some("blaze_powder"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert_eq!(bar.slot(1), Some("bucket"));
        assert_eq!(bar.slot(2), Some("awkward_potion"));
        assert!(bar.collect("nether_wart_block"));
        assert_eq!(bar.brew_target(), Some("nether_wart_block"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert!(bar.index_of("healing_potion").is_some());
        assert!(bar.index_of("awkward_potion").is_none());
    }

    #[test]
    fn a_brewing_stand_turns_awkward_and_powder_into_strength() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("awkward_potion"));
        assert!(bar.collect("blaze_powder"));
        assert_eq!(bar.brew_target(), Some("blaze_powder"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert_eq!(bar.slot(1), Some("strength_potion"));
        assert!(bar.index_of("awkward_potion").is_none());
        assert!(bar.index_of("blaze_powder").is_none());
    }

    #[test]
    fn a_brewing_stand_turns_awkward_and_magma_cream_into_fire_resistance() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("awkward_potion"));
        assert!(bar.collect("magma_cream"));
        assert_eq!(bar.brew_target(), Some("magma_cream"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert_eq!(bar.slot(1), Some("fire_resistance_potion"));
        assert!(bar.index_of("awkward_potion").is_none());
        assert!(bar.index_of("magma_cream").is_none());
    }

    #[test]
    fn a_brewing_stand_turns_awkward_and_ghast_tear_into_fire_resistance() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("awkward_potion"));
        assert!(bar.collect("ghast_tear"));
        assert_eq!(bar.brew_target(), Some("ghast_tear"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert!(bar.index_of("fire_resistance_potion").is_some());
    }

    #[test]
    fn a_brewing_stand_turns_healing_and_gravel_into_a_splash() {
        let mut bar = Hotbar::new();
        assert!(bar.collect("healing_potion"));
        assert!(bar.collect("gravel"));
        assert_eq!(bar.brew_target(), Some("gravel"));
        assert_eq!(bar.brew_potion(), "brewed");
        assert_eq!(bar.slot(1), Some("splash_healing_potion"));
        assert_eq!(bar.count(1), 1);
        assert_ne!(bar.slot(2), Some("gravel"));
    }

    #[test]
    fn a_grindstone_clears_efficiency_and_gives_four_uses_back() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.grind_pick(), "need a pick");
        bar.select_digit(2);
        assert!(bar.give_stack("iron_pickaxe"));
        for _ in 0..6 {
            assert!(bar.wear(1, "stone"));
        }
        assert_eq!(bar.count(1), 26);
        bar.select_digit(1);
        assert!(bar.give_stack("lapis_lazuli"));
        bar.select_digit(2);
        assert_eq!(bar.enchant_pick(), "efficiency");
        assert_eq!(bar.dig_factor("stone"), 12.0);
        assert_eq!(bar.grind_pick(), "ground");
        assert_eq!(bar.dig_factor("stone"), 6.0);
        assert_eq!(bar.count(1), 30);
        assert_eq!(bar.grind_pick(), "sharpened");
        assert_eq!(bar.count(1), 32);
        assert_eq!(bar.grind_pick(), "plain");
        bar.select_digit(3);
        assert!(bar.give_stack("shield"));
        assert_eq!(bar.grind_pick(), "need a pick");
    }

    #[test]
    fn a_smithing_table_turns_stone_into_iron_then_diamond() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.smith_pick(), "need a pick");
        bar.select_digit(2);
        assert!(bar.give_stack("stone_pickaxe"));
        assert_eq!(bar.smith_pick(), "need iron ore");
        assert_eq!(bar.smith_target(), None);
        bar.select_digit(1);
        assert!(bar.give_stack("iron_ore"));
        bar.select_digit(2);
        assert_eq!(bar.smith_target(), Some("stone_pickaxe"));
        assert_eq!(bar.smith_pick(), "iron");
        assert_eq!(bar.slot(1), Some("iron_pickaxe"));
        assert_eq!(bar.count(1), 32);
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.dig_factor("stone"), 6.0);
        assert_eq!(bar.smith_target(), Some("iron_pickaxe"));
        assert_eq!(bar.smith_pick(), "diamond");
        assert_eq!(bar.slot(1), Some("diamond_pickaxe"));
        assert_eq!(bar.count(1), 48);
        assert_eq!(bar.count(0), 62);
        assert_eq!(bar.dig_factor("stone"), 8.0);
        assert_eq!(bar.dig_factor("dirt"), 1.0);
        assert_eq!(bar.smith_target(), None);
        assert_eq!(bar.smith_pick(), "need ingot");
        bar.select_digit(3);
        assert!(bar.give_stack("netherite_ingot"));
        bar.select_digit(2);
        assert_eq!(bar.smith_target(), Some("diamond_pickaxe"));
        assert_eq!(bar.smith_pick(), "netherite");
        assert_eq!(bar.slot(1), Some("netherite_pickaxe"));
        assert_eq!(bar.count(1), 64);
        assert_eq!(bar.dig_factor("ancient_debris"), 10.0);
        assert_eq!(bar.smith_target(), None);
        assert_eq!(bar.smith_pick(), "best");
        bar.select_digit(4);
        assert!(bar.give_stack("wooden_pickaxe"));
        assert_eq!(bar.smith_pick(), "need a pick");
    }

    #[test]
    fn a_smithing_table_upgrades_diamond_gear_to_netherite() {
        let mut bar = Hotbar::new();
        bar.install_saved(0, &[]);
        bar.select_digit(2);
        assert!(bar.give_stack("diamond_sword"));
        assert_eq!(bar.smith_pick(), "need ingot");
        bar.select_digit(1);
        assert!(bar.give_stack("netherite_ingot"));
        bar.select_digit(2);
        assert_eq!(bar.smith_target(), Some("diamond_sword"));
        assert_eq!(bar.smith_pick(), "netherite");
        assert_eq!(bar.slot(1), Some("netherite_sword"));
        assert_eq!(bar.count(1), 64);
        assert_eq!(bar.dig_factor("stone"), 10.0);
        bar.select_digit(3);
        assert!(bar.give_stack("diamond_helmet"));
        bar.select_digit(1);
        assert!(bar.give_stack("netherite_ingot"));
        bar.select_digit(3);
        assert_eq!(bar.smith_pick(), "netherite");
        assert_eq!(bar.slot(2), Some("netherite_helmet"));
        assert_eq!(bar.smith_pick(), "best");
    }

    #[test]
    fn a_blast_furnace_turns_ancient_debris_into_netherite_scrap() {
        let mut bar = Hotbar::new();
        bar.install_saved(0, &[]);
        assert_eq!(bar.blast_now(), "need blast");
        assert!(bar.collect("ancient_debris"));
        assert!(bar.select_digit(1));
        assert_eq!(bar.slot(0), Some("ancient_debris"));
        assert_eq!(bar.blast_now(), "netherite_scrap");
        assert_eq!(bar.slot(0), Some("netherite_scrap"));
        assert_eq!(bar.blast_now(), "need blast");
    }

    #[test]
    fn a_smithing_table_sets_a_heavy_core_into_the_mace() {
        let mut bar = Hotbar::new();
        bar.select_digit(2);
        assert!(bar.give_stack("mace"));
        assert_eq!(bar.count(1), 16);
        assert_eq!(bar.smith_pick(), "need a core");
        assert_eq!(bar.smith_target(), None);
        bar.select_digit(1);
        assert!(bar.give_stack("heavy_core"));
        bar.select_digit(2);
        assert_eq!(bar.smith_target(), Some("mace"));
        assert_eq!(bar.smith_pick(), "heavy");
        assert_eq!(bar.smith_target(), None);
        assert_eq!(bar.slot(1), Some("heavy_mace"));
        assert_eq!(bar.count(1), 32);
        assert_eq!(bar.count(0), 63);
        assert_eq!(bar.smith_pick(), "heavy");
        assert_eq!(bar.count(0), 63);
    }

    #[test]
    fn a_loom_weaves_wool_into_a_banner() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.weave_banner(), "need wool");
        assert_eq!(bar.weave_target(), None);
        assert!(bar.collect("wool"));
        assert_eq!(bar.weave_target(), Some("wool"));
        assert_eq!(bar.weave_banner(), "woven");
        assert_eq!(bar.weave_target(), None);
        assert_eq!(bar.slot(1), Some("banner"));
        assert_eq!(bar.count(1), 1);
        assert!(bar.slot(0) != Some("banner"));
    }

    #[test]
    fn a_cartography_table_widens_the_held_map() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.chart_map(), "need a map");
        assert_eq!(bar.map_zoom(), 0);
        bar.select_digit(2);
        assert!(bar.give_stack("map"));
        assert_eq!(bar.chart_map(), "wider");
        assert_eq!(bar.map_zoom(), 1);
        assert_eq!(bar.chart_map(), "wider");
        assert_eq!(bar.map_zoom(), 2);
        assert_eq!(bar.chart_map(), "widest");
        assert_eq!(bar.map_zoom(), 2);
        bar.select_digit(1);
        assert_eq!(bar.chart_map(), "need a map");
        assert_eq!(bar.map_zoom(), 2);
    }

    #[test]
    fn a_book_sits_on_the_bar_until_it_is_spent() {
        let mut bar = Hotbar::new();
        assert!(bar.index_of("book").is_none());
        assert!(bar.collect("book"));
        assert_eq!(bar.index_of("book"), Some(1));
        assert_eq!(bar.count(1), 1);
        assert!(bar.consume_one("book"));
        assert!(bar.index_of("book").is_none());
    }

    #[test]
    fn a_composter_fills_from_a_plant_and_then_yields_bone_meal() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.compost_step(0), CompostStep::NeedPlants);
        assert_eq!(compost_prompt(0), "enter adds a plant");
        assert!(!is_compost("cobblestone"));
        assert!(is_compost("flower"));
        assert!(is_compost("apple"));
        assert!(!is_compost("bone_meal"));
        assert!(bar.collect("apple"));
        assert_eq!(bar.compost_step(0), CompostStep::Fill(1));
        assert_eq!(bar.compost_step(2), CompostStep::Fill(1));
        assert_eq!(compost_word(1), "level 1");
        assert_eq!(compost_word(2), "level 2");
        assert_eq!(compost_word(3), "level 3");
        assert_eq!(bar.compost_step(COMPOST_READY), CompostStep::Take);
        assert_eq!(compost_prompt(COMPOST_READY), "enter takes bone meal");
        assert!(bar.collect("stone"));
        assert!(bar.collect("dirt"));
        assert!(bar.collect("grass"));
        assert!(bar.collect("wool"));
        assert!(bar.collect("sand"));
        assert!(bar.collect("gravel"));
        assert!(bar.collect("oak_log"));
        assert!(bar.collect("glass"));
        assert_eq!(bar.compost_step(COMPOST_READY), CompostStep::NoRoom);
    }

    #[test]
    fn a_beehive_fills_from_a_flower_and_then_yields_honey() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.hive_step(0), HiveStep::NeedFlower);
        assert_eq!(hive_prompt(0), "enter adds a flower");
        assert!(bar.collect("flower"));
        assert_eq!(bar.hive_step(0), HiveStep::Fill(1));
        assert_eq!(bar.hive_step(2), HiveStep::Fill(1));
        assert_eq!(hive_word(1), "level 1");
        assert_eq!(hive_word(2), "level 2");
        assert_eq!(hive_word(3), "level 3");
        assert_eq!(bar.hive_step(HONEY_READY), HiveStep::Take);
        assert_eq!(hive_prompt(HONEY_READY), "enter takes honey");
        assert!(bar.collect("stone"));
        assert!(bar.collect("dirt"));
        assert!(bar.collect("grass"));
        assert!(bar.collect("wool"));
        assert!(bar.collect("sand"));
        assert!(bar.collect("gravel"));
        assert!(bar.collect("oak_log"));
        assert!(bar.collect("glass"));
        assert_eq!(bar.hive_step(HONEY_READY), HiveStep::NoRoom);
    }

    #[test]
    fn a_respawn_anchor_charges_from_glowstone_then_sets_spawn() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.anchor_step(0), AnchorStep::NeedGlow);
        assert_eq!(anchor_prompt(0), "enter adds glowstone");
        assert!(bar.collect("glowstone"));
        assert_eq!(bar.anchor_step(0), AnchorStep::Charge(1));
        assert_eq!(bar.anchor_step(3), AnchorStep::Charge(1));
        assert_eq!(anchor_word(1), "charge 1");
        assert_eq!(anchor_word(4), "charge 4");
        assert_eq!(bar.anchor_step(ANCHOR_FULL), AnchorStep::SetSpawn);
        assert_eq!(anchor_prompt(ANCHOR_FULL), "enter sets your spawn");
        bar.consume_at(1);
        assert_eq!(bar.anchor_step(1), AnchorStep::SetSpawn);
        assert_eq!(anchor_prompt(1), "enter charges or sets spawn");
    }

    #[test]
    fn a_cauldron_pours_from_a_water_bucket_and_scoops_into_an_empty_one() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.cauldron_step(0), CauldronStep::NeedBucket);
        assert_eq!(cauldron_prompt(0), "enter pours a water bucket");
        assert!(bar.collect("water_bucket"));
        assert_eq!(bar.cauldron_step(0), CauldronStep::Pour(1));
        assert_eq!(bar.cauldron_step(2), CauldronStep::Pour(1));
        assert_eq!(cauldron_word(1), "level 1");
        assert_eq!(cauldron_word(2), "level 2");
        assert_eq!(cauldron_word(3), "level 3");
        assert_eq!(bar.cauldron_step(CAULDRON_FULL), CauldronStep::Full);
        assert!(bar.collect("bucket"));
        assert_eq!(bar.cauldron_step(1), CauldronStep::Pour(1));
        assert_eq!(bar.cauldron_step(CAULDRON_FULL), CauldronStep::Scoop(2));
        assert_eq!(cauldron_prompt(CAULDRON_FULL), "enter scoops a bucket");
        assert_eq!(cauldron_prompt(1), "enter pours or scoops");
    }

    #[test]
    fn a_stonecutter_cuts_stone_into_slabs_stairs_and_chiseled_bricks() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.cut_stone(), "need stone");
        assert!(bar.collect("stone"));
        assert!(bar.select_digit(2));
        assert_eq!(bar.cut_stone(), "slab");
        assert_eq!(bar.slot(1), Some("slab"));
        assert_eq!(bar.count(1), 2);
        assert_eq!(bar.cut_stone(), "stairs");
        assert_eq!(bar.slot(1), Some("slab"));
        assert_eq!(bar.count(1), 1);
        assert_eq!(bar.slot(2), Some("stone_stairs"));
        assert_eq!(bar.cut_stone(), "stairs");
        assert_eq!(bar.slot(1), Some("stone_stairs"));
        assert!(bar.collect("stone_bricks"));
        assert!(bar.select_digit(4));
        assert_eq!(bar.slot(3), Some("stone_bricks"));
        assert_eq!(bar.cut_stone(), "chiseled");
        assert_eq!(bar.slot(3), Some("chiseled_stone_bricks"));
    }

    #[test]
    fn a_campfire_heats_raw_food_before_it_cooks() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.cook_raw(), "need raw");
        assert!(bar.collect("raw"));
        assert!(bar.select_digit(2));
        assert_eq!(bar.cook_raw(), "cooking");
        assert_eq!(bar.slot(1), Some("raw"));
        assert_eq!(bar.count(1), 1);
        assert_eq!(bar.cook_raw(), "cooked");
        assert_eq!(bar.slot(1), Some("cooked"));
        assert!(bar.select_digit(1));
        assert_eq!(bar.cook_raw(), "need raw");
        assert!(bar.collect("raw"));
        assert!(bar.select_digit(3));
        assert_eq!(bar.slot(2), Some("raw"));
        assert_eq!(bar.cook_raw(), "cooking");
        assert_eq!(bar.cook_raw(), "cooked");
        assert_eq!(bar.slot(2), Some("cooked"));
        assert!(bar.collect("raw"));
        assert!(bar.select_digit(4));
        assert_eq!(bar.cook_raw_now(), "cooked");
        assert_eq!(bar.slot(3), Some("cooked"));
    }

    #[test]
    fn a_blast_furnace_cooks_cobble_in_one_use() {
        let mut bar = Hotbar::new();
        assert!(bar.select_digit(2));
        assert_eq!(bar.blast_now(), "need blast");
        assert!(bar.select_digit(1));
        assert_eq!(bar.slot(0), Some("cobblestone"));
        let before = bar.count(0);
        assert_eq!(bar.blast_now(), "stone");
        assert_eq!(bar.slot(0), Some("cobblestone"));
        assert_eq!(bar.count(0), before - 1);
        assert_eq!(bar.slot(1), Some("stone"));
        assert!(bar.select_digit(2));
        assert_eq!(bar.blast_now(), "smooth_stone");
        assert_eq!(bar.slot(1), Some("smooth_stone"));
        assert_eq!(bar.blast_now(), "need blast");
    }

    #[test]
    fn a_furnace_bakes_clay_into_bricks_in_one_use() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.furnace_now(), "need clay");
        assert!(bar.collect("clay"));
        assert!(bar.select_digit(2));
        assert_eq!(bar.slot(1), Some("clay"));
        assert_eq!(bar.furnace_now(), "bricks");
        assert_eq!(bar.slot(1), Some("bricks"));
        assert_eq!(bar.furnace_now(), "need clay");
        assert_eq!(bar.blast_now(), "need blast");
    }

    #[test]
    fn a_table_shapes_bricks_and_clay_into_a_decorated_pot() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.shape_pot(), "need bricks");
        assert!(bar.collect("bricks"));
        assert_eq!(bar.shape_pot(), "need clay");
        assert!(bar.collect("clay"));
        assert_eq!(bar.shape_pot(), "potted");
        assert!(bar.index_of("bricks").is_none());
        assert!(bar.index_of("clay").is_none());
        assert_eq!(bar.index_of("decorated_pot"), Some(1));
        assert_eq!(bar.shape_pot(), "need bricks");
    }

    #[test]
    fn a_table_shapes_a_chest_and_shell_into_a_shulker_box() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.shape_box(), "need chest");
        assert!(bar.collect("chest"));
        assert_eq!(bar.shape_box(), "need shell");
        assert!(bar.collect("shulker_shell"));
        assert_eq!(bar.shape_box(), "boxed");
        assert!(bar.index_of("chest").is_none());
        assert!(bar.index_of("shulker_shell").is_none());
        assert_eq!(bar.index_of("shulker_box"), Some(1));
        assert_eq!(bar.shape_box(), "need chest");
    }

    #[test]
    fn a_fletching_table_turns_stick_wool_and_gravel_into_an_arrow() {
        let mut bar = Hotbar::new();
        assert_eq!(bar.fletch_arrow(), "need a stick");
        assert!(bar.collect("stick"));
        assert_eq!(bar.fletch_arrow(), "need wool");
        assert!(bar.collect("wool"));
        assert_eq!(bar.fletch_arrow(), "need gravel");
        assert!(bar.collect("gravel"));
        assert_eq!(bar.fletch_arrow(), "fletched");
        assert!(bar.index_of("stick").is_none());
        assert!(bar.index_of("wool").is_none());
        assert!(bar.index_of("gravel").is_none());
        assert_eq!(bar.index_of("arrow"), Some(1));
        assert_eq!(bar.count(1), 1);
    }
}
