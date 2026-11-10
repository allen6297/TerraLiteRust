// A sprint from cell y 4 stays at a walk. Any other cell plants dirt
// one block above the feet and the walk goes fast.

fn dash(x: Int, y: Int, z: Int): Bool {
    if (y == 4) {
        return false;
    }
    world.set_block(x, y + 1, z, "dirt");
    return true;
}

fn main(): Int {
    events.sprint("stay-slow", dash);
    return 0;
}
