//! The real mods folder, in its own process so pack registrations start empty.

use std::path::Path;

use rebuild_world::{Hooks, load_dir};

#[test]
fn shipped_mods_still_load_with_the_guide() {
    let mut hooks = Hooks::new();
    let loaded = load_dir(Path::new(r"C:\CODE\TerraLite\mods"), &mut hooks).expect("mods load");
    assert_eq!(loaded.len(), 4);
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
    let pack = rebuild_world::Pack::builtin();
    assert!(pack.contains_block("guide:marker"));
    assert!(pack.contains_block("ember:glow_brick"));
    assert!(pack.contains_block("craft:paver"));
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
}
