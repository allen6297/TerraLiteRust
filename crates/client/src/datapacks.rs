//! Datapack picker state for the egui screen.
//!
//! Discovers folders/zips under `datapacks/`, toggles overlays, and applies
//! them so loot / recipes / smelting reload.

use rebuild_world::{DataPackEntry, DataPackKind, DataPackSelection};

/// One row on the datapack picker.
#[derive(Clone, Debug)]
pub struct DataPackRow {
    pub id: String,
    pub description: String,
    pub kind: &'static str,
    pub base: bool,
    pub enabled: bool,
}

/// Draft selection while the picker is open (Apply commits).
#[derive(Clone, Debug)]
pub struct DataPackPicker {
    draft: DataPackSelection,
    note: String,
    dirty: bool,
}

impl DataPackPicker {
    pub fn load() -> Self {
        Self {
            draft: rebuild_world::datapack_selection(),
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

    pub fn rows(&self) -> Vec<DataPackRow> {
        rebuild_world::discover_datapacks()
            .into_iter()
            .map(|entry| DataPackRow {
                id: entry.id.clone(),
                description: if entry.description.is_empty() {
                    entry.id.clone()
                } else {
                    entry.description.clone()
                },
                kind: match entry.kind {
                    DataPackKind::Folder => "folder",
                    DataPackKind::Zip => "zip",
                },
                base: entry.base,
                enabled: entry.base || self.draft.is_enabled(&entry.id),
            })
            .collect()
    }

    pub fn toggle(&mut self, id: &str) {
        if id == rebuild_world::DATA_BASE_PACK_ID {
            self.note = "base datapack stays on".into();
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

    /// Persist, extract zips, install live selection. Caller reloads book/loot.
    pub fn apply(&mut self) -> Result<(), String> {
        let _roots = rebuild_world::prepare_datapack_overlays(&self.draft)?;
        rebuild_world::install_datapack_selection(self.draft.clone())?;
        self.dirty = false;
        let n = self.draft.enabled().len();
        self.note = if n == 0 {
            "applied base datapack".into()
        } else {
            format!("applied {n} overlay{}", if n == 1 { "" } else { "s" })
        };
        Ok(())
    }

    pub fn refresh_from_disk(&mut self) {
        self.draft = rebuild_world::datapack_selection();
        self.dirty = false;
        self.note.clear();
    }
}

pub fn entry_by_id(id: &str) -> Option<DataPackEntry> {
    rebuild_world::discover_datapacks()
        .into_iter()
        .find(|pack| pack.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_include_base_and_demos() {
        let picker = DataPackPicker::load();
        let rows = picker.rows();
        assert!(
            rows.iter()
                .any(|row| row.id == "terralite" && row.base && row.enabled)
        );
        assert!(
            rows.iter()
                .any(|row| row.id == "demo-extra" && row.kind == "folder")
        );
        assert!(
            rows.iter()
                .any(|row| row.id == "demo-clay-bake" && row.kind == "zip")
        );
    }
}
