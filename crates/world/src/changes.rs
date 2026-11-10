//! Block changes laid over the synthetic demo.
//!
//! A save is the cells a break or a place left behind. It is not a full world.
//! The hotbar, the nine inventory slots, and where the player is standing are
//! optional on the same object. A missing file is an empty list, so the demo
//! stays as it was generated. A block edit keeps whatever else the file held.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;

use crate::chunk::Block;
use crate::cluster::Cluster;

/// One cell after a break or a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellChange {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: Block,
}

/// How many slots the saved inventory holds. Empty slots are left out of the file.
pub const INVENTORY_SLOTS: usize = 9;

/// One occupied slot in the nine-slot inventory. `slot` is `0..9`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarriedSlot {
    pub slot: u8,
    pub id: String,
    pub count: u16,
}

/// How many main hotbar slots a save holds (`0..10`). Slot `10` is offhand.
pub const HOTBAR_SAVE_SLOTS: usize = 10;
/// Inclusive slot index for the offhand equipment stack in a hotbar save.
pub const HOTBAR_OFFHAND_SLOT: u8 = 10;
/// Parse limit so slot `10` (offhand) is accepted alongside the ten bar cells.
pub const HOTBAR_SAVE_SLOT_LIMIT: usize = 11;

/// The ten-slot hotbar. `selected` is `0..10`. Empty slots are left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotbarSave {
    pub selected: u8,
    pub slots: Vec<CarriedSlot>,
}

/// Eye position and look. Yaw and pitch are radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPlace {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// One block-entity chest. Empty slots are left out. `slots` uses `0..9`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredChest {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub id: String,
    pub slots: Vec<CarriedSlot>,
}

/// World-save statistics counters (blocks, walk, kills, time, deaths).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatisticsSave {
    pub blocks_broken: u64,
    pub blocks_placed: u64,
    pub distance_walked: f64,
    pub mobs_killed: u64,
    pub time_played: f64,
    pub deaths: u64,
}

impl StatisticsSave {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default)]
struct Aside {
    inventory: Vec<CarriedSlot>,
    hotbar: Option<HotbarSave>,
    player: Option<PlayerPlace>,
    chests: Vec<StoredChest>,
    /// Baked milestone ids the player has unlocked (advancement toasts).
    advancements: Vec<String>,
    statistics: StatisticsSave,
}

/// Read `path`. A file that is not there is an empty list.
pub fn read_changes(path: &Path) -> Result<Vec<CellChange>, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_changes(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Write every changed cell. Parent folders are created.
///
/// Inventory, hotbar, and player already in the file are written back.
pub fn write_changes(path: &Path, changes: &[CellChange]) -> Result<(), String> {
    let aside = read_aside(path)?;
    write_document(path, changes, &aside)
}

/// Write the nine-slot inventory. Cells, hotbar, and player stay.
pub fn write_inventory(path: &Path, slots: &[CarriedSlot]) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.inventory = slots.to_vec();
    write_document(path, &changes, &aside)
}

/// Write the hotbar. Cells, inventory, and player stay.
pub fn write_hotbar(path: &Path, hotbar: &HotbarSave) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.hotbar = Some(hotbar.clone());
    write_document(path, &changes, &aside)
}

/// Write where the player is standing. Cells, inventory, and hotbar stay.
pub fn write_player(path: &Path, player: PlayerPlace) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.player = Some(player);
    write_document(path, &changes, &aside)
}

/// Write unlocked advancement ids. Cells, inventory, hotbar, and player stay.
pub fn write_advancements(path: &Path, ids: &[String]) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.advancements = ids.to_vec();
    write_document(path, &changes, &aside)
}

/// Unlocked advancement ids in `path`. Missing file / field is empty.
pub fn read_advancements(path: &Path) -> Result<Vec<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            parse_advancements(&text).map_err(|err| format!("{}: {err}", path.display()))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Write world statistics. Cells, inventory, hotbar, player, and advancements stay.
pub fn write_statistics(path: &Path, stats: &StatisticsSave) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.statistics = stats.clone();
    write_document(path, &changes, &aside)
}

