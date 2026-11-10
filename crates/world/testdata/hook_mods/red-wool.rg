// Place wool and it is dyed red. Stone broken while holding cobblestone drops
// cobblestone. Dirt drops stay on the ground.

fn on_place(id: String, x: Int, y: Int, z: Int) {
    if (id == "wool" && world.get_block(x, y, z) == "wool" && world.get_meta(x, y, z) == 0) {
        world.set_meta(x, y, z, 14);
    }
}

fn on_drop(block: String, left: String): String {
    if (block == "stone" && left != "stone" && player.held() == "cobblestone") {
        return "cobblestone";
    }
    return block;
}

fn on_pickup(id: String): Bool {
    if (id == "dirt") {
        return false;
    }
    if (world.has_time() && world.is_day() && world.season() == "spring" && player.present() && player.x() > 0.0 && item.exists(id) && item.max_count(id) == 64) {
        print("day");
    }
    return item.exists(id);
}

fn main(): Int {
    events.placed("red-wool", on_place);
    events.break_drop("stone-to-cobble", on_drop);
    events.pickup_item("leave-dirt", on_pickup);
    return 0;
}
