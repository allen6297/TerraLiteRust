// Saying secret in chat swallows the line and writes dirt at 1, 2, 1.
// Any other ordinary line is shown. A slash line is a command, not chat.

fn hush(line: String): Bool {
    if (line != "secret") {
        return true;
    }
    world.set_block(1, 2, 1, "dirt");
    return false;
}

fn main(): Int {
    events.chat("hush-secret", hush);
    return 0;
}
