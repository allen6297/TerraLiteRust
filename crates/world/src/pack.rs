//! Per-block models register shapes and blocks by string id.
//!
//! Each block is a `.json` or `.bbmodel` under
//! `assets/terralite/models/block/<id>`. Named shapes used by metadata variants
//! (door open, wall torch, script default `cube`, …) live under
//! `assets/terralite/models/shapes/`. Place variants (slab, stairs, plate) are
//! separate per-block models; the client Alt picker maps a block family onto
//! those ids. A block model can name a PNG (`texture`) and a colored light
//! (`light`). Hardness, and the block a break leaves behind, live in the data
//! pack. Those registries are what the world and the mesher look up. An id that
//! was not registered has no shape, so it does not mesh. A texture path with no
//! file is skipped. Dropping a model file under `models/block/` is enough for
//! that id to mesh.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::chunk::{Block, FILL_EMBER};
use crate::registry::Registry;
use crate::shape::{CellBox, MeshFace, Plane, Shape};
use serde_json::Value;

/// Folder of per-block models inside the resource pack.
pub const BLOCK_MODELS_DIR: &str = "assets/terralite/models/block";

/// Folder of named shapes (door_open, torch_west, cube, …).
pub const SHAPE_MODELS_DIR: &str = "assets/terralite/models/shapes";

/// Resource pack the client reads. `pack.json` sits at this root.
pub const RESOURCE_PACK_ROOT: &str = r"C:\CODE\TerraLite\resourcepacks\terralite";

pub fn resource_pack_root() -> &'static Path {
    Path::new(RESOURCE_PACK_ROOT)
}

/// Per-block models directory under the resource pack.
pub fn block_definitions_file() -> PathBuf {
    block_models_dir()
}

pub fn block_models_dir() -> PathBuf {
    join_pack_parts(BLOCK_MODELS_DIR)
}

pub fn shape_models_dir() -> PathBuf {
    join_pack_parts(SHAPE_MODELS_DIR)
}

fn join_pack_parts(relative: &str) -> PathBuf {
    let mut path = resource_pack_root().to_path_buf();
    for part in relative.split('/') {
        path.push(part);
    }
    path
}

/// JSON or other text from the resource pack, then from each `mods/<name>/`.
pub fn asset_text(path: &str) -> Option<String> {
    read_text_asset(path)
}

/// PNG bytes for a path inside the resource pack.
///
/// Enabled overlays win first (topmost enabled), then the base pack, then
/// each `mods/<name>/`. A missing file, a path that leaves the pack, or a
/// pack with no `pack.json` is `None`.
pub fn asset_png(path: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = crate::resource_packs::overlay_bytes(path) {
        return Some(bytes);
    }
    if let Some(bytes) = read_root(resource_pack_root(), path, true) {
        return Some(bytes);
    }
    let mods = Path::new(r"C:\CODE\TerraLite\mods");
    let entries = fs::read_dir(mods).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir.is_dir() {
            if let Some(bytes) = read_root(&dir, path, false) {
                return Some(bytes);
            }
        }
    }
    None
}

fn read_root(root: &Path, path: &str, require_meta: bool) -> Option<Vec<u8>> {
    if require_meta && !root.join("pack.json").is_file() {
        return None;
    }
    let file = join_inside(root, path)?;
    let bytes = fs::read(file).ok()?;
    if bytes.is_empty() { None } else { Some(bytes) }
}

/// Data pack the server reads. `pack.meta` sits at this root.
pub const DATA_PACK_ROOT: &str = r"C:\CODE\TerraLite\datapacks\terralite";

pub fn data_pack_root() -> &'static Path {
    Path::new(DATA_PACK_ROOT)
}

/// Read the data pack and apply it onto a fresh load of the resource pack's
/// per-block models.
///
/// A missing `pack.json`, a bad block file, or a block id the pack does not
/// register is an error. The returned pack is dropped; [`Pack::builtin`] keeps
/// the one the game uses.
pub fn load_data_pack() -> Result<(), String> {
    load_builtin_pack().map(|_| ())
}

/// Join `path` onto the resource pack when it stays inside that folder.
fn pack_file(path: &str) -> Option<PathBuf> {
    if !resource_pack_root().join("pack.json").is_file() {
        return None;
    }
    join_inside(resource_pack_root(), path)
}

/// Join `path` onto `root` when every part stays inside that folder.
fn join_inside(root: &Path, path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains('\\') {
        return None;
    }
    let mut out = root.to_path_buf();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// A colored light. `level` is `1..=15`. `color` channels are `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub color: [f32; 3],
    pub level: u8,
}

impl Light {
    /// Emission per channel, attenuated from `level` by the color. `0..=15`.
    pub fn channels(self) -> [u8; 3] {
        let mut out = [0; 3];
        for channel in 0..3 {
            let scaled = (self.color[channel] * f32::from(self.level)).round();
            out[channel] = scaled.clamp(0.0, 15.0) as u8;
        }
        out
    }
}

pub struct BlockEntry {
    geometry: Geometry,
    /// Block left behind on break. `None` means the cell becomes air.
    replaces: Option<String>,
    light: Option<Light>,
    texture: Option<String>,
    /// Seconds of holding a break before this block gives way.
    hardness: Option<f32>,
}

impl BlockEntry {
    pub fn geometry_id(&self) -> &str {
        self.geometry.id()
    }

    pub fn uses_model(&self) -> bool {
        matches!(self.geometry, Geometry::Model(_))
    }

    pub fn replaces(&self) -> Option<&str> {
        self.replaces.as_deref()
    }

    pub fn light(&self) -> Option<Light> {
        self.light
    }

    /// Path inside the resource pack, when this block names a PNG.
    pub fn texture(&self) -> Option<&str> {
        self.texture.as_deref()
    }

    /// Seconds of holding a break before this block gives way.
    pub fn hardness(&self) -> Option<f32> {
        self.hardness
    }
}

enum Geometry {
    Shape(String),
    Model(String),
}

impl Geometry {
    fn id(&self) -> &str {
        match self {
            Geometry::Shape(id) | Geometry::Model(id) => id,
        }
    }
}

/// Minecraft-style item `display.gui` (degrees / sixteenths / unit scale).
///
/// Inventory and creative slots project the block/item model with this
/// transform — the same contract authors set in Blockbench / models JSON.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDisplay {
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

impl ItemDisplay {
    /// Vanilla `block/block` gui defaults: isometric inventory cube.
    pub const GUI_BLOCK: Self = Self {
        rotation: [30.0, 225.0, 0.0],
        translation: [0.0, 0.0, 0.0],
        scale: [0.625, 0.625, 0.625],
    };

    /// Flat `item/generated` gui (no tilt).
    pub const GUI_ITEM: Self = Self {
        rotation: [0.0, 0.0, 0.0],
        translation: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
    };
}

pub struct ShapeEntry {
    boxes: Vec<CellBox>,
    planes: Vec<Plane>,
    /// Tilted quads from a mesh element. Not solid.
    faces: Vec<MeshFace>,
    /// PNG paths named by a Blockbench model, indexed by the face's texture id.
    textures: Vec<String>,
    /// Inventory / creative projection for this model.
    gui: ItemDisplay,
}

impl ShapeEntry {
    pub fn shape(&self) -> Shape<'_> {
        Shape::new(&self.boxes, &self.planes)
            .with_faces(&self.faces)
            .with_textures(&self.textures)
    }

    pub fn textures(&self) -> &[String] {
        &self.textures
    }

    /// `display.gui` for this model (Blockbench / MC JSON, or block default).
    pub fn gui_display(&self) -> ItemDisplay {
        self.gui
    }
}

struct ScriptBlock {
    id: String,
    hardness: f32,
    /// Script path that registered this id. The same file can load again.
    source: String,
    /// Pack PNG, or a leaked path string. `None` meshes as a flat cube.
    texture: Option<&'static str>,
    /// Shape or model id. `None` uses the cube shape.
    model: Option<String>,
    /// Colored light from `block.light`. `None` emits nothing.
    light: Option<Light>,
}

struct ScriptItem {
    id: String,
    source: String,
    /// Block id a hotbar place of this item becomes. `None` does not place.
    places: Option<String>,
    /// Hotbar stack limit from `item.stack`. `None` means 64.
    max_count: Option<u16>,
}

struct ScriptRecipe {
    result: String,
    ingredients: Vec<String>,
    /// Nine cells, row by row. `None` is a pile, so the grid order does not matter.
    pattern: Option<Vec<String>>,
    source: String,
}

/// Break or chest loot from `loot.block` / `loot.chest`.
struct ScriptLoot {
    /// Block id or chest table id (`camp_chest`, …).
    id: String,
    drops: Vec<String>,
    source: String,
}

/// Heat cook from `recipe.smelt` / `smoke` / `blast` or datapack smelting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeatStation {
    Furnace,
    Smoker,
    Blast,
}

struct ScriptSmelt {
    result: String,
    input: String,
    station: HeatStation,
    source: String,
}

struct ScriptScreen {
    id: String,
    source: String,
}

struct StoredGui {
    screen: rebuild_rosegold::GuiScreen,
    source: String,
}

struct StoredSetting {
    option: rebuild_rosegold::SettingOption,
    source: String,
}

struct ScriptEntity {
    id: String,
    source: String,
    /// Blockbench `.bbmodel` path. Empty keeps the pink Guest boxes.
    model: Option<String>,
    /// Chase/attack when the client Guest AI runs.
    hostile: bool,
    /// Creative item id that spawns this mob on use.
    spawn_egg: Option<String>,
}

struct ScriptSpawn {
    id: String,
    x: i32,
    y: i32,
    z: i32,
    source: String,
}

struct ScriptRegistry {
    blocks: Vec<ScriptBlock>,
    items: Vec<ScriptItem>,
    recipes: Vec<ScriptRecipe>,
    /// Furnace / smoker / blast lines from mods or datapack smelting.
    smelts: Vec<ScriptSmelt>,
    /// Break drops from `loot.block`.
    block_loots: Vec<ScriptLoot>,
    /// Chest `drops` overrides from `loot.chest`.
    chest_loots: Vec<ScriptLoot>,
    screens: Vec<ScriptScreen>,
    gui: Vec<StoredGui>,
    settings: Vec<StoredSetting>,
    /// `namespace:path` from `BLOCK_ENTITY.register` / `block_entity.bind`.
    /// Marks a block id as a script tile (key/value data). Not a chest UI.
    block_entities: Vec<String>,
    /// Per-cell string fields for script block entities: `(x,y,z) → key → value`.
    tiles: HashMap<(i32, i32, i32), HashMap<String, String>>,
    /// Mobs from `entity.register` and `ENTITY` registry entries.
    entities: Vec<ScriptEntity>,
    /// Bodies from `entity.spawn` that `entity.despawn` did not take back.
    spawns: Vec<ScriptSpawn>,
    /// Blockbench models loaded from a pack or a mod folder. The entry is leaked.
    models: Vec<(&'static str, &'static ShapeEntry)>,
}

struct ToolDef {
    id: String,
    tier: u8,
    icon: String,
    uses: u16,
}

struct FoodDef {
    id: String,
    restore: i32,
    icon: String,
}

struct ArmorDef {
    id: String,
    slot: u8,
    soak: i32,
    icon: String,
}

struct ShieldDef {
    id: String,
    uses: u16,
    icon: String,
}

struct BoatDef {
    id: String,
    icon: String,
}

struct MinecartDef {
    id: String,
    icon: String,
}

struct LeadDef {
    id: String,
    icon: String,
}

struct CompassDef {
    id: String,
    icon: String,
}

struct SpyglassDef {
    id: String,
    icon: String,
}

struct BrushDef {
    id: String,
    uses: u16,
    icon: String,
}

struct HornDef {
    id: String,
    icon: String,
}

struct MaceDef {
    id: String,
    uses: u16,
    smash: i32,
    icon: String,
}

struct BucketDef {
    id: String,
    /// `None` is empty. `Some("water")` pours a water block.
    fills: Option<String>,
    icon: String,
}

/// Places a compound fill (water or ember) into a host block.
struct FillDef {
    id: String,
    /// `water` or `ember`.
    kind: String,
    icon: String,
}

/// Removes a compound fill from a host block.
struct DrainDef {
    id: String,
    icon: String,
}

struct SaddleDef {
    id: String,
    icon: String,
}

struct ClockDef {
    id: String,
    icon: String,
}

struct MapDef {
    id: String,
    icon: String,
}

struct BookDef {
    id: String,
    icon: String,
}

struct ModelDef {
    id: String,
    shape: ShapeEntry,
}

struct FertilizerDef {
    id: String,
    icon: String,
}

struct ArrowDef {
    id: String,
    icon: String,
}

struct BowDef {
    id: String,
    icon: String,
}

struct CrossbowDef {
    id: String,
    icon: String,
}

struct TotemDef {
    id: String,
    icon: String,
}

struct FireworkDef {
    id: String,
    icon: String,
}

struct SplashDef {
    id: String,
    icon: String,
    /// `"heal"` restores the thrower; `"harm"` hurts nearby mobs.
    effect: String,
}

struct PearlDef {
    id: String,
    icon: String,
}

struct WindDef {
    id: String,
    icon: String,
}

struct RodDef {
    id: String,
    icon: String,
}

struct FishingDef {
    id: String,
    icon: String,
}

struct SeedsDef {
    id: String,
    icon: String,
}

struct HoeDef {
    id: String,
    uses: u16,
    icon: String,
}

struct CoreDef {
    id: String,
    icon: String,
}

struct KeyDef {
    id: String,
    icon: String,
}

struct OmenDef {
    id: String,
    icon: String,
}

struct EyeDef {
    id: String,
    icon: String,
}

pub struct Pack {
    shapes: Registry<ShapeEntry>,
    models: Registry<ShapeEntry>,
    blocks: Registry<BlockEntry>,
    /// Pickaxes from the data pack. They are items, not blocks.
    tools: Vec<ToolDef>,
    /// Food from the data pack. Eating one restores `restore` hunger.
    foods: Vec<FoodDef>,
    /// Armor from the data pack. `slot` is `0` cap, `1` tunic, `2` boots.
    armors: Vec<ArmorDef>,
    /// Shields from the data pack. Holding one blocks zombie bites.
    shields: Vec<ShieldDef>,
    /// Boats from the data pack. They place a hull on water.
    boats: Vec<BoatDef>,
    /// Minecarts from the data pack. They place a hull on rails.
    minecarts: Vec<MinecartDef>,
    /// Leads from the data pack. One hitches a tamed horse to a fence.
    leads: Vec<LeadDef>,
    /// Compasses from the data pack. Holding one points at the stairs.
    compasses: Vec<CompassDef>,
    /// Spyglasses from the data pack. Holding one zooms the view.
    spyglasses: Vec<SpyglassDef>,
    /// Brushes from the data pack. One sweeps a soft block for a find.
    brushes: Vec<BrushDef>,
    /// Goat horns from the data pack. Blowing one plays a call.
    horns: Vec<HornDef>,
    /// Maces from the data pack. A swing hits harder while falling.
    maces: Vec<MaceDef>,
    /// Buckets from the data pack. An empty one scoops water.
    buckets: Vec<BucketDef>,
    /// Compound fill charges (ember, extra fluid vials).
    fills: Vec<FillDef>,
    /// Drain tools that clear compound fills from hosts.
    drains: Vec<DrainDef>,
    /// Saddles from the data pack. One goes on the pig.
    saddles: Vec<SaddleDef>,
    /// Clocks from the data pack. Holding one shows the time of day.
    clocks: Vec<ClockDef>,
    /// Maps from the data pack. Holding one shows the ground around you.
    maps: Vec<MapDef>,
    /// Books from the data pack. A lectern holds one open.
    books: Vec<BookDef>,
    /// Fertilizer from the data pack. A full composter yields one.
    fertilizers: Vec<FertilizerDef>,
    /// Arrows from the data pack. A fletching table makes them.
    arrows: Vec<ArrowDef>,
    /// Bows from the data pack. Hold use to draw, release to fire an arrow.
    bows: Vec<BowDef>,
    /// Crossbows — hold use to load (spend an arrow), click again to fire a bolt.
    crossbows: Vec<CrossbowDef>,
    /// Totems from the data pack. Holding one blocks a lethal hit once.
    totems: Vec<TotemDef>,
    /// Firework rockets — boost while gliding with an elytra.
    fireworks: Vec<FireworkDef>,
    /// Splash potions from the data pack. Throw for an AOE heal or harm burst.
    splashes: Vec<SplashDef>,
    /// Crafting lines from `data/terralite/recipes`. The fletching table reads arrow.
    pack_recipes: Vec<PackRecipe>,
    /// Ender pearls from the data pack. Throwing one teleports along the look.
    pearls: Vec<PearlDef>,
    /// Wind charges from the data pack. Throwing one bursts and shoves.
    winds: Vec<WindDef>,
    /// Breeze rods from the data pack. One lifts you, and crafts a wind charge.
    rods: Vec<RodDef>,
    /// Fishing rods from the data pack. Cast into water for a catch.
    fishings: Vec<FishingDef>,
    /// Crop seeds from the data pack. Plant on soil.
    seeds: Vec<SeedsDef>,
    /// Hoes from the data pack. One tills dirt into farmland.
    hoes: Vec<HoeDef>,
    /// Heavy cores from the data pack. A smithing table sets one into a mace.
    cores: Vec<CoreDef>,
    /// Trial keys from the data pack. One opens a copper chest a single time.
    keys: Vec<KeyDef>,
    /// Ominous bottles from the data pack. Drinking one marks the next trial drop.
    omens: Vec<OmenDef>,
    /// Eyes of ender from the data pack. Throwing one flies toward the stairs.
    eyes: Vec<EyeDef>,
    /// Items whose icon is a Blockbench model. The file is a `.bbmodel`.
    model_items: Vec<ModelDef>,
    /// Blocks and items `block.register` / `item.register` added after the pack file.
    script: Mutex<ScriptRegistry>,
}

