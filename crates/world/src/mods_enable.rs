//! Discover and enable/disable RoseGold mods under `mods/`.
//!
//! Disabled ids are skipped by [`crate::rosegold::load_dir`]. New mods default
//! to enabled so shipped ember / rose-kit / k's-guide stay on unless toggled.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Folder that lists RoseGold mods.
pub const MODS_DIR: &str = r"C:\CODE\TerraLite\mods";

/// Where the client remembers which mods are disabled.
pub fn selection_file() -> PathBuf {
    PathBuf::from(r"C:\CODE\TerraLite\saves\mods.json")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModKind {
    /// Flat `mods/<id>.rg`.
    Script,
    /// Folder mod (`src/mod.rg` and/or nested `*.rg`).
    Folder,
}

/// One discoverable mod under `mods/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModEntry {
    pub id: String,
    pub kind: ModKind,
    pub path: PathBuf,
    pub description: String,
}

/// Disabled mod ids. Empty means every discovered mod loads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModSelection {
    disabled: Vec<String>,
}

impl ModSelection {
    pub fn disabled(&self) -> &[String] {
        &self.disabled
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.iter().any(|row| row == id)
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) {
        self.disabled.retain(|row| row != id);
        if !on {
            self.disabled.push(id.to_string());
        }
    }

    pub fn toggle(&mut self, id: &str) {
        let on = !self.is_enabled(id);
        self.set_enabled(id, on);
    }

    pub fn load() -> Self {
        match fs::read_to_string(selection_file()) {
            Ok(text) => parse_selection(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = selection_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        fs::write(&path, format_selection(self))
            .map_err(|err| format!("{}: {err}", path.display()))
    }
}

fn selection_state() -> &'static Mutex<ModSelection> {
    static STATE: OnceLock<Mutex<ModSelection>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ModSelection::load()))
}

pub fn mod_selection() -> ModSelection {
    selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

pub fn install_mod_selection(next: ModSelection) -> Result<(), String> {
    next.save()?;
    *selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = next;
    Ok(())
}

/// True when `id` is allowed to load (missing selection = enabled).
pub fn mod_is_enabled(id: &str) -> bool {
    mod_selection().is_enabled(id)
}

fn format_selection(sel: &ModSelection) -> String {
    let mut out = String::from("{\n  \"disabled\": [\n");
    for (index, id) in sel.disabled.iter().enumerate() {
        let comma = if index + 1 == sel.disabled.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!("    \"{}\"{comma}\n", escape_json(id)));
    }
    out.push_str("  ]\n}\n");
    out
}

fn escape_json(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_selection(text: &str) -> Result<ModSelection, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let arr = root
        .get("disabled")
        .and_then(Value::as_array)
        .ok_or_else(|| "disabled must be an array".to_string())?;
    let mut disabled = Vec::new();
    for entry in arr {
        let Some(id) = entry.as_str() else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        if !disabled.iter().any(|row| row == id) {
            disabled.push(id.to_string());
        }
    }
    Ok(ModSelection { disabled })
}

/// Unique mods under `mods/` (flat `.rg` and folder packs).
pub fn discover_mods() -> Vec<ModEntry> {
    let root = Path::new(MODS_DIR);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if !folder_has_script(&path) {
                continue;
            }
            // Prefer folder entry when both `id.rg` and `id/` exist.
            out.retain(|row: &ModEntry| row.id != name);
            let description = folder_description(&path, &name);
            out.push(ModEntry {
                id: name,
                kind: ModKind::Folder,
                path,
                description,
            });
        } else if name.to_ascii_lowercase().ends_with(".rg") && path.is_file() {
            let id = name
                .trim_end_matches(".rg")
                .trim_end_matches(".RG")
                .to_string();
            if id.is_empty() {
                continue;
            }
            if out.iter().any(|row| row.id == id) {
                continue;
            }
            let description = script_description(&path).unwrap_or_else(|| "RoseGold script".into());
            out.push(ModEntry {
                id,
                kind: ModKind::Script,
                path,
                description,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn folder_has_script(path: &Path) -> bool {
    if path.join("src").join("mod.rg").is_file() {
        return true;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let file = entry.path();
        file.is_file()
            && file
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rg"))
    })
}

fn folder_description(path: &Path, id: &str) -> String {
    let candidates = [
        path.join("src").join("mod.rg"),
        path.join(format!("{id}.rg")),
    ];
    for candidate in candidates {
        if let Some(text) = script_description(&candidate) {
            return text;
        }
    }
    "RoseGold folder mod".into()
}

fn script_description(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("//") {
            let note = rest.trim();
            if !note.is_empty() {
                return Some(note.to_string());
            }
        }
        break;
    }
    None
}

/// Mod id for a script path under `mods/` (`ember.rg` → `ember`, `k's-guide/src/mod.rg` → `k's-guide`).
pub fn mod_id_for_path(mods_root: &Path, script: &Path) -> Option<String> {
    let rel = script.strip_prefix(mods_root).ok()?;
    let mut components = rel.components();
    let first = components.next()?;
    let name = first.as_os_str().to_str()?;
    if name.eq_ignore_ascii_case("src") {
        return None;
    }
    if name.to_ascii_lowercase().ends_with(".rg") {
        Some(
            name.trim_end_matches(".rg")
                .trim_end_matches(".RG")
                .to_string(),
        )
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_shipped_mods() {
        let mods = discover_mods();
        let ids: Vec<_> = mods.iter().map(|row| row.id.as_str()).collect();
        assert!(ids.contains(&"ember"));
        assert!(ids.contains(&"rose-kit"));
        assert!(ids.contains(&"k's-guide"));
        assert!(ids.contains(&"craft-story"));
    }

    #[test]
    fn default_selection_enables_all() {
        let sel = ModSelection::default();
        assert!(sel.is_enabled("ember"));
        assert!(sel.is_enabled("rose-kit"));
        assert!(sel.is_enabled("k's-guide"));
    }

    #[test]
    fn disable_persists_id() {
        let mut sel = ModSelection::default();
        sel.set_enabled("craft-story", false);
        assert!(!sel.is_enabled("craft-story"));
        assert!(sel.is_enabled("ember"));
        sel.toggle("craft-story");
        assert!(sel.is_enabled("craft-story"));
    }

    #[test]
    fn mod_id_from_paths() {
        let root = Path::new(MODS_DIR);
        assert_eq!(
            mod_id_for_path(root, &root.join("ember.rg")).as_deref(),
            Some("ember")
        );
        assert_eq!(
            mod_id_for_path(root, &root.join("k's-guide").join("src").join("mod.rg")).as_deref(),
            Some("k's-guide")
        );
    }
}
