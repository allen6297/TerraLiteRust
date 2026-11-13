//! Loot tables from the data pack (and optional RoseGold overrides).
//!
//! `data/terralite/loot/copper_chest.json` is what a trial key finds in a
//! copper chest. `data/terralite/loot/camp_chest.json` is what the first open
//! of the camp chest finds. `data/terralite/loot/shrine_chest.json` is the
//! southeast shrine. `data/terralite/loot/mine_chest.json` is the west mine
//! mouth. `data/terralite/loot/portal_chest.json` is the northwest ruined portal.
//! `data/terralite/loot/temple_chest.json` is the east desert temple.
//! `data/terralite/loot/shipwreck_chest.json` is the west beach shipwreck.
//! `data/terralite/loot/monument_chest.json` is the ocean monument remnant.
//! `data/terralite/loot/stronghold_chest.json` is the stronghold portal room.
//! `data/terralite/loot/stronghold_library_chest.json` is the stronghold library.
//! `data/terralite/loot/end_city_chest.json` is the End city remnant tower.
//! `data/terralite/loot/village_chest.json` is the north village remnant.
//! `data/terralite/loot/dungeon_chest.json` is the underground mine dungeon.
//! `data/terralite/loot/mineshaft_chest.json` is the abandoned mineshaft spur.
//! `data/terralite/loot/fortress_chest.json` is the nether fortress remnant.
//! `data/terralite/loot/bastion_chest.json` is the nether bastion remnant.
//! `data/terralite/loot/ancient_city_chest.json` is the deep-dark city hall.
//! `data/terralite/loot/blocks/<id>.json` overrides break drops for that
//! block (`drops` only; optional `rolls` when entries are weighted).
//! `loot.chest` / ModAPI `chestLoot` replaces a table's `drops` list with
//! equal-weight ids. Datapack entries may be plain strings or
//! `{ "id", "weight"?, "when"? }` objects. `when` accepts `omen` / `!omen`.
//! The chest's coordinates pick one weighted entry. Plain string lists keep
//! the old equal-weight `% len` behavior. Repeating an id still works as a
//! weight shortcut.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

/// One loot candidate. Plain JSON strings become `weight: 1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub weight: u32,
    pub when: When,
}

/// Simple v1 conditions. Biome/tool/fortune stay out of scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    Omen,
    NotOmen,
}

pub struct Table {
    pub drops: Vec<Entry>,
    pub omen: Vec<Entry>,
}

impl Entry {
    pub fn plain(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            weight: 1,
            when: When::Always,
        }
    }
}

impl Table {
    /// Legacy helper: drop ids only (ignores weight / when).
    pub fn drop_ids(&self) -> Vec<String> {
        self.drops.iter().map(|entry| entry.id.clone()).collect()
    }

    /// Legacy helper: omen ids only.
    pub fn omen_ids(&self) -> Vec<String> {
        self.omen.iter().map(|entry| entry.id.clone()).collect()
    }
}

fn loot_file(name: &str) -> PathBuf {
    rebuild_world::resolve_datapack_file(&format!("data/terralite/loot/{name}.json")).unwrap_or_else(
        || {
            rebuild_world::data_pack_root()
                .join("data")
                .join("terralite")
                .join("loot")
                .join(format!("{name}.json"))
        },
    )
}

pub fn copper_chest_file() -> PathBuf {
    loot_file("copper_chest")
}

/// The copper-chest table from the data pack.
pub fn load_copper_chest() -> Result<Table, String> {
    load_chest("copper_chest", &copper_chest_file())
}

pub fn camp_chest_file() -> PathBuf {
    loot_file("camp_chest")
}

/// The camp-chest table from the data pack.
pub fn load_camp_chest() -> Result<Table, String> {
    load_chest("camp_chest", &camp_chest_file())
}

pub fn shrine_chest_file() -> PathBuf {
    loot_file("shrine_chest")
}

/// The shrine-chest table from the data pack.
pub fn load_shrine_chest() -> Result<Table, String> {
    load_chest("shrine_chest", &shrine_chest_file())
}

pub fn mine_chest_file() -> PathBuf {
    loot_file("mine_chest")
}

/// The mine-chest table from the data pack.
pub fn load_mine_chest() -> Result<Table, String> {
    load_chest("mine_chest", &mine_chest_file())
}

pub fn portal_chest_file() -> PathBuf {
    loot_file("portal_chest")
}

