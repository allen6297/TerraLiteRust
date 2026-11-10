//! RoseGold mods enable list for the egui screen.
//!
//! Discovers `.rg` / folder mods under `mods/`. Apply persists a disabled list;
//! scripts reload on the next world load (no hot-reload).

use rebuild_world::{ModEntry, ModKind, ModSelection};

/// One row on the mods enable list.
#[derive(Clone, Debug)]
pub struct ModRow {
    pub id: String,
    pub description: String,
    pub kind: &'static str,
    pub enabled: bool,
}

/// Draft selection while the picker is open (Apply commits).
#[derive(Clone, Debug)]
pub struct ModPicker {
    draft: ModSelection,
    note: String,
    dirty: bool,
}

impl ModPicker {
    pub fn load() -> Self {
        Self {
            draft: rebuild_world::mod_selection(),
            note: String::new(),
            dirty: false,
        }
    }

    pub fn note(&self) -> &str {
        &self.note
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn rows(&self) -> Vec<ModRow> {
        rebuild_world::discover_mods()
            .into_iter()
            .map(|entry| ModRow {
                id: entry.id.clone(),
                description: if entry.description.is_empty() {
                    entry.id.clone()
                } else {
                    entry.description.clone()
                },
                kind: match entry.kind {
                    ModKind::Script => "script",
                    ModKind::Folder => "folder",
                },
                enabled: self.draft.is_enabled(&entry.id),
            })
            .collect()
    }

    pub fn toggle(&mut self, id: &str) {
        self.draft.toggle(id);
        self.dirty = true;
        self.note = if self.draft.is_enabled(id) {
            format!("enabled {id} — Apply, then reload world")
        } else {
            format!("disabled {id} — Apply, then reload world")
        };
    }

    /// Persist disabled list. Caller should note next-world-load.
    pub fn apply(&mut self) -> Result<(), String> {
        rebuild_world::install_mod_selection(self.draft.clone())?;
        self.dirty = false;
        let disabled = self.draft.disabled().len();
        self.note = if disabled == 0 {
            "saved — all mods on; reloads next world load".into()
        } else {
            format!("saved — {disabled} off; reloads next world load")
        };
        Ok(())
    }

    pub fn refresh_from_disk(&mut self) {
        self.draft = rebuild_world::mod_selection();
        self.dirty = false;
        self.note.clear();
    }
}

pub fn entry_by_id(id: &str) -> Option<ModEntry> {
    rebuild_world::discover_mods()
        .into_iter()
        .find(|row| row.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_include_shipped_core_mods() {
        let picker = ModPicker::load();
        let rows = picker.rows();
        for id in ["ember", "rose-kit", "k's-guide", "craft-story"] {
            assert!(
                rows.iter().any(|row| row.id == id && row.enabled),
                "missing or disabled by default: {id}"
            );
        }
    }
}
