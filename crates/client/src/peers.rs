//! Other players on a joined server.
//!
//! Each one is the same Blockbench player body as the local avatar, posed at the
//! eye the server sent. They are not saved and they do not block a walk. A solo
//! world has none. A short nameplate rides above the head in screen space. The
//! selected hotbar item is drawn as a small cube in the right hand. A dig/attack
//! swing pulse plays the dig clip and lifts the grip briefly.

use glam::{Mat4, Vec3, Vec4};

use crate::gui;
use crate::hotbar::Quad;
use crate::mesh::Vertex;
use crate::palette;
use crate::player;

pub struct Other {
    pub eye: [f32; 3],
    pub yaw: f32,
    pub name: String,
    /// Selected hotbar item id; empty when bare-handed.
    pub held: String,
    /// True while the peer is left-click digging / attacking.
    pub swinging: bool,
    /// Dig/idle clip clock in seconds (advanced locally while swinging).
    pub pose: f32,
}

pub fn mesh(players: &[Other]) -> Vec<Vertex> {
    let mut out = Vec::new();
    for player in players {
        // Camera yaw 0 looks +X. The Blockbench figure faces +Z at yaw 0.
        let (sy, cy) = player.yaw.sin_cos();
        let model_yaw = cy.atan2(sy);
        out.extend(player::peer_mesh(
            player.eye,
            model_yaw,
            player.swinging,
            player.pose,
        ));
        if !player.held.is_empty() {
            out.extend(held_cube(
                player.eye,
                player.yaw,
                &player.held,
                player.swinging,
                player.pose,
            ));
        }
    }
    out
}

/// Advance dig clocks for swinging peers. Returns true when any mesh must rebuild.
pub fn tick(players: &mut [Other], dt: f32) -> bool {
    let mut dirty = false;
    for player in players {
        if player.swinging {
            player.pose = (player.pose + dt.max(0.0)).rem_euclid(64.0);
            dirty = true;
        } else if player.pose != 0.0 {
            player.pose = 0.0;
            dirty = true;
        }
    }
    dirty
}

/// Default Host/Join label when the peer sent no name (`Player2`, `Player3`, …).
pub fn default_label(index: usize) -> String {
    format!("Player{}", index + 2)
}

/// Right-hand grip in world space for a peer facing `yaw` (camera convention).
#[cfg_attr(not(test), allow(dead_code))]
pub fn hand_pos(eye: [f32; 3], yaw: f32) -> [f32; 3] {
    hand_pos_swing(eye, yaw, false, 0.0)
}

