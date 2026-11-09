// Ember kit: a warm glow brick and a handful of placeable embers.
// The block model lives under the per-block layout:
//   mods/ember/assets/ember/models/block/glow_brick.json
// Playtest: Items list → ember:glow_brick / ember:ember → place near stairs → dig (harder than dirt) → see the warm light.

import ModAPI;
import Blocks;
import Items;

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("ember", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("ember kit loaded");

    Register.BLOCK.open();
    Register.ITEM.open();

    var brick = Blocks.defineBlock(
        "ember",
        "glow_brick",
        "assets/ember/models/block/glow_brick.json",
        "assets/ember/textures/glow_brick.png",
        2.0,
        12
    );
    var ember = Items.defineItem("ember", "ember", brick, 16);
    Register.recipe(brick, ember);
    Mod.log("registered " + brick + " and " + ember);
}

fn main(): Int {
    return 0;
}
