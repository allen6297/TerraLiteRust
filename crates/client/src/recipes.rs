//! Recipe book loaded from the data pack.
//!
//! Each file under `data/terralite/recipes` names one result and the items that
//! make it. The Items screen lists those results and their ingredients.
//! `data/terralite/smelting` registers furnace / smoker / blast cooks.

use std::fs;
use std::path::PathBuf;

use rebuild_world::HeatStation;
use serde_json::Value;

pub fn recipes_dir() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("recipes")
}

pub fn smelting_dir() -> PathBuf {
    rebuild_world::data_pack_root()
        .join("data")
        .join("terralite")
        .join("smelting")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    result: String,
    ingredients: Vec<String>,
    /// Nine cells when the shape matters. `None` is a pile.
    pattern: Option<Vec<String>>,
}

impl Recipe {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn result(&self) -> &str {
        &self.result
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ingredients(&self) -> &[String] {
        &self.ingredients
    }
}

#[derive(Clone, Debug)]
struct HeatLine {
    result: String,
    input: String,
    station: HeatStation,
}

#[derive(Clone, Debug)]
pub struct Book {
    recipes: Vec<Recipe>,
    heats: Vec<HeatLine>,
}

impl Book {
    pub fn empty() -> Self {
        Self {
            recipes: Vec::new(),
            heats: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty() && self.heats.is_empty()
    }

    /// Results and ingredients, sorted, each id once.
    pub fn entries(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for recipe in &self.recipes {
            ids.push(recipe.result.clone());
            ids.extend(recipe.ingredients.iter().cloned());
        }
        ids.sort();
        ids.dedup();
        ids
    }

    /// How `id` is made, or what it is used to make.
    pub fn detail(&self, id: &str) -> String {
        if let Some(recipe) = self.recipes.iter().find(|recipe| recipe.result == id) {
            let inputs = recipe
                .ingredients
                .iter()
                .map(|item| display_name(item))
                .collect::<Vec<_>>()
                .join(" and ");
            let verb = if recipe.ingredients.len() == 1 {
                "makes"
            } else {
                "make"
            };
            return format!("{inputs} {verb} {}", display_name(&recipe.result));
        }
        let mut uses = Vec::new();
        for recipe in &self.recipes {
            if recipe.ingredients.iter().any(|item| item == id) {
                uses.push(display_name(&recipe.result));
            }
        }
        uses.sort();
        uses.dedup();
        if uses.is_empty() {
            "no recipe".to_string()
        } else {
            format!("used to make {}", uses.join(" and "))
        }
    }

    pub fn from_recipes(recipes: Vec<Recipe>) -> Result<Self, String> {
        let mut seen = Vec::new();
        for recipe in &recipes {
            if seen.iter().any(|id: &String| id == &recipe.result) {
                return Err(format!("duplicate recipe {}", recipe.result));
            }
            seen.push(recipe.result.clone());
        }
        Ok(Self {
            recipes,
            heats: Vec::new(),
        })
    }

    /// Furnace / smoker / blast cook. An existing input for that station stays.
    pub fn add_heat(&mut self, result: String, input: String, station: HeatStation) {
        if result.is_empty()
            || input.is_empty()
            || self
                .heats
                .iter()
                .any(|line| line.input == input && line.station == station)
        {
            return;
        }
        self.heats.push(HeatLine {
            result,
            input,
            station,
        });
    }

    fn heat_of(&self, input: &str, station: HeatStation) -> Option<String> {
        self.heats
            .iter()
            .find(|line| line.input == input && line.station == station)
            .map(|line| line.result.clone())
    }

    /// Add a mod recipe. An existing result stays as it was.
    pub fn add_line(&mut self, result: String, ingredients: Vec<String>) {
        if ingredients.is_empty() || self.recipes.iter().any(|recipe| recipe.result == result) {
            return;
        }
        self.recipes.push(Recipe {
            result,
            ingredients,
            pattern: None,
        });
    }

    /// A shaped mod recipe. `pattern` is nine cells, blanks empty.
    ///
    /// The shape can sit in any corner of the crafting grid. An existing result stays.
    pub fn add_shaped(&mut self, result: String, pattern: Vec<String>) {
        if pattern.len() != 9
            || pattern.iter().all(|cell| cell.is_empty())
            || self.recipes.iter().any(|recipe| recipe.result == result)
        {
            return;
        }
        let ingredients = pattern
            .iter()
            .filter(|cell| !cell.is_empty())
            .cloned()
            .collect();
        self.recipes.push(Recipe {
            result,
            ingredients,
            pattern: Some(pattern),
        });
    }

    /// The recipe whose ingredients are exactly `filled`, in any order.
    ///
    /// The first match in the book wins. One oak log is oak stairs, because
    /// that file is read before the sign.
    pub fn match_grid(&self, filled: &[String]) -> Option<String> {
        let mut have = filled.to_vec();
        have.sort();
        self.recipes.iter().find_map(|recipe| {
            if recipe.pattern.is_some() {
                return None;
            }
            let mut need = recipe.ingredients.clone();
            need.sort();
            (have == need).then(|| recipe.result.clone())
        })
    }

    /// The crafting table. A shape matches anywhere on the 3 by 3. A pile ignores order.
    pub fn match_slots(&self, slots: &[String]) -> Option<String> {
        if slots.len() == 9 {
            for recipe in &self.recipes {
                if let Some(pattern) = &recipe.pattern {
                    if pattern_fits(pattern, slots) {
                        return Some(recipe.result.clone());
                    }
                }
            }
        }
        let filled: Vec<String> = slots.iter().filter(|id| !id.is_empty()).cloned().collect();
        self.match_grid(&filled)
    }

    /// A one-ingredient stone or bricks line also cooks in the furnace.
    /// Stone cooks into smooth stone, stone bricks into cracked stone bricks, cobbled deepslate into polished deepslate, deepslate tiles into cracked deepslate tiles, blackstone bricks into cracked blackstone bricks, basalt or polished basalt into smooth basalt, nether bricks into cracked nether bricks, and netherrack into nether bricks, when those blocks are in the book.
    /// Registered `recipe.smelt` / datapack smelting lines win first.
    pub fn smelt(&self, input: &str) -> Option<String> {
        if let Some(result) = self.heat_of(input, HeatStation::Furnace) {
            return Some(result);
        }
        if input == "stone"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "smooth_stone")
        {
            return Some("smooth_stone".to_string());
        }
        if input == "stone_bricks"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "cracked_stone_bricks")
        {
            return Some("cracked_stone_bricks".to_string());
        }
        if input == "cobbled_deepslate"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "polished_deepslate")
        {
            return Some("polished_deepslate".to_string());
        }
        if input == "deepslate_tiles"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "cracked_deepslate_tiles")
        {
            return Some("cracked_deepslate_tiles".to_string());
        }
        if input == "blackstone_bricks"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "cracked_blackstone_bricks")
        {
            return Some("cracked_blackstone_bricks".to_string());
        }
        if (input == "basalt" || input == "polished_basalt")
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "smooth_basalt")
        {
            return Some("smooth_basalt".to_string());
        }
        if input == "nether_bricks"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "cracked_nether_bricks")
        {
            return Some("cracked_nether_bricks".to_string());
        }
        if input == "raw" {
            return Some("cooked".to_string());
        }
        if input == "netherrack"
            && self
                .recipes
                .iter()
                .any(|recipe| recipe.result == "nether_bricks")
        {
            return Some("nether_bricks".to_string());
        }
        self.recipes.iter().find_map(|recipe| {
            let cooks = recipe.result == "stone" || recipe.result == "bricks";
            (cooks && recipe.ingredients.len() == 1 && recipe.ingredients[0] == input)
                .then(|| recipe.result.clone())
        })
    }

    /// Food the smoker finishes. Stone and brick lines stay on the furnace.
    pub fn smoke(&self, input: &str) -> Option<String> {
        if let Some(result) = self.heat_of(input, HeatStation::Smoker) {
            return Some(result);
        }
        let result = self.smelt(input)?;
        (result == "cooked").then_some(result)
    }

    /// Ore and stone the blast furnace finishes. Food and clay stay elsewhere.
    pub fn blast(&self, input: &str) -> Option<String> {
        if let Some(result) = self.heat_of(input, HeatStation::Blast) {
            return Some(result);
        }
        let result = self.smelt(input)?;
        (result != "cooked" && result != "bricks").then_some(result)
    }

    /// Ingredients that craft `id`, when `id` is a result.
    pub fn inputs(&self, id: &str) -> Option<Vec<String>> {
        self.recipes
            .iter()
            .find(|recipe| recipe.result == id)
            .map(|recipe| recipe.ingredients.clone())
    }

    /// Nine-cell shape for `id`, when the recipe is shaped.
    pub fn pattern_of(&self, id: &str) -> Option<&[String]> {
        self.recipes
            .iter()
            .find(|recipe| recipe.result == id)
            .and_then(|recipe| recipe.pattern.as_deref())
    }

    /// Results that list `id` as an ingredient (EMI USES panel).
    pub fn uses_of(&self, id: &str) -> Vec<String> {
        let mut uses = Vec::new();
        for recipe in &self.recipes {
            if recipe.ingredients.iter().any(|item| item == id) {
                uses.push(recipe.result.clone());
            }
        }
        uses.sort();
        uses.dedup();
        uses
    }
}

