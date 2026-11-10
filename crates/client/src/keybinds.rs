//! Remappable keyboard (and mouse) binds.
//!
//! Defaults match the shipped WASD / Space / Shift / Ctrl layout. The keybinds
//! screen stores overrides in `saves/keybinds.json`. Escape stays reserved for
//! pause / back and is never rebindable.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// Where the client remembers remaps.
pub fn keybinds_file() -> PathBuf {
    PathBuf::from(r"C:\CODE\TerraLite\saves\keybinds.json")
}

/// One named action the player can rebind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sneak,
    Sprint,
    Inventory,
    Items,
    Chat,
    Commands,
    Use,
    ThirdPerson,
    Attack,
    Place,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Sneak,
        Action::Sprint,
        Action::Inventory,
        Action::Items,
        Action::Chat,
        Action::Commands,
        Action::Use,
        Action::ThirdPerson,
        Action::Attack,
        Action::Place,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Jump => "jump",
            Action::Sneak => "sneak",
            Action::Sprint => "sprint",
            Action::Inventory => "inventory",
            Action::Items => "items",
            Action::Chat => "chat",
            Action::Commands => "commands",
            Action::Use => "use",
            Action::ThirdPerson => "third_person",
            Action::Attack => "attack",
            Action::Place => "place",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Action::Forward => "Move forward",
            Action::Back => "Move back",
            Action::Left => "Move left",
            Action::Right => "Move right",
            Action::Jump => "Jump / swim up",
            Action::Sneak => "Sneak / crouch",
            Action::Sprint => "Sprint",
            Action::Inventory => "Inventory",
            Action::Items => "Items / recipes",
            Action::Chat => "Chat",
            Action::Commands => "Commands",
            Action::Use => "Use / interact",
            Action::ThirdPerson => "Third person",
            Action::Attack => "Attack / dig",
            Action::Place => "Place / use item",
        }
    }

    pub fn is_mouse(self) -> bool {
        matches!(self, Action::Attack | Action::Place)
    }

    fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|action| action.id() == id)
    }
}

/// A key or mouse button binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Bind {
    pub fn label(self) -> String {
        match self {
            Bind::Key(code) => key_label(code),
            Bind::Mouse(MouseButton::Left) => "Mouse Left".into(),
            Bind::Mouse(MouseButton::Right) => "Mouse Right".into(),
            Bind::Mouse(MouseButton::Middle) => "Mouse Middle".into(),
            Bind::Mouse(MouseButton::Back) => "Mouse Back".into(),
            Bind::Mouse(MouseButton::Forward) => "Mouse Forward".into(),
            Bind::Mouse(other) => format!("Mouse {other:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keybinds {
    forward: KeyCode,
    back: KeyCode,
    left: KeyCode,
    right: KeyCode,
    jump: KeyCode,
    sneak: KeyCode,
    sprint: KeyCode,
    inventory: KeyCode,
    items: KeyCode,
    chat: KeyCode,
    commands: KeyCode,
    use_key: KeyCode,
    third_person: KeyCode,
    attack: MouseButton,
    place: MouseButton,
    /// Which action is waiting for the next key/mouse press.
    pub listening: Option<Action>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            forward: KeyCode::KeyW,
            back: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
            jump: KeyCode::Space,
            sneak: KeyCode::ShiftLeft,
            sprint: KeyCode::ControlLeft,
            inventory: KeyCode::KeyI,
            items: KeyCode::KeyE,
            chat: KeyCode::KeyT,
            commands: KeyCode::KeyC,
            use_key: KeyCode::KeyF,
            third_person: KeyCode::F5,
            attack: MouseButton::Left,
            place: MouseButton::Right,
            listening: None,
        }
    }
}