/// World statistics in `path`. Missing file / field is empty defaults.
pub fn read_statistics(path: &Path) -> Result<StatisticsSave, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_statistics(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(StatisticsSave::default()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Replace the chest list. Cells, inventory, hotbar, and player stay.
pub fn write_chests(path: &Path, chests: &[StoredChest]) -> Result<(), String> {
    let changes = read_changes(path)?;
    let mut aside = read_aside(path)?;
    aside.chests = chests.to_vec();
    write_document(path, &changes, &aside)
}

/// Chests in `path`. A missing file, or a save with none, is empty.
pub fn read_chests(path: &Path) -> Result<Vec<StoredChest>, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_chests(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Add an empty chest when that cell has none. An existing chest is left alone.
///
/// An ender chest keeps one personal store, shared by every ender chest.
pub fn ensure_chest(path: &Path, x: i32, y: i32, z: i32, id: &str) -> Result<(), String> {
    if id == "ender_chest" {
        return ensure_ender_chest(path);
    }
    let mut chests = read_chests(path)?;
    if chests
        .iter()
        .any(|chest| chest.x == x && chest.y == y && chest.z == z)
    {
        return Ok(());
    }
    chests.push(StoredChest {
        x,
        y,
        z,
        id: id.to_string(),
        slots: Vec::new(),
    });
    write_chests(path, &chests)
}

/// The personal ender chest. Every ender chest reads this list.
pub fn read_ender_chest(path: &Path) -> Result<Vec<CarriedSlot>, String> {
    let chests = read_chests(path)?;
    Ok(chests
        .into_iter()
        .find(|chest| chest.id == "ender_chest")
        .map(|chest| chest.slots)
        .unwrap_or_default())
}

fn ensure_ender_chest(path: &Path) -> Result<(), String> {
    let chests = read_chests(path)?;
    if chests.iter().any(|chest| chest.id == "ender_chest") {
        return Ok(());
    }
    store_ender_chest(path, &[])
}

/// Replace the personal ender chest. Per-cell copies fold into the one store.
pub fn store_ender_chest(path: &Path, slots: &[CarriedSlot]) -> Result<(), String> {
    let mut chests = read_chests(path)?;
    chests.retain(|chest| chest.id != "ender_chest");
    chests.push(StoredChest {
        x: 0,
        y: -1,
        z: 0,
        id: "ender_chest".to_string(),
        slots: slots.to_vec(),
    });
    write_chests(path, &chests)
}

/// Drop the chest at that cell, if there is one.
pub fn remove_chest(path: &Path, x: i32, y: i32, z: i32) -> Result<(), String> {
    let mut chests = read_chests(path)?;
    let before = chests.len();
    chests.retain(|chest| chest.x != x || chest.y != y || chest.z != z);
    if chests.len() == before {
        return Ok(());
    }
    write_chests(path, &chests)
}

/// Replace the slots of the chest at that cell.
pub fn store_chest_slots(
    path: &Path,
    x: i32,
    y: i32,
    z: i32,
    id: &str,
    slots: &[CarriedSlot],
) -> Result<(), String> {
    if id == "ender_chest" {
        return store_ender_chest(path, slots);
    }
    let mut chests = read_chests(path)?;
    if let Some(chest) = chests
        .iter_mut()
        .find(|chest| chest.x == x && chest.y == y && chest.z == z)
    {
        chest.id = id.to_string();
        chest.slots = slots.to_vec();
    } else {
        chests.push(StoredChest {
            x,
            y,
            z,
            id: id.to_string(),
            slots: slots.to_vec(),
        });
    }
    write_chests(path, &chests)
}

/// The inventory in `path`. A missing file, or a save with only blocks, is empty.
pub fn read_inventory(path: &Path) -> Result<Vec<CarriedSlot>, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_inventory(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

fn read_aside(path: &Path) -> Result<Aside, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_aside(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Aside::default()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

fn parse_aside(text: &str) -> Result<Aside, String> {
    Ok(Aside {
        inventory: parse_inventory(text)?,
        hotbar: parse_hotbar(text)?,
        player: parse_player(text)?,
        chests: parse_chests(text)?,
        advancements: parse_advancements(text)?,
        statistics: parse_statistics(text)?,
    })
}

fn write_document(path: &Path, changes: &[CellChange], aside: &Aside) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
    }
    let text = format_document(
        changes,
        &aside.inventory,
        aside.hotbar.as_ref(),
        aside.player,
        &aside.chests,
        &aside.advancements,
        &aside.statistics,
    );
    fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

pub fn parse_changes(text: &str) -> Result<Vec<CellChange>, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let root = root
        .as_object()
        .ok_or_else(|| "save must be an object".to_string())?;
    let blocks = root
        .get("blocks")
        .ok_or_else(|| "save is missing blocks".to_string())?
        .as_array()
        .ok_or_else(|| "blocks must be an array".to_string())?;
    let mut changes: Vec<CellChange> = Vec::with_capacity(blocks.len());
    for (index, entry) in blocks.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| format!("block {index} must be an object"))?;
        let x = coord(obj, "x", index)?;
        let y = coord(obj, "y", index)?;
        let z = coord(obj, "z", index)?;
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("block {index} needs a string id"))?;
        let metadata = match obj.get("metadata") {
            None => 0,
            Some(value) => {
                let metadata = value
                    .as_u64()
                    .ok_or_else(|| format!("block {index} metadata must be an integer"))?;
                if metadata > 15 {
                    return Err(format!("block {index} metadata {metadata} is above 15"));
                }
                metadata as u8
            }
        };
        let fill = match obj.get("fill") {
            None => 0,
            Some(value) => {
                let fill = value
                    .as_u64()
                    .ok_or_else(|| format!("block {index} fill must be an integer"))?;
                if fill > 3 {
                    return Err(format!("block {index} fill {fill} is above 3"));
                }
                fill as u8
            }
        };
        let block = Block::new(id, metadata).with_fill(fill);
        if let Some(existing) = changes
            .iter_mut()
            .find(|cell| cell.x == x && cell.y == y && cell.z == z)
        {
            existing.block = block;
        } else {
            changes.push(CellChange { x, y, z, block });
        }
    }
    Ok(changes)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn format_changes(changes: &[CellChange]) -> String {
    format_document(
        changes,
        &[],
        None,
        None,
        &[],
        &[],
        &StatisticsSave::default(),
    )
}

fn format_document(
    changes: &[CellChange],
    inventory: &[CarriedSlot],
    hotbar: Option<&HotbarSave>,
    player: Option<PlayerPlace>,
    chests: &[StoredChest],
    advancements: &[String],
    statistics: &StatisticsSave,
) -> String {
    let mut cells = changes.to_vec();
    cells.sort_by_key(|cell| (cell.z, cell.y, cell.x));
    let mut blocks = String::from("  \"blocks\": [\n");
    for (index, cell) in cells.iter().enumerate() {
        let comma = if index + 1 == cells.len() { "" } else { "," };
        if cell.block.fill == 0 {
            blocks.push_str(&format!(
                "    {{ \"x\": {}, \"y\": {}, \"z\": {}, \"id\": {}, \"metadata\": {} }}{comma}\n",
                cell.x,
                cell.y,
                cell.z,
                json_string(cell.block.id),
                cell.block.metadata
            ));
        } else {
            blocks.push_str(&format!(
                "    {{ \"x\": {}, \"y\": {}, \"z\": {}, \"id\": {}, \"metadata\": {}, \"fill\": {} }}{comma}\n",
                cell.x,
                cell.y,
                cell.z,
                json_string(cell.block.id),
                cell.block.metadata,
                cell.block.fill
            ));
        }
    }
    blocks.push_str("  ]");
    let mut sections = vec![blocks];
    if !inventory.is_empty() {
        sections.push(format_slot_list("inventory", inventory));
    }
    if let Some(hotbar) = hotbar {
        sections.push(format_hotbar(hotbar));
    }
    if let Some(player) = player {
        sections.push(format!(
            "  \"player\": {{ \"x\": {}, \"y\": {}, \"z\": {}, \"yaw\": {}, \"pitch\": {} }}",
            json_num(player.x),
            json_num(player.y),
            json_num(player.z),
            json_num(player.yaw),
            json_num(player.pitch)
        ));
    }
    if !chests.is_empty() {
        sections.push(format_chests(chests));
    }
    if !advancements.is_empty() {
        sections.push(format_advancements(advancements));
    }
    if !statistics.is_empty() {
        sections.push(format_statistics(statistics));
    }
    let mut out = String::from("{\n");
    for (index, section) in sections.iter().enumerate() {
        out.push_str(section);
        if index + 1 != sections.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn format_statistics(stats: &StatisticsSave) -> String {
    format!(
        "  \"statistics\": {{\n    \"blocks_broken\": {},\n    \"blocks_placed\": {},\n    \"distance_walked\": {},\n    \"mobs_killed\": {},\n    \"time_played\": {},\n    \"deaths\": {}\n  }}",
        stats.blocks_broken,
        stats.blocks_placed,
        json_num(stats.distance_walked as f32),
        stats.mobs_killed,
        json_num(stats.time_played as f32),
        stats.deaths
    )
}

pub fn parse_statistics(text: &str) -> Result<StatisticsSave, String> {
    let root = save_object(text)?;
    let Some(obj) = root.get("statistics") else {
        return Ok(StatisticsSave::default());
    };
    let obj = obj
        .as_object()
        .ok_or_else(|| "statistics must be an object".to_string())?;
    let u64_field = |key: &str| -> Result<u64, String> {
        match obj.get(key) {
            None => Ok(0),
            Some(value) => value
                .as_u64()
                .ok_or_else(|| format!("statistics.{key} must be an integer")),
        }
    };
    let f64_field = |key: &str| -> Result<f64, String> {
        match obj.get(key) {
            None => Ok(0.0),
            Some(value) => value
                .as_f64()
                .ok_or_else(|| format!("statistics.{key} must be a number")),
        }
    };
    Ok(StatisticsSave {
        blocks_broken: u64_field("blocks_broken")?,
        blocks_placed: u64_field("blocks_placed")?,
        distance_walked: f64_field("distance_walked")?.max(0.0),
        mobs_killed: u64_field("mobs_killed")?,
        time_played: f64_field("time_played")?.max(0.0),
        deaths: u64_field("deaths")?,
    })
}

fn format_advancements(ids: &[String]) -> String {
    let mut ordered = ids.to_vec();
    ordered.sort();
    ordered.dedup();
    let mut out = String::from("  \"advancements\": [\n");
    for (index, id) in ordered.iter().enumerate() {
        let comma = if index + 1 == ordered.len() { "" } else { "," };
        out.push_str(&format!("    {}{comma}\n", json_string(id)));
    }
    out.push_str("  ]");
    out
}

pub fn parse_advancements(text: &str) -> Result<Vec<String>, String> {
    let root = save_object(text)?;
    let Some(entries) = root.get("advancements") else {
        return Ok(Vec::new());
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| "advancements must be an array".to_string())?;
    let mut ids = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let id = entry
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 32)
            .ok_or_else(|| format!("advancement {index} needs a short string id"))?
            .to_string();
        if ids.iter().any(|have| have == &id) {
            return Err(format!("advancement {id} is listed twice"));
        }
        ids.push(id);
    }
    Ok(ids)
}

fn format_hotbar(hotbar: &HotbarSave) -> String {
    let mut ordered = hotbar.slots.clone();
    ordered.sort_by_key(|slot| slot.slot);
    let mut out = format!(
        "  \"hotbar\": {{\n    \"selected\": {},\n    \"slots\": [\n",
        hotbar.selected
    );
    for (index, slot) in ordered.iter().enumerate() {
        let comma = if index + 1 == ordered.len() { "" } else { "," };
        out.push_str(&format!(
            "      {{ \"slot\": {}, \"id\": {}, \"count\": {} }}{comma}\n",
            slot.slot,
            json_string(&slot.id),
            slot.count
        ));
    }
    out.push_str("    ]\n  }");
    out
}

fn format_chests(chests: &[StoredChest]) -> String {
    let mut ordered = chests.to_vec();
    ordered.sort_by_key(|chest| (chest.z, chest.y, chest.x));
    let mut out = String::from("  \"chests\": [\n");
    for (index, chest) in ordered.iter().enumerate() {
        let comma = if index + 1 == ordered.len() { "" } else { "," };
        out.push_str(&format!(
            "    {{ \"x\": {}, \"y\": {}, \"z\": {}, \"id\": {}, \"slots\": [\n",
            chest.x,
            chest.y,
            chest.z,
            json_string(&chest.id)
        ));
        let mut slots = chest.slots.clone();
        slots.sort_by_key(|slot| slot.slot);
        for (slot_index, slot) in slots.iter().enumerate() {
            let slot_comma = if slot_index + 1 == slots.len() {
                ""
            } else {
                ","
            };
            out.push_str(&format!(
                "      {{ \"slot\": {}, \"id\": {}, \"count\": {} }}{slot_comma}\n",
                slot.slot,
                json_string(&slot.id),
                slot.count
            ));
        }
        out.push_str(&format!("    ] }}{comma}\n"));
    }
    out.push_str("  ]");
    out
}

fn format_slot_list(name: &str, slots: &[CarriedSlot]) -> String {
    let mut ordered = slots.to_vec();
    ordered.sort_by_key(|slot| slot.slot);
    let mut out = format!("  \"{name}\": [\n");
    for (index, slot) in ordered.iter().enumerate() {
        let comma = if index + 1 == ordered.len() { "" } else { "," };
        out.push_str(&format!(
            "    {{ \"slot\": {}, \"id\": {}, \"count\": {} }}{comma}\n",
            slot.slot,
            json_string(&slot.id),
            slot.count
        ));
    }
    out.push_str("  ]");
    out
}

pub fn parse_inventory(text: &str) -> Result<Vec<CarriedSlot>, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let root = root
        .as_object()
        .ok_or_else(|| "save must be an object".to_string())?;
    let Some(entries) = root.get("inventory") else {
        return Ok(Vec::new());
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| "inventory must be an array".to_string())?;
    let mut slots = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| format!("inventory {index} must be an object"))?;
        let slot = obj
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("inventory {index} needs a slot"))?;
        if slot >= INVENTORY_SLOTS as u64 {
            return Err(format!("inventory {index} slot {slot} is past the bag"));
        }
        if slots
            .iter()
            .any(|held: &CarriedSlot| held.slot == slot as u8)
        {
            return Err(format!("inventory slot {slot} is listed twice"));
        }
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && *id != "air" && id.len() <= 64)
            .ok_or_else(|| format!("inventory {index} needs a string id"))?
            .to_string();
        let count = obj
            .get("count")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("inventory {index} needs a count"))?;
        if !(1..=64).contains(&count) {
            return Err(format!("inventory {index} count {count} is out of range"));
        }
        slots.push(CarriedSlot {
            slot: slot as u8,
            id,
            count: count as u16,
        });
    }
    Ok(slots)
}

