//! A visible sheep.
//!
//! The entity tick already runs the kind `"sheep"` every 50 ms. Each of those
//! steps walks this body one notch around a square on the demo grass. A player
//! who gets close pushes it off that square. The body is solid, so a walk
//! stops against it. It is not saved.

use rebuild_world::Cluster;

use crate::mesh::Vertex;
use crate::mobs::slide_land;

/// Blocks along one side of the square path.
const LEG: f32 = 2.0;
/// Entity ticks to cross one side.
const STEPS_PER_LEG: u32 = 8;
/// How far the sheep keeps from the player's feet.
const AVOID: f32 = 1.6;

pub struct Sheep {
    steps: u32,
    origin: [f32; 3],
    /// Entity ticks left before the square path resumes.
    loose: u32,
}

impl Sheep {
    pub fn new() -> Self {
        Self {
            steps: 0,
            origin: path_point(0),
            loose: 0,
        }
    }

    /// Shove the sheep away from a wind burst. True when it was inside.
    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        let Some(butt) = crate::mobs::goat_shove(from, self.origin) else {
            return false;
        };
        self.origin[0] += butt.dx;
        self.origin[2] += butt.dz;
        self.loose = 15;
        true
    }

    /// Advance by the same count the entity tick just ran.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tick(&mut self, count: u32, world: &Cluster) {
        self.tick_near(count, [1_000.0, 1_000.0], world);
    }

    /// Walk the square, then step away when `player` (x, z) is inside [`AVOID`].
    pub fn tick_near(&mut self, count: u32, player: [f32; 2], world: &Cluster) {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return;
        }
        self.steps = self.steps.wrapping_add(count);
        let [x, y, z] = path_point(self.steps);
        let dx = x - player[0];
        let dz = z - player[1];
        let dist = (dx * dx + dz * dz).sqrt();
        let goal = if dist < AVOID {
            let (ux, uz) = if dist > 1.0e-4 {
                (dx / dist, dz / dist)
            } else {
                (1.0, 0.0)
            };
            let push = AVOID - dist;
            [x + ux * push, y, z + uz * push]
        } else {
            [x, y, z]
        };
        slide_land(&mut self.origin, goal, world, 2);
    }

    /// Feet position on the grass.
    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Solid box. Taller than one block so a walk cannot step up onto it.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.45, y, z - 0.28], [x + 0.58, y + 1.15, z + 0.28])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin();
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.55, z],
            [0.45, 0.28, 0.25],
            [0.92, 0.92, 0.90],
        );
        push_box(
            &mut out,
            [x + 0.42, y + 0.72, z],
            [0.16, 0.16, 0.14],
            [0.72, 0.70, 0.66],
        );
        out
    }
}

fn path_point(steps: u32) -> [f32; 3] {
    let leg = (steps / STEPS_PER_LEG) % 4;
    let along = (steps % STEPS_PER_LEG) as f32 / STEPS_PER_LEG as f32 * LEG;
    let (x, z) = match leg {
        0 => (12.0 + along, 10.0),
        1 => (14.0, 10.0 + along),
        2 => (14.0 - along, 12.0),
        _ => (12.0, 12.0 - along),
    };
    [x, 1.0, z]
}

fn push_box(out: &mut Vec<Vertex>, center: [f32; 3], half: [f32; 3], color: [f32; 3]) {
    let min = [
        center[0] - half[0],
        center[1] - half[1],
        center[2] - half[2],
    ];
    let max = [
        center[0] + half[0],
        center[1] + half[1],
        center[2] + half[2],
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
        let tri = [face[0], face[1], face[2], face[0], face[2], face[3]];
        for index in tri {
            out.push(Vertex {
                position: corner[index],
                color,
                normal,
                uv: [0.0, 0.0],
                textured: 0.0,
                block: color,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Chunk, Cluster};

    fn open_plot() -> Cluster {
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap()
    }

    #[test]
    fn the_sheep_walks_a_square_when_the_tick_runs() {
        let plot = open_plot();
        let mut sheep = Sheep::new();
        assert_eq!(sheep.origin(), [12.0, 1.0, 10.0]);
        assert_eq!(sheep.vertices().len(), 72);
        sheep.tick(STEPS_PER_LEG, &plot);
        assert_eq!(sheep.origin(), [14.0, 1.0, 10.0]);
        sheep.tick(STEPS_PER_LEG, &plot);
        assert_eq!(sheep.origin(), [14.0, 1.0, 12.0]);
        sheep.tick(STEPS_PER_LEG, &plot);
        assert_eq!(sheep.origin(), [12.0, 1.0, 12.0]);
        sheep.tick(STEPS_PER_LEG, &plot);
        assert_eq!(sheep.origin(), [12.0, 1.0, 10.0]);
        let body = sheep.vertices();
        assert!(body.iter().any(|vertex| vertex.color[0] > 0.9));
        assert!(body.iter().any(|vertex| vertex.position[1] > 1.2));
        let (min, max) = sheep.bounds();
        assert!(max[1] - min[1] > 1.0);
        assert!(min[0] < 12.0 && max[0] > 12.0);
    }

    #[test]
    fn the_sheep_steps_away_from_the_player() {
        let plot = open_plot();
        let mut sheep = Sheep::new();
        sheep.tick_near(0, [12.0, 10.0], &plot);
        let [x, _, z] = sheep.origin();
        let dist = ((x - 12.0).powi(2) + (z - 10.0).powi(2)).sqrt();
        assert!(dist >= AVOID - 1.0e-3, "kept {dist} from the player");
        sheep.tick_near(STEPS_PER_LEG, [0.0, 0.0], &plot);
        assert_eq!(sheep.origin(), [14.0, 1.0, 10.0]);
    }
}