impl Pack {
    pub fn builtin() -> &'static Pack {
        static PACK: OnceLock<Pack> = OnceLock::new();
        PACK.get_or_init(|| load_builtin_pack().unwrap_or_else(|err| panic!("{err}")))
    }

    pub fn parse(text: &str) -> Result<Pack, String> {
        let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
        let root = object(&root, "pack")?;
        require_only(root, &["shapes", "blocks"], "pack")?;
        let shapes = parse_shapes(root.get("shapes").ok_or("pack is missing shapes")?)?;
        let mut models = Registry::new();
        let blocks = parse_blocks(
            root.get("blocks").ok_or("pack is missing blocks")?,
            &shapes,
            &mut models,
        )?;
        Ok(Pack {
            shapes,
            models,
            blocks,
            tools: Vec::new(),
            foods: Vec::new(),
            armors: Vec::new(),
            shields: Vec::new(),
            boats: Vec::new(),
            minecarts: Vec::new(),
            leads: Vec::new(),
            compasses: Vec::new(),
            spyglasses: Vec::new(),
            brushes: Vec::new(),
            horns: Vec::new(),
            maces: Vec::new(),
            buckets: Vec::new(),
            fills: Vec::new(),
            drains: Vec::new(),
            saddles: Vec::new(),
            clocks: Vec::new(),
            maps: Vec::new(),
            books: Vec::new(),
            fertilizers: Vec::new(),
            arrows: Vec::new(),
            bows: Vec::new(),
            crossbows: Vec::new(),
            totems: Vec::new(),
            fireworks: Vec::new(),
            splashes: Vec::new(),
            pack_recipes: Vec::new(),
            pearls: Vec::new(),
            winds: Vec::new(),
            rods: Vec::new(),
            fishings: Vec::new(),
            seeds: Vec::new(),
            hoes: Vec::new(),
            cores: Vec::new(),
            keys: Vec::new(),
            omens: Vec::new(),
            eyes: Vec::new(),
            model_items: Vec::new(),
            script: Mutex::new(ScriptRegistry {
                blocks: Vec::new(),
                items: Vec::new(),
                recipes: Vec::new(),
                smelts: Vec::new(),
                block_loots: Vec::new(),
                chest_loots: Vec::new(),
                screens: Vec::new(),
                gui: Vec::new(),
                settings: Vec::new(),
                block_entities: Vec::new(),
                tiles: HashMap::new(),
                entities: Vec::new(),
                spawns: Vec::new(),
                models: Vec::new(),
            }),
        })
    }

    pub fn shapes(&self) -> &Registry<ShapeEntry> {
        &self.shapes
    }

    pub fn models(&self) -> &Registry<ShapeEntry> {
        &self.models
    }

    pub fn block_registry(&self) -> &Registry<BlockEntry> {
        &self.blocks
    }

    /// A mob id `namespace:path`. The same script can register it again.
    pub fn register_entity_from(
        &self,
        namespace: &str,
        path: &str,
        source: &str,
    ) -> Result<String, String> {
        let id = identifier(namespace, path)?;
        self.remember_entity(&id, source)?;
        Ok(id)
    }

    /// Remember a mob id that is already `namespace:path`.
    pub fn register_entity(&self, id: &str, source: &str) -> Result<(), String> {
        if !stack_id(id) {
            return Err(format!("bad entity {id}"));
        }
        self.remember_entity(id, source)
    }

    fn remember_entity(&self, id: &str, source: &str) -> Result<(), String> {
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script.entities.iter().find(|entity| entity.id == id) {
            if source.is_empty() || existing.source == source {
                return Ok(());
            }
            return Err(format!("duplicate entity {id}"));
        }
        script.entities.push(ScriptEntity {
            id: id.to_string(),
            source: source.to_string(),
            model: None,
            hostile: false,
            spawn_egg: None,
        });
        Ok(())
    }

    /// Attach a Blockbench entity model path. The file must exist in the pack or a mod folder.
    pub fn set_entity_model(&self, id: &str, model: &str, source: &str) -> Result<(), String> {
        if model.is_empty() || !model.ends_with(".bbmodel") {
            return Err(format!("entity {id} model must be a .bbmodel"));
        }
        if read_text_asset(model).is_none() {
            return Err(format!("unknown entity model {model}"));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(entity) = script.entities.iter_mut().find(|entity| entity.id == id) else {
            return Err(format!("unknown entity {id}"));
        };
        if !source.is_empty() && entity.source != source {
            return Err(format!("entity {id} belongs to another script"));
        }
        if let Some(existing) = &entity.model {
            if existing == model {
                return Ok(());
            }
            if !source.is_empty() && entity.source == source {
                entity.model = Some(model.to_string());
                return Ok(());
            }
            return Err(format!("entity {id} already has a model"));
        }
        entity.model = Some(model.to_string());
        Ok(())
    }

    /// Default chase/attack AI for a script mob.
    pub fn set_entity_hostile(&self, id: &str, hostile: bool, source: &str) -> Result<(), String> {
        let mut script = self.script.lock().expect("script registry");
        let Some(entity) = script.entities.iter_mut().find(|entity| entity.id == id) else {
            return Err(format!("unknown entity {id}"));
        };
        if !source.is_empty() && entity.source != source {
            return Err(format!("entity {id} belongs to another script"));
        }
        entity.hostile = hostile;
        Ok(())
    }

    /// Link a registered item as this mob's spawn egg.
    pub fn set_entity_spawn_egg(&self, id: &str, item: &str, source: &str) -> Result<(), String> {
        if !stack_id(item) {
            return Err(format!("bad spawn egg {item}"));
        }
        if !self.contains_item(item) && !self.contains_block(item) {
            return Err(format!("unknown spawn egg item {item}"));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(entity) = script.entities.iter_mut().find(|entity| entity.id == id) else {
            return Err(format!("unknown entity {id}"));
        };
        if !source.is_empty() && entity.source != source {
            return Err(format!("entity {id} belongs to another script"));
        }
        entity.spawn_egg = Some(item.to_string());
        Ok(())
    }

    /// Model path for a script entity, when set.
    pub fn entity_model(&self, id: &str) -> Option<String> {
        self.script
            .lock()
            .expect("script registry")
            .entities
            .iter()
            .find(|entity| entity.id == id)
            .and_then(|entity| entity.model.clone())
    }

    /// Whether a script entity chases and attacks.
    pub fn entity_hostile(&self, id: &str) -> bool {
        self.script
            .lock()
            .expect("script registry")
            .entities
            .iter()
            .find(|entity| entity.id == id)
            .map(|entity| entity.hostile)
            .unwrap_or(false)
    }

    /// Entity id linked to a spawn-egg item, if any.
    pub fn entity_for_spawn_egg(&self, item: &str) -> Option<String> {
        self.script
            .lock()
            .expect("script registry")
            .entities
            .iter()
            .find(|entity| entity.spawn_egg.as_deref() == Some(item))
            .map(|entity| entity.id.clone())
    }

    /// `(id, model, hostile)` for every script entity, registration order.
    pub fn script_entity_defs(&self) -> Vec<(String, Option<String>, bool)> {
        self.script
            .lock()
            .expect("script registry")
            .entities
            .iter()
            .map(|entity| (entity.id.clone(), entity.model.clone(), entity.hostile))
            .collect()
    }

    /// Remove a previously noted spawn. True when a body stood there.
    pub fn take_spawn(&self, id: &str, x: i32, y: i32, z: i32) -> bool {
        let mut script = self.script.lock().expect("script registry");
        if let Some(index) = script
            .spawns
            .iter()
            .position(|spawn| spawn.id == id && spawn.x == x && spawn.y == y && spawn.z == z)
        {
            script.spawns.remove(index);
            true
        } else {
            false
        }
    }

    /// Remember a body placed by `entity.spawn`. The id must already be registered.
    /// The same script can place the same cell again.
    pub fn note_spawn(&self, id: &str, x: i32, y: i32, z: i32, source: &str) -> Result<(), String> {
        let mut script = self.script.lock().expect("script registry");
        if !script.entities.iter().any(|entity| entity.id == id) {
            return Err(format!("unknown entity {id}"));
        }
        if let Some(existing) = script
            .spawns
            .iter()
            .find(|spawn| spawn.id == id && spawn.x == x && spawn.y == y && spawn.z == z)
        {
            if source.is_empty() || existing.source == source {
                return Ok(());
            }
            return Err(format!("duplicate spawn {id}"));
        }
        script.spawns.push(ScriptSpawn {
            id: id.to_string(),
            x,
            y,
            z,
            source: source.to_string(),
        });
        Ok(())
    }

    /// Mob ids from scripts, in registration order.
    pub fn script_entities(&self) -> Vec<String> {
        self.script
            .lock()
            .expect("script registry")
            .entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect()
    }

    /// Spawned bodies, in the order scripts placed them.
    pub fn script_spawns(&self) -> Vec<(String, i32, i32, i32)> {
        self.script
            .lock()
            .expect("script registry")
            .spawns
            .iter()
            .map(|spawn| (spawn.id.clone(), spawn.x, spawn.y, spawn.z))
            .collect()
    }

    /// Remember a block entity id. Registering the same id again is fine.
    ///
    /// Script ids get per-cell key/value tile data. Builtin containers keep the
    /// chest UI; script tiles do not open an empty chest.
    pub fn note_block_entity(&self, id: &str) -> Result<(), String> {
        if !stack_id(id) {
            return Err(format!("bad block entity {id}"));
        }
        let mut script = self.script.lock().expect("script registry");
        if script.block_entities.iter().any(|existing| existing == id) {
            return Ok(());
        }
        script.block_entities.push(id.to_string());
        Ok(())
    }

    /// Builtin inventory containers (chest UI). Script tiles are not included.
    pub fn is_container(&self, id: &str) -> bool {
        matches!(
            id,
            "chest"
                | "copper_chest"
                | "ender_chest"
                | "shulker_box"
                | "barrel"
                | "decorated_pot"
                | "shelf"
                | "item_frame"
        )
    }

    /// A builtin container or a script-registered tile block.
    pub fn is_block_entity(&self, id: &str) -> bool {
        self.is_container(id)
            || self
                .script
                .lock()
                .expect("script registry")
                .block_entities
                .iter()
                .any(|existing| existing == id)
    }

    /// True when `id` was registered with `BLOCK_ENTITY` / `block_entity.bind`.
    pub fn is_script_block_entity(&self, id: &str) -> bool {
        self.script
            .lock()
            .expect("script registry")
            .block_entities
            .iter()
            .any(|existing| existing == id)
    }

    /// Read a tile field. Empty when missing.
    pub fn tile_get(&self, x: i32, y: i32, z: i32, key: &str) -> String {
        self.script
            .lock()
            .expect("script registry")
            .tiles
            .get(&(x, y, z))
            .and_then(|fields| fields.get(key).cloned())
            .unwrap_or_default()
    }

    /// Write a tile field on a script block entity cell. False when the key or
    /// value is bad, or the cell is not a script tile id.
    pub fn tile_set(
        &self,
        x: i32,
        y: i32,
        z: i32,
        block_id: &str,
        key: &str,
        value: &str,
    ) -> bool {
        if !self.is_script_block_entity(block_id) || !tile_key(key) || !tile_value(value) {
            return false;
        }
        let mut script = self.script.lock().expect("script registry");
        script
            .tiles
            .entry((x, y, z))
            .or_default()
            .insert(key.to_string(), value.to_string());
        true
    }

    pub fn tile_get_int(&self, x: i32, y: i32, z: i32, key: &str) -> i64 {
        self.tile_get(x, y, z, key).parse().unwrap_or(0)
    }

    pub fn tile_set_int(
        &self,
        x: i32,
        y: i32,
        z: i32,
        block_id: &str,
        key: &str,
        value: i64,
    ) -> bool {
        self.tile_set(x, y, z, block_id, key, &value.to_string())
    }

    pub fn tile_has(&self, x: i32, y: i32, z: i32) -> bool {
        self.script
            .lock()
            .expect("script registry")
            .tiles
            .get(&(x, y, z))
            .is_some_and(|fields| !fields.is_empty())
    }

    /// Drop all tile fields at a cell (break / replace).
    pub fn clear_tile_at(&self, x: i32, y: i32, z: i32) {
        self.script
            .lock()
            .expect("script registry")
            .tiles
            .remove(&(x, y, z));
    }

    /// True when a `GUIAPI.createScreen` registered this name.
    pub fn has_gui_screen(&self, name: &str) -> bool {
        self.script
            .lock()
            .expect("script registry")
            .gui
            .iter()
            .any(|row| row.screen.name == name)
    }

    /// A cube block id `namespace:path`, hardness 1.5, breaks to air.
    ///
    /// The id has to be a Fabric-style identifier. A second registration, or an
    /// id the pack file already owns, is an error.
    pub fn register_block(&self, namespace: &str, path: &str) -> Result<String, String> {
        self.register_block_from(namespace, path, "")
    }

    /// Same as [`Self::register_block`]. `source` is the script path.
    ///
    /// Loading that same file again keeps the block. A different file, or the
    /// pack, cannot take the id.
    pub fn register_block_from(
        &self,
        namespace: &str,
        path: &str,
        source: &str,
    ) -> Result<String, String> {
        let id = identifier(namespace, path)?;
        let mut script = self.script.lock().expect("script registry");
        if self.blocks.contains(&id) {
            return Err(format!("duplicate block {id}"));
        }
        if script.items.iter().any(|item| item.id == id) {
            return Err(format!("duplicate item {id}"));
        }
        if let Some(block) = script.blocks.iter().find(|block| block.id == id) {
            if block.source == source && !source.is_empty() {
                return Ok(id);
            }
            return Err(format!("duplicate block {id}"));
        }
        script.blocks.push(ScriptBlock {
            id: id.clone(),
            hardness: 1.5,
            source: source.to_string(),
            texture: None,
            model: None,
            light: None,
        });
        Ok(id)
    }

    /// Point a script block at a pack PNG, or at another block's texture.
    ///
    /// The same script file can set the same path again. A missing file still
    /// stores the path; the face stays flat until that PNG decodes.
    pub fn set_script_texture(&self, id: &str, texture: &str, source: &str) -> Result<(), String> {
        let path = resolve_texture(self, texture)?;
        let leaked = intern_path(&path);
        let mut script = self.script.lock().expect("script registry");
        let Some(block) = script.blocks.iter_mut().find(|block| block.id == id) else {
            return Err(format!("unknown block {id}"));
        };
        if !source.is_empty() && block.source != source {
            return Err(format!("block {id} belongs to another script"));
        }
        if let Some(existing) = block.texture {
            if existing == leaked {
                return Ok(());
            }
            if !source.is_empty() && block.source == source {
                block.texture = Some(leaked);
                return Ok(());
            }
            return Err(format!("block {id} already has a texture"));
        }
        block.texture = Some(leaked);
        Ok(())
    }

    /// Use a pack shape, a pack model, or a Blockbench JSON in a mod folder.
    /// `cube` is the default.
    pub fn set_script_model(&self, id: &str, model: &str, source: &str) -> Result<(), String> {
        self.ensure_model(model)?;
        let mut script = self.script.lock().expect("script registry");
        let Some(block) = script.blocks.iter_mut().find(|block| block.id == id) else {
            return Err(format!("unknown block {id}"));
        };
        if !source.is_empty() && block.source != source {
            return Err(format!("block {id} belongs to another script"));
        }
        if let Some(existing) = &block.model {
            if existing == model {
                return Ok(());
            }
            if !source.is_empty() && block.source == source {
                block.model = Some(model.to_string());
                return Ok(());
            }
            return Err(format!("block {id} already has a model"));
        }
        block.model = Some(model.to_string());
        Ok(())
    }

    /// Seconds to break a script block. The same script can set it again.
    pub fn set_script_hardness(&self, id: &str, hardness: f32, source: &str) -> Result<(), String> {
        if !hardness.is_finite() || hardness <= 0.0 {
            return Err(format!(
                "block {id} hardness must be a positive number of seconds"
            ));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(block) = script.blocks.iter_mut().find(|block| block.id == id) else {
            return Err(format!("unknown block {id}"));
        };
        if !source.is_empty() && block.source != source {
            return Err(format!("block {id} belongs to another script"));
        }
        block.hardness = hardness;
        Ok(())
    }

    /// An item id `namespace:path` that can sit in the hotbar and be dropped.
    ///
    /// It is not a block. `world.set_block` still rejects it.
    pub fn register_item(&self, namespace: &str, path: &str) -> Result<String, String> {
        self.register_item_from(namespace, path, "")
    }

    /// Same as [`Self::register_item`]. The same script file can load again.
    pub fn register_item_from(
        &self,
        namespace: &str,
        path: &str,
        source: &str,
    ) -> Result<String, String> {
        let id = identifier(namespace, path)?;
        let mut script = self.script.lock().expect("script registry");
        if self.blocks.contains(&id) || script.blocks.iter().any(|block| block.id == id) {
            return Err(format!("duplicate item {id}"));
        }
        if let Some(item) = script.items.iter().find(|item| item.id == id) {
            if !source.is_empty() && item.source == source {
                return Ok(id);
            }
            return Err(format!("duplicate item {id}"));
        }
        script.items.push(ScriptItem {
            id: id.clone(),
            source: source.to_string(),
            places: None,
            max_count: None,
        });
        Ok(id)
    }

    /// Warm light `1..=15` on a script block. The same script can set it again.
    pub fn set_script_light(&self, id: &str, level: u8, source: &str) -> Result<(), String> {
        if !(1..=15).contains(&level) {
            return Err(format!("block {id} light level {level} is outside 1..=15"));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(block) = script.blocks.iter_mut().find(|block| block.id == id) else {
            return Err(format!("unknown block {id}"));
        };
        if !source.is_empty() && block.source != source {
            return Err(format!("block {id} belongs to another script"));
        }
        block.light = Some(Light {
            color: [1.0, 0.35, 0.1],
            level,
        });
        Ok(())
    }

    /// Hotbar stack limit `1..=64` for a script item. The same script can set it again.
    pub fn set_item_stack(&self, item: &str, count: u16, source: &str) -> Result<(), String> {
        if !(1..=64).contains(&count) {
            return Err(format!("item {item} stack must be from 1 to 64"));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(entry) = script.items.iter_mut().find(|entry| entry.id == item) else {
            return Err(format!("unknown item {item}"));
        };
        if !source.is_empty() && entry.source != source {
            return Err(format!("item {item} belongs to another script"));
        }
        entry.max_count = Some(count);
        Ok(())
    }

    /// Hotbar stack limit for an item. Missing or unknown is 64 when the item exists, else 0.
    pub fn max_stack(&self, id: &str) -> u16 {
        if !self.contains_item(id) {
            return 0;
        }
        self.script
            .lock()
            .expect("script registry")
            .items
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| entry.max_count)
            .unwrap_or(64)
    }

    /// A hotbar place of `item` becomes `block`. Both ids must already exist.
    pub fn set_item_places(&self, item: &str, block: &str, source: &str) -> Result<(), String> {
        if !self.contains_block(block) {
            return Err(format!("unknown block {block}"));
        }
        let mut script = self.script.lock().expect("script registry");
        let Some(entry) = script.items.iter_mut().find(|entry| entry.id == item) else {
            return Err(format!("unknown item {item}"));
        };
        if !source.is_empty() && entry.source != source {
            return Err(format!("item {item} belongs to another script"));
        }
        if let Some(existing) = &entry.places {
            if existing == block {
                return Ok(());
            }
            if !source.is_empty() && entry.source == source {
                entry.places = Some(block.to_string());
                return Ok(());
            }
            return Err(format!("item {item} already places a block"));
        }
        entry.places = Some(block.to_string());
        Ok(())
    }

    /// The block a script item places, when `item.places` named one.
    pub fn placed_block(&self, item: &str) -> Option<String> {
        self.script
            .lock()
            .expect("script registry")
            .items
            .iter()
            .find(|entry| entry.id == item)
            .and_then(|entry| entry.places.clone())
    }

    /// Script items that place a block, in registration order.
    pub fn placeable_item_ids(&self) -> Vec<String> {
        self.script
            .lock()
            .expect("script registry")
            .items
            .iter()
            .filter(|entry| entry.places.is_some())
            .map(|entry| entry.id.clone())
            .collect()
    }

    pub fn contains_block(&self, id: &str) -> bool {
        self.blocks.contains(id) || self.script_block(id).is_some()
    }

    /// A pack block, a script block, or a script item.
    pub fn contains_item(&self, id: &str) -> bool {
        self.contains_block(id) || self.script_item(id)
    }

    /// Ids added with `block.register`, in registration order.
    pub fn script_block_ids(&self) -> Vec<String> {
        self.script
            .lock()
            .expect("script registry")
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect()
    }

    fn script_block(&self, id: &str) -> Option<f32> {
        self.script
            .lock()
            .expect("script registry")
            .blocks
            .iter()
            .find(|block| block.id == id)
            .map(|block| block.hardness)
    }

    fn script_light(&self, id: &str) -> Option<Light> {
        self.script
            .lock()
            .expect("script registry")
            .blocks
            .iter()
            .find(|block| block.id == id)
            .and_then(|block| block.light)
    }

    fn script_texture(&self, id: &str) -> Option<&'static str> {
        self.script
            .lock()
            .expect("script registry")
            .blocks
            .iter()
            .find(|block| block.id == id)
            .and_then(|block| block.texture)
    }

    /// Shape or model id for a script block. `None` when `id` is not one.
    fn script_geometry(&self, id: &str) -> Option<String> {
        let script = self.script.lock().expect("script registry");
        let block = script.blocks.iter().find(|block| block.id == id)?;
        Some(block.model.clone().unwrap_or_else(|| "cube".to_string()))
    }

    fn shape_named(&self, id: &str) -> Shape<'_> {
        if let Some(entry) = self.shapes.get(id) {
            return entry.shape();
        }
        if let Some(entry) = self.models.get(id) {
            return entry.shape();
        }
        if let Some(entry) = self.script_model(id) {
            return entry.shape();
        }
        Shape::empty()
    }

    fn script_model(&self, id: &str) -> Option<&'static ShapeEntry> {
        self.script
            .lock()
            .expect("script registry")
            .models
            .iter()
            .find(|(name, _)| *name == id)
            .map(|(_, entry)| *entry)
    }

    /// Load `model` from the resource pack or `mods/<name>/` when it is not a shape.
    fn ensure_model(&self, model: &str) -> Result<(), String> {
        if self.shapes.get(model).is_some()
            || self.models.get(model).is_some()
            || self.script_model(model).is_some()
        {
            return Ok(());
        }
        let text = read_text_asset(model).ok_or_else(|| format!("unknown model {model}"))?;
        let entry = parse_model_in(&text, model).map_err(|err| format!("{model}: {err}"))?;
        let leaked = Box::leak(Box::new(entry));
        let name = intern_path(model);
        self.script
            .lock()
            .expect("script registry")
            .models
            .push((name, leaked));
        Ok(())
    }

    /// A crafting line for the Items screen. Both ids must already be registered.
    pub fn add_script_recipe(
        &self,
        result: &str,
        ingredient: &str,
        source: &str,
    ) -> Result<(), String> {
        self.add_script_line(result, &[ingredient], source)
    }

    /// A crafting line with 1 to 4 ingredients. The same script can load it again.
    pub fn add_script_line(
        &self,
        result: &str,
        ingredients: &[&str],
        source: &str,
    ) -> Result<(), String> {
        if result.is_empty() || ingredients.is_empty() || ingredients.len() > 4 {
            return Err("recipe id is empty".into());
        }
        if ingredients.iter().any(|ingredient| ingredient.is_empty()) {
            return Err("recipe id is empty".into());
        }
        if ingredients.iter().any(|ingredient| *ingredient == result) {
            return Err(format!("recipe {result} uses itself"));
        }
        if !self.contains_block(result) && !self.contains_item(result) {
            return Err(format!("unknown recipe result {result}"));
        }
        for ingredient in ingredients {
            if !self.contains_block(ingredient) && !self.contains_item(ingredient) {
                return Err(format!("unknown recipe ingredient {ingredient}"));
            }
        }
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script.recipes.iter().find(|recipe| recipe.result == result) {
            let same = existing.ingredients.len() == ingredients.len()
                && existing
                    .ingredients
                    .iter()
                    .zip(ingredients)
                    .all(|(have, want)| have == want);
            if same && (source.is_empty() || existing.source == source) {
                return Ok(());
            }
            return Err(format!("duplicate recipe {result}"));
        }
        script.recipes.push(ScriptRecipe {
            result: result.to_string(),
            ingredients: ingredients.iter().map(|id| (*id).to_string()).collect(),
            pattern: None,
            source: source.to_string(),
        });
        Ok(())
    }

    /// A 3 by 3 crafting shape. `grid` is nine cells separated by commas.
    ///
    /// An empty cell is blank. The shape may sit anywhere on the crafting grid.
    pub fn add_script_shaped(&self, result: &str, grid: &str, source: &str) -> Result<(), String> {
        let cells: Vec<String> = grid
            .split(',')
            .map(|cell| cell.trim().to_string())
            .collect();
        if cells.len() != 9 {
            return Err(format!("shaped recipe needs 9 cells, got {}", cells.len()));
        }
        if cells.iter().any(|cell| !cell.is_empty() && !stack_id(cell)) {
            return Err(format!("shaped recipe {result} has a bad id"));
        }
        let ingredients: Vec<&str> = cells
            .iter()
            .filter(|cell| !cell.is_empty())
            .map(String::as_str)
            .collect();
        if result.is_empty() || ingredients.is_empty() {
            return Err("recipe id is empty".into());
        }
        if ingredients.iter().any(|ingredient| *ingredient == result) {
            return Err(format!("recipe {result} uses itself"));
        }
        if !self.contains_block(result) && !self.contains_item(result) {
            return Err(format!("unknown recipe result {result}"));
        }
        for ingredient in &ingredients {
            if !self.contains_block(ingredient) && !self.contains_item(ingredient) {
                return Err(format!("unknown recipe ingredient {ingredient}"));
            }
        }
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script.recipes.iter().find(|recipe| recipe.result == result) {
            let same = existing.pattern.as_ref() == Some(&cells)
                && (source.is_empty() || existing.source == source);
            if same {
                return Ok(());
            }
            return Err(format!("duplicate recipe {result}"));
        }
        script.recipes.push(ScriptRecipe {
            result: result.to_string(),
            ingredients: ingredients.iter().map(|id| (*id).to_string()).collect(),
            pattern: Some(cells),
            source: source.to_string(),
        });
        Ok(())
    }

    /// Break drops for `block` from `loot.block`. Every listed id must exist.
    pub fn set_block_loot(
        &self,
        block: &str,
        drops: &[&str],
        source: &str,
    ) -> Result<(), String> {
        self.set_loot_list(true, block, drops, source)
    }

    /// Override a named chest table's `drops` list (`camp_chest`, …).
    pub fn set_chest_loot(
        &self,
        table: &str,
        drops: &[&str],
        source: &str,
    ) -> Result<(), String> {
        self.set_loot_list(false, table, drops, source)
    }

    fn set_loot_list(
        &self,
        block: bool,
        id: &str,
        drops: &[&str],
        source: &str,
    ) -> Result<(), String> {
        if id.is_empty() || drops.is_empty() || drops.len() > 16 {
            return Err("loot list needs 1 to 16 ids".into());
        }
        if drops.iter().any(|drop| drop.is_empty()) {
            return Err("loot id is empty".into());
        }
        if block && !self.contains_block(id) {
            return Err(format!("unknown loot block {id}"));
        }
        for drop in drops {
            if !self.contains_item(drop) && self.tool_icon(drop).is_none() {
                return Err(format!("unknown loot drop {drop}"));
            }
        }
        let entry = ScriptLoot {
            id: id.to_string(),
            drops: drops.iter().map(|drop| (*drop).to_string()).collect(),
            source: source.to_string(),
        };
        let mut script = self.script.lock().expect("script registry");
        let list = if block {
            &mut script.block_loots
        } else {
            &mut script.chest_loots
        };
        if let Some(existing) = list.iter_mut().find(|loot| loot.id == id) {
            if existing.drops == entry.drops
                && (source.is_empty() || existing.source == source)
            {
                return Ok(());
            }
            if !source.is_empty() && existing.source == source {
                *existing = entry;
                return Ok(());
            }
            return Err(format!("duplicate loot {id}"));
        }
        list.push(entry);
        Ok(())
    }

    /// Mod-registered break drops for `block`, when present.
    pub fn script_block_loot(&self, block: &str) -> Option<Vec<String>> {
        let script = self.script.lock().expect("script registry");
        script
            .block_loots
            .iter()
            .find(|loot| loot.id == block)
            .map(|loot| loot.drops.clone())
    }

    /// Mod override for a chest table's `drops` list.
    pub fn script_chest_loot(&self, table: &str) -> Option<Vec<String>> {
        let script = self.script.lock().expect("script registry");
        script
            .chest_loots
            .iter()
            .find(|loot| loot.id == table)
            .map(|loot| loot.drops.clone())
    }

    /// Furnace / smoker / blast cook. Same input may exist once per station.
    pub fn set_smelt_recipe(
        &self,
        result: &str,
        input: &str,
        station: HeatStation,
        source: &str,
    ) -> Result<(), String> {
        if result.is_empty() || input.is_empty() {
            return Err("smelt id is empty".into());
        }
        if result == input {
            return Err(format!("smelt {result} uses itself"));
        }
        if !self.contains_item(result) && self.tool_icon(result).is_none() {
            return Err(format!("unknown smelt result {result}"));
        }
        if !self.contains_item(input) && self.tool_icon(input).is_none() {
            return Err(format!("unknown smelt input {input}"));
        }
        let entry = ScriptSmelt {
            result: result.to_string(),
            input: input.to_string(),
            station,
            source: source.to_string(),
        };
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script
            .smelts
            .iter_mut()
            .find(|line| line.input == input && line.station == station)
        {
            if existing.result == result && (source.is_empty() || existing.source == source) {
                return Ok(());
            }
            if !source.is_empty() && existing.source == source {
                *existing = entry;
                return Ok(());
            }
            return Err(format!("duplicate smelt {input}"));
        }
        script.smelts.push(entry);
        Ok(())
    }

    /// `(result, input, station)` cook lines from mods and datapack smelting.
    pub fn script_smelt_lines(&self) -> Vec<(String, String, HeatStation)> {
        let script = self.script.lock().expect("script registry");
        script
            .smelts
            .iter()
            .map(|line| (line.result.clone(), line.input.clone(), line.station))
            .collect()
    }

    pub fn script_recipes(&self) -> Vec<(String, String)> {
        self.script_recipe_lines()
            .into_iter()
            .filter_map(|(result, ingredients)| {
                ingredients
                    .into_iter()
                    .next()
                    .map(|ingredient| (result, ingredient))
            })
            .collect()
    }

    /// Shaped results and their nine cells. A pile has no entry here.
    pub fn script_patterns(&self) -> Vec<(String, Vec<String>)> {
        self.script
            .lock()
            .expect("script registry")
            .recipes
            .iter()
            .filter_map(|recipe| {
                recipe
                    .pattern
                    .clone()
                    .map(|pattern| (recipe.result.clone(), pattern))
            })
            .collect()
    }

    /// Result id and every ingredient, in registration order.
    pub fn script_recipe_lines(&self) -> Vec<(String, Vec<String>)> {
        self.script
            .lock()
            .expect("script registry")
            .recipes
            .iter()
            .map(|recipe| (recipe.result.clone(), recipe.ingredients.clone()))
            .collect()
    }

    /// A mod screen id such as `guide:book`. The same script can register it again.
    pub fn add_script_screen(&self, id: &str, source: &str) -> Result<(), String> {
        if !stack_id(id) {
            return Err(format!("bad screen id {id}"));
        }
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script.screens.iter().find(|screen| screen.id == id) {
            if source.is_empty() || existing.source == source {
                return Ok(());
            }
            return Err(format!("duplicate screen {id}"));
        }
        script.screens.push(ScriptScreen {
            id: id.to_string(),
            source: source.to_string(),
        });
        Ok(())
    }

    /// A screen built by `GUIAPI.createScreen`. The same script can register it again.
    ///
    /// A trial script named `mod.rg` can share a name with a file-backed mod,
    /// and the later one wins. Two different mod files still cannot share a name.
    pub fn add_gui_screen(
        &self,
        screen: rebuild_rosegold::GuiScreen,
        source: &str,
    ) -> Result<(), String> {
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script
            .gui
            .iter_mut()
            .find(|row| row.screen.name == screen.name)
        {
            if source.is_empty()
                || existing.source == source
                || source == "mod.rg"
                || existing.source == "mod.rg"
            {
                existing.screen = screen;
                if !source.is_empty() {
                    existing.source = source.to_string();
                }
                return Ok(());
            }
            return Err(format!("duplicate screen {}", screen.name));
        }
        script.gui.push(StoredGui {
            screen,
            source: source.to_string(),
        });
        Ok(())
    }

    /// Replace a screen a press just edited. The script that registered it stays the same.
    pub fn note_gui_screen(&self, screen: rebuild_rosegold::GuiScreen) {
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script
            .gui
            .iter_mut()
            .find(|row| row.screen.name == screen.name)
        {
            existing.screen = screen;
            return;
        }
        script.gui.push(StoredGui {
            screen,
            source: String::new(),
        });
    }

    /// A settings row from `SettingsAPI.createOption`. At most four across every mod.
    ///
    /// The same file can register the row again. A trial script named `mod.rg`
    /// can share a name with a file-backed mod, and the later one wins. Two
    /// different mod files still cannot share a name.
    pub fn add_setting(
        &self,
        option: rebuild_rosegold::SettingOption,
        source: &str,
    ) -> Result<(), String> {
        let mut script = self.script.lock().expect("script registry");
        if let Some(existing) = script
            .settings
            .iter_mut()
            .find(|row| row.option.name == option.name)
        {
            if source.is_empty()
                || existing.source == source
                || source == "mod.rg"
                || existing.source == "mod.rg"
            {
                existing.option = option;
                if !source.is_empty() {
                    existing.source = source.to_string();
                }
                return Ok(());
            }
            return Err(format!("duplicate setting {}", option.name));
        }
        if script.settings.len() >= 4 {
            return Err("too many settings".into());
        }
        script.settings.push(StoredSetting {
            option,
            source: source.to_string(),
        });
        Ok(())
    }

    pub fn settings(&self) -> Vec<rebuild_rosegold::SettingOption> {
        self.script
            .lock()
            .expect("script registry")
            .settings
            .iter()
            .map(|row| row.option.clone())
            .collect()
    }

    /// Flip `on` and `off`. Any other value stays. False when `name` is not a script row.
    pub fn toggle_setting(&self, name: &str) -> bool {
        let mut script = self.script.lock().expect("script registry");
        let Some(row) = script
            .settings
            .iter_mut()
            .find(|row| row.option.name == name)
        else {
            return false;
        };
        if !row.option.choices.is_empty() {
            let index = row
                .option
                .choices
                .iter()
                .position(|word| word == &row.option.value)
                .unwrap_or(0);
            let next = (index + 1) % row.option.choices.len();
            row.option.value = row.option.choices[next].clone();
            return true;
        }
        row.option.value = match row.option.value.as_str() {
            "on" => "off".to_string(),
            "off" => "on".to_string(),
            other => other.to_string(),
        };
        true
    }

    /// Set a script settings row to an exact value. False when missing or refused.
    pub fn set_setting_value(&self, name: &str, value: &str) -> bool {
        let mut script = self.script.lock().expect("script registry");
        let Some(row) = script
            .settings
            .iter_mut()
            .find(|row| row.option.name == name)
        else {
            return false;
        };
        if !row.option.choices.is_empty() {
            if !row.option.choices.iter().any(|word| word == value) {
                return false;
            }
            if row.option.value == value {
                return false;
            }
            row.option.value = value.to_string();
            return true;
        }
        match value {
            "on" | "off" if row.option.value == "on" || row.option.value == "off" => {
                if row.option.value == value {
                    return false;
                }
                row.option.value = value.to_string();
                true
            }
            _ => false,
        }
    }

    pub fn gui_screens(&self) -> Vec<rebuild_rosegold::GuiScreen> {
        self.script
            .lock()
            .expect("script registry")
            .gui
            .iter()
            .map(|row| row.screen.clone())
            .collect()
    }

    pub fn script_screens(&self) -> Vec<String> {
        self.script
            .lock()
            .expect("script registry")
            .screens
            .iter()
            .map(|screen| screen.id.clone())
            .collect()
    }

    /// `mods/<name>/data/<namespace>/blocks` and `recipes`.
    ///
    /// A missing `data` folder does nothing. Block files set hardness on a
    /// script block from `source`. Recipe files add a crafting line.
    pub fn load_mod_data(&self, root: &Path, source: &str) -> Result<(), String> {
        let data = root.join("data");
        if !data.is_dir() {
            return Ok(());
        }
        let mut namespaces = Vec::new();
        let entries = fs::read_dir(&data).map_err(|err| format!("{}: {err}", data.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("{}: {err}", data.display()))?;
            let path = entry.path();
            if path.is_dir() {
                namespaces.push(path);
            }
        }
        namespaces.sort();
        for ns_dir in namespaces {
            let ns = ns_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !valid_name(ns) {
                return Err(format!("bad data namespace {ns}"));
            }
            self.load_mod_blocks(&ns_dir.join("blocks"), ns, source)?;
            self.load_mod_recipes(&ns_dir.join("recipes"), ns, source)?;
        }
        Ok(())
    }

    fn load_mod_blocks(&self, dir: &Path, namespace: &str, source: &str) -> Result<(), String> {
        for (stem, text) in json_files(dir)? {
            let id = format!("{namespace}:{stem}");
            let root: Value = serde_json::from_str(&text)
                .map_err(|err| format!("{namespace}/blocks/{stem}.json: {err}"))?;
            let obj = object(&root, &format!("{stem}.json"))?;
            require_only(obj, &["hardness"], &format!("{stem}.json"))?;
            let hardness = match obj.get("hardness") {
                Some(value) => positive_seconds(value, &format!("block {id} hardness"))?,
                None => return Err(format!("block {id} needs a hardness")),
            };
            self.set_script_hardness(&id, hardness, source)?;
        }
        Ok(())
    }

    fn load_mod_recipes(&self, dir: &Path, namespace: &str, source: &str) -> Result<(), String> {
        for (stem, text) in json_files(dir)? {
            let expected = format!("{namespace}:{stem}");
            let root: Value = serde_json::from_str(&text)
                .map_err(|err| format!("{namespace}/recipes/{stem}.json: {err}"))?;
            let obj = object(&root, &format!("{stem}.json"))?;
            require_only(obj, &["result", "ingredients"], &format!("{stem}.json"))?;
            let result = obj
                .get("result")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{stem}.json needs a result"))?;
            if !stack_id(result) || result != expected {
                return Err(format!("{stem}.json result must be {expected}"));
            }
            let list = obj
                .get("ingredients")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{stem}.json ingredients must be an array"))?;
            if list.is_empty() || list.len() > 4 {
                return Err(format!("{stem}.json needs 1 to 4 ingredients"));
            }
            let mut ingredients = Vec::new();
            for (index, ingredient) in list.iter().enumerate() {
                let Some(item) = ingredient.as_str() else {
                    return Err(format!("{stem}.json ingredient {index} must be a string"));
                };
                if !stack_id(item) || item == result {
                    return Err(format!("{stem}.json ingredient {item} is a bad id"));
                }
                if ingredients.iter().any(|existing: &String| existing == item) {
                    return Err(format!("{stem}.json ingredient {item} is repeated"));
                }
                ingredients.push(item.to_string());
            }
            let refs: Vec<&str> = ingredients.iter().map(String::as_str).collect();
            self.add_script_line(result, &refs, source)?;
        }
        Ok(())
    }

    fn script_item(&self, id: &str) -> bool {
        self.script
            .lock()
            .expect("script registry")
            .items
            .iter()
            .any(|item| item.id == id)
    }

    pub fn blocks(&self) -> impl Iterator<Item = (&str, &str)> {
        self.blocks
            .iter()
            .map(|(id, block)| (id, block.geometry_id()))
    }

    pub fn break_result(&self, block: Block) -> Block {
        if self.script_block(block.id).is_some() {
            return Block::AIR;
        }
        let Some(def) = self.blocks.get(block.id) else {
            return Block::AIR;
        };
        match def.replaces() {
            Some(stone) => Block::new(stone, 0),
            None => Block::AIR,
        }
    }

    pub fn shape_of(&self, block: Block) -> Shape<'_> {
        if block.is_air() {
            return Shape::empty();
        }
        if block.metadata & 1 == 1 {
            if block.id == "door" || block.id == "copper_door" {
                return self.shape_named("door_open");
            }
            if block.id == "trapdoor" || block.id == "copper_trapdoor" {
                return self.shape_named("trapdoor_open");
            }
            if block.id == "slab" {
                return self.shape_named("slab_top");
            }
            if block.id == "fence_gate" {
                return self.shape_named("fence_gate_open");
            }
            if block.id == "lever" {
                return self.shape_named("lever_on");
            }
            if block.id == "flower_pot" {
                return self.shape_named("flower_pot_planted");
            }
            if block.id == "end_portal_frame" {
                return self.shape_named("end_portal_frame_filled");
            }
        }
        if block.id == "cake" && block.metadata > 0 {
            return cake_slice(block.metadata);
        }
        if block.id == "wheat_crop" {
            let name = match block.metadata.min(3) {
                0 => "wheat_crop_0",
                1 => "wheat_crop_1",
                2 => "wheat_crop_2",
                _ => "wheat_crop",
            };
            return self.shape_named(name);
        }
        if block.id == "cauldron" {
            return cauldron_shape(block.metadata);
        }
        if block.id == "bell" {
            return bell_shape(block.metadata);
        }
        if block.id == "candle" {
            return candle_shape(block.metadata);
        }
        if block.id == "respawn_anchor" {
            return anchor_shape(block.metadata);
        }
        if block.id == "painting" {
            let name = match block.metadata {
                2 => "painting_east",
                3 => "painting_north",
                4 => "painting_south",
                _ => "painting_west",
            };
            return self.shape_named(name);
        }
        if block.id == "item_frame" {
            return frame_shape(block.metadata);
        }
        if block.id == "vine" {
            let name = match block.metadata {
                2 => "vine_east",
                3 => "vine_north",
                4 => "vine_south",
                _ => "vine_west",
            };
            return self.shape_named(name);
        }
        if block.id == "torch" {
            let name = match block.metadata {
                1 => "torch_west",
                2 => "torch_east",
                3 => "torch_north",
                4 => "torch_south",
                _ => "torch",
            };
            return self.shape_named(name);
        }
        if block.id == "end_rod" {
            let name = match block.metadata {
                1 => "end_rod_west",
                2 => "end_rod_east",
                3 => "end_rod_north",
                4 => "end_rod_south",
                _ => "end_rod",
            };
            return self.shape_named(name);
        }
        if block.id == "chain" {
            let name = match block.metadata {
                1 | 2 => "chain_x",
                3 | 4 => "chain_z",
                _ => "chain",
            };
            return self.shape_named(name);
        }
        if block.id == "hanging_sign" {
            let name = match block.metadata {
                1 | 2 => "hanging_sign_x",
                3 | 4 => "hanging_sign_z",
                _ => "hanging_sign",
            };
            return self.shape_named(name);
        }
        if block.id == "lightning_rod" {
            return rod_shape(block.metadata);
        }
        if block.id == "target" {
            return target_shape(block.metadata);
        }
        if block.id == "trial_spawner" {
            return spawner_shape(block.metadata);
        }
        if block.id == "copper_bulb" {
            return bulb_shape(block.metadata);
        }
        if block.id == "decorated_pot" {
            return vessel_shape(block.metadata);
        }
        if block.id == "shelf" {
            return board_shape(block.metadata);
        }
        if block.id == "chiseled_bookshelf" {
            return shelf_shape(block.metadata);
        }
        if let Some(model) = self.script_geometry(block.id) {
            return self.shape_named(&model);
        }
        let Some(def) = self.blocks.get(block.id) else {
            return Shape::empty();
        };
        let entry = match &def.geometry {
            Geometry::Shape(id) => self.shapes.get(id),
            Geometry::Model(id) => self.models.get(id),
        };
        match entry {
            Some(entry) => entry.shape(),
            None => Shape::empty(),
        }
    }

    /// The light this block emits. Blocks without one contribute nothing.
    pub fn light_of(&self, block: Block) -> Option<Light> {
        if block.is_air() {
            return None;
        }
        if block.id == "pumpkin" && block.metadata >= 2 {
            return Some(Light {
                color: [1.0, 0.65, 0.15],
                level: 14,
            });
        }
        if block.id == "candle" && block.metadata & 1 == 1 {
            return Some(Light {
                color: [1.0, 0.75, 0.25],
                level: 10,
            });
        }
        if let Some(light) = anchor_light(block) {
            return Some(light);
        }
        if block.id == "lightning_rod" && block.metadata & 8 != 0 {
            return Some(Light {
                color: [0.75, 0.9, 1.0],
                level: 15,
            });
        }
        if block.id == "target" && block.metadata & 1 != 0 {
            return Some(Light {
                color: [1.0, 0.2, 0.15],
                level: 10,
            });
        }
        if block.id == "trial_spawner" && block.metadata & 1 != 0 {
            return Some(Light {
                color: [0.75, 0.35, 1.0],
                level: 10,
            });
        }
        if block.id == "copper_bulb" && block.metadata & 1 != 0 {
            return Some(Light {
                color: [1.0, 0.55, 0.2],
                level: 15,
            });
        }
        if block.id == "lamp" && crate::chunk::lamp_on(block.metadata) {
            return Some(Light {
                color: [1.0, 0.45, 0.12],
                level: 14,
            });
        }
        if block.id == "generator" && crate::chunk::generator_on(block.metadata) {
            return Some(Light {
                color: [1.0, 0.4, 0.08],
                level: 12,
            });
        }
        if let Some(light) = self.script_light(block.id) {
            return Some(light);
        }
        if let Some(light) = self.blocks.get(block.id).and_then(BlockEntry::light) {
            return Some(light);
        }
        // Ember energy occupying a compound host glows warm through the solid.
        // Batteries brighten with stored charge (dim at 1, near-torch at max).
        if block.fill == FILL_EMBER {
            let level = if crate::chunk::is_battery(block.id) {
                let units = crate::chunk::battery_level(block).max(1);
                let max = crate::chunk::BATTERY_MAX.max(1);
                6 + ((9u16 * u16::from(units)) / u16::from(max)) as u8
            } else {
                11
            };
            return Some(Light {
                color: [1.0, 0.42, 0.1],
                level: level.min(15),
            });
        }
        None
    }

    /// The PNG path this block names, relative to `assets/`.
    pub fn texture_of(&self, block: Block) -> Option<&str> {
        if block.is_air() {
            return None;
        }
        if block.id == "pumpkin" && block.metadata >= 1 {
            return Some("assets/terralite/textures/carved_pumpkin.png");
        }
        if let Some(path) = self.script_texture(block.id) {
            return Some(path);
        }
        self.blocks.get(block.id).and_then(BlockEntry::texture)
    }

    /// PNGs named by a Blockbench model. These are not the block's one texture.
    pub fn model_texture_paths(&self) -> Vec<&str> {
        let mut paths = Vec::new();
        for (_, entry) in self.models.iter() {
            for path in entry.textures() {
                if !path.is_empty() && !paths.contains(&path.as_str()) {
                    paths.push(path.as_str());
                }
            }
        }
        paths
    }

    /// Seconds to break this block, when the pack or the data pack lists it.
    pub fn hardness_of(&self, block: Block) -> Option<f32> {
        if block.is_air() {
            return None;
        }
        self.blocks
            .get(block.id)
            .and_then(BlockEntry::hardness)
            .or_else(|| self.script_block(block.id))
    }

    /// Data pack values replace hardness and replaces already stored on these blocks.
    fn apply_rules(&mut self, rules: &[BlockRule]) -> Result<(), String> {
        let mut seen = Vec::new();
        for rule in rules {
            if !valid_name(&rule.id) {
                return Err(format!("bad data pack block id {}", rule.id));
            }
            if seen.iter().any(|id: &String| id == &rule.id) {
                return Err(format!("duplicate data pack block {}", rule.id));
            }
            seen.push(rule.id.clone());
            if !self.blocks.contains(&rule.id) {
                return Err(format!("data pack block {} is not in the pack", rule.id));
            }
            if let Some(stone) = &rule.replaces {
                if !valid_name(stone) || stone == "air" {
                    return Err(format!("block {} replaces a bad id {stone}", rule.id));
                }
                if stone == &rule.id {
                    return Err(format!("block {} replaces itself", rule.id));
                }
                if !self.blocks.contains(stone) {
                    return Err(format!(
                        "block {} replaces {stone}, which is not in the pack",
                        rule.id
                    ));
                }
            }
        }
        for rule in rules {
            let block = self
                .blocks
                .get_mut(&rule.id)
                .expect("id was checked against the pack");
            block.hardness = Some(rule.hardness);
            block.replaces = rule.replaces.clone();
        }
        Ok(())
    }

    /// Ids of pickaxes, in data-pack order.
    pub fn tool_ids(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.id.clone()).collect()
    }

    /// Block texture a tool or a food borrows for its icon.
    pub fn tool_icon(&self, id: &str) -> Option<&str> {
        self.tools
            .iter()
            .find(|tool| tool.id == id)
            .map(|tool| tool.icon.as_str())
            .or_else(|| {
                self.foods
                    .iter()
                    .find(|food| food.id == id)
                    .map(|food| food.icon.as_str())
            })
            .or_else(|| {
                self.armors
                    .iter()
                    .find(|armor| armor.id == id)
                    .map(|armor| armor.icon.as_str())
            })
            .or_else(|| {
                self.shields
                    .iter()
                    .find(|shield| shield.id == id)
                    .map(|shield| shield.icon.as_str())
            })
            .or_else(|| {
                self.boats
                    .iter()
                    .find(|boat| boat.id == id)
                    .map(|boat| boat.icon.as_str())
            })
            .or_else(|| {
                self.minecarts
                    .iter()
                    .find(|cart| cart.id == id)
                    .map(|cart| cart.icon.as_str())
            })
            .or_else(|| {
                self.leads
                    .iter()
                    .find(|lead| lead.id == id)
                    .map(|lead| lead.icon.as_str())
            })
            .or_else(|| {
                self.compasses
                    .iter()
                    .find(|compass| compass.id == id)
                    .map(|compass| compass.icon.as_str())
            })
            .or_else(|| {
                self.spyglasses
                    .iter()
                    .find(|glass| glass.id == id)
                    .map(|glass| glass.icon.as_str())
            })
            .or_else(|| {
                self.brushes
                    .iter()
                    .find(|brush| brush.id == id)
                    .map(|brush| brush.icon.as_str())
            })
            .or_else(|| {
                self.horns
                    .iter()
                    .find(|horn| horn.id == id)
                    .map(|horn| horn.icon.as_str())
            })
            .or_else(|| {
                self.maces
                    .iter()
                    .find(|mace| mace.id == id)
                    .map(|mace| mace.icon.as_str())
            })
            .or_else(|| {
                self.buckets
                    .iter()
                    .find(|bucket| bucket.id == id)
                    .map(|bucket| bucket.icon.as_str())
            })
            .or_else(|| {
                self.fills
                    .iter()
                    .find(|fill| fill.id == id)
                    .map(|fill| fill.icon.as_str())
            })
            .or_else(|| {
                self.drains
                    .iter()
                    .find(|drain| drain.id == id)
                    .map(|drain| drain.icon.as_str())
            })
            .or_else(|| {
                self.saddles
                    .iter()
                    .find(|saddle| saddle.id == id)
                    .map(|saddle| saddle.icon.as_str())
            })
            .or_else(|| {
                self.clocks
                    .iter()
                    .find(|clock| clock.id == id)
                    .map(|clock| clock.icon.as_str())
            })
            .or_else(|| {
                self.maps
                    .iter()
                    .find(|map| map.id == id)
                    .map(|map| map.icon.as_str())
            })
            .or_else(|| {
                self.books
                    .iter()
                    .find(|book| book.id == id)
                    .map(|book| book.icon.as_str())
            })
            .or_else(|| {
                self.fertilizers
                    .iter()
                    .find(|meal| meal.id == id)
                    .map(|meal| meal.icon.as_str())
            })
            .or_else(|| {
                self.arrows
                    .iter()
                    .find(|arrow| arrow.id == id)
                    .map(|arrow| arrow.icon.as_str())
            })
            .or_else(|| {
                self.bows
                    .iter()
                    .find(|bow| bow.id == id)
                    .map(|bow| bow.icon.as_str())
            })
            .or_else(|| {
                self.crossbows
                    .iter()
                    .find(|bow| bow.id == id)
                    .map(|bow| bow.icon.as_str())
            })
            .or_else(|| {
                self.totems
                    .iter()
                    .find(|totem| totem.id == id)
                    .map(|totem| totem.icon.as_str())
            })
            .or_else(|| {
                self.fireworks
                    .iter()
                    .find(|rocket| rocket.id == id)
                    .map(|rocket| rocket.icon.as_str())
            })
            .or_else(|| {
                self.splashes
                    .iter()
                    .find(|splash| splash.id == id)
                    .map(|splash| splash.icon.as_str())
            })
            .or_else(|| {
                self.pearls
                    .iter()
                    .find(|pearl| pearl.id == id)
                    .map(|pearl| pearl.icon.as_str())
            })
            .or_else(|| {
                self.winds
                    .iter()
                    .find(|wind| wind.id == id)
                    .map(|wind| wind.icon.as_str())
            })
            .or_else(|| {
                self.rods
                    .iter()
                    .find(|rod| rod.id == id)
                    .map(|rod| rod.icon.as_str())
            })
            .or_else(|| {
                self.fishings
                    .iter()
                    .find(|rod| rod.id == id)
                    .map(|rod| rod.icon.as_str())
            })
            .or_else(|| {
                self.seeds
                    .iter()
                    .find(|seed| seed.id == id)
                    .map(|seed| seed.icon.as_str())
            })
            .or_else(|| {
                self.hoes
                    .iter()
                    .find(|hoe| hoe.id == id)
                    .map(|hoe| hoe.icon.as_str())
            })
            .or_else(|| {
                self.cores
                    .iter()
                    .find(|core| core.id == id)
                    .map(|core| core.icon.as_str())
            })
            .or_else(|| {
                self.keys
                    .iter()
                    .find(|key| key.id == id)
                    .map(|key| key.icon.as_str())
            })
            .or_else(|| {
                self.omens
                    .iter()
                    .find(|omen| omen.id == id)
                    .map(|omen| omen.icon.as_str())
            })
            .or_else(|| {
                self.eyes
                    .iter()
                    .find(|eye| eye.id == id)
                    .map(|eye| eye.icon.as_str())
            })
    }

    /// True when holding `id` should show the ground around you.
    pub fn is_map(&self, id: &str) -> bool {
        self.maps.iter().any(|map| map.id == id)
    }

    /// Map ids, in data-pack order.
    pub fn map_ids(&self) -> Vec<String> {
        self.maps.iter().map(|map| map.id.clone()).collect()
    }

    /// True when `id` is a book a lectern can hold.
    pub fn is_book(&self, id: &str) -> bool {
        self.books.iter().any(|book| book.id == id)
    }

    /// Book ids, in data-pack order.
    pub fn book_ids(&self) -> Vec<String> {
        self.books.iter().map(|book| book.id.clone()).collect()
    }

    /// Item ids whose icon is a Blockbench model, in data-pack order.
    pub fn model_item_ids(&self) -> Vec<String> {
        self.model_items
            .iter()
            .map(|item| item.id.clone())
            .collect()
    }

    /// Boxes of an item that names a `.bbmodel`. `None` for every other id.
    pub fn item_model(&self, id: &str) -> Option<Shape<'_>> {
        self.model_items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.shape.shape())
    }

    /// Shape + `display.gui` for an inventory / creative slot icon.
    ///
    /// Resolution mirrors Minecraft item models: a datapack item that names a
    /// `.bbmodel` / model JSON wins; otherwise a placeable block uses
    /// `models/block/<id>` (the same Shape the world meshes).
    pub fn slot_model(&self, id: &str) -> Option<(Shape<'_>, ItemDisplay)> {
        if id.is_empty() || id == "air" || id == "empty" {
            return None;
        }
        if let Some(item) = self.model_items.iter().find(|item| item.id == id) {
            return Some((item.shape.shape(), item.shape.gui_display()));
        }
        let block_id = if self.contains_block(id) {
            id
        } else if let Some(placed) = self.placed_block(id) {
            // `placed_block` returns an owned String — resolve via contains + shape_of.
            return self.slot_model_block(&placed);
        } else {
            return None;
        };
        self.slot_model_block(block_id)
    }

    fn slot_model_block(&self, id: &str) -> Option<(Shape<'_>, ItemDisplay)> {
        let shape = self.shape_of(Block::new(id, 0));
        if shape.is_empty() {
            return None;
        }
        let gui = self
            .blocks
            .get(id)
            .and_then(|def| match &def.geometry {
                Geometry::Model(path) => self.models.get(path).map(|entry| entry.gui_display()),
                Geometry::Shape(_) => None,
            })
            .unwrap_or(ItemDisplay::GUI_BLOCK);
        Some((shape, gui))
    }

    /// True when `id` is bone meal from a composter.
    pub fn is_fertilizer(&self, id: &str) -> bool {
        self.fertilizers.iter().any(|meal| meal.id == id)
    }

    /// Fertilizer ids, in data-pack order.
    pub fn fertilizer_ids(&self) -> Vec<String> {
        self.fertilizers
            .iter()
            .map(|meal| meal.id.clone())
            .collect()
    }

    /// Ingredients for a data-pack recipe, in file order. `None` when that result is absent.
    pub fn pack_recipe(&self, id: &str) -> Option<&[String]> {
        self.pack_recipes
            .iter()
            .find(|recipe| recipe.result == id)
            .map(|recipe| recipe.ingredients.as_slice())
    }

    /// True when `id` is an arrow from a fletching table.
    pub fn is_arrow(&self, id: &str) -> bool {
        self.arrows.iter().any(|arrow| arrow.id == id)
    }

    /// Arrow ids, in data-pack order.
    pub fn arrow_ids(&self) -> Vec<String> {
        self.arrows.iter().map(|arrow| arrow.id.clone()).collect()
    }

    /// True when `id` is a bow.
    pub fn is_bow(&self, id: &str) -> bool {
        self.bows.iter().any(|bow| bow.id == id)
    }

    /// Bow ids, in data-pack order.
    pub fn bow_ids(&self) -> Vec<String> {
        self.bows.iter().map(|bow| bow.id.clone()).collect()
    }

    /// True when `id` is a crossbow.
    pub fn is_crossbow(&self, id: &str) -> bool {
        self.crossbows.iter().any(|bow| bow.id == id)
    }

    /// Crossbow ids, in data-pack order.
    pub fn crossbow_ids(&self) -> Vec<String> {
        self.crossbows.iter().map(|bow| bow.id.clone()).collect()
    }

    /// True when `id` is a totem of undying.
    pub fn is_totem(&self, id: &str) -> bool {
        self.totems.iter().any(|totem| totem.id == id)
    }

    /// Totem ids, in data-pack order.
    pub fn totem_ids(&self) -> Vec<String> {
        self.totems.iter().map(|totem| totem.id.clone()).collect()
    }

    /// True when `id` is a wearable elytra (chest armor that enables glide).
    pub fn is_elytra(&self, id: &str) -> bool {
        id == "elytra" && self.armor_of(id).is_some_and(|(slot, _)| slot == 1)
    }

    /// True when `id` is a firework rocket (elytra boost).
    pub fn is_firework(&self, id: &str) -> bool {
        self.fireworks.iter().any(|rocket| rocket.id == id)
    }

    /// Firework rocket ids, in data-pack order.
    pub fn firework_ids(&self) -> Vec<String> {
        self.fireworks
            .iter()
            .map(|rocket| rocket.id.clone())
            .collect()
    }

    /// True when `id` is a throwable splash potion.
    pub fn is_splash(&self, id: &str) -> bool {
        self.splashes.iter().any(|splash| splash.id == id)
    }

    /// Splash potion ids, in data-pack order.
    pub fn splash_ids(&self) -> Vec<String> {
        self.splashes.iter().map(|splash| splash.id.clone()).collect()
    }

    /// Splash effect tag (`heal` / `harm`) when `id` is a splash potion.
    pub fn splash_effect(&self, id: &str) -> Option<&str> {
        self.splashes
            .iter()
            .find(|splash| splash.id == id)
            .map(|splash| splash.effect.as_str())
    }

    /// True when `id` is a thrown ender pearl.
    pub fn is_pearl(&self, id: &str) -> bool {
        self.pearls.iter().any(|pearl| pearl.id == id)
    }

    /// Ender pearl ids, in data-pack order.
    pub fn pearl_ids(&self) -> Vec<String> {
        self.pearls.iter().map(|pearl| pearl.id.clone()).collect()
    }

    /// True when `id` is a wind charge.
    pub fn is_wind(&self, id: &str) -> bool {
        self.winds.iter().any(|wind| wind.id == id)
    }

    /// Wind charge ids, in data-pack order.
    pub fn wind_ids(&self) -> Vec<String> {
        self.winds.iter().map(|wind| wind.id.clone()).collect()
    }

    /// True when `id` is a breeze rod.
    pub fn is_rod(&self, id: &str) -> bool {
        self.rods.iter().any(|rod| rod.id == id)
    }

    /// Breeze rod ids, in data-pack order.
    pub fn rod_ids(&self) -> Vec<String> {
        self.rods.iter().map(|rod| rod.id.clone()).collect()
    }

    /// True when `id` casts a fishing bobber.
    pub fn is_fishing(&self, id: &str) -> bool {
        self.fishings.iter().any(|rod| rod.id == id)
    }

    /// Fishing rod ids, in data-pack order.
    pub fn fishing_ids(&self) -> Vec<String> {
        self.fishings.iter().map(|rod| rod.id.clone()).collect()
    }

    /// True when `id` plants a crop.
    pub fn is_seeds(&self, id: &str) -> bool {
        self.seeds.iter().any(|seed| seed.id == id)
    }

    /// Seed item ids, in data-pack order.
    pub fn seed_ids(&self) -> Vec<String> {
        self.seeds.iter().map(|seed| seed.id.clone()).collect()
    }

    /// True when `id` tills dirt into farmland.
    pub fn is_hoe(&self, id: &str) -> bool {
        self.hoes.iter().any(|hoe| hoe.id == id)
    }

    /// Hoe ids, in data-pack order.
    pub fn hoe_ids(&self) -> Vec<String> {
        self.hoes.iter().map(|hoe| hoe.id.clone()).collect()
    }

    /// True when `id` is a heavy core.
    pub fn is_core(&self, id: &str) -> bool {
        self.cores.iter().any(|core| core.id == id)
    }

    /// Heavy core ids, in data-pack order.
    pub fn core_ids(&self) -> Vec<String> {
        self.cores.iter().map(|core| core.id.clone()).collect()
    }

    /// True when `id` is a trial key.
    pub fn is_key(&self, id: &str) -> bool {
        self.keys.iter().any(|key| key.id == id)
    }

    /// Trial key ids, in data-pack order.
    pub fn key_ids(&self) -> Vec<String> {
        self.keys.iter().map(|key| key.id.clone()).collect()
    }

    /// True when `id` is an ominous bottle.
    pub fn is_omen(&self, id: &str) -> bool {
        self.omens.iter().any(|omen| omen.id == id)
    }

    /// Ominous bottle ids, in data-pack order.
    pub fn omen_ids(&self) -> Vec<String> {
        self.omens.iter().map(|omen| omen.id.clone()).collect()
    }

    /// True when `id` is an eye of ender.
    pub fn is_eye(&self, id: &str) -> bool {
        self.eyes.iter().any(|eye| eye.id == id)
    }

    /// Eye of ender ids, in data-pack order.
    pub fn eye_ids(&self) -> Vec<String> {
        self.eyes.iter().map(|eye| eye.id.clone()).collect()
    }

    /// True when holding `id` should show the time of day.
    pub fn is_clock(&self, id: &str) -> bool {
        self.clocks.iter().any(|clock| clock.id == id)
    }

    /// Clock ids, in data-pack order.
    pub fn clock_ids(&self) -> Vec<String> {
        self.clocks.iter().map(|clock| clock.id.clone()).collect()
    }

    /// True when `id` saddles the pig.
    pub fn is_saddle(&self, id: &str) -> bool {
        self.saddles.iter().any(|saddle| saddle.id == id)
    }

    /// Saddle ids, in data-pack order.
    pub fn saddle_ids(&self) -> Vec<String> {
        self.saddles
            .iter()
            .map(|saddle| saddle.id.clone())
            .collect()
    }

    /// True when `id` is an empty bucket.
    pub fn is_bucket(&self, id: &str) -> bool {
        self.buckets
            .iter()
            .any(|bucket| bucket.id == id && bucket.fills.is_none())
    }

    /// True when `id` pours water.
    pub fn is_water_bucket(&self, id: &str) -> bool {
        self.buckets
            .iter()
            .any(|bucket| bucket.id == id && bucket.fills.as_deref() == Some("water"))
    }

    /// Compound fill kind for `id`: `water` or `ember`. Water buckets count as water.
    pub fn compound_fill_of(&self, id: &str) -> Option<&'static str> {
        if self.is_water_bucket(id) {
            return Some("water");
        }
        self.fills.iter().find(|fill| fill.id == id).map(|fill| {
            match fill.kind.as_str() {
                "water" => "water",
                "ember" => "ember",
                _ => "ember",
            }
        })
    }

    /// True when `id` drains compound fills from a host.
    pub fn is_drain(&self, id: &str) -> bool {
        self.drains.iter().any(|drain| drain.id == id)
    }

    /// Compound fill item ids (ember_charge, …), in data-pack order.
    pub fn fill_ids(&self) -> Vec<String> {
        self.fills.iter().map(|fill| fill.id.clone()).collect()
    }

    /// Compound drain tool ids (drain_sponge, …), in data-pack order.
    pub fn drain_ids(&self) -> Vec<String> {
        self.drains.iter().map(|drain| drain.id.clone()).collect()
    }

    /// Bucket ids, in data-pack order.
    pub fn bucket_ids(&self) -> Vec<String> {
        self.buckets
            .iter()
            .map(|bucket| bucket.id.clone())
            .collect()
    }

    /// True when holding `id` should show the way to the stairs.
    pub fn is_compass(&self, id: &str) -> bool {
        self.compasses.iter().any(|compass| compass.id == id)
    }

    /// Compass ids, in data-pack order.
    pub fn compass_ids(&self) -> Vec<String> {
        self.compasses
            .iter()
            .map(|compass| compass.id.clone())
            .collect()
    }

    /// True when holding `id` should zoom the view.
    pub fn is_spyglass(&self, id: &str) -> bool {
        self.spyglasses.iter().any(|glass| glass.id == id)
    }

    /// Spyglass ids, in data-pack order.
    pub fn spyglass_ids(&self) -> Vec<String> {
        self.spyglasses
            .iter()
            .map(|glass| glass.id.clone())
            .collect()
    }

    /// True when `id` sweeps a soft block.
    pub fn is_brush(&self, id: &str) -> bool {
        self.brushes.iter().any(|brush| brush.id == id)
    }

    /// Brush ids, in data-pack order.
    pub fn brush_ids(&self) -> Vec<String> {
        self.brushes.iter().map(|brush| brush.id.clone()).collect()
    }

    /// True when `id` is a goat horn.
    pub fn is_horn(&self, id: &str) -> bool {
        self.horns.iter().any(|horn| horn.id == id)
    }

    /// Goat horn ids, in data-pack order.
    pub fn horn_ids(&self) -> Vec<String> {
        self.horns.iter().map(|horn| horn.id.clone()).collect()
    }

    /// True when `id` is a mace.
    pub fn is_mace(&self, id: &str) -> bool {
        self.maces.iter().any(|mace| mace.id == id)
    }

    /// Mace ids, in data-pack order.
    pub fn mace_ids(&self) -> Vec<String> {
        self.maces.iter().map(|mace| mace.id.clone()).collect()
    }

    /// Hearts a falling swing of `id` deals. A plain mace is 3.
    pub fn mace_smash(&self, id: &str) -> i32 {
        self.maces
            .iter()
            .find(|mace| mace.id == id)
            .map(|mace| mace.smash)
            .unwrap_or(3)
    }

    /// True when `id` places a boat on water.
    pub fn is_boat(&self, id: &str) -> bool {
        self.boats.iter().any(|boat| boat.id == id)
    }

    /// Boat ids, in data-pack order.
    pub fn boat_ids(&self) -> Vec<String> {
        self.boats.iter().map(|boat| boat.id.clone()).collect()
    }

    /// True when `id` places a minecart on rails.
    pub fn is_minecart(&self, id: &str) -> bool {
        self.minecarts.iter().any(|cart| cart.id == id)
    }

    /// Minecart ids, in data-pack order.
    pub fn minecart_ids(&self) -> Vec<String> {
        self.minecarts.iter().map(|cart| cart.id.clone()).collect()
    }

    /// True when `id` hitches a tamed horse to a fence.
    pub fn is_lead(&self, id: &str) -> bool {
        self.leads.iter().any(|lead| lead.id == id)
    }

    /// Lead ids, in data-pack order.
    pub fn lead_ids(&self) -> Vec<String> {
        self.leads.iter().map(|lead| lead.id.clone()).collect()
    }

    /// True when `id` is a shield. Holding it blocks zombie bites.
    pub fn is_shield(&self, id: &str) -> bool {
        self.shields.iter().any(|shield| shield.id == id)
    }

    /// Shield ids, in data-pack order.
    pub fn shield_ids(&self) -> Vec<String> {
        self.shields
            .iter()
            .map(|shield| shield.id.clone())
            .collect()
    }

    /// Slot and hearts soaked by `id`. `None` when it is not armor.
    ///
    /// Slot `0` is the cap, `1` the tunic, `2` the boots.
    pub fn armor_of(&self, id: &str) -> Option<(u8, i32)> {
        self.armors
            .iter()
            .find(|armor| armor.id == id)
            .map(|armor| (armor.slot, armor.soak))
    }

    /// Armor ids, in data-pack order.
    pub fn armor_ids(&self) -> Vec<String> {
        self.armors.iter().map(|armor| armor.id.clone()).collect()
    }

    /// Hunger points `id` restores. `None` when it is not food.
    pub fn food_of(&self, id: &str) -> Option<i32> {
        self.foods
            .iter()
            .find(|food| food.id == id)
            .map(|food| food.restore)
    }

    /// Food ids, in data-pack order.
    pub fn food_ids(&self) -> Vec<String> {
        self.foods.iter().map(|food| food.id.clone()).collect()
    }

    /// Uses a fresh tool, shield, or brush of `id` has. `0` when it is none of those.
    pub fn tool_uses(&self, id: &str) -> u16 {
        self.tools
            .iter()
            .find(|tool| tool.id == id)
            .map(|tool| tool.uses)
            .or_else(|| {
                self.shields
                    .iter()
                    .find(|shield| shield.id == id)
                    .map(|shield| shield.uses)
            })
            .or_else(|| {
                self.brushes
                    .iter()
                    .find(|brush| brush.id == id)
                    .map(|brush| brush.uses)
            })
            .or_else(|| {
                self.maces
                    .iter()
                    .find(|mace| mace.id == id)
                    .map(|mace| mace.uses)
            })
            .or_else(|| {
                self.hoes
                    .iter()
                    .find(|hoe| hoe.id == id)
                    .map(|hoe| hoe.uses)
            })
            .unwrap_or(0)
    }

    /// How much faster `held` breaks `block`. `1` is a bare hand.
    ///
    /// A pickaxe speeds stone, smooth stone, quartz, sea lantern, prismarine, dark prismarine, prismarine bricks, stone bricks, cracked stone bricks, mossy stone bricks, chiseled stone bricks, sandstone, red sandstone, cobble, ores, deepslate, deepslate bricks, deepslate tiles, cracked deepslate tiles, cobbled deepslate, polished deepslate, blackstone, polished blackstone, blackstone bricks, cracked blackstone bricks, chiseled blackstone, gilded blackstone, basalt, polished basalt, smooth basalt, nether bricks, cracked nether bricks, red nether bricks, chiseled nether bricks, netherrack, end stone, end stone bricks, purpur, purpur pillar, and the furnace.
    /// Wood is 2, stone is 4, iron is 6, diamond is 8. Dirt, grass, and wood stay at 1.
    pub fn mine_factor(&self, held: &str, block: &str) -> f32 {
        let Some(tool) = self.tools.iter().find(|tool| tool.id == held) else {
            return 1.0;
        };
        if !pick_block(block) {
            return 1.0;
        }
        match tool.tier {
            1 => 2.0,
            2 => 4.0,
            3 => 6.0,
            4 => 8.0,
            _ => 1.0,
        }
    }

    fn load_pack_recipes(&mut self, root: &Path) -> Result<(), String> {
        self.pack_recipes = load_pack_recipes_from(root)?;
        Ok(())
    }

    fn load_tools(&mut self, root: &Path) -> Result<(), String> {
        let dir = root.join("data").join("terralite").join("items");
        if !dir.is_dir() {
            return Ok(());
        }
        let entries = fs::read_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| format!("{}: {err}", dir.display()))?;
            let path = entry.path();
            let is_json = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            if path.is_file() && is_json {
                paths.push(path);
            }
        }
        paths.sort();
        let mut tools = Vec::new();
        let mut foods = Vec::new();
        let mut armors = Vec::new();
        let mut shields = Vec::new();
        let mut boats = Vec::new();
        let mut minecarts = Vec::new();
        let mut leads = Vec::new();
        let mut compasses = Vec::new();
        let mut spyglasses = Vec::new();
        let mut brushes = Vec::new();
        let mut horns = Vec::new();
        let mut maces = Vec::new();
        let mut buckets = Vec::new();
        let mut fills = Vec::new();
        let mut drains = Vec::new();
        let mut saddles = Vec::new();
        let mut clocks = Vec::new();
        let mut maps = Vec::new();
        let mut books = Vec::new();
        let mut fertilizers = Vec::new();
        let mut arrows = Vec::new();
        let mut bows = Vec::new();
        let mut crossbows = Vec::new();
        let mut totems = Vec::new();
        let mut fireworks = Vec::new();
        let mut splashes = Vec::new();
        let mut pearls = Vec::new();
        let mut winds = Vec::new();
        let mut rods = Vec::new();
        let mut fishings = Vec::new();
        let mut seeds = Vec::new();
        let mut hoes = Vec::new();
        let mut cores = Vec::new();
        let mut keys = Vec::new();
        let mut omens = Vec::new();
        let mut eyes = Vec::new();
        let mut model_items = Vec::new();
        for path in paths {
            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
                .ok_or_else(|| format!("bad item file {}", path.display()))?;
            if !valid_name(&id) {
                return Err(format!("bad item id {id}"));
            }
            if tools.iter().any(|tool: &ToolDef| tool.id == id)
                || foods.iter().any(|food: &FoodDef| food.id == id)
                || armors.iter().any(|armor: &ArmorDef| armor.id == id)
                || shields.iter().any(|shield: &ShieldDef| shield.id == id)
                || boats.iter().any(|boat: &BoatDef| boat.id == id)
                || minecarts.iter().any(|cart: &MinecartDef| cart.id == id)
                || leads.iter().any(|lead: &LeadDef| lead.id == id)
                || compasses
                    .iter()
                    .any(|compass: &CompassDef| compass.id == id)
                || spyglasses.iter().any(|glass: &SpyglassDef| glass.id == id)
                || brushes.iter().any(|brush: &BrushDef| brush.id == id)
                || horns.iter().any(|horn: &HornDef| horn.id == id)
                || maces.iter().any(|mace: &MaceDef| mace.id == id)
                || buckets.iter().any(|bucket: &BucketDef| bucket.id == id)
                || fills.iter().any(|fill: &FillDef| fill.id == id)
                || drains.iter().any(|drain: &DrainDef| drain.id == id)
                || saddles.iter().any(|saddle: &SaddleDef| saddle.id == id)
                || clocks.iter().any(|clock: &ClockDef| clock.id == id)
                || maps.iter().any(|map: &MapDef| map.id == id)
                || books.iter().any(|book: &BookDef| book.id == id)
                || fertilizers.iter().any(|meal: &FertilizerDef| meal.id == id)
                || arrows.iter().any(|arrow: &ArrowDef| arrow.id == id)
                || bows.iter().any(|bow: &BowDef| bow.id == id)
                || crossbows
                    .iter()
                    .any(|bow: &CrossbowDef| bow.id == id)
                || totems.iter().any(|totem: &TotemDef| totem.id == id)
                || fireworks
                    .iter()
                    .any(|rocket: &FireworkDef| rocket.id == id)
                || splashes.iter().any(|splash: &SplashDef| splash.id == id)
                || pearls.iter().any(|pearl: &PearlDef| pearl.id == id)
                || winds.iter().any(|wind: &WindDef| wind.id == id)
                || rods.iter().any(|rod: &RodDef| rod.id == id)
                || fishings.iter().any(|rod: &FishingDef| rod.id == id)
                || seeds.iter().any(|seed: &SeedsDef| seed.id == id)
                || hoes.iter().any(|hoe: &HoeDef| hoe.id == id)
                || cores.iter().any(|core: &CoreDef| core.id == id)
                || keys.iter().any(|key: &KeyDef| key.id == id)
                || omens.iter().any(|omen: &OmenDef| omen.id == id)
                || eyes.iter().any(|eye: &EyeDef| eye.id == id)
                || model_items.iter().any(|item: &ModelDef| item.id == id)
                || self.blocks.contains(&id)
            {
                return Err(format!("duplicate item {id}"));
            }
            let text =
                fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
            match parse_item(&id, &text)? {
                PackItem::Tool(tool) => tools.push(tool),
                PackItem::Food(food) => foods.push(food),
                PackItem::Armor(armor) => armors.push(armor),
                PackItem::Shield(shield) => shields.push(shield),
                PackItem::Boat(boat) => boats.push(boat),
                PackItem::Minecart(cart) => minecarts.push(cart),
                PackItem::Lead(lead) => leads.push(lead),
                PackItem::Compass(compass) => compasses.push(compass),
                PackItem::Spyglass(glass) => spyglasses.push(glass),
                PackItem::Brush(brush) => brushes.push(brush),
                PackItem::Horn(horn) => horns.push(horn),
                PackItem::Mace(mace) => maces.push(mace),
                PackItem::Bucket(bucket) => buckets.push(bucket),
                PackItem::Fill(fill) => fills.push(fill),
                PackItem::Drain(drain) => drains.push(drain),
                PackItem::Saddle(saddle) => saddles.push(saddle),
                PackItem::Clock(clock) => clocks.push(clock),
                PackItem::Map(map) => maps.push(map),
                PackItem::Book(book) => books.push(book),
                PackItem::Fertilizer(meal) => fertilizers.push(meal),
                PackItem::Arrow(arrow) => arrows.push(arrow),
                PackItem::Bow(bow) => bows.push(bow),
                PackItem::Crossbow(bow) => crossbows.push(bow),
                PackItem::Totem(totem) => totems.push(totem),
                PackItem::Firework(rocket) => fireworks.push(rocket),
                PackItem::Splash(splash) => splashes.push(splash),
                PackItem::Pearl(pearl) => pearls.push(pearl),
                PackItem::Wind(wind) => winds.push(wind),
                PackItem::Rod(rod) => rods.push(rod),
                PackItem::Fishing(rod) => fishings.push(rod),
                PackItem::Seeds(seed) => seeds.push(seed),
                PackItem::Hoe(hoe) => hoes.push(hoe),
                PackItem::Core(core) => cores.push(core),
                PackItem::Key(key) => keys.push(key),
                PackItem::Omen(omen) => omens.push(omen),
                PackItem::Eye(eye) => eyes.push(eye),
                PackItem::Model(item) => model_items.push(item),
            }
        }
        self.tools = tools;
        self.foods = foods;
        self.armors = armors;
        self.shields = shields;
        self.boats = boats;
        self.minecarts = minecarts;
        self.leads = leads;
        self.compasses = compasses;
        self.spyglasses = spyglasses;
        self.brushes = brushes;
        self.horns = horns;
        self.maces = maces;
        self.buckets = buckets;
        self.fills = fills;
        self.drains = drains;
        self.saddles = saddles;
        self.clocks = clocks;
        self.maps = maps;
        self.books = books;
        self.fertilizers = fertilizers;
        self.arrows = arrows;
        self.bows = bows;
        self.crossbows = crossbows;
        self.totems = totems;
        self.fireworks = fireworks;
        self.splashes = splashes;
        self.pearls = pearls;
        self.winds = winds;
        self.rods = rods;
        self.fishings = fishings;
        self.seeds = seeds;
        self.hoes = hoes;
        self.cores = cores;
        self.keys = keys;
        self.omens = omens;
        self.eyes = eyes;
        self.model_items = model_items;
        Ok(())
    }
}

