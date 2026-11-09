//! Decode pack PNGs into one atlas. A bad or missing file is skipped, so the
//! renderer still starts and that block stays flat color.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use rebuild_world::{Block, Pack, asset_png, asset_text};

const TILE: u32 = 16;
/// Extra pixels copied from the tile edge so a chunk-border sample cannot
/// read the next column (dirt sits beside grass and reads as red).
const GUTTER: u32 = 1;
const CELL: u32 = TILE + GUTTER * 2;

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct Atlas {
    image: Image,
    slots: Vec<(String, u32)>,
    tiles: u32,
}

/// Atlas of every pack PNG that decodes to a 16×16 image, or a white pixel
/// when none do.
pub fn load_pack_texture() -> Image {
    atlas().image.clone()
}

/// How many tiles sit side by side in [`load_pack_texture`].
pub fn atlas_tile_count() -> f32 {
    atlas().tiles as f32
}

/// Column of `path` in the atlas. `None` when that file is missing or unusable.
pub fn texture_slot(path: &str) -> Option<u32> {
    atlas()
        .slots
        .iter()
        .find(|(name, _)| name == path)
        .map(|(_, slot)| *slot)
}

/// Atlas column for a block or a placeable item, stored as `slot + 1`.
///
/// `0` means the quad stays a flat color. The GUI and the hotbar use this
/// so a panel can name `stone` and a button can name `cobblestone`.
pub fn block_face(id: &str) -> f32 {
    if id.is_empty() || id == "air" || id == "empty" {
        return 0.0;
    }
    let pack = Pack::builtin();
    if let Some(path) = pack.texture_of(Block::new(id, 0)) {
        if let Some(slot) = texture_slot(path) {
            return (slot + 1) as f32;
        }
    }
    if let Some(icon) = pack.tool_icon(id) {
        if let Some(path) = pack.texture_of(Block::new(icon, 0)) {
            if let Some(slot) = texture_slot(path) {
                return (slot + 1) as f32;
            }
        }
    }
    if let Some(placed) = pack.placed_block(id) {
        if let Some(path) = pack.texture_of(Block::new(&placed, 0)) {
            if let Some(slot) = texture_slot(path) {
                return (slot + 1) as f32;
            }
        }
    }
    0.0
}

fn atlas() -> &'static Atlas {
    static ATLAS: OnceLock<Atlas> = OnceLock::new();
    ATLAS.get_or_init(build_atlas)
}

/// One of the textures named beside `path` when that file asks for a random
/// pick. The path itself is the pick when there is no list.
pub fn varied_texture(path: &str, cell: [i32; 3]) -> String {
    let variants = texture_variants(path);
    if variants.len() <= 1 {
        return path.to_string();
    }
    let mixed = (cell[0] as u32).wrapping_mul(73_856_093)
        ^ (cell[1] as u32).wrapping_mul(19_349_663)
        ^ (cell[2] as u32).wrapping_mul(83_492_791);
    variants[(mixed as usize) % variants.len()].clone()
}

fn texture_variants(path: &str) -> Vec<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("texture variants");
    if let Some(found) = cache.get(path) {
        return found.clone();
    }
    let found = read_texture_variants(path);
    cache.insert(path.to_string(), found.clone());
    found
}

fn read_texture_variants(path: &str) -> Vec<String> {
    let fallback = vec![path.to_string()];
    let Some(text) = asset_text(&format!("{path}.json")) else {
        return fallback;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return fallback;
    };
    if value.get("random_texture").and_then(|flag| flag.as_bool()) != Some(true) {
        return fallback;
    }
    let Some(map) = value
        .get("textures")
        .and_then(|textures| textures.as_object())
    else {
        return fallback;
    };
    let mut keys: Vec<&String> = map
        .keys()
        .filter(|key| key.as_str() != "particle")
        .collect();
    keys.sort();
    let mut resolved = Vec::new();
    for key in keys {
        let Some(name) = map.get(key).and_then(|item| item.as_str()) else {
            continue;
        };
        let Some(file) = namespaced_png(name) else {
            continue;
        };
        if asset_png(&file).is_some() {
            resolved.push(file);
        }
    }
    if resolved.is_empty() {
        fallback
    } else {
        resolved
    }
}

fn namespaced_png(name: &str) -> Option<String> {
    let (namespace, rest) = name.split_once(':').unwrap_or(("terralite", name));
    if namespace.is_empty() || rest.is_empty() || rest.contains("..") {
        return None;
    }
    let file = if rest.ends_with(".png") {
        rest.to_string()
    } else {
        format!("{rest}.png")
    };
    Some(format!("assets/{namespace}/textures/{file}"))
}

