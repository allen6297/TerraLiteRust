// First approach of a named landmark tips chat and may cancel the built-in tip.
// A village discovery says a line and grants a dirt token at 1, 2, 1.
// Returning false for "camp" skips the built-in tip.

fn on_found(structure: String): Bool {
    chat.say("discovered " + structure);
    if (structure == "village") {
        chat.say("token village seal");
        world.set_block(1, 2, 1, "dirt");
        return true;
    }
    if (structure == "camp") {
        return false;
    }
    return true;
}

fn main(): Int {
    events.structure_discovered("story-find", on_found);
    return 0;
}
