//! The real mods folder, in its own process so pack registrations start empty.

use std::path::Path;

use rebuild_world::{Hooks, load_dir};

#[test]
fn shipped_mods_still_load_with_the_guide() {
    let mut hooks = Hooks::new();
    let loaded = load_dir(Path::new(r"C:\CODE\TerraLite\mods"), &mut hooks).expect("mods load");
    assert_eq!(loaded.len(), 31);
    let guide = Path::new("k's-guide").join("src").join("mod.rg");
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with(&guide) && hooks.is_empty() })
    );
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("keep-stone.rg") && hooks.iter().any(|hook| hook.id() == "keep-stone")
    }));
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("rose-kit.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("log-place.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("log-break.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("log-player.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("field-notes.rg") && !hooks.is_empty() })
    );
    assert!(
        loaded
            .iter()
            .any(|(path, hooks)| { path.ends_with("red-wool.rg") && !hooks.is_empty() })
    );
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("use-wand.rg")
            && hooks.iter().any(|hook| hook.id() == "wand-plant")
            && hooks.iter().any(|hook| hook.id() == "wand-hold")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("spark.rg") && hooks.iter().any(|hook| hook.id() == "spark")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("use-mob.rg") && hooks.iter().any(|hook| hook.id() == "calm-wolf")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("attack.rg") && hooks.iter().any(|hook| hook.id() == "glove-guard")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("seal.rg")
            && hooks.iter().any(|hook| hook.id() == "seal-dig")
            && hooks.iter().any(|hook| hook.id() == "seal-place")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("hush.rg") && hooks.iter().any(|hook| hook.id() == "hush-secret")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("respawn.rg") && hooks.iter().any(|hook| hook.id() == "mark-return")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("craft.rg") && hooks.iter().any(|hook| hook.id() == "no-stick")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("smelt.rg") && hooks.iter().any(|hook| hook.id() == "keep-iron")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("repair.rg") && hooks.iter().any(|hook| hook.id() == "keep-ore")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("enchant.rg") && hooks.iter().any(|hook| hook.id() == "keep-book")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("brew.rg") && hooks.iter().any(|hook| hook.id() == "keep-wart")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("smith.rg") && hooks.iter().any(|hook| hook.id() == "keep-pick")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("weave.rg") && hooks.iter().any(|hook| hook.id() == "keep-wool")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("tick.rg") && hooks.iter().any(|hook| hook.id() == "one-mark")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("dawn.rg") && hooks.iter().any(|hook| hook.id() == "first-light")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("dusk.rg") && hooks.iter().any(|hook| hook.id() == "last-light")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("jump.rg") && hooks.iter().any(|hook| hook.id() == "stay-down")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("sprint.rg") && hooks.iter().any(|hook| hook.id() == "stay-slow")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("land.rg") && hooks.iter().any(|hook| hook.id() == "mark-fall")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("notes.rg") && hooks.iter().any(|hook| hook.id() == "notes")
    }));
    assert!(loaded.iter().any(|(path, hooks)| {
        path.ends_with("glow.rg") && hooks.iter().any(|hook| hook.id() == "glow")
    }));
    assert!(
        loaded
            .iter()
            .any(|(path, _hooks)| path.ends_with("ember.rg"))
    );
    let pack = rebuild_world::Pack::builtin();
    assert!(pack.contains_item("wand:rose"));
    assert!(pack.contains_item("mob:calm"));
    assert!(pack.contains_item("hit:glove"));
    assert!(pack.contains_block("guide:marker"));
    assert!(pack.contains_block("ember:glow_brick"));
    assert!(pack.contains_item("ember:ember"));
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
    let glow = pack
        .settings()
        .into_iter()
        .find(|option| option.name == "glow")
        .expect("glow setting");
    assert_eq!(glow.category, "game");
    assert_eq!(glow.category_text, "how it feels");
    assert_eq!(glow.description, "soft glow");
    assert_eq!(glow.value, "on");
    assert!(glow.choices.is_empty());
    let pace = pack
        .settings()
        .into_iter()
        .find(|option| option.name == "pace")
        .expect("pace setting");
    assert_eq!(pace.value, "mid");
    assert_eq!(
        pace.choices,
        vec!["low".to_string(), "mid".to_string(), "high".to_string()]
    );
    assert!(pack.toggle_setting("pace"));
    let mut world = rebuild_world::synthetic_cluster();
    let said = hooks.setting_changed(&mut world, "pace", &rebuild_world::ScriptView::bare());
    assert_eq!(said, vec!["pace high".to_string()]);
    assert!(pack.toggle_setting("glow"));
    let mut world = rebuild_world::synthetic_cluster();
    let said = hooks.setting_changed(&mut world, "glow", &rebuild_world::ScriptView::bare());
    assert_eq!(said, vec!["glow off".to_string()]);
    let notes = pack
        .gui_screens()
        .into_iter()
        .find(|screen| screen.name == "notes")
        .expect("notes screen");
    assert_eq!(notes.key, "n");
    assert_eq!(notes.elements.len(), 9);
    assert_eq!(notes.elements[0].name, "places");
    assert_eq!(notes.elements[0].font, "heading");
    assert_eq!(notes.elements[0].y, 40.0);
    assert_eq!(notes.elements[1].name, "oakface");
    assert_eq!(notes.elements[1].font, "icon");
    assert_eq!(notes.elements[1].text, "oak_log");
    assert_eq!(notes.elements[1].y, 68.0);
    assert_eq!(notes.elements[2].name, "oak");
    assert_eq!(notes.elements[2].y, 68.0);
    assert_eq!(notes.elements[2].color, 36 * 65536 + 78 * 256 + 40);
    assert_eq!(notes.elements[3].name, "mark");
    assert_eq!(notes.elements[3].text, "mark oak");
    assert_eq!(notes.elements[3].font, "button");
    assert_eq!(notes.elements[3].x, 204.0);
    assert_eq!(notes.elements[3].y, 68.0);
    assert_eq!(notes.elements[4].name, "pool");
    assert_eq!(notes.elements[4].text, "water");
    assert_eq!(notes.elements[5].name, "marsh");
    assert_eq!(notes.elements[5].y, 108.0);
    assert_eq!(notes.elements[5].color, 28 * 65536 + 78 * 256 + 110);
    assert_eq!(notes.elements[6].name, "peck");
    assert_eq!(notes.elements[6].text, "raw");
    assert_eq!(notes.elements[7].name, "hen");
    assert_eq!(notes.elements[7].y, 148.0);
    assert_eq!(notes.elements[7].color, 220 * 65536 + 190 * 256 + 120);
    assert_eq!(notes.elements[7].font_color, 0);
    assert_eq!(notes.elements[8].name, "line");
    assert_eq!(notes.elements[8].font, "field");
    assert_eq!(notes.elements[8].text, "");
    assert_eq!(notes.elements[8].y, 196.0);
    let mut world = rebuild_world::synthetic_cluster();
    let said = hooks.screen_row(
        &mut world,
        "notes",
        "oak",
        &rebuild_world::ScriptView::bare(),
    );
    assert_eq!(said, vec!["oak west of the stairs".to_string()]);
    let said = hooks.screen_row(
        &mut world,
        "notes",
        "mark",
        &rebuild_world::ScriptView::bare(),
    );
    assert_eq!(said, vec!["oak marked".to_string()]);
    let notes = rebuild_world::Pack::builtin()
        .gui_screens()
        .into_iter()
        .find(|screen| screen.name == "notes")
        .expect("notes screen");
    assert_eq!(notes.elements.len(), 10);
    assert_eq!(notes.elements[9].name, "kept");
    assert_eq!(notes.elements[9].text, "oak noted");
    assert_eq!(notes.elements[9].y, 236.0);
    assert_eq!(notes.elements[9].font, "capital");
    let said = hooks.screen_row(
        &mut world,
        "notes",
        "kept",
        &rebuild_world::ScriptView::bare(),
    );
    assert_eq!(said, vec!["oak noted".to_string()]);
    let said = hooks.screen_row_text(
        &mut world,
        "notes",
        "line",
        Some("west oak"),
        &rebuild_world::ScriptView::bare(),
    );
    assert_eq!(said, vec!["west oak".to_string()]);
    let notes = rebuild_world::Pack::builtin()
        .gui_screens()
        .into_iter()
        .find(|screen| screen.name == "notes")
        .expect("notes screen");
    assert_eq!(notes.elements.len(), 9);
    assert!(notes.elements.iter().all(|row| row.name != "kept"));
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
            .any(|(kind, id)| kind == "COMMAND" && id == "spark")
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
}