/// The hotbar in `path`. A missing file, or a save with no hotbar, is `None`.
pub fn read_hotbar(path: &Path) -> Result<Option<HotbarSave>, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_hotbar(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// Where the player was standing. A missing file, or a save with no player, is `None`.
pub fn read_player(path: &Path) -> Result<Option<PlayerPlace>, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_player(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

pub fn parse_hotbar(text: &str) -> Result<Option<HotbarSave>, String> {
    let root = save_object(text)?;
    let Some(hotbar) = root.get("hotbar") else {
        return Ok(None);
    };
    let obj = hotbar
        .as_object()
        .ok_or_else(|| "hotbar must be an object".to_string())?;
    let selected = obj
        .get("selected")
        .and_then(Value::as_u64)
        .ok_or_else(|| "hotbar needs a selected slot".to_string())?;
    if selected >= HOTBAR_SAVE_SLOTS as u64 {
        return Err(format!("hotbar selected {selected} is past the bar"));
    }
    let entries = obj
        .get("slots")
        .and_then(Value::as_array)
        .ok_or_else(|| "hotbar slots must be an array".to_string())?;
    let slots = parse_slot_entries(entries, HOTBAR_SAVE_SLOT_LIMIT, "hotbar")?;
    Ok(Some(HotbarSave {
        selected: selected as u8,
        slots,
    }))
}

pub fn parse_chests(text: &str) -> Result<Vec<StoredChest>, String> {
    let root = save_object(text)?;
    let Some(entries) = root.get("chests") else {
        return Ok(Vec::new());
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| "chests must be an array".to_string())?;
    let mut chests = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| format!("chest {index} must be an object"))?;
        let x = chest_coord(obj, "x", index)?;
        let y = chest_coord(obj, "y", index)?;
        let z = chest_coord(obj, "z", index)?;
        if chests
            .iter()
            .any(|chest: &StoredChest| chest.x == x && chest.y == y && chest.z == z)
        {
            return Err(format!("chest at {x} {y} {z} is listed twice"));
        }
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 64)
            .ok_or_else(|| format!("chest {index} needs a string id"))?
            .to_string();
        let slots = obj
            .get("slots")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("chest {index} slots must be an array"))?;
        let slots = parse_slot_entries(slots, INVENTORY_SLOTS, "chest")?;
        chests.push(StoredChest { x, y, z, id, slots });
    }
    Ok(chests)
}

