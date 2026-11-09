// A death writes dirt one block above the return cell and still lets the
// player come back. A bed stays awake. An anchor still saves the spawn.

fn back(cause: String, x: Int, y: Int, z: Int): Bool {
    if (cause == "death") {
        world.set_block(x, y + 1, z, "dirt");
        return true;
    }
    if (cause == "bed") {
        return false;
    }
    return true;
}

fn main(): Int {
    events.respawn("mark-return", back);
    return 0;
}
