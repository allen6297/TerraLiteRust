//! Greedy mesh of loaded chunks.
//!
//! Full cubes still merge exposed faces of the same id and metadata. A face
//! is dropped when the neighbor's shape covers that whole side, including a
//! neighbor in the next chunk. Shapes, models, and lights come from the pack
//! registries. A smaller shape is meshed from the boxes listed there. A model
//! plane is one quad. A mesh cube that fills the cell stays on the greedy
//! path, and its tilted faces (grass blades) are extra quads in the same
//! biome color. An id that was not registered does not mesh.
//! Colored light from the pack tints a face and falls off per cell, so two
//! faces merge only when that light matches. Bounce is not part of that key:
//! a sun step would otherwise retile the grass into new triangles, and those
//! triangles read as jagged shadows while the cascades catch up. A block that names a PNG, and
//! whose file is present, carries that texture's atlas slot. Other faces stay flat
//! color. The sun's brightness and direction are applied when drawing. Cast
//! shadows are not stored on these vertices; the renderer reads them from a
//! shadow map per texel. Each face stores
//! one bounce of light gathered from nearby voxels, scaled by the sun or moon
//! when drawing. Each corner stores how open the sky is from that point. A few
//! rays leave the face, so a cave and the side of a wall go darker. That shade
//! fades across the face. It is not the cast shadow; the shadow map draws those.
//! A colored block light is not dimmed by the corner. The lit side of
//! the caster stays bright. Empty chunks are not meshed. Past the full-detail
//! ring, a 2-block mesh covers the middle distance and a 4-block mesh covers
//! the rest, so a far rebuild stays cheap on a large world. The blocks of a
//! coarse chunk that touch a finer chunk are meshed at that finer step, so
//! the shared edge keeps the real height instead of a coarse cube. The face
//! between that strip and the coarse interior is split to the same step, and
//! a skirt sits just inside the coarse top so a t-junction does not open a
//! crack. Walking away holds
//! the finer mesh for one extra chunk. A block edit rebakes that chunk and the
//! chunks that share its face, not the square. A sun step rebakes the full
//! detail ring, because a coarse mesh does not store the sun.
//! `mesh_chunk` has no neighbors outside the chunk, so outer faces stay visible.
//! The frame thread does not bake. It copies the chunk, its neighbors, and the
//! light field, and a worker runs the remesh, including bounce rays, on that
//! copy. An edit made while the worker runs bumps the epoch so the stale mesh
//! is dropped.

use std::collections::{HashMap, HashSet};

use rebuild_world::{
    Block, CellBox, Chunk, Cluster, Emitter, FILL_EMBER, FILL_WATER, LIGHT_REACH, LightField, Pack,
    Plane, Shape, buffer_level, buffer_max, compound_accepts, conduit_edge_tube, conduit_fill_span,
    conduit_links, conduit_shell_boxes, conduit_span, is_buffer, is_drain, is_lamp, is_spout,
    is_tank, lamp_on, pump_dir, pump_on, shape_of, tank_level,
};

use crate::daylight;
use crate::palette;
use crate::texture::texture_slot;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// `slot + 1` samples that column of the pack atlas. `0` stays flat.
    pub textured: f32,
    /// Albedo times the block-light channels. The sun does not scale this.
    pub block: [f32; 3],
    /// One bounce off nearby voxels, times this face's color. Drawing scales
    /// it by the sun or moon brightness. Block light is not in here.
    pub bounce: [f32; 3],
    /// `1` when this corner sees the sky, lower in a cave or against a wall.
    pub ao: f32,
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    #[cfg(test)]
    fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

/// Dim floor so a colored light can tint the faces that reach it.
const AMBIENT: f32 = 0.2;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    block: Block,
    light: [u8; 3],
    /// One-bounce color in steps of 1/32.
    bounce: [u8; 3],
}

impl Stamp {
    const EMPTY: Self = Self {
        block: Block::AIR,
        light: [0, 0, 0],
        bounce: [0, 0, 0],
    };

    fn is_empty(self) -> bool {
        self.block.is_air()
    }

    /// Block and block light. Bounce is left out so a daylight step does not
    /// split a ground quad along a new diagonal.
    fn same_surface(self, other: Self) -> bool {
        self.block == other.block && self.light == other.light
    }
}

#[cfg(test)]
pub fn mesh_chunk(chunk: &Chunk) -> Mesh {
    let origin_x = chunk.x() * 16;
    let origin_z = chunk.z() * 16;
    let sample = |x: i32, y: i32, z: i32| {
        let local_x = x - origin_x;
        let local_z = z - origin_z;
        if (0..16).contains(&local_x) && (0..256).contains(&y) && (0..16).contains(&local_z) {
            chunk.block(local_x as u8, y as u16, local_z as u8)
        } else {
            Block::AIR
        }
    };
    let light = LightField::solve_bounds(origin_x, origin_x + 16, origin_z, origin_z + 16, &sample);
    mesh_chunk_with(chunk, &sample, &light, daylight::NOON_SUN)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn mesh_cluster(cluster: &Cluster) -> Mesh {
    mesh_world(cluster, daylight::NOON_SUN)
}

/// Mesh `cluster` with cast shadows falling away from `sun`.
///
/// Every solid chunk is full detail. The window uses [`mesh_around`] so chunks
/// far from the camera stay coarse.
pub fn mesh_world(cluster: &Cluster, sun: [f32; 3]) -> Mesh {
    let mut job = MeshJob::start(cluster, sun, None);
    let _ = job.pump(cluster, std::time::Duration::from_secs(60));
    job.into_mesh()
}

/// Full detail near `focus` (a chunk coordinate) and a coarse mesh past that.
#[cfg_attr(not(test), allow(dead_code))]
pub fn mesh_around(cluster: &Cluster, sun: [f32; 3], focus: [i32; 2]) -> Mesh {
    let mut job = MeshJob::start(cluster, sun, Some(focus));
    let _ = job.pump(cluster, std::time::Duration::from_secs(60));
    job.into_mesh()
}

/// Chunks this many steps from the camera, in chunk coordinates, stay full detail.
const LOD_FULL: i32 = 2;
/// Through this distance the mesh is 2-block cells. Past it, cells are 4 blocks.
const LOD_MID: i32 = 6;
const LOD_STEP_MID: i32 = 2;
const LOD_STEP_FAR: i32 = 4;
/// Extra chunks a finer ring keeps when the camera walks away from it.
const LOD_HOLD: i32 = 1;
/// A skirt sits this far inside the coarse cell, so it only shows through a crack.
const LOD_SKIRT: f32 = 0.03;

/// One sun or moon direction, baked a chunk at a time.
///
/// Empty chunks are skipped. Each [`MeshJob::pump`] stops after its time
/// budget, so a shadow step does not rebuild the whole world in one frame.
/// `focus` is the camera's chunk. `None` keeps every chunk at full detail.
pub struct MeshJob {
    sun: [f32; 3],
    focus: Option<[i32; 2]>,
    /// Focus the mesh on screen was built for. Walking away holds finer detail.
    previous: Option<[i32; 2]>,
    light: LightField,
    chunks: Vec<usize>,
    cursor: usize,
    mesh: Mesh,
}

impl MeshJob {
    pub fn start(cluster: &Cluster, sun: [f32; 3], focus: Option<[i32; 2]>) -> Self {
        Self::resume(cluster, sun, focus, None)
    }

    /// `previous` is the focus already on screen. A chunk that just fell out of
    /// a finer ring stays there until the camera is another chunk farther.
    pub fn resume(
        cluster: &Cluster,
        sun: [f32; 3],
        focus: Option<[i32; 2]>,
        previous: Option<[i32; 2]>,
    ) -> Self {
        let chunks = cluster
            .chunks()
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.non_air_count() > 0)
            .map(|(index, _)| index)
            .collect();
        Self {
            sun,
            focus,
            previous,
            light: LightField::solve(cluster),
            chunks,
            cursor: 0,
            mesh: Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
            },
        }
    }

    /// Bakes the next solid chunks until `budget` has passed. One chunk is
    /// always finished once it has started, then the clock stops the rest.
    /// Returns whether every solid chunk is in the mesh.
    pub fn pump(&mut self, cluster: &Cluster, budget: std::time::Duration) -> bool {
        self.pump_limited(cluster, budget, usize::MAX)
    }

    /// Like [`pump`](Self::pump), and also stops after `max_chunks` even when
    /// the budget has time left. The window uses this so a cheap far chunk
    /// does not pull the rest of the square into the same frame.
    pub fn pump_limited(
        &mut self,
        cluster: &Cluster,
        budget: std::time::Duration,
        max_chunks: usize,
    ) -> bool {
        if self.cursor >= self.chunks.len() || max_chunks == 0 {
            return self.cursor >= self.chunks.len();
        }
        let started = std::time::Instant::now();
        let mut baked = 0usize;
        while self.cursor < self.chunks.len() {
            let chunk = &cluster.chunks()[self.chunks[self.cursor]];
            let sample = |x, y, z| cluster.block_at(x, y, z);
            let part = if let Some(step) = chunk_lod_step(chunk, self.focus, self.previous) {
                let focus = self.focus.unwrap_or([chunk.x(), chunk.z()]);
                let borders = borders_of(
                    chunk.x(),
                    chunk.z(),
                    step,
                    focus,
                    self.previous,
                    |nx, nz| {
                        cluster
                            .get(nx, nz)
                            .is_some_and(|neighbor| neighbor.non_air_count() > 0)
                    },
                );
                mesh_chunk_lod(chunk, &sample, &self.light, step, borders)
            } else {
                mesh_chunk_with(chunk, sample, &self.light, self.sun)
            };
            let base = self.mesh.vertices.len() as u32;
            self.mesh.vertices.extend(part.vertices);
            self.mesh
                .indices
                .extend(part.indices.into_iter().map(|index| index + base));
            self.cursor += 1;
            baked += 1;
            if baked >= max_chunks || started.elapsed() >= budget {
                break;
            }
        }
        self.cursor >= self.chunks.len()
    }

    pub fn into_mesh(self) -> Mesh {
        self.mesh
    }
}

/// One chunk after a bake. An empty mesh means the chunk left the world.
pub struct BakedChunk {
    pub x: i32,
    pub z: i32,
    pub mesh: Mesh,
}

/// Per-chunk meshes. The window uploads a chunk when it changes and leaves the rest.
///
/// [`ChunkBake::pump`] stops after its chunk cap, so a break, a new terrain
/// chunk, or a sun step cannot rebuild the square in one frame. LOD matches
/// [`chunk_lod_step`]: full detail near the camera, then 2-block and 4-block cells.
/// [`ChunkBake::take_work`] is the same queue for a worker. The worker bakes a
/// snapshot, and [`ChunkBake::accept`] keeps the mesh only when the chunk was
/// not edited again.
pub struct ChunkBake {
    sun: [f32; 3],
    focus: [i32; 2],
    previous: Option<[i32; 2]>,
    light: LightField,
    light_dirty: bool,
    /// Held torch / lantern / glow brick — same field as placed block light.
    held: Option<Emitter>,
    baked: HashSet<(i32, i32)>,
    pending: Vec<(i32, i32)>,
    queued: HashSet<(i32, i32)>,
    urgent: HashSet<(i32, i32)>,
    /// Chunks a worker is baking. A newer edit bumps [`Self::epochs`].
    inflight: HashSet<(i32, i32)>,
    epochs: HashMap<(i32, i32), u64>,
    /// Empty chunks resolved on the frame thread. Nothing to send a worker.
    ready: Vec<BakedChunk>,
}

impl ChunkBake {
    pub fn new(cluster: &Cluster, sun: [f32; 3], focus: [i32; 2]) -> Self {
        Self {
            sun,
            focus,
            previous: None,
            light: LightField::solve(cluster),
            light_dirty: false,
            held: None,
            baked: HashSet::new(),
            pending: Vec::new(),
            queued: HashSet::new(),
            urgent: HashSet::new(),
            inflight: HashSet::new(),
            epochs: HashMap::new(),
            ready: Vec::new(),
        }
    }

    fn resolve_light(&self, cluster: &Cluster) -> LightField {
        match self.held {
            Some(extra) => LightField::solve_with(cluster, &[extra]),
            None => LightField::solve(cluster),
        }
    }

    /// Move or clear the held light. Remeshes chunks that the old or new glow reaches.
    pub fn set_held(&mut self, held: Option<Emitter>) {
        if self.held == held {
            return;
        }
        if let Some(old) = self.held {
            self.touch_light_reach(old.x, old.z);
        }
        self.held = held;
        self.light_dirty = true;
        if let Some(next) = held {
            self.touch_light_reach(next.x, next.z);
        }
    }

    /// Chunks a level-15 emitter can still touch from `(x, z)`.
    fn touch_light_reach(&mut self, x: i32, z: i32) {
        self.light_dirty = true;
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        let span = (LIGHT_REACH + 15) / 16;
        for dz in -span..=span {
            for dx in -span..=span {
                self.enqueue(cx + dx, cz + dz, true);
            }
        }
    }

    pub fn focus(&self) -> [i32; 2] {
        self.focus
    }

    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_urgent(&self) -> bool {
        !self.urgent.is_empty()
    }

    /// True when nothing is waiting on a worker or the frame queue.
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.inflight.is_empty() && !self.light_dirty
    }

    /// The edited cell, plus a neighbor chunk when the cell is on that shared face.
    pub fn touch(&mut self, x: i32, z: i32) {
        self.light_dirty = true;
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        self.enqueue(cx, cz, true);
        let local_x = x.rem_euclid(16);
        let local_z = z.rem_euclid(16);
        if local_x == 0 {
            self.enqueue(cx - 1, cz, true);
        }
        if local_x == 15 {
            self.enqueue(cx + 1, cz, true);
        }
        if local_z == 0 {
            self.enqueue(cx, cz - 1, true);
        }
        if local_z == 15 {
            self.enqueue(cx, cz + 1, true);
        }
    }

    /// Solid chunks that have never been meshed. Terrain uses this so a finished
    /// square does not rebake the ground that is already on screen.
    pub fn queue_unmeshed(&mut self, cluster: &Cluster) {
        for chunk in cluster.chunks() {
            if chunk.non_air_count() == 0 {
                continue;
            }
            let key = (chunk.x(), chunk.z());
            if self.baked.contains(&key) || self.inflight.contains(&key) {
                continue;
            }
            self.enqueue(chunk.x(), chunk.z(), false);
        }
    }

    /// The next chunk for a worker, or `None` when the queue is empty.
    /// Empty slots are returned from [`Self::drain_ready`] instead.
    pub fn take_work(&mut self, cluster: &Cluster) -> Option<MeshWork> {
        if self.light_dirty {
            self.light = self.resolve_light(cluster);
            self.light_dirty = false;
        }
        while let Some((x, z)) = self.take_next() {
            let Some(chunk) = cluster.get(x, z).filter(|chunk| chunk.non_air_count() > 0) else {
                if self.baked.remove(&(x, z)) {
                    self.ready.push(BakedChunk {
                        x,
                        z,
                        mesh: Mesh {
                            vertices: Vec::new(),
                            indices: Vec::new(),
                        },
                    });
                }
                continue;
            };
            self.inflight.insert((x, z));
            let epoch = self.epochs.get(&(x, z)).copied().unwrap_or(0);
            let step = chunk_lod_step(chunk, Some(self.focus), self.previous);
            let borders = borders_of(
                x,
                z,
                step.unwrap_or(1),
                self.focus,
                self.previous,
                |nx, nz| {
                    cluster
                        .get(nx, nz)
                        .is_some_and(|neighbor| neighbor.non_air_count() > 0)
                },
            );
            let mut neighbors = Vec::new();
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if let Some(neighbor) = cluster.get(x + dx, z + dz) {
                        neighbors.push(neighbor.clone());
                    }
                }
            }
            return Some(MeshWork {
                job_epoch: 0,
                epoch,
                x,
                z,
                chunk: chunk.clone(),
                neighbors,
                light: self.light.clone(),
                sun: self.sun,
                step,
                borders,
            });
        }
        None
    }

    /// Meshes that were empty and did not need a worker.
    pub fn drain_ready(&mut self) -> Vec<BakedChunk> {
        std::mem::take(&mut self.ready)
    }

    /// Keep a worker mesh when `epoch` is still current. A newer edit drops it.
    pub fn accept(&mut self, epoch: u64, baked: BakedChunk) -> Option<BakedChunk> {
        self.inflight.remove(&(baked.x, baked.z));
        let current = self.epochs.get(&(baked.x, baked.z)).copied().unwrap_or(0);
        if current != epoch {
            return None;
        }
        if baked.mesh.indices.is_empty() {
            self.baked.remove(&(baked.x, baked.z));
        } else {
            self.baked.insert((baked.x, baked.z));
        }
        Some(baked)
    }

    /// Queue chunks whose LOD changed, and full-detail chunks when the sun moved.
    /// A coarse chunk ignores the sun, so a daylight step leaves it alone.
    pub fn retarget(&mut self, cluster: &Cluster, sun: [f32; 3], focus: [i32; 2]) {
        let sun_moved = sun != self.sun;
        let focus_moved = focus != self.focus;
        if !sun_moved && !focus_moved {
            return;
        }
        let old_focus = self.focus;
        let old_previous = self.previous;
        if focus_moved {
            self.previous = Some(self.focus);
            self.focus = focus;
        }
        if sun_moved {
            self.sun = sun;
        }
        for chunk in cluster.chunks() {
            if chunk.non_air_count() == 0 {
                continue;
            }
            if !self.baked.contains(&(chunk.x(), chunk.z())) {
                continue;
            }
            let old_step = chunk_lod_step(chunk, Some(old_focus), old_previous);
            let new_step = chunk_lod_step(chunk, Some(self.focus), self.previous);
            let sun_hits = sun_moved && (old_step.is_none() || new_step.is_none());
            if old_step != new_step || sun_hits {
                self.enqueue(chunk.x(), chunk.z(), false);
            }
        }
    }

    /// Bakes at most `max_chunks`. One chunk finishes once started, then the
    /// budget can stop the rest. An edit is baked before new ground.
    /// Tests use this on the calling thread. The window sends [`MeshWork`] instead.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pump(
        &mut self,
        cluster: &Cluster,
        budget: std::time::Duration,
        max_chunks: usize,
    ) -> Vec<BakedChunk> {
        if max_chunks == 0 || self.pending.is_empty() {
            return Vec::new();
        }
        if self.light_dirty {
            self.light = self.resolve_light(cluster);
            self.light_dirty = false;
        }
        let started = std::time::Instant::now();
        let mut baked = 0usize;
        let mut out = Vec::new();
        while !self.pending.is_empty() {
            let Some((x, z)) = self.take_next() else {
                break;
            };
            let Some(chunk) = cluster.get(x, z).filter(|chunk| chunk.non_air_count() > 0) else {
                if self.baked.remove(&(x, z)) {
                    out.push(BakedChunk {
                        x,
                        z,
                        mesh: Mesh {
                            vertices: Vec::new(),
                            indices: Vec::new(),
                        },
                    });
                }
                baked += 1;
                if baked >= max_chunks || started.elapsed() >= budget {
                    break;
                }
                continue;
            };
            let sample = |sx, sy, sz| cluster.block_at(sx, sy, sz);
            let mesh = if let Some(step) = chunk_lod_step(chunk, Some(self.focus), self.previous) {
                let borders = borders_of(
                    chunk.x(),
                    chunk.z(),
                    step,
                    self.focus,
                    self.previous,
                    |nx, nz| {
                        cluster
                            .get(nx, nz)
                            .is_some_and(|neighbor| neighbor.non_air_count() > 0)
                    },
                );
                mesh_chunk_lod(chunk, &sample, &self.light, step, borders)
            } else {
                mesh_chunk_with(chunk, sample, &self.light, self.sun)
            };
            self.baked.insert((x, z));
            out.push(BakedChunk { x, z, mesh });
            baked += 1;
            if baked >= max_chunks || started.elapsed() >= budget {
                break;
            }
        }
        out
    }

    fn enqueue(&mut self, x: i32, z: i32, urgent: bool) {
        let key = (x, z);
        if self.inflight.contains(&key) {
            let slot = self.epochs.entry(key).or_insert(0);
            *slot = slot.wrapping_add(1);
        }
        if urgent {
            self.urgent.insert(key);
        }
        if self.queued.insert(key) {
            self.pending.push(key);
        }
    }

    fn take_next(&mut self) -> Option<(i32, i32)> {
        let focus = self.focus;
        let urgent_only = !self.urgent.is_empty();
        let index = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, key)| !urgent_only || self.urgent.contains(*key))
            .min_by_key(|(_, key)| (key.0 - focus[0]).abs().max((key.1 - focus[1]).abs()))
            .map(|(index, _)| index)?;
        let key = self.pending.swap_remove(index);
        self.queued.remove(&key);
        self.urgent.remove(&key);
        Some(key)
    }
}

