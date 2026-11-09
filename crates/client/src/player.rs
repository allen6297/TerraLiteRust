//! Local player Blockbench body.
//!
//! Cubes live in `models/entity/player.bbmodel` with `textures/entity/player.png`.
//! Sixteen units are one block, the model faces +Z, and the feet sit on Y = 0.
//! `idle`, `walk`, and `dig` are the clips. Parts may be `head`/`body`/`arm_l`…
//! or Blockbench names `Head`/`Body`/`Larm`/`Rarm`/`Lleg`/`Rleg`.

use crate::blockbench::{self, Figure};
use crate::body::EYE_HEIGHT;
use crate::mesh::Vertex;

pub struct Pose {
    pose: f32,
    yaw: f32,
    walking: bool,
    digging: bool,
}

impl Pose {
    pub fn new() -> Self {
        Self {
            pose: 0.0,
            yaw: 0.0,
            walking: false,
            digging: false,
        }
    }

    /// Advance the clip clock and face the look on the ground plane.
    pub fn tick(&mut self, dt: f32, look_x: f32, look_z: f32, walking: bool, digging: bool) {
        self.pose = (self.pose + dt.max(0.0)).rem_euclid(64.0);
        if look_x * look_x + look_z * look_z > 1.0e-8 {
            self.yaw = look_x.atan2(look_z);
        }
        self.walking = walking;
        self.digging = digging;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    fn clip(&self) -> &'static str {
        if self.digging {
            "dig"
        } else if self.walking {
            "walk"
        } else {
            "idle"
        }
    }

    /// Draw the body at the camera feet. `hide_head` drops the skull in first person.
    pub fn vertices(&self, eye: [f32; 3], hide_head: bool) -> Vec<Vertex> {
        let feet = [eye[0], eye[1] - EYE_HEIGHT, eye[2]];
        let hide: &[&str] = if hide_head { &["head"] } else { &[] };
        blockbench::mesh(figure(), self.clip(), self.pose, self.yaw, feet, hide)
    }
}

fn figure() -> &'static Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/player.bbmodel"
        ))
        .expect("player.bbmodel")
    })
}

/// Mesh used by remote peers: rest pose facing `yaw` (Blockbench +Z convention).
pub fn peer_mesh(eye: [f32; 3], yaw: f32) -> Vec<Vertex> {
    let feet = [eye[0], eye[1] - EYE_HEIGHT, eye[2]];
    blockbench::mesh(figure(), "idle", 0.0, yaw, feet, &[])
}

/// Soft-render the idle body for an egui inventory preview (third-person style).
///
/// Returns tightly packed RGBA8 pixels, `width * height * 4` bytes. Transparent
/// outside the silhouette. The model faces the viewer with a slight yaw so both
/// the front and a bit of the side read.
pub fn preview_rgba(width: usize, height: usize) -> Vec<u8> {
    let width = width.max(8);
    let height = height.max(8);
    // Mid-idle so the arms breathe; yaw keeps the face toward the camera (+Z).
    let vertices = blockbench::mesh(figure(), "idle", 0.35, 0.0, [0.0, 0.0, 0.0], &[]);
    soft_project(&vertices, width, height)
}

