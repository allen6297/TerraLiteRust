//! A small slice of a Blockbench model.
//!
//! Cubes are named bones. Sixteen units are one block, Y is up, and the model
//! faces +Z. An `animations` list is a `.bbmodel` save. An `animations` object
//! is a Bedrock clip export. `idle` and `walk` are the clips a mob can play.

use serde_json::Value;

use crate::mesh::Vertex;

const UNIT: f32 = 16.0;

#[derive(Clone, Debug)]
pub struct Figure {
    bones: Vec<Bone>,
    clips: Vec<Clip>,
}

#[derive(Clone, Debug)]
struct Bone {
    name: String,
    from: [f32; 3],
    to: [f32; 3],
    origin: [f32; 3],
    rest: [f32; 3],
    color: [f32; 3],
}

#[derive(Clone, Debug)]
struct Clip {
    name: String,
    length: f32,
    bones: Vec<(String, Vec<Key>)>,
}

#[derive(Clone, Copy, Debug)]
struct Key {
    time: f32,
    rot: [f32; 3],
}

pub fn parse(text: &str) -> Result<Figure, String> {
    let root: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let root = root
        .as_object()
        .ok_or_else(|| "model must be an object".to_string())?;
    let elements = root
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| "model has no elements".to_string())?;
    if elements.is_empty() {
        return Err("model has no elements".to_string());
    }
    let mut bones = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let obj = element
            .as_object()
            .ok_or_else(|| format!("element {index} must be an object"))?;
        if let Some(kind) = obj.get("type").and_then(Value::as_str) {
            if kind != "cube" {
                return Err(format!("element {index} type {kind} is not a cube"));
            }
        }
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("cube")
            .to_string();
        let from = point(obj.get("from"), &format!("element {index} from"))?;
        let to = point(obj.get("to"), &format!("element {index} to"))?;
        for axis in 0..3 {
            if from[axis] > to[axis] {
                return Err(format!("element {index} from must not pass to"));
            }
        }
        let origin = match obj.get("origin") {
            Some(value) => point(Some(value), &format!("element {index} origin"))?,
            None => [
                (from[0] + to[0]) * 0.5,
                (from[1] + to[1]) * 0.5,
                (from[2] + to[2]) * 0.5,
            ],
        };
        let rest = match obj.get("rotation") {
            Some(value) => point(Some(value), &format!("element {index} rotation"))?,
            None => [0.0, 0.0, 0.0],
        };
        let color = tint(obj.get("color"), &name)?;
        bones.push(Bone {
            name,
            from,
            to,
            origin,
            rest,
            color,
        });
    }
    let clips = clips_of(root)?;
    Ok(Figure { bones, clips })
}

/// Draw `clip` at `time` seconds. `yaw` turns the whole model around its center.
/// `hide` drops cubes with those names. A missing clip draws the rest pose.
pub fn mesh(
    figure: &Figure,
    clip: &str,
    time: f32,
    yaw: f32,
    feet: [f32; 3],
    hide: &[&str],
) -> Vec<Vertex> {
    posed(figure, yaw, feet, hide, |bone| {
        bone_rotation(figure, clip, time, bone)
    })
}

/// Draw a mix of two clips. `blend` 0 is `from`, 1 is `to`, and the mix eases
/// at both ends so a clip change does not pop.
pub fn mesh_blend(
    figure: &Figure,
    from: &str,
    to: &str,
    time: f32,
    yaw: f32,
    feet: [f32; 3],
    hide: &[&str],
    blend: f32,
) -> Vec<Vertex> {
    let blend = smooth(blend.clamp(0.0, 1.0));
    posed(figure, yaw, feet, hide, |bone| {
        let rest = bone_rotation(figure, from, time, bone);
        let next = bone_rotation(figure, to, time, bone);
        [
            rest[0] + (next[0] - rest[0]) * blend,
            rest[1] + (next[1] - rest[1]) * blend,
            rest[2] + (next[2] - rest[2]) * blend,
        ]
    })
}

