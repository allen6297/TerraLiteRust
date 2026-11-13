// First-hour modder sample (VG-695 / research bet 3).
// Proves register + datapack without new cancelable event spam:
//   1) item + shaped recipe from .rg
//   2) that item drops from datapack loot (datapacks/first-hour → sand)
//   3) one custom Guest players can see / right-click
//   4) EMI picks up the shaped line with no engine edits
// Playtest: enable mod + datapack `first-hour` → wait ~2s for wisp at (8,1,12)
// → E → EMI search `charm` → CRAFTING / USES → break sand → hour:charm →
// right-click the wisp.

import ModAPI;
import Items;
import Entities;

class HourPulse impl Tickable {
    fn onTick(n: Int) {
        if (n == 2) {
            // Friendly Guest on the compound pad east of camp.
            Entities.spawn("hour:wisp", 8, 1, 12);
            print("hour wisp planted");
        }
    }
}

class HourTouch impl Interactable {
    fn interact(player: String, hand: String): Bool {
        return true;
    }

    fn onUseBlock(item: String, block: String, x: Int, y: Int, z: Int): Bool {
        return true;
    }

    fn onUseItem(item: String): Bool {
        return true;
    }

    fn onUseEntity(item: String, kind: String): Bool {
        if (kind == "hour:wisp") {
            chat.say("hour wisp nods");
            return false;
        }
        return true;
    }
}

fn hour_tick(n: Int) {
    var pulse = HourPulse {};
    pulse.onTick(n);
}

fn hour_use_entity(item: String, kind: String): Bool {
    var touch = HourTouch {};
    return touch.onUseEntity(item, kind);
}

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("first-hour", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("first-hour sample loaded");

    Register.ITEM.open();
    Register.ENTITY.open();

    // One item — stack 16, does not place a block.
    var charm = Items.defineItem("hour", "charm", "", 16);
    // Shaped 3×3: cobble over torch over oak_log (column of known pack ids).
    Register.shaped(charm.id, "cobblestone,,,torch,,,oak_log,,");

    // Visible Guest (shipped zombie mesh). Egg in creative; not hostile.
    var wisp = Entities.defineEntity(
        "hour",
        "wisp",
        "assets/terralite/models/entity/zombie.bbmodel",
        "wisp_egg",
        false
    );
    wisp.setHearts(4);
    wisp.setHurtbox(0.9);
    wisp.setSpeed(0.8);

    // Existing ModAPI bridges only — no new events.*.
    Mod.listenTick("hour-pulse", hour_tick);
    Mod.listenUseEntity("hour-touch", hour_use_entity);
    Mod.log("registered " + charm.id + " shaped + " + wisp.id);
}

fn main(): Int {
    return 0;
}