/// A chunk copied for a worker. The world on the frame thread can change.
pub struct MeshWork {
    pub job_epoch: u64,
    epoch: u64,
    x: i32,
    z: i32,
    chunk: Chunk,
    neighbors: Vec<Chunk>,
    light: LightField,
    sun: [f32; 3],
    step: Option<i32>,
    /// Neighbor LOD step on -X, +X, -Z, +Z. `1` is full detail.
    borders: [i32; 4],
}

/// What the worker returns. `epoch` is the chunk generation it baked.
pub struct MeshDone {
    pub job_epoch: u64,
    pub epoch: u64,
    pub baked: BakedChunk,
}

impl MeshWork {
    /// Remesh this snapshot, including bounce rays on a full-detail chunk.
    pub fn bake(self) -> MeshDone {
        let MeshWork {
            job_epoch,
            epoch,
            x,
            z,
            chunk,
            neighbors,
            light,
            sun,
            step,
            borders,
        } = self;
        let sample = |sx, sy, sz| copied_block(&neighbors, sx, sy, sz);
        let mesh = if let Some(step) = step {
            mesh_chunk_lod(&chunk, &sample, &light, step, borders)
        } else {
            mesh_chunk_with(&chunk, &sample, &light, sun)
        };
        MeshDone {
            job_epoch,
            epoch,
            baked: BakedChunk { x, z, mesh },
        }
    }
}

fn copied_block(chunks: &[Chunk], x: i32, y: i32, z: i32) -> Block {
    if !(0..256).contains(&y) {
        return Block::AIR;
    }
    let cx = x.div_euclid(16);
    let cz = z.div_euclid(16);
    let Some(chunk) = chunks
        .iter()
        .find(|chunk| chunk.x() == cx && chunk.z() == cz)
    else {
        return Block::AIR;
    };
    chunk.block(x.rem_euclid(16) as u8, y as u16, z.rem_euclid(16) as u8)
}

fn mesh_chunk_with(
    chunk: &Chunk,
    sample: impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
) -> Mesh {
    let dir = unit(sun);
    let mut mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [false, true] {
            mesh_direction(chunk, axis, positive, &sample, light, dir, &mut mesh);
        }
    }
    mesh_partial_shapes(chunk, &sample, light, dir, &mut mesh);
    mesh
}

/// `None` is full detail. `Some(step)` is a coarse cell of that many blocks.
fn chunk_lod_step(
    chunk: &Chunk,
    focus: Option<[i32; 2]>,
    previous: Option<[i32; 2]>,
) -> Option<i32> {
    let Some(focus) = focus else {
        return None;
    };
    let step = lod_step_of(chunk.x(), chunk.z(), focus, previous);
    if step == 1 { None } else { Some(step) }
}

/// `1` is full detail. A retreat holds the finer ring for one extra chunk.
fn lod_step_of(x: i32, z: i32, focus: [i32; 2], previous: Option<[i32; 2]>) -> i32 {
    let dist = (x - focus[0]).abs().max((z - focus[1]).abs());
    let retreat = previous
        .map(|prev| dist > (x - prev[0]).abs().max((z - prev[1]).abs()))
        .unwrap_or(false);
    let hold = if retreat { LOD_HOLD } else { 0 };
    if dist <= LOD_FULL + hold {
        1
    } else if dist <= LOD_MID + hold {
        LOD_STEP_MID
    } else {
        LOD_STEP_FAR
    }
}

/// Neighbor step on -X, +X, -Z, +Z. A missing chunk keeps `step`, so no strip.
fn borders_of(
    x: i32,
    z: i32,
    step: i32,
    focus: [i32; 2],
    previous: Option<[i32; 2]>,
    present: impl Fn(i32, i32) -> bool,
) -> [i32; 4] {
    let neighbors = [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)];
    let mut borders = [step; 4];
    for (index, (nx, nz)) in neighbors.into_iter().enumerate() {
        if present(nx, nz) {
            borders[index] = lod_step_of(nx, nz, focus, previous);
        }
    }
    borders
}

/// A coarse mesh. No bounce rays. The cell keeps the full cube that occurs most.
/// `borders` is the neighbor step on each side. A finer neighbor keeps a strip
/// of real blocks so the chunk edge does not jump to the coarse grid.
fn mesh_chunk_lod(
    chunk: &Chunk,
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    step: i32,
    borders: [i32; 4],
) -> Mesh {
    let mut mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    let origin = [chunk.x() as f32 * 16.0, 0.0, chunk.z() as f32 * 16.0];
    let occupied = coarse_blocks(chunk, step);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [false, true] {
            mesh_lod_direction(
                chunk, axis, positive, step, borders, &occupied, sample, light, origin, &mut mesh,
            );
        }
    }
    mesh_lod_strips(chunk, step, borders, sample, light, origin, &mut mesh);
    mesh
}

fn coarse_blocks(chunk: &Chunk, step: i32) -> Vec<Block> {
    let n = lod_count(step);
    let mut occupied = vec![Block::AIR; n.0 * n.1 * n.2];
    for cz in 0..n.2 {
        for cy in 0..n.1 {
            for cx in 0..n.0 {
                occupied[lod_index(cx, cy, cz, step)] = coarse_block(chunk, cx, cy, cz, step);
            }
        }
    }
    occupied
}

fn lod_count(step: i32) -> (usize, usize, usize) {
    let step = step as usize;
    (16 / step, 256 / step, 16 / step)
}

fn lod_index(cx: usize, cy: usize, cz: usize, step: i32) -> usize {
    let (nx, _, nz) = lod_count(step);
    (cy * nz + cz) * nx + cx
}

fn coarse_block(chunk: &Chunk, cx: usize, cy: usize, cz: usize, step: i32) -> Block {
    let x0 = cx as i32 * step;
    let y0 = cy as i32 * step;
    let z0 = cz as i32 * step;
    let mut tally: Vec<(Block, i32, i32)> = Vec::new();
    for z in z0..z0 + step {
        for y in y0..y0 + step {
            for x in x0..x0 + step {
                let block = chunk.block(x as u8, y as u16, z as u8);
                if !shape_of(block).is_full_cube() {
                    continue;
                }
                if let Some(entry) = tally.iter_mut().find(|entry| entry.0 == block) {
                    entry.1 += 1;
                    entry.2 = entry.2.max(y);
                } else {
                    tally.push((block, 1, y));
                }
            }
        }
    }
    let mut found = Block::AIR;
    let mut best_count = 0;
    let mut best_top = -1;
    for (block, count, top) in tally {
        if count > best_count || (count == best_count && top > best_top) {
            found = block;
            best_count = count;
            best_top = top;
        }
    }
    found
}

fn mesh_lod_direction(
    chunk: &Chunk,
    axis: Axis,
    positive: bool,
    step: i32,
    borders: [i32; 4],
    occupied: &[Block],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    let (nx, ny, nz) = lod_count(step);
    let (u_len, v_len, slices) = match axis {
        Axis::X => (nz, ny, nx),
        Axis::Y => (nx, nz, ny),
        Axis::Z => (nx, ny, nz),
    };
    let scale = step as f32;
    let mut mask = vec![Stamp::EMPTY; u_len * v_len];
    for slice in 0..slices {
        mask.fill(Stamp::EMPTY);
        for v in 0..v_len {
            for u in 0..u_len {
                let (cx, cy, cz) = coarse_coords(axis, slice, u, v);
                if !lod_interior(cx, cz, nx, nz, step, borders) {
                    continue;
                }
                let block = occupied[lod_index(cx, cy, cz, step)];
                if block.is_air() {
                    continue;
                }
                if let Some(sub) = strip_beside(axis, positive, cx, cz, nx, nz, step, borders) {
                    emit_split_face(
                        chunk, axis, positive, step, sub, cx, cy, cz, block, sample, light, origin,
                        mesh,
                    );
                    continue;
                }
                if !coarse_face_open(chunk, occupied, axis, positive, step, cx, cy, cz, sample) {
                    continue;
                }
                let stamp = face_stamp(axis, positive, slice, u, v, origin, scale, block, light);
                // A top that touches the strip stays one cell wide. Merging it
                // would drop the vertices the skirt hangs from.
                if axis == Axis::Y && positive && touches_strip(cx, cz, nx, nz, step, borders) {
                    emit_quad(
                        axis, positive, slice, u, v, 1, 1, stamp, origin, scale, sample, mesh,
                    );
                    emit_border_skirts(cx, cy, cz, nx, nz, step, borders, stamp, origin, mesh);
                    continue;
                }
                mask[v * u_len + u] = stamp;
            }
        }
        greedy(
            axis, positive, slice, u_len, v_len, origin, scale, sample, &mut mask, mesh,
        );
    }
}

fn coarse_coords(axis: Axis, slice: usize, u: usize, v: usize) -> (usize, usize, usize) {
    match axis {
        Axis::X => (slice, v, u),
        Axis::Y => (u, slice, v),
        Axis::Z => (u, v, slice),
    }
}

fn coarse_face_open(
    chunk: &Chunk,
    occupied: &[Block],
    axis: Axis,
    positive: bool,
    cell: i32,
    cx: usize,
    cy: usize,
    cz: usize,
    sample: &impl Fn(i32, i32, i32) -> Block,
) -> bool {
    let step = if positive { 1isize } else { -1 };
    let (nx, ny, nz) = lod_count(cell);
    let (x, y, z) = (cx as isize, cy as isize, cz as isize);
    let (sx, sy, sz) = match axis {
        Axis::X => (x + step, y, z),
        Axis::Y => (x, y + step, z),
        Axis::Z => (x, y, z + step),
    };
    if (0..nx as isize).contains(&sx)
        && (0..ny as isize).contains(&sy)
        && (0..nz as isize).contains(&sz)
    {
        return occupied[lod_index(sx as usize, sy as usize, sz as usize, cell)].is_air();
    }
    if !(0..ny as isize).contains(&sy) {
        return true;
    }
    let world_x = chunk.x() * 16 + sx as i32 * cell;
    let world_y = sy as i32 * cell;
    let world_z = chunk.z() * 16 + sz as i32 * cell;
    !coarse_solid(sample, world_x, world_y, world_z, cell)
}

