import SettingsAPI;
import GUIAPI;

fn glow_flip(name: String) {
    GUIAPI.say("glow " + SettingsAPI.value(name));
}

fn pace_flip(name: String) {
    GUIAPI.say("pace " + SettingsAPI.value(name));
}

fn main(): Int {
    var game = SettingsAPI.createCategory("game", "how it feels");
    var glow = SettingsAPI.createOption(game, "glow", "soft glow", "on");
    SettingsAPI.on_change(glow, glow_flip);
    var pace = SettingsAPI.createOption(game, "pace", "walk speed", "mid");
    SettingsAPI.choices(pace, "low mid high");
    SettingsAPI.on_change(pace, pace_flip);
    return 0;
}
