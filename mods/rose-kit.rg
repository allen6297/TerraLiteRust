// The rose block is a textured cube. The hotbar stocks it, and a right click
// places it. The rose item places that same block. Spawn writes one with
// world.set_block. A wisp stands just east of the oak stairs. A second wisp
// stands at 12, 1, 14.
// Player, sheep, pig, and zombie ticks run every 50 ms. A long fall hurts.

import helpers;

fn on_place(id: String, x: Int, y: Int, z: Int) {
    if (helpers.same_id(id, "rosegold:rose_block") && (helpers.holding("rosegold:rose_block") || helpers.holding("rosegold:rose"))) {
        print("placed rose");
    }
}

fn on_spawn(kind: String, x: Int, y: Int, z: Int) {
    if (kind == "player") {
        print("spawned " + kind);
        world.set_block(x + 1, y, z, "rosegold:rose_block");
    }
}

fn on_hurt(kind: String, amount: Int): Bool {
    if (kind == "player" && amount < 0) {
        return false;
    }
    return true;
}

fn on_tick(kind: String) {
    if (kind == "player") {
        return;
    }
}

fn on_loot(item: String, x: Int, z: Int, omen: Bool): String {
    if (item == "apple") {
        return "rosegold:rose";
    }
    return item;
}

fn main(): Int {
    block.register("rosegold", "rose_block");
    block.texture("rosegold:rose_block", "assets/rosegold/textures/rose_block.png");
    block.model("rosegold:rose_block", "cube");
    item.register("rosegold", "rose");
    item.places("rosegold:rose", "rosegold:rose_block");
    block.register("rosegold", "rose_bud");
    block.model("rosegold:rose_bud", "assets/rosegold/models/rose_bud.json");
    recipe.register("rosegold:rose_block", "rosegold:rose");
    item.register("rosegold", "bouquet");
    recipe.shapeless("rosegold:bouquet", "rosegold:rose+rosegold:rose");
    item.register("rosegold", "rose_post");
    recipe.shaped("rosegold:rose_post", "rosegold:rose,,,rosegold:rose,,,rosegold:rose,,");
    entity.register("rosegold", "wisp");
    entity.spawn("rosegold:wisp", 12, 1, 14);
    events.placed("rose-kit", on_place);
    events.entity_spawn("rose-kit-spawn", on_spawn);
    events.entity_hurt("rose-kit-hurt", on_hurt);
    events.entity_tick("rose-kit-tick", on_tick);
    events.loot("rose-kit-loot", on_loot);
    return 0;
}