impl Keybinds {
    pub fn load() -> Self {
        Self::load_from(&keybinds_file())
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        Self::from_json(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = self.save_to(&keybinds_file());
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::write(path, self.to_json()).map_err(|err| err.to_string())
    }

    pub fn reset_defaults(&mut self) {
        let listening = self.listening;
        *self = Self::default();
        self.listening = listening;
        self.save();
    }

    pub fn get(&self, action: Action) -> Bind {
        match action {
            Action::Forward => Bind::Key(self.forward),
            Action::Back => Bind::Key(self.back),
            Action::Left => Bind::Key(self.left),
            Action::Right => Bind::Key(self.right),
            Action::Jump => Bind::Key(self.jump),
            Action::Sneak => Bind::Key(self.sneak),
            Action::Sprint => Bind::Key(self.sprint),
            Action::Inventory => Bind::Key(self.inventory),
            Action::Items => Bind::Key(self.items),
            Action::Chat => Bind::Key(self.chat),
            Action::Commands => Bind::Key(self.commands),
            Action::Use => Bind::Key(self.use_key),
            Action::ThirdPerson => Bind::Key(self.third_person),
            Action::Attack => Bind::Mouse(self.attack),
            Action::Place => Bind::Mouse(self.place),
        }
    }

    pub fn is_key(&self, action: Action, code: KeyCode) -> bool {
        matches!(self.get(action), Bind::Key(bound) if bound == code)
            || secondary_mod(action, code, self)
    }

    pub fn is_mouse(&self, action: Action, button: MouseButton) -> bool {
        matches!(self.get(action), Bind::Mouse(bound) if bound == button)
    }

    /// Map a physical key to the shipped default the rest of the client expects.
    pub fn canonicalize_key(&self, code: KeyCode) -> KeyCode {
        for action in Action::ALL {
            if action.is_mouse() {
                continue;
            }
            if self.is_key(action, code) {
                return default_key(action);
            }
        }
        // Defaults that were moved away must not keep their old jobs.
        for action in Action::ALL {
            if action.is_mouse() {
                continue;
            }
            let default = default_key(action);
            if code == default && !self.is_key(action, code) {
                return KeyCode::F24;
            }
        }
        code
    }

    pub fn begin_listen(&mut self, action: Action) {
        self.listening = Some(action);
    }

    pub fn cancel_listen(&mut self) {
        self.listening = None;
    }

    /// Apply a key while listening. Escape cancels. Returns true when handled.
    pub fn hear_key(&mut self, code: KeyCode) -> bool {
        let Some(action) = self.listening else {
            return false;
        };
        if code == KeyCode::Escape {
            self.listening = None;
            return true;
        }
        if action.is_mouse() {
            return true;
        }
        if matches!(
            code,
            KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight
        ) {
            // Allow binding modifiers themselves.
        }
        self.set_key(action, code);
        self.listening = None;
        self.save();
        true
    }

    /// Apply a mouse button while listening. Returns true when handled.
    pub fn hear_mouse(&mut self, button: MouseButton) -> bool {
        let Some(action) = self.listening else {
            return false;
        };
        if !action.is_mouse() {
            return true;
        }
        self.set_mouse(action, button);
        self.listening = None;
        self.save();
        true
    }

    fn set_key(&mut self, action: Action, code: KeyCode) {
        // Swap if another action already owns this key.
        for other in Action::ALL {
            if other == action || other.is_mouse() {
                continue;
            }
            if self.is_key(other, code) {
                let previous = match action {
                    Action::Forward => self.forward,
                    Action::Back => self.back,
                    Action::Left => self.left,
                    Action::Right => self.right,
                    Action::Jump => self.jump,
                    Action::Sneak => self.sneak,
                    Action::Sprint => self.sprint,
                    Action::Inventory => self.inventory,
                    Action::Items => self.items,
                    Action::Chat => self.chat,
                    Action::Commands => self.commands,
                    Action::Use => self.use_key,
                    Action::ThirdPerson => self.third_person,
                    Action::Attack | Action::Place => return,
                };
                self.write_key(other, previous);
            }
        }
        self.write_key(action, code);
    }

    fn set_mouse(&mut self, action: Action, button: MouseButton) {
        match action {
            Action::Attack => {
                if self.place == button {
                    self.place = self.attack;
                }
                self.attack = button;
            }
            Action::Place => {
                if self.attack == button {
                    self.attack = self.place;
                }
                self.place = button;
            }
            _ => {}
        }
    }

    fn write_key(&mut self, action: Action, code: KeyCode) {
        match action {
            Action::Forward => self.forward = code,
            Action::Back => self.back = code,
            Action::Left => self.left = code,
            Action::Right => self.right = code,
            Action::Jump => self.jump = code,
            Action::Sneak => self.sneak = code,
            Action::Sprint => self.sprint = code,
            Action::Inventory => self.inventory = code,
            Action::Items => self.items = code,
            Action::Chat => self.chat = code,
            Action::Commands => self.commands = code,
            Action::Use => self.use_key = code,
            Action::ThirdPerson => self.third_person = code,
            Action::Attack | Action::Place => {}
        }
    }

    fn to_json(&self) -> String {
        let binds: Vec<Value> = Action::ALL
            .iter()
            .map(|action| {
                let bind = match self.get(*action) {
                    Bind::Key(code) => format!("key:{}", key_id(code)),
                    Bind::Mouse(button) => format!("mouse:{}", mouse_id(button)),
                };
                json!({ "action": action.id(), "bind": bind })
            })
            .collect();
        serde_json::to_string_pretty(&json!({ "binds": binds })).unwrap_or_else(|_| "{}".into())
    }

    fn from_json(text: &str) -> Option<Self> {
        let root: Value = serde_json::from_str(text).ok()?;
        let list = root.get("binds")?.as_array()?;
        let mut binds = Self::default();
        for entry in list {
            let action = entry.get("action")?.as_str().and_then(Action::from_id);
            let bind = entry.get("bind")?.as_str();
            let (Some(action), Some(bind)) = (action, bind) else {
                continue;
            };
            if let Some(code) = bind.strip_prefix("key:") {
                if let Some(key) = key_from_id(code) {
                    if !action.is_mouse() {
                        binds.write_key(action, key);
                    }
                }
            } else if let Some(name) = bind.strip_prefix("mouse:") {
                if let Some(button) = mouse_from_id(name) {
                    binds.set_mouse(action, button);
                }
            }
        }
        binds.listening = None;
        Some(binds)
    }
}

fn secondary_mod(action: Action, code: KeyCode, binds: &Keybinds) -> bool {
    // Left/right modifier pairs share a job when the primary is that side.
    match action {
        Action::Sneak
            if binds.sneak == KeyCode::ShiftLeft && code == KeyCode::ShiftRight
                || binds.sneak == KeyCode::ShiftRight && code == KeyCode::ShiftLeft =>
        {
            true
        }
        Action::Sprint
            if binds.sprint == KeyCode::ControlLeft && code == KeyCode::ControlRight
                || binds.sprint == KeyCode::ControlRight && code == KeyCode::ControlLeft =>
        {
            true
        }
        _ => false,
    }
}

fn default_key(action: Action) -> KeyCode {
    match action {
        Action::Forward => KeyCode::KeyW,
        Action::Back => KeyCode::KeyS,
        Action::Left => KeyCode::KeyA,
        Action::Right => KeyCode::KeyD,
        Action::Jump => KeyCode::Space,
        Action::Sneak => KeyCode::ShiftLeft,
        Action::Sprint => KeyCode::ControlLeft,
        Action::Inventory => KeyCode::KeyI,
        Action::Items => KeyCode::KeyE,
        Action::Chat => KeyCode::KeyT,
        Action::Commands => KeyCode::KeyC,
        Action::Use => KeyCode::KeyF,
        Action::ThirdPerson => KeyCode::F5,
        Action::Attack | Action::Place => KeyCode::F24,
    }
}

fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::KeyA => "A".into(),
        KeyCode::KeyB => "B".into(),
        KeyCode::KeyC => "C".into(),
        KeyCode::KeyD => "D".into(),
        KeyCode::KeyE => "E".into(),
        KeyCode::KeyF => "F".into(),
        KeyCode::KeyG => "G".into(),
        KeyCode::KeyH => "H".into(),
        KeyCode::KeyI => "I".into(),
        KeyCode::KeyJ => "J".into(),
        KeyCode::KeyK => "K".into(),
        KeyCode::KeyL => "L".into(),
        KeyCode::KeyM => "M".into(),
        KeyCode::KeyN => "N".into(),
        KeyCode::KeyO => "O".into(),
        KeyCode::KeyP => "P".into(),
        KeyCode::KeyQ => "Q".into(),
        KeyCode::KeyR => "R".into(),
        KeyCode::KeyS => "S".into(),
        KeyCode::KeyT => "T".into(),
        KeyCode::KeyU => "U".into(),
        KeyCode::KeyV => "V".into(),
        KeyCode::KeyW => "W".into(),
        KeyCode::KeyX => "X".into(),
        KeyCode::KeyY => "Y".into(),
        KeyCode::KeyZ => "Z".into(),
        KeyCode::Digit0 => "0".into(),
        KeyCode::Digit1 => "1".into(),
        KeyCode::Digit2 => "2".into(),
        KeyCode::Digit3 => "3".into(),
        KeyCode::Digit4 => "4".into(),
        KeyCode::Digit5 => "5".into(),
        KeyCode::Digit6 => "6".into(),
        KeyCode::Digit7 => "7".into(),
        KeyCode::Digit8 => "8".into(),
        KeyCode::Digit9 => "9".into(),
        KeyCode::Space => "Space".into(),
        KeyCode::ShiftLeft => "Left Shift".into(),
        KeyCode::ShiftRight => "Right Shift".into(),
        KeyCode::ControlLeft => "Left Ctrl".into(),
        KeyCode::ControlRight => "Right Ctrl".into(),
        KeyCode::AltLeft => "Left Alt".into(),
        KeyCode::AltRight => "Right Alt".into(),
        KeyCode::F1 => "F1".into(),
        KeyCode::F2 => "F2".into(),
        KeyCode::F3 => "F3".into(),
        KeyCode::F4 => "F4".into(),
        KeyCode::F5 => "F5".into(),
        KeyCode::F6 => "F6".into(),
        KeyCode::F7 => "F7".into(),
        KeyCode::F8 => "F8".into(),
        KeyCode::F9 => "F9".into(),
        KeyCode::F10 => "F10".into(),
        KeyCode::F11 => "F11".into(),
        KeyCode::F12 => "F12".into(),
        KeyCode::Escape => "Esc".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::ArrowUp => "Up".into(),
        KeyCode::ArrowDown => "Down".into(),
        KeyCode::ArrowLeft => "Left".into(),
        KeyCode::ArrowRight => "Right".into(),
        other => format!("{other:?}").replace("Key", ""),
    }
}

