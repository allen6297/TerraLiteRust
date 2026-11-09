// Repairing an iron pickaxe at the anvil writes dirt at 1, 2, 1 and leaves
// the iron ore unspent. Any other worn pick is repaired as usual.

fn gate(item: String): Bool {
    if (item != "iron_pickaxe") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.repair("keep-ore", gate);
    return 0;
}