fn chest_coord(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    index: usize,
) -> Result<i32, String> {
    let number = obj
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("chest {index} needs {key}"))?;
    i32::try_from(number).map_err(|_| format!("chest {index} {key} is out of range"))
}

pub fn parse_player(text: &str) -> Result<Option<PlayerPlace>, String> {
    let root = save_object(text)?;
    let Some(player) = root.get("player") else {
        return Ok(None);
    };
    let obj = player
        .as_object()
        .ok_or_else(|| "player must be an object".to_string())?;
    Ok(Some(PlayerPlace {
        x: place_axis(obj, "x", -8000.0, 8000.0)?,
        y: place_axis(obj, "y", -64.0, 320.0)?,
        z: place_axis(obj, "z", -8000.0, 8000.0)?,
        yaw: place_axis(obj, "yaw", -20.0, 20.0)?,
        pitch: place_axis(obj, "pitch", -1.6, 1.6)?,
    }))
}

fn save_object(text: &str) -> Result<serde_json::Map<String, Value>, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    root.as_object()
        .cloned()
        .ok_or_else(|| "save must be an object".to_string())
}

fn place_axis(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    min: f32,
    max: f32,
) -> Result<f32, String> {
    let value = obj
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("player needs {key}"))?;
    if !value.is_finite() {
        return Err(format!("player {key} is not finite"));
    }
    let value = value as f32;
    if !(min..=max).contains(&value) {
        return Err(format!("player {key} is out of range"));
    }
    Ok(value)
}

