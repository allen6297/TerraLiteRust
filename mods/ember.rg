// Ember kit: glow brick, placeable embers, hostile shade, and ModAPI traits.
// Tickable: once a second, tick 2 plants a dirt marker at (2, 2, 1).
// Interactable: right-click a block while holding ember:ember plants glow_brick on top.
// Playtest: load world → wait ~2s for dirt → hold ember → right-click stone/dirt near stairs.

import ModAPI;
import Blocks;
import Items;
import Entities;

class EmberPulse impl Tickable {
    fn onTick(n: Int) {
        if (n == 2) {
            world.set_block(2, 2, 1, "dirt");
            print("ember pulse");
        }
    }
}

class EmberWand impl Interactable {
    fn interact(player: String, hand: String): Bool {
        return true;
    }

    fn onUseBlock(item: String, block: String, x: Int, y: Int, z: Int): Bool {
        if (item != "ember:ember" || block == "") {
            return true;
        }
        world.set_block(x, y + 1, z, "ember:glow_brick");
        print("ember wand placed glow_brick");
        return false;
    }

    fn onUseItem(item: String): Bool {
        return true;
    }

    fn onUseEntity(item: String, kind: String): Bool {
        return true;
    }
}

fn ember_tick(n: Int) {
    var pulse = EmberPulse {};
    pulse.onTick(n);
}

fn ember_use_block(item: String, block: String, x: Int, y: Int, z: Int): Bool {
    var wand = EmberWand {};
    return wand.onUseBlock(item, block, x, y, z);
}

fn on_shade_spawn(kind: String, x: Int, y: Int, z: Int): Bool {
    if (kind == "ember:shade") {
        print("ember shade at " + kind);
    }
    return true;
}

fn on_shade_hurt(kind: String, amount: Int): Int {
    if (kind == "ember:shade") {
        print("ember shade hurt halved");
        return amount / 2;
    }
    return amount;
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

    Mod.listenTick("ember-pulse", ember_tick);
    Mod.listenUseBlock("ember-wand", ember_use_block);
    Mod.log("registered " + brick.id + ", " + ember.id + ", and " + shade.id);
}

fn main(): Int {
    return 0;
}