fn push_tile(path: &str, tiles: &mut Vec<Image>, slots: &mut Vec<(String, u32)>) {
    if slots.iter().any(|(existing, _)| existing == path) {
        return;
    }
    let Some(bytes) = asset_png(path) else {
        return;
    };
    let Some(image) = decode_png(&bytes) else {
        return;
    };
    if image.width != TILE || image.height != TILE {
        return;
    }
    let slot = tiles.len() as u32;
    slots.push((path.to_string(), slot));
    tiles.push(image);
}

fn mod_pngs() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(mods) = fs::read_dir(r"C:\CODE\TerraLite\mods") else {
        return out;
    };
    for entry in mods.flatten() {
        let assets = entry.path().join("assets");
        if assets.is_dir() {
            walk_pngs(&assets, "assets", &mut out);
        }
    }
    out.sort();
    out
}

fn walk_pngs(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let next = format!("{prefix}/{name}");
        if path.is_dir() {
            walk_pngs(&path, &next, out);
        } else if name.ends_with(".png") {
            out.push(next);
        }
    }
}

fn build_atlas() -> Atlas {
    let mut tiles = Vec::new();
    let mut slots = Vec::new();
    for (id, _) in Pack::builtin().blocks() {
        for metadata in 0..=2 {
            if let Some(path) = Pack::builtin().texture_of(Block::new(id, metadata)) {
                push_tile(path, &mut tiles, &mut slots);
            }
        }
    }
    for id in Pack::builtin().script_block_ids() {
        if let Some(path) = Pack::builtin().texture_of(Block::new(&id, 0)) {
            push_tile(path, &mut tiles, &mut slots);
        }
    }
    for path in mod_pngs() {
        push_tile(&path, &mut tiles, &mut slots);
    }
    let model_textures: Vec<String> = Pack::builtin()
        .model_texture_paths()
        .into_iter()
        .map(str::to_string)
        .collect();
    for path in &model_textures {
        push_tile(path, &mut tiles, &mut slots);
        for variant in texture_variants(path) {
            push_tile(&variant, &mut tiles, &mut slots);
        }
    }
    if tiles.is_empty() {
        return Atlas {
            image: Image {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 255],
            },
            slots,
            tiles: 0,
        };
    }
    let count = tiles.len() as u32;
    let width = CELL * count;
    let mut rgba = vec![0u8; (width * CELL) as usize * 4];
    for (slot, tile) in tiles.iter().enumerate() {
        for y in 0..CELL as usize {
            for x in 0..CELL as usize {
                let sx = x.saturating_sub(GUTTER as usize).min(TILE as usize - 1);
                let sy = y.saturating_sub(GUTTER as usize).min(TILE as usize - 1);
                let src = (sy * TILE as usize + sx) * 4;
                let dst = (y * width as usize + slot * CELL as usize + x) * 4;
                rgba[dst..dst + 4].copy_from_slice(&tile.rgba[src..src + 4]);
            }
        }
    }
    Atlas {
        image: Image {
            width,
            height: CELL,
            rgba,
        },
        slots,
        tiles: count,
    }
}