fn parse_slot_entries(
    entries: &[Value],
    limit: usize,
    what: &str,
) -> Result<Vec<CarriedSlot>, String> {
    let mut slots = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| format!("{what} {index} must be an object"))?;
        let slot = obj
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{what} {index} needs a slot"))?;
        if slot >= limit as u64 {
            return Err(format!("{what} {index} slot {slot} is past the list"));
        }
        if slots
            .iter()
            .any(|held: &CarriedSlot| held.slot == slot as u8)
        {
            return Err(format!("{what} slot {slot} is listed twice"));
        }
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && *id != "air" && id.len() <= 64)
            .ok_or_else(|| format!("{what} {index} needs a string id"))?
            .to_string();
        let count = obj
            .get("count")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{what} {index} needs a count"))?;
        if !(1..=64).contains(&count) {
            return Err(format!("{what} {index} count {count} is out of range"));
        }
        slots.push(CarriedSlot {
            slot: slot as u8,
            id,
            count: count as u16,
        });
    }
    Ok(slots)
}

/// Ten or eleven wire pairs become a hotbar. `selected` is 1-based (bar only).
/// Index `10` is the offhand equipment slot when present.
pub fn bar_from_wire(selected: i32, slots: &[(String, i32)]) -> Result<HotbarSave, String> {
    if slots.len() != HOTBAR_SAVE_SLOTS && slots.len() != HOTBAR_SAVE_SLOT_LIMIT {
        return Err(format!("hotbar has {} slots", slots.len()));
    }
    if !(1..=HOTBAR_SAVE_SLOTS as i32).contains(&selected) {
        return Err(format!("hotbar selected {selected} is past the bar"));
    }
    let mut out = Vec::new();
    for (index, (id, count)) in slots.iter().enumerate() {
        if id.is_empty() || *count <= 0 {
            continue;
        }
        if id == "air" || id.len() > 64 {
            return Err(format!("hotbar slot {index} has a bad id"));
        }
        if *count > 64 {
            return Err(format!("hotbar slot {index} count {count} is above 64"));
        }
        out.push(CarriedSlot {
            slot: index as u8,
            id: id.clone(),
            count: *count as u16,
        });
    }
    Ok(HotbarSave {
        selected: (selected - 1) as u8,
        slots: out,
    })
}

/// Nine wire pairs become occupied slots. An empty id or a count of 0 is an empty slot.
pub fn carried_from_wire(slots: &[(String, i32)]) -> Result<Vec<CarriedSlot>, String> {
    if slots.len() != INVENTORY_SLOTS {
        return Err(format!("inventory has {} slots", slots.len()));
    }
    let mut out = Vec::new();
    for (index, (id, count)) in slots.iter().enumerate() {
        if id.is_empty() || *count <= 0 {
            continue;
        }
        if id == "air" || id.len() > 64 {
            return Err(format!("inventory slot {index} has a bad id"));
        }
        if *count > 64 {
            return Err(format!("inventory slot {index} count {count} is above 64"));
        }
        out.push(CarriedSlot {
            slot: index as u8,
            id: id.clone(),
            count: *count as u16,
        });
    }
    Ok(out)
}

