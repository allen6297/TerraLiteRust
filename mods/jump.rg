// A jump from cell y 4 stays on the ground. Any other cell plants dirt
// one block above the feet and the jump launches.

fn leap(x: Int, y: Int, z: Int): Bool {
    if (y == 4) {
        return false;
    }
    world.set_block(x, y + 1, z, "dirt");
    return true;
}

fn main(): Int {
    events.jump("stay-down", leap);
    return 0;
}
