// Kiln: script furnace-like station on Tickable + BlockEntities slot contract.
// Playtest: wait ~2s for kiln at (6,1,12) → hold sand, right-click → Put input →
// hold oak_log → Put fuel → wait ticks → Take out (glass). No new events.*.

import ModAPI;
import Blocks;
import BlockEntities;
import GUIAPI;

class KilnPulse impl Tickable {
    fn onTick(n: Int) {
        if (n == 2) {
            world.set_block(6, 1, 12, "kiln:kiln");
            BlockEntities.setSlot(6, 1, 12, "in", "", 0);
            BlockEntities.setSlot(6, 1, 12, "fuel", "", 0);
            BlockEntities.setSlot(6, 1, 12, "out", "", 0);
            BlockEntities.setInt(6, 1, 12, "progress", 0);
            BlockEntities.setInt(6, 1, 12, "burn", 0);
            print("kiln planted");
        }
        if (n > 2) {
            cook_cell(6, 1, 12);
        }
    }
}

fn cook_cell(x: Int, y: Int, z: Int) {
    if (world.get_block(x, y, z) != "kiln:kiln") {
        return;
    }
    var input = BlockEntities.slotId(x, y, z, "in");
    var fuel_n = BlockEntities.slotCount(x, y, z, "fuel");
    var out_id = BlockEntities.slotId(x, y, z, "out");
    var out_n = BlockEntities.slotCount(x, y, z, "out");
    var progress = BlockEntities.getInt(x, y, z, "progress");
    var burn = BlockEntities.getInt(x, y, z, "burn");

    if (input != "sand") {
        if (progress != 0) {
            BlockEntities.setInt(x, y, z, "progress", 0);
        }
        return;
    }
    if (out_id != "" && out_id != "glass") {
        return;
    }
    if (out_n >= 64) {
        return;
    }

    if (burn <= 0) {
        if (fuel_n <= 0) {
            BlockEntities.setInt(x, y, z, "progress", 0);
            return;
        }
        var fuel_id = BlockEntities.slotId(x, y, z, "fuel");
        BlockEntities.setSlot(x, y, z, "fuel", fuel_id, fuel_n - 1);
        burn = 4;
        BlockEntities.setInt(x, y, z, "burn", burn);
    }

    progress = progress + 1;
    burn = burn - 1;
    BlockEntities.setInt(x, y, z, "burn", burn);
    if (progress < 3) {
        BlockEntities.setInt(x, y, z, "progress", progress);
        return;
    }

    var in_n = BlockEntities.slotCount(x, y, z, "in");
    BlockEntities.setSlot(x, y, z, "in", "sand", in_n - 1);
    BlockEntities.setSlot(x, y, z, "out", "glass", out_n + 1);
    BlockEntities.setInt(x, y, z, "progress", 0);
    print("kiln cooked glass");
}

class KilnUse impl Interactable {
    fn interact(player: String, hand: String): Bool {
        return true;
    }

    fn onUseBlock(item: String, block: String, x: Int, y: Int, z: Int): Bool {
        if (block != "kiln:kiln") {
            return true;
        }
        say_status(x, y, z);
        BlockEntities.openAt(x, y, z, "kiln_panel");
        return false;
    }

    fn onUseItem(item: String): Bool {
        return true;
    }

    fn onUseEntity(item: String, kind: String): Bool {
        return true;
    }
}

fn say_status(x: Int, y: Int, z: Int) {
    var input = BlockEntities.slotId(x, y, z, "in");
    var in_n = BlockEntities.slotCount(x, y, z, "in");
    var fuel = BlockEntities.slotId(x, y, z, "fuel");
    var fuel_n = BlockEntities.slotCount(x, y, z, "fuel");
    var out = BlockEntities.slotId(x, y, z, "out");
    var out_n = BlockEntities.slotCount(x, y, z, "out");
    var progress = BlockEntities.getInt(x, y, z, "progress");
    chat.say("kiln in " + input + "x" + in_n);
    chat.say("fuel " + fuel + "x" + fuel_n + " out " + out + "x" + out_n);
    chat.say("cook " + progress + "/3");
}

fn on_kiln_press(row: String) {
    if (row == "put_in") {
        if (BlockEntities.depositFocus("in")) {
            chat.say("stoked input");
        } else {
            chat.say("need matching held item");
        }
        return;
    }
    if (row == "put_fuel") {
        if (BlockEntities.depositFocus("fuel")) {
            chat.say("stoked fuel");
        } else {
            chat.say("need matching held item");
        }
        return;
    }
    if (row == "take_out") {
        if (BlockEntities.withdrawFocus("out")) {
            chat.say("took output");
        } else {
            chat.say("output empty");
        }
        return;
    }
    if (row == "status") {
        say_status(BlockEntities.focusX(), BlockEntities.focusY(), BlockEntities.focusZ());
    }
}

fn kiln_tick(n: Int) {
    var pulse = KilnPulse {};
    pulse.onTick(n);
}

fn kiln_use_block(item: String, block: String, x: Int, y: Int, z: Int): Bool {
    var use = KilnUse {};
    return use.onUseBlock(item, block, x, y, z);
}

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("kiln", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("kiln machine loaded");

    Register.BLOCK.open();
    Register.ITEM.open();
    Register.BLOCK_ENTITY.open();
    Register.SCREEN.open();

    var station = Blocks.defineBlockItem(
        "kiln",
        "kiln",
        "piece",
        "assets/kiln/models/block/kiln.json",
        "assets/kiln/textures/kiln.png",
        2.0,
        0,
        64
    );
    BlockEntities.bind(station.id);
    Register.BLOCK_ENTITY.register("kiln", "kiln");

    var panel = GUIAPI.createScreen("kiln_panel", GUIAPI.vec(300.0, 180.0), GUIAPI.vec(48.0, 48.0));
    GUIAPI.addElement(panel, GUIAPI.heading("title", "kiln"));
    GUIAPI.addElement(panel, GUIAPI.button("put_in", "Put input"));
    GUIAPI.addElement(panel, GUIAPI.button("put_fuel", "Put fuel"));
    GUIAPI.addElement(panel, GUIAPI.button("take_out", "Take out"));
    GUIAPI.addElement(panel, GUIAPI.button("status", "Status"));
    GUIAPI.on_press(panel, "k", on_kiln_press);
    Register.SCREEN.register("kiln", "panel");

    Mod.listenTick("kiln-pulse", kiln_tick);
    Mod.listenUseBlock("kiln-use", kiln_use_block);
    Mod.log("registered " + station.id + " machine");
}

fn main(): Int {
    return 0;
}
