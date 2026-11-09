// The first morning after the opening day plants dirt at 1, 2, 1.
// Day 0 is that opening spring day. Later mornings leave the cell alone.

fn morning(day: Int) {
    if (day == 1) {
        world.set_block(1, 2, 1, "dirt");
    }
}

fn main(): Int {
    events.dawn("first-light", morning);
    return 0;
}
