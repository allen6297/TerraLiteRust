// Digging seal:cap is consumed, and dirt is written above that cell.
// Placing seal:wax is consumed, and dirt is written at 1, 2, 1.
// Other digs and places go through.

fn on_dig(block: String, x: Int, y: Int, z: Int): Bool {
    if (block != "seal:cap") {
        return true;
    }
    world.set_block(x, y + 1, z, "dirt");
    return false;
}

fn on_place(item: String, x: Int, y: Int, z: Int): Bool {
    if (item != "seal:wax") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    block.register("seal", "cap");
    block.register("seal", "wax");
    events.break("seal-dig", on_dig);
    events.place("seal-place", on_place);
    return 0;
}
