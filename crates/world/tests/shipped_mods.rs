//! The real mods folder, in its own process so pack registrations start empty.

use std::path::Path;

use rebuild_world::{Hooks, load_dir};

#[test]
fn shipped_mods_still_load_with_the_guide() {
    let mut hooks = Hooks::new();
    let loaded = load_dir(Path::new(r"C:\CODE\TerraLite\mods"), &mut hooks).expect("mods load");
    assert_eq!(loaded.len(), 6);
    let guide = Path::new("k's-guide").join("src").join("mod.rg");
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with(&guide) && hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("rose-kit.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, _hooks)| path.ends_with("ember.rg"))
    );
    assert!(
        loaded
            .iter()
            .any(|(path, _hooks)| path.ends_with("craft-story.rg"))
    );
    assert!(
        loaded
            .iter()
            .any(|(path, _hooks)| path.ends_with("kiln.rg"))
    );
    assert!(
        loaded
            .iter()
            .any(|(path, _hooks)| path.ends_with("first-hour.rg"))
    );
    let pack = rebuild_world::Pack::builtin();
    assert!(pack.contains_block("guide:marker"));
    assert!(pack.contains_block("ember:glow_brick"));
    assert!(pack.contains_block("kiln:kiln"));
    assert!(pack.contains_block("craft:paver"));
    assert!(pack.is_script_block_entity("kiln:kiln"));
    assert!(pack.has_gui_screen("kiln_panel"));
    assert_eq!(
        pack.script_block_loot("dirt").as_deref(),
        Some(&["torch".to_string(), "apple".to_string()][..])
    );
    assert_eq!(
        pack.script_chest_loot("camp_chest").as_deref(),
        Some(
            &[
                "craft:paver".to_string(),
                "torch".to_string(),
                "apple".to_string(),
                "map".to_string()
            ][..]
        )
    );
    assert!(
        pack.script_smelt_lines().iter().any(|(result, input, station)| {
            result == "craft:paver"
                && input == "dirt"
                && *station == rebuild_world::HeatStation::Furnace
        })
    );
    assert!(pack.contains_item("ember:ember"));
    assert!(pack.is_script_block_entity("ember:glow_brick"));
    assert!(pack.is_block_entity("ember:glow_brick"));
    assert!(!pack.is_container("ember:glow_brick"));
    assert!(pack.has_gui_screen("ember_glow"));
    assert_eq!(
        pack.hardness_of(rebuild_world::Block::new("ember:glow_brick", 0)),
        Some(2.0)
    );
    assert_eq!(
        pack.light_of(rebuild_world::Block::new("ember:glow_brick", 0))
            .map(|light| light.level),
        Some(12)
    );
    assert_eq!(pack.max_stack("ember:ember"), 16);
    assert_eq!(
        pack.placed_block("ember:ember").as_deref(),
        Some("ember:glow_brick")
    );
    assert_eq!(
        pack.texture_of(rebuild_world::Block::new("ember:glow_brick", 0)),
        Some("assets/ember/textures/glow_brick.png")
    );
    assert_eq!(
        pack.hardness_of(rebuild_world::Block::new("guide:marker", 0)),
        Some(2.5)
    );
    assert!(
        pack.script_recipes()
            .iter()
            .any(|(result, ingredient)| result == "guide:marker" && ingredient == "cobblestone")
    );
    assert_eq!(
        pack.placed_block("guide:page").as_deref(),
        Some("guide:marker")
    );
    assert!(pack.script_screens().iter().any(|id| id == "guide:book"));
    assert!(
        hooks
            .script_content()
            .iter()
            .any(|(kind, id)| kind == "COMMAND" && id == "guide:day")
    );
    assert!(
        hooks
            .script_content()
            .iter()
            .any(|(kind, id)| kind == "COMMAND" && id == "guide:home")
    );
    assert!(
        hooks
            .script_content()
            .iter()
            .any(|(kind, id)| kind == "COMMAND" && id == "guide:night")
    );
    assert!(
        hooks
            .script_content()
            .iter()
            .any(|(kind, id)| kind == "PARTICLE" && id == "guide:spark")
    );
    assert!(pack.contains_block("guide:post"));
    assert_eq!(
        pack.hardness_of(rebuild_world::Block::new("guide:post", 0)),
        Some(1.25)
    );
    assert_eq!(
        pack.placed_block("guide:slip").as_deref(),
        Some("guide:post")
    );
    assert!(pack.is_block_entity("guide:chest"));
    assert_eq!(
        pack.placed_block("guide:key").as_deref(),
        Some("guide:chest")
    );
    assert!(pack.contains_block("rosegold:rose_block"));
    assert!(pack.contains_item("rosegold:rose"));
    // VG-695 first-hour: item + shaped + Guest (datapack loot proved separately).
    assert!(pack.contains_item("hour:charm"));
    assert_eq!(pack.max_stack("hour:charm"), 16);
    assert!(
        pack.script_patterns()
            .iter()
            .any(|(result, pattern)| {
                result == "hour:charm"
                    && pattern.as_slice()
                        == [
                            "cobblestone".to_string(),
                            String::new(),
                            String::new(),
                            "torch".to_string(),
                            String::new(),
                            String::new(),
                            "oak_log".to_string(),
                            String::new(),
                            String::new(),
                        ]
            })
    );
    assert!(
        pack.script_entity_def("hour:wisp")
            .is_some_and(|def| !def.hostile)
    );
    // Datapack overlay: sand → sand + hour:charm (register+datapack story).
    let prior = rebuild_world::datapack_selection();
    let mut sel = prior.clone();
    sel.set_enabled("first-hour", true);
    rebuild_world::install_datapack_selection(sel).expect("enable first-hour datapack");
    let sand_drops = rebuild_world::break_loot_of("sand");
    assert!(
        sand_drops.iter().any(|(id, _)| id == "hour:charm"),
        "expected hour:charm in sand loot, got {sand_drops:?}"
    );
    assert!(sand_drops.iter().any(|(id, _)| id == "sand"));
    rebuild_world::install_datapack_selection(prior).expect("restore datapacks");
}