fn key_id(code: KeyCode) -> String {
    format!("{code:?}")
}

fn key_from_id(id: &str) -> Option<KeyCode> {
    // Round-trip through Debug names we wrote.
    Some(match id {
        "KeyA" => KeyCode::KeyA,
        "KeyB" => KeyCode::KeyB,
        "KeyC" => KeyCode::KeyC,
        "KeyD" => KeyCode::KeyD,
        "KeyE" => KeyCode::KeyE,
        "KeyF" => KeyCode::KeyF,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyI" => KeyCode::KeyI,
        "KeyJ" => KeyCode::KeyJ,
        "KeyK" => KeyCode::KeyK,
        "KeyL" => KeyCode::KeyL,
        "KeyM" => KeyCode::KeyM,
        "KeyN" => KeyCode::KeyN,
        "KeyO" => KeyCode::KeyO,
        "KeyP" => KeyCode::KeyP,
        "KeyQ" => KeyCode::KeyQ,
        "KeyR" => KeyCode::KeyR,
        "KeyS" => KeyCode::KeyS,
        "KeyT" => KeyCode::KeyT,
        "KeyU" => KeyCode::KeyU,
        "KeyV" => KeyCode::KeyV,
        "KeyW" => KeyCode::KeyW,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        "Digit0" => KeyCode::Digit0,
        "Digit1" => KeyCode::Digit1,
        "Digit2" => KeyCode::Digit2,
        "Digit3" => KeyCode::Digit3,
        "Digit4" => KeyCode::Digit4,
        "Digit5" => KeyCode::Digit5,
        "Digit6" => KeyCode::Digit6,
        "Digit7" => KeyCode::Digit7,
        "Digit8" => KeyCode::Digit8,
        "Digit9" => KeyCode::Digit9,
        "Space" => KeyCode::Space,
        "ShiftLeft" => KeyCode::ShiftLeft,
        "ShiftRight" => KeyCode::ShiftRight,
        "ControlLeft" => KeyCode::ControlLeft,
        "ControlRight" => KeyCode::ControlRight,
        "AltLeft" => KeyCode::AltLeft,
        "AltRight" => KeyCode::AltRight,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        "Tab" => KeyCode::Tab,
        "Enter" => KeyCode::Enter,
        "Backspace" => KeyCode::Backspace,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        _ => return None,
    })
}

