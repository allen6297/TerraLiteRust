// Brewing with nether wart writes dirt at 1, 2, 1 and leaves the water and
// the wart in the bar. A flower still becomes a healing potion.

fn gate(ingredient: String): Bool {
    if (ingredient != "nether_wart_block") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.brew("keep-wart", gate);
    return 0;
}
