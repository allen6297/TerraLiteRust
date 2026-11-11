// Ember kit: glow brick tile, placeable embers, shade mob, GUIAPI panel.
// Block entity: glow_brick stores charge; right-click opens the ember panel.
// Block item: ember:ember places glow_brick (creative uses the block model).
// Playtest: load world → wait ~2s for glow at (2,3,1) → right-click it → charge / panel.

import ModAPI;
import Blocks;
import Items;
import Entities;
import BlockEntities;
import GUIAPI;

class EmberPulse impl Tickable {
    fn onTick(n: Int) {
        if (n == 2) {
            world.set_block(2, 3, 1, "ember:glow_brick");
            BlockEntities.setInt(2, 3, 1, "charge", 3);
            print("ember glow tile planted");
            // Compound demo: grate + empty fence, then charge the grate with ember fill.
            world.set_block(4, 1, 12, "copper_grate");
            world.set_block(5, 1, 12, "fence");
            if (world.is_compound(4, 1, 12)) {
                world.set_fill(4, 1, 12, "ember");
                print("ember fill on grate: " + world.get_fill(4, 1, 12));
            }
        }
        if (n > 2 && n % 5 == 0) {
            if (world.get_block(2, 3, 1) == "ember:glow_brick") {
                var c = BlockEntities.getInt(2, 3, 1, "charge");
                if (c > 0) {
                    BlockEntities.setInt(2, 3, 1, "charge", c - 1);
                    world.set_meta(2, 3, 1, c - 1);
                    print("glow charge " + (c - 1));
                }
            }
        }
    }
}

fn on_ember_fill(kind: String, x: Int, y: Int, z: Int) {
    if (kind == "ember") {
        chat.say("ember fill @" + x + "," + y + "," + z);
    }
}

fn on_structure_found(structure: String): Bool {
    chat.say("discovered " + structure);
    if (structure == "ruined_portal" || structure == "village" || structure == "desert_temple") {
        chat.say("token " + structure + " charm");
    }
    return true;
}

class EmberWand impl Interactable {
    fn interact(player: String, hand: String): Bool {
        return true;
    }

    fn onUseBlock(item: String, block: String, x: Int, y: Int, z: Int): Bool {
        if (block == "ember:glow_brick") {
            var c = BlockEntities.getInt(x, y, z, "charge") + 1;
            if (c > 15) {
                c = 0;
            }
            BlockEntities.setInt(x, y, z, "charge", c);
            world.set_meta(x, y, z, c);
            chat.say("glow " + c);
            GUIAPI.openNamed("ember_glow");
            return false;
        }
        if (item != "ember:ember" || block == "") {
            return true;
        }
        world.set_block(x, y + 1, z, "ember:glow_brick");
        BlockEntities.setInt(x, y + 1, z, "charge", 0);
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

fn on_glow_press(row: String) {
    if (row == "charge") {
        chat.say("right click brick");
    }
    if (row == "hint") {
        chat.say("ember places brick");
    }
}

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("ember", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("ember kit loaded");

    Register.BLOCK.open();
    Register.ITEM.open();
    Register.ENTITY.open();
    Register.BLOCK_ENTITY.open();
    Register.SCREEN.open();

    var brick = Blocks.defineBlockItem(
        "ember",
        "glow_brick",
        "ember",
        "assets/ember/models/block/glow_brick.json",
        "assets/ember/textures/glow_brick.png",
        2.0,
        12,
        16
    );
    BlockEntities.bind(brick.id);
    Register.BLOCK_ENTITY.register("ember", "glow_brick");
    Register.recipe(brick.id, "ember:ember");

    var panel = GUIAPI.createScreen("ember_glow", GUIAPI.vec(280.0, 140.0), GUIAPI.vec(48.0, 64.0));
    GUIAPI.addElement(panel, GUIAPI.heading("title", "ember glow"));
    GUIAPI.addElement(panel, GUIAPI.button("charge", "charge tip"));
    GUIAPI.addElement(panel, GUIAPI.text("hint", "hold ember, place brick"));
    GUIAPI.on_press(panel, "g", on_glow_press);
    Register.SCREEN.register("ember", "glow");

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
    Mod.listenFillPlace("ember-fill-place", on_ember_fill);
    Mod.listenStructureDiscovered("ember-find", on_structure_found);
    Mod.log("registered " + brick.id + ", ember:ember, " + shade.id + ", ember_glow");
}

fn main(): Int {
    return 0;
}
