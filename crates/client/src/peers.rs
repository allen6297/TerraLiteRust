//! Other players on a joined server.
//!
//! Each one is the same Blockbench player body as the local avatar, posed at the
//! eye the server sent. They are not saved and they do not block a walk. A solo
//! world has none. A short nameplate rides above the head in screen space.

use glam::{Mat4, Vec3, Vec4};

use crate::gui;
use crate::hotbar::Quad;
use crate::mesh::Vertex;
use crate::player;

pub struct Other {
    pub eye: [f32; 3],
    pub yaw: f32,
    pub name: String,
}

pub fn mesh(players: &[Other]) -> Vec<Vertex> {
    let mut out = Vec::new();
    for player in players {
        // Camera yaw 0 looks +X. The Blockbench figure faces +Z at yaw 0.
        let (sy, cy) = player.yaw.sin_cos();
        let model_yaw = cy.atan2(sy);
        out.extend(player::peer_mesh(player.eye, model_yaw));
    }
    out
}

/// Default Host/Join label when the peer sent no name (`Player2`, `Player3`, …).
pub fn default_label(index: usize) -> String {
    format!("Player{}", index + 2)
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
            },
            Other {
                eye: [10.0, 5.12, 12.0],
                yaw: 1.0,
                name: String::new(),
            },
        ];
        let body = mesh(&players);
        assert!(!body.is_empty());
        assert_eq!(body.len() % 36, 0);
        assert!(body.iter().any(|vertex| vertex.color[2] > 0.6));
        assert!(body.iter().any(|vertex| vertex.position[1] > 5.0));
        assert!(mesh(&[]).is_empty());
        assert_eq!(default_label(0), "Player2");
        assert_eq!(default_label(1), "Player3");
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
        }];
        let plates = nameplate_quads(&players, vp, 1280.0, 720.0, eye);
        assert!(!plates.is_empty());
        assert!(nameplate_quads(&[], vp, 1280.0, 720.0, eye).is_empty());
    }
}