enum PackItem {
    Tool(ToolDef),
    Food(FoodDef),
    Armor(ArmorDef),
    Shield(ShieldDef),
    Boat(BoatDef),
    Minecart(MinecartDef),
    Lead(LeadDef),
    Compass(CompassDef),
    Spyglass(SpyglassDef),
    Brush(BrushDef),
    Horn(HornDef),
    Mace(MaceDef),
    Bucket(BucketDef),
    Fill(FillDef),
    Drain(DrainDef),
    Saddle(SaddleDef),
    Clock(ClockDef),
    Map(MapDef),
    Book(BookDef),
    Fertilizer(FertilizerDef),
    Arrow(ArrowDef),
    Bow(BowDef),
    Crossbow(CrossbowDef),
    Totem(TotemDef),
    Firework(FireworkDef),
    Splash(SplashDef),
    Pearl(PearlDef),
    Wind(WindDef),
    Rod(RodDef),
    Fishing(FishingDef),
    Seeds(SeedsDef),
    Hoe(HoeDef),
    Core(CoreDef),
    Key(KeyDef),
    Omen(OmenDef),
    Eye(EyeDef),
    Model(ModelDef),
}

fn pick_block(id: &str) -> bool {
    matches!(
        id,
        "stone"
            | "smooth_stone"
            | "quartz"
            | "sea_lantern"
            | "prismarine"
            | "dark_prismarine"
            | "prismarine_bricks"
            | "stone_bricks"
            | "cracked_stone_bricks"
            | "mossy_stone_bricks"
            | "chiseled_stone_bricks"
            | "sandstone"
            | "red_sandstone"
            | "cobblestone"
            | "coal_ore"
            | "iron_ore"
            | "emerald_ore"
            | "deepslate"
            | "deepslate_bricks"
            | "cobbled_deepslate"
            | "polished_deepslate"
            | "deepslate_tiles"
            | "cracked_deepslate_tiles"
            | "blackstone"
            | "polished_blackstone"
            | "blackstone_bricks"
            | "cracked_blackstone_bricks"
            | "chiseled_blackstone"
            | "gilded_blackstone"
            | "basalt"
            | "polished_basalt"
            | "smooth_basalt"
            | "nether_bricks"
            | "cracked_nether_bricks"
            | "red_nether_bricks"
            | "chiseled_nether_bricks"
            | "netherrack"
            | "end_stone"
            | "end_stone_bricks"
            | "end_portal_frame"
            | "dragon_egg"
            | "purpur"
            | "purpur_pillar"
            | "furnace"
    )
}

