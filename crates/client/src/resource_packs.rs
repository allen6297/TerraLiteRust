//! Resource pack picker state for the egui screen.
//!
//! Discovers folders/zips under `resourcepacks/`, toggles overlays, and applies
//! them so the atlas reloads.

use rebuild_world::{PackEntry, PackKind, PackSelection};

/// One row on the resource pack picker.
#[derive(Clone, Debug)]
pub struct PackRow {
    pub id: String,
    pub description: String,
    pub kind: &'static str,
    pub base: bool,
    pub enabled: bool,
}

/// Draft selection while the picker is open (Apply commits).
#[derive(Clone, Debug)]
pub struct PackPicker {
    draft: PackSelection,
    note: String,
    dirty: bool,
}

impl PackPicker {
    pub fn load() -> Self {
        Self {
            draft: rebuild_world::resource_pack_selection(),
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

    pub fn rows(&self) -> Vec<PackRow> {
        rebuild_world::discover_resource_packs()
            .into_iter()
            .map(|entry| PackRow {
                id: entry.id.clone(),
                description: if entry.description.is_empty() {
                    entry.id.clone()
                } else {
                    entry.description.clone()
                },
                kind: match entry.kind {
                    PackKind::Folder => "folder",
                    PackKind::Zip => "zip",
                },
                base: entry.base,
                enabled: entry.base || self.draft.is_enabled(&entry.id),
            })
            .collect()
    }

    pub fn toggle(&mut self, id: &str) {
        if id == rebuild_world::BASE_PACK_ID {
            self.note = "base pack stays on".into();
            return;
        }
        self.draft.toggle(id);
        self.dirty = true;
        self.note = if self.draft.is_enabled(id) {
            format!("enabled {id} — Apply to reload")
        } else {
            format!("disabled {id} — Apply to reload")
        };
    }

    /// Persist, extract zips, install live selection. Caller reloads textures.
    pub fn apply(&mut self) -> Result<(), String> {
        let _roots = rebuild_world::prepare_overlays(&self.draft)?;
        rebuild_world::install_pack_selection(self.draft.clone())?;
        self.dirty = false;
        let n = self.draft.enabled().len();
        self.note = if n == 0 {
            "applied base pack".into()
        } else {
            format!("applied {n} overlay{}", if n == 1 { "" } else { "s" })
        };
        Ok(())
    }

    pub fn refresh_from_disk(&mut self) {
        self.draft = rebuild_world::resource_pack_selection();
        self.dirty = false;
        self.note.clear();
    }
}

/// Helper for tests / shots: enable a named overlay in the draft.
pub fn entry_by_id(id: &str) -> Option<PackEntry> {
    rebuild_world::discover_resource_packs()
        .into_iter()
        .find(|pack| pack.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_include_base_and_demos() {
        let picker = PackPicker::load();
        let rows = picker.rows();
        assert!(rows.iter().any(|row| row.id == "terralite" && row.base && row.enabled));
        assert!(rows.iter().any(|row| row.id == "demo-magenta" && row.kind == "folder"));
        assert!(rows.iter().any(|row| row.id == "demo-cyan-dirt" && row.kind == "zip"));
        assert!(rows.iter().any(|row| row.id == "whimscape" && row.kind == "folder"));
    }
}
