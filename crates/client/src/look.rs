//! Ray from the eye through shape boxes and model planes.
//!
//! The first solid box along the look direction is the block to break. The
//! empty cell in front of the hit face is where a place would go. Reach is
//! five blocks. `forward` is a unit vector.

use glam::Vec3;
use rebuild_world::{Cluster, for_each_collider, shape_of};

const REACH: f32 = 5.0;

pub struct Target {
    pub block: [i32; 3],
    pub place: [i32; 3],
    /// The hit is a ceiling, or the upper half of a side face.
    pub upper: bool,
    /// 0, 1, or 2. The face that was hit.
    pub axis: u8,
    /// -1 or 1. From the block toward the empty cell.
    pub normal: i8,
}

/// Which way a torch sits. 0 stands on the floor. 1 to 4 hang on a wall,
/// with the support on the west, east, north, or south of the place cell.
pub fn torch_facing(axis: u8, normal: i8) -> u8 {
    painting_facing(axis, normal).unwrap_or(0)
}

/// Which wall a painting hangs on. Floor and ceiling hits hang nothing.
pub fn painting_facing(axis: u8, normal: i8) -> Option<u8> {
    match (axis, normal) {
        (0, 1) => Some(1),
        (0, -1) => Some(2),
        (2, 1) => Some(3),
        (2, -1) => Some(4),
        _ => None,
    }
}

pub fn raycast(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<Target> {
    walk(eye, forward, world).map(|(target, _)| target)
}

/// The block the look ray hits, and how far it is. Water is included.
pub fn block_along(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<(&'static str, f32)> {
    let (target, distance) = walk(eye, forward, world)?;
    let id = world
        .block_at(target.block[0], target.block[1], target.block[2])
        .id;
    Some((id, distance))
}

/// The first water or lava cell the ray enters. A cell the eye is already in is skipped.
pub fn fluid_along(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<(&'static str, f32)> {
    if forward.length_squared() < 1.0e-8 {
        return None;
    }
    let mut cell = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let step = [step_of(forward.x), step_of(forward.y), step_of(forward.z)];
    let delta = [
        if step[0] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.x).abs()
        },
        if step[1] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.y).abs()
        },
        if step[2] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.z).abs()
        },
    ];
    let mut tmax = [
        next_boundary(eye.x, cell[0], step[0], forward.x),
        next_boundary(eye.y, cell[1], step[1], forward.y),
        next_boundary(eye.z, cell[2], step[2], forward.z),
    ];
    for _ in 0..49 {
        let id = world.block_at(cell[0], cell[1], cell[2]).id;
        if is_fluid(id) {
            let min = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
            if let Some(hit) = ray_box(eye, forward, min, min + Vec3::ONE) {
                if hit.t <= REACH {
                    return Some((id, hit.t));
                }
            }
        }
        let axis = smallest(&tmax);
        if tmax[axis] > REACH {
            return None;
        }
        cell[axis] += step[axis];
        tmax[axis] += delta[axis];
    }
    None
}

/// Distance to an entity box, within reach. A box the eye is inside does not count.
pub fn ray_aabb(eye: Vec3, forward: Vec3, min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    if min[0] >= max[0] || min[1] >= max[1] || min[2] >= max[2] {
        return None;
    }
    let hit = ray_box(eye, forward, Vec3::from_array(min), Vec3::from_array(max))?;
    (hit.t <= REACH).then_some(hit.t)
}

/// The closest named thing. Air is quiet. An equal distance keeps the earlier hit.
pub fn nearest_label<'a>(hits: [Option<(&'a str, f32)>; 3]) -> Option<&'a str> {
    let mut best: Option<(&str, f32)> = None;
    for hit in hits {
        let Some((id, distance)) = hit else {
            continue;
        };
        if id.is_empty() || id == "air" {
            continue;
        }
        if best.is_none_or(|(_, so_far)| distance < so_far) {
            best = Some((id, distance));
        }
    }
    best.map(|(id, _)| id)
}

