//! Loot tables from the data pack (and optional RoseGold overrides).
//!
//! `data/terralite/loot/copper_chest.json` is what a trial key finds in a
//! copper chest. `data/terralite/loot/camp_chest.json` is what the first open
//! of the camp chest finds. `data/terralite/loot/shrine_chest.json` is the
//! southeast shrine. `data/terralite/loot/mine_chest.json` is the west mine
//! mouth. `data/terralite/loot/blocks/<id>.json` overrides break drops for that
//! block (`drops` only). `loot.chest` / ModAPI `chestLoot` replaces a table's
//! `drops` list. `drops` is the ordinary list. `omen` is the list after an
//! ominous bottle. The chest's coordinates pick one entry. Repeating an id
//! makes that drop more common.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

pub struct Table {
    pub drops: Vec<String>,
    pub omen: Vec<String>,
}

pub fn copper_chest_file() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("loot")
        .join("copper_chest.json")
}

/// The copper-chest table from the data pack.
pub fn load_copper_chest() -> Result<Table, String> {
    load_chest("copper_chest", &copper_chest_file())
}

pub fn camp_chest_file() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("loot")
        .join("camp_chest.json")
}

/// The camp-chest table from the data pack.
pub fn load_camp_chest() -> Result<Table, String> {
    load_chest("camp_chest", &camp_chest_file())
}

pub fn shrine_chest_file() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("loot")
        .join("shrine_chest.json")
}

/// The shrine-chest table from the data pack.
pub fn load_shrine_chest() -> Result<Table, String> {
    load_chest("shrine_chest", &shrine_chest_file())
}

pub fn mine_chest_file() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("loot")
        .join("mine_chest.json")
}

/// The mine-chest table from the data pack.
pub fn load_mine_chest() -> Result<Table, String> {
    load_chest("mine_chest", &mine_chest_file())
}

fn load_chest(table: &str, path: &std::path::Path) -> Result<Table, String> {
    let text = fs::read_to_string(path).map_err(|_| format!("unreadable {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("loot.json");
    let mut loaded = parse_named(&text, name)?;
    if let Some(drops) = rebuild_world::Pack::builtin().script_chest_loot(table) {
        loaded.drops = drops;
    }
    Ok(loaded)
}

pub fn parse_table(text: &str) -> Result<Table, String> {
    parse_named(text, "copper_chest.json")
}

fn parse_named(text: &str, file: &str) -> Result<Table, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{file}: {err}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| format!("{file} must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "drops" | "omen") {
            return Err(format!("{file} has unknown field {key}"));
        }
    }
    let drops = read_list(file, obj.get("drops"), "drops")?;
    let omen = read_list(file, obj.get("omen"), "omen")?;
    Ok(Table { drops, omen })
}

/// One id from the table. The same cell always picks the same entry.
pub fn pick<'a>(table: &'a Table, x: i32, z: i32, omen: bool) -> &'a str {
    let list = if omen { &table.omen } else { &table.drops };
    let n = x
        .unsigned_abs()
        .wrapping_add(z.unsigned_abs().wrapping_mul(3));
    list[(n as usize) % list.len()].as_str()
}

