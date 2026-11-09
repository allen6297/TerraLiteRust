// Creative lists mob:calm because it can place mob:post. Right-click a wolf
// while holding it. The click is consumed, so the wolf is not fed and the
// post is not placed. A dirt block is written at 1, 2, 1. Other mobs and
// other items fall through to feed, saddle, and the mace swing.

fn calm(item: String, kind: String): Bool {
    if (item != "mob:calm" || kind != "wolf") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    block.register("mob", "post");
    item.register("mob", "calm");
    item.places("mob:calm", "mob:post");
    events.use_entity("calm-wolf", calm);
    return 0;
}
