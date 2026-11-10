// Taking an iron ingot from a furnace, smoker, or blast furnace writes dirt
// at 1, 2, 1 and leaves the ingot in the output slot. Any other result is taken.

fn gate(result: String): Bool {
    if (result != "iron_ingot") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.smelt("keep-iron", gate);
    return 0;
}