fn is_fluid(id: &str) -> bool {
    id == "water" || id == "lava"
}

fn walk(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<(Target, f32)> {
    if forward.length_squared() < 1.0e-8 {
        return None;
    }
    let mut cell = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let step = [step_of(forward.x), step_of(forward.y), step_of(forward.z)];
    let delta = [
        if step[0] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.x).abs()
        },
        if step[1] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.y).abs()
        },
        if step[2] == 0 {
            f32::INFINITY
        } else {
            (1.0 / forward.z).abs()
        },
    ];
    let mut tmax = [
        next_boundary(eye.x, cell[0], step[0], forward.x),
        next_boundary(eye.y, cell[1], step[1], forward.y),
        next_boundary(eye.z, cell[2], step[2], forward.z),
    ];

    match hit_cell(cell, eye, forward, world) {
        Walk::Hit(target, distance) => return Some((target, distance)),
        Walk::Stop => return None,
        Walk::Miss => {}
    }
    for _ in 0..48 {
        let axis = smallest(&tmax);
        if tmax[axis] > REACH {
            return None;
        }
        cell[axis] += step[axis];
        tmax[axis] += delta[axis];
        match hit_cell(cell, eye, forward, world) {
            Walk::Hit(target, distance) => return Some((target, distance)),
            Walk::Stop => return None,
            Walk::Miss => {}
        }
    }
    None
}

/// True when no solid block sits strictly between the two points.
///
/// A bite or a shot uses this so a wall stops the hit. The segment is the
/// straight line, and a block whose box is short of the end is in the way.
pub fn clear_between(from: [f32; 3], to: [f32; 3], world: &Cluster) -> bool {
    let delta = Vec3::new(to[0] - from[0], to[1] - from[1], to[2] - from[2]);
    let len = delta.length();
    if len < 0.05 {
        return true;
    }
    let forward = delta / len;
    let Some(hit) = raycast(Vec3::new(from[0], from[1], from[2]), forward, world) else {
        return true;
    };
    gap_to_cell(from, hit.block) + 0.2 >= len
}

fn gap_to_cell(from: [f32; 3], cell: [i32; 3]) -> f32 {
    let mut dist2 = 0.0;
    for axis in 0..3 {
        let point = from[axis];
        let low = cell[axis] as f32;
        let high = low + 1.0;
        let clamped = point.clamp(low, high);
        let delta = point - clamped;
        dist2 += delta * delta;
    }
    dist2.sqrt()
}

enum Walk {
    Miss,
    Stop,
    Hit(Target, f32),
}

struct BoxHit {
    t: f32,
    axis: usize,
    normal: i32,
    face: f32,
}

fn hit_cell(cell: [i32; 3], eye: Vec3, forward: Vec3, world: &Cluster) -> Walk {
    let block = world.block_at(cell[0], cell[1], cell[2]);
    let shape = shape_of(block);
    if shape.is_empty() {
        return Walk::Miss;
    }
    let origin = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
    let mut best: Option<BoxHit> = None;
    for_each_collider(block, |box_min, box_max| {
        let min = origin + Vec3::new(box_min[0], box_min[1], box_min[2]) / 16.0;
        let max = origin + Vec3::new(box_max[0], box_max[1], box_max[2]) / 16.0;
        let Some(hit) = ray_box(eye, forward, min, max) else {
            return;
        };
        if best.as_ref().is_none_or(|current| hit.t < current.t) {
            best = Some(hit);
        }
    });
    match best {
        Some(hit) if hit.t <= REACH => {
            let mut place = cell;
            place[hit.axis] = (hit.face + hit.normal as f32 * 1.0e-3).floor() as i32;
            let upper = if hit.axis == 1 {
                hit.normal < 0
            } else {
                let along = eye.y + forward.y * hit.t;
                along - cell[1] as f32 >= 0.5
            };
            Walk::Hit(
                Target {
                    block: cell,
                    place,
                    upper,
                    axis: hit.axis as u8,
                    normal: hit.normal as i8,
                },
                hit.t,
            )
        }
        Some(_) => Walk::Stop,
        None => Walk::Miss,
    }
}

fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<BoxHit> {
    let mut t_enter = 0.0_f32;
    let mut t_exit = f32::INFINITY;
    let mut axis = 0usize;
    let mut normal = 0i32;
    let mut face = 0.0_f32;
    for i in 0..3 {
        let delta = dir[i];
        let start = origin[i];
        if delta.abs() < 1.0e-8 {
            if start < min[i] || start > max[i] {
                return None;
            }
            continue;
        }
        let mut t0 = (min[i] - start) / delta;
        let mut t1 = (max[i] - start) / delta;
        let mut near_normal = -1;
        let mut near_face = min[i];
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
            near_normal = 1;
            near_face = max[i];
        }
        if t0 > t_enter {
            t_enter = t0;
            axis = i;
            normal = near_normal;
            face = near_face;
        }
        if t1 < t_exit {
            t_exit = t1;
        }
        if t_enter > t_exit {
            return None;
        }
    }
    if normal == 0 || t_enter < 0.0 || t_enter > t_exit {
        return None;
    }
    Some(BoxHit {
        t: t_enter,
        axis,
        normal,
        face,
    })
}

fn step_of(delta: f32) -> i32 {
    if delta > 0.0 {
        1
    } else if delta < 0.0 {
        -1
    } else {
        0
    }
}

fn next_boundary(origin: f32, cell: i32, step: i32, dir: f32) -> f32 {
    if step == 0 {
        f32::INFINITY
    } else if step > 0 {
        (cell as f32 + 1.0 - origin) / dir
    } else {
        (cell as f32 - origin) / dir
    }
}

