// /spark in chat, or spark on the C command list. The return value is the
// chat line. /spark dirt writes dirt at 1, 2, 1.

fn on_spark(args: String): String {
    if (args == "dirt") {
        world.set_block(1, 2, 1, "dirt");
    }
    if (args == "") {
        return "spark";
    }
    return "spark " + args;
}

fn main(): Int {
    events.command("spark", on_spark);
    return 0;
}
