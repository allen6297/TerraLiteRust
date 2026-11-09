// The first second of play plants dirt at 1, 2, 1. Later seconds leave it.

fn pulse(n: Int) {
    if (n == 1) {
        world.set_block(1, 2, 1, "dirt");
    }
}

fn main(): Int {
    events.tick("one-mark", pulse);
    return 0;
}
