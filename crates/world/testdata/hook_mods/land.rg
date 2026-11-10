// A landing of at least one block plants dirt one block above the feet.
// A shorter hop leaves the cell alone.

fn touch(x: Int, y: Int, z: Int, drop: Int) {
    if (drop >= 1) {
        world.set_block(x, y + 1, z, "dirt");
    }
}

fn main(): Int {
    events.land("mark-fall", touch);
    return 0;
}