pub fn decode_png(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::normalize_to_color8(),
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(data.len() / 3 * 4);
            for px in data.chunks(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        _ => return None,
    };
    if rgba.len() != (info.width as usize) * (info.height as usize) * 4 {
        return None;
    }
    Some(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pngs_decode_into_one_atlas_and_a_bad_file_does_not() {
        let paths = [
            "assets/terralite/textures/grass.png",
            "assets/terralite/textures/stone.png",
            "assets/terralite/textures/smooth_stone.png",
            "assets/terralite/textures/quartz.png",
            "assets/terralite/textures/glowstone.png",
            "assets/terralite/textures/sea_lantern.png",
            "assets/terralite/textures/prismarine.png",
            "assets/terralite/textures/dark_prismarine.png",
            "assets/terralite/textures/prismarine_bricks.png",
            "assets/terralite/textures/cobblestone.png",
            "assets/terralite/textures/coal_ore.png",
            "assets/terralite/textures/iron_ore.png",
            "assets/terralite/textures/anvil.png",
            "assets/terralite/textures/enchanting_table.png",
            "assets/terralite/textures/brewing_stand.png",
            "assets/terralite/textures/grindstone.png",
            "assets/terralite/textures/smithing_table.png",
            "assets/terralite/textures/loom.png",
            "assets/terralite/textures/banner.png",
            "assets/terralite/textures/cartography_table.png",
            "assets/terralite/textures/lectern.png",
            "assets/terralite/textures/chiseled_bookshelf.png",
            "assets/terralite/textures/composter.png",
            "assets/terralite/textures/stonecutter.png",
            "assets/terralite/textures/stone_stairs.png",
            "assets/terralite/textures/campfire.png",
            "assets/terralite/textures/fletching_table.png",
            "assets/terralite/textures/smoker.png",
            "assets/terralite/textures/blast_furnace.png",
            "assets/terralite/textures/beehive.png",
            "assets/terralite/textures/cauldron.png",
            "assets/terralite/textures/bell.png",
            "assets/terralite/textures/candle.png",
            "assets/terralite/textures/chain.png",
            "assets/terralite/textures/lodestone.png",
            "assets/terralite/textures/respawn_anchor.png",
            "assets/terralite/textures/lightning_rod.png",
            "assets/terralite/textures/target.png",
            "assets/terralite/textures/copper_bulb.png",
            "assets/terralite/textures/decorated_pot.png",
            "assets/terralite/textures/copper_grate.png",
            "assets/terralite/textures/copper_door.png",
            "assets/terralite/textures/copper_trapdoor.png",
            "assets/terralite/textures/deepslate.png",
            "assets/terralite/textures/blackstone.png",
            "assets/terralite/textures/polished_blackstone.png",
            "assets/terralite/textures/blackstone_bricks.png",
            "assets/terralite/textures/cracked_blackstone_bricks.png",
            "assets/terralite/textures/chiseled_blackstone.png",
            "assets/terralite/textures/gilded_blackstone.png",
            "assets/terralite/textures/basalt.png",
            "assets/terralite/textures/polished_basalt.png",
            "assets/terralite/textures/smooth_basalt.png",
            "assets/terralite/textures/nether_bricks.png",
            "assets/terralite/textures/cracked_nether_bricks.png",
            "assets/terralite/textures/red_nether_bricks.png",
            "assets/terralite/textures/chiseled_nether_bricks.png",
            "assets/terralite/textures/nether_wart_block.png",
            "assets/terralite/textures/soul_sand.png",
            "assets/terralite/textures/magma.png",
            "assets/terralite/textures/netherrack.png",
            "assets/terralite/textures/end_stone.png",
            "assets/terralite/textures/end_portal_frame.png",
            "assets/terralite/textures/dragon_egg.png",
            "assets/terralite/textures/end_crystal.png",
            "assets/terralite/textures/chest.png",
            "assets/terralite/textures/copper_chest.png",
            "assets/terralite/textures/barrel.png",
            "assets/terralite/textures/ender_chest.png",
            "assets/terralite/textures/shulker_box.png",
            "assets/terralite/textures/end_rod.png",
            "assets/terralite/textures/end_stone_bricks.png",
            "assets/terralite/textures/purpur.png",
            "assets/terralite/textures/purpur_pillar.png",
            "assets/terralite/textures/deepslate_bricks.png",
            "assets/terralite/textures/deepslate_tiles.png",
            "assets/terralite/textures/cracked_deepslate_tiles.png",
            "assets/terralite/textures/cobbled_deepslate.png",
            "assets/terralite/textures/polished_deepslate.png",
            "assets/terralite/textures/dirt.png",
            "assets/terralite/textures/water.png",
            "assets/rosegold/textures/rose_block.png",
            "assets/terralite/textures/oak_log.png",
            "assets/terralite/textures/wool.png",
            "assets/terralite/textures/oak_stairs.png",
            "assets/terralite/textures/sign.png",
            "assets/terralite/textures/item_frame.png",
            "assets/terralite/textures/hanging_sign.png",
            "assets/terralite/textures/lantern.png",
            "assets/terralite/textures/bookshelf.png",
            "assets/terralite/textures/sand.png",
            "assets/terralite/textures/sandstone.png",
            "assets/terralite/textures/red_sandstone.png",
            "assets/terralite/textures/cactus.png",
            "assets/terralite/textures/cake.png",
            "assets/terralite/textures/pumpkin.png",
            "assets/terralite/textures/carved_pumpkin.png",
            "assets/terralite/textures/melon.png",
            "assets/terralite/textures/hay_bale.png",
            "assets/terralite/textures/cobweb.png",
            "assets/terralite/textures/vine.png",
            "assets/terralite/textures/lily_pad.png",
            "assets/terralite/textures/snow.png",
            "assets/terralite/textures/sugar_cane.png",
            "assets/terralite/textures/chorus_plant.png",
            "assets/terralite/textures/chorus_flower.png",
            "assets/terralite/textures/ice.png",
            "assets/terralite/textures/clay.png",
            "assets/terralite/textures/bricks.png",
            "assets/terralite/textures/gravel.png",
            "assets/terralite/textures/mossy_cobblestone.png",
            "assets/terralite/textures/mossy_stone_bricks.png",
            "assets/terralite/textures/chiseled_stone_bricks.png",
            "assets/terralite/textures/stone_bricks.png",
            "assets/terralite/textures/cracked_stone_bricks.png",
        ];
        let mut images = Vec::new();
        let mut slots = Vec::new();
        for path in paths {
            let image = decode_png(&asset_png(path).unwrap()).unwrap();
            assert_eq!((image.width, image.height), (16, 16));
            assert_eq!(image.rgba.len(), 16 * 16 * 4);
            images.push(image.rgba);
            slots.push(texture_slot(path).unwrap());
        }
        for i in 0..images.len() {
            for j in (i + 1)..images.len() {
                assert_ne!(images[i], images[j]);
                assert_ne!(slots[i], slots[j]);
            }
        }
        assert!(texture_slot("assets/terralite/textures/missing.png").is_none());
        assert!(decode_png(&[]).is_none());
        assert!(decode_png(b"not a png").is_none());
        let loaded = load_pack_texture();
        assert_eq!(loaded.height, 18);
        assert!(atlas_tile_count() >= paths.len() as f32);
        assert_eq!(loaded.width, 18 * atlas_tile_count() as u32);
        assert!(texture_slot("assets/terralite/textures/block/grass_block_top.png").is_some());
        assert!(
            texture_slot("assets/terralite/textures/block/grass_block_side_overlay.png").is_some()
        );
        assert!(texture_slot("assets/terralite/textures/block/dirt.png").is_some());
        let overlay = decode_png(
            &asset_png("assets/terralite/textures/block/grass_block_side_overlay.png").unwrap(),
        )
        .expect("overlay expands to color");
        let mut clear = false;
        let mut fringe = false;
        for pixel in overlay.rgba.chunks(4) {
            if pixel[3] < 16 {
                clear = true;
            }
            if pixel[3] > 240 && pixel[1] > pixel[2] {
                fringe = true;
            }
        }
        assert!(clear, "overlay keeps transparent texels");
        assert!(fringe, "overlay keeps the grass fringe");
        let grass = texture_slot("assets/terralite/textures/grass.png").unwrap();
        let dirt = texture_slot("assets/terralite/textures/block/dirt.png").unwrap();
        assert_eq!(dirt, grass + 1, "the dirt block is the column after grass");
        assert!(texture_slot("assets/terralite/textures/block/dirt_alt_1.png").is_some());
        assert!(texture_slot("assets/terralite/textures/block/dirt_alt_2.png").is_some());
        assert!(texture_slot("assets/terralite/textures/block/dirt_alt_3.png").is_some());
        let grass_px =
            decode_png(&asset_png("assets/terralite/textures/grass.png").unwrap()).unwrap();
        let dirt_px =
            decode_png(&asset_png("assets/terralite/textures/block/dirt.png").unwrap()).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * loaded.width + x) * 4) as usize;
            [
                loaded.rgba[i],
                loaded.rgba[i + 1],
                loaded.rgba[i + 2],
                loaded.rgba[i + 3],
            ]
        };
        let src = |image: &Image, x: u32, y: u32| {
            let i = ((y * image.width + x) * 4) as usize;
            [
                image.rgba[i],
                image.rgba[i + 1],
                image.rgba[i + 2],
                image.rgba[i + 3],
            ]
        };
        let cell = grass * 18;
        assert_eq!(
            at(cell + 1, 1),
            src(&grass_px, 0, 0),
            "content starts inside the gutter"
        );
        assert_eq!(
            at(cell, 1),
            src(&grass_px, 0, 0),
            "left gutter repeats the grass edge"
        );
        assert_eq!(
            at(cell + 17, 1),
            src(&grass_px, 15, 0),
            "right gutter repeats grass, not dirt"
        );
        assert_ne!(at(cell + 17, 8), src(&dirt_px, 0, 8));
    }
}
