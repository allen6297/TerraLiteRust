//! Shared world data for this game.
//!
//! The world names blocks with strings (`stone`, `oak_stairs`). Shapes, models,
//! and blocks are registered from the pack by those ids. Anvil region
//! files still store 1.8 numeric ids, metadata nibbles, and an optional Add
//! nibble. That translation stays at the file boundary. Chunks are not stored
//! with the later block-state palette. A world is a finite square: small,
//! medium, large, or mega. The synthetic demo is small: 31×31 chunks, 496
//! blocks on a side. Past the center 3×3 demo pad, chunks fill with biome
//! terrain a few at a time. The
//! private localhost frames live in this crate. They are version 1, and they
//! are not protocol 47. Tag 6 is the player hotbar. Mods register Fabric-style
//! `events` (`break_block`, `place_block`, `broken`, `placed`, `pickup_item`,
//! `break_drop`, `loot`, `entity_spawn`, `entity_hurt`, `entity_tick`, `use_item`,
//! `use_block`, `use_entity`, `attack`, `chat`, `respawn`, `craft`, `smelt`,
//! `repair`, `enchant`, `brew`, `smith`, `weave`, `tick`, `dawn`, `dusk`,
//! `jump`, `sprint`, `land`, `command`) or the older
//! `hooks` names. `block.register` and `item.register` add ids during `main`.
//! `entity.spawn` and `entity.despawn` place and remove a registered mob.
//! A registered block places from the hotbar and from `world.set_block`.
//! A registered item can be held and dropped, and it is not a block.
//! Inside a hook, `world`, `player`, and `item` read the level, the player, and
//! the pack. The server hotbar is what `player.held` and `world.slot` see online.

mod changes;
mod chunk;
mod cluster;
mod edit;
mod error;
mod fixture;
mod hooks;
mod ids;
mod light;
mod nbt;
mod net;
mod pack;
mod region;
mod registry;
mod rosegold;
mod shape;
mod size;
mod terrain;

pub use changes::{
    CarriedSlot, CellChange, HOTBAR_SAVE_SLOTS, HotbarSave, INVENTORY_SLOTS, PlayerPlace,
    StoredChest, apply_changes, bar_from_wire, carried_from_wire, ensure_chest, parse_inventory,
    read_changes, read_chests, read_ender_chest, read_hotbar, read_inventory, read_player,
    remove_chest, store_chest_slots, write_changes, write_chests, write_hotbar, write_inventory,
    write_player,
};
pub use chunk::{Block, Chunk};
pub use cluster::{Cluster, load_cluster_from_path, load_cluster_from_region};
pub use edit::{Edit, PlayerBox, apply};
pub use error::Error;
pub use fixture::{synthetic_chunk, synthetic_cluster, synthetic_cluster_sized, synthetic_region};
pub use hooks::{
    AfterBreak, AfterPlace, BeforeBreak, BeforePlace, BreakVerdict, EntityClock, Hooks,
    PLAYER_SPAWN, TestHook, TestHookEvent, apply_authoritative,
};
pub use light::{Emitter, LIGHT_REACH, LightField};
pub use net::{
    BAG_SLOTS, CellUpdate, ClientRequest, DEFAULT_PORT, HOTBAR_SLOTS, MAX_FRAME, NetCell,
    PROTOCOL_VERSION, PeerPose, TAG_BAG, TAG_BREAK, TAG_CELL, TAG_CHAT, TAG_CHEST, TAG_HELLO,
    TAG_HOTBAR, TAG_PEERS, TAG_PLACE, TAG_POSE, TAG_SAVE, TAG_WELCOME, TAG_WORLD, Welcome,
    chat_line, decode_cell, decode_client, decode_peers, decode_welcome, encode_bag, encode_break,
    encode_cell, encode_chat, encode_chest, encode_hello, encode_hotbar, encode_peers,
    encode_place, encode_pose, encode_save, encode_welcome, encode_world_size, localhost,
    occupied_cells, read_frame, valid_save_name, world_size_from_radius, write_frame,
};
pub use pack::{
    BlockEntry, ItemDisplay, Light, Pack, ShapeEntry, asset_png, asset_text,
    block_definitions_file, data_pack_root, for_each_collider, harvest_of, held_light_of,
    light_of, load_data_pack, resource_pack_root, shape_of, shelf_facing, shelf_filled,
    shelf_meta,
};
pub use rebuild_rosegold::{GuiElement, GuiScreen, SettingOption};
pub use region::{load_chunk_from_path, load_chunk_from_region};
pub use registry::Registry;
pub use rosegold::{ScriptHook, ScriptView, load_dir, load_source};
pub use shape::{
    COAL_ORE, CellBox, DEEPSLATE, IRON_ORE, LANTERN, MeshFace, OAK_STAIRS, Plane, SIGN, STONE,
    Shape,
};
pub use size::WorldSize;
pub use terrain::{
    Biome, CAMP_CHEST, CAMP_FIRE, CAMP_SMOKER, CLAY_BANK, SHAFT_BLAST, SHAFT_CHEST, SHAFT_MOUTH,
    SHRINE, SHRINE_CHEST, SHRINE_FURNACE, TerrainPump,
    biome_at,
    ensure_terrain, generate_chunk, seed_terrain_ring, sky_tint,
};