pub fn load_book() -> Result<Book, String> {
    let root = rebuild_world::data_pack_root();
    if !root.join("pack.meta").is_file() {
        return Err(format!("data pack missing pack.meta ({})", root.display()));
    }
    let listed = rebuild_world::list_datapack_json("recipes");
    if listed.is_empty() {
        return Err(format!("data pack has no recipes in {}", recipes_dir().display()));
    }
    let mut recipes = Vec::with_capacity(listed.len());
    for (id, path) in listed {
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        let recipe = parse_recipe(&id, &text)?;
        if recipe.result != id {
            return Err(format!(
                "{} result is {}, expected {id}",
                path.display(),
                recipe.result
            ));
        }
        recipes.push(recipe);
    }
    Book::from_recipes(recipes)
}

/// Data-pack recipes plus `recipe.register` lines from loaded mods.
pub fn load_emi() -> Result<Book, String> {
    let mut book = load_book()?;
    load_smelting_into(&mut book)?;
    let pack = rebuild_world::Pack::builtin();
    let patterns = pack.script_patterns();
    for (result, ingredients) in pack.script_recipe_lines() {
        if let Some((_, pattern)) = patterns.iter().find(|(id, _)| id == &result) {
            book.add_shaped(result, pattern.clone());
        } else {
            book.add_line(result, ingredients);
        }
    }
    for (result, input, station) in pack.script_smelt_lines() {
        book.add_heat(result, input, station);
    }
    Ok(book)
}

fn load_smelting_into(book: &mut Book) -> Result<(), String> {
    for (name, path) in rebuild_world::list_datapack_json("smelting") {
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        let (result, input, station) = parse_smelting(&name, &text)?;
        book.add_heat(result, input, station);
    }
    Ok(())
}

