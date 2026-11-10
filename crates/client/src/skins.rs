//! Player skin picker for settings.
//!
//! Lists PNGs under `skins/`, supports a file-dialog path, applies the sheet to
//! the local player mesh immediately, and persists the choice in `saves/skin.json`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::blockbench::SkinModel;
use crate::player;

/// Folder of drop-in classic/slim PNGs.
pub const SKINS_DIR: &str = r"C:\CODE\TerraLite\skins";

fn selection_file() -> PathBuf {
    PathBuf::from(r"C:\CODE\TerraLite\saves\skin.json")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinEntry {
    pub id: String,
    pub path: PathBuf,
    pub model: SkinModel,
    pub custom: bool,
}

#[derive(Clone, Debug)]
pub struct SkinRow {
    pub id: String,
    pub path: PathBuf,
    pub model: &'static str,
    pub selected: bool,
    pub custom: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkinSelection {
    /// Empty / missing = pack default.
    path: Option<PathBuf>,
}

impl SkinSelection {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
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

fn format_selection(sel: &SkinSelection) -> String {
    match &sel.path {
        Some(path) => format!(
            "{{\n  \"path\": \"{}\"\n}}\n",
            escape_json(&path.display().to_string())
        ),
        None => "{\n  \"path\": null\n}\n".into(),
    }
}

fn escape_json(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_selection(text: &str) -> Result<SkinSelection, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|err| err.to_string())?;
    let path = match root.get("path") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => {
            let Some(text) = value.as_str() else {
                return Err("path must be a string or null".into());
            };
            if text.is_empty() {
                None
            } else {
                Some(PathBuf::from(text))
            }
        }
    };
    Ok(SkinSelection { path })
}

/// Discover PNGs under `skins/` plus any custom path already selected.
pub fn discover_skins() -> Vec<SkinEntry> {
    let mut out = Vec::new();
    out.push(SkinEntry {
        id: "default".into(),
        path: PathBuf::new(),
        model: SkinModel::Classic,
        custom: false,
    });
    let root = Path::new(SKINS_DIR);
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".png") || !path.is_file() {
                continue;
            }
            let id = name
                .trim_end_matches(".png")
                .trim_end_matches(".PNG")
                .to_string();
            if id.eq_ignore_ascii_case("default") {
                // Prefer the real file for "default" when present.
                if let Some(row) = out.iter_mut().find(|row| row.id == "default") {
                    row.path = path;
                    if let Ok(bytes) = fs::read(&row.path) {
                        if let Ok(skin) = crate::blockbench::load_skin_png(&bytes) {
                            row.model = skin.detect_model();
                        }
                    }
                }
                continue;
            }
            let model = fs::read(&path)
                .ok()
                .and_then(|bytes| crate::blockbench::load_skin_png(&bytes).ok())
                .map(|skin| skin.detect_model())
                .unwrap_or(SkinModel::Classic);
            out.push(SkinEntry {
                id,
                path,
                model,
                custom: false,
            });
        }
    }
    out.sort_by(|a, b| {
        if a.id == "default" {
            std::cmp::Ordering::Less
        } else if b.id == "default" {
            std::cmp::Ordering::Greater
        } else {
            a.id.cmp(&b.id)
        }
    });
    out
}

#[derive(Clone, Debug)]
pub struct SkinPicker {
    draft: Option<PathBuf>,
    note: String,
    dirty: bool,
    /// Extra path from Browse… not necessarily under skins/.
    browsed: Option<PathBuf>,
}

impl SkinPicker {
    pub fn load() -> Self {
        let saved = SkinSelection::load();
        if let Some(path) = saved.path() {
            let _ = player::install_skin_path(path);
        }
        Self {
            draft: saved.path().map(Path::to_path_buf),
            note: String::new(),
            dirty: false,
            browsed: None,
        }
    }

    pub fn note(&self) -> &str {
        &self.note
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn rows(&self) -> Vec<SkinRow> {
        let mut rows: Vec<SkinRow> = discover_skins()
            .into_iter()
            .map(|entry| {
                let selected = match (&self.draft, entry.path.as_os_str().is_empty()) {
                    (None, true) => true,
                    (Some(path), false) => paths_equal(path, &entry.path),
                    _ => false,
                };
                SkinRow {
                    id: entry.id,
                    path: entry.path,
                    model: model_label(entry.model),
                    selected,
                    custom: entry.custom,
                }
            })
            .collect();
        if let Some(path) = &self.browsed {
            if !rows.iter().any(|row| paths_equal(&row.path, path)) {
                let model = fs::read(path)
                    .ok()
                    .and_then(|bytes| crate::blockbench::load_skin_png(&bytes).ok())
                    .map(|skin| skin.detect_model())
                    .unwrap_or(SkinModel::Classic);
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("custom")
                    .to_string();
                rows.push(SkinRow {
                    id,
                    path: path.clone(),
                    model: model_label(model),
                    selected: self
                        .draft
                        .as_ref()
                        .is_some_and(|draft| paths_equal(draft, path)),
                    custom: true,
                });
            }
        }
        rows
    }

    pub fn select_default(&mut self) {
        self.draft = None;
        self.dirty = true;
        self.note = "default pack skin — Apply to use".into();
    }

    pub fn select_path(&mut self, path: PathBuf) {
        if path.as_os_str().is_empty() {
            self.select_default();
            return;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("skin")
            .to_string();
        self.draft = Some(path);
        self.dirty = true;
        self.note = format!("selected {name} — Apply to paint mesh");
    }

    pub fn browse(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Choose player skin PNG")
            .add_filter("PNG skin", &["png"])
            .set_directory(SKINS_DIR)
            .pick_file();
        let Some(path) = picked else {
            self.note = "browse cancelled".into();
            return;
        };
        match crate::blockbench::load_skin_png(
            &fs::read(&path).unwrap_or_default(),
        ) {
            Ok(_) => {
                self.browsed = Some(path.clone());
                self.select_path(path);
            }
            Err(err) => {
                self.note = format!("browse failed: {err}");
            }
        }
    }

    /// Persist and install onto the local player mesh right away.
    pub fn apply(&mut self) -> Result<(), String> {
        let next = SkinSelection {
            path: self.draft.clone(),
        };
        next.save()?;
        match &next.path {
            Some(path) => {
                let model = player::install_skin_path(path)?;
                self.note = format!(
                    "applied {} ({})",
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("skin"),
                    model_label(model)
                );
            }
            None => {
                player::clear_skin();
                self.note = "applied default pack skin".into();
            }
        }
        self.dirty = false;
        Ok(())
    }

    pub fn refresh_from_disk(&mut self) {
        let saved = SkinSelection::load();
        self.draft = saved.path().map(Path::to_path_buf);
        self.dirty = false;
        self.note.clear();
        self.browsed = None;
    }
}

fn model_label(model: SkinModel) -> &'static str {
    match model {
        SkinModel::Classic => "classic",
        SkinModel::Slim => "slim",
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let a = a.to_string_lossy().to_ascii_lowercase();
    let b = b.to_string_lossy().to_ascii_lowercase();
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_lists_demo_skins() {
        let skins = discover_skins();
        assert!(skins.iter().any(|row| row.id == "default"));
        assert!(skins.iter().any(|row| row.id == "demo-magenta"));
        assert!(skins.iter().any(|row| row.id == "demo-slim-teal"));
    }
}
