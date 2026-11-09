import ModAPI;

// A file-level const can only be an Int, so the mod is built when on_init runs.
fn on_init() {
    var Mod: TLMod = ModAPI.createMod("k's guide", "1.0.0");
    var Register: TLRegister = ModAPI.createRegister(Mod);

    Mod.log("k's guide loaded");

    Register.ITEM.open();
    Register.BLOCK.open();
    Register.ENTITY.open();
    Register.BLOCK_ENTITY.open();
    Register.ITEM_GROUP.open();
    Register.SCREEN.open();

    Register.ITEM_GROUP.open();
    Register.EVENT.open();
    Register.COMMAND.open();
    Register.CONFIG.open();
    Register.RESOURCE.open();
    Register.TEXTURE.open();
    Register.PARTICLE.open();

    Register.BLOCK.register("guide", "marker");
    Register.model("guide:marker", "cube");
    Register.ITEM.register("guide", "page");
    Register.places("guide:page", "guide:marker");
    Register.SCREEN.register("guide", "book");
    Register.COMMAND.register("guide", "day");
    Register.COMMAND.register("guide", "night");
    Register.COMMAND.register("guide", "home");
    Register.PARTICLE.register("guide", "spark");

    Register.BLOCK.register("guide", "post");
    Register.model("guide:post", "cube");
    Register.ITEM.register("guide", "slip");
    Register.places("guide:slip", "guide:post");

    Register.BLOCK.register("guide", "chest");
    Register.model("guide:chest", "cube");
    Register.BLOCK_ENTITY.register("guide", "chest");
    Register.ITEM.register("guide", "key");
    Register.places("guide:key", "guide:chest");
}
