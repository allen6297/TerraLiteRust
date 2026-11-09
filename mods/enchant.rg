// Enchanting an iron pickaxe writes dirt at 1, 2, 1 and leaves the bookshelf
// unspent. Any other plain pick gets Efficiency as usual.

fn gate(item: String): Bool {
    if (item != "iron_pickaxe") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.enchant("keep-book", gate);
    return 0;
}
