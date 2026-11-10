//! Discover, enable, and resolve overlay datapacks under `datapacks/`.
//!
//! Base `terralite` always loads. Enabled overlays sit on top (last wins) for
//! loot / recipe / smelting JSON. Zip packs extract into
//! `datapacks/.extracted/<id>/` when applied.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Folder that lists datapacks (folders + zips).
pub const DATAPACKS_DIR: &str = r"C:\CODE\TerraLite\datapacks";

/// Always-on base datapack id (folder name).
pub const DATA_BASE_PACK_ID: &str = "terralite";

/// Where the client remembers which overlays are enabled.
pub fn selection_file() -> PathBuf {
    PathBuf::from(r"C:\CODE\TerraLite\saves\datapacks.json")
}

fn extracted_root() -> PathBuf {
    Path::new(DATAPACKS_DIR).join(".extracted")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataPackKind {
    Folder,
    Zip,
}

/// One folder or zip discovered under `datapacks/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPackEntry {
    pub id: String,
    pub kind: DataPackKind,
    pub path: PathBuf,
    pub description: String,
    pub base: bool,
}

/// Enabled overlay ids, bottom → top (last wins on conflicts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataPackSelection {
    enabled: Vec<String>,
}

impl DataPackSelection {
    pub fn enabled(&self) -> &[String] {
        &self.enabled
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.iter().any(|row| row == id)
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) {
        if id == DATA_BASE_PACK_ID {
            return;
        }
        self.enabled.retain(|row| row != id);
        if on {
            self.enabled.push(id.to_string());
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

fn selection_state() -> &'static Mutex<DataPackSelection> {
    static STATE: OnceLock<Mutex<DataPackSelection>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(DataPackSelection::load()))
}

pub fn datapack_selection() -> DataPackSelection {
    selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

pub fn install_datapack_selection(next: DataPackSelection) -> Result<(), String> {
    next.save()?;
    *selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = next;
    Ok(())
}

fn format_selection(sel: &DataPackSelection) -> String {
    let mut out = String::from("{\n  \"enabled\": [\n");
    for (index, id) in sel.enabled.iter().enumerate() {
        let comma = if index + 1 == sel.enabled.len() {
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

fn parse_selection(text: &str) -> Result<DataPackSelection, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let arr = root
        .get("enabled")
        .and_then(Value::as_array)
        .ok_or_else(|| "enabled must be an array".to_string())?;
    let mut enabled = Vec::new();
    for entry in arr {
        let Some(id) = entry.as_str() else {
            continue;
        };
        if id.is_empty() || id == DATA_BASE_PACK_ID {
            continue;
        }
        if !enabled.iter().any(|row| row == id) {
            enabled.push(id.to_string());
        }
    }
    Ok(DataPackSelection { enabled })
}

/// Folders and zips under `datapacks/` (base first).
pub fn discover_datapacks() -> Vec<DataPackEntry> {
    let root = Path::new(DATAPACKS_DIR);
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
            if !path.join("pack.meta").is_file() {
                continue;
            }
            let description = read_pack_description(&path.join("pack.meta"));
            out.push(DataPackEntry {
                id: name.clone(),
                kind: DataPackKind::Folder,
                path,
                description,
                base: name == DATA_BASE_PACK_ID,
            });
        } else if name.to_ascii_lowercase().ends_with(".zip") && path.is_file() {
            let id = name
                .trim_end_matches(".zip")
                .trim_end_matches(".ZIP")
                .to_string();
            if id.is_empty() || id == DATA_BASE_PACK_ID {
                continue;
            }
            let description =
                read_zip_description(&path).unwrap_or_else(|| "zip datapack".to_string());
            out.push(DataPackEntry {
                id,
                kind: DataPackKind::Zip,
                path,
                description,
                base: false,
            });
        }
    }
    out.sort_by(|a, b| match (a.base, b.base) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.id.cmp(&b.id),
    });
    out
}

fn read_pack_description(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|root| {
            root.get("pack")
                .and_then(|pack| pack.get("description"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn read_zip_description(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("pack.meta").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|root| {
            root.get("pack")
                .and_then(|pack| pack.get("description"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// Ensure enabled zip packs are extracted; returns overlay roots top-last.
pub fn prepare_datapack_overlays(sel: &DataPackSelection) -> Result<Vec<PathBuf>, String> {
    let packs = discover_datapacks();
    let mut roots = Vec::new();
    for id in &sel.enabled {
        let Some(entry) = packs.iter().find(|pack| &pack.id == id) else {
            continue;
        };
        let root = match entry.kind {
            DataPackKind::Folder => entry.path.clone(),
            DataPackKind::Zip => extract_zip(entry)?,
        };
        if root.join("pack.meta").is_file() {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn extract_zip(entry: &DataPackEntry) -> Result<PathBuf, String> {
    let dest = extracted_root().join(&entry.id);
    let stamp = dest.join(".source");
    let source = entry.path.to_string_lossy().to_string();
    if dest.join("pack.meta").is_file()
        && fs::read_to_string(&stamp).ok().as_deref() == Some(source.as_str())
    {
        return Ok(dest);
    }
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|err| format!("{}: {err}", dest.display()))?;
    }
    fs::create_dir_all(&dest).map_err(|err| format!("{}: {err}", dest.display()))?;
    let file = fs::File::open(&entry.path)
        .map_err(|err| format!("{}: {err}", entry.path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("{}: {err}", entry.path.display()))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| format!("{}: {err}", entry.path.display()))?;
        let Some(rel) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let out = dest.join(&rel);
        if file.is_dir() {
            fs::create_dir_all(&out).map_err(|err| format!("{}: {err}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        let mut target =
            fs::File::create(&out).map_err(|err| format!("{}: {err}", out.display()))?;
        std::io::copy(&mut file, &mut target)
            .map_err(|err| format!("{}: {err}", out.display()))?;
    }
    let mut stamp_file =
        fs::File::create(&stamp).map_err(|err| format!("{}: {err}", stamp.display()))?;
    stamp_file
        .write_all(source.as_bytes())
        .map_err(|err| format!("{}: {err}", stamp.display()))?;
    Ok(dest)
}

fn live_overlay_roots() -> Vec<PathBuf> {
    let sel = datapack_selection();
    prepare_datapack_overlays(&sel).unwrap_or_default()
}

/// Base + enabled overlays (base first).
pub fn datapack_roots() -> Vec<PathBuf> {
    let mut roots = vec![crate::pack::data_pack_root().to_path_buf()];
    roots.extend(live_overlay_roots());
    roots
}

/// Resolve a relative path (`data/terralite/...`) — top overlay wins.
pub fn resolve_datapack_file(relative: &str) -> Option<PathBuf> {
    for root in live_overlay_roots().into_iter().rev() {
        let path = join_inside(&root, relative)?;
        if path.is_file() {
            return Some(path);
        }
    }
    let path = join_inside(crate::pack::data_pack_root(), relative)?;
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// JSON files under `data/terralite/<folder>/`, overlays override by stem.
pub fn list_datapack_json(folder: &str) -> Vec<(String, PathBuf)> {
    let relative = format!("data/terralite/{folder}");
    let mut by_stem: BTreeMap<String, PathBuf> = BTreeMap::new();
    for root in datapack_roots() {
        let dir = root.join("data").join("terralite").join(folder);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_json = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            if !path.is_file() || !is_json {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            by_stem.insert(stem.to_string(), path);
        }
    }
    let _ = relative;
    by_stem.into_iter().collect()
}

fn join_inside(root: &Path, path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains('\\') {
        return None;
    }
    let mut out = root.to_path_buf();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_finds_base_and_demos() {
        let packs = discover_datapacks();
        assert!(
            packs
                .iter()
                .any(|pack| pack.id == DATA_BASE_PACK_ID && pack.base)
        );
        assert!(packs.iter().any(|pack| pack.id == "demo-extra"));
        assert!(
            packs
                .iter()
                .any(|pack| pack.id == "demo-clay-bake" && pack.kind == DataPackKind::Zip)
        );
    }

    #[test]
    fn selection_skips_base() {
        let mut sel = DataPackSelection::default();
        sel.set_enabled(DATA_BASE_PACK_ID, true);
        assert!(sel.enabled().is_empty());
    }
}
