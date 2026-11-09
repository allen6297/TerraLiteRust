// Weaving wool into a banner writes dirt at 1, 2, 1 and leaves the wool in
// the bar. Any other id is allowed.

fn gate(wool: String): Bool {
    if (wool != "wool") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.weave("keep-wool", gate);
    return 0;
}
