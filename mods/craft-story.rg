// Craft story: ModAPI recipe registration (shapeless + shaped) with placeable results.
// Playtest: E → EMI search "paver" / "pole" → CRAFTING shows the mod lines → Fill on Craft.

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

    Mod.log("registered " + paver.id + " + " + pole.id + " recipes");
}

fn main(): Int {
    return 0;
}