/// The ruined-portal chest table from the data pack.
pub fn load_portal_chest() -> Result<Table, String> {
    load_chest("portal_chest", &portal_chest_file())
}

pub fn temple_chest_file() -> PathBuf {
    loot_file("temple_chest")
}

/// The desert-temple chest table from the data pack.
pub fn load_temple_chest() -> Result<Table, String> {
    load_chest("temple_chest", &temple_chest_file())
}

pub fn shipwreck_chest_file() -> PathBuf {
    loot_file("shipwreck_chest")
}

/// The shipwreck chest table from the data pack.
pub fn load_shipwreck_chest() -> Result<Table, String> {
    load_chest("shipwreck_chest", &shipwreck_chest_file())
}

pub fn monument_chest_file() -> PathBuf {
    loot_file("monument_chest")
}

/// The ocean-monument remnant chest table from the data pack.
pub fn load_monument_chest() -> Result<Table, String> {
    load_chest("monument_chest", &monument_chest_file())
}

pub fn stronghold_chest_file() -> PathBuf {
    loot_file("stronghold_chest")
}

/// The stronghold portal-room chest table from the data pack.
pub fn load_stronghold_chest() -> Result<Table, String> {
    load_chest("stronghold_chest", &stronghold_chest_file())
}

pub fn stronghold_library_chest_file() -> PathBuf {
    loot_file("stronghold_library_chest")
}

/// The stronghold library book-room chest table from the data pack.
pub fn load_stronghold_library_chest() -> Result<Table, String> {
    load_chest("stronghold_library_chest", &stronghold_library_chest_file())
}

pub fn end_city_chest_file() -> PathBuf {
    loot_file("end_city_chest")
}

/// The End city remnant tower chest table from the data pack.
pub fn load_end_city_chest() -> Result<Table, String> {
    load_chest("end_city_chest", &end_city_chest_file())
}

pub fn village_chest_file() -> PathBuf {
    loot_file("village_chest")
}

/// The village-remnant chest table from the data pack.
pub fn load_village_chest() -> Result<Table, String> {
    load_chest("village_chest", &village_chest_file())
}

pub fn dungeon_chest_file() -> PathBuf {
    loot_file("dungeon_chest")
}

/// The underground dungeon chest table from the data pack.
pub fn load_dungeon_chest() -> Result<Table, String> {
    load_chest("dungeon_chest", &dungeon_chest_file())
}

pub fn mineshaft_chest_file() -> PathBuf {
    loot_file("mineshaft_chest")
}

/// The abandoned mineshaft chest table from the data pack.
pub fn load_mineshaft_chest() -> Result<Table, String> {
    load_chest("mineshaft_chest", &mineshaft_chest_file())
}

pub fn fortress_chest_file() -> PathBuf {
    loot_file("fortress_chest")
}

/// The nether fortress remnant chest table from the data pack.
pub fn load_fortress_chest() -> Result<Table, String> {
    load_chest("fortress_chest", &fortress_chest_file())
}

pub fn bastion_chest_file() -> PathBuf {
    loot_file("bastion_chest")
}

/// The nether bastion remnant chest table from the data pack.
pub fn load_bastion_chest() -> Result<Table, String> {
    load_chest("bastion_chest", &bastion_chest_file())
}

pub fn ancient_city_chest_file() -> PathBuf {
    loot_file("ancient_city_chest")
}

/// The ancient city remnant chest table from the data pack.
pub fn load_ancient_city_chest() -> Result<Table, String> {
    load_chest("ancient_city_chest", &ancient_city_chest_file())
}

fn load_chest(table: &str, path: &std::path::Path) -> Result<Table, String> {
    let text = fs::read_to_string(path).map_err(|_| format!("unreadable {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("loot.json");
    let mut loaded = parse_named(&text, name)?;
    if let Some(drops) = rebuild_world::Pack::builtin().script_chest_loot(table) {
        loaded.drops = drops.into_iter().map(Entry::plain).collect();
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
/// Equal weights keep the old `n % len` mapping.
pub fn pick<'a>(table: &'a Table, x: i32, z: i32, omen: bool) -> &'a str {
    let list = if omen { &table.omen } else { &table.drops };
    let n = x
        .unsigned_abs()
        .wrapping_add(z.unsigned_abs().wrapping_mul(3));
    pick_weighted(list, n, omen)
}

/// Weighted pick from a filtered entry list. Shared with block break loot.
pub fn pick_weighted<'a>(entries: &'a [Entry], seed: u32, omen: bool) -> &'a str {
    let filtered: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.when.allows(omen))
        .collect();
    // Fall back so a mis-tagged list still pays something.
    let active: Vec<&Entry> = if filtered.is_empty() {
        entries.iter().collect()
    } else {
        filtered
    };
    if active.is_empty() {
        return "air";
    }
    let total: u64 = active.iter().map(|entry| u64::from(entry.weight.max(1))).sum();
    let total = total.max(1);
    let mut slot = u64::from(seed) % total;
    for entry in &active {
        let weight = u64::from(entry.weight.max(1));
        if slot < weight {
            return entry.id.as_str();
        }
        slot -= weight;
    }
    active[active.len() - 1].id.as_str()
}

