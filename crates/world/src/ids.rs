//! String block ids, and the 1.8 numbers used only inside an Anvil file.
//!
//! The world never uses those numbers as ids. They exist so a region file
//! can still be read and written.

use std::sync::Mutex;

const ANVIL: &[(&str, u16)] = &[
    ("stone", 1),
    ("deepslate", 121),
    ("grass", 2),
    ("dirt", 3),
    ("cobblestone", 4),
    ("planks", 5),
    ("flowing_water", 8),
    ("water", 9),
    ("iron_ore", 15),
    ("coal_ore", 16),
    ("oak_log", 17),
    ("wool", 35),
    ("oak_stairs", 53),
    ("sign", 63),
    ("lantern", 90),
    ("plate", 201),
    ("marker", 300),
];

static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static NUMBERS: Mutex<Vec<(u16, &'static str)>> = Mutex::new(Vec::new());

pub fn intern(name: &str) -> &'static str {
    if name == "air" {
        return "air";
    }
    if let Some((known, _)) = ANVIL.iter().find(|(known, _)| *known == name) {
        return known;
    }
    let mut names = NAMES.lock().expect("block names");
    if let Some(found) = names.iter().find(|existing| **existing == name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

pub fn anvil_name(id: u16) -> &'static str {
    if id == 0 {
        return "air";
    }
    if let Some((name, _)) = ANVIL.iter().find(|(_, number)| *number == id) {
        return name;
    }
    {
        let numbers = NUMBERS.lock().expect("anvil ids");
        if let Some((_, name)) = numbers.iter().find(|(number, _)| *number == id) {
            return name;
        }
    }
    let name = intern(&format!("block_{id}"));
    let mut numbers = NUMBERS.lock().expect("anvil ids");
    if let Some((_, existing)) = numbers.iter().find(|(number, _)| *number == id) {
        return existing;
    }
    numbers.push((id, name));
    name
}

pub fn anvil_id(name: &str) -> u16 {
    if name == "air" {
        return 0;
    }
    let name = intern(name);
    if let Some((_, id)) = ANVIL.iter().find(|(known, _)| *known == name) {
        return *id;
    }
    let mut numbers = NUMBERS.lock().expect("anvil ids");
    if let Some((id, _)) = numbers.iter().find(|(_, known)| *known == name) {
        return *id;
    }
    let mut id = 1u16;
    while id <= 0x0FFF
        && (ANVIL.iter().any(|(_, number)| *number == id)
            || numbers.iter().any(|(number, _)| *number == id))
    {
        id += 1;
    }
    assert!(id <= 0x0FFF, "ran out of anvil ids");
    numbers.push((id, name));
    id
}
