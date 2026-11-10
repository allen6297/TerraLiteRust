//! Items left where a block broke.
//!
//! A drop is a tile from the pack atlas that faces the camera eye. The mesh
//! is rebuilt every frame, so a step or a turn cannot leave it edge-on. It
//! is not saved and does not enter the hotbar until the player walks into
//! it. A full hotbar leaves it where it is, unless the caller also offers
//! the inventory. Nearby loot gently homes toward the player after a short
//! settle (tunable magnet — not a whole-chunk vacuum). Sparks stay small
//! untextured cubes.

use rebuild_world::Block;

use crate::hotbar::Hotbar;
use crate::mesh::Vertex;
use crate::palette;

/// Half the width of a dropped item, in blocks. Pickup uses the same reach.
const HALF: f32 = 0.12;
/// Settle time before a drop starts homing (seconds).
pub const MAGNET_DELAY: f32 = 0.45;
/// How close the player must be before loot pulls in (blocks).
pub const MAGNET_RANGE: f32 = 2.75;
/// Homing speed while magnetized (blocks / second).
pub const MAGNET_SPEED: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub id: &'static str,
    pub center: [f32; 3],
    /// A pickup hook already refused this drop while the player is touching it.
    refused: bool,
    /// Seconds since spawn — gates the magnet delay.
    age: f32,
}

struct Mote {
    center: [f32; 3],
    velocity: [f32; 3],
    /// Lit color (ambient + block). Sparks use the same value for both.
    color: [f32; 3],
    /// Block-light channel. Smoke keeps a soft ambient (`color - block`).
    block: [f32; 3],
    life: f32,
    /// Soft campfire smoke floats up instead of falling.
    rise: bool,
    /// Dusk firefly: drifts and bobs, no gravity.
    hover: bool,
    /// Forest leaf: tumbles down with a soft sway.
    tumble: bool,
    /// Cold breath frost puff: drifts forward from the mouth, then fades.
    frost: bool,
    /// Snow footprint: sits on the snow layer and fades.
    print: bool,
    half: f32,
}

#[derive(Default)]
pub struct Drops {
    items: Vec<Drop>,
    motes: Vec<Mote>,
}