impl When {
    fn allows(self, omen: bool) -> bool {
        match self {
            When::Always => true,
            When::Omen => omen,
            When::NotOmen => !omen,
        }
    }
}

fn read_list(file: &str, value: Option<&Value>, name: &str) -> Result<Vec<Entry>, String> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Err(format!("{file} {name} must be an array"));
    };
    if list.is_empty() || list.len() > 16 {
        return Err(format!("{file} {name} needs 1 to 16 ids"));
    }
    let mut items = Vec::with_capacity(list.len());
    for (index, entry) in list.iter().enumerate() {
        items.push(read_entry(file, name, index, entry)?);
    }
    Ok(items)
}

fn read_entry(file: &str, name: &str, index: usize, entry: &Value) -> Result<Entry, String> {
    let pack = rebuild_world::Pack::builtin();
    if let Some(id) = entry.as_str() {
        if !pack.contains_item(id) && pack.tool_icon(id).is_none() {
            return Err(format!("{file} {name} {id} is unknown"));
        }
        return Ok(Entry::plain(id));
    }
    let obj = entry
        .as_object()
        .ok_or_else(|| format!("{file} {name} {index} must be a string or object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "id" | "weight" | "when") {
            return Err(format!("{file} {name} {index} has unknown field {key}"));
        }
    }
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{file} {name} {index} needs id"))?;
    if !pack.contains_item(id) && pack.tool_icon(id).is_none() {
        return Err(format!("{file} {name} {id} is unknown"));
    }
    let weight = match obj.get("weight") {
        None => 1u32,
        Some(value) => {
            let Some(n) = value.as_u64() else {
                return Err(format!("{file} {name} {index} weight must be an integer"));
            };
            if n == 0 || n > 256 {
                return Err(format!("{file} {name} {index} weight needs 1 to 256"));
            }
            n as u32
        }
    };
    let when = match obj.get("when") {
        None => When::Always,
        Some(value) => read_when(file, name, index, value)?,
    };
    Ok(Entry {
        id: id.to_string(),
        weight,
        when,
    })
}