fn coarse_solid(
    sample: &impl Fn(i32, i32, i32) -> Block,
    x0: i32,
    y0: i32,
    z0: i32,
    cell: i32,
) -> bool {
    for z in z0..z0 + cell {
        for y in y0..y0 + cell {
            if !(0..256).contains(&y) {
                continue;
            }
            for x in x0..x0 + cell {
                if !shape_of(sample(x, y, z)).is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

fn lod_inset(step: i32, border: i32) -> usize {
    if border < step { 1 } else { 0 }
}

fn lod_interior(cx: usize, cz: usize, nx: usize, nz: usize, step: i32, borders: [i32; 4]) -> bool {
    let x0 = lod_inset(step, borders[0]);
    let x1 = nx.saturating_sub(lod_inset(step, borders[1]));
    let z0 = lod_inset(step, borders[2]);
    let z1 = nz.saturating_sub(lod_inset(step, borders[3]));
    cx >= x0 && cx < x1 && cz >= z0 && cz < z1
}

fn touches_strip(cx: usize, cz: usize, nx: usize, nz: usize, step: i32, borders: [i32; 4]) -> bool {
    let x0 = lod_inset(step, borders[0]);
    let x1 = nx.saturating_sub(lod_inset(step, borders[1]));
    let z0 = lod_inset(step, borders[2]);
    let z1 = nz.saturating_sub(lod_inset(step, borders[3]));
    (borders[0] < step && cx == x0)
        || (borders[1] < step && cx + 1 == x1)
        || (borders[2] < step && cz == z0)
        || (borders[3] < step && cz + 1 == z1)
}

/// The finer step just across this face, when that neighbor cell is a border strip.
fn strip_beside(
    axis: Axis,
    positive: bool,
    cx: usize,
    cz: usize,
    nx: usize,
    nz: usize,
    step: i32,
    borders: [i32; 4],
) -> Option<i32> {
    let (ncx, ncz) = match axis {
        Axis::Y => return None,
        Axis::X => {
            let next = cx as i32 + if positive { 1 } else { -1 };
            if !(0..nx as i32).contains(&next) {
                return None;
            }
            (next as usize, cz)
        }
        Axis::Z => {
            let next = cz as i32 + if positive { 1 } else { -1 };
            if !(0..nz as i32).contains(&next) {
                return None;
            }
            (cx, next as usize)
        }
    };
    if lod_interior(ncx, ncz, nx, nz, step, borders) {
        return None;
    }
    Some(cell_strip_step(ncx, ncz, nx, nz, step, borders))
}

fn cell_strip_step(
    cx: usize,
    cz: usize,
    nx: usize,
    nz: usize,
    step: i32,
    borders: [i32; 4],
) -> i32 {
    let mut sub = step;
    if cx == 0 && borders[0] < sub {
        sub = borders[0];
    }
    if cx + 1 == nx && borders[1] < sub {
        sub = borders[1];
    }
    if cz == 0 && borders[2] < sub {
        sub = borders[2];
    }
    if cz + 1 == nz && borders[3] < sub {
        sub = borders[3];
    }
    sub.max(1)
}

fn face_stamp(
    axis: Axis,
    positive: bool,
    slice: usize,
    u: usize,
    v: usize,
    origin: [f32; 3],
    scale: f32,
    block: Block,
    light: &LightField,
) -> Stamp {
    let (center, normal) = face_frame(axis, positive, slice, u, v, origin, scale);
    let outside = [
        (center[0] + normal[0] * 0.25).floor() as i32,
        (center[1] + normal[1] * 0.25).floor() as i32,
        (center[2] + normal[2] * 0.25).floor() as i32,
    ];
    Stamp {
        block,
        light: light.channels(outside[0], outside[1], outside[2]),
        bounce: [0, 0, 0],
    }
}

/// The part of a coarse face the finer strip does not already cover.
fn emit_split_face(
    chunk: &Chunk,
    axis: Axis,
    positive: bool,
    step: i32,
    sub: i32,
    cx: usize,
    cy: usize,
    cz: usize,
    block: Block,
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    let sub = sub.max(1);
    let parts = (step / sub).max(1) as usize;
    let cell = [
        chunk.x() * 16 + cx as i32 * step,
        cy as i32 * step,
        chunk.z() * 16 + cz as i32 * step,
    ];
    let scale = sub as f32;
    for iv in 0..parts {
        for iu in 0..parts {
            let neighbor = split_neighbor(axis, positive, cell, step, sub, iu, iv);
            if blocks_cover_face(
                sample,
                neighbor[0],
                neighbor[1],
                neighbor[2],
                sub,
                axis,
                positive,
            ) {
                continue;
            }
            let (slice, u, v) = split_slot(axis, positive, cx, cy, cz, step, sub, iu, iv);
            let stamp = face_stamp(axis, positive, slice, u, v, origin, scale, block, light);
            emit_quad(
                axis, positive, slice, u, v, 1, 1, stamp, origin, scale, sample, mesh,
            );
        }
    }
}

fn split_neighbor(
    axis: Axis,
    positive: bool,
    cell: [i32; 3],
    step: i32,
    sub: i32,
    iu: usize,
    iv: usize,
) -> [i32; 3] {
    let along_u = iu as i32 * sub;
    let along_v = iv as i32 * sub;
    match axis {
        Axis::X => {
            let x = if positive {
                cell[0] + step
            } else {
                cell[0] - sub
            };
            [x, cell[1] + along_v, cell[2] + along_u]
        }
        Axis::Z => {
            let z = if positive {
                cell[2] + step
            } else {
                cell[2] - sub
            };
            [cell[0] + along_u, cell[1] + along_v, z]
        }
        Axis::Y => cell,
    }
}

fn split_slot(
    axis: Axis,
    positive: bool,
    cx: usize,
    cy: usize,
    cz: usize,
    step: i32,
    sub: i32,
    iu: usize,
    iv: usize,
) -> (usize, usize, usize) {
    let sub = sub as usize;
    let parts = step as usize / sub;
    let sx = cx * parts;
    let sy = cy * parts;
    let sz = cz * parts;
    match axis {
        Axis::X => {
            let slice = if positive { sx + parts - 1 } else { sx };
            (slice, sz + iu, sy + iv)
        }
        Axis::Z => {
            let slice = if positive { sz + parts - 1 } else { sz };
            (slice, sx + iu, sy + iv)
        }
        Axis::Y => (sy, sx, sz),
    }
}

/// True when every block on the far side of the face covers it.
fn blocks_cover_face(
    sample: &impl Fn(i32, i32, i32) -> Block,
    x0: i32,
    y0: i32,
    z0: i32,
    sub: i32,
    axis: Axis,
    positive: bool,
) -> bool {
    for dz in 0..sub {
        for dy in 0..sub {
            for dx in 0..sub {
                let y = y0 + dy;
                if exposed(y, sample(x0 + dx, y, z0 + dz), axis, positive) {
                    return false;
                }
            }
        }
    }
    true
}

fn emit_border_skirts(
    cx: usize,
    cy: usize,
    cz: usize,
    nx: usize,
    nz: usize,
    step: i32,
    borders: [i32; 4],
    stamp: Stamp,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    let x0 = lod_inset(step, borders[0]);
    let x1 = nx.saturating_sub(lod_inset(step, borders[1]));
    let z0 = lod_inset(step, borders[2]);
    let z1 = nz.saturating_sub(lod_inset(step, borders[3]));
    if borders[0] < step && cx == x0 {
        emit_skirt(Axis::X, false, cx, cy, cz, step, stamp, origin, mesh);
    }
    if borders[1] < step && cx + 1 == x1 {
        emit_skirt(Axis::X, true, cx, cy, cz, step, stamp, origin, mesh);
    }
    if borders[2] < step && cz == z0 {
        emit_skirt(Axis::Z, false, cx, cy, cz, step, stamp, origin, mesh);
    }
    if borders[3] < step && cz + 1 == z1 {
        emit_skirt(Axis::Z, true, cx, cy, cz, step, stamp, origin, mesh);
    }
}

fn emit_skirt(
    axis: Axis,
    positive: bool,
    cx: usize,
    cy: usize,
    cz: usize,
    step: i32,
    stamp: Stamp,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    let scale = step as f32;
    let nudge = if positive { -LOD_SKIRT } else { LOD_SKIRT };
    let (u0, v0, u1, v1, plane) = match axis {
        Axis::X => {
            let plane = if positive {
                (cx as f32 + 1.0) * scale
            } else {
                cx as f32 * scale
            };
            (
                cz as f32 * scale,
                cy as f32 * scale,
                (cz as f32 + 1.0) * scale,
                (cy as f32 + 1.0) * scale,
                plane + nudge,
            )
        }
        Axis::Z => {
            let plane = if positive {
                (cz as f32 + 1.0) * scale
            } else {
                cz as f32 * scale
            };
            (
                cx as f32 * scale,
                cy as f32 * scale,
                (cx as f32 + 1.0) * scale,
                (cy as f32 + 1.0) * scale,
                plane + nudge,
            )
        }
        Axis::Y => return,
    };
    let corners = [
        corner(axis, plane, u0, v0, origin),
        corner(axis, plane, u1, v0, origin),
        corner(axis, plane, u1, v1, origin),
        corner(axis, plane, u0, v1, origin),
    ];
    let normal = match axis {
        Axis::X => [if positive { 1.0 } else { -1.0 }, 0.0, 0.0],
        Axis::Z => [0.0, 0.0, if positive { 1.0 } else { -1.0 }],
        Axis::Y => [0.0, 1.0, 0.0],
    };
    let albedo = face_color(stamp.block.id, stamp.block.metadata, normal);
    let (color, block) = lit_channels(albedo, stamp.light);
    let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    push_face(
        corners,
        uvs,
        color,
        block,
        normal,
        face_texture(stamp.block),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0, 1.0],
        mesh,
    );
}

fn mesh_lod_strips(
    chunk: &Chunk,
    step: i32,
    borders: [i32; 4],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    for sub in [1, 2] {
        if sub >= step {
            continue;
        }
        let (nx, ny, nz) = lod_count(sub);
        let mut owned = vec![false; nx * nz];
        let mut occupied = vec![Block::AIR; nx * ny * nz];
        for cz in 0..nz {
            for cx in 0..nx {
                let x0 = cx as i32 * sub;
                let z0 = cz as i32 * sub;
                if !cell_owned(x0, z0, sub, step, borders) {
                    continue;
                }
                owned[cz * nx + cx] = true;
                for cy in 0..ny {
                    occupied[lod_index(cx, cy, cz, sub)] = coarse_block(chunk, cx, cy, cz, sub);
                }
            }
        }
        if owned.iter().all(|cell| !*cell) {
            continue;
        }
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for positive in [false, true] {
                mesh_owned_direction(
                    chunk, axis, positive, sub, &owned, &occupied, sample, light, origin, mesh,
                );
            }
        }
    }
}

fn column_sub(x: i32, z: i32, step: i32, borders: [i32; 4]) -> Option<i32> {
    let mut sub: Option<i32> = None;
    let mut consider = |border: i32| {
        if border < step {
            sub = Some(match sub {
                Some(have) => have.min(border),
                None => border,
            });
        }
    };
    if x < step {
        consider(borders[0]);
    }
    if x >= 16 - step {
        consider(borders[1]);
    }
    if z < step {
        consider(borders[2]);
    }
    if z >= 16 - step {
        consider(borders[3]);
    }
    sub
}

fn cell_owned(x0: i32, z0: i32, sub: i32, step: i32, borders: [i32; 4]) -> bool {
    for z in z0..z0 + sub {
        for x in x0..x0 + sub {
            if column_sub(x, z, step, borders) != Some(sub) {
                return false;
            }
        }
    }
    true
}

fn mesh_owned_direction(
    chunk: &Chunk,
    axis: Axis,
    positive: bool,
    sub: i32,
    owned: &[bool],
    occupied: &[Block],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    origin: [f32; 3],
    mesh: &mut Mesh,
) {
    let (nx, ny, nz) = lod_count(sub);
    let (u_len, v_len, slices) = match axis {
        Axis::X => (nz, ny, nx),
        Axis::Y => (nx, nz, ny),
        Axis::Z => (nx, ny, nz),
    };
    let scale = sub as f32;
    for slice in 0..slices {
        for v in 0..v_len {
            for u in 0..u_len {
                let (cx, cy, cz) = coarse_coords(axis, slice, u, v);
                if !owned[cz * nx + cx] {
                    continue;
                }
                let block = occupied[lod_index(cx, cy, cz, sub)];
                if block.is_air() {
                    continue;
                }
                if !owned_face_open(
                    chunk, occupied, owned, axis, positive, sub, cx, cy, cz, nx, ny, nz, sample,
                ) {
                    continue;
                }
                let stamp = face_stamp(axis, positive, slice, u, v, origin, scale, block, light);
                emit_quad(
                    axis, positive, slice, u, v, 1, 1, stamp, origin, scale, sample, mesh,
                );
            }
        }
    }
}

fn owned_face_open(
    chunk: &Chunk,
    occupied: &[Block],
    owned: &[bool],
    axis: Axis,
    positive: bool,
    sub: i32,
    cx: usize,
    cy: usize,
    cz: usize,
    nx: usize,
    ny: usize,
    nz: usize,
    sample: &impl Fn(i32, i32, i32) -> Block,
) -> bool {
    let delta = if positive { 1isize } else { -1 };
    let (x, y, z) = (cx as isize, cy as isize, cz as isize);
    let (sx, sy, sz) = match axis {
        Axis::X => (x + delta, y, z),
        Axis::Y => (x, y + delta, z),
        Axis::Z => (x, y, z + delta),
    };
    if (0..nx as isize).contains(&sx)
        && (0..ny as isize).contains(&sy)
        && (0..nz as isize).contains(&sz)
        && owned[sz as usize * nx + sx as usize]
    {
        return occupied[lod_index(sx as usize, sy as usize, sz as usize, sub)].is_air();
    }
    let world_x = chunk.x() * 16 + sx as i32 * sub;
    let world_y = sy as i32 * sub;
    let world_z = chunk.z() * 16 + sz as i32 * sub;
    !blocks_cover_face(sample, world_x, world_y, world_z, sub, axis, positive)
}

fn mesh_direction(
    chunk: &Chunk,
    axis: Axis,
    positive: bool,
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    let (u_len, v_len, slices) = match axis {
        Axis::X => (16usize, 256usize, 16usize),
        Axis::Y => (16, 16, 256),
        Axis::Z => (16, 256, 16),
    };
    let origin = [chunk.x() as f32 * 16.0, 0.0, chunk.z() as f32 * 16.0];
    let mut mask = vec![Stamp::EMPTY; u_len * v_len];
    for slice in 0..slices {
        mask.fill(Stamp::EMPTY);
        for v in 0..v_len {
            for u in 0..u_len {
                let (x, y, z) = coords(axis, slice, u, v);
                let block = chunk.block(x, y, z);
                let shape = shape_of(block);
                // A model that fills the cell draws its own faces, so the
                // greedy cube would paint grass.png over the model's textures.
                if !shape.is_full_cube() || !shape.faces().is_empty() {
                    continue;
                }
                let (nx, ny, nz) = neighbor(axis, positive, x, y, z);
                let world_x = chunk.x() * 16 + nx;
                let world_z = chunk.z() * 16 + nz;
                if exposed(ny, sample(world_x, ny, world_z), axis, positive) {
                    let outside = [world_x, ny, world_z];
                    let channels = light.channels(outside[0], outside[1], outside[2]);
                    let (center, normal) = face_frame(axis, positive, slice, u, v, origin, 1.0);
                    let bounce = quantize_bounce(bounce_light(
                        sample,
                        center,
                        normal,
                        sun,
                        face_color(block.id, block.metadata, normal),
                    ));
                    let creased = face_is_creased(sample, axis, positive, slice, u, v, origin);
                    let stamp = Stamp {
                        block,
                        // The cell the face looks into. Mixing the block behind
                        // it with the next chunk invents a color on unlit grass.
                        light: channels,
                        bounce,
                    };
                    if creased {
                        emit_quad(
                            axis, positive, slice, u, v, 1, 1, stamp, origin, 1.0, sample, mesh,
                        );
                        continue;
                    }
                    mask[v * u_len + u] = stamp;
                }
            }
        }
        greedy(
            axis, positive, slice, u_len, v_len, origin, 1.0, sample, &mut mask, mesh,
        );
    }
}

fn mesh_partial_shapes(
    chunk: &Chunk,
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    for y in 0..256u16 {
        for z in 0..16u8 {
            for x in 0..16u8 {
                let block = chunk.block(x, y, z);
                let cell = [
                    chunk.x() * 16 + i32::from(x),
                    i32::from(y),
                    chunk.z() * 16 + i32::from(z),
                ];
                if block.id == "pipe" || block.id == "wire" || block.id == "pump" {
                    let links = conduit_sample_links(block.id, cell, sample);
                    mesh_conduit(block, links, cell, sample, light, sun, mesh);
                    if let Some((fill, boxes)) = conduit_fill_boxes(block, links) {
                        let fill_shape = Shape::from_boxes(&boxes);
                        mesh_cell(fill, fill_shape, cell, sample, light, sun, mesh);
                    }
                    continue;
                }
                if is_buffer(block.id) {
                    let shape = shape_of(block);
                    if !shape.is_empty() {
                        mesh_cell(block, shape, cell, sample, light, sun, mesh);
                        if !shape.faces().is_empty() {
                            mesh_faces(block, shape, cell, sample, light, sun, mesh);
                        }
                    }
                    if let Some((fill, boxes)) = buffer_fill_boxes(block) {
                        let fill_shape = Shape::from_boxes(&boxes);
                        mesh_cell(fill, fill_shape, cell, sample, light, sun, mesh);
                    }
                    continue;
                }
                if is_lamp(block.id) {
                    let shape = shape_of(block);
                    if !shape.is_empty() {
                        mesh_cell(block, shape, cell, sample, light, sun, mesh);
                        if !shape.faces().is_empty() {
                            mesh_faces(block, shape, cell, sample, light, sun, mesh);
                        }
                    }
                    if lamp_on(block.metadata) {
                        let fill = Block::new("magma", 0);
                        let fill_shape = Shape::from_boxes(&[CellBox {
                            min: [5, 3, 5],
                            max: [11, 11, 11],
                        }]);
                        mesh_cell(fill, fill_shape, cell, sample, light, sun, mesh);
                    }
                    continue;
                }
                if is_spout(block.id) || is_drain(block.id) {
                    let shape = shape_of(block);
                    if !shape.is_empty() {
                        mesh_cell(block, shape, cell, sample, light, sun, mesh);
                        if !shape.faces().is_empty() {
                            mesh_faces(block, shape, cell, sample, light, sun, mesh);
                        }
                    }
                    // Open spout/drain with water shows a drip at the nozzle.
                    if pump_on(block.metadata) {
                        let dir = pump_dir(block.metadata);
                        let wet = if is_drain(block.id) {
                            sample(
                                cell[0] + dir[0],
                                cell[1] + dir[1],
                                cell[2] + dir[2],
                            )
                            .id == "water"
                        } else {
                            let intake = sample(
                                cell[0] - dir[0],
                                cell[1] - dir[1],
                                cell[2] - dir[2],
                            );
                            intake.fill == FILL_WATER
                                || (is_tank(intake.id) && tank_level(intake) > 0)
                        };
                        if wet {
                            let drip = Block::new("water", 0);
                            let drip_shape = Shape::from_boxes(&[CellBox {
                                min: [7, 0, 7],
                                max: [9, 3, 9],
                            }]);
                            mesh_cell(drip, drip_shape, cell, sample, light, sun, mesh);
                        }
                    }
                    continue;
                }
                let shape = shape_of(block);
                if shape.is_empty() && !block.has_fill() {
                    continue;
                }
                if !shape.is_empty() {
                    if !shape.is_full_cube() {
                        mesh_cell(block, shape, cell, sample, light, sun, mesh);
                    }
                    if !shape.faces().is_empty() {
                        mesh_faces(block, shape, cell, sample, light, sun, mesh);
                    }
                }
                if let Some(fill) = compound_fill_proxy(block) {
                    // Slightly inset so the host lattice stays readable over the fill.
                    let fill_shape = shape_of(fill);
                    if !fill_shape.is_empty() {
                        mesh_cell(fill, fill_shape, cell, sample, light, sun, mesh);
                    }
                }
            }
        }
    }
}

/// Visual proxy for a compound fill: water or ember (magma look).
fn compound_fill_proxy(block: Block) -> Option<Block> {
    match block.fill {
        FILL_WATER => Some(Block::new("water", 0)),
        FILL_EMBER => Some(Block::new("magma", 0)),
        _ => None,
    }
}

/// Bit mask: +X -X +Y -Y +Z -Z from a mesh sample fn.
fn conduit_sample_links(
    id: &str,
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
) -> u8 {
    let dirs = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
    let mut mask = 0u8;
    for (i, [dx, dy, dz]) in dirs.into_iter().enumerate() {
        let neighbor = sample(cell[0] + dx, cell[1] + dy, cell[2] + dz);
        if conduit_links(id, neighbor.id) {
            mask |= 1 << i;
        }
    }
    mask
}

/// Pipe / wire shell: MC-like junctions — same cross-section arms, straight tubes,
/// and small end caps on open sides.
fn mesh_conduit(
    block: Block,
    links: u8,
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    let (lo, hi) = conduit_span(block.id);
    let caps = block.id == "pipe" || block.id == "pump";
    let boxes = conduit_shell_boxes(lo, hi, links, caps);
    let shape = Shape::from_boxes(&boxes);
    mesh_cell(block, shape, cell, sample, light, sun, mesh);
}

/// Fill follows the same junction topology, slightly inset.
fn conduit_fill_boxes(block: Block, links: u8) -> Option<(Block, Vec<CellBox>)> {
    if !compound_accepts(block.id, block.fill) {
        return None;
    }
    let fill = match block.fill {
        FILL_WATER => Block::new("water", 0),
        FILL_EMBER => Block::new("magma", 0),
        _ => return None,
    };
    let (lo, hi) = conduit_fill_span(block.id);
    Some((fill, conduit_shell_boxes(lo, hi, links, false)))
}

/// Reservoir / battery fill rises with buffer level inside the shell.
fn buffer_fill_boxes(block: Block) -> Option<(Block, Vec<CellBox>)> {
    let level = buffer_level(block);
    if level == 0 {
        return None;
    }
    let fill = match block.fill {
        FILL_WATER => Block::new("water", 0),
        FILL_EMBER => Block::new("magma", 0),
        _ => return None,
    };
    let max = buffer_max(block.id).max(1);
    // Interior 4..12 wide; height from floor (2) up toward lid (14).
    let height = 2 + ((12u16 * u16::from(level)) / u16::from(max)) as u8;
    let top = height.max(3).min(14);
    Some((
        fill,
        vec![CellBox {
            min: [4, 2, 4],
            max: [12, top, 12],
        }],
    ))
}

fn mesh_cell(
    block: Block,
    shape: Shape<'_>,
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    let origin = [cell[0] as f32, cell[1] as f32, cell[2] as f32];
    let channels = light.channels(cell[0], cell[1], cell[2]);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [false, true] {
            let mut mask = [Stamp::EMPTY; 16 * 16];
            for slice in 0..16usize {
                mask.fill(Stamp::EMPTY);
                for v in 0..16usize {
                    for u in 0..16usize {
                        let (x, y, z) = micro_coords(axis, slice, u, v);
                        if !shape.occludes(x, y, z) {
                            continue;
                        }
                        let (nx, ny, nz) = micro_neighbor(axis, positive, x, y, z);
                        if shape_covers(block, shape, nx, ny, nz, cell, sample) {
                            continue;
                        }
                        let (center, normal) =
                            face_frame(axis, positive, slice, u, v, origin, 1.0 / 16.0);
                        let bounce = quantize_bounce(bounce_light(
                            sample,
                            center,
                            normal,
                            sun,
                            face_color(block.id, block.metadata, normal),
                        ));
                        mask[v * 16 + u] = Stamp {
                            block,
                            light: channels,
                            bounce,
                        };
                    }
                }
                greedy(
                    axis,
                    positive,
                    slice,
                    16,
                    16,
                    origin,
                    1.0 / 16.0,
                    sample,
                    &mut mask,
                    mesh,
                );
            }
        }
    }
    for plane in shape.planes() {
        let (center, normal) = plane_center(origin, *plane);
        let bounce = bounce_light(
            sample,
            center,
            normal,
            sun,
            face_color(block.id, block.metadata, normal),
        );
        emit_plane(block, *plane, origin, channels, bounce, sample, mesh, false);
        if block.id == "flower" {
            let back = [-normal[0], -normal[1], -normal[2]];
            let bounce = bounce_light(
                sample,
                center,
                back,
                sun,
                face_color(block.id, block.metadata, back),
            );
            emit_plane(block, *plane, origin, channels, bounce, sample, mesh, true);
        }
    }
}

/// Model quads. A cube face uses its Blockbench texture and is culled when the
/// neighbor covers it. A fringe plane is drawn from both sides.
fn mesh_faces(
    block: Block,
    shape: Shape<'_>,
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    let origin = [cell[0] as f32, cell[1] as f32, cell[2] as f32];
    for face in shape.faces() {
        let local = face.corners();
        let world = local.map(|point| {
            [
                origin[0] + point[0],
                origin[1] + point[1],
                origin[2] + point[2],
            ]
        });
        let uvs = face.uvs();
        let base = shape.texture_of(*face);
        let owned = if block.id == "dirt" {
            base.map(|path| crate::texture::varied_texture(path, cell))
        } else {
            None
        };
        let path = owned.as_deref().or(base);
        let tint_for = |normal: [f32; 3]| {
            if block.id == "dirt" && block.metadata == 1 {
                return face_color(block.id, block.metadata, normal);
            }
            match path {
                Some(path) if dirt_art(path) => [1.0, 1.0, 1.0],
                _ => face_color(block.id, block.metadata, normal),
            }
        };
        if let Some((axis, step)) = boundary_side(&local) {
            let mut neighbor = cell;
            neighbor[axis] += step;
            if (0..256).contains(&neighbor[1])
                && shape_of(sample(neighbor[0], neighbor[1], neighbor[2])).is_full_cube()
            {
                continue;
            }
            let channels = if (0..256).contains(&neighbor[1]) {
                light.channels(neighbor[0], neighbor[1], neighbor[2])
            } else {
                light.channels(cell[0], cell[1], cell[2])
            };
            let mut normal = quad_normal(world);
            let mut corners = world;
            let mut uvs = uvs;
            let mut outward = [0.0; 3];
            outward[axis] = step as f32;
            if normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2] < 0.0 {
                normal = [-normal[0], -normal[1], -normal[2]];
                corners = [corners[0], corners[3], corners[2], corners[1]];
                uvs = [uvs[0], uvs[3], uvs[2], uvs[1]];
            }
            emit_free_face(
                block,
                corners,
                uvs,
                normal,
                tint_for(normal),
                path,
                channels,
                cell,
                sample,
                sun,
                mesh,
            );
            continue;
        }
        let Some(channels) = blade_light(&local, cell, sample, light) else {
            continue;
        };
        let normal = quad_normal(world);
        emit_free_face(
            block,
            world,
            uvs,
            normal,
            tint_for(normal),
            path,
            channels,
            cell,
            sample,
            sun,
            mesh,
        );
        let back = [world[0], world[3], world[2], world[1]];
        let back_uv = [uvs[0], uvs[3], uvs[2], uvs[1]];
        emit_free_face(
            block,
            back,
            back_uv,
            [-normal[0], -normal[1], -normal[2]],
            tint_for([-normal[0], -normal[1], -normal[2]]),
            path,
            channels,
            cell,
            sample,
            sun,
            mesh,
        );
    }
}

/// A quad that sits on one side of the cell, and which way that side faces.
fn boundary_side(corners: &[[f32; 3]; 4]) -> Option<(usize, i32)> {
    for axis in 0..3 {
        let on_lo = corners.iter().all(|corner| corner[axis].abs() <= 0.02);
        let on_hi = corners
            .iter()
            .all(|corner| (corner[axis] - 1.0).abs() <= 0.02);
        if on_lo {
            return Some((axis, -1));
        }
        if on_hi {
            return Some((axis, 1));
        }
    }
    None
}

fn dirt_art(path: &str) -> bool {
    path.ends_with("dirt.png") || path.contains("dirt_alt_")
}

