fn allow(block: String): Bool {
    return block != "stone";
}

fn main(): Int {
    hooks.before_break("keep-stone", allow);
    return 0;
}