fn parse_item(id: &str, text: &str) -> Result<PackItem, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}.json: {err}"))?;
    let obj = object(&root, &format!("{id}.json"))?;
    if obj.contains_key("model") {
        require_only(obj, &["model"], &format!("{id}.json"))?;
        let path = obj
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json model must be a string"))?;
        if !model_file(path) {
            return Err(format!("{id}.json model must be a .json or a .bbmodel"));
        }
        let shape = load_model(path)?;
        return Ok(PackItem::Model(ModelDef {
            id: id.to_string(),
            shape,
        }));
    }
    if obj.contains_key("food") {
        require_only(obj, &["food", "icon"], &format!("{id}.json"))?;
        let restore = obj
            .get("food")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("{id}.json food must be a whole number"))?;
        if !(1..=10).contains(&restore) {
            return Err(format!("{id}.json food must be from 1 to 10"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Food(FoodDef {
            id: id.to_string(),
            restore: restore as i32,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("armor") {
        require_only(obj, &["armor", "soak", "icon"], &format!("{id}.json"))?;
        let slot = match obj.get("armor").and_then(Value::as_str) {
            Some("head") => 0,
            Some("chest") => 1,
            Some("feet") => 2,
            Some(other) => return Err(format!("{id}.json armor {other} is unknown")),
            None => return Err(format!("{id}.json needs an armor slot")),
        };
        let soak = obj
            .get("soak")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("{id}.json soak must be a whole number"))?;
        if !(1..=10).contains(&soak) {
            return Err(format!("{id}.json soak must be from 1 to 10"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Armor(ArmorDef {
            id: id.to_string(),
            slot,
            soak: soak as i32,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("shield") {
        require_only(obj, &["shield", "uses", "icon"], &format!("{id}.json"))?;
        if obj.get("shield") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json shield must be true"));
        }
        let uses = obj
            .get("uses")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{id}.json uses must be a whole number"))?;
        if !(1..=10_000).contains(&uses) {
            return Err(format!("{id}.json uses must be from 1 to 10000"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Shield(ShieldDef {
            id: id.to_string(),
            uses: uses as u16,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("boat") {
        require_only(obj, &["boat", "icon"], &format!("{id}.json"))?;
        if obj.get("boat") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json boat must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Boat(BoatDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("minecart") {
        require_only(obj, &["minecart", "icon"], &format!("{id}.json"))?;
        if obj.get("minecart") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json minecart must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Minecart(MinecartDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("lead") {
        require_only(obj, &["lead", "icon"], &format!("{id}.json"))?;
        if obj.get("lead") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json lead must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Lead(LeadDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("compass") {
        require_only(obj, &["compass", "icon"], &format!("{id}.json"))?;
        if obj.get("compass") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json compass must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Compass(CompassDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("spyglass") {
        require_only(obj, &["spyglass", "icon"], &format!("{id}.json"))?;
        if obj.get("spyglass") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json spyglass must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Spyglass(SpyglassDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("brush") {
        require_only(obj, &["brush", "uses", "icon"], &format!("{id}.json"))?;
        if obj.get("brush") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json brush must be true"));
        }
        let uses = obj
            .get("uses")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{id}.json uses must be a whole number"))?;
        if !(1..=10_000).contains(&uses) {
            return Err(format!("{id}.json uses must be from 1 to 10000"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Brush(BrushDef {
            id: id.to_string(),
            uses: uses as u16,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("horn") {
        require_only(obj, &["horn", "icon"], &format!("{id}.json"))?;
        if obj.get("horn") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json horn must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Horn(HornDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("mace") {
        require_only(
            obj,
            &["mace", "uses", "smash", "icon"],
            &format!("{id}.json"),
        )?;
        if obj.get("mace") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json mace must be true"));
        }
        let uses = obj
            .get("uses")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{id}.json uses must be a whole number"))?;
        if !(1..=10_000).contains(&uses) {
            return Err(format!("{id}.json uses must be from 1 to 10000"));
        }
        let smash = match obj.get("smash") {
            None => 3,
            Some(value) => {
                let smash = value
                    .as_i64()
                    .ok_or_else(|| format!("{id}.json smash must be a whole number"))?;
                if !(1..=20).contains(&smash) {
                    return Err(format!("{id}.json smash must be from 1 to 20"));
                }
                smash as i32
            }
        };
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Mace(MaceDef {
            id: id.to_string(),
            uses: uses as u16,
            smash,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("bucket") {
        require_only(obj, &["bucket", "icon"], &format!("{id}.json"))?;
        let fills = match obj.get("bucket").and_then(Value::as_str) {
            Some("empty") => None,
            Some("water") => Some("water".to_string()),
            Some(other) => return Err(format!("{id}.json bucket {other} is unknown")),
            None => return Err(format!("{id}.json needs a bucket kind")),
        };
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Bucket(BucketDef {
            id: id.to_string(),
            fills,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("fill") {
        require_only(obj, &["fill", "icon"], &format!("{id}.json"))?;
        let kind = match obj.get("fill").and_then(Value::as_str) {
            Some("water") | Some("ember") => obj
                .get("fill")
                .and_then(Value::as_str)
                .unwrap()
                .to_string(),
            Some(other) => return Err(format!("{id}.json fill {other} is unknown")),
            None => return Err(format!("{id}.json needs a fill kind")),
        };
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Fill(FillDef {
            id: id.to_string(),
            kind,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("drain") {
        require_only(obj, &["drain", "icon"], &format!("{id}.json"))?;
        if obj.get("drain") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json drain must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Drain(DrainDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("pearl") {
        require_only(obj, &["pearl", "icon"], &format!("{id}.json"))?;
        if obj.get("pearl") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json pearl must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Pearl(PearlDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("wind") {
        require_only(obj, &["wind", "icon"], &format!("{id}.json"))?;
        if obj.get("wind") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json wind must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Wind(WindDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("rod") {
        require_only(obj, &["rod", "icon"], &format!("{id}.json"))?;
        if obj.get("rod") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json rod must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Rod(RodDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("fishing") {
        require_only(obj, &["fishing", "icon"], &format!("{id}.json"))?;
        if obj.get("fishing") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json fishing must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Fishing(FishingDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("seeds") {
        require_only(obj, &["seeds", "icon"], &format!("{id}.json"))?;
        if obj.get("seeds") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json seeds must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Seeds(SeedsDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("hoe") {
        require_only(obj, &["hoe", "uses", "icon"], &format!("{id}.json"))?;
        if obj.get("hoe") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json hoe must be true"));
        }
        let uses = obj
            .get("uses")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{id}.json uses must be a whole number"))?;
        if !(1..=256).contains(&uses) {
            return Err(format!("{id}.json uses must be from 1 to 256"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Hoe(HoeDef {
            id: id.to_string(),
            uses: uses as u16,
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("core") {
        require_only(obj, &["core", "icon"], &format!("{id}.json"))?;
        if obj.get("core") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json core must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Core(CoreDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("key") {
        require_only(obj, &["key", "icon"], &format!("{id}.json"))?;
        if obj.get("key") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json key must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Key(KeyDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("omen") {
        require_only(obj, &["omen", "icon"], &format!("{id}.json"))?;
        if obj.get("omen") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json omen must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Omen(OmenDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("eye") {
        require_only(obj, &["eye", "icon"], &format!("{id}.json"))?;
        if obj.get("eye") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json eye must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Eye(EyeDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("saddle") {
        require_only(obj, &["saddle", "icon"], &format!("{id}.json"))?;
        if obj.get("saddle") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json saddle must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Saddle(SaddleDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("clock") {
        require_only(obj, &["clock", "icon"], &format!("{id}.json"))?;
        if obj.get("clock") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json clock must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Clock(ClockDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("map") {
        require_only(obj, &["map", "icon"], &format!("{id}.json"))?;
        if obj.get("map") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json map must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Map(MapDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("book") {
        require_only(obj, &["book", "icon"], &format!("{id}.json"))?;
        if obj.get("book") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json book must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Book(BookDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("fertilizer") {
        require_only(obj, &["fertilizer", "icon"], &format!("{id}.json"))?;
        if obj.get("fertilizer") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json fertilizer must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Fertilizer(FertilizerDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("arrow") {
        require_only(obj, &["arrow", "icon"], &format!("{id}.json"))?;
        if obj.get("arrow") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json arrow must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Arrow(ArrowDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("bow") {
        require_only(obj, &["bow", "icon"], &format!("{id}.json"))?;
        if obj.get("bow") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json bow must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Bow(BowDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("crossbow") {
        require_only(obj, &["crossbow", "icon"], &format!("{id}.json"))?;
        if obj.get("crossbow") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json crossbow must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Crossbow(CrossbowDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("totem") {
        require_only(obj, &["totem", "icon"], &format!("{id}.json"))?;
        if obj.get("totem") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json totem must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Totem(TotemDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("firework") {
        require_only(obj, &["firework", "icon"], &format!("{id}.json"))?;
        if obj.get("firework") != Some(&Value::Bool(true)) {
            return Err(format!("{id}.json firework must be true"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Firework(FireworkDef {
            id: id.to_string(),
            icon: icon.to_string(),
        }));
    }
    if obj.contains_key("splash") {
        require_only(obj, &["splash", "icon"], &format!("{id}.json"))?;
        let effect = obj
            .get("splash")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json splash needs heal or harm"))?;
        if effect != "heal" && effect != "harm" {
            return Err(format!("{id}.json splash {effect} is unknown"));
        }
        let icon = obj
            .get("icon")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{id}.json needs an icon"))?;
        if !valid_name(icon) {
            return Err(format!("{id}.json icon {icon} is a bad id"));
        }
        return Ok(PackItem::Splash(SplashDef {
            id: id.to_string(),
            icon: icon.to_string(),
            effect: effect.to_string(),
        }));
    }
    parse_tool(id, obj).map(PackItem::Tool)
}

fn parse_tool(id: &str, obj: &serde_json::Map<String, Value>) -> Result<ToolDef, String> {
    require_only(
        obj,
        &["tool", "tier", "icon", "uses"],
        &format!("{id}.json"),
    )?;
    let kind = obj
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{id}.json needs a tool"))?;
    if kind != "pickaxe" {
        return Err(format!("{id}.json tool {kind} is not a pickaxe"));
    }
    let tier = match obj.get("tier").and_then(Value::as_str) {
        Some("wood") => 1,
        Some("stone") => 2,
        Some("iron") => 3,
        Some("diamond") => 4,
        Some(other) => return Err(format!("{id}.json tier {other} is unknown")),
        None => return Err(format!("{id}.json needs a tier")),
    };
    let icon = obj
        .get("icon")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{id}.json needs an icon"))?;
    if !valid_name(icon) {
        return Err(format!("{id}.json icon {icon} is a bad id"));
    }
    let uses = obj
        .get("uses")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{id}.json uses must be a whole number"))?;
    if !(1..=10_000).contains(&uses) {
        return Err(format!("{id}.json uses must be from 1 to 10000"));
    }
    Ok(ToolDef {
        id: id.to_string(),
        tier,
        icon: icon.to_string(),
        uses: uses as u16,
    })
}

struct BlockRule {
    id: String,
    hardness: f32,
    replaces: Option<String>,
}

fn require_resource_pack_meta() -> Result<(), String> {
    if !resource_pack_root().join("pack.json").is_file() {
        return Err(format!(
            "resource pack missing pack.json ({})",
            resource_pack_root().display()
        ));
    }
    Ok(())
}

fn load_builtin_pack() -> Result<Pack, String> {
    let mut pack = Pack::load_resource_models()?;
    let rules = read_data_pack(data_pack_root())?;
    pack.apply_rules(&rules)?;
    pack.load_tools(data_pack_root())?;
    pack.load_pack_recipes(data_pack_root())?;
    Ok(pack)
}

impl Pack {
    /// Scan `models/shapes/` and `models/block/` under the resource pack.
    pub fn load_resource_models() -> Result<Pack, String> {
        require_resource_pack_meta()?;
        let shapes = load_named_models(shape_models_dir(), SHAPE_MODELS_DIR)?;
        let mut models = Registry::new();
        let blocks = load_block_models(&mut models)?;
        Ok(Pack {
            shapes,
            models,
            blocks,
            tools: Vec::new(),
            foods: Vec::new(),
            armors: Vec::new(),
            shields: Vec::new(),
            boats: Vec::new(),
            minecarts: Vec::new(),
            leads: Vec::new(),
            compasses: Vec::new(),
            spyglasses: Vec::new(),
            brushes: Vec::new(),
            horns: Vec::new(),
            maces: Vec::new(),
            buckets: Vec::new(),
            fills: Vec::new(),
            drains: Vec::new(),
            saddles: Vec::new(),
            clocks: Vec::new(),
            maps: Vec::new(),
            books: Vec::new(),
            fertilizers: Vec::new(),
            arrows: Vec::new(),
            bows: Vec::new(),
            crossbows: Vec::new(),
            totems: Vec::new(),
            fireworks: Vec::new(),
            splashes: Vec::new(),
            pack_recipes: Vec::new(),
            pearls: Vec::new(),
            winds: Vec::new(),
            rods: Vec::new(),
            fishings: Vec::new(),
            seeds: Vec::new(),
            hoes: Vec::new(),
            cores: Vec::new(),
            keys: Vec::new(),
            omens: Vec::new(),
            eyes: Vec::new(),
            model_items: Vec::new(),
            script: Mutex::new(ScriptRegistry {
                blocks: Vec::new(),
                items: Vec::new(),
                recipes: Vec::new(),
                smelts: Vec::new(),
                block_loots: Vec::new(),
                chest_loots: Vec::new(),
                screens: Vec::new(),
                gui: Vec::new(),
                settings: Vec::new(),
                block_entities: Vec::new(),
                tiles: HashMap::new(),
                entities: Vec::new(),
                spawns: Vec::new(),
                models: Vec::new(),
            }),
        })
    }
}

fn load_named_models(dir: PathBuf, label: &str) -> Result<Registry<ShapeEntry>, String> {
    let entries = fs::read_dir(&dir).map_err(|_| format!("resource pack missing {label}"))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_file() && model_path_file(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(format!("resource pack has no models in {label}"));
    }
    let mut shapes = Registry::new();
    for path in paths {
        let id = file_stem_id(&path)?;
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        let entry = parse_model_in(&text, &path.display().to_string())
            .map_err(|err| format!("{label}/{id}: {err}"))?;
        shapes
            .register(id.clone(), entry)
            .map_err(|_| format!("duplicate shape {id}"))?;
    }
    Ok(shapes)
}

fn load_block_models(
    models: &mut Registry<ShapeEntry>,
) -> Result<Registry<BlockEntry>, String> {
    let dir = block_models_dir();
    let entries =
        fs::read_dir(&dir).map_err(|_| format!("resource pack missing {BLOCK_MODELS_DIR}"))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_file() && model_path_file(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(format!(
            "resource pack has no block models in {BLOCK_MODELS_DIR}"
        ));
    }
    let mut blocks = Registry::new();
    for path in paths {
        let id = file_stem_id(&path)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("bad block model {}", path.display()))?;
        let model_path = format!("{BLOCK_MODELS_DIR}/{file_name}");
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        let (texture, light) = parse_block_meta(&text, &id)?;
        let entry = parse_model_in(&text, &model_path)
            .map_err(|err| format!("{model_path}: {err}"))?;
        models
            .register(model_path.clone(), entry)
            .map_err(|_| format!("duplicate model {model_path}"))?;
        if blocks.contains(&id) {
            return Err(format!("duplicate block {id}"));
        }
        blocks
            .register(
                id,
                BlockEntry {
                    geometry: Geometry::Model(model_path),
                    replaces: None,
                    light,
                    texture,
                    hardness: None,
                },
            )
            .expect("duplicate block was already rejected");
    }
    Ok(blocks)
}

fn model_path_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(model_file)
}

fn file_stem_id(path: &Path) -> Result<String, String> {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("bad model file {}", path.display()))?;
    if !valid_name(id) {
        return Err(format!("bad model id {id}"));
    }
    Ok(id.to_string())
}

fn parse_block_meta(text: &str, id: &str) -> Result<(Option<String>, Option<Light>), String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}: {err}"))?;
    let obj = object(&root, id)?;
    let texture = parse_texture(obj, id)?;
    let light = match obj.get("light") {
        None => None,
        Some(value) => Some(parse_light(value, id)?),
    };
    Ok((texture, light))
}

struct PackRecipe {
    result: String,
    ingredients: Vec<String>,
}

fn load_pack_recipes_from(root: &Path) -> Result<Vec<PackRecipe>, String> {
    let dir = root.join("data").join("terralite").join("recipes");
    let entries = fs::read_dir(&dir).map_err(|_| format!("data pack missing {}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if path.is_file() && is_json {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(format!("data pack has no recipes in {}", dir.display()));
    }
    let mut recipes = Vec::with_capacity(paths.len());
    for path in paths {
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
            .ok_or_else(|| format!("bad recipe file {}", path.display()))?;
        if !valid_name(&id) {
            return Err(format!("bad recipe id {id}"));
        }
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        let recipe = parse_pack_recipe(&id, &text)?;
        if recipes
            .iter()
            .any(|existing: &PackRecipe| existing.result == recipe.result)
        {
            return Err(format!("duplicate recipe {}", recipe.result));
        }
        recipes.push(recipe);
    }
    Ok(recipes)
}

fn parse_pack_recipe(id: &str, text: &str) -> Result<PackRecipe, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}.json: {err}"))?;
    let obj = object(&root, &format!("{id}.json"))?;
    require_only(obj, &["result", "ingredients"], &format!("{id}.json"))?;
    let result = match obj.get("result").and_then(Value::as_str) {
        Some(result) if valid_name(result) => result.to_string(),
        Some(_) => return Err(format!("{id}.json result must be an id")),
        None => return Err(format!("{id}.json needs a result")),
    };
    if result != id {
        return Err(format!("{id}.json result is {result}, expected {id}"));
    }
    let list = obj
        .get("ingredients")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{id}.json ingredients must be an array"))?;
    if list.is_empty() || list.len() > 4 {
        return Err(format!("{id}.json needs 1 to 4 ingredients"));
    }
    let mut ingredients = Vec::with_capacity(list.len());
    for (index, ingredient) in list.iter().enumerate() {
        let Some(item) = ingredient.as_str() else {
            return Err(format!("{id}.json ingredient {index} must be a string"));
        };
        if !valid_name(item) || item == result {
            return Err(format!("{id}.json ingredient {item} is a bad id"));
        }
        ingredients.push(item.to_string());
    }
    Ok(PackRecipe {
        result,
        ingredients,
    })
}

fn read_data_pack(root: &Path) -> Result<Vec<BlockRule>, String> {
    let meta = root.join("pack.meta");
    if !meta.is_file() {
        return Err(format!("data pack missing pack.meta ({})", root.display()));
    }
    let dir = root.join("data").join("terralite").join("blocks");
    let entries = fs::read_dir(&dir).map_err(|_| format!("data pack missing {}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if path.is_file() && is_json {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(format!("data pack has no block files in {}", dir.display()));
    }
    let mut rules = Vec::with_capacity(paths.len());
    for path in paths {
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
            .ok_or_else(|| format!("bad data pack file {}", path.display()))?;
        if !valid_name(&id) {
            return Err(format!("bad data pack block id {id}"));
        }
        let text =
            fs::read_to_string(&path).map_err(|_| format!("unreadable {}", path.display()))?;
        rules.push(parse_block_rule(&id, &text)?);
    }
    Ok(rules)
}

fn parse_block_rule(id: &str, text: &str) -> Result<BlockRule, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("{id}.json: {err}"))?;
    let obj = object(&root, &format!("{id}.json"))?;
    require_only(obj, &["hardness", "replaces"], &format!("{id}.json"))?;
    let hardness = match obj.get("hardness") {
        Some(value) => positive_seconds(value, &format!("block {id} hardness"))?,
        None => return Err(format!("block {id} needs a hardness")),
    };
    let replaces = match obj.get("replaces") {
        None => None,
        Some(stone) => {
            let stone = block_name(Some(stone), &format!("block {id} replaces"))?;
            if stone == id {
                return Err(format!("block {id} replaces itself"));
            }
            Some(stone)
        }
    };
    Ok(BlockRule {
        id: id.to_string(),
        hardness,
        replaces,
    })
}

pub fn shape_of(block: Block) -> Shape<'static> {
    Pack::builtin().shape_of(block)
}

/// The item a break drops, and how many. A melon becomes four slices.
pub fn harvest_of(id: &str) -> (&str, u8) {
    if id == "melon" {
        ("melon_slice", 4)
    } else {
        (id, 1)
    }
}

/// Every item a break should spawn. Script `loot.block` wins, then a datapack
/// `loot/blocks/<id>.json` `drops` list, else [`harvest_of`].
pub fn break_loot_of(id: &str) -> Vec<(String, u8)> {
    let pack = Pack::builtin();
    if let Some(drops) = pack.script_block_loot(id) {
        return drops.into_iter().map(|drop| (drop, 1)).collect();
    }
    if let Some(drops) = load_block_loot_file(id) {
        return drops.into_iter().map(|drop| (drop, 1)).collect();
    }
    let (drop, count) = harvest_of(id);
    vec![(drop.to_string(), count)]
}

fn load_block_loot_file(id: &str) -> Option<Vec<String>> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return None;
    }
    let path = crate::datapacks::resolve_datapack_file(&format!(
        "data/terralite/loot/blocks/{id}.json"
    ))?;
    let text = fs::read_to_string(path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let list = root.get("drops")?.as_array()?;
    if list.is_empty() || list.len() > 16 {
        return None;
    }
    let pack = Pack::builtin();
    let mut drops = Vec::with_capacity(list.len());
    for entry in list {
        let id = entry.as_str()?;
        if !pack.contains_item(id) && pack.tool_icon(id).is_none() {
            return None;
        }
        drops.push(id.to_string());
    }
    Some(drops)
}

/// A bitten cake, one sixteenth pair narrower per slice already eaten.
fn cake_slice(metadata: u8) -> Shape<'static> {
    const BITTEN: [CellBox; 5] = [
        CellBox {
            min: [3, 0, 1],
            max: [15, 8, 15],
        },
        CellBox {
            min: [5, 0, 1],
            max: [15, 8, 15],
        },
        CellBox {
            min: [7, 0, 1],
            max: [15, 8, 15],
        },
        CellBox {
            min: [9, 0, 1],
            max: [15, 8, 15],
        },
        CellBox {
            min: [11, 0, 1],
            max: [15, 8, 15],
        },
    ];
    let index = (metadata as usize).saturating_sub(1).min(BITTEN.len() - 1);
    Shape::from_boxes(std::slice::from_ref(&BITTEN[index]))
}

/// An iron pot. Metadata `1` through `3` adds a water line inside.
fn cauldron_shape(metadata: u8) -> Shape<'static> {
    const EMPTY: [CellBox; 5] = [
        CellBox {
            min: [2, 0, 2],
            max: [14, 3, 14],
        },
        CellBox {
            min: [2, 3, 2],
            max: [14, 12, 4],
        },
        CellBox {
            min: [2, 3, 12],
            max: [14, 12, 14],
        },
        CellBox {
            min: [2, 3, 4],
            max: [4, 12, 12],
        },
        CellBox {
            min: [12, 3, 4],
            max: [14, 12, 12],
        },
    ];
    const ONE: [CellBox; 6] = [
        CellBox {
            min: [2, 0, 2],
            max: [14, 3, 14],
        },
        CellBox {
            min: [2, 3, 2],
            max: [14, 12, 4],
        },
        CellBox {
            min: [2, 3, 12],
            max: [14, 12, 14],
        },
        CellBox {
            min: [2, 3, 4],
            max: [4, 12, 12],
        },
        CellBox {
            min: [12, 3, 4],
            max: [14, 12, 12],
        },
        CellBox {
            min: [4, 3, 4],
            max: [12, 5, 12],
        },
    ];
    const TWO: [CellBox; 6] = [
        CellBox {
            min: [2, 0, 2],
            max: [14, 3, 14],
        },
        CellBox {
            min: [2, 3, 2],
            max: [14, 12, 4],
        },
        CellBox {
            min: [2, 3, 12],
            max: [14, 12, 14],
        },
        CellBox {
            min: [2, 3, 4],
            max: [4, 12, 12],
        },
        CellBox {
            min: [12, 3, 4],
            max: [14, 12, 12],
        },
        CellBox {
            min: [4, 3, 4],
            max: [12, 8, 12],
        },
    ];
    const THREE: [CellBox; 6] = [
        CellBox {
            min: [2, 0, 2],
            max: [14, 3, 14],
        },
        CellBox {
            min: [2, 3, 2],
            max: [14, 12, 4],
        },
        CellBox {
            min: [2, 3, 12],
            max: [14, 12, 14],
        },
        CellBox {
            min: [2, 3, 4],
            max: [4, 12, 12],
        },
        CellBox {
            min: [12, 3, 4],
            max: [14, 12, 12],
        },
        CellBox {
            min: [4, 3, 4],
            max: [12, 10, 12],
        },
    ];
    match metadata {
        1 => Shape::from_boxes(&ONE),
        2 => Shape::from_boxes(&TWO),
        3.. => Shape::from_boxes(&THREE),
        _ => Shape::from_boxes(&EMPTY),
    }
}

/// A hanging bell. Metadata `1` swings the body to one side.
fn bell_shape(metadata: u8) -> Shape<'static> {
    const REST: [CellBox; 4] = [
        CellBox {
            min: [2, 13, 7],
            max: [14, 15, 9],
        },
        CellBox {
            min: [6, 5, 6],
            max: [10, 13, 10],
        },
        CellBox {
            min: [5, 3, 5],
            max: [11, 6, 11],
        },
        CellBox {
            min: [7, 6, 7],
            max: [9, 9, 9],
        },
    ];
    const SWUNG: [CellBox; 4] = [
        CellBox {
            min: [2, 13, 7],
            max: [14, 15, 9],
        },
        CellBox {
            min: [8, 5, 6],
            max: [12, 13, 10],
        },
        CellBox {
            min: [7, 3, 5],
            max: [13, 6, 11],
        },
        CellBox {
            min: [9, 6, 7],
            max: [11, 9, 9],
        },
    ];
    if metadata & 1 == 1 {
        Shape::from_boxes(&SWUNG)
    } else {
        Shape::from_boxes(&REST)
    }
}

/// A thin candle. Metadata `1` adds the flame.
fn candle_shape(metadata: u8) -> Shape<'static> {
    const WAX: [CellBox; 2] = [
        CellBox {
            min: [7, 0, 7],
            max: [9, 6, 9],
        },
        CellBox {
            min: [7, 6, 7],
            max: [9, 8, 9],
        },
    ];
    const LIT: [CellBox; 3] = [
        CellBox {
            min: [7, 0, 7],
            max: [9, 6, 9],
        },
        CellBox {
            min: [7, 6, 7],
            max: [9, 8, 9],
        },
        CellBox {
            min: [6, 8, 6],
            max: [10, 13, 10],
        },
    ];
    if metadata & 1 == 1 {
        Shape::from_boxes(&LIT)
    } else {
        Shape::from_boxes(&WAX)
    }
}

/// A respawn anchor. Metadata `1` to `4` raises the charge inside the cradle.
fn anchor_shape(metadata: u8) -> Shape<'static> {
    const EMPTY: [CellBox; 2] = [
        CellBox {
            min: [1, 0, 1],
            max: [15, 10, 15],
        },
        CellBox {
            min: [0, 10, 0],
            max: [16, 13, 16],
        },
    ];
    const ONE: [CellBox; 3] = [
        CellBox {
            min: [1, 0, 1],
            max: [15, 10, 15],
        },
        CellBox {
            min: [0, 10, 0],
            max: [16, 13, 16],
        },
        CellBox {
            min: [4, 4, 4],
            max: [12, 6, 12],
        },
    ];
    const TWO: [CellBox; 3] = [
        CellBox {
            min: [1, 0, 1],
            max: [15, 10, 15],
        },
        CellBox {
            min: [0, 10, 0],
            max: [16, 13, 16],
        },
        CellBox {
            min: [4, 4, 4],
            max: [12, 8, 12],
        },
    ];
    const THREE: [CellBox; 3] = [
        CellBox {
            min: [1, 0, 1],
            max: [15, 10, 15],
        },
        CellBox {
            min: [0, 10, 0],
            max: [16, 13, 16],
        },
        CellBox {
            min: [4, 4, 4],
            max: [12, 10, 12],
        },
    ];
    const FOUR: [CellBox; 3] = [
        CellBox {
            min: [1, 0, 1],
            max: [15, 10, 15],
        },
        CellBox {
            min: [0, 10, 0],
            max: [16, 13, 16],
        },
        CellBox {
            min: [4, 4, 4],
            max: [12, 12, 12],
        },
    ];
    match metadata {
        1 => Shape::from_boxes(&ONE),
        2 => Shape::from_boxes(&TWO),
        3 => Shape::from_boxes(&THREE),
        4.. => Shape::from_boxes(&FOUR),
        _ => Shape::from_boxes(&EMPTY),
    }
}

fn anchor_light(block: Block) -> Option<Light> {
    if block.id != "respawn_anchor" {
        return None;
    }
    let level = match block.metadata {
        1 => 4,
        2 => 8,
        3 => 12,
        4.. => 15,
        _ => return None,
    };
    Some(Light {
        color: [0.65, 0.2, 0.95],
        level,
    })
}

/// A thin copper rod. Bit 8 is the strike flash. The low bits pick the axis.
fn rod_shape(metadata: u8) -> Shape<'static> {
    let struck = metadata & 8 != 0;
    match metadata & 7 {
        1 | 2 => {
            if struck {
                const LIT: [CellBox; 3] = [
                    CellBox {
                        min: [0, 7, 7],
                        max: [14, 9, 9],
                    },
                    CellBox {
                        min: [14, 6, 6],
                        max: [16, 10, 10],
                    },
                    CellBox {
                        min: [12, 5, 5],
                        max: [16, 11, 11],
                    },
                ];
                Shape::from_boxes(&LIT)
            } else {
                const REST: [CellBox; 2] = [
                    CellBox {
                        min: [0, 7, 7],
                        max: [14, 9, 9],
                    },
                    CellBox {
                        min: [14, 6, 6],
                        max: [16, 10, 10],
                    },
                ];
                Shape::from_boxes(&REST)
            }
        }
        3 | 4 => {
            if struck {
                const LIT: [CellBox; 3] = [
                    CellBox {
                        min: [7, 7, 0],
                        max: [9, 9, 14],
                    },
                    CellBox {
                        min: [6, 6, 14],
                        max: [10, 10, 16],
                    },
                    CellBox {
                        min: [5, 5, 12],
                        max: [11, 11, 16],
                    },
                ];
                Shape::from_boxes(&LIT)
            } else {
                const REST: [CellBox; 2] = [
                    CellBox {
                        min: [7, 7, 0],
                        max: [9, 9, 14],
                    },
                    CellBox {
                        min: [6, 6, 14],
                        max: [10, 10, 16],
                    },
                ];
                Shape::from_boxes(&REST)
            }
        }
        _ => {
            if struck {
                const LIT: [CellBox; 3] = [
                    CellBox {
                        min: [7, 0, 7],
                        max: [9, 14, 9],
                    },
                    CellBox {
                        min: [6, 14, 6],
                        max: [10, 16, 10],
                    },
                    CellBox {
                        min: [5, 12, 5],
                        max: [11, 16, 11],
                    },
                ];
                Shape::from_boxes(&LIT)
            } else {
                const REST: [CellBox; 2] = [
                    CellBox {
                        min: [7, 0, 7],
                        max: [9, 14, 9],
                    },
                    CellBox {
                        min: [6, 14, 6],
                        max: [10, 16, 10],
                    },
                ];
                Shape::from_boxes(&REST)
            }
        }
    }
}

/// A cage. Metadata 1 sets a core in the middle while a wave is running.
fn spawner_shape(metadata: u8) -> Shape<'static> {
    const CAGE: [CellBox; 6] = [
        CellBox {
            min: [2, 0, 2],
            max: [14, 2, 14],
        },
        CellBox {
            min: [2, 14, 2],
            max: [14, 16, 14],
        },
        CellBox {
            min: [2, 2, 2],
            max: [4, 14, 4],
        },
        CellBox {
            min: [12, 2, 2],
            max: [14, 14, 4],
        },
        CellBox {
            min: [2, 2, 12],
            max: [4, 14, 14],
        },
        CellBox {
            min: [12, 2, 12],
            max: [14, 14, 14],
        },
    ];
    if metadata & 1 != 0 {
        const LIT: [CellBox; 7] = [
            CAGE[0],
            CAGE[1],
            CAGE[2],
            CAGE[3],
            CAGE[4],
            CAGE[5],
            CellBox {
                min: [6, 6, 6],
                max: [10, 10, 10],
            },
        ];
        Shape::from_boxes(&LIT)
    } else {
        Shape::from_boxes(&CAGE)
    }
}

/// A hay board with a red bullseye. Metadata 1 pushes the eye out for a hit.
fn target_shape(metadata: u8) -> Shape<'static> {
    const BOARD: CellBox = CellBox {
        min: [0, 0, 2],
        max: [16, 16, 14],
    };
    const RING: CellBox = CellBox {
        min: [2, 2, 0],
        max: [14, 14, 3],
    };
    const INNER: CellBox = CellBox {
        min: [5, 5, 0],
        max: [11, 11, 4],
    };
    const EYE: CellBox = CellBox {
        min: [7, 7, 0],
        max: [9, 9, 5],
    };
    if metadata & 1 != 0 {
        const HIT: [CellBox; 5] = [
            BOARD,
            RING,
            INNER,
            EYE,
            CellBox {
                min: [6, 6, 0],
                max: [10, 10, 8],
            },
        ];
        Shape::from_boxes(&HIT)
    } else {
        const REST: [CellBox; 4] = [BOARD, RING, INNER, EYE];
        Shape::from_boxes(&REST)
    }
}

/// A blocky copper lamp. Metadata `1` adds the lit core.
fn bulb_shape(metadata: u8) -> Shape<'static> {
    const DARK: [CellBox; 4] = [
        CellBox {
            min: [4, 0, 4],
            max: [12, 3, 12],
        },
        CellBox {
            min: [2, 3, 2],
            max: [14, 12, 14],
        },
        CellBox {
            min: [4, 12, 4],
            max: [12, 14, 12],
        },
        CellBox {
            min: [5, 14, 5],
            max: [11, 16, 11],
        },
    ];
    if metadata & 1 == 1 {
        const LIT: [CellBox; 5] = [
            CellBox {
                min: [4, 0, 4],
                max: [12, 3, 12],
            },
            CellBox {
                min: [2, 3, 2],
                max: [14, 12, 14],
            },
            CellBox {
                min: [4, 12, 4],
                max: [12, 14, 12],
            },
            CellBox {
                min: [5, 14, 5],
                max: [11, 16, 11],
            },
            CellBox {
                min: [5, 5, 5],
                max: [11, 10, 11],
            },
        ];
        Shape::from_boxes(&LIT)
    } else {
        Shape::from_boxes(&DARK)
    }
}

/// A thin wall frame. Metadata low bits are the wall, and bit 8 adds the shown item.
fn frame_shape(metadata: u8) -> Shape<'static> {
    const WEST: [CellBox; 1] = [CellBox {
        min: [0, 2, 2],
        max: [1, 14, 14],
    }];
    const WEST_HELD: [CellBox; 2] = [
        CellBox {
            min: [0, 2, 2],
            max: [1, 14, 14],
        },
        CellBox {
            min: [1, 5, 5],
            max: [4, 11, 11],
        },
    ];
    const EAST: [CellBox; 1] = [CellBox {
        min: [15, 2, 2],
        max: [16, 14, 14],
    }];
    const EAST_HELD: [CellBox; 2] = [
        CellBox {
            min: [15, 2, 2],
            max: [16, 14, 14],
        },
        CellBox {
            min: [12, 5, 5],
            max: [15, 11, 11],
        },
    ];
    const NORTH: [CellBox; 1] = [CellBox {
        min: [2, 2, 0],
        max: [14, 14, 1],
    }];
    const NORTH_HELD: [CellBox; 2] = [
        CellBox {
            min: [2, 2, 0],
            max: [14, 14, 1],
        },
        CellBox {
            min: [5, 5, 1],
            max: [11, 11, 4],
        },
    ];
    const SOUTH: [CellBox; 1] = [CellBox {
        min: [2, 2, 15],
        max: [14, 14, 16],
    }];
    const SOUTH_HELD: [CellBox; 2] = [
        CellBox {
            min: [2, 2, 15],
            max: [14, 14, 16],
        },
        CellBox {
            min: [5, 5, 12],
            max: [11, 11, 15],
        },
    ];
    let filled = metadata & 8 != 0;
    match (metadata & 7, filled) {
        (2, false) => Shape::from_boxes(&EAST),
        (2, true) => Shape::from_boxes(&EAST_HELD),
        (3, false) => Shape::from_boxes(&NORTH),
        (3, true) => Shape::from_boxes(&NORTH_HELD),
        (4, false) => Shape::from_boxes(&SOUTH),
        (4, true) => Shape::from_boxes(&SOUTH_HELD),
        (_, true) => Shape::from_boxes(&WEST_HELD),
        _ => Shape::from_boxes(&WEST),
    }
}

/// Pack a wall facing (`1` to `4`) and a stack count (`0` to `3`) into four bits.
pub fn shelf_meta(facing: u8, filled: u8) -> u8 {
    let face = facing.saturating_sub(1) & 3;
    face | (filled.min(3) << 2)
}

/// The wall a shelf hangs on, as a painting facing (`1` to `4`).
pub fn shelf_facing(metadata: u8) -> u8 {
    (metadata & 3) + 1
}

/// How many stacks sit on the shelf, `0` to `3`.
pub fn shelf_filled(metadata: u8) -> u8 {
    (metadata >> 2) & 3
}

/// A thin board on a wall. The nibble holds the wall and up to three stacks.
fn board_shape(metadata: u8) -> Shape<'static> {
    const WEST: [CellBox; 4] = [
        CellBox {
            min: [0, 7, 1],
            max: [8, 9, 15],
        },
        CellBox {
            min: [1, 9, 2],
            max: [4, 13, 5],
        },
        CellBox {
            min: [1, 9, 6],
            max: [4, 13, 9],
        },
        CellBox {
            min: [1, 9, 11],
            max: [4, 13, 14],
        },
    ];
    const EAST: [CellBox; 4] = [
        CellBox {
            min: [8, 7, 1],
            max: [16, 9, 15],
        },
        CellBox {
            min: [12, 9, 2],
            max: [15, 13, 5],
        },
        CellBox {
            min: [12, 9, 6],
            max: [15, 13, 9],
        },
        CellBox {
            min: [12, 9, 11],
            max: [15, 13, 14],
        },
    ];
    const NORTH: [CellBox; 4] = [
        CellBox {
            min: [1, 7, 0],
            max: [15, 9, 8],
        },
        CellBox {
            min: [2, 9, 1],
            max: [5, 13, 4],
        },
        CellBox {
            min: [6, 9, 1],
            max: [9, 13, 4],
        },
        CellBox {
            min: [11, 9, 1],
            max: [14, 13, 4],
        },
    ];
    const SOUTH: [CellBox; 4] = [
        CellBox {
            min: [1, 7, 8],
            max: [15, 9, 16],
        },
        CellBox {
            min: [2, 9, 12],
            max: [5, 13, 15],
        },
        CellBox {
            min: [6, 9, 12],
            max: [9, 13, 15],
        },
        CellBox {
            min: [11, 9, 12],
            max: [14, 13, 15],
        },
    ];
    let count = usize::from(shelf_filled(metadata));
    match shelf_facing(metadata) {
        2 => Shape::from_boxes(&EAST[..=count]),
        3 => Shape::from_boxes(&NORTH[..=count]),
        4 => Shape::from_boxes(&SOUTH[..=count]),
        _ => Shape::from_boxes(&WEST[..=count]),
    }
}

/// A shelf case plus one front spine for each stored book, up to six.
fn shelf_shape(metadata: u8) -> Shape<'static> {
    const ALL: [CellBox; 7] = [
        CellBox {
            min: [0, 0, 2],
            max: [16, 16, 16],
        },
        CellBox {
            min: [0, 3, 0],
            max: [2, 13, 3],
        },
        CellBox {
            min: [3, 3, 0],
            max: [5, 13, 3],
        },
        CellBox {
            min: [6, 3, 0],
            max: [8, 13, 3],
        },
        CellBox {
            min: [9, 3, 0],
            max: [11, 13, 3],
        },
        CellBox {
            min: [12, 3, 0],
            max: [14, 13, 3],
        },
        CellBox {
            min: [14, 3, 0],
            max: [16, 13, 3],
        },
    ];
    let count = usize::from(metadata.min(6));
    Shape::from_boxes(&ALL[..=count])
}

/// A clay vessel. Metadata `1` shows one stored stack in the mouth.
fn vessel_shape(metadata: u8) -> Shape<'static> {
    const EMPTY: [CellBox; 4] = [
        CellBox {
            min: [4, 0, 4],
            max: [12, 2, 12],
        },
        CellBox {
            min: [2, 2, 2],
            max: [14, 11, 14],
        },
        CellBox {
            min: [5, 11, 5],
            max: [11, 13, 11],
        },
        CellBox {
            min: [3, 13, 3],
            max: [13, 15, 13],
        },
    ];
    if metadata & 1 == 1 {
        const HELD: [CellBox; 5] = [
            CellBox {
                min: [4, 0, 4],
                max: [12, 2, 12],
            },
            CellBox {
                min: [2, 2, 2],
                max: [14, 11, 14],
            },
            CellBox {
                min: [5, 11, 5],
                max: [11, 13, 11],
            },
            CellBox {
                min: [3, 13, 3],
                max: [13, 15, 13],
            },
            CellBox {
                min: [6, 13, 6],
                max: [10, 16, 10],
            },
        ];
        Shape::from_boxes(&HELD)
    } else {
        Shape::from_boxes(&EMPTY)
    }
}

/// Boxes a body, a look ray, and a place check hit.
///
/// Glass is drawn as an open rim, so its shape does not fill the cell. The
/// collider is still the whole cell.
pub fn for_each_collider(block: Block, mut visit: impl FnMut([f32; 3], [f32; 3])) {
    if block.id == "glass" {
        visit([0.0, 0.0, 0.0], [16.0, 16.0, 16.0]);
        return;
    }
    // Portal planes are walk-through — travel detects the cell instead.
    if block.id == "nether_portal" {
        return;
    }
    shape_of(block).for_each_solid(visit);
}

pub fn light_of(block: Block) -> Option<Light> {
    Pack::builtin().light_of(block)
}

/// Light a held placeable would cast around the player.
///
/// Uses the block the item places when it is a script placeable (`ember:ember`
/// → `ember:glow_brick`). Only the default placed state counts, so an unlit
/// candle or copper bulb stays dark in hand, while a torch, lantern, or
/// glow brick lights the air.
pub fn held_light_of(pack: &Pack, item_id: &str) -> Option<Light> {
    if item_id.is_empty() || item_id == "air" {
        return None;
    }
    let block_id = if pack.contains_block(item_id) {
        item_id
    } else if let Some(placed) = pack.placed_block(item_id) {
        return pack.light_of(Block::new(&placed, 0));
    } else {
        return None;
    };
    pack.light_of(Block::new(block_id, 0))
}

fn parse_shapes(value: &Value) -> Result<Registry<ShapeEntry>, String> {
    let shapes = object(value, "shapes")?;
    let mut parsed = Registry::new();
    for (name, value) in shapes {
        if !valid_name(name) {
            return Err(format!("bad shape name {name}"));
        }
        let shape = object(value, &format!("shape {name}"))?;
        require_only(shape, &["boxes"], &format!("shape {name}"))?;
        let boxes = shape
            .get("boxes")
            .ok_or_else(|| format!("shape {name} has no boxes"))?
            .as_array()
            .ok_or_else(|| format!("shape {name} boxes must be an array"))?;
        if boxes.is_empty() {
            return Err(format!("shape {name} has no boxes"));
        }
        let mut cell_boxes = Vec::with_capacity(boxes.len());
        for (index, box_value) in boxes.iter().enumerate() {
            cell_boxes.push(parse_box(name, index, box_value)?);
        }
        parsed
            .register(
                name.clone(),
                ShapeEntry {
                    boxes: cell_boxes,
                    planes: Vec::new(),
                    faces: Vec::new(),
                    textures: Vec::new(),
                    gui: ItemDisplay::GUI_BLOCK,
                },
            )
            .map_err(|_| format!("duplicate shape {name}"))?;
    }
    Ok(parsed)
}

fn parse_box(shape: &str, index: usize, value: &Value) -> Result<CellBox, String> {
    let coords = value
        .as_array()
        .ok_or_else(|| format!("shape {shape} box {index} must be 6 numbers"))?;
    if coords.len() != 6 {
        return Err(format!("shape {shape} box {index} needs 6 numbers"));
    }
    let mut nums = [0u8; 6];
    for (axis, coord) in coords.iter().enumerate() {
        let number = coord
            .as_u64()
            .ok_or_else(|| format!("shape {shape} box {index} has a bad number"))?;
        if number > 16 {
            return Err(format!("shape {shape} box coordinate {number} is above 16"));
        }
        nums[axis] = number as u8;
    }
    let min = [nums[0], nums[1], nums[2]];
    let max = [nums[3], nums[4], nums[5]];
    if min.iter().zip(max).any(|(start, end)| *start >= end) {
        return Err(format!(
            "shape {shape} box {index} min must be less than max"
        ));
    }
    Ok(CellBox { min, max })
}

fn parse_blocks(
    value: &Value,
    shapes: &Registry<ShapeEntry>,
    models: &mut Registry<ShapeEntry>,
) -> Result<Registry<BlockEntry>, String> {
    let entries = value
        .as_array()
        .ok_or_else(|| "blocks must be an array".to_string())?;
    let mut blocks = Registry::new();
    for (index, entry) in entries.iter().enumerate() {
        let obj = object(entry, &format!("block {index}"))?;
        require_only(
            obj,
            &[
                "id", "shape", "model", "replaces", "light", "texture", "hardness",
            ],
            &format!("block {index}"),
        )?;
        let id = block_name(obj.get("id"), &format!("block {index} id"))?;
        if blocks.contains(&id) {
            return Err(format!("duplicate block {id}"));
        }
        let geometry = block_geometry(obj, &id, shapes, models)?;
        let replaces = match obj.get("replaces") {
            None => None,
            Some(stone) => {
                let stone = block_name(Some(stone), &format!("block {id} replaces"))?;
                if stone == id {
                    return Err(format!("block {id} replaces itself"));
                }
                Some(stone)
            }
        };
        let light = match obj.get("light") {
            None => None,
            Some(value) => Some(parse_light(value, &id)?),
        };
        let texture = parse_texture(obj, &id)?;
        let hardness = parse_hardness(obj, &id)?;
        blocks
            .register(
                id,
                BlockEntry {
                    geometry,
                    replaces,
                    light,
                    texture,
                    hardness,
                },
            )
            .expect("duplicate block was already rejected");
    }
    for (id, block) in blocks.iter() {
        let Some(stone) = block.replaces() else {
            continue;
        };
        if !blocks.contains(stone) {
            return Err(format!(
                "block {id} replaces {stone}, which is not in the pack"
            ));
        }
    }
    Ok(blocks)
}

fn block_geometry(
    obj: &serde_json::Map<String, Value>,
    id: &str,
    shapes: &Registry<ShapeEntry>,
    models: &mut Registry<ShapeEntry>,
) -> Result<Geometry, String> {
    let has_shape = obj.contains_key("shape");
    let has_model = obj.contains_key("model");
    if has_shape && has_model {
        return Err(format!("block {id} has both a shape and a model"));
    }
    if !has_shape && !has_model {
        return Err(format!("block {id} needs a shape or a model"));
    }
    if has_shape {
        let shape = obj
            .get("shape")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("block {id} shape must be a string"))?;
        if !shapes.contains(shape) {
            return Err(format!("unknown shape {shape}"));
        }
        return Ok(Geometry::Shape(shape.to_string()));
    }
    let path = obj
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("block {id} model must be a string"))?;
    if path.is_empty() {
        return Err(format!("block {id} model path is empty"));
    }
    if !models.contains(path) {
        models
            .register(path, load_model(path)?)
            .map_err(|_| format!("duplicate model {path}"))?;
    }
    Ok(Geometry::Model(path.to_string()))
}

fn model_file(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.ends_with(".json") || name.ends_with(".bbmodel")
}

fn load_model(path: &str) -> Result<ShapeEntry, String> {
    if !model_file(path) {
        return Err(format!("{path} must be a .json or a .bbmodel"));
    }
    let text = read_text_asset(path).ok_or_else(|| format!("unknown model {path}"))?;
    parse_model_in(&text, path).map_err(|err| format!("{path}: {err}"))
}

/// JSON text from overlays, then the resource pack, then each `mods/<name>/`.
fn read_text_asset(path: &str) -> Option<String> {
    if let Some(bytes) = crate::resource_packs::overlay_bytes(path) {
        if let Ok(text) = String::from_utf8(bytes) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    if let Some(file) = pack_file(path) {
        if let Ok(text) = fs::read_to_string(file) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    let mods = Path::new(r"C:\CODE\TerraLite\mods");
    let entries = fs::read_dir(mods).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(file) = join_inside(&dir, path) else {
            continue;
        };
        if let Ok(text) = fs::read_to_string(file) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

#[cfg(test)]
fn parse_model(text: &str) -> Result<ShapeEntry, String> {
    parse_model_in(text, "")
}

fn parse_model_in(text: &str, model_path: &str) -> Result<ShapeEntry, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let root = object(&root, "model")?;
    let textures = parse_model_textures(root, model_path)?;
    let texture_size = model_texture_size(root);
    let elements = root
        .get("elements")
        .ok_or_else(|| "model has no elements".to_string())?
        .as_array()
        .ok_or_else(|| "model elements must be an array".to_string())?;
    if elements.is_empty() {
        return Err("model has no elements".to_string());
    }
    let mut boxes = Vec::new();
    let mut planes = Vec::new();
    let mut faces = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let obj = object(element, &format!("element {index}"))?;
        let kind = match obj.get("type") {
            None => "cube",
            Some(kind) => kind
                .as_str()
                .ok_or_else(|| format!("element {index} type must be a string"))?,
        };
        if kind == "mesh" {
            parse_mesh_element(obj, index, &textures, &mut boxes, &mut faces)?;
            continue;
        }
        if kind != "cube" {
            return Err(format!("element {index} type {kind} is not a cube"));
        }
        let from = element_point(obj.get("from"), &format!("element {index} from"))?;
        let to = element_point(obj.get("to"), &format!("element {index} to"))?;
        for axis in 0..3 {
            if from[axis] > to[axis] {
                return Err(format!("element {index} from must not pass to"));
            }
        }
        let origin = match obj.get("origin") {
            None => [0.0; 3],
            Some(value) => model_point(value, &format!("element {index} origin"))?,
        };
        let rotation = match obj.get("rotation") {
            None => [0.0; 3],
            Some(value) => model_point(value, &format!("element {index} rotation"))?,
        };
        if rotation.iter().any(|degrees| degrees.abs() > 0.01) {
            push_rotated_cube(index, from, to, origin, rotation, &mut boxes, &mut faces)?;
            continue;
        }
        let mut flat = None;
        let mut flat_count = 0u8;
        for axis in 0..3 {
            if from[axis] == to[axis] {
                flat = Some(axis as u8);
                flat_count += 1;
            }
        }
        match flat_count {
            0 => boxes.push(CellBox { min: from, max: to }),
            1 => {
                let axis = flat.expect("one flat axis");
                let mut min = [0u8; 2];
                let mut max = [0u8; 2];
                let mut uv = 0;
                for axis_i in 0..3 {
                    if axis_i == axis as usize {
                        continue;
                    }
                    min[uv] = from[axis_i];
                    max[uv] = to[axis_i];
                    uv += 1;
                }
                planes.push(Plane {
                    axis,
                    at: from[axis as usize],
                    min,
                    max,
                });
            }
            _ => return Err(format!("element {index} is not a cube or a plane")),
        }
        if !textures.is_empty() {
            push_textured_cube_faces(
                index,
                obj,
                from,
                to,
                &textures,
                texture_size,
                &mut faces,
            )?;
        }
    }
    let gui = parse_display_gui(root).unwrap_or(ItemDisplay::GUI_BLOCK);
    Ok(ShapeEntry {
        boxes,
        planes,
        faces,
        textures: textures.into_iter().map(|texture| texture.path).collect(),
        gui,
    })
}

/// Read Minecraft / Blockbench `display.gui` when present.
fn parse_display_gui(root: &serde_json::Map<String, Value>) -> Option<ItemDisplay> {
    let gui = root
        .get("display")
        .and_then(Value::as_object)?
        .get("gui")
        .and_then(Value::as_object)?;
    let rotation = display_vec3(gui.get("rotation")).unwrap_or(ItemDisplay::GUI_BLOCK.rotation);
    let translation =
        display_vec3(gui.get("translation")).unwrap_or(ItemDisplay::GUI_BLOCK.translation);
    let scale = display_vec3(gui.get("scale")).unwrap_or(ItemDisplay::GUI_BLOCK.scale);
    Some(ItemDisplay {
        rotation,
        translation,
        scale,
    })
}

fn display_vec3(value: Option<&Value>) -> Option<[f32; 3]> {
    let coords = value?.as_array()?;
    if coords.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for axis in 0..3 {
        out[axis] = coords[axis].as_f64()? as f32;
    }
    Some(out)
}

#[derive(Clone)]
struct ModelTexture {
    path: String,
    size: [f32; 2],
}

fn model_texture_size(root: &serde_json::Map<String, Value>) -> [f32; 2] {
    let Some(pair) = root.get("texture_size").and_then(Value::as_array) else {
        return [16.0, 16.0];
    };
    if pair.len() != 2 {
        return [16.0, 16.0];
    }
    let axis = |index: usize| {
        pair[index]
            .as_f64()
            .map(|value| value as f32)
            .filter(|value| *value >= 1.0)
            .unwrap_or(16.0)
    };
    [axis(0), axis(1)]
}

fn parse_model_textures(
    root: &serde_json::Map<String, Value>,
    model_path: &str,
) -> Result<Vec<ModelTexture>, String> {
    match root.get("textures") {
        Some(Value::Object(map)) => return parse_java_textures(map),
        Some(Value::Array(_)) => {}
        _ => return Ok(Vec::new()),
    }
    let Some(list) = root.get("textures").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut textures = Vec::new();
    for (index, texture) in list.iter().enumerate() {
        let obj = object(texture, &format!("texture {index}"))?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        let relative = obj.get("relative_path").and_then(Value::as_str);
        let path = resolve_model_texture(model_path, name, relative)
            .ok_or_else(|| format!("model texture {name} was not found"))?;
        let id = texture_id(obj.get("id")).unwrap_or(index);
        let width = obj
            .get("uv_width")
            .or_else(|| obj.get("width"))
            .and_then(Value::as_f64)
            .unwrap_or(16.0) as f32;
        let height = obj
            .get("uv_height")
            .or_else(|| obj.get("height"))
            .and_then(Value::as_f64)
            .unwrap_or(16.0) as f32;
        if textures.len() <= id {
            textures.resize(
                id + 1,
                ModelTexture {
                    path: String::new(),
                    size: [16.0, 16.0],
                },
            );
        }
        textures[id] = ModelTexture {
            path,
            size: [width.max(1.0), height.max(1.0)],
        };
    }
    Ok(textures)
}

fn texture_id(value: Option<&Value>) -> Option<usize> {
    match value {
        Some(Value::Number(number)) => number.as_u64().map(|id| id as usize),
        Some(Value::String(text)) => text.trim_start_matches('#').parse().ok(),
        _ => None,
    }
}

/// `"terralite:block/dirt"` is `assets/terralite/textures/block/dirt.png`.
pub fn namespaced_texture(name: &str) -> Option<String> {
    let (namespace, rest) = name.split_once(':').unwrap_or(("terralite", name));
    if namespace.is_empty() || rest.is_empty() || rest.contains("..") {
        return None;
    }
    let file = if rest.ends_with(".png") {
        rest.to_string()
    } else {
        format!("{rest}.png")
    };
    let path = format!("assets/{namespace}/textures/{file}");
    asset_png(&path).map(|_| path)
}

fn parse_java_textures(
    map: &serde_json::Map<String, Value>,
) -> Result<Vec<ModelTexture>, String> {
    let mut textures = Vec::new();
    for (key, value) in map {
        if key == "particle" {
            continue;
        }
        let id: usize = key
            .parse()
            .map_err(|_| format!("model texture {key} is not an index"))?;
        let name = value
            .as_str()
            .ok_or_else(|| format!("model texture {key} must be a name"))?;
        let path = namespaced_texture(name)
            .ok_or_else(|| format!("model texture {name} was not found"))?;
        if textures.len() <= id {
            textures.resize(
                id + 1,
                ModelTexture {
                    path: String::new(),
                    size: [16.0, 16.0],
                },
            );
        }
        textures[id] = ModelTexture {
            path,
            size: [16.0, 16.0],
        };
    }
    if textures.is_empty() || textures.iter().all(|texture| texture.path.is_empty()) {
        return Err("model textures did not resolve".into());
    }
    Ok(textures)
}

/// Cube faces from a Java-style model. Blockbench exports UV as
/// `editor * 16 / texture_size`, and these images are 16 pixels, so
/// `[0, 0, 4, 4]` on a 64 texture is the whole tile.
fn push_textured_cube_faces(
    index: usize,
    obj: &serde_json::Map<String, Value>,
    from: [u8; 3],
    to: [u8; 3],
    textures: &[ModelTexture],
    texture_size: [f32; 2],
    faces: &mut Vec<MeshFace>,
) -> Result<(), String> {
    let Some(listed) = obj.get("faces").and_then(Value::as_object) else {
        return Ok(());
    };
    let from = from.map(f32::from);
    let to = to.map(f32::from);
    for (name, spec) in listed {
        let Some((axis, at_max)) = cube_face_side(name) else {
            continue;
        };
        let spec = spec
            .as_object()
            .ok_or_else(|| format!("element {index} face {name} must be an object"))?;
        let tex = texture_id(spec.get("texture")).unwrap_or(0);
        if textures
            .get(tex)
            .map(|item| item.path.is_empty())
            .unwrap_or(true)
        {
            return Err(format!(
                "element {index} face {name} texture {tex} was not found"
            ));
        }
        let rect = match spec.get("uv") {
            None => [0.0, 0.0, 16.0, 16.0],
            Some(value) => uv_rect(value, &format!("element {index} face {name}"))?,
        };
        let corners = face_corners(from, to, axis, at_max).map(cube_to_cell);
        faces.push(mesh_face(
            index,
            corners,
            blockbench_face_uv(rect, texture_size),
            tex as u8,
        )?);
    }
    Ok(())
}

fn cube_face_side(name: &str) -> Option<(usize, bool)> {
    match name {
        "west" => Some((0, false)),
        "east" => Some((0, true)),
        "down" => Some((1, false)),
        "up" => Some((1, true)),
        "north" => Some((2, false)),
        "south" => Some((2, true)),
        _ => None,
    }
}

fn uv_rect(value: &Value, what: &str) -> Result<[f32; 4], String> {
    let coords = value
        .as_array()
        .ok_or_else(|| format!("{what} uv must be 4 numbers"))?;
    if coords.len() != 4 {
        return Err(format!("{what} uv must be 4 numbers"));
    }
    let mut rect = [0.0; 4];
    for index in 0..4 {
        rect[index] = coords[index]
            .as_f64()
            .ok_or_else(|| format!("{what} uv must be 4 numbers"))? as f32;
    }
    Ok(rect)
}

fn blockbench_face_uv(rect: [f32; 4], texture_size: [f32; 2]) -> [[f32; 2]; 4] {
    let map = |value: f32, axis: usize| value * texture_size[axis] / 16.0 / 16.0;
    let u0 = map(rect[0], 0);
    let v0 = map(rect[1], 1);
    let u1 = map(rect[2], 0);
    let v1 = map(rect[3], 1);
    [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
}

/// `../../textures/block/name.png` next to the model, then the block folder,
/// then the pack texture folder.
fn resolve_model_texture(model_path: &str, name: &str, relative: Option<&str>) -> Option<String> {
    if let Some(relative) = relative {
        if let Some(path) = join_model_relative(model_path, relative) {
            if asset_png(&path).is_some() {
                return Some(path);
            }
        }
    }
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if file.is_empty() {
        return None;
    }
    for candidate in [
        format!("assets/terralite/textures/block/{file}"),
        format!("assets/terralite/textures/{file}"),
    ] {
        if asset_png(&candidate).is_some() {
            return Some(candidate);
        }
    }
    None
}

fn join_model_relative(model_path: &str, relative: &str) -> Option<String> {
    if model_path.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = model_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    parts.pop();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if parts.pop().is_none() {
                return None;
            }
            continue;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// A Blockbench mesh. An axis-aligned cube that fills the cell is the solid
/// box, and its faces stay so they can use the model's textures. Rotated
/// quads are the grass fringe. Vertices are offsets from the element origin,
/// and the spin is Three.js `XYZ` (Z, then Y, then X), which is what a mesh
/// uses in Blockbench.
fn parse_mesh_element(
    obj: &serde_json::Map<String, Value>,
    index: usize,
    textures: &[ModelTexture],
    boxes: &mut Vec<CellBox>,
    faces: &mut Vec<MeshFace>,
) -> Result<(), String> {
    if obj.get("visibility").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }
    let origin = match obj.get("origin") {
        None => [0.0; 3],
        Some(value) => model_point(value, &format!("element {index} origin"))?,
    };
    let rotation = match obj.get("rotation") {
        None => [0.0; 3],
        Some(value) => model_point(value, &format!("element {index} rotation"))?,
    };
    let vertices = obj
        .get("vertices")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("element {index} mesh needs vertices"))?;
    let face_obj = obj
        .get("faces")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("element {index} mesh needs faces"))?;
    if vertices.is_empty() || face_obj.is_empty() {
        return Err(format!("element {index} mesh needs vertices"));
    }
    let mut quads = Vec::new();
    for (name, face) in face_obj {
        let face = face
            .as_object()
            .ok_or_else(|| format!("element {index} face {name} must be an object"))?;
        let ids = face
            .get("vertices")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("element {index} face {name} needs vertices"))?;
        if ids.len() != 4 {
            return Err(format!("element {index} face {name} needs 4 vertices"));
        }
        let tex = texture_id(face.get("texture")).unwrap_or(0);
        if !textures.is_empty()
            && textures
                .get(tex)
                .map(|item| item.path.is_empty())
                .unwrap_or(true)
        {
            return Err(format!(
                "element {index} face {name} texture {tex} was not found"
            ));
        }
        let size = textures
            .get(tex)
            .map(|item| item.size)
            .unwrap_or([16.0, 16.0]);
        let mut corners = [[0.0; 3]; 4];
        let mut uvs = [[0.0; 2]; 4];
        for (slot, id) in ids.iter().enumerate() {
            let id = id
                .as_str()
                .ok_or_else(|| format!("element {index} face {name} vertex must be a name"))?;
            let raw = vertices
                .get(id)
                .ok_or_else(|| format!("element {index} face {name} missing vertex {id}"))?;
            let local = model_point(raw, &format!("element {index} vertex {id}"))?;
            corners[slot] = model_to_cell(place_mesh(local, origin, rotation));
            uvs[slot] = face_uv(face, id, size, &format!("element {index} face {name}"))?;
        }
        quads.push((corners, uvs, tex as u8));
    }
    let still = rotation.iter().all(|degrees| degrees.abs() < 0.01);
    if still
        && is_unit_cube(
            &quads
                .iter()
                .map(|(corners, _, _)| *corners)
                .collect::<Vec<_>>(),
        )
    {
        boxes.push(CellBox {
            min: [0, 0, 0],
            max: [16, 16, 16],
        });
    }
    for (corners, uvs, tex) in quads {
        faces.push(mesh_face(index, corners, uvs, tex)?);
    }
    Ok(())
}

fn face_uv(
    face: &serde_json::Map<String, Value>,
    vertex: &str,
    size: [f32; 2],
    what: &str,
) -> Result<[f32; 2], String> {
    let Some(uvs) = face.get("uv").and_then(Value::as_object) else {
        return Ok([0.0, 0.0]);
    };
    let Some(value) = uvs.get(vertex) else {
        return Ok([0.0, 0.0]);
    };
    let coords = value
        .as_array()
        .ok_or_else(|| format!("{what} uv must be 2 numbers"))?;
    if coords.len() != 2 {
        return Err(format!("{what} uv must be 2 numbers"));
    }
    let mut point = [0.0; 2];
    for axis in 0..2 {
        point[axis] = coords[axis]
            .as_f64()
            .ok_or_else(|| format!("{what} uv must be 2 numbers"))? as f32
            / size[axis];
    }
    Ok(point)
}

fn is_unit_cube(quads: &[[[f32; 3]; 4]]) -> bool {
    let mut low = [false; 3];
    let mut high = [false; 3];
    for quad in quads {
        for corner in quad {
            for axis in 0..3 {
                let value = corner[axis];
                if value.abs() <= 0.02 {
                    low[axis] = true;
                } else if (value - 1.0).abs() <= 0.02 {
                    high[axis] = true;
                } else {
                    return false;
                }
            }
        }
    }
    low.iter().all(|seen| *seen) && high.iter().all(|seen| *seen)
}

fn mesh_face(
    index: usize,
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    tex: u8,
) -> Result<MeshFace, String> {
    let mut corner = [[0i16; 3]; 4];
    for slot in 0..4 {
        for axis in 0..3 {
            let scaled = corners[slot][axis] * 1024.0;
            if !scaled.is_finite() || scaled.abs() > 16_000.0 {
                return Err(format!("element {index} mesh vertex is outside the block"));
            }
            corner[slot][axis] = scaled.round() as i16;
        }
    }
    let mut uv = [[0u16; 2]; 4];
    for slot in 0..4 {
        for axis in 0..2 {
            let scaled = uvs[slot][axis] * 1024.0;
            if !scaled.is_finite() || !(0.0..=1024.0).contains(&scaled) {
                return Err(format!("element {index} mesh uv is outside the texture"));
            }
            uv[slot][axis] = scaled.round() as u16;
        }
    }
    Ok(MeshFace { corner, uv, tex })
}

fn model_point(value: &Value, what: &str) -> Result<[f32; 3], String> {
    let coords = value
        .as_array()
        .ok_or_else(|| format!("{what} must be 3 numbers"))?;
    if coords.len() != 3 {
        return Err(format!("{what} must be 3 numbers"));
    }
    let mut point = [0.0; 3];
    for axis in 0..3 {
        point[axis] = coords[axis]
            .as_f64()
            .ok_or_else(|| format!("{what} must be 3 numbers"))? as f32;
    }
    Ok(point)
}

/// A cube or plane (`from`/`to`) turned the way Blockbench shows it.
///
/// Cube geometry is stored relative to the pivot, and the format euler order
/// is `ZYX`: X, then Y, then Z. `from`/`to` are absolute, so the pivot is
/// `origin + R(point - origin)`.
fn place_cube(point: [f32; 3], origin: [f32; 3], rot: [f32; 3]) -> [f32; 3] {
    let local = [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ];
    let turned = spin_zyx(local, rot);
    [
        origin[0] + turned[0],
        origin[1] + turned[1],
        origin[2] + turned[2],
    ]
}

/// A mesh vertex turned the way Blockbench shows it.
///
/// Mesh vertices are offsets from the element origin, and a mesh leaves
/// Three.js on its default `XYZ` order: Z, then Y, then X. Adding the origin
/// after that spin is the pivot. Subtracting it again would swing the fringe
/// around the wrong point.
fn place_mesh(point: [f32; 3], origin: [f32; 3], rot: [f32; 3]) -> [f32; 3] {
    let turned = spin_xyz(point, rot);
    [
        origin[0] + turned[0],
        origin[1] + turned[1],
        origin[2] + turned[2],
    ]
}

fn model_to_cell(model: [f32; 3]) -> [f32; 3] {
    [
        (model[0] + 8.0) / 16.0,
        model[1] / 16.0,
        (model[2] + 8.0) / 16.0,
    ]
}

fn cube_to_cell(point: [f32; 3]) -> [f32; 3] {
    [point[0] / 16.0, point[1] / 16.0, point[2] / 16.0]
}

fn push_rotated_cube(
    index: usize,
    from: [u8; 3],
    to: [u8; 3],
    origin: [f32; 3],
    rotation: [f32; 3],
    boxes: &mut Vec<CellBox>,
    faces: &mut Vec<MeshFace>,
) -> Result<(), String> {
    let from = from.map(f32::from);
    let to = to.map(f32::from);
    let mut flat = None;
    for axis in 0..3 {
        if from[axis] == to[axis] {
            if flat.is_some() {
                return Err(format!("element {index} is not a cube or a plane"));
            }
            flat = Some(axis);
        }
    }
    let quads: Vec<[[f32; 3]; 4]> = match flat {
        Some(axis) => vec![face_corners(from, to, axis, false)],
        None => (0..3)
            .flat_map(|axis| {
                [
                    face_corners(from, to, axis, false),
                    face_corners(from, to, axis, true),
                ]
            })
            .collect(),
    };
    let mut cells = Vec::with_capacity(quads.len());
    for quad in quads {
        let corners = quad.map(|point| cube_to_cell(place_cube(point, origin, rotation)));
        cells.push(corners);
        faces.push(mesh_face(
            index,
            corners,
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0,
        )?);
    }
    if is_unit_cube(&cells) {
        boxes.push(CellBox {
            min: [0, 0, 0],
            max: [16, 16, 16],
        });
    }
    Ok(())
}

fn face_corners(from: [f32; 3], to: [f32; 3], axis: usize, at_max: bool) -> [[f32; 3]; 4] {
    let value = if at_max { to[axis] } else { from[axis] };
    let mut corner = [from; 4];
    for point in &mut corner {
        point[axis] = value;
    }
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    if at_max {
        corner[1][u] = to[u];
        corner[2][u] = to[u];
        corner[2][v] = to[v];
        corner[3][v] = to[v];
    } else {
        corner[1][v] = to[v];
        corner[2][u] = to[u];
        corner[2][v] = to[v];
        corner[3][u] = to[u];
    }
    corner
}

/// Three.js `ZYX`: turn around X, then Y, then Z. Blockbench cubes use this.
fn spin_zyx(point: [f32; 3], rot: [f32; 3]) -> [f32; 3] {
    let point = rot_x(point, rot[0].to_radians());
    let point = rot_y(point, rot[1].to_radians());
    rot_z(point, rot[2].to_radians())
}

/// Three.js `XYZ`: turn around Z, then Y, then X. Blockbench meshes use this.
fn spin_xyz(point: [f32; 3], rot: [f32; 3]) -> [f32; 3] {
    let point = rot_z(point, rot[2].to_radians());
    let point = rot_y(point, rot[1].to_radians());
    rot_x(point, rot[0].to_radians())
}

fn rot_x(point: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    [
        point[0],
        point[1] * c - point[2] * s,
        point[1] * s + point[2] * c,
    ]
}

fn rot_y(point: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    [
        point[0] * c + point[2] * s,
        point[1],
        -point[0] * s + point[2] * c,
    ]
}

fn rot_z(point: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    [
        point[0] * c - point[1] * s,
        point[0] * s + point[1] * c,
        point[2],
    ]
}

fn element_point(value: Option<&Value>, what: &str) -> Result<[u8; 3], String> {
    let Some(value) = value else {
        return Err(format!("{what} is missing"));
    };
    let coords = value
        .as_array()
        .ok_or_else(|| format!("{what} must be 3 numbers"))?;
    if coords.len() != 3 {
        return Err(format!("{what} needs 3 numbers"));
    }
    let mut point = [0u8; 3];
    for (axis, coord) in coords.iter().enumerate() {
        point[axis] = whole_sixteenth(coord, what)?;
    }
    Ok(point)
}

fn whole_sixteenth(value: &Value, what: &str) -> Result<u8, String> {
    let number = if let Some(number) = value.as_f64() {
        number
    } else if let Some(number) = value.as_i64() {
        number as f64
    } else {
        return Err(format!("{what} must be a number"));
    };
    if !number.is_finite() || (number - number.round()).abs() > 1.0e-4 {
        return Err(format!("{what} must be a whole number of sixteenths"));
    }
    let rounded = number.round() as i64;
    if !(0..=16).contains(&rounded) {
        return Err(format!("{what} {rounded} is outside 0..=16"));
    }
    Ok(rounded as u8)
}

fn parse_light(value: &Value, id: &str) -> Result<Light, String> {
    let obj = object(value, &format!("block {id} light"))?;
    require_only(obj, &["color", "level"], &format!("block {id} light"))?;
    let channels = obj
        .get("color")
        .ok_or_else(|| format!("block {id} light needs a color"))?
        .as_array()
        .ok_or_else(|| format!("block {id} light color must be three numbers"))?;
    if channels.len() != 3 {
        return Err(format!("block {id} light color needs 3 numbers"));
    }
    let mut color = [0.0; 3];
    for (index, channel) in channels.iter().enumerate() {
        let number = channel
            .as_f64()
            .ok_or_else(|| format!("block {id} light color must be numbers"))?;
        if !(0.0..=1.0).contains(&number) {
            return Err(format!(
                "block {id} light color channel {number} is outside 0..=1"
            ));
        }
        color[index] = number as f32;
    }
    let level = obj
        .get("level")
        .ok_or_else(|| format!("block {id} light needs a level"))?;
    let level = level
        .as_u64()
        .ok_or_else(|| format!("block {id} light level must be an integer from 1 to 15"))?;
    if !(1..=15).contains(&level) {
        return Err(format!("block {id} light level {level} is outside 1..=15"));
    }
    Ok(Light {
        color,
        level: level as u8,
    })
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what} must be an object"))
}

fn require_only(
    obj: &serde_json::Map<String, Value>,
    allowed: &[&str],
    what: &str,
) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{what} has unknown field {key}"));
        }
    }
    Ok(())
}

fn parse_hardness(obj: &serde_json::Map<String, Value>, id: &str) -> Result<Option<f32>, String> {
    match obj.get("hardness") {
        None => Ok(None),
        Some(value) => Ok(Some(positive_seconds(
            value,
            &format!("block {id} hardness"),
        )?)),
    }
}

fn positive_seconds(value: &Value, what: &str) -> Result<f32, String> {
    let Some(seconds) = value.as_f64() else {
        return Err(format!("{what} must be a positive number of seconds"));
    };
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(format!("{what} must be a positive number of seconds"));
    }
    Ok(seconds as f32)
}

fn parse_texture(obj: &serde_json::Map<String, Value>, id: &str) -> Result<Option<String>, String> {
    match obj.get("texture") {
        None => Ok(None),
        Some(Value::String(path)) if texture_path(path) => Ok(Some(path.clone())),
        Some(Value::String(_)) => Err(format!(
            "block {id} texture must be a png path under assets"
        )),
        Some(_) => Err(format!("block {id} texture must be a string path")),
    }
}

/// A relative PNG path. `..` and absolute paths stay out of the pack.
fn texture_path(path: &str) -> bool {
    if path.is_empty() || !path.ends_with(".png") || path.contains('\\') {
        return false;
    }
    if path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return false;
    }
    path.chars().all(|ch| {
        ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '/' || ch == '.'
    })
}

fn block_name(value: Option<&Value>, what: &str) -> Result<String, String> {
    let Some(value) = value else {
        return Err(format!("{what} is missing"));
    };
    if value.is_number() {
        return Err(format!("{what} must be a string id, not a number"));
    }
    let Some(name) = value.as_str() else {
        return Err(format!("{what} must be a string id"));
    };
    if !valid_name(name) {
        return Err(format!("{what} has a bad id {name}"));
    }
    if name == "air" {
        return Err(format!("{what} cannot be air"));
    }
    Ok(name.to_string())
}

fn resolve_texture(pack: &Pack, texture: &str) -> Result<String, String> {
    if texture_path(texture) {
        return Ok(texture.to_string());
    }
    if valid_name(texture) {
        if let Some(path) = pack.blocks.get(texture).and_then(BlockEntry::texture) {
            return Ok(path.to_string());
        }
    }
    Err(format!("bad texture {texture}"))
}

fn intern_path(path: &str) -> &'static str {
    static PATHS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut paths = PATHS.lock().expect("texture paths");
    if let Some(found) = paths.iter().find(|existing| **existing == path) {
        return found;
    }
    let leaked: &'static str = Box::leak(path.to_string().into_boxed_str());
    paths.push(leaked);
    leaked
}

fn identifier(namespace: &str, path: &str) -> Result<String, String> {
    if !valid_name(namespace) || !valid_name(path) {
        return Err(format!("bad identifier {namespace}:{path}"));
    }
    Ok(format!("{namespace}:{path}"))
}

fn stack_id(id: &str) -> bool {
    match id.split_once(':') {
        Some((namespace, path)) => valid_name(namespace) && valid_name(path) && !id.contains("::"),
        None => valid_name(id),
    }
}

fn tile_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn tile_value(value: &str) -> bool {
    value.len() <= 256 && value.chars().all(|ch| ch != '\0' && ch != '\n' && ch != '\r')
}

fn json_files(dir: &Path) -> Result<Vec<(String, String)>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", dir.display()))?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if path.is_file() && is_json {
            paths.push(path);
        }
    }
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
            .ok_or_else(|| format!("bad data file {}", path.display()))?;
        if !valid_name(&stem) {
            return Err(format!("bad data file {}", path.display()));
        }
        let text = fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        files.push((stem, text));
    }
    Ok(files)
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::{OAK_STAIRS, Plane, SIGN};

    #[test]
    fn builtin_pack_supplies_cubes_stairs_and_a_file_only_block() {
        let pack = Pack::builtin();
        for id in [
            "stone",
            "grass",
            "dirt",
            "cobblestone",
            "coal_ore",
            "iron_ore",
            "deepslate",
            "oak_log",
            "wool",
            "marker",
            "lantern",
        ] {
            assert!(
                pack.shape_of(Block::new(id, 0)).is_full_cube(),
                "block {id}"
            );
        }
        let stairs = pack.shape_of(Block::new(OAK_STAIRS, 0));
        assert_eq!(stairs.boxes().len(), 2);
        assert!(!stairs.is_full_cube());
        assert!(stairs.occludes(8, 0, 15));
        assert!(!stairs.occludes(8, 12, 15));
        assert!(stairs.occludes(8, 12, 0));
        assert!(stairs.covers_cell_face(1, false));
        assert!(!stairs.covers_cell_face(1, true));

        let (extra_id, extra_shape) = pack
            .blocks()
            .find(|(id, name)| {
                *name != "cube"
                    && *name != "stair_north"
                    && !pack.shape_of(Block::new(id, 0)).is_full_cube()
            })
            .expect("a block defined only in the pack file");
        assert!(!extra_shape.is_empty());
        assert_ne!(extra_id, OAK_STAIRS);
        let extra = pack.shape_of(Block::new(extra_id, 0));
        assert!(!extra.is_empty());
        assert!(!extra.is_full_cube());
        assert!(pack.shape_of(Block::new("missing", 0)).is_empty());
        assert!(pack.shape_of(Block::AIR).is_empty());

        let sign = pack.shape_of(Block::new(SIGN, 0));
        assert_eq!(
            sign.boxes(),
            &[CellBox {
                min: [6, 0, 6],
                max: [10, 12, 10],
            }]
        );
        assert_eq!(
            sign.planes(),
            &[Plane {
                axis: 2,
                at: 8,
                min: [1, 7],
                max: [15, 15],
            }]
        );
        assert!(!sign.is_full_cube());
        assert_eq!(
            pack.blocks()
                .find(|(id, _)| *id == SIGN)
                .map(|(_, path)| path),
            Some("assets/terralite/models/block/sign.json")
        );
        let lantern = pack.light_of(Block::new("lantern", 0)).expect("lantern");
        assert_eq!(lantern.level, 12);
        assert_eq!(lantern.color, [1.0, 0.25, 0.0]);
        assert_eq!(lantern.channels(), [12, 3, 0]);
        let torch = pack.light_of(Block::new("torch", 0)).expect("torch");
        assert_eq!(torch.level, 14);
        assert_eq!(torch.color, [1.0, 0.65, 0.15]);
        let campfire = pack.light_of(Block::new("campfire", 0)).expect("campfire");
        assert_eq!(campfire.level, 15);
        assert_eq!(campfire.color, [1.0, 0.5, 0.1]);
        assert_eq!(campfire.channels(), [15, 8, 2]);
        assert_eq!(torch.channels(), [14, 9, 2]);
        assert_eq!(pack.light_of(Block::new("torch", 3)).unwrap().level, 14);
        let rod = pack.light_of(Block::new("end_rod", 0)).expect("end rod");
        assert_eq!(rod.level, 14);
        assert_eq!(rod.color, [0.72, 0.88, 1.0]);
        assert_eq!(rod.channels(), [10, 12, 14]);
        assert_eq!(pack.light_of(Block::new("end_rod", 3)).unwrap().level, 14);
        let glowstone = pack
            .light_of(Block::new("glowstone", 0))
            .expect("glowstone");
        assert_eq!(glowstone.level, 15);
        assert_eq!(glowstone.color, [1.0, 0.8, 0.2]);
        assert_eq!(glowstone.channels(), [15, 12, 3]);
        let sea = pack
            .light_of(Block::new("sea_lantern", 0))
            .expect("sea lantern");
        assert_eq!(sea.level, 15);
        assert_eq!(sea.color, [0.25, 0.75, 1.0]);
        assert_eq!(sea.channels(), [4, 11, 15]);
        let standing = pack.shape_of(Block::new("torch", 0));
        assert!(!standing.is_full_cube());
        assert_eq!(
            standing.boxes().iter().map(|box_| box_.max[1]).max(),
            Some(14)
        );
        assert_eq!(pack.shape_of(Block::new("torch", 1)).boxes()[0].min[0], 0);
        let standing_rod = pack.shape_of(Block::new("end_rod", 0));
        assert!(!standing_rod.is_full_cube());
        assert_eq!(
            standing_rod.boxes().iter().map(|box_| box_.max[1]).max(),
            Some(16)
        );
        assert_eq!(pack.shape_of(Block::new("end_rod", 1)).boxes()[0].min[0], 0);
        assert!(pack.light_of(Block::new("stone", 0)).is_none());
        assert!(pack.light_of(Block::new(SIGN, 0)).is_none());
        let low = pack
            .light_of(crate::chunk::battery_at(1))
            .expect("battery glow");
        let high = pack
            .light_of(crate::chunk::battery_at(crate::chunk::BATTERY_MAX))
            .expect("full battery glow");
        assert!(low.level < high.level, "battery brightens with charge");
        assert!(high.level >= 14);
        assert_eq!(held_light_of(&pack, "torch"), Some(torch));
        assert_eq!(held_light_of(&pack, "lantern"), Some(lantern));
        assert_eq!(held_light_of(&pack, "glowstone"), Some(glowstone));
        assert_eq!(held_light_of(&pack, "sea_lantern"), Some(sea));
        assert_eq!(held_light_of(&pack, "campfire"), Some(campfire));
        assert_eq!(held_light_of(&pack, "end_rod"), Some(rod));
        assert!(held_light_of(&pack, "stone").is_none());
        assert!(held_light_of(&pack, "candle").is_none());
        assert!(held_light_of(&pack, "pumpkin").is_none());
        assert!(held_light_of(&pack, "").is_none());
        for (id, path) in [
            ("grass", "assets/terralite/textures/grass.png"),
            ("stone", "assets/terralite/textures/stone.png"),
            ("cobblestone", "assets/terralite/textures/cobblestone.png"),
            ("coal_ore", "assets/terralite/textures/coal_ore.png"),
            ("iron_ore", "assets/terralite/textures/iron_ore.png"),
            ("emerald_ore", "assets/terralite/textures/emerald_ore.png"),
            ("emerald", "assets/terralite/textures/emerald.png"),
            ("deepslate", "assets/terralite/textures/deepslate.png"),
            ("dirt", "assets/terralite/textures/block/dirt.png"),
            ("water", "assets/terralite/textures/water.png"),
            ("oak_log", "assets/terralite/textures/oak_log.png"),
            ("wool", "assets/terralite/textures/wool.png"),
            ("oak_stairs", "assets/terralite/textures/oak_stairs.png"),
            ("sign", "assets/terralite/textures/sign.png"),
            ("lantern", "assets/terralite/textures/lantern.png"),
            ("plate", "assets/terralite/textures/stone.png"),
            ("bookshelf", "assets/terralite/textures/bookshelf.png"),
        ] {
            assert_eq!(pack.texture_of(Block::new(id, 0)), Some(path));
            assert_eq!(&asset_png(path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        }
        assert!(pack.texture_of(Block::new("marker", 0)).is_none());
        assert!(asset_png("assets/terralite/textures/missing.png").is_none());
        assert!(asset_png("../pack.json").is_none());
    }

    #[test]
    fn the_pack_registers_blocks_shapes_and_models() {
        let pack = Pack::builtin();
        assert!(pack.shapes().contains("cube"));
        assert!(pack.shapes().contains("stair_north"));
        assert!(pack.shapes().contains("plate"));
        assert!(!pack.shapes().contains("assets/terralite/models/block/sign.json"));
        assert!(pack.models().contains("assets/terralite/models/block/sign.json"));
        assert!(
            pack.models()
                .get("assets/terralite/models/block/sign.json")
                .unwrap()
                .planes
                .len()
                == 1
        );
        for id in [
            "stone",
            "deepslate",
            "oak_stairs",
            "sign",
            "lantern",
            "coal_ore",
            "iron_ore",
            "emerald_ore",
            "emerald",
        ] {
            assert!(pack.block_registry().contains(id), "{id}");
        }
        assert!(!pack.block_registry().contains("missing"));
        assert_eq!(
            pack.block_registry()
                .get("oak_stairs")
                .unwrap()
                .geometry_id(),
            "assets/terralite/models/block/oak_stairs.json"
        );
        assert!(
            pack.block_registry()
                .get("oak_stairs")
                .unwrap()
                .uses_model()
        );
        let sign = pack.block_registry().get("sign").unwrap();
        assert!(sign.uses_model());
        assert_eq!(
            sign.geometry_id(),
            "assets/terralite/models/block/sign.json"
        );
        assert_eq!(
            pack.block_registry().get("coal_ore").unwrap().replaces(),
            Some("stone")
        );
        assert_eq!(
            pack.block_registry().get("iron_ore").unwrap().replaces(),
            Some("deepslate")
        );
        assert!(
            pack.block_registry()
                .get("lantern")
                .unwrap()
                .light()
                .is_some()
        );
        assert!(pack.shape_of(Block::new("missing", 0)).is_empty());
    }

    #[test]
    fn a_new_block_in_the_json_is_enough_to_define_a_shape() {
        let pack = Pack::parse(
            r#"
            {
              "shapes": {
                "post": { "boxes": [[6, 0, 6, 10, 14, 10]] }
              },
              "blocks": [
                { "id": "post", "shape": "post", "hardness": 1 }
              ]
            }
            "#,
        )
        .unwrap();
        let shape = pack.shape_of(Block::new("post", 0));
        assert_eq!(
            shape.boxes(),
            &[CellBox {
                min: [6, 0, 6],
                max: [10, 14, 10],
            }]
        );
        assert!(
            Pack::parse(r#"{"shapes":{},"blocks":[{"id":"post","shape":"missing"}]}"#).is_err()
        );
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":1,"shape":"cube"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn ore_entry_names_the_stone_left_behind() {
        let pack = Pack::parse(
            r#"
            {
              "shapes": {
                "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] }
              },
              "blocks": [
                { "id": "stone", "shape": "cube", "hardness": 1.5 },
                { "id": "deepslate", "shape": "cube", "hardness": 2 },
                { "id": "coal_ore", "shape": "cube", "replaces": "stone", "hardness": 2 },
                { "id": "iron_ore", "shape": "cube", "replaces": "deepslate", "hardness": 2.5 }
              ]
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            pack.break_result(Block::new("coal_ore", 0)),
            Block::new("stone", 0)
        );
        assert_eq!(
            pack.break_result(Block::new("iron_ore", 0)),
            Block::new("deepslate", 0)
        );
        assert_ne!(
            pack.break_result(Block::new("coal_ore", 0)),
            pack.break_result(Block::new("iron_ore", 0))
        );
        assert!(pack.break_result(Block::new("stone", 0)).is_air());
        assert!(pack.break_result(Block::new("deepslate", 0)).is_air());
        assert!(
            Pack::parse(
                r#"
                {
                  "shapes": { "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] } },
                  "blocks": [ { "id": "coal_ore", "shape": "cube", "replaces": "stone" } ]
                }
                "#
            )
            .is_err()
        );
    }

    #[test]
    fn a_pack_block_can_point_at_a_model_file() {
        let pack = Pack::parse(
            r#"
            {
              "shapes": {
                "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] }
              },
              "blocks": [
                { "id": "stone", "shape": "cube", "hardness": 1 },
                { "id": "sign", "model": "assets/terralite/models/block/sign.json", "hardness": 1 }
              ]
            }
            "#,
        )
        .unwrap();
        let sign = pack.shape_of(Block::new(SIGN, 0));
        assert_eq!(sign.boxes().len(), 1);
        assert_eq!(sign.planes().len(), 1);
        assert!(pack.shape_of(Block::new("stone", 0)).is_full_cube());
        assert!(
            Pack::parse(r#"{"shapes":{},"blocks":[{"id":"sign","model":"assets/terralite/models/block/missing.json"}]}"#)
                .is_err()
        );
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"sign","shape":"cube","model":"assets/terralite/models/block/sign.json"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_bbmodel_is_a_block_and_an_item() {
        let pack = Pack::builtin();
        let perch = pack.shape_of(Block::new("perch", 0));
        assert_eq!(perch.boxes().len(), 2);
        assert!(!perch.is_full_cube());
        assert_eq!(
            pack.texture_of(Block::new("perch", 0)),
            Some("assets/terralite/textures/oak_log.png")
        );
        let coin = pack.item_model("coin").expect("coin");
        assert_eq!(coin.boxes().len(), 1);
        assert_eq!(coin.boxes()[0].min, [3, 2, 6]);
        assert_eq!(coin.boxes()[0].max, [13, 14, 10]);
        assert!(pack.model_item_ids().iter().any(|id| id == "coin"));
        assert!(pack.item_model("perch").is_none());
        assert!(
            Pack::parse(
                r#"{"shapes":{},"blocks":[{"id":"post","model":"assets/terralite/models/post.txt"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn grass_slot_model_uses_gui_display() {
        let pack = Pack::builtin();
        let (shape, gui) = pack.slot_model("grass").expect("grass slot model");
        assert!(!shape.faces().is_empty());
        assert_eq!(gui.rotation, ItemDisplay::GUI_BLOCK.rotation);
        assert_eq!(gui.scale, ItemDisplay::GUI_BLOCK.scale);
    }

    #[test]
    fn grass_keeps_a_full_cube_and_four_blades() {
        let grass = Pack::builtin().shape_of(Block::new("grass", 0));
        assert!(grass.is_full_cube());
        let blades: Vec<_> = grass
            .faces()
            .iter()
            .copied()
            .filter(|face| {
                face.corners()
                    .iter()
                    .any(|corner| corner.iter().any(|axis| *axis < -0.02 || *axis > 1.02))
            })
            .collect();
        assert_eq!(blades.len(), 4);
        let names = grass.textures();
        assert!(
            names
                .iter()
                .any(|path| path.ends_with("grass_block_side_overlay.png"))
        );
        assert!(
            names
                .iter()
                .any(|path| path.ends_with("grass_block_top.png"))
        );
        assert!(
            names
                .iter()
                .any(|path| path.ends_with("/dirt.png") || path.ends_with("block/dirt.png"))
        );
        let overlay = names
            .iter()
            .position(|path| path.ends_with("grass_block_side_overlay.png"))
            .expect("overlay");
        assert!(
            blades
                .iter()
                .all(|face| face.texture_index() as usize == overlay)
        );
        assert!(blades.iter().all(|face| {
            let uv = face.uvs();
            let min_u = uv.iter().map(|point| point[0]).fold(f32::MAX, f32::min);
            let max_u = uv.iter().map(|point| point[0]).fold(f32::MIN, f32::max);
            max_u - min_u > 0.2
        }));
        for face in blades {
            let corners = face.corners();
            let max_y = corners.iter().map(|corner| corner[1]).fold(0.0, f32::max);
            let min_y = corners.iter().map(|corner| corner[1]).fold(2.0, f32::min);
            assert!(
                min_y < 0.6 && max_y > 0.95 && max_y < 1.15,
                "blade should hang from the top edge down the side: {corners:?}"
            );
            assert!(
                max_y - min_y > 0.4,
                "blade should have some height: {corners:?}"
            );
            for corner in corners {
                assert!(
                    (-0.2..1.2).contains(&corner[0]) && (-0.2..1.2).contains(&corner[2]),
                    "blade left the cell: {corner:?}"
                );
            }
        }
    }

    #[test]
    fn blockbench_mesh_spins_xyz_around_its_origin() {
        let model = parse_model(
            r#"
            {
              "elements": [
                {
                  "type": "mesh",
                  "origin": [8, 0, 0],
                  "rotation": [90, 0, 90],
                  "vertices": {
                    "a": [16, 0, 0],
                    "b": [16, 0, -8],
                    "c": [0, 0, 0],
                    "d": [0, 0, -8]
                  },
                  "faces": {
                    "f": { "vertices": ["a", "b", "d", "c"], "uv": { "a": [0, 0], "b": [1, 0], "d": [1, 1], "c": [0, 1] } }
                  }
                }
              ]
            }
            "#,
        )
        .unwrap();
        let corners = model.faces[0].corners();
        let near = |got: [f32; 3], want: [f32; 3]| {
            got.iter()
                .zip(want)
                .all(|(value, expected)| (value - expected).abs() < 0.02)
        };
        // Z then X sends the +X offset onto +Z. Treating the vertex as absolute
        // and subtracting the origin would land somewhere else.
        assert!(
            corners.iter().any(|corner| near(*corner, [1.0, 0.0, 1.5])),
            "xyz spin around the origin: {corners:?}"
        );
        assert!(
            corners.iter().any(|corner| near(*corner, [1.0, 0.0, 0.5])),
            "the pivot edge stays on the origin: {corners:?}"
        );
    }

    #[test]
    fn blockbench_cube_spins_zyx_around_its_pivot() {
        let model = parse_model(
            r#"
            {
              "elements": [
                {
                  "from": [0, 0, 0],
                  "to": [0, 2, 2],
                  "origin": [8, 0, 0],
                  "rotation": [90, 0, 90]
                }
              ]
            }
            "#,
        )
        .unwrap();
        assert!(model.boxes.is_empty());
        assert!(model.planes.is_empty());
        let corners = model.faces[0].corners();
        let near = |got: [f32; 3], want: [f32; 3]| {
            got.iter()
                .zip(want)
                .all(|(value, expected)| (value - expected).abs() < 0.02)
        };
        // Absolute [0,0,0] is 8 units behind the pivot. ZYX (X then Z) swings
        // that to [8, -8, 0]. Mesh order would leave it on y = 0.
        assert!(
            corners.iter().any(|corner| near(*corner, [0.5, -0.5, 0.0])),
            "cube pivots around its origin: {corners:?}"
        );
        let full = parse_model(
            r#"
            {
              "elements": [
                {
                  "from": [0, 0, 0],
                  "to": [16, 16, 16],
                  "origin": [8, 8, 8],
                  "rotation": [0, 90, 0]
                }
              ]
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            full.boxes,
            vec![CellBox {
                min: [0, 0, 0],
                max: [16, 16, 16],
            }]
        );
    }

    #[test]
    fn dirt_model_covers_the_block_texture() {
        let dirt = Pack::builtin().shape_of(Block::new("dirt", 0));
        assert!(dirt.is_full_cube());
        assert_eq!(dirt.faces().len(), 6);
        assert!(
            dirt.textures()
                .iter()
                .any(|path| path.ends_with("block/dirt.png"))
        );
        assert!(dirt.faces().iter().all(|face| {
            dirt.texture_of(*face)
                .is_some_and(|path| path.ends_with("block/dirt.png"))
        }));
        for face in dirt.faces() {
            let uv = face.uvs();
            let min_u = uv.iter().map(|point| point[0]).fold(f32::MAX, f32::min);
            let max_u = uv.iter().map(|point| point[0]).fold(f32::MIN, f32::max);
            let min_v = uv.iter().map(|point| point[1]).fold(f32::MAX, f32::min);
            let max_v = uv.iter().map(|point| point[1]).fold(f32::MIN, f32::max);
            assert!(
                max_u - min_u > 0.9 && max_v - min_v > 0.9,
                "dirt uv should cover the tile, not the 4px corner: {uv:?}"
            );
        }
        assert_eq!(
            Pack::builtin().texture_of(Block::new("dirt", 0)),
            Some("assets/terralite/textures/block/dirt.png")
        );
    }

    #[test]
    fn a_model_element_can_be_a_cube_or_a_plane() {
        let model = parse_model(
            r##"
            {
              "credit": "Made with Blockbench",
              "format_version": "4.10",
              "elements": [
                {
                  "name": "post",
                  "type": "cube",
                  "from": [0, 0, 0],
                  "to": [4, 4, 4],
                  "faces": { "north": { "uv": [0, 0, 4, 4], "texture": "#0" } }
                },
                {
                  "name": "board",
                  "from": [0, 2, 8],
                  "to": [16, 14, 8]
                }
              ]
            }
            "##,
        )
        .unwrap();
        assert_eq!(
            model.boxes,
            vec![CellBox {
                min: [0, 0, 0],
                max: [4, 4, 4],
            }]
        );
        assert_eq!(
            model.planes,
            vec![Plane {
                axis: 2,
                at: 8,
                min: [0, 2],
                max: [16, 14],
            }]
        );
        assert!(parse_model(r#"{"elements":[{"from":[0,0,0],"to":[0,0,4]}]}"#).is_err());
        assert!(
            parse_model(r#"{"elements":[{"type":"mesh","from":[0,0,0],"to":[1,1,1]}]}"#).is_err()
        );
    }

    #[test]
    fn a_block_can_be_a_colored_light() {
        let pack = Pack::parse(
            r#"
            {
              "shapes": { "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] } },
              "blocks": [
                { "id": "stone", "shape": "cube", "hardness": 1 },
                { "id": "glow", "shape": "cube", "light": { "color": [0.0, 0.5, 1.0], "level": 8 }, "hardness": 1 }
              ]
            }
            "#,
        )
        .unwrap();
        let glow = pack.light_of(Block::new("glow", 0)).unwrap();
        assert_eq!(glow.color, [0.0, 0.5, 1.0]);
        assert_eq!(glow.level, 8);
        assert_eq!(glow.channels(), [0, 4, 8]);
        assert!(pack.light_of(Block::new("stone", 0)).is_none());
        assert!(pack.shape_of(Block::new("stone", 0)).is_full_cube());
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"glow","shape":"cube","light":{"color":[1,0,0],"level":0}}]}"#
            )
            .is_err()
        );
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"glow","shape":"cube","light":{"color":[1.2,0,0],"level":4}}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_missing_texture_file_is_not_an_error() {
        let pack = Pack::parse(
            r#"
            {
              "shapes": { "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] } },
              "blocks": [
                { "id": "grass", "shape": "cube", "texture": "assets/terralite/textures/missing.png", "hardness": 0.25 }
              ]
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            pack.texture_of(Block::new("grass", 0)),
            Some("assets/terralite/textures/missing.png")
        );
        assert!(asset_png("assets/terralite/textures/missing.png").is_none());
        assert_eq!(pack.hardness_of(Block::new("grass", 0)), Some(0.25));
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"grass","shape":"cube","texture":"../grass.png"}]}"#
            )
            .is_err()
        );
        assert!(
            Pack::parse(
                r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"grass","shape":"cube","texture":1}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn hardness_is_a_positive_number_of_seconds() {
        let pack = Pack::builtin();
        assert_eq!(pack.hardness_of(Block::new("grass", 0)), Some(0.25));
        assert_eq!(pack.hardness_of(Block::new("stone", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("coal_ore", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("iron_ore", 0)), Some(2.5));
        assert!(pack.hardness_of(Block::AIR).is_none());
        assert!(pack.hardness_of(Block::new("missing", 0)).is_none());
        let cube = r#"{"shapes":{"cube":{"boxes":[[0,0,0,16,16,16]]}},"blocks":[{"id":"stone","shape":"cube""#;
        let bare = Pack::parse(&format!("{cube}}}]}}")).unwrap();
        assert!(bare.hardness_of(Block::new("stone", 0)).is_none());
        assert!(Pack::parse(&format!(r#"{cube},"hardness":0}}]}}"#)).is_err());
        assert!(Pack::parse(&format!(r#"{cube},"hardness":-1}}]}}"#)).is_err());
        assert!(Pack::parse(&format!(r#"{cube},"hardness":"slow"}}]}}"#)).is_err());
    }

    #[test]
    fn the_data_pack_owns_hardness_and_replaces() {
        let bare = Pack::load_resource_models().unwrap();
        assert_eq!(
            block_definitions_file(),
            resource_pack_root()
                .join("assets")
                .join("terralite")
                .join("models")
                .join("block")
        );
        assert!(block_definitions_file().is_dir());
        assert!(!std::path::Path::new(r"C:\CODE\TerraLite\assets").exists());
        assert!(bare.hardness_of(Block::new("grass", 0)).is_none());
        assert!(bare.hardness_of(Block::new("stone", 0)).is_none());
        assert!(
            bare.block_registry()
                .get("coal_ore")
                .unwrap()
                .replaces()
                .is_none()
        );
        assert!(
            bare.block_registry()
                .get("iron_ore")
                .unwrap()
                .replaces()
                .is_none()
        );
        assert!(data_pack_root().join("pack.meta").is_file());
        load_data_pack().unwrap();
        let pack = Pack::builtin();
        assert_eq!(pack.hardness_of(Block::new("grass", 0)), Some(0.25));
        assert_eq!(pack.hardness_of(Block::new("stone", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("cobblestone", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("coal_ore", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("iron_ore", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("deepslate", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("dirt", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("oak_log", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("wool", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("oak_stairs", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("marker", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("plate", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("sign", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("lantern", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("crafting_table", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("furnace", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("bed", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("door", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("copper_door", 0)), Some(3.0));
        assert_eq!(
            pack.shape_of(Block::new("copper_door", 0)).boxes()[0].max[2],
            3
        );
        assert_eq!(
            pack.shape_of(Block::new("copper_door", 1)).boxes()[0].max[0],
            3
        );
        assert_eq!(pack.hardness_of(Block::new("fence", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("fence_gate", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("ladder", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("trapdoor", 0)), Some(1.0));
        assert_eq!(
            pack.hardness_of(Block::new("copper_trapdoor", 0)),
            Some(3.0)
        );
        assert_eq!(
            pack.shape_of(Block::new("copper_trapdoor", 0)).boxes()[0].min[1],
            13
        );
        assert_eq!(
            pack.shape_of(Block::new("copper_trapdoor", 1)).boxes()[0].max[0],
            3
        );
        assert_eq!(pack.hardness_of(Block::new("glass", 0)), Some(0.4));
        assert_eq!(pack.hardness_of(Block::new("slab", 0)), Some(1.0));
        assert_eq!(pack.hardness_of(Block::new("carpet", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("torch", 0)), Some(0.1));
        assert_eq!(pack.hardness_of(Block::new("end_rod", 0)), Some(0.1));
        assert_eq!(pack.hardness_of(Block::new("flower", 0)), Some(0.1));
        assert_eq!(pack.hardness_of(Block::new("chorus_flower", 0)), Some(0.1));
        assert_eq!(pack.hardness_of(Block::new("button", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("lever", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("note_block", 0)), Some(0.8));
        assert_eq!(pack.hardness_of(Block::new("tripwire", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("painting", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("item_frame", 1)), Some(0.5));
        assert_eq!(pack.shape_of(Block::new("item_frame", 1)).boxes().len(), 1);
        assert_eq!(pack.shape_of(Block::new("item_frame", 9)).boxes().len(), 2);
        assert!(pack.is_block_entity("item_frame"));
        assert_eq!(pack.hardness_of(Block::new("bookshelf", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("chiseled_bookshelf", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.shape_of(Block::new("chiseled_bookshelf", 0))
                .boxes()
                .len(),
            1
        );
        assert_eq!(
            pack.shape_of(Block::new("chiseled_bookshelf", 3))
                .boxes()
                .len(),
            4
        );
        assert_eq!(
            pack.shape_of(Block::new("chiseled_bookshelf", 6))
                .boxes()
                .len(),
            7
        );
        assert_eq!(pack.hardness_of(Block::new("flower_pot", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("sand", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("soul_sand", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("magma", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("cactus", 0)), Some(0.4));
        assert_eq!(pack.hardness_of(Block::new("cake", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("pumpkin", 0)), Some(1.0));
        assert!(pack.light_of(Block::new("pumpkin", 0)).is_none());
        assert!(pack.light_of(Block::new("pumpkin", 1)).is_none());
        assert_eq!(pack.light_of(Block::new("pumpkin", 2)).unwrap().level, 14);
        assert_eq!(
            pack.texture_of(Block::new("pumpkin", 0)),
            Some("assets/terralite/textures/pumpkin.png")
        );
        assert_eq!(
            pack.texture_of(Block::new("pumpkin", 2)),
            Some("assets/terralite/textures/carved_pumpkin.png")
        );
        assert_eq!(pack.mine_factor("", "stone"), 1.0);
        assert_eq!(pack.mine_factor("wooden_pickaxe", "grass"), 1.0);
        assert_eq!(pack.mine_factor("wooden_pickaxe", "stone"), 2.0);
        assert_eq!(pack.mine_factor("stone_pickaxe", "coal_ore"), 4.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "iron_ore"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "furnace"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "stone_bricks"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "cracked_stone_bricks"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "mossy_stone_bricks"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "chiseled_stone_bricks"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "deepslate_bricks"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "cobbled_deepslate"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "polished_deepslate"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "deepslate_tiles"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "cracked_deepslate_tiles"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "blackstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "polished_blackstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "blackstone_bricks"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "cracked_blackstone_bricks"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "chiseled_blackstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "gilded_blackstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "basalt"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "polished_basalt"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "smooth_basalt"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "nether_bricks"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "cracked_nether_bricks"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "red_nether_bricks"), 6.0);
        assert_eq!(
            pack.mine_factor("iron_pickaxe", "chiseled_nether_bricks"),
            6.0
        );
        assert_eq!(pack.mine_factor("iron_pickaxe", "netherrack"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "end_stone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "end_portal_frame"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "dragon_egg"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "end_stone_bricks"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "purpur"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "purpur_pillar"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "sandstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "red_sandstone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "smooth_stone"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "quartz"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "sea_lantern"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "prismarine"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "dark_prismarine"), 6.0);
        assert_eq!(pack.mine_factor("iron_pickaxe", "prismarine_bricks"), 6.0);
        assert_eq!(pack.tool_uses("wooden_pickaxe"), 8);
        assert_eq!(pack.tool_uses("stone_pickaxe"), 16);
        assert_eq!(pack.tool_uses("iron_pickaxe"), 32);
        assert_eq!(pack.tool_uses("diamond_pickaxe"), 48);
        assert_eq!(pack.mine_factor("diamond_pickaxe", "stone"), 8.0);
        assert_eq!(pack.mine_factor("diamond_pickaxe", "dirt"), 1.0);
        assert_eq!(pack.tool_icon("diamond_pickaxe"), Some("quartz"));
        assert_eq!(pack.tool_uses("apple"), 0);
        assert_eq!(pack.tool_icon("wooden_pickaxe"), Some("oak_log"));
        assert!(pack.tool_ids().contains(&"iron_pickaxe".to_string()));
        assert_eq!(pack.hardness_of(Block::new("melon", 0)), Some(1.0));
        assert_eq!(
            pack.hardness_of(Block::new("nether_wart_block", 0)),
            Some(1.0)
        );
        assert_eq!(pack.hardness_of(Block::new("hay_bale", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("cobweb", 0)), Some(0.4));
        assert_eq!(pack.hardness_of(Block::new("vine", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("lily_pad", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("snow", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("sugar_cane", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("chorus_plant", 0)), Some(0.2));
        assert_eq!(pack.hardness_of(Block::new("ice", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("clay", 0)), Some(0.6));
        assert_eq!(pack.hardness_of(Block::new("bricks", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("gravel", 0)), Some(0.6));
        assert_eq!(
            pack.hardness_of(Block::new("mossy_cobblestone", 0)),
            Some(1.5)
        );
        assert_eq!(pack.hardness_of(Block::new("stone_bricks", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("cracked_stone_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("mossy_stone_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("chiseled_stone_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("deepslate_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("cobbled_deepslate", 0)),
            Some(2.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("polished_deepslate", 0)),
            Some(2.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("deepslate_tiles", 0)),
            Some(2.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("cracked_deepslate_tiles", 0)),
            Some(2.0)
        );
        assert_eq!(pack.hardness_of(Block::new("blackstone", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("polished_blackstone", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("blackstone_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("cracked_blackstone_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("chiseled_blackstone", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("gilded_blackstone", 0)),
            Some(1.5)
        );
        assert_eq!(pack.hardness_of(Block::new("basalt", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("polished_basalt", 0)),
            Some(1.5)
        );
        assert_eq!(pack.hardness_of(Block::new("smooth_basalt", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("nether_bricks", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("netherrack", 0)), Some(0.4));
        assert_eq!(pack.hardness_of(Block::new("end_stone", 0)), Some(3.0));
        assert_eq!(
            pack.hardness_of(Block::new("end_stone_bricks", 0)),
            Some(3.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("end_portal_frame", 0)),
            Some(3.0)
        );
        assert_eq!(pack.hardness_of(Block::new("dragon_egg", 0)), Some(3.0));
        assert_eq!(pack.hardness_of(Block::new("end_crystal", 0)), Some(0.3));
        assert_eq!(pack.hardness_of(Block::new("chest", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("copper_chest", 0)), Some(3.0));
        assert!(pack.shape_of(Block::new("copper_chest", 0)).is_full_cube());
        assert_eq!(pack.hardness_of(Block::new("ender_chest", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("shulker_box", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("barrel", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("stonecutter", 0)), Some(3.5));
        assert_eq!(pack.hardness_of(Block::new("stone_stairs", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("campfire", 0)), Some(2.0));
        assert_eq!(
            pack.hardness_of(Block::new("fletching_table", 0)),
            Some(2.5)
        );
        assert_eq!(pack.hardness_of(Block::new("smoker", 0)), Some(3.5));
        assert_eq!(pack.hardness_of(Block::new("blast_furnace", 0)), Some(4.0));
        assert_eq!(pack.hardness_of(Block::new("beehive", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("cauldron", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("bell", 0)), Some(3.0));
        assert_eq!(pack.hardness_of(Block::new("candle", 0)), Some(0.5));
        assert!(pack.light_of(Block::new("candle", 0)).is_none());
        assert_eq!(pack.light_of(Block::new("candle", 1)).unwrap().level, 10);
        assert_eq!(pack.hardness_of(Block::new("chain", 0)), Some(5.0));
        assert_eq!(pack.hardness_of(Block::new("hanging_sign", 0)), Some(1.0));
        assert_eq!(
            pack.shape_of(Block::new("hanging_sign", 0)).boxes()[1].max[2]
                - pack.shape_of(Block::new("hanging_sign", 0)).boxes()[1].min[2],
            2
        );
        assert_eq!(
            pack.shape_of(Block::new("hanging_sign", 1)).boxes()[0].min[0],
            0
        );
        assert_eq!(
            pack.shape_of(Block::new("hanging_sign", 3)).boxes()[0].max[2],
            16
        );
        assert_eq!(pack.hardness_of(Block::new("lodestone", 0)), Some(3.5));
        assert_eq!(pack.hardness_of(Block::new("respawn_anchor", 0)), Some(5.0));
        assert!(pack.light_of(Block::new("respawn_anchor", 0)).is_none());
        assert_eq!(
            pack.light_of(Block::new("respawn_anchor", 1))
                .unwrap()
                .level,
            4
        );
        assert_eq!(
            pack.light_of(Block::new("respawn_anchor", 4))
                .unwrap()
                .level,
            15
        );
        assert_eq!(pack.hardness_of(Block::new("lightning_rod", 0)), Some(3.0));
        assert!(pack.light_of(Block::new("lightning_rod", 0)).is_none());
        assert_eq!(
            pack.light_of(Block::new("lightning_rod", 8)).unwrap().level,
            15
        );
        assert_eq!(pack.hardness_of(Block::new("target", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("trial_spawner", 0)), Some(5.0));
        assert!(pack.light_of(Block::new("target", 0)).is_none());
        assert_eq!(pack.light_of(Block::new("target", 1)).unwrap().level, 10);
        assert_eq!(pack.hardness_of(Block::new("copper_bulb", 0)), Some(3.0));
        assert!(pack.light_of(Block::new("copper_bulb", 0)).is_none());
        assert_eq!(
            pack.light_of(Block::new("copper_bulb", 1)).unwrap().level,
            15
        );
        assert_eq!(pack.hardness_of(Block::new("decorated_pot", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("shelf", 0)), Some(1.5));
        let board = pack.shape_of(Block::new("shelf", shelf_meta(1, 0)));
        assert!(!board.is_full_cube());
        assert_eq!(board.boxes().len(), 1);
        assert_eq!(board.boxes()[0].max[0], 8);
        assert_eq!(
            pack.shape_of(Block::new("shelf", shelf_meta(1, 2)))
                .boxes()
                .len(),
            3
        );
        assert_eq!(shelf_facing(shelf_meta(4, 3)), 4);
        assert_eq!(shelf_filled(shelf_meta(4, 3)), 3);
        assert_eq!(pack.hardness_of(Block::new("copper_grate", 0)), Some(3.0));
        assert!(!pack.shape_of(Block::new("copper_grate", 0)).is_full_cube());
        assert_eq!(pack.hardness_of(Block::new("anvil", 0)), Some(5.0));
        assert_eq!(
            pack.hardness_of(Block::new("enchanting_table", 0)),
            Some(5.0)
        );
        assert_eq!(pack.hardness_of(Block::new("brewing_stand", 0)), Some(0.5));
        assert_eq!(pack.hardness_of(Block::new("grindstone", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("smithing_table", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("loom", 0)), Some(2.0));
        assert_eq!(pack.hardness_of(Block::new("banner", 0)), Some(1.0));
        assert_eq!(
            pack.hardness_of(Block::new("cartography_table", 0)),
            Some(2.5)
        );
        assert_eq!(pack.hardness_of(Block::new("lectern", 0)), Some(2.5));
        assert_eq!(pack.hardness_of(Block::new("composter", 0)), Some(0.6));
        assert_eq!(pack.tool_icon("healing_potion"), Some("nether_wart_block"));
        assert_eq!(pack.food_of("healing_potion"), Some(4));
        assert!(pack.is_container("chest"));
        assert!(pack.is_block_entity("chest"));
        assert!(pack.is_block_entity("copper_chest"));
        assert!(pack.is_block_entity("ender_chest"));
        assert!(pack.is_block_entity("shulker_box"));
        assert!(pack.is_block_entity("barrel"));
        assert!(pack.is_block_entity("decorated_pot"));
        assert!(pack.is_block_entity("shelf"));
        assert!(!pack.is_block_entity("stone"));
        assert!(!pack.is_script_block_entity("chest"));
        pack.note_block_entity("demo:tile").unwrap();
        assert!(pack.is_script_block_entity("demo:tile"));
        assert!(pack.is_block_entity("demo:tile"));
        assert!(!pack.is_container("demo:tile"));
        assert!(pack.tile_set(1, 2, 3, "demo:tile", "charge", "4"));
        assert_eq!(pack.tile_get(1, 2, 3, "charge"), "4");
        assert_eq!(pack.tile_get_int(1, 2, 3, "charge"), 4);
        assert!(pack.tile_has(1, 2, 3));
        pack.clear_tile_at(1, 2, 3);
        assert!(!pack.tile_has(1, 2, 3));
        assert_eq!(pack.hardness_of(Block::new("purpur", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("purpur_pillar", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("cracked_nether_bricks", 0)),
            Some(2.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("red_nether_bricks", 0)),
            Some(2.0)
        );
        assert_eq!(
            pack.hardness_of(Block::new("chiseled_nether_bricks", 0)),
            Some(2.0)
        );
        assert_eq!(pack.hardness_of(Block::new("sandstone", 0)), Some(0.8));
        assert_eq!(pack.hardness_of(Block::new("red_sandstone", 0)), Some(0.8));
        assert_eq!(pack.hardness_of(Block::new("smooth_stone", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("quartz", 0)), Some(0.8));
        assert_eq!(pack.hardness_of(Block::new("glowstone", 0)), Some(0.3));
        assert_eq!(pack.hardness_of(Block::new("sea_lantern", 0)), Some(0.3));
        assert_eq!(pack.hardness_of(Block::new("prismarine", 0)), Some(1.5));
        assert_eq!(
            pack.hardness_of(Block::new("dark_prismarine", 0)),
            Some(1.5)
        );
        assert_eq!(
            pack.hardness_of(Block::new("prismarine_bricks", 0)),
            Some(1.5)
        );
        assert_eq!(pack.food_of("apple"), Some(4));
        assert_eq!(pack.food_of("raw"), Some(1));
        assert_eq!(pack.food_of("cooked"), Some(6));
        assert_eq!(pack.food_of("honey"), Some(2));
        assert_eq!(pack.food_of("slimeball"), Some(1));
        assert_eq!(pack.food_of("chorus_fruit"), Some(4));
        assert_eq!(pack.tool_icon("chorus_fruit"), Some("chorus_flower"));
        assert_eq!(pack.food_of("melon_slice"), Some(2));
        assert_eq!(harvest_of("melon"), ("melon_slice", 4));
        assert_eq!(harvest_of("stone"), ("stone", 1));
        // Datapack loot/blocks/grass.json lists dirt + wheat_seeds.
        let grass = break_loot_of("grass");
        assert_eq!(
            grass,
            vec![("dirt".to_string(), 1), ("wheat_seeds".to_string(), 1)]
        );
        assert_eq!(break_loot_of("stone"), vec![("stone".to_string(), 1)]);
        pack.set_block_loot("dirt", &["torch", "apple"], "loot-test")
            .unwrap();
        assert_eq!(
            break_loot_of("dirt"),
            vec![("torch".to_string(), 1), ("apple".to_string(), 1)]
        );
        assert_eq!(pack.tool_icon("melon_slice"), Some("melon"));
        assert_eq!(pack.food_of("stone"), None);
        assert_eq!(pack.tool_icon("apple"), Some("grass"));
        assert!(pack.food_ids().contains(&"apple".to_string()));
        assert_eq!(pack.armor_of("leather_cap"), Some((0, 1)));
        assert_eq!(pack.armor_of("leather_tunic"), Some((1, 1)));
        assert_eq!(pack.armor_of("leather_boots"), Some((2, 1)));
        assert_eq!(pack.armor_of("apple"), None);
        assert!(pack.is_shield("shield"));
        assert!(!pack.is_shield("leather_cap"));
        assert_eq!(pack.tool_uses("shield"), 4);
        assert_eq!(pack.tool_icon("shield"), Some("cobblestone"));
        assert!(pack.shield_ids().contains(&"shield".to_string()));
        assert!(pack.is_boat("boat"));
        assert!(!pack.is_boat("shield"));
        assert_eq!(pack.tool_icon("boat"), Some("oak_log"));
        assert!(pack.boat_ids().contains(&"boat".to_string()));
        assert!(pack.is_minecart("minecart"));
        assert!(!pack.is_minecart("boat"));
        assert_eq!(pack.tool_icon("minecart"), Some("iron_ore"));
        assert!(pack.minecart_ids().contains(&"minecart".to_string()));
        assert!(pack.is_lead("lead"));
        assert!(!pack.is_lead("saddle"));
        assert_eq!(pack.tool_icon("lead"), Some("tripwire"));
        assert!(pack.lead_ids().contains(&"lead".to_string()));
        assert_eq!(pack.hardness_of(Block::new("rail", 0)), Some(0.7));
        assert_eq!(pack.hardness_of(Block::new("powered_rail", 0)), Some(0.7));
        assert!(pack.is_compass("compass"));
        assert!(!pack.is_compass("boat"));
        assert_eq!(pack.tool_icon("compass"), Some("iron_ore"));
        assert!(pack.compass_ids().contains(&"compass".to_string()));
        assert!(pack.is_spyglass("spyglass"));
        assert!(!pack.is_spyglass("compass"));
        assert_eq!(pack.tool_icon("spyglass"), Some("copper_grate"));
        assert!(pack.spyglass_ids().contains(&"spyglass".to_string()));
        assert!(pack.is_brush("brush"));
        assert!(!pack.is_brush("spyglass"));
        assert_eq!(pack.tool_uses("brush"), 8);
        assert_eq!(pack.tool_icon("brush"), Some("sand"));
        assert_eq!(pack.mine_factor("brush", "stone"), 1.0);
        assert!(pack.brush_ids().contains(&"brush".to_string()));
        assert!(pack.is_horn("goat_horn"));
        assert!(!pack.is_horn("brush"));
        assert_eq!(pack.tool_icon("goat_horn"), Some("bone_meal"));
        assert!(pack.horn_ids().contains(&"goat_horn".to_string()));
        assert!(pack.is_mace("mace"));
        assert!(!pack.is_mace("goat_horn"));
        assert_eq!(pack.tool_uses("mace"), 16);
        assert_eq!(pack.tool_icon("mace"), Some("iron_ore"));
        assert_eq!(pack.mine_factor("mace", "stone"), 1.0);
        assert!(pack.mace_ids().contains(&"mace".to_string()));
        assert!(pack.is_mace("heavy_mace"));
        assert_eq!(pack.tool_uses("heavy_mace"), 32);
        assert_eq!(pack.mace_smash("mace"), 3);
        assert_eq!(pack.mace_smash("heavy_mace"), 5);
        assert_eq!(pack.mine_factor("heavy_mace", "stone"), 1.0);
        assert!(pack.is_core("heavy_core"));
        assert!(!pack.is_core("mace"));
        assert!(!pack.is_mace("heavy_core"));
        assert_eq!(pack.tool_icon("heavy_core"), Some("deepslate"));
        assert!(pack.core_ids().contains(&"heavy_core".to_string()));
        assert!(pack.is_key("trial_key"));
        assert!(!pack.is_key("heavy_core"));
        assert_eq!(pack.tool_icon("trial_key"), Some("copper_chest"));
        assert!(pack.key_ids().contains(&"trial_key".to_string()));
        assert!(pack.is_omen("ominous_bottle"));
        assert!(!pack.is_omen("trial_key"));
        assert!(!pack.is_key("ominous_bottle"));
        assert_eq!(pack.tool_icon("ominous_bottle"), Some("nether_wart_block"));
        assert!(pack.omen_ids().contains(&"ominous_bottle".to_string()));
        assert!(pack.is_bucket("bucket"));
        assert!(pack.is_water_bucket("water_bucket"));
        assert!(!pack.is_bucket("water_bucket"));
        assert!(!pack.is_water_bucket("bucket"));
        assert_eq!(pack.tool_icon("water_bucket"), Some("water"));
        assert!(pack.is_saddle("saddle"));
        assert!(!pack.is_saddle("bucket"));
        assert_eq!(pack.tool_icon("saddle"), Some("wool"));
        assert!(pack.saddle_ids().contains(&"saddle".to_string()));
        assert!(pack.is_pearl("ender_pearl"));
        assert!(!pack.is_pearl("saddle"));
        assert_eq!(pack.tool_icon("ender_pearl"), Some("sea_lantern"));
        assert!(pack.pearl_ids().contains(&"ender_pearl".to_string()));
        assert!(pack.is_wind("wind_charge"));
        assert!(!pack.is_wind("ender_pearl"));
        assert_eq!(pack.tool_icon("wind_charge"), Some("wool"));
        assert!(pack.wind_ids().contains(&"wind_charge".to_string()));
        assert!(pack.is_rod("breeze_rod"));
        assert!(!pack.is_rod("wind_charge"));
        assert!(!pack.is_wind("breeze_rod"));
        assert_eq!(pack.tool_icon("breeze_rod"), Some("stick"));
        assert!(pack.rod_ids().contains(&"breeze_rod".to_string()));
        assert!(pack.is_fishing("fishing_rod"));
        assert!(!pack.is_fishing("breeze_rod"));
        assert!(!pack.is_rod("fishing_rod"));
        assert_eq!(pack.tool_icon("fishing_rod"), Some("stick"));
        assert!(pack.fishing_ids().contains(&"fishing_rod".to_string()));
        assert_eq!(pack.food_of("fish"), Some(2));
        assert_eq!(pack.tool_icon("fish"), Some("lily_pad"));
        assert!(pack.is_seeds("wheat_seeds"));
        assert!(!pack.is_seeds("wheat"));
        assert_eq!(pack.tool_icon("wheat_seeds"), Some("grass"));
        assert!(pack.seed_ids().contains(&"wheat_seeds".to_string()));
        assert_eq!(pack.food_of("wheat"), Some(2));
        assert_eq!(pack.tool_icon("wheat"), Some("hay_bale"));
        assert_eq!(pack.food_of("bread"), Some(5));
        assert_eq!(pack.tool_icon("bread"), Some("hay_bale"));
        assert_eq!(pack.hardness_of(Block::new("wheat_crop", 0)), Some(0.1));
        assert!(pack.is_hoe("wooden_hoe"));
        assert!(!pack.is_hoe("wooden_pickaxe"));
        assert_eq!(pack.tool_uses("wooden_hoe"), 32);
        assert_eq!(pack.tool_icon("wooden_hoe"), Some("stick"));
        assert!(pack.hoe_ids().contains(&"wooden_hoe".to_string()));
        assert_eq!(pack.hardness_of(Block::new("farmland", 0)), Some(0.5));
        assert!(pack.is_eye("eye_of_ender"));
        assert!(!pack.is_eye("ender_pearl"));
        assert!(!pack.is_pearl("eye_of_ender"));
        assert_eq!(pack.tool_icon("eye_of_ender"), Some("glowstone"));
        assert!(pack.eye_ids().contains(&"eye_of_ender".to_string()));
        assert!(pack.is_clock("clock"));
        assert!(!pack.is_clock("compass"));
        assert_eq!(pack.tool_icon("clock"), Some("lantern"));
        assert!(pack.clock_ids().contains(&"clock".to_string()));
        assert!(pack.is_map("map"));
        assert!(!pack.is_map("clock"));
        assert!(pack.is_book("book"));
        assert!(!pack.is_book("map"));
        assert_eq!(pack.tool_icon("book"), Some("bookshelf"));
        assert!(pack.is_fertilizer("bone_meal"));
        assert!(pack.is_arrow("arrow"));
        assert!(pack.is_bow("bow"));
        assert!(!pack.is_bow("arrow"));
        assert_eq!(pack.tool_icon("bow"), Some("stick"));
        assert!(pack.bow_ids().contains(&"bow".to_string()));
        assert!(pack.is_crossbow("crossbow"));
        assert!(!pack.is_crossbow("bow"));
        assert_eq!(pack.tool_icon("crossbow"), Some("stick"));
        assert!(pack
            .crossbow_ids()
            .contains(&"crossbow".to_string()));
        assert!(pack.is_totem("totem_of_undying"));
        assert!(!pack.is_totem("shield"));
        assert_eq!(pack.tool_icon("totem_of_undying"), Some("glowstone"));
        assert!(pack
            .totem_ids()
            .contains(&"totem_of_undying".to_string()));
        assert!(pack.is_elytra("elytra"));
        assert!(!pack.is_elytra("leather_tunic"));
        assert_eq!(pack.armor_of("elytra"), Some((1, 1)));
        assert!(pack.is_firework("firework_rocket"));
        assert!(!pack.is_firework("wind_charge"));
        assert_eq!(pack.tool_icon("firework_rocket"), Some("glowstone"));
        assert!(pack
            .firework_ids()
            .contains(&"firework_rocket".to_string()));
        assert!(pack.is_splash("splash_healing_potion"));
        assert!(pack.is_splash("splash_harming_potion"));
        assert!(!pack.is_splash("healing_potion"));
        assert_eq!(pack.splash_effect("splash_healing_potion"), Some("heal"));
        assert_eq!(pack.splash_effect("splash_harming_potion"), Some("harm"));
        assert_eq!(
            pack.tool_icon("splash_healing_potion"),
            Some("nether_wart_block")
        );
        assert!(pack
            .splash_ids()
            .contains(&"splash_healing_potion".to_string()));
        assert_eq!(
            pack.pack_recipe("arrow"),
            Some(
                &[
                    "stick".to_string(),
                    "wool".to_string(),
                    "gravel".to_string()
                ][..]
            )
        );
        assert!(pack.pack_recipe("not_a_recipe").is_none());
        assert!(!pack.is_arrow("stick"));
        assert_eq!(pack.tool_icon("arrow"), Some("stick"));
        assert!(!pack.is_fertilizer("book"));
        assert_eq!(pack.tool_icon("bone_meal"), Some("quartz"));
        assert_eq!(pack.tool_icon("map"), Some("wool"));
        assert!(pack.map_ids().contains(&"map".to_string()));
        assert_eq!(
            pack.break_result(Block::new("coal_ore", 0)),
            Block::new("stone", 0)
        );
        assert_eq!(
            pack.break_result(Block::new("iron_ore", 0)),
            Block::new("deepslate", 0)
        );
        assert!(pack.break_result(Block::new("grass", 0)).is_air());
        for (id, _) in pack.blocks() {
            assert!(pack.hardness_of(Block::new(id, 0)).is_some(), "{id}");
        }
    }

    #[test]
    fn a_data_pack_rule_overrides_the_pack_file() {
        let mut pack = Pack::parse(
            r#"
            {
              "shapes": { "cube": { "boxes": [[0, 0, 0, 16, 16, 16]] } },
              "blocks": [
                { "id": "stone", "shape": "cube", "hardness": 9 },
                { "id": "deepslate", "shape": "cube", "hardness": 9 },
                { "id": "coal_ore", "shape": "cube", "replaces": "deepslate", "hardness": 9 }
              ]
            }
            "#,
        )
        .unwrap();
        assert_eq!(pack.hardness_of(Block::new("coal_ore", 0)), Some(9.0));
        assert_eq!(
            pack.break_result(Block::new("coal_ore", 0)),
            Block::new("deepslate", 0)
        );
        pack.apply_rules(&[
            BlockRule {
                id: "stone".to_string(),
                hardness: 1.5,
                replaces: None,
            },
            BlockRule {
                id: "coal_ore".to_string(),
                hardness: 2.0,
                replaces: Some("stone".to_string()),
            },
        ])
        .unwrap();
        assert_eq!(pack.hardness_of(Block::new("stone", 0)), Some(1.5));
        assert_eq!(pack.hardness_of(Block::new("coal_ore", 0)), Some(2.0));
        assert_eq!(
            pack.break_result(Block::new("coal_ore", 0)),
            Block::new("stone", 0)
        );
        assert_eq!(pack.hardness_of(Block::new("deepslate", 0)), Some(9.0));
        assert!(
            pack.apply_rules(&[BlockRule {
                id: "missing".to_string(),
                hardness: 1.0,
                replaces: None,
            }])
            .is_err()
        );
        assert!(
            pack.apply_rules(&[BlockRule {
                id: "coal_ore".to_string(),
                hardness: 1.0,
                replaces: Some("missing".to_string()),
            }])
            .is_err()
        );
        assert!(parse_block_rule("stone", r#"{"hardness":0}"#).is_err());
        assert!(parse_block_rule("stone", r#"{"seconds":1}"#).is_err());
        let ore = parse_block_rule("coal_ore", r#"{"hardness":2,"replaces":"stone"}"#).unwrap();
        assert_eq!(ore.replaces.as_deref(), Some("stone"));
        assert_eq!(ore.hardness, 2.0);
    }

    #[test]
    fn a_mod_data_folder_sets_hardness_and_a_recipe() {
        let root = std::env::temp_dir().join("terralite-mod-data-d4ab");
        let _ = fs::remove_dir_all(&root);
        let source = "mod-data-d4ab.rg";
        let pack = Pack::builtin();
        pack.register_block_from("datakit", "brick", source)
            .unwrap();
        pack.register_item_from("datakit", "chip", source).unwrap();
        let blocks = root.join("data").join("datakit").join("blocks");
        fs::create_dir_all(&blocks).unwrap();
        fs::write(blocks.join("brick.json"), r#"{ "hardness": 3.25 }"#).unwrap();
        let recipes = root.join("data").join("datakit").join("recipes");
        fs::create_dir_all(&recipes).unwrap();
        fs::write(
            recipes.join("chip.json"),
            r#"{ "result": "datakit:chip", "ingredients": ["cobblestone", "dirt"] }"#,
        )
        .unwrap();
        pack.load_mod_data(&root, source).unwrap();
        assert_eq!(pack.hardness_of(Block::new("datakit:brick", 0)), Some(3.25));
        let lines = pack.script_recipe_lines();
        assert!(lines.iter().any(|(result, ingredients)| {
            result == "datakit:chip"
                && ingredients == &vec!["cobblestone".to_string(), "dirt".to_string()]
        }));
        fs::write(
            blocks.join("brick.json"),
            r#"{ "hardness": 3.25, "extra": 1 }"#,
        )
        .unwrap();
        assert!(pack.load_mod_data(&root, source).is_err());
        assert_eq!(pack.hardness_of(Block::new("datakit:brick", 0)), Some(3.25));
        let _ = fs::remove_dir_all(&root);
    }
}