impl Drops {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            motes: Vec::new(),
        }
    }

    /// Chips from a broken block. They fly out and fade faster than a spark.
    pub fn puff(&mut self, origin: [f32; 3], color: [f32; 3]) {
        const DIRS: [[f32; 3]; 8] = [
            [1.6, 2.2, 0.4],
            [-1.8, 1.8, 0.2],
            [0.4, 2.6, -1.5],
            [-0.3, 1.4, -1.7],
            [1.4, 2.0, 1.3],
            [-1.5, 2.4, 1.1],
            [0.2, 3.0, 0.2],
            [-0.6, 1.6, -0.4],
        ];
        for dir in DIRS {
            self.motes.push(Mote {
                center: origin,
                velocity: dir,
                color,
                block: color,
                life: 0.35,
                rise: false,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.04,
            });
        }
    }

    /// A short rising burst. Commands and a registered particle use this.
    pub fn burst(&mut self, origin: [f32; 3], color: [f32; 3]) {
        const DIRS: [[f32; 3]; 6] = [
            [0.4, 1.4, 0.1],
            [-0.5, 1.1, 0.2],
            [0.2, 1.6, -0.4],
            [-0.2, 1.0, -0.5],
            [0.6, 1.3, 0.4],
            [-0.35, 1.5, 0.25],
        ];
        for dir in DIRS {
            self.motes.push(Mote {
                center: origin,
                velocity: dir,
                color,
                block: color,
                life: 0.7,
                rise: false,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.04,
            });
        }
    }

    /// How many dusk fireflies are still drifting.
    pub fn firefly_count(&self) -> usize {
        self.motes
            .iter()
            .filter(|mote| mote.hover && !mote.rise && !mote.tumble && !mote.frost)
            .count()
    }

    /// How many cold-breath frost puffs are still in the air.
    pub fn frost_breath_count(&self) -> usize {
        self.motes.iter().filter(|mote| mote.frost).count()
    }

    /// A short pale puff from the player's mouth in the cold. `forward` is the
    /// look direction so the cloud drifts into view. Capped so the air stays quiet.
    pub fn frost_breath(&mut self, origin: [f32; 3], forward: [f32; 3]) {
        const MAX_FROST: usize = 24;
        if self.frost_breath_count() >= MAX_FROST {
            return;
        }
        let fx = forward[0];
        let fy = forward[1];
        let fz = forward[2];
        // Pale cyan-white; block channel keeps them lit at night.
        const PUFFS: [([f32; 3], [f32; 3], [f32; 3], f32); 3] = [
            (
                [0.18, 0.22, 0.06],
                [0.95, 0.98, 1.0],
                [0.78, 0.86, 0.95],
                0.95,
            ),
            (
                [0.12, 0.16, -0.05],
                [0.90, 0.95, 1.0],
                [0.72, 0.80, 0.92],
                0.82,
            ),
            (
                [0.22, 0.28, 0.04],
                [1.0, 1.0, 1.0],
                [0.82, 0.90, 1.0],
                1.05,
            ),
        ];
        let seed = ((origin[0] * 4.1 + origin[2] * 2.3).abs() as usize) % PUFFS.len();
        let n = 3;
        let mut count = self.frost_breath_count();
        for i in 0..n {
            if count >= MAX_FROST {
                break;
            }
            let (jitter, color, block, life) = PUFFS[(seed + i) % PUFFS.len()];
            let wobble = (i as f32) * 0.04;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.04,
                    origin[1] + (i as f32) * 0.02,
                    origin[2] - wobble * 0.4,
                ],
                velocity: [
                    fx * 0.48 + jitter[0] * 0.35,
                    fy * 0.28 + jitter[1],
                    fz * 0.48 + jitter[2] * 0.35,
                ],
                color,
                block,
                life,
                rise: false,
                hover: false,
                tumble: false,
                frost: true, print: false,
                half: 0.06,
            });
            count += 1;
        }
    }

    /// A stronger exhale after a sprint stop — a few more motes, same cap.
    pub fn frost_breath_burst(&mut self, origin: [f32; 3], forward: [f32; 3]) {
        for _ in 0..2 {
            self.frost_breath(origin, forward);
        }
    }

    /// How many snow footprints are still pressed into the snow.
    pub fn snow_print_count(&self) -> usize {
        self.motes.iter().filter(|mote| mote.print).count()
    }

    /// A short-lived dark print on a snow layer. `origin` should sit just above
    /// the snow top. Capped so a long walk does not fill the mote pool.
    pub fn snow_print(&mut self, origin: [f32; 3]) {
        const MAX_PRINTS: usize = 40;
        if self.snow_print_count() >= MAX_PRINTS {
            return;
        }
        let seed = ((origin[0] * 7.3 + origin[2] * 5.1).abs() as usize) % 3;
        // Cool slate — dark enough to read on white snow in daylight.
        let (color, block, life, half) = match seed {
            0 => ([0.42, 0.48, 0.58], [0.28, 0.34, 0.44], 6.4, 0.11),
            1 => ([0.38, 0.44, 0.54], [0.24, 0.30, 0.40], 5.8, 0.10),
            _ => ([0.46, 0.52, 0.62], [0.32, 0.38, 0.48], 7.0, 0.115),
        };
        self.motes.push(Mote {
            center: origin,
            velocity: [0.0, 0.0, 0.0],
            color,
            block,
            life,
            rise: false,
            hover: false,
            tumble: false,
            frost: false,
            print: true,
            half,
        });
    }

    /// How many rain splash / ripple motes are still alive.
    pub fn rain_splash_count(&self) -> usize {
        self.motes
            .iter()
            .filter(|mote| mote.hover && mote.rise)
            .count()
    }

    /// Tiny ground splash + expanding ripple when a raindrop hits open ground or a puddle.
    pub fn rain_splash(&mut self, origin: [f32; 3]) {
        const MAX_SPLASH: usize = 72;
        if self.rain_splash_count() >= MAX_SPLASH {
            return;
        }
        // Bright droplet pops (gravity) — sit clear of the ground to avoid Z-fight.
        self.motes.push(Mote {
            center: [origin[0], origin[1] + 0.18, origin[2]],
            velocity: [0.1, 2.4, -0.08],
            color: [0.95, 0.98, 1.0],
            block: [0.95, 0.98, 1.0],
            life: 0.55,
            rise: false,
            hover: false,
            tumble: false,            frost: false, print: false,            half: 0.07,
        });
        self.motes.push(Mote {
            center: [origin[0] + 0.06, origin[1] + 0.16, origin[2] - 0.05],
            velocity: [-0.12, 2.0, 0.1],
            color: [0.9, 0.96, 1.0],
            block: [0.9, 0.96, 1.0],
            life: 0.45,
            rise: false,
            hover: false,
            tumble: false,            frost: false, print: false,            half: 0.06,
        });
        // Flat ripples: both rise+hover marks rain splash (not smoke, not firefly).
        // Keep centers above the top face so expanding cubes do not Z-fight grass.
        const RING: [([f32; 3], [f32; 3], f32); 5] = [
            ([0.85, 0.02, 0.22], [0.92, 0.97, 1.0], 0.85),
            ([-0.78, 0.01, 0.4], [0.88, 0.94, 1.0], 0.78),
            ([0.35, 0.02, -0.85], [0.95, 0.98, 1.0], 0.9),
            ([-0.5, 0.01, -0.55], [0.85, 0.92, 0.98], 0.72),
            ([0.62, 0.02, -0.28], [0.9, 0.96, 1.0], 0.8),
        ];
        let seed = ((origin[0] * 5.3 + origin[2] * 2.1).abs() as usize) % RING.len();
        for (i, (vel, color, life)) in RING.into_iter().enumerate() {
            if self.rain_splash_count() >= MAX_SPLASH {
                break;
            }
            let turn = ((seed + i) % RING.len()) as f32 * 0.04;
            self.motes.push(Mote {
                center: [
                    origin[0] + turn - 0.05,
                    origin[1] + 0.14,
                    origin[2] - turn * 0.5,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: true,
                hover: true,
                tumble: false,                frost: false, print: false,                half: 0.08,
            });
        }
    }

    /// Bright lime hover mote — XP orb stand-in for stills / soft sparks.
    pub fn lime_spark(&mut self, origin: [f32; 3]) {
        self.motes.push(Mote {
            center: origin,
            velocity: [0.04, 0.18, -0.05],
            color: [0.62, 1.0, 0.28],
            block: [0.85, 1.0, 0.4],
            life: 10.0,
            rise: false,
            hover: true,
            tumble: false,            frost: false, print: false,            half: 0.11,
        });
    }

    /// Soft warm motes that drift in night meadows / forest clearings (and shores).
    /// Capped so the air stays quiet.
    pub fn firefly(&mut self, origin: [f32; 3]) {
        const MAX_FLIES: usize = 48;
        if self.firefly_count() >= MAX_FLIES {
            return;
        }
        // Warm yellow-green glow; block channel keeps them lit at night.
        const FLIES: [([f32; 3], [f32; 3], [f32; 3], f32); 3] = [
            (
                [0.12, 0.05, -0.08],
                [1.0, 0.92, 0.35],
                [0.85, 0.72, 0.12],
                6.4,
            ),
            (
                [-0.1, 0.08, 0.1],
                [0.95, 1.0, 0.4],
                [0.7, 0.85, 0.15],
                5.6,
            ),
            (
                [0.05, -0.04, 0.12],
                [1.0, 0.85, 0.28],
                [0.9, 0.65, 0.08],
                7.0,
            ),
        ];
        let seed = ((origin[0] * 3.1 + origin[2] * 1.7).abs() as usize) % FLIES.len();
        let (vel, color, block, life) = FLIES[seed];
        self.motes.push(Mote {
            center: [
                origin[0] + (seed as f32) * 0.07 - 0.07,
                origin[1] + 0.15,
                origin[2] - (seed as f32) * 0.05,
            ],
            velocity: vel,
            color,
            block,
            life,
            rise: false,
            hover: true,
            tumble: false,
            frost: false, print: false,
            // Tiny warm points — blink grows half in tick_motes.
            half: 0.035,
        });
    }

    /// Dawn: snuff night fireflies quickly so they do not linger into day.
    pub fn dawn_fireflies(&mut self, dt: f32) -> bool {
        let mut any = false;
        let fade = (dt * 3.2).max(0.0);
        for mote in &mut self.motes {
            if mote.hover && !mote.rise && !mote.tumble && !mote.frost {
                mote.life -= fade;
                any = true;
            }
        }
        if any {
            self.motes.retain(|mote| mote.life > 0.0);
        }
        any
    }

    /// How many forest leaf tumble motes are still falling.
    pub fn leaf_count(&self) -> usize {
        self.motes.iter().filter(|mote| mote.tumble).count()
    }

    /// A soft leaf that drifts and tumbles down from the canopy. `tint` is
    /// seasonal (spring green through autumn amber). Capped so woods stay quiet.
    pub fn leaf(&mut self, origin: [f32; 3], tint: [f32; 3]) {
        const MAX_LEAVES: usize = 40;
        if self.leaf_count() >= MAX_LEAVES {
            return;
        }
        const DRIFTS: [([f32; 3], f32); 4] = [
            ([0.28, -0.35, 0.12], 5.4),
            ([-0.22, -0.28, 0.18], 4.8),
            ([0.16, -0.42, -0.14], 5.8),
            ([-0.3, -0.32, -0.1], 5.1),
        ];
        let seed = ((origin[0] * 2.3 + origin[2] * 1.7).abs() as usize) % DRIFTS.len();
        let (vel, life) = DRIFTS[seed];
        let wobble = (seed as f32) * 0.05;
        self.motes.push(Mote {
            center: [
                origin[0] + wobble - 0.08,
                origin[1] - 0.05,
                origin[2] - wobble * 0.4,
            ],
            velocity: vel,
            color: tint,
            block: tint,
            life,
            rise: false,
            hover: false,
            tumble: true,            frost: false, print: false,            half: 0.055,
        });
    }

    /// A red cork bobber that hovers on the water for a cast beat.
    pub fn bobber(&mut self, origin: [f32; 3]) {
        self.motes.push(Mote {
            center: origin,
            velocity: [0.0, 0.12, 0.0],
            color: [0.92, 0.28, 0.22],
            block: [0.92, 0.28, 0.22],
            life: 0.7,
            rise: false,
            hover: true,
            tumble: false,            frost: false, print: false,            half: 0.08,
        });
        self.motes.push(Mote {
            center: [origin[0], origin[1] + 0.06, origin[2]],
            velocity: [0.0, 0.08, 0.0],
            color: [0.95, 0.95, 0.92],
            block: [0.95, 0.95, 0.92],
            life: 0.65,
            rise: false,
            hover: true,
            tumble: false,            frost: false, print: false,            half: 0.05,
        });
    }

    /// Warm pollen crumbs that hang and drift behind a loaded bee.
    pub fn pollen_trail(&mut self, origin: [f32; 3]) {
        const CRUMBS: [([f32; 3], [f32; 3], f32); 4] = [
            ([0.04, 0.18, -0.02], [0.96, 0.72, 0.18], 0.85),
            ([-0.06, 0.12, 0.05], [0.92, 0.55, 0.12], 0.7),
            ([0.02, 0.22, 0.04], [0.98, 0.82, 0.28], 0.95),
            ([-0.03, 0.08, -0.06], [0.88, 0.48, 0.1], 0.6),
        ];
        let pollen_n = self
            .motes
            .iter()
            .filter(|mote| {
                mote.hover && !mote.rise && mote.color[0] > 0.85 && mote.color[1] > 0.4
            })
            .count();
        if pollen_n >= 24 {
            return;
        }
        for (i, (vel, color, life)) in CRUMBS.into_iter().enumerate() {
            let wobble = (i as f32) * 0.03;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.04,
                    origin[1] + 0.08,
                    origin[2] - wobble * 0.4,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: false,
                hover: true,
                tumble: false,                frost: false, print: false,                half: 0.035,
            });
        }
    }

    /// Gold pop when a totem of undying blocks a lethal hit.
    pub fn totem_pop(&mut self, origin: [f32; 3]) {
        const COLOR: [f32; 3] = [0.95, 0.78, 0.22];
        const GLOW: [f32; 3] = [0.85, 0.55, 0.08];
        const DIRS: [[f32; 3]; 12] = [
            [1.1, 2.0, 0.2],
            [-1.2, 1.7, 0.4],
            [0.4, 2.4, -1.0],
            [-0.5, 1.4, -1.1],
            [1.3, 1.8, 1.0],
            [-1.1, 2.1, 0.85],
            [0.15, 2.6, 0.1],
            [-0.7, 0.9, -0.35],
            [0.9, 1.1, -0.7],
            [-0.95, 1.0, 0.95],
            [0.55, 2.2, 0.55],
            [-0.35, 2.3, -0.45],
        ];
        for dir in DIRS {
            self.motes.push(Mote {
                center: [origin[0], origin[1] + 0.4, origin[2]],
                velocity: dir,
                color: COLOR,
                block: GLOW,
                life: 0.95,
                rise: false,
                hover: false,
                tumble: true,                frost: false, print: false,                half: 0.06,
            });
        }
    }

    /// Teal pop when an ender pearl lands and teleports the thrower.
    pub fn pearl_pop(&mut self, origin: [f32; 3]) {
        const COLOR: [f32; 3] = [0.22, 0.72, 0.58];
        const DIRS: [[f32; 3]; 8] = [
            [0.85, 1.6, 0.15],
            [-0.9, 1.4, 0.3],
            [0.3, 1.9, -0.75],
            [-0.35, 1.15, -0.9],
            [1.0, 1.5, 0.8],
            [-0.85, 1.75, 0.65],
            [0.1, 2.2, 0.05],
            [-0.5, 0.7, -0.25],
        ];
        for dir in DIRS {
            self.motes.push(Mote {
                center: [origin[0], origin[1] + 0.2, origin[2]],
                velocity: dir,
                color: COLOR,
                block: COLOR,
                life: 0.7,
                rise: false,
                hover: false,
                tumble: true,                frost: false, print: false,                half: 0.05,
            });
        }
    }

    /// Colored splash-potion burst motes at an impact point.
    pub fn splash_potion(&mut self, origin: [f32; 3], color: [f32; 3]) {
        const DIRS: [[f32; 3]; 10] = [
            [0.9, 1.8, 0.2],
            [-1.0, 1.5, 0.35],
            [0.35, 2.1, -0.85],
            [-0.4, 1.2, -1.0],
            [1.1, 1.6, 0.9],
            [-0.95, 1.9, 0.7],
            [0.15, 2.4, 0.1],
            [-0.55, 1.35, -0.3],
            [0.7, 0.6, -0.55],
            [-0.65, 0.55, 0.8],
        ];
        for dir in DIRS {
            self.motes.push(Mote {
                center: [
                    origin[0],
                    origin[1] + 0.15,
                    origin[2],
                ],
                velocity: dir,
                color,
                block: color,
                life: 0.85,
                rise: false,
                hover: false,
                tumble: true,                frost: false, print: false,                half: 0.055,
            });
        }
    }

    /// Soft cyan bubbles rising from a brewing stand.
    pub fn brew_bubbles(&mut self, origin: [f32; 3]) {
        const BUBBLES: [([f32; 3], [f32; 3], f32); 6] = [
            ([0.08, 1.05, 0.04], [0.45, 0.82, 0.95], 1.3),
            ([-0.12, 0.9, -0.06], [0.55, 0.90, 1.0], 1.15),
            ([0.14, 1.2, -0.1], [0.35, 0.72, 0.88], 1.4),
            ([-0.06, 1.1, 0.12], [0.62, 0.95, 1.0], 1.2),
            ([0.02, 0.85, -0.14], [0.40, 0.78, 0.92], 1.05),
            ([-0.1, 1.25, 0.05], [0.70, 0.98, 1.0], 1.35),
        ];
        for (i, (vel, color, life)) in BUBBLES.into_iter().enumerate() {
            let wobble = (i as f32) * 0.035;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.06,
                    origin[1] + 0.25,
                    origin[2] - wobble * 0.3,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: true,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.06,
            });
        }
    }

    /// Purple book pages swirling up from an enchanting table.
    pub fn book_swirl(&mut self, origin: [f32; 3]) {
        // Tangential + rise so a still PNG still reads as a swirl.
        const PAGES: [([f32; 3], [f32; 3], f32); 6] = [
            ([0.55, 0.95, 0.12], [0.72, 0.42, 0.92], 1.25),
            ([-0.48, 1.05, 0.35], [0.58, 0.32, 0.88], 1.15),
            ([-0.22, 0.85, -0.52], [0.82, 0.55, 0.96], 1.35),
            ([0.38, 1.15, -0.28], [0.48, 0.28, 0.78], 1.05),
            ([0.12, 1.25, 0.48], [0.90, 0.62, 0.98], 1.4),
            ([-0.42, 0.75, -0.18], [0.64, 0.38, 0.90], 1.2),
        ];
        for (i, (vel, color, life)) in PAGES.into_iter().enumerate() {
            let wobble = (i as f32) * 0.04;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.08,
                    origin[1] + 0.2,
                    origin[2] - wobble * 0.35,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: true,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.07,
            });
        }
    }

    /// A short pitch-tinted mote above a note block. Color steps with the note.
    pub fn note(&mut self, origin: [f32; 3], pitch: u8) {
        let color = note_color(pitch);
        const DIRS: [[f32; 3]; 3] = [
            [0.05, 1.15, 0.02],
            [-0.08, 0.95, -0.04],
            [0.02, 1.35, 0.06],
        ];
        for (i, dir) in DIRS.into_iter().enumerate() {
            let wobble = (i as f32) * 0.03;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble,
                    origin[1] + 0.15,
                    origin[2] - wobble * 0.4,
                ],
                velocity: dir,
                color,
                block: color,
                life: 1.1,
                rise: true,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.08,
            });
        }
    }

    /// How many scarecrow straw motes are still drifting.
    pub fn straw_count(&self) -> usize {
        self.motes
            .iter()
            .filter(|mote| {
                mote.hover
                    && !mote.rise
                    && mote.color[0] > 0.75
                    && mote.color[1] > 0.55
                    && mote.color[1] < 0.82
                    && mote.color[2] < 0.4
            })
            .count()
    }

    /// How many wheat chaff motes are still drifting.
    pub fn chaff_count(&self) -> usize {
        self.motes
            .iter()
            .filter(|mote| {
                mote.hover
                    && !mote.rise
                    && mote.color[1] >= mote.color[0]
                    && mote.color[2] < 0.4
            })
            .count()
    }

    /// Soft gold-green husks that lift off wheat tops in a breeze.
    pub fn chaff(&mut self, origin: [f32; 3]) {
        const MAX_CHAFF: usize = 30;
        if self.chaff_count() >= MAX_CHAFF {
            return;
        }
        const HUSKS: [([f32; 3], [f32; 3], f32); 5] = [
            ([0.28, 0.35, 0.06], [0.78, 0.86, 0.28], 1.45),
            ([-0.18, 0.42, 0.12], [0.7, 0.82, 0.22], 1.25),
            ([0.36, 0.28, -0.1], [0.84, 0.9, 0.32], 1.6),
            ([-0.24, 0.38, -0.08], [0.66, 0.78, 0.2], 1.15),
            ([0.12, 0.48, 0.14], [0.74, 0.88, 0.26], 1.35),
        ];
        let seed = ((origin[0] * 1.9 + origin[2] * 2.3).abs() as usize) % HUSKS.len();
        for (i, (vel, color, life)) in HUSKS.into_iter().enumerate() {
            if self.chaff_count() >= MAX_CHAFF {
                break;
            }
            let wobble = ((seed + i) as f32) * 0.04;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.08,
                    origin[1] + 0.1 + (i as f32) * 0.06,
                    origin[2] - wobble * 0.35,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: false,
                hover: true,
                tumble: false,                frost: false, print: false,                half: 0.05,
            });
        }
    }

    /// Pale hay flecks that blow past the camp scarecrow on the breeze.
    pub fn straw(&mut self, origin: [f32; 3]) {
        const MAX_STRAW: usize = 28;
        if self.straw_count() >= MAX_STRAW {
            return;
        }
        const FLECKS: [([f32; 3], [f32; 3], f32); 5] = [
            ([0.55, 0.12, 0.08], [0.98, 0.82, 0.32], 1.6),
            ([0.42, 0.18, -0.1], [0.94, 0.72, 0.24], 1.35),
            ([0.68, 0.05, 0.04], [1.0, 0.88, 0.38], 1.8),
            ([0.35, 0.22, 0.14], [0.9, 0.68, 0.2], 1.2),
            ([0.5, 0.08, -0.16], [0.96, 0.78, 0.28], 1.5),
        ];
        let seed = ((origin[0] * 2.7 + origin[2] * 1.3).abs() as usize) % FLECKS.len();
        for (i, (vel, color, life)) in FLECKS.into_iter().enumerate() {
            if self.straw_count() >= MAX_STRAW {
                break;
            }
            let wobble = ((seed + i) as f32) * 0.05;
            self.motes.push(Mote {
                center: [
                    origin[0] - 0.55 + wobble,
                    origin[1] + 0.15 + (i as f32) * 0.1,
                    origin[2] + wobble * 0.5 - 0.15,
                ],
                velocity: vel,
                color,
                // Keep block near color so daytime shade does not wash flecks out.
                block: color,
                life,
                rise: false,
                hover: true,
                tumble: false,                frost: false, print: false,                half: 0.055,
            });
        }
    }

    /// Hot orange sparks that pop up from a campfire, then fall. Capped.
    pub fn embers(&mut self, origin: [f32; 3]) {
        const MAX_EMBERS: usize = 20;
        let mut n = self
            .motes
            .iter()
            .filter(|mote| !mote.rise && !mote.hover && mote.color[0] > 0.85 && mote.color[1] > 0.3)
            .count();
        if n >= MAX_EMBERS {
            return;
        }
        const SPARKS: [([f32; 3], [f32; 3], f32); 5] = [
            ([0.18, 2.4, 0.05], [1.0, 0.55, 0.12], 0.55),
            ([-0.22, 2.1, -0.08], [1.0, 0.72, 0.18], 0.48),
            ([0.08, 2.7, -0.14], [0.98, 0.42, 0.08], 0.62),
            ([-0.1, 1.9, 0.16], [1.0, 0.82, 0.28], 0.42),
            ([0.14, 2.2, 0.1], [0.95, 0.35, 0.06], 0.5),
        ];
        for (i, (vel, color, life)) in SPARKS.into_iter().enumerate() {
            if n >= MAX_EMBERS {
                break;
            }
            let wobble = (i as f32) * 0.03;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.05,
                    origin[1] + 0.12,
                    origin[2] - wobble * 0.4,
                ],
                velocity: vel,
                color,
                block: color,
                life,
                rise: false,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.04,
            });
            n += 1;
        }
    }

    /// Soft gray puffs that float up from a campfire. Cheap and capped.
    pub fn smoke(&mut self, origin: [f32; 3]) {
        const MAX_SMOKE: usize = 28;
        let mut smoke_n = self
            .motes
            .iter()
            .filter(|mote| mote.rise && !mote.hover)
            .count();
        if smoke_n >= MAX_SMOKE {
            return;
        }
        // `color` is lit ambient+block; keep a soft ambient (`color - block`)
        // so sun shade does not wash the puff away.
        const PUFFS: [([f32; 3], [f32; 3], [f32; 3], f32); 4] = [
            (
                [0.08, 0.55, 0.03],
                [0.78, 0.78, 0.82],
                [0.28, 0.28, 0.30],
                1.8,
            ),
            (
                [-0.06, 0.42, -0.05],
                [0.70, 0.70, 0.74],
                [0.22, 0.22, 0.24],
                1.55,
            ),
            (
                [0.03, 0.62, -0.08],
                [0.86, 0.86, 0.90],
                [0.32, 0.32, 0.34],
                2.0,
            ),
            (
                [-0.04, 0.48, 0.07],
                [0.64, 0.64, 0.68],
                [0.18, 0.18, 0.20],
                1.45,
            ),
        ];
        for (i, (vel, color, block, life)) in PUFFS.into_iter().enumerate() {
            if smoke_n >= MAX_SMOKE {
                break;
            }
            let wobble = (i as f32) * 0.04;
            self.motes.push(Mote {
                center: [
                    origin[0] + wobble - 0.05,
                    origin[1],
                    origin[2] - wobble * 0.5,
                ],
                velocity: vel,
                color,
                block,
                life,
                rise: true,
                hover: false,
                tumble: false,                frost: false, print: false,                half: 0.12,
            });
            smoke_n += 1;
        }
    }

    /// Move the burst. True when any mote is still in the air, so the mesh should update.
    pub fn tick_motes(&mut self, dt: f32) -> bool {
        if self.motes.is_empty() {
            return false;
        }
        let step = dt.max(0.0);
        for mote in &mut self.motes {
            mote.life -= step;
            mote.center[0] += mote.velocity[0] * step;
            mote.center[1] += mote.velocity[1] * step;
            mote.center[2] += mote.velocity[2] * step;
            if mote.print {
                // Stay planted on the snow; soft fade + slight shrink.
                mote.velocity = [0.0, 0.0, 0.0];
                let fade = (mote.life / 6.5).clamp(0.15, 1.0);
                mote.color = [
                    (mote.block[0] * fade).min(1.0),
                    (mote.block[1] * fade).min(1.0),
                    (mote.block[2] * fade).min(1.0),
                ];
                mote.half = (0.05 + 0.045 * fade).min(0.1);
            } else if mote.frost {
                // Soft forward drift + slight lift; puff expands and fades pale.
                mote.velocity[1] += 0.12 * step;
                let damp = (1.0 - 1.35 * step).max(0.0);
                mote.velocity[0] *= damp;
                mote.velocity[2] *= damp;
                mote.half = (mote.half + 0.07 * step).min(0.14);
                let fade = (mote.life * 1.35).clamp(0.25, 1.0);
                mote.color = [
                    (mote.block[0] * fade).min(1.0),
                    (mote.block[1] * fade).min(1.0),
                    (mote.block[2] * fade.min(1.05)).min(1.0),
                ];
            } else if mote.hover && mote.rise {
                // Rain ripple: expand on the surface, stay flat, fade cool.
                mote.velocity[1] = 0.0;
                let damp = (1.0 - 1.6 * step).max(0.0);
                mote.velocity[0] *= damp;
                mote.velocity[2] *= damp;
                // Cap growth so ripples stay above the ground plane.
                mote.half = (mote.half + 0.18 * step).min(0.12);
                let fade = (mote.life * 1.6).clamp(0.45, 1.0);
                mote.color = [
                    (mote.block[0] * fade).min(1.0),
                    (mote.block[1] * fade).min(1.0),
                    (mote.block[2] * fade.min(1.08)).min(1.0),
                ];
            } else if mote.hover {
                // Sin bob + twinkle so dusk flies read against the night sky.
                let bob = (mote.life * 2.9).sin() * 0.75;
                mote.velocity[1] += (bob - mote.velocity[1]) * (2.4 * step).min(1.0);
                if mote.life.sin().abs() > 0.9 {
                    mote.velocity[0] = -mote.velocity[0] * 0.88;
                    mote.velocity[2] = -mote.velocity[2] * 0.88;
                }
                let damp = (1.0 - 0.28 * step).max(0.0);
                mote.velocity[0] *= damp;
                mote.velocity[2] *= damp;
                let twinkle = (mote.life * 5.4).sin().abs();
                mote.half = 0.03 + 0.05 * twinkle;
                // Brighten toward the lit color from the block ember as they pulse.
                mote.color = [
                    (mote.block[0] + (1.0 - mote.block[0]) * twinkle).min(1.0),
                    (mote.block[1] + (1.0 - mote.block[1]) * twinkle).min(1.0),
                    (mote.block[2] + (0.45 - mote.block[2]).max(0.0) * twinkle).min(1.0),
                ];
            } else if mote.tumble {
                // Soft fall + sway so canopy leaves read as tumbling, not chips.
                mote.velocity[1] -= 0.55 * step;
                mote.velocity[1] = mote.velocity[1].max(-1.35);
                let sway = (mote.life * 3.1).sin();
                mote.velocity[0] += sway * 0.95 * step;
                mote.velocity[2] += (mote.life * 2.4).cos() * 0.75 * step;
                let damp = (1.0 - 0.12 * step).max(0.0);
                mote.velocity[0] *= damp;
                mote.velocity[2] *= damp;
                let flip = (mote.life * 4.2).sin().abs();
                mote.half = 0.028 + 0.038 * flip;
                let fade = (mote.life * 0.32).clamp(0.4, 1.0);
                mote.color = [
                    (mote.block[0] * fade).min(1.0),
                    (mote.block[1] * fade).min(1.0),
                    (mote.block[2] * fade).min(1.0),
                ];
            } else if mote.rise {
                mote.velocity[1] += 0.28 * step;
                let damp = (1.0 - 1.1 * step).max(0.0);
                mote.velocity[0] *= damp;
                mote.velocity[2] *= damp;
                mote.half = (mote.half + 0.02 * step).min(0.14);
            } else {
                mote.velocity[1] -= 2.4 * step;
            }
        }
        self.motes.retain(|mote| mote.life > 0.0);
        true
    }

    /// Leave one cube for `id`. A solid cell keeps the cube on top of it.
    pub fn spawn(&mut self, id: &str, cell: [i32; 3], remains_solid: bool) {
        if id.is_empty() || id == "air" {
            return;
        }
        self.items.push(Drop {
            id: Block::new(id, 0).id,
            center: center_for(cell, remains_solid),
            refused: false,
            age: 0.0,
        });
    }

    /// Pin a drop at an exact world point (still / magnet mid-home).
    pub fn pin(&mut self, id: &str, center: [f32; 3]) {
        if id.is_empty() || id == "air" {
            return;
        }
        self.items.push(Drop {
            id: Block::new(id, 0).id,
            center,
            refused: false,
            age: MAGNET_DELAY + 0.2,
        });
    }

    /// Age drops and gently home nearby loot toward the player eye.
    /// Returns true when any center moved (mesh should rebuild).
    pub fn tick_magnet(&mut self, dt: f32, eye: [f32; 3]) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let step = dt.max(0.0);
        if step <= 0.0 {
            return false;
        }
        let range2 = MAGNET_RANGE * MAGNET_RANGE;
        // Pull toward mid-torso so the tile enters the body AABB.
        let target = [eye[0], eye[1] - 0.85, eye[2]];
        let mut dirty = false;
        for drop in &mut self.items {
            drop.age += step;
            if drop.age < MAGNET_DELAY || drop.refused {
                continue;
            }
            let dx = target[0] - drop.center[0];
            let dy = target[1] - drop.center[1];
            let dz = target[2] - drop.center[2];
            let dist2 = dx * dx + dy * dy + dz * dz;
            if dist2 > range2 || dist2 < 1.0e-8 {
                continue;
            }
            let dist = dist2.sqrt();
            let inv = MAGNET_SPEED / dist;
            drop.center[0] += dx * inv * step;
            drop.center[1] += dy * inv * step;
            drop.center[2] += dz * inv * step;
            dirty = true;
        }
        dirty
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Drops the body just started touching and a hook has not refused yet.
    /// Walking off a refused drop clears it, so the next touch asks again.
    pub fn pending_pickups(&mut self, body_min: [f32; 3], body_max: [f32; 3]) -> Vec<&'static str> {
        let mut ids = Vec::new();
        for drop in &mut self.items {
            if !overlaps(drop.center, body_min, body_max) {
                drop.refused = false;
                continue;
            }
            if !drop.refused {
                ids.push(drop.id);
            }
        }
        ids
    }

    /// Add overlapping drops to `hotbar`. A drop the bar cannot take stays.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pickup(&mut self, body_min: [f32; 3], body_max: [f32; 3], hotbar: &mut Hotbar) -> bool {
        self.pickup_filtered(body_min, body_max, hotbar, &[])
    }

    /// Like [`Drops::pickup`], and any id in `refuse` stays on the ground.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pickup_filtered(
        &mut self,
        body_min: [f32; 3],
        body_max: [f32; 3],
        hotbar: &mut Hotbar,
        refuse: &[&str],
    ) -> bool {
        let before = self.items.len();
        self.items.retain_mut(|drop| {
            if !overlaps(drop.center, body_min, body_max) {
                return true;
            }
            if refuse.iter().any(|id| *id == drop.id) {
                drop.refused = true;
                return true;
            }
            !hotbar.collect(drop.id)
        });
        self.items.len() != before
    }

    /// Like [`Drops::pickup`], then the inventory when the hotbar cannot take it.
    pub fn pickup_carried(
        &mut self,
        body_min: [f32; 3],
        body_max: [f32; 3],
        hotbar: &mut Hotbar,
        bag: &mut crate::hotbar::Bag,
        refuse: &[&str],
    ) -> bool {
        let before = self.items.len();
        self.items.retain_mut(|drop| {
            if !overlaps(drop.center, body_min, body_max) {
                return true;
            }
            if refuse.iter().any(|id| *id == drop.id) {
                drop.refused = true;
                return true;
            }
            if hotbar.collect(drop.id) {
                return false;
            }
            !bag.collect(drop.id)
        });
        self.items.len() != before
    }

    /// The nearest food drop inside `reach`, if the fox can walk to it.
    pub fn food_near(&self, at: [f32; 3], reach: f32) -> Option<[f32; 3]> {
        self.nearest_food(at, reach).map(|drop| drop.center)
    }

    /// Remove the nearest food drop inside `reach`.
    pub fn take_food(&mut self, at: [f32; 3], reach: f32) -> Option<&'static str> {
        let index = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, drop)| is_food(drop.id) && dist2(drop.center, at) <= reach * reach)
            .min_by(|(_, left), (_, right)| {
                dist2(left.center, at).total_cmp(&dist2(right.center, at))
            })
            .map(|(index, _)| index)?;
        Some(self.items.remove(index).id)
    }

    /// Dropped items the crosshair can name. Each box is the pickup sprite.
    pub fn boxes(&self) -> impl Iterator<Item = (&'static str, [f32; 3], [f32; 3])> + '_ {
        self.items.iter().map(|drop| {
            let [x, y, z] = drop.center;
            (
                drop.id,
                [x - HALF, y - HALF, z - HALF],
                [x + HALF, y + HALF, z + HALF],
            )
        })
    }

    fn nearest_food(&self, at: [f32; 3], reach: f32) -> Option<&Drop> {
        self.items
            .iter()
            .filter(|drop| is_food(drop.id) && dist2(drop.center, at) <= reach * reach)
            .min_by(|left, right| dist2(left.center, at).total_cmp(&dist2(right.center, at)))
    }

    /// Items face +Z. Sparks stay cubes. Tests use this facing.
    #[cfg(test)]
    pub fn vertices(&self) -> Vec<Vertex> {
        self.vertices_facing([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    }

    /// Item tiles face the camera. `right` and `up` are that camera's axes.
    #[cfg(test)]
    pub fn vertices_facing(&self, right: [f32; 3], up: [f32; 3]) -> Vec<Vertex> {
        let mut vertices = Vec::with_capacity(self.items.len() * 6 + self.motes.len() * 36);
        for drop in &self.items {
            push_billboard(&mut vertices, drop.center, HALF, drop.id, right, up);
        }
        for mote in &self.motes {
            push_box(&mut vertices, mote.center, mote.half, mote.color, mote.block);
        }
        vertices
    }

    /// Each item points at `eye`. Sparks stay cubes.
    pub fn vertices_toward(&self, eye: [f32; 3]) -> Vec<Vertex> {
        let mut vertices = Vec::with_capacity(self.items.len() * 6 + self.motes.len() * 36);
        for drop in &self.items {
            let (right, up) = axes_toward(drop.center, eye);
            push_billboard(&mut vertices, drop.center, HALF, drop.id, right, up);
        }
        for mote in &self.motes {
            push_box(&mut vertices, mote.center, mote.half, mote.color, mote.block);
        }
        vertices
    }
}

/// Right and up for a quad whose face points from `center` at `eye`.
fn axes_toward(center: [f32; 3], eye: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let mut to_eye = sub(eye, center);
    if len2(to_eye) < 1.0e-6 {
        to_eye = [0.0, 0.0, 1.0];
    } else {
        to_eye = normalize(to_eye);
    }
    let mut right = cross([0.0, 1.0, 0.0], to_eye);
    if len2(right) < 1.0e-6 {
        right = [1.0, 0.0, 0.0];
    } else {
        right = normalize(right);
    }
    let up = normalize(cross(to_eye, right));
    (right, up)
}

pub fn center_for(cell: [i32; 3], remains_solid: bool) -> [f32; 3] {
    let y = if remains_solid {
        cell[1] as f32 + 1.2
    } else {
        cell[1] as f32 + 0.32
    };
    [cell[0] as f32 + 0.5, y, cell[2] as f32 + 0.5]
}

fn is_food(id: &str) -> bool {
    rebuild_world::Pack::builtin().food_of(id).is_some()
}

/// Pale dust for stone, near-black for coal, and a warm chip for iron.
/// Hue steps across the sixteen note pitches (C → D#5).
pub fn note_color(pitch: u8) -> [f32; 3] {
    let t = f32::from(pitch & 0x0F) / 15.0;
    // Warm low → green mid → cool high, so each step reads in a still PNG.
    let r = (1.0 - t * 0.85).clamp(0.15, 1.0);
    let g = (0.25 + t * 0.7).clamp(0.2, 0.95);
    let b = (0.15 + (t - 0.35).max(0.0) * 1.4).clamp(0.1, 1.0);
    [r, g, b]
}

pub fn chip_color(id: &str) -> [f32; 3] {
    match id {
        "coal_ore" => [0.14, 0.14, 0.16],
        "iron_ore" => [0.86, 0.58, 0.36],
        "emerald_ore" => [0.22, 0.72, 0.38],
        _ => [0.78, 0.76, 0.70],
    }
}

fn dist2(from: [f32; 3], to: [f32; 3]) -> f32 {
    let dx = from[0] - to[0];
    let dy = from[1] - to[1];
    let dz = from[2] - to[2];
    dx * dx + dy * dy + dz * dz
}

fn overlaps(center: [f32; 3], min: [f32; 3], max: [f32; 3]) -> bool {
    (0..3).all(|axis| center[axis] + HALF > min[axis] && center[axis] - HALF < max[axis])
}

fn push_box(
    out: &mut Vec<Vertex>,
    center: [f32; 3],
    half: f32,
    color: [f32; 3],
    block: [f32; 3],
) {
    let min = [center[0] - half, center[1] - half, center[2] - half];
    let max = [center[0] + half, center[1] + half, center[2] + half];
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
                block,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

fn push_billboard(
    out: &mut Vec<Vertex>,
    center: [f32; 3],
    half: f32,
    id: &str,
    right: [f32; 3],
    up: [f32; 3],
) {
    let right = scale(right, half);
    let up = scale(up, half);
    let corner = [
        add(center, add(scale(right, -1.0), scale(up, -1.0))),
        add(center, add(right, scale(up, -1.0))),
        add(center, add(right, up)),
        add(center, add(scale(right, -1.0), up)),
    ];
    let uv = [[0.0, 0.999], [0.999, 0.999], [0.999, 0.0], [0.0, 0.0]];
    let normal = normalize(cross(right, up));
    let textured = crate::texture::block_face(id);
    let color = if textured > 0.0 {
        [1.0, 1.0, 1.0]
    } else {
        palette::color(id, 0)
    };
    let block = if textured > 0.0 {
        [0.45, 0.45, 0.45]
    } else {
        color
    };
    let tri = [0, 1, 2, 0, 2, 3];
    for index in tri {
        out.push(Vertex {
            position: corner[index],
            color,
            normal,
            uv: uv[index],
            textured,
            block,
            bounce: [0.0, 0.0, 0.0],
            ao: 1.0,
        });
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(v: [f32; 3], by: f32) -> [f32; 3] {
    [v[0] * by, v[1] * by, v[2] * by]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len2(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = len2(v).sqrt();
    if len < 1.0e-6 {
        [0.0, 0.0, 1.0]
    } else {
        scale(v, 1.0 / len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotbar::Hotbar;

    #[test]
    fn a_break_spawns_a_small_cube_and_skips_the_hotbar() {
        let mut drops = Drops::new();
        let bar = Hotbar::new();
        let before = bar.count(0);
        drops.spawn("grass", [8, 0, 14], false);
        assert_eq!(drops.items.len(), 1);
        assert_eq!(bar.count(0), before);
        assert!(bar.slot(1).is_none());

        let center = center_for([8, 0, 14], false);
        assert_eq!(center, [8.5, 0.32, 14.5]);
        let solid = center_for([9, 1, 13], true);
        assert_eq!(solid[1], 2.2);

        let vertices = drops.vertices();
        assert_eq!(vertices.len(), 6);
        let face = crate::texture::block_face("grass");
        assert!(face > 0.0);
        assert!(vertices.iter().all(|vertex| {
            vertex.color == [1.0, 1.0, 1.0]
                && vertex.textured == face
                && within(vertex.position, center)
        }));
        assert!(vertices.iter().any(|vertex| vertex.uv[0] > 0.5));
    }

    #[test]
    fn a_nearby_food_drop_can_be_taken_and_stone_stays() {
        let mut drops = Drops::new();
        drops.spawn("stone", [6, 1, 6], false);
        drops.spawn("apple", [7, 1, 6], false);
        let at = [6.2, 1.0, 6.2];
        let spot = drops.food_near(at, 6.0).expect("apple");
        assert!(spot[0] > 7.0);
        assert_eq!(drops.take_food(at, 6.0), Some("apple"));
        assert!(drops.food_near(at, 6.0).is_none());
        assert_eq!(drops.items.len(), 1);
    }

    #[test]
    fn a_drop_turns_to_face_the_player() {
        let mut drops = Drops::new();
        drops.spawn("grass", [0, 1, 0], false);
        let center = drops.items[0].center;
        let south = drops.vertices_toward([center[0], center[1], center[2] + 4.0]);
        let east = drops.vertices_toward([center[0] + 4.0, center[1], center[2]]);
        let above = drops.vertices_toward([center[0], center[1] + 4.0, center[2]]);
        assert_eq!(south.len(), 6);
        assert!(south.iter().all(|vertex| vertex.normal[2] > 0.9));
        assert!(east.iter().all(|vertex| vertex.normal[0] > 0.9));
        assert!(above.iter().all(|vertex| vertex.normal[1] > 0.9));
        assert_ne!(south[0].position, east[0].position);
        assert_ne!(south[0].position, above[0].position);
    }

    #[test]
    fn nearby_loot_homes_after_delay_but_far_stays() {
        let mut drops = Drops::new();
        drops.spawn("cobblestone", [8, 1, 10], false);
        drops.spawn("oak_log", [20, 1, 20], false);
        let near = drops.items[0].center;
        let far = drops.items[1].center;
        let eye = [8.5, 2.6, 8.5];
        // Still settling — no pull yet.
        assert!(!drops.tick_magnet(MAGNET_DELAY * 0.5, eye));
        assert_eq!(drops.items[0].center, near);
        // Past delay: near drop moves toward the player, far drop stays put.
        assert!(drops.tick_magnet(MAGNET_DELAY + 0.05, eye));
        let pulled = drops.items[0].center;
        assert!((pulled[0] - near[0]).abs() + (pulled[2] - near[2]).abs() > 0.05);
        assert_eq!(drops.items[1].center, far);
        // Keep pulling until it would overlap a body around the eye.
        for _ in 0..40 {
            let _ = drops.tick_magnet(0.05, eye);
        }
        let body = (
            [eye[0] - 0.35, eye[1] - 1.7, eye[2] - 0.35],
            [eye[0] + 0.35, eye[1] + 0.2, eye[2] + 0.35],
        );
        let mut bar = Hotbar::new();
        assert!(drops.pickup(body.0, body.1, &mut bar));
        assert_eq!(bar.slot(1), Some("cobblestone"));
        assert_eq!(drops.item_count(), 1);
        assert_eq!(drops.items[0].id, "oak_log");
        drops.pin("iron_ore", [9.0, 2.0, 9.0]);
        assert_eq!(drops.item_count(), 2);
        assert!(drops.items[1].age >= MAGNET_DELAY);
    }

    #[test]
    fn walking_into_a_drop_adds_one_and_a_full_bar_leaves_it() {
        let mut drops = Drops::new();
        let mut bar = Hotbar::new();
        drops.spawn("grass", [2, 1, 2], false);
        drops.spawn("stone", [2, 1, 2], false);
        let missed = ([10.0, 0.0, 10.0], [10.6, 1.8, 10.6]);
        assert!(!drops.pickup(missed.0, missed.1, &mut bar));
        assert_eq!(drops.items.len(), 2);
        assert!(bar.slot(1).is_none());

        let center = center_for([2, 1, 2], false);
        let body = (
            [center[0] - 0.3, center[1] - 0.2, center[2] - 0.3],
            [center[0] + 0.3, center[1] + 1.6, center[2] + 0.3],
        );
        assert!(drops.pickup(body.0, body.1, &mut bar));
        assert!(drops.items.is_empty());
        assert_eq!(bar.slot(1), Some("grass"));
        assert_eq!(bar.count(1), 1);
        assert_eq!(bar.slot(2), Some("stone"));
        assert_eq!(bar.count(2), 1);

        let mut full = Hotbar::new();
        for id in [
            "grass", "stone", "dirt", "oak_log", "wool", "sand", "gravel", "planks", "glass",
        ] {
            for _ in 0..64 {
                assert!(full.collect(id));
            }
        }
        assert!(!full.collect("brick"));
        let mut left = Drops::new();
        left.spawn("brick", [4, 1, 4], false);
        let center = center_for([4, 1, 4], false);
        let body = (
            [center[0] - 0.3, center[1] - 0.2, center[2] - 0.3],
            [center[0] + 0.3, center[1] + 1.6, center[2] + 0.3],
        );
        assert!(!left.pickup(body.0, body.1, &mut full));
        assert_eq!(left.items.len(), 1);
        assert_eq!(left.items[0].id, "brick");
    }

    #[test]
    fn a_melon_break_is_four_slices_you_can_pick_up() {
        let (id, count) = rebuild_world::harvest_of("melon");
        assert_eq!((id, count), ("melon_slice", 4));
        let mut drops = Drops::new();
        let mut bar = Hotbar::new();
        for _ in 0..count {
            drops.spawn(id, [2, 1, 2], false);
        }
        assert_eq!(drops.items.len(), 4);
        let center = center_for([2, 1, 2], false);
        let body = (
            [center[0] - 0.3, center[1] - 0.2, center[2] - 0.3],
            [center[0] + 0.3, center[1] + 1.6, center[2] + 0.3],
        );
        assert!(drops.pickup(body.0, body.1, &mut bar));
        assert!(drops.items.is_empty());
        assert!(bar.holds("melon_slice", 4));
    }

    #[test]
    fn a_spark_rises_and_then_fades() {
        let mut drops = Drops::new();
        drops.burst([1.0, 2.0, 3.0], [1.0, 0.8, 0.2]);
        assert_eq!(drops.vertices().len(), 6 * 36);
        assert!(drops.tick_motes(0.1));
        assert!(!drops.vertices().is_empty());
        assert!(drops.tick_motes(2.0));
        assert!(drops.vertices().is_empty());
        assert!(!drops.tick_motes(0.1));
    }

    #[test]
    fn dusk_fireflies_drift_and_fade() {
        let mut drops = Drops::new();
        drops.firefly([8.0, 2.0, 24.0]);
        drops.firefly([9.0, 2.2, 25.0]);
        assert_eq!(drops.firefly_count(), 2);
        let before = drops.vertices().len();
        assert!(before >= 2 * 36);
        let half_a = drops.motes[0].half;
        assert!(drops.tick_motes(0.2));
        assert_eq!(drops.firefly_count(), 2);
        assert!(
            (drops.motes[0].half - half_a).abs() > 0.001
                || drops.motes.iter().any(|m| m.half > 0.04),
            "twinkle should pulse the mote size"
        );
        assert!(drops.tick_motes(8.0));
        assert_eq!(drops.firefly_count(), 0);
        assert!(drops.vertices().is_empty());
    }

    #[test]
    fn dawn_snuffs_fireflies_quickly() {
        let mut drops = Drops::new();
        drops.firefly([4.0, 2.0, 4.0]);
        drops.firefly([5.0, 2.2, 5.0]);
        assert_eq!(drops.firefly_count(), 2);
        assert!(drops.dawn_fireflies(0.5));
        assert!(drops.firefly_count() <= 2);
        assert!(drops.dawn_fireflies(2.0));
        assert_eq!(drops.firefly_count(), 0);
    }

    #[test]
    fn falling_leaves_tumble_and_cap() {
        let mut drops = Drops::new();
        let tint = [0.88, 0.48, 0.14];
        drops.leaf([8.0, 5.0, 8.0], tint);
        drops.leaf([8.4, 5.2, 7.6], tint);
        assert_eq!(drops.leaf_count(), 2);
        assert_eq!(drops.firefly_count(), 0);
        let y0 = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.25));
        assert!(drops.motes[0].center[1] < y0, "leaf should fall");
        assert!(drops.motes[0].tumble);
        for _ in 0..40 {
            drops.leaf([8.0, 5.0, 8.0], tint);
        }
        assert!(drops.leaf_count() <= 40);
        assert!(drops.tick_motes(8.0));
        assert_eq!(drops.leaf_count(), 0);
    }

    #[test]
    fn a_break_puff_throws_chips_that_fade() {
        let mut drops = Drops::new();
        drops.puff([4.0, 2.0, 4.0], chip_color("stone"));
        assert_eq!(drops.vertices().len(), 8 * 36);
        assert!(drops.tick_motes(0.1));
        assert!(drops.tick_motes(0.4));
        assert!(drops.vertices().is_empty());
    }

    #[test]
    fn campfire_smoke_rises_and_stays_capped() {
        let mut drops = Drops::new();
        let start_y = 2.0;
        drops.smoke([8.0, start_y, 8.0]);
        assert_eq!(drops.motes.len(), 4);
        assert!(drops.motes.iter().all(|mote| mote.rise));
        let before = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.2));
        assert!(drops.motes[0].center[1] > before);
        for _ in 0..20 {
            drops.smoke([8.0, start_y, 8.0]);
        }
        assert!(
            drops
                .motes
                .iter()
                .filter(|mote| mote.rise && !mote.hover)
                .count()
                <= 28
        );
        assert!(drops.tick_motes(3.0));
        assert!(drops.motes.iter().all(|mote| !mote.rise));
    }

    #[test]
    fn campfire_embers_pop_up_then_fall() {
        let mut drops = Drops::new();
        drops.embers([8.0, 2.0, 8.0]);
        assert_eq!(drops.motes.len(), 5);
        assert!(drops.motes.iter().all(|mote| !mote.rise && !mote.hover));
        assert!(drops.motes.iter().all(|mote| mote.color[0] > 0.9));
        let before = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.05));
        assert!(drops.motes[0].center[1] > before, "embers shoot upward first");
        assert!(drops.tick_motes(1.5));
        assert!(drops.motes.is_empty());
    }

    #[test]
    fn a_note_mote_rises_and_shifts_color_with_pitch() {
        let mut drops = Drops::new();
        drops.note([4.5, 2.5, 4.5], 0);
        assert_eq!(drops.motes.len(), 3);
        assert!(drops.motes.iter().all(|mote| mote.rise));
        let low = note_color(0);
        let high = note_color(12);
        assert!(drops.motes[0].color == low);
        assert!(high[2] > low[2], "high notes lean cooler");
        assert!(high[0] < low[0], "high notes lose red");
        let before = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.15));
        assert!(drops.motes[0].center[1] > before);
        assert!(drops.tick_motes(2.0));
        assert!(drops.motes.is_empty());
    }

    #[test]
    fn book_swirl_rises_in_purple() {
        let mut drops = Drops::new();
        drops.book_swirl([3.5, 1.0, 3.5]);
        assert_eq!(drops.motes.len(), 6);
        assert!(drops.motes.iter().all(|mote| mote.rise));
        assert!(drops.motes.iter().all(|mote| mote.color[2] > mote.color[1]));
        let before = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.15));
        assert!(drops.motes[0].center[1] > before);
    }

    #[test]
    fn brew_bubbles_rise_in_cyan() {
        let mut drops = Drops::new();
        drops.brew_bubbles([2.5, 1.0, 2.5]);
        assert_eq!(drops.motes.len(), 6);
        assert!(drops.motes.iter().all(|mote| mote.rise));
        assert!(drops.motes.iter().all(|mote| mote.color[2] >= mote.color[0]));
        let before = drops.motes[0].center[1];
        assert!(drops.tick_motes(0.15));
        assert!(drops.motes[0].center[1] > before);
    }

    #[test]
    fn pollen_trail_hangs_warm_and_caps() {
        let mut drops = Drops::new();
        drops.pollen_trail([5.0, 2.5, 5.0]);
        assert_eq!(drops.motes.len(), 4);
        assert!(drops.motes.iter().all(|mote| mote.hover));
        assert!(drops.motes.iter().all(|mote| mote.color[0] > mote.color[2]));
        for _ in 0..20 {
            drops.pollen_trail([5.0, 2.5, 5.0]);
        }
        assert!(drops.motes.len() <= 28);
        assert!(drops.tick_motes(2.0));
        assert!(drops.motes.is_empty());
    }

    #[test]
    fn straw_drifts_hay_colored_and_caps() {
        let mut drops = Drops::new();
        drops.straw([8.0, 3.0, 8.0]);
        assert!(drops.straw_count() >= 4);
        assert!(drops.motes.iter().all(|mote| mote.hover));
        assert!(drops.motes.iter().all(|mote| mote.color[0] > mote.color[2]));
        assert!(drops.motes.iter().all(|mote| mote.velocity[0] > 0.2));
        for _ in 0..20 {
            drops.straw([8.0, 3.0, 8.0]);
        }
        assert!(drops.straw_count() <= 28);
        assert!(drops.tick_motes(3.0));
        assert_eq!(drops.straw_count(), 0);
    }

    #[test]
    fn chaff_lifts_gold_green_and_caps() {
        let mut drops = Drops::new();
        drops.chaff([12.0, 2.5, 18.0]);
        assert!(drops.chaff_count() >= 4);
        assert_eq!(drops.straw_count(), 0);
        assert!(drops.motes.iter().all(|mote| mote.hover));
        assert!(drops.motes.iter().all(|mote| mote.color[1] >= mote.color[0]));
        for _ in 0..20 {
            drops.chaff([12.0, 2.5, 18.0]);
        }
        assert!(drops.chaff_count() <= 30);
        assert!(drops.tick_motes(3.0));
        assert_eq!(drops.chaff_count(), 0);
    }

    #[test]
    fn rain_splash_pops_and_ripples_on_the_ground() {
        let mut drops = Drops::new();
        drops.rain_splash([8.0, 2.0, 8.0]);
        assert!(drops.rain_splash_count() >= 4);
        assert!(drops.motes.iter().any(|mote| !mote.rise && !mote.hover));
        assert!(drops
            .motes
            .iter()
            .any(|mote| mote.rise && mote.hover && mote.color[2] > mote.color[0]));
        let before = drops.rain_splash_count();
        for _ in 0..40 {
            drops.rain_splash([8.0, 2.0, 8.0]);
        }
        assert!(drops.rain_splash_count() <= 72);
        assert!(drops.rain_splash_count() >= before);
        assert!(drops.tick_motes(0.08));
        assert!(drops.rain_splash_count() > 0);
        assert!(drops.tick_motes(1.5));
        assert_eq!(drops.rain_splash_count(), 0);
    }

    #[test]
    fn snow_prints_sit_and_fade_capped() {
        let mut drops = Drops::new();
        drops.snow_print([8.0, 1.1, 8.0]);
        assert_eq!(drops.snow_print_count(), 1);
        assert!(drops.motes.iter().all(|mote| mote.print));
        let before = drops.motes[0].color[0];
        assert!(drops.tick_motes(1.0));
        assert!(drops.motes[0].color[0] < before);
        assert!((drops.motes[0].center[1] - 1.1).abs() < 1.0e-4);
        for i in 0..50 {
            drops.snow_print([8.0 + i as f32 * 0.2, 1.1, 8.0]);
        }
        assert!(drops.snow_print_count() <= 40);
        assert!(drops.tick_motes(8.0));
        assert_eq!(drops.snow_print_count(), 0);
    }

    #[test]
    fn frost_breath_drifts_forward_and_stays_capped() {
        let mut drops = Drops::new();
        drops.frost_breath([8.0, 2.5, 8.0], [0.0, 0.0, -1.0]);
        assert!(drops.frost_breath_count() >= 2);
        assert!(drops.motes.iter().all(|mote| mote.frost));
        let before_z = drops.motes[0].center[2];
        let before_half = drops.motes[0].half;
        assert!(drops.tick_motes(0.15));
        assert!(
            drops.motes[0].center[2] < before_z,
            "puff drifts along look forward"
        );
        assert!(drops.motes[0].half > before_half);
        for _ in 0..40 {
            drops.frost_breath_burst([8.0, 2.5, 8.0], [0.0, 0.0, -1.0]);
        }
        assert!(drops.frost_breath_count() <= 24);
        assert!(drops.tick_motes(2.0));
        assert_eq!(drops.frost_breath_count(), 0);
    }

    #[test]
    fn coal_chips_are_dark_and_iron_chips_are_warm() {
        let mut coal = Drops::new();
        coal.puff([0.0, 1.0, 0.0], chip_color("coal_ore"));
        let mut iron = Drops::new();
        iron.puff([0.0, 1.0, 0.0], chip_color("iron_ore"));
        let mut stone = Drops::new();
        stone.puff([0.0, 1.0, 0.0], chip_color("stone"));
        let dark = coal.vertices()[0].color;
        let warm = iron.vertices()[0].color;
        let pale = stone.vertices()[0].color;
        assert!(dark[0] < 0.3 && dark[1] < 0.3 && dark[2] < 0.3);
        assert!(warm[0] > warm[2]);
        assert!(warm[0] > pale[0]);
        assert!((pale[0] - pale[2]).abs() < 0.15);
        assert_ne!(dark, warm);
        assert_eq!(chip_color("cobblestone"), chip_color("stone"));
    }

    fn within(position: [f32; 3], center: [f32; 3]) -> bool {
        (0..3).all(|axis| (position[axis] - center[axis]).abs() <= HALF + 1.0e-4)
    }
}