fn read_when(file: &str, name: &str, index: usize, value: &Value) -> Result<When, String> {
    let Some(tags) = value.as_array() else {
        return Err(format!("{file} {name} {index} when must be an array"));
    };
    if tags.is_empty() {
        return Ok(When::Always);
    }
    if tags.len() > 1 {
        return Err(format!("{file} {name} {index} when accepts one tag in v1"));
    }
    match tags[0].as_str() {
        Some("omen") => Ok(When::Omen),
        Some("!omen") => Ok(When::NotOmen),
        Some(other) => Err(format!("{file} {name} {index} unknown when {other}")),
        None => Err(format!("{file} {name} {index} when tag must be a string")),
    }
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
    fn weighted_entries_bias_the_cell_pick() {
        let table = parse_table(
            r#"{"drops":[{"id":"apple","weight":9},{"id":"iron_ore","weight":1}],"omen":[{"id":"mace","weight":1}]}"#,
        )
        .unwrap();
        assert_eq!(table.drops[0].weight, 9);
        assert_eq!(table.drops[1].weight, 1);
        // seed 0..8 → apple; seed 9 → iron_ore (total 10).
        assert_eq!(pick(&table, 0, 0, false), "apple");
        assert_eq!(pick(&table, 9, 0, false), "iron_ore");
        assert_eq!(pick(&table, 19, 0, false), "iron_ore");
        assert_eq!(pick(&table, 0, 0, true), "mace");
        assert!(parse_table(r#"{"drops":[{"id":"apple","weight":0}],"omen":["mace"]}"#).is_err());
        assert!(parse_table(r#"{"drops":[{"id":"apple","rolls":1}],"omen":["mace"]}"#).is_err());
    }

    #[test]
    fn entry_when_filters_omen_tags() {
        let table = parse_table(
            r#"{"drops":[{"id":"apple","when":["!omen"]},{"id":"mace","when":["omen"]}],"omen":["healing_potion"]}"#,
        )
        .unwrap();
        assert_eq!(pick(&table, 0, 0, false), "apple");
        // omen list still wins when the open is ominous.
        assert_eq!(pick(&table, 0, 0, true), "healing_potion");
        let unified = parse_table(
            r#"{"drops":[{"id":"apple","weight":1,"when":["!omen"]},{"id":"mace","weight":1,"when":["omen"]}],"omen":[{"id":"apple","weight":1,"when":["!omen"]},{"id":"mace","weight":1,"when":["omen"]}]}"#,
        )
        .unwrap();
        assert_eq!(pick(&unified, 0, 0, false), "apple");
        assert_eq!(pick(&unified, 0, 0, true), "mace");
    }

    #[test]
    fn the_sample_chest_file_is_what_a_key_pays() {
        let table = load_copper_chest().unwrap();
        assert_eq!(
            table.drop_ids(),
            [
                "iron_ore".to_string(),
                "wind_charge".to_string(),
                "breeze_rod".to_string(),
                "apple".to_string()
            ]
        );
        assert_eq!(
            table.omen_ids(),
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
            table.drop_ids(),
            [
                "raw".to_string(),
                "apple".to_string(),
                "wool".to_string(),
                "torch".to_string()
            ]
        );
        assert_eq!(
            table.omen_ids(),
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
            table.drop_ids(),
            [
                "book".to_string(),
                "clock".to_string(),
                "brush".to_string(),
                "map".to_string()
            ]
        );
        assert_eq!(
            table.omen_ids(),
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
    fn the_shipwreck_chest_pays_a_heart_of_the_sea() {
        let table = load_shipwreck_chest().unwrap();
        assert!(table.drops.iter().any(|entry| entry.id == "heart_of_the_sea"));
        assert!(table.drops.iter().any(|entry| entry.id == "nautilus_shell"));
        assert!(table.omen.iter().any(|entry| entry.id == "heart_of_the_sea"));
        let (x, z) = rebuild_world::SHIPWRECK_CHEST;
        assert_eq!(pick(&table, x, z, false), "heart_of_the_sea");
    }

    #[test]
    fn the_monument_chest_lists_prismarine_and_gold() {
        let table = load_monument_chest().unwrap();
        assert!(table.drops.iter().any(|entry| entry.id == "gold_block"));
        assert!(table.drops.iter().any(|entry| entry.id == "sponge"));
        assert!(table.drops.iter().any(|entry| entry.id == "prismarine_shard"));
        assert!(table.omen.iter().any(|entry| entry.id == "heart_of_the_sea"));
        let (x, z) = rebuild_world::OCEAN_MONUMENT_CHEST;
        assert_eq!(pick(&table, x, z, false), pick(&table, x, z, false));
    }

    #[test]
    fn the_stronghold_chest_lists_eyes_of_ender() {
        let table = load_stronghold_chest().unwrap();
        assert!(table.drops.iter().any(|entry| entry.id == "eye_of_ender"));
        assert!(table.drops.iter().any(|entry| entry.id == "ender_pearl"));
        assert!(table.omen.iter().any(|entry| entry.id == "eye_of_ender"));
        let (x, z) = rebuild_world::STRONGHOLD_CHEST;
        assert_eq!(pick(&table, x, z, false), pick(&table, x, z, false));
    }

    #[test]
    fn the_stronghold_library_chest_lists_books() {
        let table = load_stronghold_library_chest().unwrap();
        assert!(table.drops.iter().any(|entry| entry.id == "book"));
        assert!(table.drops.iter().any(|entry| entry.id == "bookshelf"));
        assert!(table.omen.iter().any(|entry| entry.id == "book"));
        let (x, z) = rebuild_world::STRONGHOLD_LIBRARY_CHEST;
        assert_eq!(pick(&table, x, z, false), pick(&table, x, z, false));
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
            table.drop_ids(),
            [
                "torch".to_string(),
                "coal_ore".to_string(),
                "iron_ore".to_string(),
                "stone_pickaxe".to_string(),
                "compass".to_string()
            ]
        );
        assert_eq!(
            table.omen_ids(),
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
