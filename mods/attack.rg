// Creative lists hit:glove because it can place hit:post. Left-click stone
// or a wolf while holding it. The click is consumed, so the stone is not
// mined, the wolf is not the start of a break, and an egg or crystal on the
// ray is not punched. Dirt is written at 1, 2, 1. Other targets fall through.

fn guard(item: String, target: String): Bool {
    if (item != "hit:glove" || (target != "stone" && target != "wolf")) {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    block.register("hit", "post");
    item.register("hit", "glove");
    item.places("hit:glove", "hit:post");
    events.attack("glove-guard", guard);
    return 0;
}