fn posed(
    figure: &Figure,
    yaw: f32,
    feet: [f32; 3],
    hide: &[&str],
    rotation: impl Fn(&Bone) -> [f32; 3],
) -> Vec<Vertex> {
    let mut out = Vec::new();
    for bone in &figure.bones {
        if hide.iter().any(|name| *name == bone.name) {
            continue;
        }
        push_bone(&mut out, bone, rotation(bone), yaw, feet);
    }
    out
}

fn bone_rotation(figure: &Figure, clip: &str, time: f32, bone: &Bone) -> [f32; 3] {
    let Some(clip) = figure.clips.iter().find(|item| item.name == clip) else {
        return bone.rest;
    };
    let time = if clip.length > 0.0 {
        time.rem_euclid(clip.length)
    } else {
        0.0
    };
    clip.bones
        .iter()
        .find(|(name, _)| name == &bone.name)
        .map(|(_, keys)| sample(keys, time))
        .unwrap_or(bone.rest)
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn clips_of(root: &serde_json::Map<String, Value>) -> Result<Vec<Clip>, String> {
    let Some(animations) = root.get("animations") else {
        return Ok(Vec::new());
    };
    if let Some(list) = animations.as_array() {
        return list
            .iter()
            .enumerate()
            .map(|(index, item)| bb_clip(item, index))
            .collect();
    }
    let Some(map) = animations.as_object() else {
        return Err("animations must be a list or an object".to_string());
    };
    map.iter()
        .map(|(name, item)| bedrock_clip(name, item))
        .collect()
}

fn bb_clip(value: &Value, index: usize) -> Result<Clip, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("animation {index} must be an object"))?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("animation {index} needs a name"))?
        .to_string();
    let length = number(obj.get("length").or_else(|| obj.get("animation_length")))
        .unwrap_or(0.0)
        .max(0.0);
    let mut bones = Vec::new();
    if let Some(animators) = obj.get("animators").and_then(Value::as_object) {
        for (key, animator) in animators {
            let Some(animator) = animator.as_object() else {
                continue;
            };
            let bone = animator
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(key.as_str())
                .to_string();
            let mut keys = Vec::new();
            let Some(frames) = animator.get("keyframes").and_then(Value::as_array) else {
                bones.push((bone, keys));
                continue;
            };
            for frame in frames {
                let Some(frame) = frame.as_object() else {
                    continue;
                };
                if frame.get("channel").and_then(Value::as_str) != Some("rotation") {
                    continue;
                }
                let Some(time) = number(frame.get("time")) else {
                    continue;
                };
                let Some(rot) = data_point(frame.get("data_points")) else {
                    continue;
                };
                keys.push(Key { time, rot });
            }
            keys.sort_by(|a, b| {
                a.time
                    .partial_cmp(&b.time)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            bones.push((bone, keys));
        }
    }
    Ok(Clip {
        name,
        length,
        bones,
    })
}

fn bedrock_clip(name: &str, value: &Value) -> Result<Clip, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("animation {name} must be an object"))?;
    let length = number(obj.get("animation_length").or_else(|| obj.get("length")))
        .unwrap_or(0.0)
        .max(0.0);
    let mut bones = Vec::new();
    if let Some(list) = obj.get("bones").and_then(Value::as_object) {
        for (bone, pose) in list {
            let Some(pose) = pose.as_object() else {
                continue;
            };
            let Some(rotation) = pose.get("rotation").and_then(Value::as_object) else {
                continue;
            };
            let mut keys = Vec::new();
            for (time, rot) in rotation {
                let Ok(time) = time.parse::<f32>() else {
                    continue;
                };
                let Some(rot) = point(Some(rot), "rotation").ok() else {
                    continue;
                };
                keys.push(Key { time, rot });
            }
            keys.sort_by(|a, b| {
                a.time
                    .partial_cmp(&b.time)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            bones.push((bone.clone(), keys));
        }
    }
    Ok(Clip {
        name: name.to_string(),
        length,
        bones,
    })
}

fn data_point(value: Option<&Value>) -> Option<[f32; 3]> {
    let value = value?;
    let point = value
        .as_array()
        .and_then(|list| list.first())
        .unwrap_or(value);
    let obj = point.as_object()?;
    Some([
        number(obj.get("x"))?,
        number(obj.get("y"))?,
        number(obj.get("z"))?,
    ])
}

fn sample(keys: &[Key], time: f32) -> [f32; 3] {
    let Some(first) = keys.first() else {
        return [0.0, 0.0, 0.0];
    };
    if time <= first.time {
        return first.rot;
    }
    for pair in keys.windows(2) {
        if time <= pair[1].time {
            let span = (pair[1].time - pair[0].time).max(0.0001);
            let t = (time - pair[0].time) / span;
            return [
                pair[0].rot[0] + (pair[1].rot[0] - pair[0].rot[0]) * t,
                pair[0].rot[1] + (pair[1].rot[1] - pair[0].rot[1]) * t,
                pair[0].rot[2] + (pair[1].rot[2] - pair[0].rot[2]) * t,
            ];
        }
    }
    keys.last().map(|key| key.rot).unwrap_or([0.0, 0.0, 0.0])
}

fn tint(value: Option<&Value>, name: &str) -> Result<[f32; 3], String> {
    if let Some(value) = value {
        let rgb = point(Some(value), &format!("{name} color"))?;
        let scale = if rgb.iter().any(|channel| *channel > 1.0) {
            255.0
        } else {
            1.0
        };
        return Ok([rgb[0] / scale, rgb[1] / scale, rgb[2] / scale]);
    }
    Ok(match name {
        "body" | "head" => [0.95, 0.95, 0.96],
        "beak" => [0.95, 0.72, 0.18],
        "comb" | "wattle" => [0.86, 0.16, 0.18],
        "leg_l" | "leg_r" | "leg" => [0.93, 0.55, 0.28],
        "egg" => [0.98, 0.90, 0.55],
        _ => [0.80, 0.80, 0.80],
    })
}

fn point(value: Option<&Value>, what: &str) -> Result<[f32; 3], String> {
    let Some(value) = value else {
        return Err(format!("{what} is missing"));
    };
    let coords = value
        .as_array()
        .ok_or_else(|| format!("{what} must be 3 numbers"))?;
    if coords.len() != 3 {
        return Err(format!("{what} needs 3 numbers"));
    }
    let mut out = [0.0; 3];
    for (axis, coord) in coords.iter().enumerate() {
        out[axis] = number(Some(coord)).ok_or_else(|| format!("{what} must be 3 numbers"))?;
    }
    Ok(out)
}

fn number(value: Option<&Value>) -> Option<f32> {
    let value = value?;
    value
        .as_f64()
        .map(|n| n as f32)
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn push_bone(out: &mut Vec<Vertex>, bone: &Bone, rot: [f32; 3], yaw: f32, feet: [f32; 3]) {
    let min = bone.from;
    let max = bone.to;
    let corner = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    let corner = corner.map(|p| place(p, bone.origin, rot, yaw, feet));
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [3, 2, 6, 7],
        [0, 3, 7, 4],
        [1, 2, 6, 5],
    ];
    const NORMALS: [[f32; 3]; 6] = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    for (face, normal) in FACES.into_iter().zip(NORMALS) {
        let normal = face_normal(
            corner[face[0]],
            corner[face[1]],
            corner[face[2]],
            normal,
            rot,
            yaw,
        );
        let tri = [face[0], face[1], face[2], face[0], face[2], face[3]];
        for index in tri {
            out.push(Vertex {
                position: corner[index],
                color: bone.color,
                normal,
                uv: [0.0, 0.0],
                textured: 0.0,
                block: bone.color,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

fn face_normal(
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    fallback: [f32; 3],
    rot: [f32; 3],
    yaw: f32,
) -> [f32; 3] {
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let n = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 0.0001 {
        return spin(fallback, rot, yaw);
    }
    [n[0] / len, n[1] / len, n[2] / len]
}

/// Model center, in blocks. Cubes sit in a 16-unit box, so the middle is half a block.
const PIVOT: f32 = 0.5;

fn place(point: [f32; 3], origin: [f32; 3], rot: [f32; 3], yaw: f32, feet: [f32; 3]) -> [f32; 3] {
    let local = [
        (point[0] - origin[0]) / UNIT,
        (point[1] - origin[1]) / UNIT,
        (point[2] - origin[2]) / UNIT,
    ];
    let turned = spin_bone(local, rot);
    let model = [
        origin[0] / UNIT + turned[0],
        origin[1] / UNIT + turned[1],
        origin[2] / UNIT + turned[2],
    ];
    let yawed = rot_y([model[0] - PIVOT, model[1], model[2] - PIVOT], yaw);
    [
        feet[0] + yawed[0] + PIVOT,
        feet[1] + yawed[1],
        feet[2] + yawed[2] + PIVOT,
    ]
}

fn spin(point: [f32; 3], rot: [f32; 3], yaw: f32) -> [f32; 3] {
    rot_y(spin_bone(point, rot), yaw)
}

fn spin_bone(point: [f32; 3], rot: [f32; 3]) -> [f32; 3] {
    let point = rot_x(point, rot[0].to_radians());
    let point = rot_y(point, rot[1].to_radians());
    rot_z(point, rot[2].to_radians())
}

fn rot_x(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]
}

fn rot_y(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]
}

fn rot_z(p: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_z(vertices: &[Vertex]) -> f32 {
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;
        for vertex in vertices {
            min_z = min_z.min(vertex.position[2]);
            max_z = max_z.max(vertex.position[2]);
        }
        max_z - min_z
    }

    #[test]
    fn idle_dips_the_head_and_walk_swings_the_legs() {
        let figure = parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/chicken.bbmodel"
        ))
        .expect("chicken model");
        let up = mesh(&figure, "idle", 0.0, 0.0, [0.0, 0.0, 0.0], &[]);
        let down = mesh(&figure, "idle", 0.4, 0.0, [0.0, 0.0, 0.0], &[]);
        let comb = |vertices: &[Vertex]| {
            vertices
                .iter()
                .filter(|vertex| vertex.color[0] > 0.8 && vertex.color[1] < 0.25)
                .map(|vertex| vertex.position[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(
            comb(&down) < comb(&up) - 0.15,
            "the peck lowers the comb, up {} down {}",
            comb(&up),
            comb(&down)
        );
        let legs = |time: f32| {
            let posed = mesh(&figure, "walk", time, 0.0, [0.0, 0.0, 0.0], &[]);
            let legs: Vec<_> = posed
                .into_iter()
                .filter(|vertex| {
                    vertex.color[0] > 0.9 && vertex.color[1] > 0.5 && vertex.color[1] < 0.65
                })
                .collect();
            span_z(&legs)
        };
        assert!(
            legs(0.0) > legs(0.15) + 0.1,
            "the walk swings the feet apart"
        );
    }

    #[test]
    fn a_bedrock_clip_rotates_the_named_bone() {
        let figure = parse(
            r#"{
                "elements": [{ "name": "head", "from": [0, 0, 0], "to": [1, 1, 4] }],
                "animations": {
                    "idle": {
                        "animation_length": 1,
                        "bones": { "head": { "rotation": { "0": [0, 0, 0], "0.5": [90, 0, 0] } } }
                    }
                }
            }"#,
        )
        .unwrap();
        let flat = mesh(&figure, "idle", 0.0, 0.0, [0.0, 0.0, 0.0], &[]);
        let turned = mesh(&figure, "idle", 0.5, 0.0, [0.0, 0.0, 0.0], &[]);
        let max_y = |vertices: &[Vertex]| {
            vertices
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(max_y(&turned) > max_y(&flat) + 0.05);
    }

    #[test]
    fn an_axolotl_idle_opens_the_gills_and_a_swim_lifts_the_tail() {
        let figure = parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/axolotl.bbmodel"
        ))
        .expect("axolotl model");
        let rest = mesh(&figure, "idle", 0.0, 0.0, [0.0, 0.0, 0.0], &[]);
        let early = mesh(&figure, "idle", 0.4, 0.0, [0.0, 0.0, 0.0], &[]);
        let open = mesh(&figure, "idle", 1.2, 0.0, [0.0, 0.0, 0.0], &[]);
        let gill = |vertices: &[Vertex]| {
            vertices
                .iter()
                .filter(|vertex| vertex.color[0] > 0.95 && vertex.color[1] < 0.25)
                .map(|vertex| vertex.position[0])
                .fold(f32::MIN, f32::max)
        };
        let opened = (gill(&open) - gill(&rest)).abs();
        let started = (gill(&early) - gill(&rest)).abs();
        assert!(opened > 0.05, "the gills sway");
        assert!(
            started > 0.02 && started < opened,
            "the gills ease open, early {started} full {opened}"
        );
        let mixed = mesh_blend(&figure, "idle", "walk", 1.2, 0.0, [0.0, 0.0, 0.0], &[], 0.5);
        let mixed = (gill(&mixed) - gill(&rest)).abs();
        assert!(
            mixed < opened,
            "a half blend sits between the clips, mixed {mixed} open {opened}"
        );
        let tail = |time: f32| {
            let posed = mesh(&figure, "walk", time, 0.0, [0.0, 0.0, 0.0], &[]);
            posed
                .into_iter()
                .filter(|vertex| {
                    vertex.color[0] > 0.85 && vertex.color[0] < 0.94 && vertex.color[1] < 0.35
                })
                .map(|vertex| vertex.position[1])
                .fold(f32::MIN, f32::max)
        };
        let lifted = (tail(0.8) - tail(0.0)).abs();
        let rising = (tail(0.4) - tail(0.0)).abs();
        assert!(lifted > 0.05, "the swim lifts the tail");
        assert!(
            rising > 0.02 && rising < lifted,
            "the tail eases up, early {rising} full {lifted}"
        );
    }

    #[test]
    fn a_yaw_turns_the_head_tail_and_gills_with_the_body() {
        let figure = parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/axolotl.bbmodel"
        ))
        .expect("axolotl model");
        let mean = |vertices: &[Vertex], axis: usize| {
            let sum: f32 = vertices.iter().map(|vertex| vertex.position[axis]).sum();
            sum / vertices.len() as f32
        };
        let part = |yaw: f32, keep: fn(&[f32; 3]) -> bool| {
            mesh(&figure, "idle", 0.0, yaw, [0.0, 0.0, 0.0], &[])
                .into_iter()
                .filter(|vertex| keep(&vertex.color))
                .collect::<Vec<_>>()
        };
        let head = |color: &[f32; 3]| color[0] > 0.95 && color[1] > 0.45 && color[1] < 0.6;
        let tail = |color: &[f32; 3]| color[0] > 0.85 && color[0] < 0.94 && color[1] < 0.35;
        let gills = |color: &[f32; 3]| color[0] > 0.95 && color[1] < 0.25;
        let span = |vertices: &[Vertex], axis: usize| {
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            for vertex in vertices {
                min = min.min(vertex.position[axis]);
                max = max.max(vertex.position[axis]);
            }
            max - min
        };
        let rest_head = part(0.0, head);
        let rest_tail = part(0.0, tail);
        let rest_gills = part(0.0, gills);
        assert!(mean(&rest_head, 2) > mean(&rest_tail, 2) + 0.3);
        let turned_head = part(std::f32::consts::FRAC_PI_2, head);
        let turned_tail = part(std::f32::consts::FRAC_PI_2, tail);
        let turned_gills = part(std::f32::consts::FRAC_PI_2, gills);
        assert!(
            mean(&turned_head, 0) > mean(&turned_tail, 0) + 0.3,
            "the head and tail yaw with the body"
        );
        assert!(
            span(&turned_gills, 2) > span(&rest_gills, 2),
            "the gills swing around to the side"
        );
    }
}
