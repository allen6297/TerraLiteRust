//! Frame shader hook in the resource pack.
//!
//! `assets/terralite/shaders/frame.json` turns the pass on. When it is on, the
//! client loads the WGSL next to it and multiplies the lit picture by a tint
//! and a brightness. The world shader is not replaced. `"enabled": false`, or
//! a missing hook file, skips the pass.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const SHADER_PATH: &str = "assets/terralite/shaders/frame.wgsl";

pub fn frame_hook_file() -> PathBuf {
    rebuild_world::resource_pack_root()
        .join("assets")
        .join("terralite")
        .join("shaders")
        .join("frame.json")
}

pub fn frame_shader_file() -> PathBuf {
    rebuild_world::resource_pack_root()
        .join("assets")
        .join("terralite")
        .join("shaders")
        .join("frame.wgsl")
}

pub struct FrameHook {
    pub enabled: bool,
    pub source: String,
}

impl FrameHook {
    pub fn off() -> Self {
        Self {
            enabled: false,
            source: String::new(),
        }
    }
}

/// The same multiply the frame shader writes: `clamp(color * tint * brightness)`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_frame(color: [f32; 3], tint: [f32; 3], brightness: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for channel in 0..3 {
        out[channel] = (color[channel] * tint[channel] * brightness).clamp(0.0, 1.0);
    }
    out
}

pub fn load_frame_hook() -> Result<FrameHook, String> {
    let root = rebuild_world::resource_pack_root();
    if !root.join("pack.json").is_file() {
        return Err(format!(
            "resource pack missing pack.json ({})",
            root.display()
        ));
    }
    let path = frame_hook_file();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return Ok(FrameHook::off()),
    };
    let hook = parse_hook(&text)?;
    if !hook.enabled {
        return Ok(FrameHook::off());
    }
    let file =
        pack_file(root, &hook.shader).ok_or_else(|| format!("bad shader path {}", hook.shader))?;
    let source =
        fs::read_to_string(&file).map_err(|_| format!("missing shader {}", file.display()))?;
    validate_frame_shader(&source)?;
    Ok(FrameHook {
        enabled: true,
        source,
    })
}

struct HookFile {
    enabled: bool,
    shader: String,
}

fn parse_hook(text: &str) -> Result<HookFile, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| format!("frame.json: {err}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "frame.json must be an object".to_string())?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "enabled" | "shader") {
            return Err(format!("frame.json has unknown field {key}"));
        }
    }
    let enabled = match obj.get("enabled") {
        Some(Value::Bool(enabled)) => *enabled,
        Some(_) => return Err("frame.json enabled must be true or false".into()),
        None => return Err("frame.json needs enabled".into()),
    };
    let shader = match obj.get("shader") {
        None => SHADER_PATH.to_string(),
        Some(Value::String(path)) if !path.is_empty() => path.clone(),
        Some(Value::String(_)) => return Err("frame.json shader is empty".into()),
        Some(_) => return Err("frame.json shader must be a string".into()),
    };
    Ok(HookFile { enabled, shader })
}

fn validate_frame_shader(source: &str) -> Result<(), String> {
    if source.len() > 64 * 1024 {
        return Err("frame shader is too large".into());
    }
    for needle in [
        "fn vs_main",
        "fn fs_main",
        "@vertex",
        "@fragment",
        "frame_tex",
        "frame_samp",
    ] {
        if !source.contains(needle) {
            return Err(format!("frame shader is missing {needle}"));
        }
    }
    if !source.contains("color * tint * brightness") {
        return Err("frame shader must multiply color * tint * brightness".into());
    }
    Ok(())
}

fn pack_file(root: &Path, path: &str) -> Option<PathBuf> {
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
    fn the_shipped_hook_is_an_identity_tint() {
        let hook_path = frame_hook_file();
        let shader_path = frame_shader_file();
        assert!(hook_path.ends_with(std::path::Path::new("assets/terralite/shaders/frame.json")));
        assert!(shader_path.ends_with(std::path::Path::new("assets/terralite/shaders/frame.wgsl")));
        let hook = load_frame_hook().unwrap();
        assert!(hook.enabled);
        assert!(hook.source.contains("let tint = vec3<f32>(1.0, 1.0, 1.0);"));
        assert!(hook.source.contains("let brightness = 1.0;"));
        assert!(hook.source.contains("color * tint * brightness"));
        assert!(!hook.source.to_lowercase().contains("ndotl"));
        let white = apply_frame([1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 1.0);
        assert_eq!(white, [1.0, 1.0, 1.0]);
        assert_eq!(
            apply_frame([1.0, 0.5, 0.25], [1.0, 0.5, 0.0], 1.0),
            [1.0, 0.25, 0.0]
        );
        assert_eq!(
            apply_frame([0.8, 0.8, 0.8], [1.0, 1.0, 1.0], 0.5),
            [0.4, 0.4, 0.4]
        );
        assert_eq!(
            apply_frame([0.8, 0.2, 0.2], [2.0, 1.0, 1.0], 1.0),
            [1.0, 0.2, 0.2]
        );
    }

    #[test]
    fn disabling_the_hook_skips_the_shader() {
        let off = parse_hook(r#"{ "enabled": false }"#).unwrap();
        assert!(!off.enabled);
        assert!(parse_hook(r#"{ "enabled": false, "extra": 1 }"#).is_err());
        assert!(parse_hook(r#"{ "enabled": "yes" }"#).is_err());
        assert!(pack_file(Path::new("root"), "../frame.wgsl").is_none());
        assert!(pack_file(Path::new("root"), "assets/terralite/shaders/frame.wgsl").is_some());
        assert!(validate_frame_shader("fn fs_main() {}").is_err());
    }
}
