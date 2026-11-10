//! Session / world statistics for the pause screen.
//!
//! Counters accumulate while playing and persist on the world save under
//! `"statistics": { ... }`.

/// One labeled row on the statistics screen.
#[derive(Clone, Debug)]
pub struct StatRow {
    pub label: &'static str,
    pub value: String,
}

/// Blocks / walk / kills / time / deaths for this world.
#[derive(Clone, Debug, Default)]
pub struct Statistics {
    pub blocks_broken: u64,
    pub blocks_placed: u64,
    /// Horizontal blocks walked (XZ).
    pub distance_walked: f64,
    pub mobs_killed: u64,
    /// Seconds spent in the world (including pause).
    pub time_played: f64,
    pub deaths: u64,
    dirty: bool,
    /// Seconds of play time since the last dirty mark (throttle world writes).
    time_since_dirty: f64,
    /// Last eye XZ used for distance (None until first tick in world).
    last_xz: Option<(f64, f64)>,
}

impl Statistics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }

    pub fn note_break(&mut self) {
        self.blocks_broken = self.blocks_broken.saturating_add(1);
        self.mark_dirty();
    }

    pub fn note_place(&mut self) {
        self.blocks_placed = self.blocks_placed.saturating_add(1);
        self.mark_dirty();
    }

    pub fn note_kill(&mut self) {
        self.mobs_killed = self.mobs_killed.saturating_add(1);
        self.mark_dirty();
    }

    pub fn note_death(&mut self) {
        self.deaths = self.deaths.saturating_add(1);
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.time_since_dirty = 0.0;
    }

    /// Accumulate play time and horizontal walk distance.
    pub fn tick_play(&mut self, dt: f32, eye_x: f32, eye_z: f32, walking: bool) {
        let dt = dt.max(0.0) as f64;
        if dt > 0.0 {
            self.time_played += dt;
            self.time_since_dirty += dt;
            // Persist time every few seconds so a broken aside does not spam writes.
            if self.time_since_dirty >= 5.0 {
                self.mark_dirty();
            }
        }
        let xz = (eye_x as f64, eye_z as f64);
        if let Some((px, pz)) = self.last_xz {
            if walking {
                let dx = xz.0 - px;
                let dz = xz.1 - pz;
                let step = (dx * dx + dz * dz).sqrt();
                // Ignore teleports / huge frame jumps.
                if step > 0.001 && step < 8.0 {
                    self.distance_walked += step;
                    self.mark_dirty();
                }
            }
        }
        self.last_xz = Some(xz);
    }

    pub fn clear_walk_anchor(&mut self) {
        self.last_xz = None;
    }

    pub fn install(&mut self, other: &Statistics) {
        self.blocks_broken = other.blocks_broken;
        self.blocks_placed = other.blocks_placed;
        self.distance_walked = other.distance_walked;
        self.mobs_killed = other.mobs_killed;
        self.time_played = other.time_played;
        self.deaths = other.deaths;
        self.dirty = false;
        self.time_since_dirty = 0.0;
        self.last_xz = None;
    }

    /// Rows for the egui statistics screen.
    pub fn screen_rows(&self) -> Vec<StatRow> {
        vec![
            StatRow {
                label: "Blocks broken",
                value: self.blocks_broken.to_string(),
            },
            StatRow {
                label: "Blocks placed",
                value: self.blocks_placed.to_string(),
            },
            StatRow {
                label: "Distance walked",
                value: format_distance(self.distance_walked),
            },
            StatRow {
                label: "Mobs killed",
                value: self.mobs_killed.to_string(),
            },
            StatRow {
                label: "Time played",
                value: format_time(self.time_played),
            },
            StatRow {
                label: "Deaths",
                value: self.deaths.to_string(),
            },
        ]
    }
}

fn format_distance(blocks: f64) -> String {
    if blocks < 10.0 {
        format!("{blocks:.1} m")
    } else if blocks < 1000.0 {
        format!("{:.0} m", blocks.round())
    } else {
        format!("{:.2} km", blocks / 1000.0)
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_and_time_accumulate() {
        let mut stats = Statistics::new();
        stats.tick_play(1.0, 0.0, 0.0, true);
        stats.tick_play(1.0, 3.0, 4.0, true);
        assert!((stats.distance_walked - 5.0).abs() < 0.01);
        assert!((stats.time_played - 2.0).abs() < 0.01);
        assert!(stats.take_dirty());
    }

    #[test]
    fn time_dirt_throttles() {
        let mut stats = Statistics::new();
        stats.tick_play(2.0, 0.0, 0.0, false);
        assert!(!stats.take_dirty());
        stats.tick_play(3.0, 0.0, 0.0, false);
        assert!(stats.take_dirty());
    }

    #[test]
    fn teleports_do_not_inflate_distance() {
        let mut stats = Statistics::new();
        stats.tick_play(0.1, 0.0, 0.0, true);
        stats.tick_play(0.1, 100.0, 0.0, true);
        assert!(stats.distance_walked < 0.01);
    }

    #[test]
    fn screen_rows_format() {
        let mut stats = Statistics::new();
        stats.note_break();
        stats.note_place();
        stats.note_kill();
        stats.note_death();
        stats.distance_walked = 12.0;
        stats.time_played = 65.0;
        let rows = stats.screen_rows();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].value, "1");
        assert!(rows[4].value.contains('m'));
    }
}