fn mouse_id(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
        MouseButton::Back => "back",
        MouseButton::Forward => "forward",
        _ => "other",
    }
}

fn mouse_from_id(id: &str) -> Option<MouseButton> {
    Some(match id {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        "back" => MouseButton::Back,
        "forward" => MouseButton::Forward,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn defaults_match_the_shipped_layout() {
        let binds = Keybinds::default();
        assert!(binds.is_key(Action::Forward, KeyCode::KeyW));
        assert!(binds.is_key(Action::Jump, KeyCode::Space));
        assert!(binds.is_key(Action::Sneak, KeyCode::ShiftRight));
        assert!(binds.is_key(Action::Sprint, KeyCode::ControlRight));
        assert!(binds.is_mouse(Action::Attack, MouseButton::Left));
        assert!(binds.is_mouse(Action::Place, MouseButton::Right));
        assert_eq!(binds.canonicalize_key(KeyCode::KeyW), KeyCode::KeyW);
    }

    #[test]
    fn a_rebind_swaps_and_round_trips_the_file() {
        let mut binds = Keybinds::default();
        binds.begin_listen(Action::Forward);
        assert!(binds.hear_key(KeyCode::KeyI));
        assert!(binds.is_key(Action::Forward, KeyCode::KeyI));
        // Inventory took W after the swap.
        assert!(binds.is_key(Action::Inventory, KeyCode::KeyW));
        assert_eq!(binds.canonicalize_key(KeyCode::KeyI), KeyCode::KeyW);

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("terralite-keybinds-{stamp}.json"));
        binds.save_to(&path).unwrap();
        let loaded = Keybinds::load_from(&path);
        assert!(loaded.is_key(Action::Forward, KeyCode::KeyI));
        assert!(loaded.is_key(Action::Inventory, KeyCode::KeyW));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn escape_cancels_listen_without_saving_a_bind() {
        let mut binds = Keybinds::default();
        binds.begin_listen(Action::Jump);
        assert!(binds.hear_key(KeyCode::Escape));
        assert!(binds.listening.is_none());
        assert!(binds.is_key(Action::Jump, KeyCode::Space));
    }

    #[test]
    fn mouse_actions_accept_a_click_while_listening() {
        let mut binds = Keybinds::default();
        binds.begin_listen(Action::Attack);
        assert!(binds.hear_mouse(MouseButton::Middle));
        assert!(binds.is_mouse(Action::Attack, MouseButton::Middle));
        assert!(binds.is_mouse(Action::Place, MouseButton::Right));
        binds.begin_listen(Action::Place);
        assert!(binds.hear_mouse(MouseButton::Middle));
        assert!(binds.is_mouse(Action::Place, MouseButton::Middle));
        assert!(binds.is_mouse(Action::Attack, MouseButton::Right));
    }
}
