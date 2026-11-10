// Ember kit: a warm glow brick, a handful of placeable embers, and a hostile
// shade that reuses the Blockbench zombie mesh.
// The block model lives under the per-block layout:
//   mods/ember/assets/ember/models/block/glow_brick.json
// Playtest: Items list → ember:glow_brick / ember:ember → place near stairs → dig
// (harder than dirt) → see the warm light. ember:shade stands at (14,1,12) with
// zombie.bbmodel + chase/attack. Creative ember:shade_egg right-clicks to spawn more.

import ModAPI;
import Blocks;
import Items;
import Entities;

fn on_shade_spawn(kind: String, x: Int, y: Int, z: Int): Bool {
    if (kind == "ember:shade") {
        print("ember shade at " + kind);
    }
    return true;
}

fn on_shade_hurt(kind: String, amount: Int): Bool {
    if (kind == "ember:shade" && amount < 0) {
        return false;
    }
    return true;
}

fn on_shade_death(kind: String, x: Int, y: Int, z: Int): Bool {
    if (kind == "ember:shade") {
        print("ember shade fell");
    }
    return true;
}

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("ember", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("ember kit loaded");

    Register.BLOCK.open();
    Register.ITEM.open();
    Register.ENTITY.open();

    var brick = Blocks.defineBlock(
        "ember",
        "glow_brick",
        "assets/ember/models/block/glow_brick.json",
        "assets/ember/textures/glow_brick.png",
        2.0,
        12
    );
    var ember = Items.defineItem("ember", "ember", brick.id, 16);
    Register.recipe(brick.id, ember.id);

    var shade = Entities.defineEntity(
        "ember",
        "shade",
        "assets/terralite/models/entity/zombie.bbmodel",
        "shade_egg",
        true
    );
    shade.spawn(14, 1, 12);
    events.entity_spawn("ember-shade-spawn", on_shade_spawn);
    events.entity_hurt("ember-shade-hurt", on_shade_hurt);
    events.entity_death("ember-shade-death", on_shade_death);
    Mod.log("registered " + brick.id + ", " + ember.id + ", and " + shade.id);
}

fn main(): Int {
    return 0;
}