fn read_list(file: &str, value: Option<&Value>, name: &str) -> Result<Vec<String>, String> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Err(format!("{file} {name} must be an array"));
    };
    if list.is_empty() || list.len() > 16 {
        return Err(format!("{file} {name} needs 1 to 16 ids"));
    }
    let mut items = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        let Some(id) = entry.as_str() else {
            return Err(format!("{file} {name} {index} must be a string"));
        };
        let pack = rebuild_world::Pack::builtin();
        if !pack.contains_item(id) && pack.tool_icon(id).is_none() {
            return Err(format!("{file} {name} {id} is unknown"));
        }
        items.push(id.to_string());
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_picks_by_cell_and_omen_uses_the_other_list() {
        let table = parse_table(r#"{"drops":["apple","iron_ore"],"omen":["mace"]}"#).unwrap();
        assert_eq!(pick(&table, 0, 0, false), "apple");
        assert_eq!(pick(&table, 1, 0, false), "iron_ore");
        assert_eq!(pick(&table, 0, 0, true), "mace");
        assert_eq!(pick(&table, 3, 5, false), pick(&table, 3, 5, false));
        assert!(parse_table(r#"{"drops":["apple"],"count":1}"#).is_err());
        assert!(parse_table(r#"{"drops":[],"omen":["apple"]}"#).is_err());
        assert!(parse_table(r#"{"drops":["not_a_real_item"],"omen":["apple"]}"#).is_err());
    }

    #[test]
    fn the_sample_chest_file_is_what_a_key_pays() {
        let table = load_copper_chest().unwrap();
        assert_eq!(
            table.drops,
            [
                "iron_ore".to_string(),
                "wind_charge".to_string(),
                "breeze_rod".to_string(),
                "apple".to_string()
            ]
        );
        assert_eq!(
            table.omen,
            [
                "heavy_core".to_string(),
                "mace".to_string(),
                "trial_key".to_string(),
                "healing_potion".to_string()
            ]
        );
        assert_eq!(pick(&table, 0, 0, false), "iron_ore");
        assert_eq!(pick(&table, 0, 0, true), "heavy_core");
    }

    #[test]
    fn the_camp_chest_file_is_what_the_first_open_pays() {
        let table = load_camp_chest().unwrap();
        assert_eq!(
            table.drops,
            [
                "raw".to_string(),
                "apple".to_string(),
                "wool".to_string(),
                "torch".to_string()
            ]
        );
        assert_eq!(
            table.omen,
            [
                "healing_potion".to_string(),
                "iron_pickaxe".to_string(),
                "compass".to_string(),
                "map".to_string()
            ]
        );
        let (x, z) = rebuild_world::CAMP_CHEST;
        assert_eq!(pick(&table, x, z, false), "raw");
        assert_eq!(pick(&table, x, z, true), "healing_potion");
    }

    #[test]
    fn the_shrine_chest_file_is_what_the_first_open_pays() {
        let table = load_shrine_chest().unwrap();
        assert_eq!(
            table.drops,
            [
                "book".to_string(),
                "clock".to_string(),
                "brush".to_string(),
                "map".to_string()
            ]
        );
        assert_eq!(
            table.omen,
            [
                "spyglass".to_string(),
                "goat_horn".to_string(),
                "compass".to_string(),
                "healing_potion".to_string()
            ]
        );
        let (x, z) = rebuild_world::SHRINE_CHEST;
        assert_eq!(pick(&table, x, z, false), pick(&table, x, z, false));
        assert_eq!(pick(&table, x, z, true), pick(&table, x, z, true));
    }

    #[test]
    fn a_script_chest_loot_overrides_datapack_drops() {
        let pack = rebuild_world::Pack::builtin();
        pack.set_chest_loot("demo_chest", &["torch", "map"], "loot-test")
            .unwrap();
        assert_eq!(
            pack.script_chest_loot("demo_chest").as_deref(),
            Some(&["torch".to_string(), "map".to_string()][..])
        );
        // Same source may rewrite the list.
        pack.set_chest_loot("demo_chest", &["apple"], "loot-test")
            .unwrap();
        assert_eq!(
            pack.script_chest_loot("demo_chest").as_deref(),
            Some(&["apple".to_string()][..])
        );
    }

    #[test]
    fn the_mine_chest_file_is_what_the_first_open_pays() {
        let table = load_mine_chest().unwrap();
        assert_eq!(
            table.drops,
            [
                "torch".to_string(),
                "coal_ore".to_string(),
                "iron_ore".to_string(),
                "stone_pickaxe".to_string()
            ]
        );
        assert_eq!(
            table.omen,
            [
                "iron_pickaxe".to_string(),
                "diamond_pickaxe".to_string(),
                "healing_potion".to_string(),
                "clock".to_string()
            ]
        );
        let (x, z) = rebuild_world::SHAFT_CHEST;
        assert_eq!(pick(&table, x, z, false), pick(&table, x, z, false));
        assert_eq!(pick(&table, x, z, true), pick(&table, x, z, true));
    }
}