fn parse_smelting(id: &str, text: &str) -> Result<(String, String, HeatStation), String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}.json: {err}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| format!("{id}.json must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "result" | "ingredient" | "station") {
            return Err(format!("{id}.json has unknown field {key}"));
        }
    }
    let pack = rebuild_world::Pack::builtin();
    let known = |item: &str| pack.contains_item(item) || pack.tool_icon(item).is_some();
    let result = match obj.get("result").and_then(Value::as_str) {
        Some(result) if known(result) => result.to_string(),
        Some(result) => return Err(format!("{id}.json result {result} is unknown")),
        None => return Err(format!("{id}.json needs a result")),
    };
    let input = match obj.get("ingredient").and_then(Value::as_str) {
        Some(input) if known(input) => input.to_string(),
        Some(input) => return Err(format!("{id}.json ingredient {input} is unknown")),
        None => return Err(format!("{id}.json needs an ingredient")),
    };
    let station = match obj.get("station").and_then(Value::as_str).unwrap_or("furnace") {
        "furnace" => HeatStation::Furnace,
        "smoker" => HeatStation::Smoker,
        "blast" | "blast_furnace" => HeatStation::Blast,
        other => return Err(format!("{id}.json station {other} is unknown")),
    };
    Ok((result, input, station))
}

pub fn parse_recipe(id: &str, text: &str) -> Result<Recipe, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}.json: {err}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| format!("{id}.json must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "result" | "ingredients") {
            return Err(format!("{id}.json has unknown field {key}"));
        }
    }
    let result = match obj.get("result").and_then(Value::as_str) {
        Some(result) if valid_item(result) => result.to_string(),
        Some(_) => return Err(format!("{id}.json result must be a block id")),
        None => return Err(format!("{id}.json needs a result")),
    };
    let ingredients = obj
        .get("ingredients")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{id}.json ingredients must be an array"))?;
    if ingredients.is_empty() || ingredients.len() > 4 {
        return Err(format!("{id}.json needs 1 to 4 ingredients"));
    }
    let mut items = Vec::with_capacity(ingredients.len());
    for (index, ingredient) in ingredients.iter().enumerate() {
        let Some(item) = ingredient.as_str() else {
            return Err(format!("{id}.json ingredient {index} must be a string"));
        };
        if !valid_item(item) {
            return Err(format!("{id}.json ingredient {item} is a bad id"));
        }
        if item == result {
            return Err(format!("{id}.json result {result} is also an ingredient"));
        }
        items.push(item.to_string());
    }
    Ok(Recipe {
        result,
        ingredients: items,
        pattern: None,
    })
}

/// The pattern's filled box, shifted, matches `slots`. Cells outside that box stay empty.
fn pattern_fits(pattern: &[String], slots: &[String]) -> bool {
    if pattern.len() != 9 || slots.len() != 9 {
        return false;
    }
    let mut min_row = 3;
    let mut min_col = 3;
    let mut max_row = 0;
    let mut max_col = 0;
    let mut any = false;
    for index in 0..9 {
        if pattern[index].is_empty() {
            continue;
        }
        any = true;
        let row = index / 3;
        let col = index % 3;
        min_row = min_row.min(row);
        min_col = min_col.min(col);
        max_row = max_row.max(row);
        max_col = max_col.max(col);
    }
    if !any {
        return false;
    }
    let height = max_row - min_row;
    let width = max_col - min_col;
    for row in 0..=(2 - height) {
        for col in 0..=(2 - width) {
            if (0..9).all(|index| {
                let grid_row = (index / 3) as i32;
                let grid_col = (index % 3) as i32;
                let pattern_row = grid_row + min_row as i32 - row as i32;
                let pattern_col = grid_col + min_col as i32 - col as i32;
                let expect = if (min_row as i32..=max_row as i32).contains(&pattern_row)
                    && (min_col as i32..=max_col as i32).contains(&pattern_col)
                {
                    pattern[pattern_row as usize * 3 + pattern_col as usize].as_str()
                } else {
                    ""
                };
                slots[index] == expect
            }) {
                return true;
            }
        }
    }
    false
}

pub fn display_name(id: &str) -> String {
    let name = id.rsplit(':').next().unwrap_or(id);
    name.replace('_', " ")
}

