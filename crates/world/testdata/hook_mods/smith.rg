// Upgrading an iron pickaxe at the smithing table writes dirt at 1, 2, 1 and
// leaves the iron ore unspent. A stone pickaxe still becomes iron, and a mace
// still takes a heavy core.

fn gate(item: String): Bool {
    if (item != "iron_pickaxe") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.smith("keep-pick", gate);
    return 0;
}