/// Light from an open cell the blade enters. `None` when every one of those
/// cells is a full cube, so the blade is buried and not drawn.
fn blade_light(
    corners: &[[f32; 3]; 4],
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    light: &LightField,
) -> Option<[u8; 3]> {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for corner in corners {
        for axis in 0..3 {
            min[axis] = min[axis].min(corner[axis]);
            max[axis] = max[axis].max(corner[axis]);
        }
    }
    let mut exits = [[0i32; 3]; 6];
    let mut count = 0;
    for axis in 0..3 {
        for (past, step) in [(min[axis] < -0.01, -1), (max[axis] > 1.01, 1)] {
            if !past {
                continue;
            }
            let mut neighbor = cell;
            neighbor[axis] += step;
            exits[count] = neighbor;
            count += 1;
        }
    }
    let solid = |neighbor: [i32; 3]| {
        (0..256).contains(&neighbor[1])
            && shape_of(sample(neighbor[0], neighbor[1], neighbor[2])).is_full_cube()
    };
    if count > 0 && exits[..count].iter().copied().all(solid) {
        return None;
    }
    for neighbor in exits[..count].iter().copied() {
        if (0..256).contains(&neighbor[1]) && !solid(neighbor) {
            return Some(light.channels(neighbor[0], neighbor[1], neighbor[2]));
        }
    }
    let above = cell[1] + 1;
    if (0..256).contains(&above) {
        Some(light.channels(cell[0], above, cell[2]))
    } else {
        Some(light.channels(cell[0], cell[1], cell[2]))
    }
}

fn quad_normal(corners: [[f32; 3]; 4]) -> [f32; 3] {
    let edge_u = [
        corners[1][0] - corners[0][0],
        corners[1][1] - corners[0][1],
        corners[1][2] - corners[0][2],
    ];
    let edge_v = [
        corners[3][0] - corners[0][0],
        corners[3][1] - corners[0][1],
        corners[3][2] - corners[0][2],
    ];
    unit([
        edge_u[1] * edge_v[2] - edge_u[2] * edge_v[1],
        edge_u[2] * edge_v[0] - edge_u[0] * edge_v[2],
        edge_u[0] * edge_v[1] - edge_u[1] * edge_v[0],
    ])
}

fn emit_free_face(
    _block: Block,
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    normal: [f32; 3],
    albedo: [f32; 3],
    texture: Option<&str>,
    channels: [u8; 3],
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    sun: [f32; 3],
    mesh: &mut Mesh,
) {
    let center = [
        (corners[0][0] + corners[1][0] + corners[2][0] + corners[3][0]) * 0.25,
        (corners[0][1] + corners[1][1] + corners[2][1] + corners[3][1]) * 0.25,
        (corners[0][2] + corners[1][2] + corners[2][2] + corners[3][2]) * 0.25,
    ];
    let bounce = bounce_light(sample, center, normal, sun, albedo);
    let (color, block_light) = lit_channels(albedo, channels);
    let ao = corners.map(|position| corner_ao(sample, cell, position, normal));
    let textured = texture
        .and_then(texture_slot)
        .map(|slot| slot as f32 + 1.0)
        .unwrap_or(0.0);
    push_face(
        corners,
        uvs,
        color,
        block_light,
        normal,
        textured,
        bounce,
        ao,
        mesh,
    );
}

fn emit_plane(
    block: Block,
    plane: Plane,
    origin: [f32; 3],
    channels: [u8; 3],
    bounce: [f32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
    mesh: &mut Mesh,
    back: bool,
) {
    let axis = match plane.axis {
        0 => Axis::X,
        1 => Axis::Y,
        _ => Axis::Z,
    };
    let at = f32::from(plane.at) / 16.0;
    // A plane stores the other two axes in index order. An X plane's corner
    // wants Z in u and Y in v, so those two swap.
    let (u0, v0, u1, v1) = if plane.axis == 0 {
        (
            f32::from(plane.min[1]) / 16.0,
            f32::from(plane.min[0]) / 16.0,
            f32::from(plane.max[1]) / 16.0,
            f32::from(plane.max[0]) / 16.0,
        )
    } else {
        (
            f32::from(plane.min[0]) / 16.0,
            f32::from(plane.min[1]) / 16.0,
            f32::from(plane.max[0]) / 16.0,
            f32::from(plane.max[1]) / 16.0,
        )
    };
    let corners = [
        corner(axis, at, u0, v0, origin),
        corner(axis, at, u1, v0, origin),
        corner(axis, at, u1, v1, origin),
        corner(axis, at, u0, v1, origin),
    ];
    let mut normal = match axis {
        Axis::X => [1.0, 0.0, 0.0],
        Axis::Y => [0.0, 1.0, 0.0],
        Axis::Z => [0.0, 0.0, 1.0],
    };
    let mut corners = corners;
    let mut uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    if back {
        normal = [-normal[0], -normal[1], -normal[2]];
        corners = [corners[0], corners[3], corners[2], corners[1]];
        uvs = [uvs[0], uvs[3], uvs[2], uvs[1]];
    }
    let albedo = face_color(block.id, block.metadata, normal);
    let (color, block_light) = lit_channels(albedo, channels);
    let cell = [
        origin[0].floor() as i32,
        origin[1].floor() as i32,
        origin[2].floor() as i32,
    ];
    let ao = corners.map(|position| corner_ao(sample, cell, position, normal));
    push_face(
        corners,
        uvs,
        color,
        block_light,
        normal,
        face_texture(block),
        bounce,
        ao,
        mesh,
    );
}

fn micro_coords(axis: Axis, slice: usize, u: usize, v: usize) -> (u8, u8, u8) {
    match axis {
        Axis::X => (slice as u8, v as u8, u as u8),
        Axis::Y => (u as u8, slice as u8, v as u8),
        Axis::Z => (u as u8, v as u8, slice as u8),
    }
}

fn micro_neighbor(axis: Axis, positive: bool, x: u8, y: u8, z: u8) -> (i32, i32, i32) {
    let step = if positive { 1 } else { -1 };
    match axis {
        Axis::X => (i32::from(x) + step, i32::from(y), i32::from(z)),
        Axis::Y => (i32::from(x), i32::from(y) + step, i32::from(z)),
        Axis::Z => (i32::from(x), i32::from(y), i32::from(z) + step),
    }
}

/// Solid just across this micro-face, either inside the cell or in the neighbor.
fn shape_covers(
    block: Block,
    shape: Shape<'_>,
    x: i32,
    y: i32,
    z: i32,
    cell: [i32; 3],
    sample: &impl Fn(i32, i32, i32) -> Block,
) -> bool {
    if (0..16).contains(&x) && (0..16).contains(&y) && (0..16).contains(&z) {
        return shape.occludes(x as u8, y as u8, z as u8);
    }
    let (wx, mx) = step_cell(cell[0], x);
    let (wy, my) = step_cell(cell[1], y);
    let (wz, mz) = step_cell(cell[2], z);
    if !(0..256).contains(&wy) {
        return false;
    }
    let neighbor = sample(wx, wy, wz);
    // Linked conduits share a tube face — cull even when pack shape is core-only.
    if conduit_links(block.id, neighbor.id) && conduit_edge_tube(neighbor.id, mx, my, mz) {
        return true;
    }
    // Unlinked conduit neighbor: only the core post covers, not phantom arms.
    if neighbor.id == "pipe" || neighbor.id == "wire" {
        let (lo, hi) = conduit_span(neighbor.id);
        return mx >= lo && mx < hi && my >= lo && my < hi && mz >= lo && mz < hi;
    }
    shape_of(neighbor).occludes(mx, my, mz)
}

fn step_cell(cell: i32, micro: i32) -> (i32, u8) {
    if micro < 0 {
        (cell - 1, 15)
    } else if micro >= 16 {
        (cell + 1, 0)
    } else {
        (cell, micro as u8)
    }
}

fn coords(axis: Axis, slice: usize, u: usize, v: usize) -> (u8, u16, u8) {
    match axis {
        Axis::X => (slice as u8, v as u16, u as u8),
        Axis::Y => (u as u8, slice as u16, v as u8),
        Axis::Z => (u as u8, v as u16, slice as u8),
    }
}

fn neighbor(axis: Axis, positive: bool, x: u8, y: u16, z: u8) -> (i32, i32, i32) {
    let step = if positive { 1 } else { -1 };
    match axis {
        Axis::X => (x as i32 + step, y as i32, z as i32),
        Axis::Y => (x as i32, y as i32 + step, z as i32),
        Axis::Z => (x as i32, y as i32, z as i32 + step),
    }
}

fn exposed(y: i32, neighbor: Block, axis: Axis, positive: bool) -> bool {
    if !(0..256).contains(&y) {
        return true;
    }
    let axis = match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    };
    // The neighbor covers this face from its opposite side.
    !shape_of(neighbor).covers_cell_face(axis, !positive)
}

fn greedy(
    axis: Axis,
    positive: bool,
    slice: usize,
    u_len: usize,
    v_len: usize,
    origin: [f32; 3],
    scale: f32,
    sample: &impl Fn(i32, i32, i32) -> Block,
    mask: &mut [Stamp],
    mesh: &mut Mesh,
) {
    let mut v = 0;
    while v < v_len {
        let mut u = 0;
        while u < u_len {
            let stamp = mask[v * u_len + u];
            if stamp.is_empty() {
                u += 1;
                continue;
            }
            let mut width = 1;
            while u + width < u_len && mask[v * u_len + u + width].same_surface(stamp) {
                width += 1;
            }
            let mut height = 1;
            while v + height < v_len {
                let row_matches =
                    (0..width).all(|du| mask[(v + height) * u_len + u + du].same_surface(stamp));
                if !row_matches {
                    break;
                }
                height += 1;
            }
            for dv in 0..height {
                for du in 0..width {
                    mask[(v + dv) * u_len + u + du] = Stamp::EMPTY;
                }
            }
            emit_quad(
                axis, positive, slice, u, v, width, height, stamp, origin, scale, sample, mesh,
            );
            u += width;
        }
        v += 1;
    }
}

fn emit_quad(
    axis: Axis,
    positive: bool,
    slice: usize,
    u: usize,
    v: usize,
    width: usize,
    height: usize,
    stamp: Stamp,
    origin: [f32; 3],
    scale: f32,
    sample: &impl Fn(i32, i32, i32) -> Block,
    mesh: &mut Mesh,
) {
    let plane = if positive {
        (slice + 1) as f32
    } else {
        slice as f32
    } * scale;
    let u0 = u as f32 * scale;
    let v0 = v as f32 * scale;
    let u1 = (u + width) as f32 * scale;
    let v1 = (v + height) as f32 * scale;
    let corners = [
        corner(axis, plane, u0, v0, origin),
        corner(axis, plane, u1, v0, origin),
        corner(axis, plane, u1, v1, origin),
        corner(axis, plane, u0, v1, origin),
    ];
    let normal = match axis {
        Axis::X => [if positive { 1.0 } else { -1.0 }, 0.0, 0.0],
        Axis::Y => [0.0, if positive { 1.0 } else { -1.0 }, 0.0],
        Axis::Z => [0.0, 0.0, if positive { 1.0 } else { -1.0 }],
    };
    let albedo = face_color(stamp.block.id, stamp.block.metadata, normal);
    let (color, block) = lit_channels(albedo, stamp.light);
    let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let owners = [
        quad_cell(axis, slice, u, v, origin, scale),
        quad_cell(axis, slice, u + width - 1, v, origin, scale),
        quad_cell(axis, slice, u + width - 1, v + height - 1, origin, scale),
        quad_cell(axis, slice, u, v + height - 1, origin, scale),
    ];
    let ao = [
        corner_ao(sample, owners[0], corners[0], normal),
        corner_ao(sample, owners[1], corners[1], normal),
        corner_ao(sample, owners[2], corners[2], normal),
        corner_ao(sample, owners[3], corners[3], normal),
    ];
    push_face(
        corners,
        uvs,
        color,
        block,
        normal,
        face_texture(stamp.block),
        dequantize_bounce(stamp.bounce),
        ao,
        mesh,
    );
}

fn push_face(
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    color: [f32; 3],
    block: [f32; 3],
    normal: [f32; 3],
    textured: f32,
    bounce: [f32; 3],
    ao: [f32; 4],
    mesh: &mut Mesh,
) {
    push_face_shadows(
        corners, uvs, color, block, normal, textured, bounce, ao, mesh,
    );
}

fn push_face_shadows(
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    color: [f32; 3],
    block: [f32; 3],
    normal: [f32; 3],
    textured: f32,
    bounce: [f32; 3],
    ao: [f32; 4],
    mesh: &mut Mesh,
) {
    let base = mesh.vertices.len() as u32;
    for (index, (position, uv)) in corners.into_iter().zip(uvs).enumerate() {
        mesh.vertices.push(Vertex {
            position,
            color,
            normal,
            uv,
            textured,
            block,
            bounce,
            ao: ao[index],
        });
    }
    // The diagonal joins the brighter pair, so a crevice does not smear
    // across the lit part of the face.
    let flip = ao[0] + ao[2] < ao[1] + ao[3];
    let diagonal: [u32; 6] = if flip {
        [1, 2, 3, 1, 3, 0]
    } else {
        [0, 1, 2, 0, 2, 3]
    };
    mesh.indices.extend(diagonal.map(|index| base + index));
}

/// Atlas column plus one, or `0` when the pack names no usable PNG.
fn face_texture(block: Block) -> f32 {
    match Pack::builtin().texture_of(block).and_then(texture_slot) {
        Some(slot) => slot as f32 + 1.0,
        None => 0.0,
    }
}

fn corner(axis: Axis, plane: f32, u: f32, v: f32, origin: [f32; 3]) -> [f32; 3] {
    let (x, y, z) = match axis {
        Axis::X => (plane, v, u),
        Axis::Y => (u, plane, v),
        Axis::Z => (u, v, plane),
    };
    [origin[0] + x, origin[1] + y, origin[2] + z]
}

/// How far a sky ray looks. A hit inside this distance closes that ray.
const SKY_REACH: f32 = 12.0;

fn face_is_creased(
    sample: &impl Fn(i32, i32, i32) -> Block,
    axis: Axis,
    positive: bool,
    slice: usize,
    u: usize,
    v: usize,
    origin: [f32; 3],
) -> bool {
    face_ao(sample, axis, positive, slice, u, v, origin, 1.0)
        .into_iter()
        .any(|ao| ao < 0.999)
}

fn face_ao(
    sample: &impl Fn(i32, i32, i32) -> Block,
    axis: Axis,
    positive: bool,
    slice: usize,
    u: usize,
    v: usize,
    origin: [f32; 3],
    scale: f32,
) -> [f32; 4] {
    let normal = match axis {
        Axis::X => [if positive { 1.0 } else { -1.0 }, 0.0, 0.0],
        Axis::Y => [0.0, if positive { 1.0 } else { -1.0 }, 0.0],
        Axis::Z => [0.0, 0.0, if positive { 1.0 } else { -1.0 }],
    };
    let plane = if positive {
        (slice as f32 + 1.0) * scale
    } else {
        slice as f32 * scale
    };
    let corners = [
        corner(axis, plane, u as f32 * scale, v as f32 * scale, origin),
        corner(
            axis,
            plane,
            (u as f32 + 1.0) * scale,
            v as f32 * scale,
            origin,
        ),
        corner(
            axis,
            plane,
            (u as f32 + 1.0) * scale,
            (v as f32 + 1.0) * scale,
            origin,
        ),
        corner(
            axis,
            plane,
            u as f32 * scale,
            (v as f32 + 1.0) * scale,
            origin,
        ),
    ];
    let cell = quad_cell(axis, slice, u, v, origin, scale);
    corners.map(|position| corner_ao(sample, cell, position, normal))
}

fn quad_cell(
    axis: Axis,
    slice: usize,
    u: usize,
    v: usize,
    origin: [f32; 3],
    scale: f32,
) -> [i32; 3] {
    if scale < 1.0 {
        return [
            origin[0].floor() as i32,
            origin[1].floor() as i32,
            origin[2].floor() as i32,
        ];
    }
    let (x, y, z) = coords(axis, slice, u, v);
    [
        origin[0] as i32 + i32::from(x),
        i32::from(y),
        origin[2] as i32 + i32::from(z),
    ]
}

/// Sky openness at one corner. Rays leave the face, the way an ear traces a
/// sound. A miss is sky. A nearby hit closes that ray. The value fades across
/// the face. It is not a cast shadow.
fn corner_ao(
    sample: &impl Fn(i32, i32, i32) -> Block,
    cell: [i32; 3],
    position: [f32; 3],
    normal: [f32; 3],
) -> f32 {
    let outside = [
        cell[0] as f32 + 0.5 + normal[0] * 0.62,
        cell[1] as f32 + 0.5 + normal[1] * 0.62,
        cell[2] as f32 + 0.5 + normal[2] * 0.62,
    ];
    // Pull off the shared edge so the ray does not start inside the neighbor.
    let origin = [
        position[0] * 0.35 + outside[0] * 0.65,
        position[1] * 0.35 + outside[1] * 0.65,
        position[2] * 0.35 + outside[2] * 0.65,
    ];
    let (tangent, bitangent) = tangent_basis(normal);
    let locals = [
        [0.0, 1.0, 0.0],
        [0.52, 0.74, 0.16],
        [-0.38, 0.76, 0.34],
        [0.12, 0.68, -0.52],
    ];
    let mut sum = 0.0;
    for local in locals {
        let hemi = combine_basis(tangent, normal, bitangent, local);
        let dir = lift_toward_sky(hemi, normal);
        sum += ray_sky(sample, origin, dir);
    }
    let sky = soften_sky(sum / locals.len() as f32);
    // A shape beside the face, such as a stair, closes a little of the sky.
    // The step is small so it does not read as a second cast shadow.
    let touch = adjacent_fill(sample, cell, normal);
    sky * (1.0 - 0.16 * touch)
}

fn adjacent_fill(
    sample: &impl Fn(i32, i32, i32) -> Block,
    cell: [i32; 3],
    normal: [f32; 3],
) -> f32 {
    let air = [
        cell[0] + axis_sign(normal[0]),
        cell[1] + axis_sign(normal[1]),
        cell[2] + axis_sign(normal[2]),
    ];
    let mut hits = 0.0;
    let mut seen = 0.0;
    for axis in 0..3 {
        if normal[axis].abs() > 0.5 {
            continue;
        }
        for dir in [-1, 1] {
            let mut at = air;
            at[axis] += dir;
            seen += 1.0;
            if !(0..256).contains(&at[1]) {
                continue;
            }
            if !shape_of(sample(at[0], at[1], at[2])).is_empty() {
                hits += 1.0;
            }
        }
    }
    if seen == 0.0 { 0.0 } else { hits / seen }
}

fn axis_sign(value: f32) -> i32 {
    if value > 0.5 {
        1
    } else if value < -0.5 {
        -1
    } else {
        0
    }
}

fn lift_toward_sky(dir: [f32; 3], normal: [f32; 3]) -> [f32; 3] {
    if normal[1] < -0.4 {
        return dir;
    }
    unit([dir[0] * 0.72, dir[1] * 0.72 + 0.38, dir[2] * 0.72])
}

fn ray_sky(sample: &impl Fn(i32, i32, i32) -> Block, origin: [f32; 3], dir: [f32; 3]) -> f32 {
    match trace_hit(sample, origin, dir, SKY_REACH) {
        None => 1.0,
        Some(hit) => ((hit.dist - 0.35) / 5.5).clamp(0.0, 1.0),
    }
}

/// Open sky stays near 1. One nearby block is a soft contact. A closed room
/// falls well below that.
fn soften_sky(raw: f32) -> f32 {
    let contact = raw.clamp(0.0, 1.0);
    0.08 + 0.92 * contact.sqrt()
}