fn smallest(values: &[f32; 3]) -> usize {
    if values[0] <= values[1] && values[0] <= values[2] {
        0
    } else if values[1] <= values[2] {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use rebuild_world::{Block, Chunk, Cluster, Edit, OAK_STAIRS, PlayerBox, apply};

    use crate::body::{self, EYE_HEIGHT, HALF_WIDTH};
    use crate::camera::{Camera, MoveInput};
    use crate::mesh;

    #[test]
    fn ray_hits_a_cube_and_the_empty_cell_in_front() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let hit = raycast(Vec3::new(0.5, 0.5, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert_eq!(hit.block, [0, 0, 0]);
        assert_eq!(hit.place, [0, 0, 1]);
    }

    #[test]
    fn a_ceiling_hit_chooses_the_upper_half() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 2, 0, Block::new("stone", 0))]).unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let ceiling = raycast(Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0), &world).unwrap();
        assert_eq!(ceiling.place, [0, 1, 0]);
        assert!(ceiling.upper);
        let floor = raycast(Vec3::new(0.5, 3.5, 0.5), Vec3::new(0.0, -1.0, 0.0), &world).unwrap();
        assert_eq!(floor.place, [0, 3, 0]);
        assert!(!floor.upper);
        let high = raycast(Vec3::new(0.5, 2.75, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert!(high.upper);
        let low = raycast(Vec3::new(0.5, 2.25, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert!(!low.upper);
    }

    #[test]
    fn a_top_hit_stands_a_torch_and_a_side_hit_hangs_it() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("stone", 0))]).unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let top = raycast(Vec3::new(0.5, 3.5, 0.5), Vec3::new(0.0, -1.0, 0.0), &world).unwrap();
        assert_eq!(top.place, [0, 2, 0]);
        assert_eq!(torch_facing(top.axis, top.normal), 0);
        let side = raycast(Vec3::new(0.5, 1.5, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert_eq!(side.place, [0, 1, 1]);
        assert_eq!(torch_facing(side.axis, side.normal), 3);
        assert_eq!(painting_facing(top.axis, top.normal), None);
        assert_eq!(painting_facing(side.axis, side.normal), Some(3));
    }

    #[test]
    fn ray_hits_a_stair_box() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new(OAK_STAIRS, 0))]).unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let lower = raycast(Vec3::new(0.5, 1.25, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert_eq!(lower.block, [0, 1, 0]);
        assert_eq!(lower.place, [0, 1, 1]);

        let upper = raycast(Vec3::new(0.5, 1.75, 3.0), Vec3::new(0.0, 0.0, -1.0), &world).unwrap();
        assert_eq!(upper.block, [0, 1, 0]);
    }

    #[test]
    fn a_wall_blocks_the_line_and_open_air_does_not() {
        let open = Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap();
        let from = [0.5, 1.5, 0.5];
        let to = [3.5, 1.5, 0.5];
        assert!(clear_between(from, to, &open));
        let wall = Chunk::from_blocks(0, 0, [(2, 1, 0, Block::new("stone", 0))]).unwrap();
        let blocked = Cluster::try_from_chunks(0, 0, [wall]).unwrap();
        assert!(!clear_between(from, to, &blocked));
        assert!(clear_between(from, [1.5, 1.5, 0.5], &blocked));
    }

    #[test]
    fn break_and_place_change_the_mesh_and_collision() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (0, 0, 1, Block::new("stone", 0)),
                (0, 0, 2, Block::new("stone", 0)),
                (1, 2, 1, Block::new("cobblestone", 0)),
            ],
        )
        .unwrap();
        let mut world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let before = mesh::mesh_cluster(&world).vertices.len();
        assert!(apply(&mut world, Edit::Break { x: 1, y: 2, z: 1 }));
        assert!(mesh::mesh_cluster(&world).vertices.len() < before);
        assert!(world.block_at(1, 2, 1).is_air());

        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.5);
        let (min, max) = body::player_box_bounds(eye);
        assert!(apply(
            &mut world,
            Edit::Place {
                x: 0,
                y: 1,
                z: 1,
                block: Block::new("cobblestone", 0),
                player: PlayerBox { min, max },
            }
        ));
        assert!(!mesh::mesh_cluster(&world).vertices.is_empty());

        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        let mut walk = MoveInput {
            forward: true,
            back: false,
            left: false,
            right: false,
            jump: false,
            fast: false,
            crouch: false,
        };
        for _ in 0..16 {
            body::step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let feet = camera.eye.y - EYE_HEIGHT;
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the placed block, feet {feet}"
        );
        assert!(
            camera.eye.z < 2.0,
            "stopped at the placed block instead of stepping up, z {}",
            camera.eye.z
        );

        assert!(apply(&mut world, Edit::Break { x: 0, y: 1, z: 1 }));
        walk.forward = true;
        for _ in 0..90 {
            body::step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        assert!(
            camera.eye.z - HALF_WIDTH < 1.0,
            "break did not clear the block"
        );
    }

    #[test]
    fn water_is_named_and_a_closer_body_wins() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 2, Block::new("water", 0)),
                (0, 1, 0, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let eye = Vec3::new(0.5, 1.5, 4.0);
        let forward = Vec3::new(0.0, 0.0, -1.0);
        let fluid = fluid_along(eye, forward, &world).expect("water");
        assert_eq!(fluid.0, "water");
        assert!((fluid.1 - 1.0).abs() < 0.05, "distance {}", fluid.1);
        let block = block_along(eye, forward, &world).expect("block");
        assert_eq!(block.0, "water");
        assert_eq!(
            nearest_label([Some(("pig", 0.4)), Some(fluid), Some(block)]),
            Some("pig")
        );
        assert_eq!(
            nearest_label([Some(("pig", 4.0)), None, Some(("stone", 1.2))]),
            Some("stone")
        );
        assert_eq!(
            nearest_label([None, Some(("water", 1.0)), Some(("stone", 3.0))]),
            Some("water")
        );
        assert_eq!(nearest_label([None, None, Some(("air", 0.2))]), None);
        let body = ray_aabb(eye, forward, [0.1, 1.1, 3.2], [0.9, 2.0, 3.6]).expect("body");
        assert!(body < fluid.1);
    }
}