fn valid_item(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    id.len() <= 32 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_pack_recipes_feed_the_book() {
        let dir = recipes_dir();
        assert!(dir.ends_with(std::path::Path::new("data/terralite/recipes")));
        assert!(rebuild_world::data_pack_root().join("pack.meta").is_file());
        let book = load_book().unwrap();
        assert_eq!(
            book.entries(),
            vec![
                "anvil".to_string(),
                "apple".to_string(),
                "arrow".to_string(),
                "barrel".to_string(),
                "basalt".to_string(),
                "battery".to_string(),
                "beacon".to_string(),
                "bed".to_string(),
                "beehive".to_string(),

                "bell".to_string(),
                "blackstone".to_string(),
                "blackstone_bricks".to_string(),
                "blast_furnace".to_string(),
                "blaze_powder".to_string(),
                "blaze_rod".to_string(),
                "boat".to_string(),
                "book".to_string(),
                "bookshelf".to_string(),
                "bow".to_string(),
                "bread".to_string(),
                "breeze_rod".to_string(),
                "brewing_stand".to_string(),
                "bricks".to_string(),
                "brush".to_string(),
                "bucket".to_string(),
                "button".to_string(),
                "cactus".to_string(),
                "cake".to_string(),
                "campfire".to_string(),
                "candle".to_string(),
                "carpet".to_string(),
                "cartography_table".to_string(),
                "cauldron".to_string(),
                "chain".to_string(),
                "chest".to_string(),
                "chiseled_blackstone".to_string(),
                "chiseled_bookshelf".to_string(),
                "chiseled_nether_bricks".to_string(),
                "chiseled_stone_bricks".to_string(),
                "chorus_flower".to_string(),
                "chorus_fruit".to_string(),
                "chorus_plant".to_string(),
                "clay".to_string(),
                "clock".to_string(),
                "coal_ore".to_string(),
                "cobbled_deepslate".to_string(),
                "cobblestone".to_string(),
                "cobweb".to_string(),
                "compass".to_string(),
                "composter".to_string(),
                "conduit".to_string(),
                "copper_bulb".to_string(),
                "copper_block".to_string(),
                "honeycomb".to_string(),

                "copper_chest".to_string(),
                "copper_door".to_string(),
                "copper_grate".to_string(),
                "copper_trapdoor".to_string(),
                "cracked_blackstone_bricks".to_string(),
                "cracked_deepslate_tiles".to_string(),
                "cracked_nether_bricks".to_string(),
                "cracked_stone_bricks".to_string(),
                "crying_obsidian".to_string(),
                "dark_prismarine".to_string(),
                "decorated_pot".to_string(),
                "deepslate".to_string(),
                "deepslate_bricks".to_string(),
                "deepslate_tiles".to_string(),
                "diamond_axe".to_string(),
                "diamond_block".to_string(),
                "diamond_boots".to_string(),
                "diamond_chestplate".to_string(),
                "diamond_helmet".to_string(),
                "diamond_hoe".to_string(),
                "diamond_pickaxe".to_string(),
                "diamond_shovel".to_string(),
                "diamond_sword".to_string(),
                "dirt".to_string(),
                "dispenser".to_string(),
                "door".to_string(),
                "dragon_egg".to_string(),
                "drain".to_string(),
                "dropper".to_string(),
                "emerald".to_string(),
                "emerald_block".to_string(),
                "enchanting_table".to_string(),

                "end_crystal".to_string(),

                "end_portal_frame".to_string(),
                "end_rod".to_string(),
                "end_stone".to_string(),
                "end_stone_bricks".to_string(),
                "ender_chest".to_string(),
                "ender_pearl".to_string(),
                "eye_of_ender".to_string(),
                "fence".to_string(),
                "fence_gate".to_string(),
                "fire_charge".to_string(),
                "fishing_rod".to_string(),

                "fletching_table".to_string(),
                "flower".to_string(),
                "flower_pot".to_string(),
                "generator".to_string(),
                "gilded_blackstone".to_string(),
                "glass".to_string(),
                "glowstone".to_string(),
                "goat_horn".to_string(),
                "gold_block".to_string(),
                "gold_ingot".to_string(),
                "golden_boots".to_string(),
                "golden_chestplate".to_string(),
                "golden_helmet".to_string(),
                "grass".to_string(),
                "gravel".to_string(),
                "grindstone".to_string(),
                "hanging_sign".to_string(),
                "hay_bale".to_string(),
                "healing_potion".to_string(),
                "heart_of_the_sea".to_string(),
                "heavy_core".to_string(),
                "hopper".to_string(),
                "ice".to_string(),
                "iron_block".to_string(),

                "iron_ore".to_string(),
                "iron_pickaxe".to_string(),

                "item_frame".to_string(),
                "jukebox".to_string(),
                "ladder".to_string(),
                "lamp".to_string(),
                "lantern".to_string(),
                "lead".to_string(),
                "leather_boots".to_string(),
                "leather_cap".to_string(),
                "leather_tunic".to_string(),
                "lectern".to_string(),
                "lever".to_string(),
                "lightning_rod".to_string(),
                "lily_pad".to_string(),
                "lodestone".to_string(),
                "loom".to_string(),
                "mace".to_string(),
                "magma".to_string(),
                "magma_cream".to_string(),
                "map".to_string(),
                "melon".to_string(),
                "minecart".to_string(),
                "mossy_cobblestone".to_string(),
                "mossy_stone_bricks".to_string(),
                "music_disc".to_string(),
                "nautilus_shell".to_string(),
                "nether_bricks".to_string(),
                "nether_star".to_string(),

                "nether_wart_block".to_string(),
                "netherite_axe".to_string(),
                "netherite_boots".to_string(),
                "netherite_chestplate".to_string(),
                "netherite_helmet".to_string(),
                "netherite_hoe".to_string(),
                "netherite_ingot".to_string(),
                "netherite_pickaxe".to_string(),
                "netherite_scrap".to_string(),
                "netherite_shovel".to_string(),
                "netherite_sword".to_string(),
                "netherrack".to_string(),
                "note_block".to_string(),
                "oak_log".to_string(),
                "oak_stairs".to_string(),
                "obsidian".to_string(),
                "ominous_bottle".to_string(),

                "painting".to_string(),
                "paper".to_string(),
                "pipe".to_string(),
                "piston".to_string(),
                "plate".to_string(),
                "polished_basalt".to_string(),
                "polished_blackstone".to_string(),
                "polished_deepslate".to_string(),
                "powered_rail".to_string(),
                "prismarine".to_string(),
                "prismarine_bricks".to_string(),
                "pump".to_string(),
                "pumpkin".to_string(),
                "purpur".to_string(),
                "purpur_pillar".to_string(),
                "quartz".to_string(),
                "rail".to_string(),
                "red_nether_bricks".to_string(),
                "red_sandstone".to_string(),
                "redstone_dust".to_string(),
                "redstone_repeater".to_string(),
                "redstone_comparator".to_string(),
                "observer".to_string(),
                "redstone_torch".to_string(),
                "respawn_anchor".to_string(),
                "saddle".to_string(),
                "sand".to_string(),
                "sandstone".to_string(),
                "sea_lantern".to_string(),
                "shelf".to_string(),
                "shield".to_string(),
                "shulker_box".to_string(),
                "shulker_shell".to_string(),
                "sign".to_string(),
                "slab".to_string(),
                "slimeball".to_string(),
                "smithing_table".to_string(),
                "smoker".to_string(),
                "smooth_basalt".to_string(),
                "smooth_stone".to_string(),
                "snow".to_string(),
                "soul_sand".to_string(),
                "soul_soil".to_string(),
                "spout".to_string(),
                "spyglass".to_string(),
                "stick".to_string(),
                "sticky_piston".to_string(),
                "stone".to_string(),
                "stone_bricks".to_string(),
                "stone_pickaxe".to_string(),
                "stonecutter".to_string(),
                "sugar_cane".to_string(),
                "tank".to_string(),
                "target".to_string(),
                "torch".to_string(),
                "trapdoor".to_string(),
                "trial_key".to_string(),
                "trial_spawner".to_string(),
                "tripwire".to_string(),
                "vine".to_string(),
                "warped_fungus_on_a_stick".to_string(),
                "water".to_string(),
                "wheat".to_string(),
                "wheat_seeds".to_string(),
                "wind_charge".to_string(),
                "wire".to_string(),
                "wooden_hoe".to_string(),
                "wooden_pickaxe".to_string(),
                "wool".to_string(),
            ]
        );
        assert_eq!(book.detail("oak_stairs"), "oak log makes oak stairs");
        assert_eq!(book.detail("stone"), "cobblestone makes stone");
        assert_eq!(book.detail("lantern"), "iron ore and oak log make lantern");
        assert_eq!(
            book.detail("cobblestone"),
            "used to make compass and dispenser and dropper and mossy cobblestone and piston and saddle and shield and smooth stone and stone and stone pickaxe"
        );
        assert_eq!(
            book.match_grid(&["wheat".into(), "wheat".into(), "wheat".into()]),
            Some("bread".to_string())
        );
        assert_eq!(book.detail("bread"), "wheat and wheat and wheat make bread");
        assert_eq!(book.detail("wheat"), "used to make bread");
        assert_eq!(
            book.match_grid(&["oak_log".into(), "iron_ore".into()]),
            Some("lantern".to_string())
        );
        assert_eq!(
            book.match_grid(&["oak_log".into()]),
            Some("oak_stairs".to_string())
        );
        assert_eq!(
            book.match_grid(&["deepslate".into()]).as_deref(),
            Some("deepslate_bricks")
        );
        assert_eq!(
            book.match_grid(&["deepslate".into(), "deepslate".into()])
                .as_deref(),
            Some("cobbled_deepslate")
        );
        assert_eq!(
            book.match_grid(&["stone_bricks".into()]).as_deref(),
            Some("cracked_stone_bricks")
        );
        assert_eq!(
            book.match_grid(&["stone_bricks".into(), "stone_bricks".into()])
                .as_deref(),
            Some("chiseled_stone_bricks")
        );
        assert_eq!(
            book.match_grid(&["bookshelf".into(), "book".into()])
                .as_deref(),
            Some("chiseled_bookshelf")
        );
        assert_eq!(
            book.match_grid(&["blackstone_bricks".into(), "blackstone_bricks".into()])
                .as_deref(),
            Some("chiseled_blackstone")
        );
        assert_eq!(
            book.match_grid(&["blackstone".into(), "iron_ore".into()])
                .as_deref(),
            Some("gilded_blackstone")
        );
        assert_eq!(
            book.match_grid(&["blackstone".into(), "deepslate".into()])
                .as_deref(),
            Some("basalt")
        );
        assert_eq!(
            book.match_grid(&["blackstone".into(), "clay".into()])
                .as_deref(),
            Some("nether_bricks")
        );
        assert_eq!(
            book.match_grid(&["nether_bricks".into()]).as_deref(),
            Some("cracked_nether_bricks")
        );
        assert_eq!(
            book.match_grid(&["nether_bricks".into(), "flower".into()])
                .as_deref(),
            Some("red_nether_bricks")
        );
        assert_eq!(
            book.match_grid(&["nether_bricks".into(), "nether_bricks".into()])
                .as_deref(),
            Some("chiseled_nether_bricks")
        );
        assert_eq!(
            book.match_grid(&["nether_bricks".into(), "flower".into(), "wool".into()])
                .as_deref(),
            Some("nether_wart_block")
        );
        assert_eq!(
            book.match_grid(&["sand".into(), "nether_wart_block".into()])
                .as_deref(),
            Some("soul_sand")
        );
        assert_eq!(
            book.match_grid(&["nether_bricks".into(), "glowstone".into()])
                .as_deref(),
            Some("magma")
        );
        assert_eq!(
            book.match_grid(&["blackstone".into(), "magma".into()])
                .as_deref(),
            Some("netherrack")
        );
        assert_eq!(book.smelt("netherrack").as_deref(), Some("nether_bricks"));
        assert_eq!(book.smelt("raw").as_deref(), Some("cooked"));
        assert_eq!(
            book.match_grid(&["quartz".into(), "sandstone".into()])
                .as_deref(),
            Some("end_stone")
        );
        assert_eq!(
            book.match_grid(&["end_stone".into()]).as_deref(),
            Some("end_stone_bricks")
        );
        assert_eq!(
            book.match_grid(&["end_stone_bricks".into(), "purpur".into()])
                .as_deref(),
            Some("end_portal_frame")
        );
        assert_eq!(
            book.match_grid(&["end_portal_frame".into(), "eye_of_ender".into()])
                .as_deref(),
            Some("dragon_egg")
        );
        assert_eq!(
            book.match_grid(&["chest".into(), "eye_of_ender".into()])
                .as_deref(),
            Some("ender_chest")
        );
        assert_eq!(
            book.match_grid(&["chest".into(), "shulker_shell".into()])
                .as_deref(),
            Some("shulker_box")
        );
        assert_eq!(
            book.match_grid(&["chest".into(), "slab".into()]).as_deref(),
            Some("barrel")
        );
        assert_eq!(
            book.match_grid(&["stone".into(), "quartz".into()])
                .as_deref(),
            Some("stonecutter")
        );
        assert_eq!(
            book.match_grid(&["hay_bale".into(), "coal_ore".into()])
                .as_deref(),
            Some("campfire")
        );
        assert_eq!(
            book.match_grid(&["gravel".into(), "hay_bale".into()])
                .as_deref(),
            Some("fletching_table")
        );
        assert_eq!(
            book.match_grid(&["campfire".into(), "netherrack".into()])
                .as_deref(),
            Some("smoker")
        );
        assert_eq!(book.smoke("raw").as_deref(), Some("cooked"));
        assert!(book.smoke("cobblestone").is_none());
        assert!(book.smoke("stone").is_none());
        assert!(book.smoke("netherrack").is_none());
        assert_eq!(
            book.match_grid(&["smoker".into(), "stone".into()])
                .as_deref(),
            Some("blast_furnace")
        );
        assert_eq!(book.blast("cobblestone").as_deref(), Some("stone"));
        assert_eq!(book.blast("stone").as_deref(), Some("smooth_stone"));
        assert!(book.blast("raw").is_none());
        assert!(book.blast("clay").is_none());
        assert_eq!(
            book.match_grid(&["flower".into(), "hay_bale".into()])
                .as_deref(),
            Some("beehive")
        );
        assert_eq!(
            book.match_grid(&["bucket".into(), "stone".into()])
                .as_deref(),
            Some("cauldron")
        );
        assert_eq!(
            book.match_grid(&["cauldron".into(), "stick".into()])
                .as_deref(),
            Some("bell")
        );
        assert_eq!(
            book.match_grid(&["campfire".into(), "stick".into()])
                .as_deref(),
            Some("candle")
        );
        assert_eq!(
            book.match_grid(&["bell".into(), "candle".into()])
                .as_deref(),
            Some("chain")
        );
        assert_eq!(
            book.match_grid(&["chain".into(), "compass".into()])
                .as_deref(),
            Some("lodestone")
        );
        assert_eq!(
            book.match_grid(&[
                "crying_obsidian".into(),
                "crying_obsidian".into(),
                "glowstone".into()
            ])
            .as_deref(),
            Some("respawn_anchor")
        );
        assert_eq!(
            book.match_grid(&["chain".into(), "respawn_anchor".into()])
                .as_deref(),
            Some("lightning_rod")
        );
        assert_eq!(
            book.match_grid(&["hay_bale".into(), "red_nether_bricks".into()])
                .as_deref(),
            Some("target")
        );
        assert_eq!(
            book.match_grid(&["lightning_rod".into(), "glowstone".into()])
                .as_deref(),
            Some("copper_bulb")
        );
        assert_eq!(
            book.match_grid(&["copper_bulb".into(), "chain".into()])
                .as_deref(),
            Some("copper_grate")
        );
        assert_eq!(
            book.match_grid(&["copper_grate".into(), "door".into()])
                .as_deref(),
            Some("copper_door")
        );
        assert_eq!(
            book.match_grid(&["chest".into(), "copper_grate".into()])
                .as_deref(),
            Some("copper_chest")
        );
        assert_eq!(
            book.match_grid(&["breeze_rod".into(), "copper_chest".into()])
                .as_deref(),
            Some("trial_key")
        );
        assert_eq!(
            book.match_grid(&["copper_chest".into(), "ominous_bottle".into()])
                .as_deref(),
            Some("trial_spawner")
        );
        assert_eq!(
            book.match_grid(&["healing_potion".into(), "trial_key".into()])
                .as_deref(),
            Some("ominous_bottle")
        );
        assert_eq!(
            book.match_grid(&["copper_door".into(), "trapdoor".into()])
                .as_deref(),
            Some("copper_trapdoor")
        );
        assert_eq!(
            book.match_grid(&["bricks".into(), "clay".into()])
                .as_deref(),
            Some("decorated_pot")
        );
        assert_eq!(
            book.match_grid(&["oak_log".into(), "slab".into()])
                .as_deref(),
            Some("shelf")
        );
        assert_eq!(
            book.match_grid(&["iron_ore".into(), "iron_ore".into()])
                .as_deref(),
            Some("anvil")
        );
        assert_eq!(
            book.match_grid(&["bookshelf".into(), "sea_lantern".into()])
                .as_deref(),
            Some("enchanting_table")
        );
        assert_eq!(
            book.match_grid(&["nether_wart_block".into(), "iron_ore".into()])
                .as_deref(),
            Some("brewing_stand")
        );
        assert_eq!(
            book.match_grid(&["stone".into(), "stick".into()])
                .as_deref(),
            Some("grindstone")
        );
        assert_eq!(
            book.match_grid(&["copper_grate".into(), "glass".into()])
                .as_deref(),
            Some("spyglass")
        );
        assert_eq!(
            book.match_grid(&["wool".into(), "gravel".into()])
                .as_deref(),
            Some("brush")
        );
        assert_eq!(
            book.match_grid(&["note_block".into(), "stick".into()])
                .as_deref(),
            Some("goat_horn")
        );
        assert_eq!(
            book.match_grid(&["deepslate".into(), "iron_ore".into()])
                .as_deref(),
            Some("mace")
        );
        assert_eq!(
            book.match_grid(&["breeze_rod".into(), "deepslate".into()])
                .as_deref(),
            Some("heavy_core")
        );
        assert_eq!(
            book.match_grid(&["stick".into(), "chorus_fruit".into()])
                .as_deref(),
            Some("breeze_rod")
        );
        assert_eq!(
            book.match_grid(&["breeze_rod".into(), "wool".into()])
                .as_deref(),
            Some("wind_charge")
        );
        assert_eq!(
            book.match_grid(&["blaze_rod".into()]).as_deref(),
            Some("blaze_powder")
        );
        assert_eq!(
            book.match_grid(&["blaze_powder".into(), "coal_ore".into()])
                .as_deref(),
            Some("fire_charge")
        );
        assert_eq!(
            book.match_grid(&["painting".into(), "chain".into()])
                .as_deref(),
            Some("item_frame")
        );
        assert_eq!(
            book.match_grid(&["sign".into(), "chain".into()]).as_deref(),
            Some("hanging_sign")
        );
        assert_eq!(
            book.match_grid(&["anvil".into(), "iron_ore".into()])
                .as_deref(),
            Some("smithing_table")
        );
        assert_eq!(
            book.match_grid(&["painting".into(), "stick".into()])
                .as_deref(),
            Some("loom")
        );
        assert_eq!(
            book.match_grid(&["map".into(), "stick".into()]).as_deref(),
            Some("cartography_table")
        );
        assert_eq!(
            book.match_grid(&["bookshelf".into(), "wool".into()])
                .as_deref(),
            Some("book")
        );
        assert_eq!(
            book.match_grid(&["bookshelf".into(), "stick".into()])
                .as_deref(),
            Some("lectern")
        );
        assert_eq!(
            book.match_grid(&["slab".into(), "fence".into()]).as_deref(),
            Some("composter")
        );
        assert_eq!(
            book.match_grid(&["dragon_egg".into(), "glowstone".into()])
                .as_deref(),
            Some("end_crystal")
        );
        assert_eq!(
            book.match_grid(&["end_stone".into(), "purpur".into()])
                .as_deref(),
            Some("chorus_plant")
        );
        assert_eq!(
            book.match_grid(&["chorus_plant".into(), "flower".into()])
                .as_deref(),
            Some("chorus_flower")
        );
        assert_eq!(
            book.match_grid(&["chorus_plant".into(), "chorus_flower".into()])
                .as_deref(),
            Some("chorus_fruit")
        );
        assert_eq!(
            book.match_grid(&["end_stone".into(), "chorus_fruit".into()])
                .as_deref(),
            Some("ender_pearl")
        );
        assert_eq!(
            book.match_grid(&["gold_block".into()]).as_deref(),
            Some("gold_ingot")
        );
        assert_eq!(
            book.match_grid(&["netherite_scrap".into(), "gold_ingot".into()])
                .as_deref(),
            Some("netherite_ingot")
        );
        assert_eq!(
            book.match_grid(&["diamond_helmet".into(), "netherite_ingot".into()])
                .as_deref(),
            Some("netherite_helmet")
        );
        assert_eq!(
            book.match_grid(&["diamond_sword".into(), "netherite_ingot".into()])
                .as_deref(),
            Some("netherite_sword")
        );
        assert_eq!(
            book.match_grid(&["diamond_pickaxe".into(), "netherite_ingot".into()])
                .as_deref(),
            Some("netherite_pickaxe")
        );
        assert_eq!(
            book.match_grid(&["ender_pearl".into(), "glowstone".into()])
                .as_deref(),
            Some("eye_of_ender")
        );
        assert_eq!(
            book.match_grid(&["end_stone".into(), "flower".into()])
                .as_deref(),
            Some("purpur")
        );
        assert_eq!(
            book.match_grid(&["purpur".into()]).as_deref(),
            Some("purpur_pillar")
        );
        assert_eq!(
            book.match_grid(&["purpur".into(), "glowstone".into()])
                .as_deref(),
            Some("end_rod")
        );
        assert_eq!(
            book.smelt("nether_bricks").as_deref(),
            Some("cracked_nether_bricks")
        );
        assert_eq!(
            book.match_grid(&["basalt".into()]).as_deref(),
            Some("polished_basalt")
        );
        assert_eq!(
            book.match_grid(&["polished_basalt".into()]).as_deref(),
            Some("smooth_basalt")
        );
        assert_eq!(book.smelt("basalt").as_deref(), Some("smooth_basalt"));
        assert_eq!(
            book.smelt("polished_basalt").as_deref(),
            Some("smooth_basalt")
        );
        assert_eq!(
            book.detail("chiseled_stone_bricks"),
            "stone bricks and stone bricks make chiseled stone bricks"
        );
        assert_eq!(book.smelt("cobblestone").as_deref(), Some("stone"));
        assert_eq!(book.smelt("stone").as_deref(), Some("smooth_stone"));
        assert_eq!(
            book.smelt("stone_bricks").as_deref(),
            Some("cracked_stone_bricks")
        );
        assert_eq!(
            book.smelt("cobbled_deepslate").as_deref(),
            Some("polished_deepslate")
        );
        assert_eq!(
            book.match_grid(&["cobbled_deepslate".into()]).as_deref(),
            Some("polished_deepslate")
        );
        assert_eq!(
            book.match_grid(&["polished_deepslate".into()]).as_deref(),
            Some("deepslate_tiles")
        );
        assert_eq!(
            book.match_grid(&["blackstone".into()]).as_deref(),
            Some("polished_blackstone")
        );
        assert_eq!(
            book.match_grid(&["polished_blackstone".into()]).as_deref(),
            Some("blackstone_bricks")
        );
        assert_eq!(
            book.match_grid(&["deepslate_tiles".into()]).as_deref(),
            Some("cracked_deepslate_tiles")
        );
        assert_eq!(
            book.smelt("deepslate_tiles").as_deref(),
            Some("cracked_deepslate_tiles")
        );
        assert_eq!(
            book.match_grid(&["blackstone_bricks".into()]).as_deref(),
            Some("cracked_blackstone_bricks")
        );
        assert_eq!(
            book.smelt("blackstone_bricks").as_deref(),
            Some("cracked_blackstone_bricks")
        );
        assert_eq!(book.smelt("clay").as_deref(), Some("bricks"));
        assert_eq!(book.smelt("oak_log"), None);
        assert_eq!(
            book.match_grid(&["stick".into(), "oak_log".into()]),
            Some("wooden_pickaxe".to_string())
        );
        assert_eq!(
            book.detail("oak_log"),
            "used to make bed and boat and bookshelf and door and jukebox and lantern and leather tunic and minecart and note block and oak stairs and piston and shelf and shield and sign and stick and wooden pickaxe"
        );
        assert_eq!(
            book.match_grid(&["stick".into(), "tripwire".into()])
                .as_deref(),
            Some("fishing_rod")
        );
        assert!(parse_recipe("stone", r#"{"result":"stone"}"#).is_err());
        assert!(parse_recipe("stone", r#"{"result":"stone","ingredients":["stone"]}"#).is_err());
        assert!(
            parse_recipe(
                "stone",
                r#"{"result":"stone","ingredients":["cobblestone"],"count":1}"#
            )
            .is_err()
        );
        let stairs = parse_recipe(
            "oak_stairs",
            r#"{"result":"oak_stairs","ingredients":["oak_log"]}"#,
        )
        .unwrap();
        assert_eq!(stairs.result(), "oak_stairs");
        assert_eq!(stairs.ingredients(), &["oak_log".to_string()]);
        let again = parse_recipe(
            "oak_stairs",
            r#"{"result":"oak_stairs","ingredients":["oak_log"]}"#,
        )
        .unwrap();
        assert!(Book::from_recipes(vec![stairs, again]).is_err());
    }

    #[test]
    fn a_script_recipe_shows_in_the_book() {
        let pack = rebuild_world::Pack::builtin();
        let _ = pack.register_item("meshkit", "petal");
        pack.add_script_recipe("meshkit:petal", "stone", "meshkit-recipe")
            .unwrap();
        let book = load_emi().unwrap();
        assert!(book.entries().iter().any(|id| id == "meshkit:petal"));
        assert_eq!(book.detail("meshkit:petal"), "stone makes petal");
        let mut shaped = Book::empty();
        shaped.add_line(
            "rosegold:bouquet".into(),
            vec!["rosegold:rose".into(), "rosegold:rose".into()],
        );
        shaped.add_shaped(
            "rosegold:rose_post".into(),
            vec![
                "rosegold:rose".into(),
                String::new(),
                String::new(),
                "rosegold:rose".into(),
                String::new(),
                String::new(),
                "rosegold:rose".into(),
                String::new(),
                String::new(),
            ],
        );
        let mut column = vec![String::new(); 9];
        column[0] = "rosegold:rose".into();
        column[3] = "rosegold:rose".into();
        column[6] = "rosegold:rose".into();
        assert_eq!(
            shaped.match_slots(&column).as_deref(),
            Some("rosegold:rose_post")
        );
        let mut shifted = vec![String::new(); 9];
        shifted[1] = "rosegold:rose".into();
        shifted[4] = "rosegold:rose".into();
        shifted[7] = "rosegold:rose".into();
        assert_eq!(
            shaped.match_slots(&shifted).as_deref(),
            Some("rosegold:rose_post")
        );
        let mut across = vec![String::new(); 9];
        across[0] = "rosegold:rose".into();
        across[1] = "rosegold:rose".into();
        across[2] = "rosegold:rose".into();
        assert!(shaped.match_slots(&across).is_none());
        let mut pile = vec![String::new(); 9];
        pile[2] = "rosegold:rose".into();
        pile[8] = "rosegold:rose".into();
        assert_eq!(
            shaped.match_slots(&pile).as_deref(),
            Some("rosegold:bouquet")
        );
        assert_eq!(
            book.inputs("stone").as_deref(),
            Some(&["cobblestone".to_string()][..])
        );
        assert!(book.inputs("cobblestone").is_none());
        assert_eq!(book.detail("stone"), "cobblestone makes stone");
        assert!(book.uses_of("cobblestone").iter().any(|id| id == "stone"));
        assert!(book.pattern_of("stone").is_none());
    }

    #[test]
    fn smelting_files_and_script_heats_cook_in_the_book() {
        let book = load_emi().unwrap();
        assert_eq!(book.smelt("sand").as_deref(), Some("glass"));
        assert_eq!(book.blast("sand").as_deref(), Some("glass"));
        assert_eq!(book.blast("ancient_debris").as_deref(), Some("netherite_scrap"));
        assert!(book.smoke("sand").is_none());
        let pack = rebuild_world::Pack::builtin();
        pack.set_smelt_recipe(
            "torch",
            "dirt",
            rebuild_world::HeatStation::Furnace,
            "smelt-test",
        )
        .unwrap();
        let cooked = load_emi().unwrap();
        assert_eq!(cooked.smelt("dirt").as_deref(), Some("torch"));
        pack.set_smelt_recipe(
            "apple",
            "wheat",
            rebuild_world::HeatStation::Smoker,
            "smelt-test",
        )
        .unwrap();
        let smoked = load_emi().unwrap();
        assert_eq!(smoked.smoke("wheat").as_deref(), Some("apple"));
        assert!(smoked.smelt("wheat").is_none());
    }
}