/// Short sun or moon check for a bounce hit. It only asks whether the nearby
/// surface is sitting in shade.
fn sun_blocked_near(
    sample: &impl Fn(i32, i32, i32) -> Block,
    ox: f32,
    oy: f32,
    oz: f32,
    dir: [f32; 3],
) -> bool {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len < 1.0e-6 || dir[1] <= 0.0 {
        return false;
    }
    let dx = dir[0] / len;
    let dy = dir[1] / len;
    let dz = dir[2] / len;
    let mut t = 0.02;
    for _ in 0..12 {
        if t > 6.0 {
            return false;
        }
        let x = ox + dx * t;
        let y = oy + dy * t;
        let z = oz + dz * t;
        if solid_at(sample, x, y, z) {
            return true;
        }
        let step = next_boundary(x, dx)
            .min(next_boundary(y, dy))
            .min(next_boundary(z, dz));
        t += step + 1.0e-3;
    }
    false
}

fn next_boundary(pos: f32, dir: f32) -> f32 {
    if dir.abs() < 1.0e-8 {
        return f32::INFINITY;
    }
    let frac = pos - pos.floor();
    if dir > 0.0 {
        (1.0 - frac).max(1.0e-4) / dir
    } else {
        frac.max(1.0e-4) / -dir
    }
}

fn solid_at(sample: &impl Fn(i32, i32, i32) -> Block, x: f32, y: f32, z: f32) -> bool {
    let iy = y.floor() as i32;
    if !(0..256).contains(&iy) {
        return false;
    }
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let shape = shape_of(sample(ix, iy, iz));
    if shape.is_empty() {
        return false;
    }
    if shape.is_full_cube() {
        return true;
    }
    let lx = (x - ix as f32) * 16.0;
    let ly = (y - iy as f32) * 16.0;
    let lz = (z - iz as f32) * 16.0;
    shape.boxes().iter().any(|box_| {
        lx >= f32::from(box_.min[0])
            && lx < f32::from(box_.max[0])
            && ly >= f32::from(box_.min[1])
            && ly < f32::from(box_.max[1])
            && lz >= f32::from(box_.min[2])
            && lz < f32::from(box_.max[2])
    }) || shape.planes().iter().any(|plane| {
        let (a, b, fixed) = match plane.axis {
            0 => (ly, lz, lx),
            1 => (lx, lz, ly),
            _ => (lx, ly, lz),
        };
        (fixed - f32::from(plane.at)).abs() <= 0.6
            && a >= f32::from(plane.min[0])
            && a < f32::from(plane.max[0])
            && b >= f32::from(plane.min[1])
            && b < f32::from(plane.max[1])
    })
}

fn face_frame(
    axis: Axis,
    positive: bool,
    slice: usize,
    u: usize,
    v: usize,
    origin: [f32; 3],
    scale: f32,
) -> ([f32; 3], [f32; 3]) {
    let plane = if positive {
        (slice as f32 + 1.0) * scale
    } else {
        slice as f32 * scale
    };
    let cu = (u as f32 + 0.5) * scale;
    let cv = (v as f32 + 0.5) * scale;
    let normal = match axis {
        Axis::X => [if positive { 1.0 } else { -1.0 }, 0.0, 0.0],
        Axis::Y => [0.0, if positive { 1.0 } else { -1.0 }, 0.0],
        Axis::Z => [0.0, 0.0, if positive { 1.0 } else { -1.0 }],
    };
    (corner(axis, plane, cu, cv, origin), normal)
}

fn plane_center(origin: [f32; 3], plane: Plane) -> ([f32; 3], [f32; 3]) {
    let at = f32::from(plane.at) / 16.0;
    let u = (f32::from(plane.min[0]) + f32::from(plane.max[0])) * 0.5 / 16.0;
    let v = (f32::from(plane.min[1]) + f32::from(plane.max[1])) * 0.5 / 16.0;
    let axis = match plane.axis {
        0 => Axis::X,
        1 => Axis::Y,
        _ => Axis::Z,
    };
    let normal = match axis {
        Axis::X => [1.0, 0.0, 0.0],
        Axis::Y => [0.0, 1.0, 0.0],
        Axis::Z => [0.0, 0.0, 1.0],
    };
    (corner(axis, at, u, v, origin), normal)
}

fn quantize_bounce(color: [f32; 3]) -> [u8; 3] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 32.0).round() as u8)
}

fn dequantize_bounce(color: [u8; 3]) -> [f32; 3] {
    color.map(|channel| f32::from(channel) / 32.0)
}

/// One bounce. Four short hemisphere rays. A miss takes the sky. A hit takes
/// that surface's color, but never brighter than the sky it blocked, and a
/// hit closer than about a block is occlusion. That keeps the grass under a
/// sign darker than open ground. The direct penumbra stays in the shadow
/// channel. Empty space stops at the distance cap, and this runs only while
/// a chunk mesh is built.
fn bounce_light(
    sample: &impl Fn(i32, i32, i32) -> Block,
    point: [f32; 3],
    normal: [f32; 3],
    sun: [f32; 3],
    albedo: [f32; 3],
) -> [f32; 3] {
    if sun[1] <= 0.0 {
        return [0.0; 3];
    }
    let sky_y = sun[1].clamp(0.0, 1.0);
    let sky = [0.16 * sky_y, 0.20 * sky_y, 0.26 * sky_y];
    let sky_lum = luminance(sky);
    let (tangent, bitangent) = tangent_basis(normal);
    let locals = [
        [0.0, 1.0, 0.0],
        [0.50, 0.72, 0.22],
        [-0.36, 0.74, 0.48],
        [0.18, 0.62, -0.58],
    ];
    let mut sum = [0.0; 3];
    for local in locals {
        let dir = combine_basis(tangent, normal, bitangent, local);
        let origin = [
            point[0] + normal[0] * 0.06,
            point[1] + normal[1] * 0.06,
            point[2] + normal[2] * 0.06,
        ];
        if let Some(hit) = trace_hit(sample, origin, dir, 4.5) {
            // 0 against a touching face, 1 once the hit is most of a block out.
            // Squaring this made a wall a step above the grass read as sky.
            let open = ((hit.dist - 0.12) / 0.55).clamp(0.0, 1.0);
            let ndotl = (hit.normal[0] * sun[0] + hit.normal[1] * sun[1] + hit.normal[2] * sun[2])
                .clamp(0.0, 1.0);
            let sunlit = !sun_blocked_near(
                sample,
                hit.point[0] + hit.normal[0] * 0.08,
                hit.point[1] + hit.normal[1] * 0.08,
                hit.point[2] + hit.normal[2] * 0.08,
                sun,
            );
            let shade = if sunlit { 0.45 + 0.55 * ndotl } else { 0.28 };
            let falloff = 1.0 / (1.0 + hit.dist * 0.35);
            let sample_color = hit.albedo.map(|channel| channel * shade * falloff);
            let sample_lum = luminance(sample_color).max(0.04);
            // Luminance stays at or below the sky this ray gave up, so a
            // nearby block cannot light the ground it shades.
            let scale = sky_lum * open / sample_lum;
            for channel in 0..3 {
                sum[channel] += sample_color[channel] * scale;
            }
        } else {
            for channel in 0..3 {
                sum[channel] += sky[channel];
            }
        }
    }
    let mut out = [0.0; 3];
    for channel in 0..3 {
        out[channel] = (albedo[channel] * sum[channel] / locals.len() as f32).clamp(0.0, 0.35);
    }
    out
}

fn luminance(color: [f32; 3]) -> f32 {
    color[0] * 0.25 + color[1] * 0.50 + color[2] * 0.25
}

