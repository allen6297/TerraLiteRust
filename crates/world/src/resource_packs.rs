//! Discover, enable, and resolve overlay resource packs under `resourcepacks/`.
//!
//! The base pack (`terralite`) always loads. Enabled overlays sit on top
//! (last enabled wins) for PNG / text asset lookups. Zip packs extract into
//! `resourcepacks/.extracted/<id>/` when applied.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Folder that lists packs (folders + zips).
pub const RESOURCE_PACKS_DIR: &str = r"C:\CODE\TerraLite\resourcepacks";

/// Always-on base pack id (folder name).
pub const BASE_PACK_ID: &str = "terralite";

/// Where the client remembers which overlays are enabled.
pub fn selection_file() -> PathBuf {
    PathBuf::from(r"C:\CODE\TerraLite\saves\resourcepacks.json")
}

fn extracted_root() -> PathBuf {
    Path::new(RESOURCE_PACKS_DIR).join(".extracted")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackKind {
    Folder,
    Zip,
}

/// One folder or zip discovered under `resourcepacks/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackEntry {
    pub id: String,
    pub kind: PackKind,
    pub path: PathBuf,
    pub description: String,
    /// True for the always-on base pack.
    pub base: bool,
}

/// Enabled overlay ids, bottom → top (last wins on conflicts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackSelection {
    enabled: Vec<String>,
}

impl PackSelection {
    pub fn enabled(&self) -> &[String] {
        &self.enabled
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.iter().any(|row| row == id)
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) {
        if id == BASE_PACK_ID {
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
            // Strip a UTF-8 BOM so Windows-edited saves still parse.
            Ok(text) => parse_selection(text.trim_start_matches('\u{feff}')).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = selection_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        fs::write(&path, format_selection(self)).map_err(|err| format!("{}: {err}", path.display()))
    }
}

fn selection_state() -> &'static Mutex<PackSelection> {
    static STATE: OnceLock<Mutex<PackSelection>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PackSelection::load()))
}

/// Live selection used by asset lookups.
pub fn resource_pack_selection() -> PackSelection {
    selection()
}

/// Live selection used by asset lookups.
pub fn selection() -> PackSelection {
    selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

/// Replace the live selection and persist it.
pub fn install_pack_selection(next: PackSelection) -> Result<(), String> {
    install_selection(next)
}

/// Replace the live selection and persist it.
pub fn install_selection(next: PackSelection) -> Result<(), String> {
    next.save()?;
    *selection_state()
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = next;
    Ok(())
}

fn format_selection(sel: &PackSelection) -> String {
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

fn parse_selection(text: &str) -> Result<PackSelection, String> {
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
        if id.is_empty() || id == BASE_PACK_ID {
            continue;
        }
        if !enabled.iter().any(|row| row == id) {
            enabled.push(id.to_string());
        }
    }
    Ok(PackSelection { enabled })
}

/// Folders and zips under `resourcepacks/` (base first, then overlays).
pub fn discover_resource_packs() -> Vec<PackEntry> {
    discover()
}

/// Folders and zips under `resourcepacks/` (base first, then overlays).
pub fn discover() -> Vec<PackEntry> {
    let root = Path::new(RESOURCE_PACKS_DIR);
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
            if !path.join("pack.json").is_file() {
                continue;
            }
            let description = read_pack_description(&path.join("pack.json"));
            out.push(PackEntry {
                id: name.clone(),
                kind: PackKind::Folder,
                path,
                description,
                base: name == BASE_PACK_ID,
            });
        } else if name.to_ascii_lowercase().ends_with(".zip") && path.is_file() {
            let id = name.trim_end_matches(".zip").trim_end_matches(".ZIP").to_string();
            if id.is_empty() || id == BASE_PACK_ID {
                continue;
            }
            let description = read_zip_description(&path)
                .unwrap_or_else(|| "zip pack".to_string());
            out.push(PackEntry {
                id,
                kind: PackKind::Zip,
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
    let mut entry = archive.by_name("pack.json").ok()?;
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
pub fn prepare_overlays(sel: &PackSelection) -> Result<Vec<PathBuf>, String> {
    let packs = discover();
    let mut roots = Vec::new();
    for id in &sel.enabled {
        let Some(entry) = packs.iter().find(|pack| &pack.id == id) else {
            continue;
        };
        let root = match entry.kind {
            PackKind::Folder => entry.path.clone(),
            PackKind::Zip => extract_zip(entry)?,
        };
        if root.join("pack.json").is_file() {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn extract_zip(entry: &PackEntry) -> Result<PathBuf, String> {
    let dest = extracted_root().join(&entry.id);
    let stamp = dest.join(".source");
    let source = entry.path.to_string_lossy().to_string();
    if dest.join("pack.json").is_file() {
        if fs::read_to_string(&stamp).ok().as_deref() == Some(source.as_str()) {
            return Ok(dest);
        }
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

/// Overlay roots for the live selection (empty when none enabled).
pub fn live_overlay_roots() -> Vec<PathBuf> {
    let sel = selection();
    prepare_overlays(&sel).unwrap_or_default()
}

/// Bytes from the topmost overlay that has `path`, else `None`.
pub fn overlay_bytes(path: &str) -> Option<Vec<u8>> {
    for root in live_overlay_roots().into_iter().rev() {
        if let Some(bytes) = read_under(&root, path) {
            return Some(bytes);
        }
    }
    None
}

fn read_under(root: &Path, path: &str) -> Option<Vec<u8>> {
    if !root.join("pack.json").is_file() {
        return None;
    }
    let file = join_inside(root, path)?;
    let bytes = fs::read(file).ok()?;
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
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
    fn discover_finds_base_and_demo() {
        let packs = discover();
        assert!(packs.iter().any(|pack| pack.id == BASE_PACK_ID && pack.base));
        assert!(packs.iter().any(|pack| pack.id == "demo-magenta"));
        assert!(packs.iter().any(|pack| pack.id == "demo-cyan-dirt"
            && pack.kind == PackKind::Zip));
    }

    #[test]
    fn selection_skips_base_and_dedupes() {
        let mut sel = PackSelection::default();
        sel.set_enabled(BASE_PACK_ID, true);
        assert!(sel.enabled().is_empty());
        sel.set_enabled("demo-magenta", true);
        sel.set_enabled("demo-magenta", true);
        assert_eq!(sel.enabled(), &["demo-magenta".to_string()]);
        sel.toggle("demo-magenta");
        assert!(!sel.is_enabled("demo-magenta"));
    }

    #[test]
    fn magenta_overlay_bytes() {
        let mut sel = PackSelection::default();
        sel.set_enabled("demo-magenta", true);
        install_selection(sel).expect("save selection");
        let bytes = overlay_bytes("assets/terralite/textures/block/grass_block_side.png");
        assert!(bytes.is_some());
        // Reset so other tests / play sessions start clean.
        install_selection(PackSelection::default()).expect("clear selection");
    }
}
