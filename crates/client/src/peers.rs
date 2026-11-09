//! Other players on a joined server.
//!
//! Each one is the same Blockbench player body as the local avatar, posed at the
//! eye the server sent. They are not saved and they do not block a walk. A solo
//! world has none.

use crate::mesh::Vertex;
use crate::player;

pub struct Other {
    pub eye: [f32; 3],
    pub yaw: f32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_players_draw_a_body_above_their_feet() {
        let players = [
            Other {
                eye: [8.5, 5.12, 14.5],
                yaw: 0.0,
            },
            Other {
                eye: [10.0, 5.12, 12.0],
                yaw: 1.0,
            },
        ];
        let body = mesh(&players);
        assert!(!body.is_empty());
        assert_eq!(body.len() % 36, 0);
        assert!(body.iter().any(|vertex| vertex.color[2] > 0.6));
        assert!(body.iter().any(|vertex| vertex.position[1] > 5.0));
        assert!(mesh(&[]).is_empty());
    }
}