/// Lay saved cells over `world`. Cells outside the cluster are skipped.
pub fn apply_changes(world: &mut Cluster, changes: &[CellChange]) -> usize {
    let mut applied = 0;
    for change in changes {
        if world.set_block(change.x, change.y, change.z, change.block) {
            applied += 1;
        }
    }
    applied
}

fn coord(obj: &serde_json::Map<String, Value>, key: &str, index: usize) -> Result<i32, String> {
    let value = obj
        .get(key)
        .ok_or_else(|| format!("block {index} needs {key}"))?;
    let number = value
        .as_i64()
        .ok_or_else(|| format!("block {index} {key} must be an integer"))?;
    i32::try_from(number).map_err(|_| format!("block {index} {key} is out of range"))
}

fn json_num(value: f32) -> String {
    let text = format!("{value:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" || trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Edit, apply};
    use crate::fixture::synthetic_cluster;
    use crate::shape::COAL_ORE;
    use std::path::PathBuf;

    #[test]
    fn a_missing_save_leaves_the_demo_unchanged() {
        let missing = std::env::temp_dir().join("rebuild-missing-blocks-save.json");
        let _ = fs::remove_file(&missing);
        let changes = read_changes(&missing).unwrap();
        assert!(changes.is_empty());

        let mut world = synthetic_cluster();
        let before = world.non_air_count();
        let stairs = world.block_at(8, 1, 12);
        assert_eq!(apply_changes(&mut world, &changes), 0);
        assert_eq!(world.non_air_count(), before);
        assert_eq!(world.block_at(8, 1, 12), stairs);
        assert_eq!(world.block_at(9, 1, 13), Block::new(COAL_ORE, 0));
    }

    #[test]
    fn breaks_and_places_reload_over_the_demo() {
        let mut edited = synthetic_cluster();
        assert!(apply(&mut edited, Edit::Break { x: 9, y: 1, z: 13 }));
        assert!(apply(
            &mut edited,
            Edit::Place {
                x: 8,
                y: 2,
                z: 14,
                block: Block::new("cobblestone", 0),
                player: crate::edit::PlayerBox {
                    min: [40.0, 40.0, 40.0],
                    max: [40.6, 41.8, 40.6],
                },
            }
        ));
        let changes = [
            CellChange {
                x: 9,
                y: 1,
                z: 13,
                block: edited.block_at(9, 1, 13),
            },
            CellChange {
                x: 8,
                y: 2,
                z: 14,
                block: edited.block_at(8, 2, 14),
            },
        ];
        assert_eq!(changes[0].block, Block::new("stone", 0));
        let text = format_changes(&changes);
        assert!(text.starts_with("{\n  \"blocks\": [\n"));
        let parsed = parse_changes(&text).unwrap();
        assert_eq!(parsed, changes);

        let path: PathBuf =
            std::env::temp_dir().join(format!("rebuild-blocks-{}.json", std::process::id()));
        write_changes(&path, &parsed).unwrap();
        let loaded = read_changes(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded, parsed);

        let mut again = synthetic_cluster();
        assert_eq!(apply_changes(&mut again, &loaded), 2);
        assert_eq!(again.block_at(9, 1, 13), Block::new("stone", 0));
        assert_eq!(again.block_at(8, 2, 14), Block::new("cobblestone", 0));
        assert_eq!(again.block_at(10, 1, 13).id, "iron_ore");
        assert_eq!(again.block_at(8, 1, 12).id, "oak_stairs");
    }

    #[test]
    fn a_later_edit_of_the_same_cell_wins() {
        let text = r#"
        {
          "blocks": [
            { "x": 1, "y": 2, "z": 3, "id": "dirt", "metadata": 0 },
            { "x": 4, "y": 0, "z": -1, "id": "wool", "metadata": 4 },
            { "x": 1, "y": 2, "z": 3, "id": "air" }
          ]
        }
        "#;
        let changes = parse_changes(text).unwrap();
        assert_eq!(
            changes,
            vec![
                CellChange {
                    x: 1,
                    y: 2,
                    z: 3,
                    block: Block::AIR,
                },
                CellChange {
                    x: 4,
                    y: 0,
                    z: -1,
                    block: Block::new("wool", 4),
                },
            ]
        );
        let written = format_changes(&changes);
        assert!(written.find("\"z\": -1").unwrap() < written.find("\"z\": 3").unwrap());
        assert!(parse_changes("[]").is_err());
        assert!(parse_changes(r#"{"blocks":[{"x":0,"y":0,"z":0}]}"#).is_err());
        assert_eq!(
            parse_changes(
                r#"{"blocks":[{"x":0,"y":0,"z":0,"id":"copper_grate","metadata":0,"fill":1}]}"#
            )
            .unwrap()[0]
                .block
                .fill,
            1
        );
        assert!(
            parse_changes(r#"{"blocks":[{"x":0,"y":0,"z":0,"id":"stone","metadata":16}]}"#)
                .is_err()
        );
    }

    #[test]
    fn statistics_survive_a_later_block_edit() {
        let path: PathBuf =
            std::env::temp_dir().join(format!("rebuild-stats-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        assert!(read_statistics(&path).unwrap().is_empty());
        let stats = StatisticsSave {
            blocks_broken: 4,
            blocks_placed: 2,
            distance_walked: 12.5,
            mobs_killed: 1,
            time_played: 90.0,
            deaths: 0,
        };
        write_statistics(&path, &stats).unwrap();
        let edit = [CellChange {
            x: 3,
            y: 1,
            z: 5,
            block: Block::new("stone", 0),
        }];
        write_changes(&path, &edit).unwrap();
        assert_eq!(read_changes(&path).unwrap(), edit);
        let loaded = read_statistics(&path).unwrap();
        assert_eq!(loaded.blocks_broken, 4);
        assert_eq!(loaded.blocks_placed, 2);
        assert!((loaded.distance_walked - 12.5).abs() < 0.01);
        assert_eq!(loaded.mobs_killed, 1);
        assert!((loaded.time_played - 90.0).abs() < 0.01);
        write_advancements(&path, &["craft".into()]).unwrap();
        assert_eq!(read_statistics(&path).unwrap().blocks_broken, 4);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn advancements_survive_a_later_block_edit() {
        let path: PathBuf = std::env::temp_dir().join(format!(
            "rebuild-advancements-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        assert!(read_advancements(&path).unwrap().is_empty());
        write_advancements(&path, &["craft".into(), "kill".into()]).unwrap();
        let edit = [CellChange {
            x: 2,
            y: 1,
            z: 4,
            block: Block::new("dirt", 0),
        }];
        write_changes(&path, &edit).unwrap();
        assert_eq!(read_changes(&path).unwrap(), edit);
        assert_eq!(
            read_advancements(&path).unwrap(),
            vec!["craft".to_string(), "kill".to_string()]
        );
        write_hotbar(
            &path,
            &HotbarSave {
                selected: 0,
                slots: vec![],
            },
        )
        .unwrap();
        assert_eq!(
            read_advancements(&path).unwrap(),
            vec!["craft".to_string(), "kill".to_string()]
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn inventory_survives_a_later_block_edit() {
        let path: PathBuf =
            std::env::temp_dir().join(format!("rebuild-bag-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        assert!(read_inventory(&path).unwrap().is_empty());
        let slots = vec![
            CarriedSlot {
                slot: 2,
                id: "dirt".into(),
                count: 4,
            },
            CarriedSlot {
                slot: 8,
                id: "guide:page".into(),
                count: 1,
            },
        ];
        write_inventory(&path, &slots).unwrap();
        let edit = [CellChange {
            x: 1,
            y: 2,
            z: 3,
            block: Block::new("stone", 0),
        }];
        write_changes(&path, &edit).unwrap();
        assert_eq!(read_changes(&path).unwrap(), edit);
        assert_eq!(read_inventory(&path).unwrap(), slots);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"inventory\""));
        assert!(parse_inventory(&format_changes(&edit)).unwrap().is_empty());
        let wire = vec![
            (String::new(), 0),
            ("cobblestone".into(), 3),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
        ];
        let carried = carried_from_wire(&wire).unwrap();
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].slot, 1);
        assert_eq!(carried[0].count, 3);
        write_inventory(&path, &carried).unwrap();
        assert_eq!(read_changes(&path).unwrap(), edit);
        assert_eq!(read_inventory(&path).unwrap(), carried);
        let hotbar = HotbarSave {
            selected: 2,
            slots: vec![CarriedSlot {
                slot: 0,
                id: "cobblestone".into(),
                count: 64,
            }],
        };
        let place = PlayerPlace {
            x: 8.5,
            y: 5.12,
            z: 14.4,
            yaw: -1.57,
            pitch: -0.2,
        };
        write_hotbar(&path, &hotbar).unwrap();
        write_player(&path, place).unwrap();
        write_changes(&path, &edit).unwrap();
        assert_eq!(read_hotbar(&path).unwrap().as_ref(), Some(&hotbar));
        let loaded = read_player(&path).unwrap().unwrap();
        assert!((loaded.x - place.x).abs() < 1.0e-3);
        assert!((loaded.y - place.y).abs() < 1.0e-3);
        assert!((loaded.z - place.z).abs() < 1.0e-3);
        assert!((loaded.yaw - place.yaw).abs() < 1.0e-3);
        assert!((loaded.pitch - place.pitch).abs() < 1.0e-3);
        assert_eq!(read_inventory(&path).unwrap(), carried);
        ensure_chest(&path, 4, 1, 4, "guide:chest").unwrap();
        store_chest_slots(
            &path,
            4,
            1,
            4,
            "guide:chest",
            &[CarriedSlot {
                slot: 1,
                id: "dirt".into(),
                count: 2,
            }],
        )
        .unwrap();
        write_changes(&path, &edit).unwrap();
        let chests = read_chests(&path).unwrap();
        assert_eq!(chests.len(), 1);
        assert_eq!(chests[0].slots[0].count, 2);
        assert!((read_player(&path).unwrap().unwrap().x - place.x).abs() < 1.0e-3);
        remove_chest(&path, 4, 1, 4).unwrap();
        assert!(read_chests(&path).unwrap().is_empty());
        let apple = CarriedSlot {
            slot: 0,
            id: "apple".into(),
            count: 4,
        };
        store_ender_chest(&path, &[apple.clone()]).unwrap();
        assert_eq!(read_ender_chest(&path).unwrap(), vec![apple.clone()]);
        ensure_chest(&path, 1, 2, 3, "ender_chest").unwrap();
        ensure_chest(&path, 8, 2, 8, "ender_chest").unwrap();
        assert_eq!(read_ender_chest(&path).unwrap(), vec![apple]);
        let dirt = CarriedSlot {
            slot: 2,
            id: "dirt".into(),
            count: 1,
        };
        store_ender_chest(&path, &[dirt.clone()]).unwrap();
        assert_eq!(read_ender_chest(&path).unwrap(), vec![dirt.clone()]);
        remove_chest(&path, 1, 2, 3).unwrap();
        assert_eq!(read_ender_chest(&path).unwrap(), vec![dirt]);
        let stored = read_chests(&path).unwrap();
        assert_eq!(
            stored
                .iter()
                .filter(|chest| chest.id == "ender_chest")
                .count(),
            1
        );
        assert_eq!(stored[0].y, -1);
        let apple = CarriedSlot {
            slot: 0,
            id: "apple".into(),
            count: 3,
        };
        let dirt = CarriedSlot {
            slot: 1,
            id: "dirt".into(),
            count: 2,
        };
        store_chest_slots(&path, 2, 1, 2, "shulker_box", &[apple.clone()]).unwrap();
        store_chest_slots(&path, 5, 1, 5, "shulker_box", &[dirt.clone()]).unwrap();
        let boxes = read_chests(&path).unwrap();
        assert_eq!(
            boxes.iter().find(|chest| chest.x == 2).unwrap().slots,
            vec![apple]
        );
        assert_eq!(
            boxes.iter().find(|chest| chest.x == 5).unwrap().slots,
            vec![dirt.clone()]
        );
        remove_chest(&path, 2, 1, 2).unwrap();
        let left = read_chests(&path).unwrap();
        assert!(left.iter().all(|chest| chest.x != 2));
        assert_eq!(
            left.iter().find(|chest| chest.x == 5).unwrap().slots,
            vec![dirt]
        );
        let wire = vec![
            ("cobblestone".into(), 64),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
            (String::new(), 0),
        ];
        let from_wire = bar_from_wire(1, &wire).unwrap();
        assert_eq!(from_wire.selected, 0);
        assert_eq!(from_wire.slots[0].id, "cobblestone");
        assert!(
            parse_player(r#"{"blocks":[],"player":{"x":0,"y":0,"z":0,"yaw":0,"pitch":9}}"#)
                .is_err()
        );
        let _ = fs::remove_file(&path);
        assert!(
            parse_inventory(r#"{"blocks":[],"inventory":[{"slot":9,"id":"dirt","count":1}]}"#)
                .is_err()
        );
        assert!(
            parse_inventory(r#"{"blocks":[],"inventory":[{"slot":0,"id":"dirt","count":0}]}"#)
                .is_err()
        );
    }

    #[test]
    fn a_barrel_keeps_slots_apart_from_a_chest() {
        let path: PathBuf =
            std::env::temp_dir().join(format!("rebuild-barrel-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        let apple = CarriedSlot {
            slot: 0,
            id: "apple".into(),
            count: 2,
        };
        let dirt = CarriedSlot {
            slot: 1,
            id: "dirt".into(),
            count: 5,
        };
        store_chest_slots(&path, 3, 1, 3, "barrel", &[apple.clone()]).unwrap();
        store_chest_slots(&path, 6, 1, 6, "chest", &[dirt.clone()]).unwrap();
        let stored = read_chests(&path).unwrap();
        let barrel = stored.iter().find(|chest| chest.id == "barrel").unwrap();
        assert_eq!(barrel.slots, vec![apple]);
        assert_eq!([barrel.x, barrel.y, barrel.z], [3, 1, 3]);
        assert_eq!(
            stored
                .iter()
                .find(|chest| chest.id == "chest")
                .unwrap()
                .slots,
            vec![dirt]
        );
        remove_chest(&path, 3, 1, 3).unwrap();
        let left = read_chests(&path).unwrap();
        assert!(left.iter().all(|chest| chest.id != "barrel"));
        assert_eq!(left[0].id, "chest");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_copper_chest_keeps_its_own_slots() {
        let path: PathBuf =
            std::env::temp_dir().join(format!("rebuild-copper-chest-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        let apple = CarriedSlot {
            slot: 0,
            id: "apple".into(),
            count: 3,
        };
        let dirt = CarriedSlot {
            slot: 0,
            id: "dirt".into(),
            count: 1,
        };
        store_chest_slots(&path, 2, 1, 2, "copper_chest", &[apple.clone()]).unwrap();
        store_chest_slots(&path, 4, 1, 4, "copper_chest", &[dirt.clone()]).unwrap();
        store_chest_slots(&path, 6, 1, 6, "chest", &[]).unwrap();
        let stored = read_chests(&path).unwrap();
        let first = stored
            .iter()
            .find(|chest| chest.x == 2 && chest.id == "copper_chest")
            .unwrap();
        let second = stored
            .iter()
            .find(|chest| chest.x == 4 && chest.id == "copper_chest")
            .unwrap();
        assert_eq!(first.slots, vec![apple]);
        assert_eq!(second.slots, vec![dirt]);
        assert_ne!(first.slots, second.slots);
        let _ = fs::remove_file(&path);
    }
}
