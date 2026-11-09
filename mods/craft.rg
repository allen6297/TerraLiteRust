// Taking a stick from the crafting grid writes dirt at 1, 2, 1 and leaves
// the ingredients in the grid. Any other result is taken as usual.

fn gate(result: String): Bool {
    if (result != "stick") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.craft("no-stick", gate);
    return 0;
}
