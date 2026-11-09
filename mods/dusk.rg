// The opening afternoon plants dirt at 1, 2, 1. That sunset is day 0.
// Later sunsets leave the cell alone.

fn evening(day: Int) {
    if (day == 0) {
        world.set_block(1, 2, 1, "dirt");
    }
}

fn main(): Int {
    events.dusk("last-light", evening);
    return 0;
}