fn tangent_basis(normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let helper = if normal[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let tangent = unit(cross(helper, normal));
    let bitangent = cross(normal, tangent);
    (tangent, bitangent)
}

fn combine_basis(
    tangent: [f32; 3],
    normal: [f32; 3],
    bitangent: [f32; 3],
    local: [f32; 3],
) -> [f32; 3] {
    unit([
        tangent[0] * local[0] + normal[0] * local[1] + bitangent[0] * local[2],
        tangent[1] * local[0] + normal[1] * local[1] + bitangent[1] * local[2],
        tangent[2] * local[0] + normal[2] * local[1] + bitangent[2] * local[2],
    ])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

struct BounceHit {
    point: [f32; 3],
    normal: [f32; 3],
    albedo: [f32; 3],
    dist: f32,
}

fn trace_hit(
    sample: &impl Fn(i32, i32, i32) -> Block,
    origin: [f32; 3],
    dir: [f32; 3],
    max_t: f32,
) -> Option<BounceHit> {
    let dir = unit(dir);
    let mut previous = origin;
    let mut t = 0.02;
    for _ in 0..24 {
        if t > max_t {
            return None;
        }
        let point = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        if solid_at(sample, point[0], point[1], point[2]) {
            let normal = axis_normal(previous, point);
            let block = sample(
                point[0].floor() as i32,
                point[1].floor() as i32,
                point[2].floor() as i32,
            );
            return Some(BounceHit {
                point: previous,
                normal,
                albedo: face_color(block.id, block.metadata, normal),
                dist: t,
            });
        }
        let step = next_boundary(point[0], dir[0])
            .min(next_boundary(point[1], dir[1]))
            .min(next_boundary(point[2], dir[2]));
        previous = point;
        t += step + 1.0e-3;
    }
    None
}

fn axis_normal(air: [f32; 3], solid: [f32; 3]) -> [f32; 3] {
    let delta = [air[0] - solid[0], air[1] - solid[1], air[2] - solid[2]];
    let ax = delta[0].abs();
    let ay = delta[1].abs();
    let az = delta[2].abs();
    if ax >= ay && ax >= az {
        [if delta[0] >= 0.0 { 1.0 } else { -1.0 }, 0.0, 0.0]
    } else if ay >= az {
        [0.0, if delta[1] >= 0.0 { 1.0 } else { -1.0 }, 0.0]
    } else {
        [0.0, 0.0, if delta[2] >= 0.0 { 1.0 } else { -1.0 }]
    }
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1.0e-6 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn lit_channels(albedo: [f32; 3], channels: [u8; 3]) -> ([f32; 3], [f32; 3]) {
    (apply_light(albedo, channels), block_term(albedo, channels))
}

fn apply_light(albedo: [f32; 3], channels: [u8; 3]) -> [f32; 3] {
    let mut color = [0.0; 3];
    for channel in 0..3 {
        let scale = AMBIENT + f32::from(channels[channel]) / 15.0;
        color[channel] = (albedo[channel] * scale).clamp(0.0, 1.0);
    }
    color
}

fn block_term(albedo: [f32; 3], channels: [u8; 3]) -> [f32; 3] {
    let mut color = [0.0; 3];
    for channel in 0..3 {
        color[channel] = albedo[channel] * f32::from(channels[channel]) / 15.0;
    }
    color
}

fn face_color(id: &str, metadata: u8, normal: [f32; 3]) -> [f32; 3] {
    if id == "grass" {
        // Plains stay the mid green. Forest cuts the red so the canopy reads
        // cool. Hills crush the blue so the dry grass reads gold.
        let (top, side) = match metadata {
            1 => ([0.14, 0.46, 0.22], [0.18, 0.36, 0.16]),
            2 => ([0.95, 0.88, 0.36], [0.78, 0.70, 0.32]),
            _ => ([0.45, 0.70, 0.30], [0.50, 0.58, 0.28]),
        };
        if normal[1] > 0.5 {
            return top;
        }
        if normal[1] < -0.5 {
            return [0.58, 0.41, 0.27];
        }
        return side;
    }
    if id == "dirt" && metadata == 1 {
        return [0.32, 0.48, 0.16];
    }
    if id == "oak_log" && normal[1].abs() > 0.5 {
        return [0.72, 0.58, 0.32];
    }
    palette::color(id, metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Cluster, OAK_STAIRS, Pack, synthetic_chunk, synthetic_cluster};

    fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for vertex in &mesh.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
        }
        (min, max)
    }

    #[test]
    fn grass_blades_use_the_biome_side_tint() {
        let dirt =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(4, 2, 4, Block::new("dirt", 0))]).unwrap());
        let plains =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(4, 2, 4, Block::new("grass", 0))]).unwrap());
        let forest =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(4, 2, 4, Block::new("grass", 1))]).unwrap());
        let hills =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(4, 2, 4, Block::new("grass", 2))]).unwrap());
        assert!(plains.quad_count() > dirt.quad_count());
        let plains_blade = blade_color(&plains, 4.0, 2.0, 4.0);
        let forest_blade = blade_color(&forest, 4.0, 2.0, 4.0);
        let hills_blade = blade_color(&hills, 4.0, 2.0, 4.0);
        assert!(
            plains_blade[1] > plains_blade[0],
            "plains blade is green, not dirt: {plains_blade:?}"
        );
        assert!(
            forest_blade[0] + 0.05 < plains_blade[0],
            "forest blade drops the red: {forest_blade:?} vs {plains_blade:?}"
        );
        assert!(
            hills_blade[0] > plains_blade[0] + 0.05,
            "hills blade is gold: {hills_blade:?} vs {plains_blade:?}"
        );
    }

    fn blade_color(mesh: &Mesh, x: f32, y: f32, z: f32) -> [f32; 3] {
        blade_color_or_none(mesh, x, y, z).expect("grass blade")
    }

    fn blade_color_or_none(mesh: &Mesh, x: f32, y: f32, z: f32) -> Option<[f32; 3]> {
        mesh.vertices.iter().find_map(|vertex| {
            let local = [
                vertex.position[0] - x,
                vertex.position[1] - y,
                vertex.position[2] - z,
            ];
            let near = local.iter().all(|axis| (-0.8..1.8).contains(axis));
            let off_grid = local.iter().any(|axis| {
                let fraction = axis.abs() % 1.0;
                fraction > 0.02 && fraction < 0.98
            });
            if near && off_grid {
                Some(vertex.color)
            } else {
                None
            }
        })
    }

    #[test]
    fn biome_tints_read_apart_on_grass_and_foliage() {
        let up = [0.0, 1.0, 0.0];
        let plains = face_color("grass", 0, up);
        let forest = face_color("grass", 1, up);
        let hills = face_color("grass", 2, up);
        let marsh = face_color("dirt", 1, up);
        let dirt = face_color("dirt", 0, up);
        let leaves = face_color("leaves", 0, up);
        assert!(
            hills[0] > plains[0] + 0.3,
            "hills grass is gold, not mid green"
        );
        assert!(hills[2] + 0.4 < hills[0], "hill blue stays crushed");
        assert!(forest[0] + 0.2 < plains[0], "forest grass drops the red");
        assert!(forest[1] + 0.15 < plains[1], "forest grass is darker");
        assert!(marsh[1] > marsh[0], "marsh dirt is olive");
        assert!(dirt[0] > dirt[1], "open dirt stays brown");
        assert!(leaves[1] + 0.25 < plains[1], "leaves are a deep canopy");
    }

    #[test]
    fn single_block_has_six_faces() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        assert_eq!(mesh.quad_count(), 6);
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(bounds(&mesh), ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    }

    #[test]
    fn vertical_pair_merges_the_sides() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (0, 1, 0, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        // Six outer rectangles: top, bottom, and four sides of height 2.
        assert_eq!(mesh_chunk(&chunk).quad_count(), 6);
    }

    #[test]
    fn different_metadata_does_not_merge() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("wool", 1)),
                (1, 0, 0, Block::new("wool", 2)),
            ],
        )
        .unwrap();
        // Shared face is culled. The other ten faces stay split by metadata.
        assert_eq!(mesh_chunk(&chunk).quad_count(), 10);
    }

    #[test]
    fn mesh_follows_chunk_origin() {
        let chunk = Chunk::from_blocks(2, -1, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let (min, max) = bounds(&mesh_chunk(&chunk));
        assert_eq!(min, [32.0, 0.0, -16.0]);
        assert_eq!(max, [33.0, 1.0, -15.0]);
    }

    #[test]
    fn synthetic_chunk_culls_hidden_faces() {
        let chunk = synthetic_chunk();
        let mesh = mesh_chunk(&chunk);
        let cubes = mesh
            .vertices
            .chunks(4)
            .filter(|quad| {
                quad.len() == 4
                    && quad.iter().all(|vertex| {
                        vertex
                            .position
                            .iter()
                            .all(|axis| (axis - axis.round()).abs() < 1.0e-3)
                    })
            })
            .count();
        assert!(cubes > 0);
        assert!(cubes < chunk.non_air_count() as usize * 6);
    }

    fn floor(chunk_x: i32, chunk_z: i32, id: &str) -> Chunk {
        let blocks =
            (0..16u8).flat_map(|z| (0..16u8).map(move |x| (x, 0u16, z, Block::new(id, 0))));
        Chunk::from_blocks(chunk_x, chunk_z, blocks).unwrap()
    }

    #[test]
    fn glass_leaves_the_block_behind_it_visible() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 1, 0, Block::new("glass", 0)),
                (0, 1, 1, Block::new("stone", 0)),
            ],
        )
        .unwrap();
        let mesh = mesh_chunk(&chunk);
        let visible = mesh.vertices.chunks(4).any(|quad| {
            if quad.len() != 4 {
                return false;
            }
            let z = quad[0].position[2];
            if quad
                .iter()
                .any(|vertex| (vertex.position[2] - z).abs() > 1.0e-3)
            {
                return false;
            }
            if (z - 1.0).abs() > 1.0e-3 {
                return false;
            }
            let span = |axis: usize| {
                let min = quad
                    .iter()
                    .map(|vertex| vertex.position[axis])
                    .fold(f32::MAX, f32::min);
                let max = quad
                    .iter()
                    .map(|vertex| vertex.position[axis])
                    .fold(f32::MIN, f32::max);
                max - min
            };
            span(0) > 0.5 && span(1) > 0.5
        });
        assert!(visible, "the stone face behind the glass should stay");
    }

    #[test]
    fn a_slab_mesh_stops_at_half_height() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("slab", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let top = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (top - 1.5).abs() < 1.0e-3,
            "the slab should be half a block, top {top}"
        );
    }

    #[test]
    fn an_upper_slab_mesh_sits_against_the_ceiling() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 2, 0, Block::new("slab", 1))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (min_y - 2.5).abs() < 1.0e-3,
            "the upper slab should start at half height, min {min_y}"
        );
        assert!(
            (max_y - 3.0).abs() < 1.0e-3,
            "the upper slab should reach the ceiling, max {max_y}"
        );
    }

    #[test]
    fn a_carpet_mesh_is_one_pixel_thick() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("carpet", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!((min_y - 1.0).abs() < 1.0e-3, "bottom {min_y}");
        assert!(
            (max_y - (1.0 + 1.0 / 16.0)).abs() < 1.0e-3,
            "a carpet should be one pixel, top {max_y}"
        );
    }

    #[test]
    fn a_lily_pad_mesh_is_two_pixels_thick() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("lily_pad", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (max_y - (1.0 + 2.0 / 16.0)).abs() < 1.0e-3,
            "a lily pad should be two pixels, top {max_y}"
        );
    }

    #[test]
    fn a_snow_layer_mesh_is_one_pixel_thick() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("snow", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (max_y - (1.0 + 1.0 / 16.0)).abs() < 1.0e-3,
            "snow should be one pixel, top {max_y}"
        );
    }

    #[test]
    fn a_torch_mesh_is_a_thin_post_or_a_wall_peg() {
        let standing = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("torch", 0))]).unwrap();
        let mesh = mesh_chunk(&standing);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        let max_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(0.0_f32, f32::max);
        assert!((min_y - 1.0).abs() < 1.0e-3, "bottom {min_y}");
        assert!(
            (max_y - (1.0 + 14.0 / 16.0)).abs() < 1.0e-3,
            "a standing torch stops short of the ceiling, top {max_y}"
        );
        assert!(
            max_x - min_x < 0.3,
            "the post should stay thin, span {}",
            max_x - min_x
        );

        let hung = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("torch", 1))]).unwrap();
        let wall = mesh_chunk(&hung);
        let peg = wall
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        assert!(
            (peg - 0.0).abs() < 1.0e-3,
            "a west torch sits on the wall, x {peg}"
        );
    }

    #[test]
    fn an_end_rod_mesh_is_a_thin_post_or_a_wall_rod() {
        let standing = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("end_rod", 0))]).unwrap();
        let mesh = mesh_chunk(&standing);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        let max_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(0.0_f32, f32::max);
        assert!((min_y - 1.0).abs() < 1.0e-3, "bottom {min_y}");
        assert!(
            (max_y - 2.0).abs() < 1.0e-3,
            "a standing end rod reaches the top of its cell, top {max_y}"
        );
        assert!(
            max_x - min_x < 0.4,
            "the rod should stay thin, span {}",
            max_x - min_x
        );

        let hung = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("end_rod", 1))]).unwrap();
        let wall = mesh_chunk(&hung);
        let peg = wall
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        assert!(
            (peg - 0.0).abs() < 1.0e-3,
            "a west end rod sits on the wall, x {peg}"
        );
    }

    #[test]
    fn a_flower_is_a_cross_you_can_see_from_both_sides() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("flower", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let mut saw_pos_x = false;
        let mut saw_neg_x = false;
        let mut saw_pos_z = false;
        let mut saw_neg_z = false;
        let mut min_y = f32::MAX;
        let mut max_y = 0.0_f32;
        for vertex in &mesh.vertices {
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
            saw_pos_x |= vertex.normal[0] > 0.9;
            saw_neg_x |= vertex.normal[0] < -0.9;
            saw_pos_z |= vertex.normal[2] > 0.9;
            saw_neg_z |= vertex.normal[2] < -0.9;
        }
        assert!(saw_pos_x && saw_neg_x && saw_pos_z && saw_neg_z);
        assert!((min_y - 1.0).abs() < 1.0e-3, "bottom {min_y}");
        assert!(
            (max_y - (1.0 + 12.0 / 16.0)).abs() < 1.0e-3,
            "the flower stops short of the ceiling, top {max_y}"
        );
        assert!(
            mesh.vertices.len() < 64,
            "a cross should stay a few quads, verts {}",
            mesh.vertices.len()
        );
    }

    #[test]
    fn a_sugar_cane_is_a_full_height_cross() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("sugar_cane", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let mut saw_x = false;
        let mut saw_z = false;
        let mut max_y = 0.0_f32;
        for vertex in &mesh.vertices {
            max_y = max_y.max(vertex.position[1]);
            saw_x |= vertex.normal[0].abs() > 0.9;
            saw_z |= vertex.normal[2].abs() > 0.9;
        }
        assert!(saw_x && saw_z, "the cane is two crossing planes");
        assert!(
            (max_y - 2.0).abs() < 1.0e-3,
            "the cane fills the cell, top {max_y}"
        );
    }

    #[test]
    fn a_chorus_plant_is_a_full_height_cross() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("chorus_plant", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let mut saw_x = false;
        let mut saw_z = false;
        let mut max_y = 0.0_f32;
        for vertex in &mesh.vertices {
            max_y = max_y.max(vertex.position[1]);
            saw_x |= vertex.normal[0].abs() > 0.9;
            saw_z |= vertex.normal[2].abs() > 0.9;
        }
        assert!(saw_x && saw_z, "the chorus plant is two crossing planes");
        assert!(
            (max_y - 2.0).abs() < 1.0e-3,
            "the chorus plant fills the cell, top {max_y}"
        );
    }

    #[test]
    fn a_chorus_flower_is_a_short_bloom() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("chorus_flower", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let mut saw_x = false;
        let mut saw_z = false;
        let mut max_y = 0.0_f32;
        for vertex in &mesh.vertices {
            max_y = max_y.max(vertex.position[1]);
            saw_x |= vertex.normal[0].abs() > 0.9;
            saw_z |= vertex.normal[2].abs() > 0.9;
        }
        assert!(saw_x && saw_z, "the chorus flower is two crossing planes");
        assert!(
            (max_y - (1.0 + 12.0 / 16.0)).abs() < 1.0e-3,
            "the bloom stops short of the ceiling, top {max_y}"
        );
    }

    #[test]
    fn a_cobweb_is_a_full_height_cross() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("cobweb", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let mut saw_x = false;
        let mut saw_z = false;
        let mut max_y = 0.0_f32;
        for vertex in &mesh.vertices {
            max_y = max_y.max(vertex.position[1]);
            saw_x |= vertex.normal[0].abs() > 0.9;
            saw_z |= vertex.normal[2].abs() > 0.9;
        }
        assert!(saw_x && saw_z, "the web is two crossing planes");
        assert!(
            (max_y - 2.0).abs() < 1.0e-3,
            "the web fills the cell, top {max_y}"
        );
    }

    #[test]
    fn a_button_mesh_is_a_short_nub() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("button", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        let max_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(0.0_f32, f32::max);
        assert!((min_y - 1.0).abs() < 1.0e-3, "bottom {min_y}");
        assert!(
            (max_y - (1.0 + 2.0 / 16.0)).abs() < 1.0e-3,
            "a button is two pixels tall, top {max_y}"
        );
        assert!(
            (max_x - min_x - 4.0 / 16.0).abs() < 1.0e-3,
            "the nub should stay narrow, span {}",
            max_x - min_x
        );
    }

    #[test]
    fn a_lever_stands_up_until_it_is_flipped() {
        let off = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("lever", 0))]).unwrap();
        let on = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("lever", 1))]).unwrap();
        let off_top = mesh_chunk(&off)
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        let on_top = mesh_chunk(&on)
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (off_top - (1.0 + 12.0 / 16.0)).abs() < 1.0e-3,
            "an off lever stands up, top {off_top}"
        );
        assert!(
            (on_top - (1.0 + 6.0 / 16.0)).abs() < 1.0e-3,
            "an on lever lies down, top {on_top}"
        );
    }

    #[test]
    fn a_tripwire_mesh_is_a_string_across_the_cell() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("tripwire", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let min_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        let max_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(0.0_f32, f32::max);
        assert!(
            (min_y - (1.0 + 6.0 / 16.0)).abs() < 1.0e-3,
            "the string should sit above the floor, bottom {min_y}"
        );
        assert!(
            (max_y - (1.0 + 8.0 / 16.0)).abs() < 1.0e-3,
            "the string should stay two pixels tall, top {max_y}"
        );
        assert!(
            (max_x - min_x - 1.0).abs() < 1.0e-3,
            "the string should cross the cell, span {}",
            max_x - min_x
        );
    }

    #[test]
    fn a_painting_sits_against_the_wall_it_faces() {
        let west = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("painting", 1))]).unwrap();
        let east = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("painting", 2))]).unwrap();
        let west_mesh = mesh_chunk(&west);
        let east_mesh = mesh_chunk(&east);
        let span = |mesh: &Mesh, axis: usize| {
            let min = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[axis])
                .fold(f32::MAX, f32::min);
            let max = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[axis])
                .fold(0.0_f32, f32::max);
            (min, max)
        };
        let (west_min, west_max) = span(&west_mesh, 0);
        let (east_min, east_max) = span(&east_mesh, 0);
        assert!((west_min - 0.0).abs() < 1.0e-3, "west min {west_min}");
        assert!(
            (west_max - 1.0 / 16.0).abs() < 1.0e-3,
            "a west painting is one pixel thick, max {west_max}"
        );
        assert!(
            (east_min - 15.0 / 16.0).abs() < 1.0e-3,
            "an east painting starts at the east wall, min {east_min}"
        );
        assert!((east_max - 1.0).abs() < 1.0e-3, "east max {east_max}");
    }

    #[test]
    fn a_vine_is_a_sheet_against_the_wall_it_faces() {
        let west = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("vine", 1))]).unwrap();
        let east = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("vine", 2))]).unwrap();
        let span = |chunk: &Chunk| {
            let mesh = mesh_chunk(chunk);
            let min = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::MAX, f32::min);
            let max = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(0.0_f32, f32::max);
            (min, max)
        };
        let (west_min, west_max) = span(&west);
        let (east_min, east_max) = span(&east);
        assert!((west_min - 0.0).abs() < 1.0e-3, "west min {west_min}");
        assert!(
            (west_max - 2.0 / 16.0).abs() < 1.0e-3,
            "a west vine is two pixels thick, max {west_max}"
        );
        assert!(
            (east_min - 14.0 / 16.0).abs() < 1.0e-3,
            "an east vine starts at the east wall, min {east_min}"
        );
        assert!((east_max - 1.0).abs() < 1.0e-3, "east max {east_max}");
    }

    #[test]
    fn a_planted_pot_grows_above_the_empty_one() {
        let empty = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("flower_pot", 0))]).unwrap();
        let planted = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("flower_pot", 1))]).unwrap();
        let top = |chunk: &Chunk| {
            mesh_chunk(chunk)
                .vertices
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(0.0_f32, f32::max)
        };
        let empty_top = top(&empty);
        let planted_top = top(&planted);
        assert!(
            (empty_top - (1.0 + 6.0 / 16.0)).abs() < 1.0e-3,
            "an empty pot stays short, top {empty_top}"
        );
        assert!(
            (planted_top - (1.0 + 14.0 / 16.0)).abs() < 1.0e-3,
            "a planted pot grows a stem, top {planted_top}"
        );
    }

    #[test]
    fn a_filled_frame_grows_an_eye_above_the_empty_one() {
        let empty =
            Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("end_portal_frame", 0))]).unwrap();
        let filled =
            Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("end_portal_frame", 1))]).unwrap();
        let top = |chunk: &Chunk| {
            mesh_chunk(chunk)
                .vertices
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(0.0_f32, f32::max)
        };
        let empty_top = top(&empty);
        let filled_top = top(&filled);
        assert!(
            (empty_top - (1.0 + 13.0 / 16.0)).abs() < 1.0e-3,
            "an empty frame stays low, top {empty_top}"
        );
        assert!(
            (filled_top - 2.0).abs() < 1.0e-3,
            "a set eye reaches the top of the cell, top {filled_top}"
        );
    }

    #[test]
    fn a_dragon_egg_is_wider_in_the_middle() {
        let egg = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("dragon_egg", 0))]).unwrap();
        let mesh = mesh_chunk(&egg);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(1.0_f32, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (min_x - 3.0 / 16.0).abs() < 1.0e-3,
            "the belly starts inset, min x {min_x}"
        );
        assert!(
            (max_y - (1.0 + 15.0 / 16.0)).abs() < 1.0e-3,
            "the tip stops short of a full block, top {max_y}"
        );
    }

    #[test]
    fn an_end_crystal_is_wider_through_the_middle() {
        let crystal = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("end_crystal", 0))]).unwrap();
        let mesh = mesh_chunk(&crystal);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(1.0_f32, f32::min);
        let max_y = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(0.0_f32, f32::max);
        assert!(
            (min_x - 4.0 / 16.0).abs() < 1.0e-3,
            "the middle starts inset, min x {min_x}"
        );
        assert!(
            (max_y - 2.0).abs() < 1.0e-3,
            "the tip reaches the top of the cell, top {max_y}"
        );
    }

    #[test]
    fn a_cactus_mesh_is_an_inset_column() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("cactus", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let min_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(1.0_f32, f32::min);
        let max_x = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(0.0_f32, f32::max);
        assert!(
            (min_x - 1.0 / 16.0).abs() < 1.0e-3,
            "cactus inset min {min_x}"
        );
        assert!(
            (max_x - 15.0 / 16.0).abs() < 1.0e-3,
            "cactus inset max {max_x}"
        );
    }

    #[test]
    fn a_bitten_cake_is_narrower_than_a_whole_one() {
        let whole = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("cake", 0))]).unwrap();
        let last = Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("cake", 5))]).unwrap();
        let span = |chunk: &Chunk| {
            let mesh = mesh_chunk(chunk);
            let min_x = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(1.0_f32, f32::min);
            let top = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(0.0_f32, f32::max);
            (min_x, top)
        };
        let (whole_x, whole_top) = span(&whole);
        let (last_x, last_top) = span(&last);
        assert!((whole_x - 1.0 / 16.0).abs() < 1.0e-3, "whole min {whole_x}");
        assert!(
            (whole_top - (1.0 + 8.0 / 16.0)).abs() < 1.0e-3,
            "cake top {whole_top}"
        );
        assert!(
            (last_x - 11.0 / 16.0).abs() < 1.0e-3,
            "last slice min {last_x}"
        );
        assert!((last_top - whole_top).abs() < 1.0e-3);
    }

    #[test]
    fn a_carved_pumpkin_uses_the_face_texture() {
        use crate::texture::texture_slot;

        let plain =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("pumpkin", 0))]).unwrap());
        let carved =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("pumpkin", 1))]).unwrap());
        let plain_slot =
            texture_slot("assets/terralite/textures/pumpkin.png").unwrap() as f32 + 1.0;
        let face_slot =
            texture_slot("assets/terralite/textures/carved_pumpkin.png").unwrap() as f32 + 1.0;
        assert!(plain_slot != face_slot);
        assert!(
            plain
                .vertices
                .iter()
                .any(|vertex| (vertex.textured - plain_slot).abs() < 1.0e-3)
        );
        assert!(
            carved
                .vertices
                .iter()
                .any(|vertex| (vertex.textured - face_slot).abs() < 1.0e-3)
        );
    }

    #[test]
    fn a_held_torch_bakes_block_light_into_nearby_faces() {
        let mut blocks = Vec::new();
        for z in 0..8u8 {
            for x in 0..8u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let sun = [0.25, 1.0, -0.72];
        let mut bake = ChunkBake::new(&cluster, sun, [0, 0]);
        bake.queue_unmeshed(&cluster);
        let dark = bake.pump(&cluster, std::time::Duration::from_secs(2), 8);
        assert!(!dark.is_empty());
        let dark_block: f32 = dark
            .iter()
            .flat_map(|baked| baked.mesh.vertices.iter())
            .map(|v| v.block[0] + v.block[1] + v.block[2])
            .sum();
        assert!(
            dark_block < 1.0e-3,
            "stone tops stay unlit without a held torch: {dark_block}"
        );

        let torch = rebuild_world::light_of(Block::new("torch", 0)).expect("torch");
        bake.set_held(Some(Emitter {
            x: 3,
            y: 1,
            z: 3,
            light: torch,
        }));
        let lit = bake.pump(&cluster, std::time::Duration::from_secs(2), 8);
        assert!(!lit.is_empty());
        let lit_block: f32 = lit
            .iter()
            .flat_map(|baked| baked.mesh.vertices.iter())
            .map(|v| v.block[0] + v.block[1] + v.block[2])
            .sum();
        assert!(
            lit_block > 1.0,
            "held torch should bake block light into stone faces: {lit_block}"
        );
        let near = lit
            .iter()
            .flat_map(|baked| baked.mesh.vertices.iter())
            .filter(|v| {
                let x = v.position[0];
                let z = v.position[2];
                (x - 3.5).abs() < 1.5 && (z - 3.5).abs() < 1.5 && v.normal[1] > 0.9
            })
            .map(|v| v.block[0])
            .fold(0.0_f32, f32::max);
        let far = lit
            .iter()
            .flat_map(|baked| baked.mesh.vertices.iter())
            .filter(|v| {
                let x = v.position[0];
                let z = v.position[2];
                x > 6.0 && z > 6.0 && v.normal[1] > 0.9
            })
            .map(|v| v.block[0])
            .fold(0.0_f32, f32::max);
        assert!(
            near > far + 0.15,
            "held light should fall off across the floor: near {near} far {far}"
        );
    }

    #[test]
    fn border_face_is_culled_when_the_neighbor_is_solid() {
        let west = Chunk::from_blocks(0, 0, [(15, 0, 0, Block::new("stone", 0))]).unwrap();
        let east = Chunk::from_blocks(1, 0, [(0, 0, 0, Block::new("cobblestone", 0))]).unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [west, east]).unwrap();
        // Six faces each, minus the shared face on both sides.
        assert_eq!(mesh_cluster(&cluster).quad_count(), 10);
    }

    #[test]
    fn open_border_keeps_the_face() {
        let west = Chunk::from_blocks(0, 0, [(15, 0, 0, Block::new("stone", 0))]).unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [west]).unwrap();
        assert_eq!(mesh_cluster(&cluster).quad_count(), 6);
    }

    #[test]
    fn touching_floors_drop_the_shared_edge() {
        let west = floor(0, 0, "grass");
        let east = floor(1, 0, "dirt");
        let west_mesh = mesh_chunk(&west);
        let east_mesh = mesh_chunk(&east);
        let cluster = Cluster::try_from_chunks(0, 0, [west, east]).unwrap();
        let joined = mesh_cluster(&cluster);
        // The grass blades sit on top. The shared wall between the floors is
        // the one that drops.
        assert!(facing_on_plane(&west_mesh, 0, 16.0, 1.0) > 0);
        assert!(facing_on_plane(&east_mesh, 0, 16.0, -1.0) > 0);
        assert_eq!(facing_on_plane(&joined, 0, 16.0, 1.0), 0);
        assert_eq!(facing_on_plane(&joined, 0, 16.0, -1.0), 0);
    }

    #[test]
    fn synthetic_cluster_culls_across_borders() {
        let cluster = synthetic_cluster();
        let light = LightField::solve(&cluster);
        let separate: usize = cluster
            .chunks()
            .iter()
            .map(|chunk| {
                let origin_x = chunk.x() * 16;
                let origin_z = chunk.z() * 16;
                let part = mesh_chunk_with(
                    chunk,
                    |x, y, z| {
                        let local_x = x - origin_x;
                        let local_z = z - origin_z;
                        if (0..16).contains(&local_x)
                            && (0..256).contains(&y)
                            && (0..16).contains(&local_z)
                        {
                            chunk.block(local_x as u8, y as u16, local_z as u8)
                        } else {
                            Block::AIR
                        }
                    },
                    &light,
                    daylight::NOON_SUN,
                );
                part.quad_count()
            })
            .sum();
        let together = mesh_cluster(&cluster).quad_count();
        assert!(together > 0);
        assert!(together < separate);
    }

    fn facing_on_plane(mesh: &Mesh, axis: usize, plane: f32, normal: f32) -> usize {
        mesh.vertices
            .chunks(4)
            .filter(|quad| {
                quad.len() == 4
                    && quad.iter().all(|vertex| {
                        (vertex.position[axis] - plane).abs() < 1e-4
                            && (vertex.normal[axis] - normal).abs() < 1e-4
                    })
            })
            .count()
    }

    #[test]
    fn stair_is_smaller_than_a_cube() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new(OAK_STAIRS, 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let (min, max) = bounds(&mesh);
        assert_eq!(min, [0.0, 0.0, 0.0]);
        assert_eq!(max[0], 1.0);
        assert_eq!(max[1], 1.0);
        assert_eq!(max[2], 1.0);
        assert!(mesh.quad_count() > 6);
        assert!(
            mesh.vertices
                .iter()
                .any(|vertex| (vertex.position[1] - 0.5).abs() < 1e-4)
        );
        // The south half of the cell is only the bottom step.
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position[2] <= 0.5 + 1e-4 || vertex.position[1] <= 0.5 + 1e-4
        }));
    }

    #[test]
    fn cube_face_is_culled_against_the_stair() {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (0, 1, 0, Block::new(OAK_STAIRS, 0)),
            ],
        )
        .unwrap();
        let mesh = mesh_chunk(&chunk);
        let cube =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap());
        let stair =
            mesh_chunk(&Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new(OAK_STAIRS, 0))]).unwrap());
        assert_eq!(facing_on_plane(&cube, 1, 1.0, 1.0), 1);
        assert_eq!(facing_on_plane(&stair, 1, 0.0, -1.0), 1);
        // The stair covers the whole underside, so both of those faces go away.
        assert_eq!(facing_on_plane(&mesh, 1, 1.0, 1.0), 0);
        assert_eq!(facing_on_plane(&mesh, 1, 1.0, -1.0), 0);
        assert!(mesh.quad_count() < cube.quad_count() + stair.quad_count());
    }

    #[test]
    fn stair_side_is_culled_across_the_chunk_border() {
        let cube = Chunk::from_blocks(0, 0, [(15, 1, 0, Block::new("cobblestone", 0))]).unwrap();
        let stair = Chunk::from_blocks(1, 0, [(0, 1, 0, Block::new(OAK_STAIRS, 0))]).unwrap();
        let cube_mesh = mesh_chunk(&cube);
        let stair_mesh = mesh_chunk(&stair);
        let cluster = Cluster::try_from_chunks(0, 0, [cube, stair]).unwrap();
        let mesh = mesh_cluster(&cluster);
        // The stair does not cover the whole side, so the cube keeps its face.
        assert_eq!(facing_on_plane(&cube_mesh, 0, 16.0, 1.0), 1);
        assert_eq!(facing_on_plane(&mesh, 0, 16.0, 1.0), 1);
        // The stair's west face sits on that border and is hidden by the cube.
        assert!(facing_on_plane(&stair_mesh, 0, 16.0, -1.0) > 0);
        assert_eq!(facing_on_plane(&mesh, 0, 16.0, -1.0), 0);
    }

    #[test]
    fn block_defined_only_in_the_pack_is_meshed() {
        let pack = Pack::builtin();
        let (id, shape_name) = pack
            .blocks()
            .find(|(id, name)| {
                *name != "cube"
                    && *name != "stair_north"
                    && !pack.shape_of(Block::new(id, 0)).is_full_cube()
            })
            .expect("pack file defines a block besides cubes and stairs");
        assert_ne!(shape_name, "cube");
        let shape = pack.shape_of(Block::new(id, 0));
        assert!(!shape.is_full_cube());
        let chunk = Chunk::from_blocks(0, 0, [(4, 2, 4, Block::new(id, 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        assert!(mesh.quad_count() > 0);
        let (min, max) = bounds(&mesh);
        let mut box_min = [16u8; 3];
        let mut box_max = [0u8; 3];
        for box_ in shape.boxes() {
            for axis in 0..3 {
                box_min[axis] = box_min[axis].min(box_.min[axis]);
                box_max[axis] = box_max[axis].max(box_.max[axis]);
            }
        }
        let origin = [4.0, 2.0, 4.0];
        for axis in 0..3 {
            let expect_min = origin[axis] + f32::from(box_min[axis]) / 16.0;
            let expect_max = origin[axis] + f32::from(box_max[axis]) / 16.0;
            assert!((min[axis] - expect_min).abs() < 1e-4, "min {axis}");
            assert!((max[axis] - expect_max).abs() < 1e-4, "max {axis}");
        }
        assert!(max[1] - min[1] < 1.0);
    }

    #[test]
    fn a_script_block_meshes_as_a_cube() {
        Pack::builtin()
            .register_block_from("meshkit", "rose_cube", "mesh-script-block")
            .unwrap();
        let id = "meshkit:rose_cube";
        let chunk = Chunk::from_blocks(0, 0, [(2, 1, 2, Block::new(id, 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        assert!(shape_of(Block::new(id, 0)).is_full_cube());
        assert!(mesh.quad_count() > 0);
        let (min, max) = bounds(&mesh);
        assert!((min[0] - 2.0).abs() < 1e-4);
        assert!((max[0] - 3.0).abs() < 1e-4);
        assert!((min[1] - 1.0).abs() < 1e-4);
        assert!((max[1] - 2.0).abs() < 1e-4);
        assert!((min[2] - 2.0).abs() < 1e-4);
        assert!((max[2] - 3.0).abs() < 1e-4);
        assert!(mesh.vertices.iter().all(|vertex| vertex.textured == 0.0));
    }

    #[test]
    fn a_script_block_samples_a_pack_texture() {
        use crate::texture::texture_slot;

        let pack = Pack::builtin();
        pack.register_block_from("meshkit", "woven", "mesh-script-texture")
            .unwrap();
        pack.set_script_texture("meshkit:woven", "wool", "mesh-script-texture")
            .unwrap();
        let id = "meshkit:woven";
        assert_eq!(
            pack.texture_of(Block::new(id, 0)),
            Some("assets/terralite/textures/wool.png")
        );
        let chunk = Chunk::from_blocks(0, 0, [(1, 1, 1, Block::new(id, 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        let slot = texture_slot("assets/terralite/textures/wool.png").unwrap() as f32 + 1.0;
        assert!(
            mesh.vertices
                .iter()
                .any(|vertex| (vertex.textured - slot).abs() < 1e-4)
        );
    }

    #[test]
    fn an_unregistered_block_does_not_mesh() {
        let chunk = Chunk::from_blocks(0, 0, [(3, 1, 3, Block::new("not_registered", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        assert!(shape_of(Block::new("not_registered", 0)).is_empty());
        assert_eq!(mesh.quad_count(), 0);
        assert!(mesh.vertices.is_empty());
    }

    #[test]
    fn sign_model_meshes_the_post_and_a_plane() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("sign", 0))]).unwrap();
        let mesh = mesh_chunk(&chunk);
        assert!(
            mesh.vertices
                .iter()
                .any(|vertex| { (vertex.position[1] - 12.0 / 16.0).abs() < 1e-4 })
        );
        assert!(mesh.vertices.iter().any(|vertex| {
            (vertex.position[2] - 0.5).abs() < 1e-4
                && (vertex.position[0] - 15.0 / 16.0).abs() < 1e-4
                && (vertex.position[1] - 15.0 / 16.0).abs() < 1e-4
        }));
        assert!(mesh.vertices.iter().any(|vertex| {
            (vertex.position[2] - 0.5).abs() < 1e-4
                && (vertex.position[0] - 1.0 / 16.0).abs() < 1e-4
        }));
    }

    fn has_color(mesh: &Mesh, color: [f32; 3]) -> bool {
        mesh.vertices.iter().any(|vertex| {
            (0..3).all(|channel| (vertex.color[channel] - color[channel]).abs() < 1.0e-4)
        })
    }

    #[test]
    fn unlit_grass_at_a_chunk_edge_is_not_tinted() {
        let cluster = synthetic_cluster();
        let light = LightField::solve(&cluster);
        let mesh = mesh_cluster(&cluster);
        let albedo = [0.45, 0.70, 0.30];
        let mut bad = Vec::new();
        for quad in mesh.vertices.chunks(4) {
            if quad.len() != 4 || quad.iter().any(|vertex| vertex.normal[1] < 0.9) {
                continue;
            }
            if (quad[0].position[1] - 1.0).abs() > 1.0e-3 {
                continue;
            }
            let min_x = quad.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
            let max_x = quad.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
            let min_z = quad.iter().map(|v| v.position[2]).fold(f32::MAX, f32::min);
            let max_z = quad.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
            // Grass floor of the center chunk only.
            if max_x <= 0.0 || min_x >= 16.0 || max_z <= 0.0 || min_z >= 16.0 {
                continue;
            }
            let x0 = min_x.ceil() as i32;
            let x1 = max_x.floor() as i32;
            let z0 = min_z.ceil() as i32;
            let z1 = max_z.floor() as i32;
            for z in z0..z1 {
                for x in x0..x1 {
                    if !(0..16).contains(&x) || !(0..16).contains(&z) {
                        continue;
                    }
                    let channels = light.channels(x, 1, z);
                    let expect = apply_light(albedo, channels);
                    if quad[0].color != expect {
                        bad.push((x, z, channels, quad[0].color, expect));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "grass top does not match its cell light: {bad:?}"
        );

        let neutral = apply_light(albedo, [0, 0, 0]);
        let mut zero_tinted = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let channels = light.channels(x, 1, z);
                if channels != [0, 0, 0] {
                    continue;
                }
                let Some(color) = top_color_at(&mesh, x, z) else {
                    continue;
                };
                if color != neutral {
                    zero_tinted.push((x, z, color));
                }
            }
        }
        assert!(
            zero_tinted.is_empty(),
            "unlit grass top picked up a tint: {zero_tinted:?}"
        );
        let mut solid_lit = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let got = light.channels(x, 0, z);
                if got != [0, 0, 0] {
                    solid_lit.push((x, z, got));
                }
            }
        }
        assert!(
            solid_lit.is_empty(),
            "light entered solid grass: {solid_lit:?}"
        );

        let mut wool = Vec::new();
        for z in -1..=1 {
            for x in -1..=1 {
                if x == 0 && z == 0 {
                    continue;
                }
                let mut blocks = Vec::new();
                for lz in 0..16u8 {
                    for lx in 0..16u8 {
                        blocks.push((lx, 0, lz, Block::new("wool", 6)));
                    }
                }
                wool.push(Chunk::from_blocks(x, z, blocks).unwrap());
            }
        }
        let mut grass_blocks = Vec::new();
        for lz in 0..16u8 {
            for lx in 0..16u8 {
                grass_blocks.push((lx, 0, lz, Block::new("grass", 0)));
            }
        }
        wool.push(Chunk::from_blocks(0, 0, grass_blocks).unwrap());
        let dark_cluster = Cluster::try_from_chunks(0, 0, wool).unwrap();
        let dark_light = LightField::solve(&dark_cluster);
        let dark_mesh = mesh_cluster(&dark_cluster);
        let mut dark_bad = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let channels = dark_light.channels(x, 1, z);
                let color = top_color_at(&dark_mesh, x, z);
                if channels != [0, 0, 0] || color != Some(neutral) {
                    dark_bad.push((x, z, channels, color));
                }
            }
        }
        assert!(
            dark_bad.is_empty(),
            "pink wool tinted grass with no light source: {dark_bad:?}"
        );

        let mut red_only = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let channels = light.channels(x, 1, z);
                if channels != [0, 0, 0] && channels[1] == 0 {
                    red_only.push((x, z, channels));
                }
            }
        }
        assert!(
            red_only.is_empty(),
            "lantern fringe on grass lost its green: {red_only:?}"
        );
    }

    #[test]
    fn a_wall_on_the_chunk_edge_keeps_the_lantern_off_the_grass() {
        let grass = Block::new("grass", 0);
        let stone = Block::new("stone", 0);
        let mut near = Vec::new();
        let mut far = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                near.push((x, 0, z, grass));
                far.push((x, 0, z, grass));
            }
            for y in 1..14u16 {
                far.push((0, y, z, stone));
            }
        }
        far.push((2, 1, 8, Block::new("lantern", 0)));
        let cluster = Cluster::try_from_chunks(
            0,
            0,
            [
                Chunk::from_blocks(0, 0, near).unwrap(),
                Chunk::from_blocks(1, 0, far).unwrap(),
            ],
        )
        .unwrap();
        let mesh = mesh_cluster(&cluster);
        let open = apply_light([0.45, 0.70, 0.30], [0, 0, 0]);
        assert_eq!(
            top_color_at(&mesh, 15, 8),
            Some(open),
            "chunk-edge grass took the lantern through the wall"
        );
    }

    #[test]
    fn lantern_tints_nearby_faces_and_far_faces_stay_dark() {
        let grass = Block::new("grass", 0);
        let mut blocks = Vec::new();
        for z in 0..8u8 {
            for x in 0..8u8 {
                blocks.push((x, 0, z, grass));
            }
        }
        blocks.push((1, 1, 1, Block::new("lantern", 0)));
        blocks.push((15, 0, 15, grass));
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        let mesh = mesh_chunk(&chunk);
        let light = LightField::solve_bounds(0, 16, 0, 16, |x, y, z| {
            if (0..16).contains(&x) && (0..256).contains(&y) && (0..16).contains(&z) {
                chunk.block(x as u8, y as u16, z as u8)
            } else {
                Block::AIR
            }
        });
        let face = |x, z| light.channels(x, 1, z);
        let top = [0.45, 0.70, 0.30];
        // The lantern covers the grass under it. The cell to its south would
        // be in the sun's shadow, and the lantern light replaces that shadow.
        let beside_light = face(1, 2);
        let step3_light = face(1, 4);
        let step6_light = face(1, 7);
        assert!(
            beside_light[1] > 0 && step3_light[1] > 0 && step6_light[1] > 0,
            "green died and left a red fringe: {beside_light:?} {step3_light:?} {step6_light:?}"
        );
        let beside = apply_light(top, beside_light);
        let step3 = apply_light(top, step3_light);
        let step6 = apply_light(top, step6_light);
        let dark = apply_light(top, [0, 0, 0]);
        assert_eq!(
            top_color_at(&mesh, 1, 2),
            Some(beside),
            "south of the lantern keeps the orange"
        );
        assert!(has_color(&mesh, beside), "grass beside the lantern");
        assert!(has_color(&mesh, step3), "three blocks out");
        assert!(has_color(&mesh, step6), "six blocks out");
        assert!(has_color(&mesh, dark), "no light");
        assert!(step3[0] > step6[0] && step6[0] > dark[0]);
        assert!(beside[0] > beside[1]);
        assert!(beside[0] > dark[0]);
        assert!(dark[1] > dark[0]);
        assert!(dark[0] < 0.12);
    }

    fn top_color_at(mesh: &Mesh, x: i32, z: i32) -> Option<[f32; 3]> {
        let px = x as f32 + 0.5;
        let pz = z as f32 + 0.5;
        mesh.vertices.chunks(4).find_map(|quad| {
            if quad.len() != 4 {
                return None;
            }
            if !quad
                .iter()
                .all(|vertex| (vertex.position[1] - 1.0).abs() < 1.0e-3 && vertex.normal[1] > 0.9)
            {
                return None;
            }
            let min_x = quad
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::MAX, f32::min);
            let max_x = quad
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::MIN, f32::max);
            let min_z = quad
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::MAX, f32::min);
            let max_z = quad
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::MIN, f32::max);
            if min_x <= px && px <= max_x && min_z <= pz && pz <= max_z {
                Some(quad[0].color)
            } else {
                None
            }
        })
    }

    /// Average of one grass top, so a soft edge and its bounce are one sample.
    fn top_quad_at(mesh: &Mesh, x: i32, z: i32) -> Option<Vertex> {
        let px = x as f32 + 0.5;
        let pz = z as f32 + 0.5;
        mesh.vertices.chunks(4).find_map(|quad| {
            if quad.len() != 4 {
                return None;
            }
            if !quad
                .iter()
                .all(|vertex| (vertex.position[1] - 1.0).abs() < 1.0e-3 && vertex.normal[1] > 0.9)
            {
                return None;
            }
            let min_x = quad
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::MAX, f32::min);
            let max_x = quad
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::MIN, f32::max);
            let min_z = quad
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::MAX, f32::min);
            let max_z = quad
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(f32::MIN, f32::max);
            if min_x <= px && px <= max_x && min_z <= pz && pz <= max_z {
                let mut vertex = quad[0];
                for channel in 0..3 {
                    vertex.bounce[channel] = quad
                        .iter()
                        .map(|corner| corner.bounce[channel])
                        .sum::<f32>()
                        / 4.0;
                }
                Some(vertex)
            } else {
                None
            }
        })
    }

    /// Green channel after the same shadow-map and sky-fill mix the shader uses.
    /// `coverage` is what the shadow map would return for that texel.
    fn shaded_green(vertex: &Vertex, brightness: f32, shadow_strength: f32, coverage: f32) -> f32 {
        let shadow_mul = 1.0 + (0.42 - 1.0) * (coverage * shadow_strength);
        let facing = 0.34 + 0.66 * vertex.normal[1].max(0.0);
        let ambient = vertex.color[1] - vertex.block[1];
        let sky = vertex.ao.clamp(0.0, 1.0);
        let extra = (1.0 - sky) * (1.0 - coverage);
        let cave_mul = 1.0 + (0.16 - 1.0) * extra;
        let indirect = vertex.bounce[1] * brightness * (1.0 - coverage) * sky;
        ambient * facing * brightness * shadow_mul * cave_mul + vertex.block[1] + indirect
    }

    #[test]
    fn ground_under_a_block_stays_darker_than_open_grass() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        blocks.push((1, 1, 1, Block::new("stone", 0)));
        blocks.push((8, 1, 8, Block::new("sign", 0)));
        let mesh = mesh_chunk(&Chunk::from_blocks(0, 0, blocks).unwrap());
        let open = top_quad_at(&mesh, 1, 6).expect("open grass");
        let south = top_quad_at(&mesh, 1, 2).expect("south of the stone");
        let north = top_quad_at(&mesh, 1, 0).expect("north of the stone");
        let under = top_quad_at(&mesh, 8, 8).expect("grass under the sign");
        for (brightness, strength, label) in [(1.0, 1.0, "day"), (0.22, 0.55, "night")] {
            // The shadow map covers the sign's grass and the stone's south side.
            let open_lit = shaded_green(&open, brightness, strength, 0.0);
            let south_lit = shaded_green(&south, brightness, strength, 1.0);
            let north_lit = shaded_green(&north, brightness, strength, 0.0);
            let under_lit = shaded_green(&under, brightness, strength, 1.0);
            assert!(
                under_lit < open_lit,
                "{label}: grass under the sign is brighter than open grass ({under_lit} vs {open_lit})"
            );
            assert!(
                south_lit < open_lit,
                "{label}: the stone's shadow is brighter than open grass ({south_lit} vs {open_lit})"
            );
            assert!(
                north_lit > south_lit,
                "{label}: the lit side of the stone is darker than its shadow ({north_lit} vs {south_lit})"
            );
        }
    }

    #[test]
    fn a_sun_step_does_not_retile_the_grass() {
        let mut blocks = Vec::new();
        for z in 0..8u8 {
            for x in 0..8u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        blocks.push((3, 1, 3, Block::new("oak_log", 0)));
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        let mesh_at = |sun: [f32; 3]| {
            let origin_x = chunk.x() * 16;
            let origin_z = chunk.z() * 16;
            let sample = |x: i32, y: i32, z: i32| {
                let local_x = x - origin_x;
                let local_z = z - origin_z;
                if (0..16).contains(&local_x) && (0..256).contains(&y) && (0..16).contains(&local_z)
                {
                    chunk.block(local_x as u8, y as u16, local_z as u8)
                } else {
                    Block::AIR
                }
            };
            let light = rebuild_world::LightField::solve_bounds(
                origin_x,
                origin_x + 16,
                origin_z,
                origin_z + 16,
                &sample,
            );
            mesh_chunk_with(&chunk, &sample, &light, sun)
        };
        let noon = mesh_at(daylight::NOON_SUN);
        let later = mesh_at([0.55, 0.45, -0.25]);
        assert_eq!(
            noon.indices, later.indices,
            "a sun step keeps the ground triangles"
        );
        assert_eq!(noon.vertices.len(), later.vertices.len());
    }

    #[test]
    fn a_shadow_bake_spreads_across_steps() {
        let cluster = synthetic_cluster();
        let mut job = MeshJob::start(&cluster, daylight::NOON_SUN, None);
        let mut steps = 0;
        loop {
            steps += 1;
            assert!(steps < 64, "the bake did not finish");
            if job.pump(&cluster, std::time::Duration::ZERO) {
                break;
            }
        }
        assert!(
            steps > 1,
            "a zero budget still finishes one chunk, then stops"
        );
        let baked = job.into_mesh();
        let full = {
            let mut once = MeshJob::start(&cluster, daylight::NOON_SUN, None);
            assert!(once.pump(&cluster, std::time::Duration::from_secs(60)));
            once.into_mesh()
        };
        assert_eq!(baked.vertices.len(), full.vertices.len());
        assert_eq!(baked.indices.len(), full.indices.len());
    }

    #[test]
    fn a_far_chunk_collapses_and_a_near_chunk_stays_full() {
        let near = pillar_chunk(0, 0);
        let far = pillar_chunk(6, 0);
        let full_near = mesh_chunk(&near).quad_count();
        let full_far = mesh_chunk(&far).quad_count();
        let cluster = Cluster::try_from_chunks(0, 0, [near, far]).unwrap();
        let mesh = mesh_around(&cluster, daylight::NOON_SUN, [0, 0]);
        let near_quads = quads_between(&mesh, -0.1, 16.1);
        let far_quads = quads_between(&mesh, 95.9, 112.1);
        assert_eq!(near_quads, full_near, "the camera chunk stays full detail");
        assert!(
            far_quads < full_far / 4,
            "chunk (6, 0) should be a coarse mesh: {far_quads} vs {full_far}"
        );
    }

    #[test]
    fn the_middle_ring_keeps_a_ridge_the_far_ring_flattens() {
        let mid = ridge_chunk(4, 0);
        let far = ridge_chunk(8, 0);
        let cluster = Cluster::try_from_chunks(0, 0, [mid, far]).unwrap();
        let mesh = mesh_around(&cluster, daylight::NOON_SUN, [0, 0]);
        let mid_quads = quads_between(&mesh, 64.0 - 0.1, 80.0 + 0.1);
        let far_quads = quads_between(&mesh, 128.0 - 0.1, 144.0 + 0.1);
        assert!(
            mid_quads > far_quads,
            "a 2-block ring should keep more of the ridge than a 4-block ring: {mid_quads} vs {far_quads}"
        );
    }

    #[test]
    fn walking_away_holds_full_detail_for_one_chunk() {
        let held = pillar_chunk(3, 0);
        let full = mesh_chunk(&held).quad_count();
        let cluster = Cluster::try_from_chunks(0, 0, [pillar_chunk(0, 0), held]).unwrap();
        let dropped = mesh_around(&cluster, daylight::NOON_SUN, [0, 0]);
        let dropped_quads = quads_between(&dropped, 48.0 - 0.1, 64.0 + 0.1);
        assert!(
            dropped_quads < full,
            "three chunks out is the 2-block ring, not full detail"
        );
        let mut job = MeshJob::resume(&cluster, daylight::NOON_SUN, Some([0, 0]), Some([1, 0]));
        assert!(job.pump(&cluster, std::time::Duration::from_secs(60)));
        let kept = job.into_mesh();
        let kept_quads = quads_between(&kept, 48.0 - 0.1, 64.0 + 0.1);
        assert_eq!(
            kept_quads, full,
            "the chunk was full at the previous camera and stays full one chunk farther"
        );
    }

    #[test]
    fn an_edit_or_a_sun_step_does_not_rebake_the_square() {
        let near = pillar_chunk(0, 0);
        let held = pillar_chunk(3, 0);
        let far = pillar_chunk(8, 0);
        let full_far = mesh_chunk(&far).quad_count();
        let cluster = Cluster::try_from_chunks(0, 0, [near, held, far]).unwrap();
        let mut bake = ChunkBake::new(&cluster, daylight::NOON_SUN, [1, 0]);
        bake.queue_unmeshed(&cluster);
        assert_eq!(bake.pending(), 3, "only the solid chunks are queued");
        let first = bake.pump(&cluster, std::time::Duration::ZERO, 1);
        assert_eq!(first.len(), 1, "a frame bakes one chunk and then stops");
        assert_eq!(bake.pending(), 2);
        let mut parts = first;
        let mut guard = 0;
        while bake.pending() > 0 {
            parts.extend(bake.pump(&cluster, std::time::Duration::from_secs(60), 1));
            guard += 1;
            assert!(guard < 8);
        }
        bake.queue_unmeshed(&cluster);
        assert_eq!(bake.pending(), 0, "ground that is already meshed stays put");
        let far_part = parts.iter().find(|part| part.x == 8).expect("far chunk");
        assert!(
            far_part.mesh.quad_count() < full_far / 4,
            "chunk (8, 0) stays on the coarse ring"
        );
        bake.retarget(&cluster, daylight::NOON_SUN, [0, 0]);
        assert_eq!(
            bake.pending(),
            0,
            "walking away holds the finer mesh and does not rebuild the square"
        );
        let later_sun = [0.2, 0.9, 0.1];
        bake.retarget(&cluster, later_sun, bake.focus());
        let mut sun_chunks = Vec::new();
        while bake.pending() > 0 {
            sun_chunks.extend(bake.pump(&cluster, std::time::Duration::ZERO, 1));
        }
        let mut sun_x: Vec<_> = sun_chunks.iter().map(|part| part.x).collect();
        sun_x.sort();
        assert_eq!(
            sun_x,
            vec![0, 3],
            "a sun step rebakes the full-detail ring only"
        );
        bake.touch(8 * 16 + 4, 4);
        assert_eq!(bake.pending(), 1, "an interior edit is that chunk alone");
        assert!(bake.has_urgent());
        let edited = bake.pump(&cluster, std::time::Duration::ZERO, 1);
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0].x, 8);
        assert_eq!(bake.pending(), 0);
        bake.touch(8 * 16, 4);
        assert_eq!(
            bake.pending(),
            2,
            "a face on the chunk border also refreshes the neighbor"
        );
        let mut border = Vec::new();
        let mut steps = 0;
        while bake.pending() > 0 {
            border.extend(bake.pump(&cluster, std::time::Duration::ZERO, 1));
            steps += 1;
        }
        assert_eq!(steps, 2, "the border edit still costs one chunk per frame");
        assert_eq!(
            border
                .iter()
                .filter(|part| !part.mesh.indices.is_empty())
                .count(),
            1,
            "the missing neighbor is not a second world bake"
        );
    }

    #[test]
    fn a_worker_bake_matches_the_frame_and_a_stale_epoch_drops() {
        let cluster = Cluster::try_from_chunks(0, 0, [pillar_chunk(0, 0)]).unwrap();
        let mut sync = ChunkBake::new(&cluster, daylight::NOON_SUN, [0, 0]);
        sync.queue_unmeshed(&cluster);
        let sync_mesh = sync.pump(&cluster, std::time::Duration::from_secs(60), 1);
        assert_eq!(sync_mesh.len(), 1);

        let mut bake = ChunkBake::new(&cluster, daylight::NOON_SUN, [0, 0]);
        bake.queue_unmeshed(&cluster);
        let work = bake.take_work(&cluster).expect("chunk");
        let epoch = work.epoch;
        let done = std::thread::spawn(move || work.bake())
            .join()
            .expect("worker");
        let baked = bake.accept(epoch, done.baked).expect("current epoch");
        assert_eq!(baked.mesh.indices, sync_mesh[0].mesh.indices);
        assert_eq!(baked.mesh.vertices.len(), sync_mesh[0].mesh.vertices.len());
        assert!(
            baked.mesh.vertices.iter().any(|vertex| {
                vertex.bounce[0] > 0.0 || vertex.bounce[1] > 0.0 || vertex.bounce[2] > 0.0
            }),
            "full detail still stores a bounce"
        );

        let mut again = ChunkBake::new(&cluster, daylight::NOON_SUN, [0, 0]);
        again.queue_unmeshed(&cluster);
        let work = again.take_work(&cluster).expect("chunk");
        let epoch = work.epoch;
        again.touch(4, 4);
        let done = work.bake();
        assert!(again.accept(epoch, done.baked).is_none());
        assert!(again.pending() >= 1, "the edit is baked again");
    }

    #[test]
    fn a_lod_border_keeps_the_full_detail_height() {
        fn flat(x: i32, top: u16) -> Chunk {
            let mut blocks = Vec::new();
            for gz in 0..16u8 {
                for gx in 0..16u8 {
                    for y in 1..=top {
                        blocks.push((gx, y, gz, Block::new("grass", 0)));
                    }
                }
            }
            Chunk::from_blocks(x, 0, blocks).unwrap()
        }
        // Chunk (2, 0) is full detail. Chunk (3, 0) is the 2-block ring.
        // Both columns stop at y = 4, so the real top is y = 5. A coarse cell
        // would inflate that to y = 6 and hide the side of the step.
        let cluster = Cluster::try_from_chunks(0, 0, [flat(2, 4), flat(3, 4)]).unwrap();
        let mesh = mesh_around(&cluster, daylight::NOON_SUN, [0, 0]);
        let border_top = mesh.vertices.iter().any(|vertex| {
            vertex.normal[1] > 0.9
                && (48.0..50.0).contains(&vertex.position[0])
                && (vertex.position[1] - 5.0).abs() < 0.05
        });
        assert!(
            border_top,
            "the first LOD blocks stay at the real grass height"
        );
        let inflated = mesh.vertices.iter().any(|vertex| {
            vertex.normal[1] > 0.9
                && (48.0..50.0).contains(&vertex.position[0])
                && vertex.position[1] > 5.5
        });
        assert!(!inflated, "the border strip does not grow a coarse step");
        let seal_low = mesh.vertices.iter().any(|vertex| {
            vertex.normal[0].abs() > 0.9
                && (vertex.position[0] - 50.0).abs() < 0.1
                && (vertex.position[1] - 5.0).abs() < 0.05
        });
        let seal_high = mesh.vertices.iter().any(|vertex| {
            vertex.normal[0].abs() > 0.9
                && (vertex.position[0] - 50.0).abs() < 0.1
                && (vertex.position[1] - 6.0).abs() < 0.05
        });
        assert!(
            seal_low && seal_high,
            "the step where the coarse interior is taller stays closed"
        );
    }

    #[test]
    fn a_single_stone_does_not_replace_the_grass_in_a_coarse_cell() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 1, z, Block::new("grass", 0)));
            }
        }
        blocks.push((1, 3, 1, Block::new("stone", 0)));
        let chunk = Chunk::from_blocks(8, 0, blocks).unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let mesh = mesh_around(&cluster, daylight::NOON_SUN, [0, 0]);
        let top = mesh
            .vertices
            .iter()
            .find(|vertex| {
                vertex.normal[1] > 0.9
                    && (vertex.position[1] - 4.0).abs() < 0.05
                    && (128.0..132.0).contains(&vertex.position[0])
                    && (0.0..4.0).contains(&vertex.position[2])
            })
            .expect("coarse grass top");
        assert!(
            top.color[1] > top.color[0],
            "the cell should stay grass, not turn into the one stone: {:?}",
            top.color
        );
    }

    fn ridge_chunk(x: i32, z: i32) -> Chunk {
        let mut blocks = Vec::new();
        for gz in 0..16u8 {
            for gx in 0..16u8 {
                let height = 1 + u16::from(gx % 4);
                for y in 1..=height {
                    blocks.push((gx, y, gz, Block::new("stone", 0)));
                }
            }
        }
        Chunk::from_blocks(x, z, blocks).unwrap()
    }

    fn pillar_chunk(x: i32, z: i32) -> Chunk {
        let mut blocks = Vec::new();
        for gz in (0..16u8).step_by(2) {
            for gx in (0..16u8).step_by(2) {
                for y in 1..8u16 {
                    blocks.push((gx, y, gz, Block::new("stone", 0)));
                }
            }
        }
        Chunk::from_blocks(x, z, blocks).unwrap()
    }

    fn quads_between(mesh: &Mesh, min_x: f32, max_x: f32) -> usize {
        mesh.vertices
            .iter()
            .filter(|vertex| (min_x..=max_x).contains(&vertex.position[0]))
            .count()
            / 4
    }

    #[test]
    fn corners_where_blocks_meet_are_darker() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        blocks.push((8, 1, 8, Block::new("stone", 0)));
        blocks.push((7, 1, 9, Block::new("stone", 0)));
        blocks.push((4, 1, 4, Block::new(OAK_STAIRS, 0)));
        let mesh = mesh_chunk(&Chunk::from_blocks(0, 0, blocks).unwrap());
        let crease = ao_at(&mesh, 8.0, 1.0, 9.0);
        let open = ao_at(&mesh, 9.0, 1.0, 10.0);
        assert!(
            (0.55..0.95).contains(&crease),
            "the inner corner is a soft contact shade, not a hard shadow: {crease}"
        );
        assert!(
            open > crease,
            "the far corner of that grass face stays lighter: {open} vs {crease}"
        );
        let by_stair = mesh
            .vertices
            .iter()
            .filter(|vertex| {
                vertex.normal[1] > 0.9
                    && (vertex.position[1] - 1.0).abs() < 0.05
                    && (4.0..=5.0).contains(&vertex.position[0])
                    && (vertex.position[2] - 5.0).abs() < 0.05
            })
            .map(|vertex| vertex.ao)
            .fold(1.0_f32, f32::min);
        assert!(
            by_stair < 0.99,
            "grass against the stair should darken a little: {by_stair}"
        );
    }

    #[test]
    fn a_cave_loses_the_sky_and_a_cast_shadow_stays_on_the_map() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        for z in 2..6u8 {
            for x in 2..6u8 {
                blocks.push((x, 2, z, Block::new("stone", 0)));
            }
        }
        for z in 2..6u8 {
            blocks.push((2, 1, z, Block::new("stone", 0)));
            blocks.push((5, 1, z, Block::new("stone", 0)));
        }
        for x in 3..5u8 {
            blocks.push((x, 1, 2, Block::new("stone", 0)));
            blocks.push((x, 1, 5, Block::new("stone", 0)));
        }
        let mesh = mesh_chunk(&Chunk::from_blocks(0, 0, blocks).unwrap());
        let open = top_quad_at(&mesh, 12, 12).expect("open grass");
        let cave = top_quad_at(&mesh, 3, 3).expect("cave floor");
        assert!(cave.ao < 0.62, "the cave floor loses the sky: {}", cave.ao);
        assert!(open.ao > 0.9, "open grass still sees the sky: {}", open.ao);
        let open_lit = shaded_green(&open, 1.0, 1.0, 0.0);
        let cave_lit = shaded_green(&cave, 1.0, 1.0, 0.0);
        assert!(
            cave_lit < open_lit * 0.8,
            "a cave the shadow map missed is still darker: {cave_lit} vs {open_lit}"
        );
        let shadowed = shaded_green(&open, 1.0, 1.0, 1.0);
        let sunlit = shaded_green(&open, 1.0, 1.0, 0.0);
        assert!(
            shadowed < sunlit,
            "open grass in a cast shadow is darker than the same grass in the sun"
        );
    }

    fn ao_at(mesh: &Mesh, x: f32, y: f32, z: f32) -> f32 {
        mesh.vertices
            .iter()
            .find(|vertex| {
                vertex.normal[1] > 0.9
                    && (vertex.position[0] - x).abs() < 0.05
                    && (vertex.position[1] - y).abs() < 0.05
                    && (vertex.position[2] - z).abs() < 0.05
            })
            .map(|vertex| vertex.ao)
            .unwrap_or_else(|| panic!("no grass corner at {x},{y},{z}"))
    }

    #[test]
    fn a_shaded_face_picks_up_green_from_the_grass() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        blocks.push((8, 1, 8, Block::new("stone", 0)));
        let mesh = mesh_chunk(&Chunk::from_blocks(0, 0, blocks).unwrap());
        let bounce = mesh
            .vertices
            .iter()
            .find(|vertex| {
                vertex.normal[2] > 0.9
                    && (vertex.position[2] - 9.0).abs() < 0.05
                    && (8.0..=9.0).contains(&vertex.position[0])
                    && (1.0..=2.0).contains(&vertex.position[1])
            })
            .map(|vertex| vertex.bounce)
            .expect("south face of the stone");
        assert!(
            bounce[1] > bounce[0] && bounce[1] > 0.02,
            "the shaded stone face should pick up grass: {bounce:?}"
        );
    }

    #[test]
    fn grass_stone_and_ores_sample_their_own_pngs() {
        use crate::texture::texture_slot;

        let placed = [
            (0u8, 0u8, "grass", "assets/terralite/textures/grass.png"),
            (2, 0, "stone", "assets/terralite/textures/stone.png"),
            (
                4,
                0,
                "cobblestone",
                "assets/terralite/textures/cobblestone.png",
            ),
            (6, 0, "coal_ore", "assets/terralite/textures/coal_ore.png"),
            (8, 0, "iron_ore", "assets/terralite/textures/iron_ore.png"),
            (
                10,
                0,
                "deepslate",
                "assets/terralite/textures/deepslate.png",
            ),
            (12, 0, "dirt", "assets/terralite/textures/block/dirt.png"),
            (0, 2, "oak_log", "assets/terralite/textures/oak_log.png"),
            (2, 2, "wool", "assets/terralite/textures/wool.png"),
            (
                4,
                2,
                "oak_stairs",
                "assets/terralite/textures/oak_stairs.png",
            ),
            (6, 2, "sign", "assets/terralite/textures/sign.png"),
            (8, 2, "lantern", "assets/terralite/textures/lantern.png"),
            (10, 2, "marker", ""),
        ];
        let blocks: Vec<_> = placed
            .iter()
            .map(|(x, y, id, _)| (*x, u16::from(*y), 0u8, Block::new(id, 0)))
            .collect();
        let mesh = mesh_chunk(&Chunk::from_blocks(0, 0, blocks).unwrap());
        let mut seen = Vec::new();
        for (x, y, _id, path) in placed {
            let x = f32::from(x);
            let y = f32::from(y);
            let face: Vec<_> = mesh
                .vertices
                .iter()
                .filter(|vertex| {
                    vertex.position[0] >= x
                        && vertex.position[0] <= x + 1.0
                        && vertex.position[1] >= y
                        && vertex.position[1] <= y + 1.0
                })
                .collect();
            assert!(!face.is_empty(), "missing block at {x}");
            let textured = if path.is_empty() {
                0.0
            } else {
                texture_slot(path).unwrap() as f32 + 1.0
            };
            if _id == "grass" {
                let top = texture_slot("assets/terralite/textures/block/grass_block_top.png")
                    .unwrap() as f32
                    + 1.0;
                let dirt =
                    texture_slot("assets/terralite/textures/block/dirt.png").unwrap() as f32 + 1.0;
                let overlay =
                    texture_slot("assets/terralite/textures/block/grass_block_side_overlay.png")
                        .unwrap() as f32
                        + 1.0;
                assert!(
                    face.iter().all(|vertex| {
                        (vertex.textured - top).abs() < 1.0e-3
                            || (vertex.textured - dirt).abs() < 1.0e-3
                            || (vertex.textured - overlay).abs() < 1.0e-3
                    }),
                    "grass face missed the model textures"
                );
                assert!(
                    face.iter()
                        .any(|vertex| (vertex.textured - overlay).abs() < 1.0e-3),
                    "grass fringe did not bind the overlay"
                );
                continue;
            }
            if _id == "dirt" {
                let choices = [
                    "assets/terralite/textures/block/dirt.png",
                    "assets/terralite/textures/block/dirt_alt_1.png",
                    "assets/terralite/textures/block/dirt_alt_2.png",
                    "assets/terralite/textures/block/dirt_alt_3.png",
                ];
                let slots: Vec<f32> = choices
                    .iter()
                    .map(|path| texture_slot(path).unwrap() as f32 + 1.0)
                    .collect();
                assert!(
                    face.iter().all(|vertex| {
                        slots
                            .iter()
                            .any(|slot| (vertex.textured - slot).abs() < 1.0e-3)
                    }),
                    "dirt missed the block textures"
                );
                let chosen = face[0].textured;
                assert!(
                    face.iter()
                        .all(|vertex| (vertex.textured - chosen).abs() < 1.0e-3),
                    "one dirt block should keep one texture"
                );
                let min_u = face
                    .iter()
                    .map(|vertex| vertex.uv[0])
                    .fold(f32::MAX, f32::min);
                let max_u = face
                    .iter()
                    .map(|vertex| vertex.uv[0])
                    .fold(f32::MIN, f32::max);
                assert!(max_u - min_u > 0.5, "dirt face should show the whole tile");
                continue;
            }
            assert!(
                face.iter().all(|vertex| vertex.textured == textured),
                "{path} textured {}",
                face[0].textured
            );
            if !path.is_empty() {
                assert!(seen.iter().all(|other| *other != textured));
                seen.push(textured);
            }
        }
        let grass: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|vertex| {
                vertex.position[0] <= 1.0 && vertex.position[1] <= 1.0 && vertex.normal[1] > 0.9
            })
            .collect();
        assert!(grass.len() >= 4);
        let min_u = grass
            .iter()
            .map(|vertex| vertex.uv[0])
            .fold(f32::MAX, f32::min);
        let max_u = grass
            .iter()
            .map(|vertex| vertex.uv[0])
            .fold(f32::MIN, f32::max);
        assert!(max_u - min_u > 0.5, "grass face has no texture span");
    }
}
