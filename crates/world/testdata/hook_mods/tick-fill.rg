// Tick plants a grate and charges it with ember fill.
// fill_place must run after the tick Host::call returns (no re-entry deadlock).

fn on_fill(kind: String, x: Int, y: Int, z: Int) {
    if (kind == "ember") {
        world.set_block(1, 2, 1, "dirt");
    }
}

fn pulse(n: Int) {
    if (n == 1) {
        world.set_block(2, 1, 2, "copper_grate");
        world.set_fill(2, 1, 2, "ember");
    }
}

fn main(): Int {
    events.fill_place("mark-fill", on_fill);
    events.tick("fill-pulse", pulse);
    return 0;
}
