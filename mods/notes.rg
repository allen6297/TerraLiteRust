import GUIAPI;
from ui import Color;

fn main(): Int {
    var screen = GUIAPI.createScreen("notes", GUIAPI.vec(320.0, 240.0), GUIAPI.vec(40.0, 48.0));

    var places = GUIAPI.heading("places", "places");
    GUIAPI.addElement(screen, GUIAPI.place(places, GUIAPI.vec(16.0, 40.0)));

    var face = GUIAPI.icon("oakface", "oak_log");
    GUIAPI.addElement(screen, GUIAPI.place(face, GUIAPI.vec(16.0, 68.0)));

    var oak = GUIAPI.text("oak", "oak");
    oak.color = Color.Rgb(36, 78, 40);
    GUIAPI.addElement(screen, GUIAPI.place(oak, GUIAPI.vec(56.0, 68.0)));

    var mark = GUIAPI.button("mark", "mark oak");
    GUIAPI.addElement(screen, GUIAPI.place(mark, GUIAPI.vec(204.0, 68.0)));

    var pool = GUIAPI.icon("pool", "water");
    GUIAPI.addElement(screen, GUIAPI.place(pool, GUIAPI.vec(16.0, 108.0)));

    var marsh = GUIAPI.text("marsh", "marsh");
    marsh.color = Color.Rgb(28, 78, 110);
    GUIAPI.addElement(screen, GUIAPI.place(marsh, GUIAPI.vec(56.0, 108.0)));

    var peck = GUIAPI.icon("peck", "raw");
    GUIAPI.addElement(screen, GUIAPI.place(peck, GUIAPI.vec(16.0, 148.0)));

    var hen = GUIAPI.text("hen", "hen");
    hen.color = Color.Rgb(220, 190, 120);
    hen.fontColor = Color.Black;
    GUIAPI.addElement(screen, GUIAPI.place(hen, GUIAPI.vec(56.0, 148.0)));

    var line = GUIAPI.field("line", "");
    GUIAPI.addElement(screen, GUIAPI.place(line, GUIAPI.vec(16.0, 196.0)));

    GUIAPI.on_press(screen, "n", fn(row: String) {
        if row == "oak" {
            GUIAPI.say("oak west of the stairs");
            return;
        }
        if row == "mark" {
            GUIAPI.say("oak marked");
            var kept = GUIAPI.text("kept", "oak noted");
            kept.position = GUIAPI.vec(16.0, 236.0);
            kept.size = GUIAPI.vec(288.0, 20.0);
            kept.font = "capital";
            GUIAPI.addElement(screen, kept);
            return;
        }
        if row == "kept" {
            GUIAPI.say("oak noted");
            var gone = GUIAPI.text("kept", "oak noted");
            GUIAPI.removeElement(screen, gone);
            return;
        }
        if row == "line" {
            GUIAPI.say(GUIAPI.read(screen, "line"));
            return;
        }
        GUIAPI.say(row);
    });
    return 0;
}