/// Grip with a brief dig lift so the held cube follows the swing.
pub fn hand_pos_swing(eye: [f32; 3], yaw: f32, swinging: bool, pose: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let forward = [cy, 0.0, sy];
    let right = [-sy, 0.0, cy];
    // Dig clip cycles ~0.5s; lift the grip on the downstroke so others see it.
    let phase = if swinging {
        ((pose * std::f32::consts::TAU * 2.0).sin() * 0.5 + 0.5).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lift = phase * 0.28;
    let reach = phase * 0.18;
    [
        eye[0] + right[0] * 0.38 + forward[0] * (0.28 + reach),
        eye[1] - 0.42 + lift,
        eye[2] + right[2] * 0.38 + forward[2] * (0.28 + reach),
    ]
}

fn held_cube(eye: [f32; 3], yaw: f32, id: &str, swinging: bool, pose: f32) -> Vec<Vertex> {
    let center = hand_pos_swing(eye, yaw, swinging, pose);
    let half = 0.14;
    let textured = crate::texture::block_face(id);
    let color = if textured > 0.0 {
        [1.0, 1.0, 1.0]
    } else {
        palette::color(id, 0)
    };
    let block = if textured > 0.0 {
        [0.55, 0.55, 0.55]
    } else {
        color
    };
    let min = [
        center[0] - half,
        center[1] - half,
        center[2] - half,
    ];
    let max = [
        center[0] + half,
        center[1] + half,
        center[2] + half,
    ];
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
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 5, 1, 0],
        [3, 2, 6, 7],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
    ];
    const NORMALS: [[f32; 3]; 6] = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    // Face UVs so atlas textures still read on the tiny grip cube.
    const UV: [[f32; 2]; 4] = [[0.0, 0.999], [0.999, 0.999], [0.999, 0.0], [0.0, 0.0]];
    let mut out = Vec::with_capacity(36);
    for (face, normal) in FACES.into_iter().zip(NORMALS) {
        let tri = [
            (face[0], 0),
            (face[1], 1),
            (face[2], 2),
            (face[0], 0),
            (face[2], 2),
            (face[3], 3),
        ];
        for (index, uv_i) in tri {
            out.push(Vertex {
                position: corner[index],
                color,
                normal,
                uv: UV[uv_i],
                textured,
                block,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
    out
}

/// World-space nameplates projected into HUD pixels. Fade out past ~32 blocks.
pub fn nameplate_quads(
    players: &[Other],
    view_proj: Mat4,
    width: f32,
    height: f32,
    eye: Vec3,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 || players.is_empty() {
        return Vec::new();
    }
    let (gw, gh) = gui::gui_space(width, height);
    let mut quads = Vec::new();
    for (index, peer) in players.iter().enumerate() {
        let label = if peer.name.is_empty() {
            default_label(index)
        } else {
            peer.name.clone()
        };
        let world = Vec3::new(peer.eye[0], peer.eye[1] + 0.42, peer.eye[2]);
        let dist = (world - eye).length();
        if dist < 1.2 || dist > 36.0 {
            continue;
        }
        let fade = ((36.0 - dist) / 28.0).clamp(0.0, 1.0);
        if fade < 0.12 {
            continue;
        }
        let clip = view_proj * Vec4::new(world.x, world.y, world.z, 1.0);
        if clip.w <= 0.05 {
            continue;
        }
        let ndc = clip.truncate() / clip.w;
        if ndc.z < 0.0 || ndc.z > 1.0 {
            continue;
        }
        if ndc.x.abs() > 1.15 || ndc.y.abs() > 1.15 {
            continue;
        }
        let px = (ndc.x * 0.5 + 0.5) * gw;
        let py = (ndc.y * 0.5 + 0.5) * gh;
        let scale = 0.85 + 0.25 * fade;
        let ink = [0.92 * fade, 0.93 * fade, 0.88 * fade];
        // Center the label roughly on the projected head.
        let text_w = label_width(&label, scale);
        gui::draw_text(
            &mut quads,
            gw,
            gh,
            px - text_w * 0.5,
            py,
            &label,
            ink,
            scale,
        );
    }
    quads
}

fn label_width(text: &str, scale: f32) -> f32 {
    // Matches gui glyph advances roughly (caps 5, lower/digit 3, gap 3) * CELL(3).
    let cell = 3.0 * scale;
    let mut w = 0.0;
    let mut spaced = false;
    for ch in text.chars() {
        let adv = if ch.is_ascii_uppercase() {
            5.0 * cell
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            3.0 * cell
        } else {
            0.0
        };
        if adv == 0.0 {
            continue;
        }
        if spaced {
            w += 3.0 * cell;
        }
        w += adv;
        spaced = true;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_players_draw_a_body_above_their_feet() {
        let players = [
            Other {
                eye: [8.5, 5.12, 14.5],
                yaw: 0.0,
                name: "Player2".into(),
                held: "torch".into(),
                swinging: false,
                pose: 0.0,
            },
            Other {
                eye: [10.0, 5.12, 12.0],
                yaw: 1.0,
                name: String::new(),
                held: String::new(),
                swinging: true,
                pose: 0.18,
            },
        ];
        let body = mesh(&players);
        assert!(!body.is_empty());
        // Body quads (36 verts each) + one held cube (36) for the torch peer.
        assert_eq!(body.len() % 36, 0);
        assert!(body.len() > 72);
        assert!(body.iter().any(|vertex| vertex.color[2] > 0.6));
        assert!(body.iter().any(|vertex| vertex.position[1] > 5.0));
        assert!(mesh(&[]).is_empty());
        assert_eq!(default_label(0), "Player2");
        assert_eq!(default_label(1), "Player3");
        let hand = hand_pos([8.5, 5.12, 14.5], 0.0);
        assert!((hand[1] - (5.12 - 0.42)).abs() < 0.01);
        let swung = hand_pos_swing([8.5, 5.12, 14.5], 0.0, true, 0.12);
        assert!(swung[1] > hand[1]);
    }

    #[test]
    fn tick_advances_swing_pose() {
        let mut players = [Other {
            eye: [8.5, 5.12, 14.5],
            yaw: 0.0,
            name: "Player2".into(),
            held: String::new(),
            swinging: true,
            pose: 0.0,
        }];
        assert!(tick(&mut players, 0.1));
        assert!(players[0].pose > 0.0);
        players[0].swinging = false;
        assert!(tick(&mut players, 0.1));
        assert_eq!(players[0].pose, 0.0);
    }

    #[test]
    fn nameplates_project_a_nearby_peer() {
        let eye = Vec3::new(8.5, 5.0, 14.5);
        let focus = Vec3::new(10.0, 5.0, 12.0);
        let camera = crate::camera::Camera::looking_at(eye, focus);
        let vp = camera.view_proj(16.0 / 9.0);
        let players = [Other {
            eye: [10.0, 5.12, 12.0],
            yaw: 0.0,
            name: "Player2".into(),
            held: String::new(),
            swinging: false,
            pose: 0.0,
        }];
        let plates = nameplate_quads(&players, vp, 1280.0, 720.0, eye);
        assert!(!plates.is_empty());
        assert!(nameplate_quads(&[], vp, 1280.0, 720.0, eye).is_empty());
    }
}
