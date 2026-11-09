// Creative lists wand:rose because it can place wand:mark. A right click on a
// block plants dirt there instead, and the click does not also place the mark.
// Any other item falls through to the usual eat, place, and feed.

fn plant(item: String, block: String, x: Int, y: Int, z: Int): Bool {
    if (item != "wand:rose" || block == "") {
        return true;
    }
    world.set_block(x, y + 1, z, "dirt");
    return false;
}

fn hold(item: String): Bool {
    return item != "wand:rose";
}

fn main(): Int {
    block.register("wand", "mark");
    item.register("wand", "rose");
    item.places("wand:rose", "wand:mark");
    events.use_block("wand-plant", plant);
    events.use_item("wand-hold", hold);
    return 0;
}
