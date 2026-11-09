import GUIAPI;

fn picked(row: String) {
    GUIAPI.say(row);
}

fn main(): Int {
    var screen = GUIAPI.createScreen("field", GUIAPI.vec(240.0, 120.0), GUIAPI.vec(24.0, 24.0));
    GUIAPI.addElement(screen, GUIAPI.text("pad", "field notes"));
    GUIAPI.on_press(screen, "f", picked);
    return 0;
}
