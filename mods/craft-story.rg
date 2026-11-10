// Craft story: ModAPI recipes + loot tables (block break + chest override).
// Playtest: break dirt → torch + apple; open camp chest → craft:paver first pick;
// break grass → dirt + wheat_seeds (datapack loot/blocks/grass.json).

import ModAPI;
import Blocks;

fn on_init() {
    var Mod: TLMod = ModAPI.createMod("craft", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);
    Mod.log("craft story loaded");

    Register.BLOCK.open();
    Register.ITEM.open();

    var paver = Blocks.defineBlock(
        "craft",
        "paver",
        "cube",
        "assets/craft/textures/paver.png",
        1.5,
        0
    );
    Register.shapeless(paver.id, "bricks+bricks");

    var pole = Blocks.defineBlock(
        "craft",
        "pole",
        "cube",
        "assets/craft/textures/pole.png",
        1.0,
        0
    );
    Register.shaped(pole.id, "oak_log,,,oak_log,,,oak_log,,");

    // Mod loot: dirt break drops torch + apple; camp chest drops start with paver.
    Register.blockLoot("dirt", "torch+apple");
    Register.chestLoot("camp_chest", "craft:paver+torch+apple+map");

    Mod.log("registered " + paver.id + " + " + pole.id + " recipes + loot");
}

fn main(): Int {
    return 0;
}