fn soft_project(vertices: &[Vertex], width: usize, height: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; width * height * 4];
    let mut depth = vec![f32::INFINITY; width * height];
    // Slight turn + elevated look, like F5 from in front.
    let yaw = 0.45_f32;
    let (sy, cy) = (yaw.sin(), yaw.cos());
    let scale = height as f32 * 0.40;
    let cx = width as f32 * 0.5;
    let cy_screen = height as f32 * 0.55;
    let body_mid = 0.95_f32;
    let project = |p: [f32; 3]| -> (f32, f32, f32) {
        let x = p[0] * cy + p[2] * sy;
        let z = -p[0] * sy + p[2] * cy;
        let sx = x * scale + cx;
        let sy = (body_mid - p[1]) * scale + cy_screen;
        (sx, sy, z)
    };
    for tri in vertices.chunks_exact(3) {
        let (ax, ay, az) = project(tri[0].position);
        let (bx, by, bz) = project(tri[1].position);
        let (cxp, cyp, cz) = project(tri[2].position);
        let color = tri[0].color;
        let min_x = ax.min(bx).min(cxp).floor().max(0.0) as usize;
        let max_x = ax
            .max(bx)
            .max(cxp)
            .ceil()
            .min(width as f32 - 1.0) as usize;
        let min_y = ay.min(by).min(cyp).floor().max(0.0) as usize;
        let max_y = ay
            .max(by)
            .max(cyp)
            .ceil()
            .min(height as f32 - 1.0) as usize;
        let area = (bx - ax) * (cyp - ay) - (cxp - ax) * (by - ay);
        if area.abs() < 1.0e-5 {
            continue;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = ((bx - px) * (cyp - py) - (cxp - px) * (by - py)) / area;
                let w1 = ((cxp - px) * (ay - py) - (ax - px) * (cyp - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -0.01 || w1 < -0.01 || w2 < -0.01 {
                    continue;
                }
                let z = az * w0 + bz * w1 + cz * w2;
                let i = y * width + x;
                if z >= depth[i] {
                    continue;
                }
                depth[i] = z;
                // Face lighting from the projected normal tilt.
                let shade = (0.72 + 0.28 * w0).clamp(0.55, 1.0);
                let o = i * 4;
                rgba[o] = (color[0] * shade * 255.0).clamp(0.0, 255.0) as u8;
                rgba[o + 1] = (color[1] * shade * 255.0).clamp(0.0, 255.0) as u8;
                rgba[o + 2] = (color[2] * shade * 255.0).clamp(0.0, 255.0) as u8;
                rgba[o + 3] = 255;
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_breathes_the_arms_and_walk_swings_the_legs() {
        let figure = figure();
        let rest = blockbench::mesh(figure, "idle", 0.0, 0.0, [0.0, 0.0, 0.0], &[]);
        let mid = blockbench::mesh(figure, "idle", 1.0, 0.0, [0.0, 0.0, 0.0], &[]);
        // Idle swings each arm on X; track the left arm tip's mean Z.
        let left_tip_z = |vertices: &[Vertex]| {
            let tips: Vec<_> = vertices
                .iter()
                .filter(|vertex| {
                    vertex.position[1] > 0.85
                        && vertex.position[1] < 1.55
                        && vertex.position[0] > 0.35
                })
                .map(|vertex| vertex.position[2])
                .collect();
            assert!(!tips.is_empty(), "left arm tips");
            tips.iter().sum::<f32>() / tips.len() as f32
        };
        assert!(
            (left_tip_z(&mid) - left_tip_z(&rest)).abs() > 0.02,
            "idle breathes the arms"
        );
        let span_z = |time: f32| {
            let posed = blockbench::mesh(figure, "walk", time, 0.0, [0.0, 0.0, 0.0], &[]);
            let legs: Vec<_> = posed
                .into_iter()
                .filter(|vertex| vertex.position[1] < 0.75 && vertex.position[0].abs() < 0.35)
                .collect();
            let mut min_z = f32::MAX;
            let mut max_z = f32::MIN;
            for vertex in &legs {
                min_z = min_z.min(vertex.position[2]);
                max_z = max_z.max(vertex.position[2]);
            }
            max_z - min_z
        };
        assert!(
            span_z(0.0) > span_z(0.2) + 0.04,
            "walk swings the feet apart"
        );
    }

    #[test]
    fn dig_raises_the_right_arm() {
        let figure = figure();
        let rest = blockbench::mesh(figure, "dig", 0.0, 0.0, [0.0, 0.0, 0.0], &[]);
        let raised = blockbench::mesh(figure, "dig", 0.2, 0.0, [0.0, 0.0, 0.0], &[]);
        // Character right is -X; keep above the feet so legs are not the tip.
        let hand = |vertices: &[Vertex]| {
            vertices
                .iter()
                .filter(|vertex| {
                    vertex.position[0] < -0.35
                        && vertex.position[1] > 0.4
                        && vertex.position[1] < 1.6
                })
                .map(|vertex| vertex.position[1])
                .fold(f32::MAX, f32::min)
        };
        let rest_y = hand(&rest);
        let raised_y = hand(&raised);
        assert!(
            raised_y.is_finite() && rest_y.is_finite(),
            "dig finds the right hand"
        );
        assert!(
            raised_y > rest_y + 0.15,
            "dig lifts the right hand (rest {rest_y} raised {raised_y})"
        );
    }

    #[test]
    fn a_look_toward_plus_z_keeps_the_head_in_front() {
        let mut pose = Pose::new();
        pose.tick(0.05, 0.0, 1.0, false, false);
        let body = pose.vertices([8.0, 1.0 + EYE_HEIGHT, 8.0], false);
        let head_z = body
            .iter()
            .filter(|vertex| vertex.position[1] > 2.3)
            .map(|vertex| vertex.position[2])
            .fold(f32::MIN, f32::max);
        let leg_z = body
            .iter()
            .filter(|vertex| vertex.position[1] < 1.7)
            .map(|vertex| vertex.position[2])
            .fold(f32::MIN, f32::max);
        assert!(head_z > leg_z, "the face points +Z with the look");
        assert!((pose.yaw() - 0.0).abs() < 1.0e-4);
    }

    #[test]
    fn first_person_can_hide_the_head() {
        let mut pose = Pose::new();
        pose.tick(0.0, 1.0, 0.0, false, false);
        let full = pose.vertices([0.0, EYE_HEIGHT, 0.0], false);
        let bare = pose.vertices([0.0, EYE_HEIGHT, 0.0], true);
        assert!(bare.len() < full.len());
        assert!(bare.iter().all(|vertex| vertex.position[1] < 1.55));
    }

    #[test]
    fn inventory_preview_paints_a_silhouette() {
        let rgba = preview_rgba(64, 96);
        assert_eq!(rgba.len(), 64 * 96 * 4);
        let opaque = rgba.chunks_exact(4).filter(|px| px[3] > 200).count();
        assert!(
            opaque > 64 * 96 / 10,
            "expected a player silhouette, got {opaque} opaque pixels"
        );
    }

    #[test]
    fn inventory_preview_uses_skin_not_cyan_grid() {
        let rgba = preview_rgba(96, 144);
        let opaque: Vec<_> = rgba.chunks_exact(4).filter(|px| px[3] > 200).collect();
        assert!(opaque.len() > 96 * 144 / 12);
        let mut sum = [0u64; 3];
        for px in &opaque {
            sum[0] += px[0] as u64;
            sum[1] += px[1] as u64;
            sum[2] += px[2] as u64;
        }
        let n = opaque.len().max(1) as f64;
        let mean = [sum[0] as f64 / n, sum[1] as f64 / n, sum[2] as f64 / n];
        assert!(
            mean[0] > 40.0,
            "skin should keep some warm red channel, mean={mean:?}"
        );
        assert!(
            !(mean[2] > mean[0] + 60.0 && mean[1] > mean[0] + 40.0),
            "skin should not read as cyan/green grid, mean={mean:?}"
        );
    }
}
