fn picked(slot: Int, block: String) {
    print("rosegold: selected " + slot + " " + block);
}

fn screen(id: String, open: Bool) {
    if open {
        print("rosegold: opened " + id);
    } else {
        print("rosegold: closed " + id);
    }
}

fn main(): Int {
    hooks.on_select("log-select", picked);
    hooks.on_screen("log-screen", screen);
    return 0;
}
