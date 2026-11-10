//! Short sounds for steps, breaks, and places.
//!
//! Each ear traces the straight path to the sound and three paths that step
//! around it. A full cube on a path keeps part of the energy, so a wall
//! muffles that ear and a clear way around stays louder. The two ears are
//! separate channels: a sound on the right is louder in the right ear, and a
//! block in front of one ear does not muffle the other. Settings volume
//! mutes all of them. The waves are generated here and played through the
//! system audio device.

/// One shot the game can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// A step on something that is not dirt, stone, wood, or water.
    Step,
    /// A soft step on dirt, grass, sand, or gravel.
    StepDirt,
    /// A short click on stone, cobble, or ore.
    StepStone,
    /// A hollow knock on a log, stair, or fence.
    StepWood,
    /// A quiet splash while the feet are in water.
    StepWater,
    /// A splash when the body enters water.
    Splash,
    /// A quieter splash when the body leaves water.
    SplashOut,
    /// A stroke while swimming. Slower and softer than a step.
    Swim,
    Break,
    /// A low crumble when coal ore breaks.
    BreakCoal,
    /// A bright ring when iron ore breaks.
    BreakIron,
    Place,
    /// A note-block tone. The pitch is 0 through 15.
    Note(u8),
    /// A goat-horn call. Two tones, low then high.
    Horn,
    /// A heavy mace hit.
    Mace,
    /// A wind-charge burst.
    Gust,
    /// A short soft hiss while the afternoon fog is up.
    Rain,
    /// Two short crunches when food is eaten.
    Bite,
    /// A quiet swallow when a potion is drunk.
    Sip,
    /// A soft exhale after a sprint.
    Breath,
}

/// Seconds before the same horn can sound again.
pub const HORN_WAIT: f32 = 2.0;

/// `Some` when the horn may sound, carrying the cooldown to store.
pub fn horn_blow(wait: f32) -> Option<f32> {
    if wait > 0.0 { None } else { Some(HORN_WAIT) }
}

/// The step that fits the block under the feet. Anything else stays the plain step.
pub fn step_for(id: &str) -> Sound {
    if id == "water" {
        return Sound::StepWater;
    }
    if dirt_name(id) {
        return Sound::StepDirt;
    }
    if stone_name(id) {
        return Sound::StepStone;
    }
    if wood_name(id) {
        return Sound::StepWood;
    }
    Sound::Step
}

/// Enter or leave a water body. Still water stays quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterEdge {
    Enter,
    Exit,
}

/// A splash on the frame the body enters or leaves water.
pub fn water_edge(was_wet: bool, wet: bool) -> Option<WaterEdge> {
    match (was_wet, wet) {
        (false, true) => Some(WaterEdge::Enter),
        (true, false) => Some(WaterEdge::Exit),
        _ => None,
    }
}

impl WaterEdge {
    pub fn sound(self) -> Sound {
        match self {
            Self::Enter => Sound::Splash,
            Self::Exit => Sound::SplashOut,
        }
    }
}

/// The break that fits the block. Coal crumbles, iron rings, and everything else snaps.
pub fn break_for(id: &str) -> Sound {
    match id {
        "coal_ore" => Sound::BreakCoal,
        "iron_ore" => Sound::BreakIron,
        _ => Sound::Break,
    }
}

fn dirt_name(id: &str) -> bool {
    id == "dirt"
        || id == "grass"
        || id == "gravel"
        || id == "clay"
        || id == "mud"
        || id == "farmland"
        || id == "podzol"
        || id == "mycelium"
        || id == "moss"
        || id.ends_with("_nylium")
        || (id.contains("sand") && !id.contains("stone"))
}

fn stone_name(id: &str) -> bool {
    id.contains("stone")
        || id.contains("cobble")
        || id.contains("deepslate")
        || id.contains("brick")
        || id.contains("basalt")
        || id.contains("ore")
        || id.contains("andesite")
        || id.contains("diorite")
        || id.contains("granite")
        || id.contains("obsidian")
        || id.contains("bedrock")
        || id.contains("netherrack")
        || id.contains("blackstone")
        || id.contains("purpur")
        || id.contains("tuff")
        || id.contains("calcite")
        || id.contains("copper")
}

fn wood_name(id: &str) -> bool {
    matches!(
        id,
        "door"
            | "fence"
            | "fence_gate"
            | "trapdoor"
            | "crafting_table"
            | "chest"
            | "barrel"
            | "bookshelf"
            | "ladder"
            | "sign"
            | "oak_planks"
            | "planks"
    ) || id.contains("log")
        || id.contains("plank")
        || id.contains("oak")
        || id.contains("spruce")
        || id.contains("birch")
        || id.contains("jungle")
        || id.contains("acacia")
        || id.contains("mangrove")
        || id.contains("cherry")
        || id.contains("bamboo")
        || id.contains("crimson")
        || id.contains("warped")
}

/// The cooldown after `dt` seconds. It stays at zero.
pub fn horn_cool(wait: f32, dt: f32) -> f32 {
    (wait - dt).max(0.0)
}

/// A above middle C, then one semitone higher for each step.
pub fn note_hz(pitch: u8) -> f32 {
    let step = f32::from(pitch & 0x0F);
    220.0 * 2.0_f32.powf(step / 12.0)
}

/// The next pitch after this one. Fifteen wraps back to zero.
pub fn next_pitch(pitch: u8) -> u8 {
    pitch.wrapping_add(1) & 0x0F
}

/// A short name for the pitch, from C up through D# an octave higher.
pub fn note_name(pitch: u8) -> &'static str {
    const NAMES: [&str; 16] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B", "C5", "C#5", "D5", "D#5",
    ];
    NAMES[(pitch & 0x0F) as usize]
}

/// How loud each ear is, and how open its path was (`1` is clear).
#[derive(Clone, Copy, Debug)]
pub struct Hearing {
    pub left: f32,
    pub right: f32,
    pub left_open: f32,
    pub right_open: f32,
}

impl Hearing {
    /// Both ears the same, with no world in the way.
    pub fn even(level: f32) -> Self {
        Self {
            left: level,
            right: level,
            left_open: 1.0,
            right_open: 1.0,
        }
    }

    /// Scale ear loudness. Openness stays put.
    pub fn attenuate(self, factor: f32) -> Self {
        let factor = factor.clamp(0.0, 1.0);
        Self {
            left: self.left * factor,
            right: self.right * factor,
            left_open: self.left_open,
            right_open: self.right_open,
        }
    }
}

pub struct Audio {
    muted: bool,
    step_left: f32,
    stroke_left: f32,
    step_wave: Vec<i16>,
    dirt_wave: Vec<i16>,
    stone_wave: Vec<i16>,
    wood_wave: Vec<i16>,
    water_wave: Vec<i16>,
    break_wave: Vec<i16>,
    coal_wave: Vec<i16>,
    iron_wave: Vec<i16>,
    place_wave: Vec<i16>,
    /// The wave `PlaySound` is still reading. The next sound replaces it.
    last_wave: Vec<u8>,
    #[cfg(test)]
    played: Vec<Sound>,
}

impl Audio {
    pub fn new() -> Self {
        Self {
            muted: false,
            step_left: 0.0,
            stroke_left: 0.0,
            step_wave: thud(140.0, 0.07, 0.28),
            dirt_wave: dirt_step(),
            stone_wave: stone_step(),
            wood_wave: wood_step(),
            water_wave: water_step(),
            break_wave: crack_wave(),
            coal_wave: coal_crack(),
            iron_wave: iron_ring(),
            place_wave: place_thud(),
            last_wave: Vec::new(),
            #[cfg(test)]
            played: Vec::new(),
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn muted(&self) -> bool {
        self.muted
    }

    /// True on the frame a step should play. The caller places it in the world.
    ///
    /// Sneak steps are farther apart so the walk reads quieter and slower.
    pub fn footstep(&mut self, moving: bool, dt: f32, sneak: bool) -> bool {
        if !moving {
            self.step_left = 0.0;
            return false;
        }
        self.step_left -= dt;
        if self.step_left <= 0.0 {
            self.step_left = if sneak { 0.72 } else { 0.42 };
            return true;
        }
        false
    }

    /// True on the frame a swim stroke should play. Standing still resets it.
    pub fn stroke(&mut self, swimming: bool, dt: f32) -> bool {
        if !swimming {
            self.stroke_left = 0.0;
            return false;
        }
        self.stroke_left -= dt;
        if self.stroke_left <= 0.0 {
            self.stroke_left = 0.72;
            return true;
        }
        false
    }

    pub fn play(&mut self, sound: Sound) {
        self.play_heard(sound, Hearing::even(1.0));
    }

    pub fn play_heard(&mut self, sound: Sound, heard: Hearing) {
        if self.muted {
            return;
        }
        let mono = self.samples(sound);
        self.last_wave = wav_stereo(&stereo(&mono, heard));
        #[cfg(test)]
        {
            assert!(self.last_wave.len() > 44);
            assert_eq!(
                u16::from_le_bytes([self.last_wave[22], self.last_wave[23]]),
                2
            );
            self.played.push(sound);
        }
        #[cfg(not(test))]
        play_wave(&self.last_wave);
    }

    fn samples(&self, sound: Sound) -> Vec<i16> {
        match sound {
            Sound::Step => self.step_wave.clone(),
            Sound::StepDirt => self.dirt_wave.clone(),
            Sound::StepStone => self.stone_wave.clone(),
            Sound::StepWood => self.wood_wave.clone(),
            Sound::StepWater => self.water_wave.clone(),
            Sound::Splash => splash_wave(),
            Sound::SplashOut => splash_out_wave(),
            Sound::Swim => swim_wave(),
            Sound::Break => self.break_wave.clone(),
            Sound::BreakCoal => self.coal_wave.clone(),
            Sound::BreakIron => self.iron_wave.clone(),
            Sound::Place => self.place_wave.clone(),
            Sound::Note(pitch) => thud(note_hz(pitch), 0.22, 0.34),
            Sound::Horn => horn_call(),
            Sound::Mace => thud(70.0, 0.16, 0.5),
            Sound::Gust => thud(520.0, 0.12, 0.28),
            Sound::Rain => rain_hiss(),
            Sound::Bite => bite(),
            Sound::Sip => sip(),
            Sound::Breath => breath(),
        }
    }
}

const EAR: f32 = 0.18;
/// Energy that gets through one full cube.
const PASS: f32 = 0.45;
/// A path that steps around the line is quieter than the straight one.
const AROUND: f32 = 0.55;
const BEND: f32 = 1.5;

/// Left and right ears. `right` is the listener's right, on the horizontal.
pub fn hear(
    source: [f32; 3],
    eye: [f32; 3],
    forward: [f32; 3],
    right: [f32; 3],
    solid: impl Fn(i32, i32, i32) -> bool,
) -> Hearing {
    let right = unit(right);
    let forward = unit(forward);
    let left_ear = add(eye, scale(right, -EAR));
    let right_ear = add(eye, scale(right, EAR));
    let to_source = sub(source, eye);
    let dist = length(to_source).max(0.2);
    let dir = scale(to_source, 1.0 / dist);
    let side = dot(dir, right).clamp(-1.0, 1.0);
    let facing = dot(dir, forward).clamp(-1.0, 1.0);
    // The head shadows a sound behind the listener.
    let rear = 0.65 + 0.35 * ((facing + 1.0) * 0.5);
    let left_pan = ((1.0 - side) * 0.5).sqrt();
    let right_pan = ((1.0 + side) * 0.5).sqrt();
    let falloff = rear / (1.0 + dist * 0.4);
    let left_open = openness(left_ear, source, &solid);
    let right_open = openness(right_ear, source, &solid);
    Hearing {
        left: left_pan * falloff * left_open,
        right: right_pan * falloff * right_open,
        left_open,
        right_open,
    }
}

fn openness(from: [f32; 3], to: [f32; 3], solid: &impl Fn(i32, i32, i32) -> bool) -> f32 {
    let direct = transmit(solids_between(from, to, solid));
    let mid = lerp(from, to, 0.5);
    let side = side_axis(sub(to, from));
    let bends = [
        add(mid, scale(side, BEND)),
        add(mid, scale(side, -BEND)),
        add(mid, [0.0, BEND, 0.0]),
    ];
    let mut around = 0.0f32;
    for via in bends {
        let blocks = solids_between(from, via, solid) + solids_between(via, to, solid);
        around = around.max(transmit(blocks) * AROUND);
    }
    (direct + around * (1.0 - direct)).clamp(0.0, 1.0)
}

fn transmit(blocks: u32) -> f32 {
    if blocks == 0 {
        1.0
    } else {
        PASS.powi(blocks as i32)
    }
}

/// Full cubes the segment crosses, not counting the cell it starts or ends in.
fn solids_between(from: [f32; 3], to: [f32; 3], solid: &impl Fn(i32, i32, i32) -> bool) -> u32 {
    let dir = sub(to, from);
    if length(dir) < 0.05 {
        return 0;
    }
    let mut cell = cell_of(from);
    let end = cell_of(to);
    let step = [axis_step(dir[0]), axis_step(dir[1]), axis_step(dir[2])];
    let t_delta = [axis_delta(dir[0]), axis_delta(dir[1]), axis_delta(dir[2])];
    let mut t_max = [
        next_t(from[0], cell[0], step[0], dir[0]),
        next_t(from[1], cell[1], step[1], dir[1]),
        next_t(from[2], cell[2], step[2], dir[2]),
    ];
    let mut count = 0u32;
    for _ in 0..48 {
        if cell == end {
            break;
        }
        let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[axis] > 1.0 {
            break;
        }
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if cell == end {
            break;
        }
        if solid(cell[0], cell[1], cell[2]) {
            count += 1;
        }
    }
    count
}

fn cell_of(point: [f32; 3]) -> [i32; 3] {
    [
        point[0].floor() as i32,
        point[1].floor() as i32,
        point[2].floor() as i32,
    ]
}

fn axis_step(dir: f32) -> i32 {
    if dir > 1.0e-8 {
        1
    } else if dir < -1.0e-8 {
        -1
    } else {
        0
    }
}

fn axis_delta(dir: f32) -> f32 {
    if dir.abs() < 1.0e-8 {
        f32::MAX
    } else {
        (1.0 / dir).abs()
    }
}

fn next_t(origin: f32, cell: i32, step: i32, dir: f32) -> f32 {
    if step == 0 {
        return f32::MAX;
    }
    let bound = if step > 0 {
        cell as f32 + 1.0
    } else {
        cell as f32
    };
    let t = (bound - origin) / dir;
    if t < 0.0 { 0.0 } else { t }
}

fn side_axis(dir: [f32; 3]) -> [f32; 3] {
    let side = [-dir[2], 0.0, dir[0]];
    let len = (side[0] * side[0] + side[2] * side[2]).sqrt();
    if len < 1.0e-4 {
        [1.0, 0.0, 0.0]
    } else {
        [side[0] / len, 0.0, side[2] / len]
    }
}

fn stereo(mono: &[i16], heard: Hearing) -> Vec<i16> {
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    let left_follow = 0.15 + 0.85 * heard.left_open.clamp(0.0, 1.0);
    let right_follow = 0.15 + 0.85 * heard.right_open.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity(mono.len() * 2);
    for sample in mono {
        let input = *sample as f32;
        left += (input - left) * left_follow;
        right += (input - right) * right_follow;
        out.push((left * heard.left).clamp(-32767.0, 32767.0) as i16);
        out.push((right * heard.right).clamp(-32767.0, 32767.0) as i16);
    }
    out
}

/// A low tone, then a higher one. Longer than a note-block tap.
fn horn_call() -> Vec<i16> {
    let mut call = thud(196.0, 0.28, 0.4);
    call.extend(thud(294.0, 0.42, 0.36));
    call
}

/// A noisy snap, then a low body. Louder at the start than a plain tone.
fn crack_wave() -> Vec<i16> {
    let seconds = 0.14;
    let count = (22_050.0 * seconds) as usize;
    let mut noise = 1u32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let snap = if t < 0.035 {
            n * (1.0 - t / 0.035)
        } else {
            0.0
        };
        let body = (t * 95.0 * std::f32::consts::TAU).sin() * (1.0 - t / seconds).max(0.0);
        let mixed = (snap * 0.7 + body * 0.45).clamp(-1.0, 1.0);
        samples.push((mixed * 32_767.0) as i16);
    }
    samples
}

/// A dull low crumble. Quieter and slower than the stone snap.
fn coal_crack() -> Vec<i16> {
    let seconds = 0.16;
    let count = (22_050.0 * seconds) as usize;
    let mut noise = 9u32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let grit = if t < 0.05 {
            n * (1.0 - t / 0.05) * 0.25
        } else {
            0.0
        };
        let body = (t * 62.0 * std::f32::consts::TAU).sin() * (1.0 - t / seconds).max(0.0);
        let mixed = (grit + body * 0.7).clamp(-1.0, 1.0);
        samples.push((mixed * 28_000.0) as i16);
    }
    samples
}

/// A short tick, then a bright ring that fades.
fn iron_ring() -> Vec<i16> {
    let seconds = 0.22;
    let count = (22_050.0 * seconds) as usize;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        let click = if t < 0.012 { 1.0 - t / 0.012 } else { 0.0 };
        let ring = (t * 880.0 * std::f32::consts::TAU).sin() * (-t * 14.0).exp();
        let mixed = (click * 0.8 + ring * 0.55).clamp(-1.0, 1.0);
        samples.push((mixed * 30_000.0) as i16);
    }
    samples
}

/// A short low hit. Deeper than the old place click.
fn place_thud() -> Vec<i16> {
    thud(150.0, 0.1, 0.55)
}

/// A muffled low step. Longer and quieter than stone.
fn dirt_step() -> Vec<i16> {
    thud(90.0, 0.09, 0.2)
}

/// A short high click.
fn stone_step() -> Vec<i16> {
    thud(520.0, 0.03, 0.4)
}

/// A mid knock, with a softer overtone so it is not a pure hum.
fn wood_step() -> Vec<i16> {
    let knock = thud(190.0, 0.055, 0.28);
    let over = thud(380.0, 0.04, 0.1);
    knock
        .into_iter()
        .zip(over.into_iter().chain(std::iter::repeat(0)))
        .map(|(a, b)| a.saturating_add(b))
        .collect()
}

/// Two crunches with a short gap, like a bite.
fn bite() -> Vec<i16> {
    let mut wave = thud(220.0, 0.045, 0.24);
    wave.extend(std::iter::repeat(0).take((22_050.0 * 0.04) as usize));
    wave.extend(thud(160.0, 0.05, 0.16));
    wave
}

/// A soft swallow. Quieter than a splash.
fn sip() -> Vec<i16> {
    let seconds = 0.16;
    let count = (22_050.0 * seconds) as usize;
    let mut noise = 19u32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let env = (t / seconds).min(1.0 - t / seconds).max(0.0) * 2.0;
        samples.push((n * 0.08 * env * 32_767.0) as i16);
    }
    samples
}

/// A soft exhale after a sprint. Longer and quieter than a sip.
fn breath() -> Vec<i16> {
    let seconds = 0.28;
    let count = (22_050.0 * seconds) as usize;
    let mut noise = 41u32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let tone = (t * 90.0 * std::f32::consts::TAU).sin() * 0.35;
        let env = (t / 0.06).min(1.0) * (1.0 - t / seconds).max(0.0);
        samples.push(((n * 0.55 + tone) * 0.07 * env * 32_767.0) as i16);
    }
    samples
}

/// A quiet hiss. Shorter and softer than a footstep, so a step still cuts through.
fn rain_hiss() -> Vec<i16> {
    let seconds = 0.28;
    let count = (22_050.0 * seconds) as usize;
    let mut noise = 11u32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let env = (1.0 - t / seconds).max(0.0);
        samples.push((n * 0.1 * env * 32_767.0) as i16);
    }
    samples
}

/// Entering water. Louder and longer than a step or an exit splash.
fn splash_wave() -> Vec<i16> {
    let mut wave = noise_burst(0.18, 0.62, 23);
    wave.extend(noise_burst(0.14, 0.28, 29));
    wave
}

/// Leaving water. Softer than the enter splash.
fn splash_out_wave() -> Vec<i16> {
    noise_burst(0.16, 0.32, 31)
}

/// A swim stroke. A soft noise, not a footstep tone.
fn swim_wave() -> Vec<i16> {
    noise_burst(0.16, 0.14, 41)
}

/// A soft splash. Noise, not a tone.
fn water_step() -> Vec<i16> {
    noise_burst(0.08, 0.22, 9)
}

fn noise_burst(seconds: f32, volume: f32, seed: u32) -> Vec<i16> {
    let count = (22_050.0 * seconds) as usize;
    let mut noise = seed;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 16) as i32 % 2001 - 1000) as f32 / 1000.0;
        let env = (1.0 - t / seconds).max(0.0);
        samples.push((n * volume * env * 32_767.0) as i16);
    }
    samples
}

fn thud(freq: f32, seconds: f32, volume: f32) -> Vec<i16> {
    let count = (22_050.0 * seconds) as usize;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / 22_050.0;
        let env = (1.0 - t / seconds).max(0.0);
        let tone = (t * freq * std::f32::consts::TAU).sin();
        samples.push((tone * volume * env * 32_767.0) as i16);
    }
    samples
}

#[cfg(test)]
fn wav(samples: &[i16]) -> Vec<u8> {
    wav_pcm(samples, 1)
}

fn wav_stereo(samples: &[i16]) -> Vec<u8> {
    wav_pcm(samples, 2)
}

fn wav_pcm(samples: &[i16], channels: u16) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let block = channels as u32 * 2;
    let rate = 22_050u32;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let len = length(a);
    if len < 1.0e-5 {
        [1.0, 0.0, 0.0]
    } else {
        scale(a, 1.0 / len)
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(scale(a, 1.0 - t), scale(b, t))
}

#[cfg(all(windows, not(test)))]
fn play_wave(bytes: &[u8]) {
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;
    unsafe {
        PlaySoundA(
            bytes.as_ptr(),
            std::ptr::null_mut(),
            SND_ASYNC | SND_NODEFAULT | SND_MEMORY,
        );
    }
}

#[cfg(all(not(windows), not(test)))]
fn play_wave(_bytes: &[u8]) {}

#[cfg(all(windows, not(test)))]
#[link(name = "winmm")]
unsafe extern "system" {
    fn PlaySoundA(sound: *const u8, module: *mut core::ffi::c_void, flags: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye() -> [f32; 3] {
        [8.0, 1.5, 8.0]
    }

    fn ahead(z: f32) -> [f32; 3] {
        [8.0, 1.5, z]
    }

    #[test]
    fn a_wave_is_audible_until_volume_mutes_it() {
        let wave = wav(&thud(200.0, 0.05, 0.4));
        assert!(wave.starts_with(b"RIFF"));
        assert!(wave.windows(4).any(|mark| mark == b"data"));
        assert!(wave.iter().skip(44).any(|byte| *byte != 0));

        let mut audio = Audio::new();
        assert!(!audio.muted());
        assert!(!audio.footstep(false, 1.0, false));
        assert!(audio.played.is_empty());
        assert!(audio.footstep(true, 0.1, false));
        audio.play(Sound::Step);
        audio.play(Sound::Break);
        audio.set_muted(true);
        audio.play(Sound::Place);
        assert!(audio.footstep(true, 1.0, false));
        audio.play(Sound::Step);
        assert_eq!(audio.played, vec![Sound::Step, Sound::Break]);
    }

    #[test]
    fn sneak_steps_are_farther_apart_and_softer() {
        let mut audio = Audio::new();
        assert!(audio.footstep(true, 0.01, true));
        // Sneak interval is 0.72; a normal 0.42 gap must not fire again yet.
        assert!(!audio.footstep(true, 0.42, true));
        assert!(!audio.footstep(true, 0.20, true));
        assert!(audio.footstep(true, 0.20, true));
        let loud = Hearing::even(1.0);
        let soft = loud.attenuate(0.35);
        assert!((soft.left - 0.35).abs() < 1e-4);
        assert!((soft.right - 0.35).abs() < 1e-4);
        assert_eq!(soft.left_open, 1.0);
    }

    #[test]
    fn a_break_snaps_and_a_place_is_a_low_thud() {
        let crack = crack_wave();
        let place = place_thud();
        let peak = |wave: &[i16]| {
            wave.iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap_or(0)
        };
        assert!(peak(&crack) > peak(&place));
        assert!(crack.len() > place.len());
        let early = peak(&crack[..400]);
        let late = peak(&crack[crack.len() - 400..]);
        assert!(early > late, "the snap is at the start");
    }

    #[test]
    fn coal_crumbles_and_iron_rings() {
        assert_eq!(break_for("coal_ore"), Sound::BreakCoal);
        assert_eq!(break_for("iron_ore"), Sound::BreakIron);
        assert_eq!(break_for("stone"), Sound::Break);
        assert_eq!(break_for("cobblestone"), Sound::Break);
        assert_eq!(break_for("deepslate"), Sound::Break);
        let stone = crack_wave();
        let coal = coal_crack();
        let iron = iron_ring();
        assert_ne!(stone, coal);
        assert_ne!(stone, iron);
        let rises = |wave: &[i16]| {
            wave.windows(2)
                .filter(|pair| pair[0] < 0 && pair[1] >= 0)
                .count()
        };
        assert!(rises(&coal) < rises(&stone), "coal is the low crumble");
        assert!(rises(&iron) > rises(&stone), "iron is the bright ring");
        let mut audio = Audio::new();
        audio.play(Sound::BreakCoal);
        audio.play(Sound::BreakIron);
        assert_eq!(audio.played, vec![Sound::BreakCoal, Sound::BreakIron]);
    }

    #[test]
    fn dirt_stone_wood_and_water_steps_are_different() {
        assert_eq!(step_for("dirt"), Sound::StepDirt);
        assert_eq!(step_for("grass"), Sound::StepDirt);
        assert_eq!(step_for("sand"), Sound::StepDirt);
        assert_eq!(step_for("soul_sand"), Sound::StepDirt);
        assert_eq!(step_for("stone"), Sound::StepStone);
        assert_eq!(step_for("cobblestone"), Sound::StepStone);
        assert_eq!(step_for("sandstone"), Sound::StepStone);
        assert_eq!(step_for("deepslate_bricks"), Sound::StepStone);
        assert_eq!(step_for("oak_log"), Sound::StepWood);
        assert_eq!(step_for("oak_stairs"), Sound::StepWood);
        assert_eq!(step_for("fence"), Sound::StepWood);
        assert_eq!(step_for("copper_door"), Sound::StepStone);
        assert_eq!(step_for("water"), Sound::StepWater);
        assert_eq!(step_for("wool"), Sound::Step);

        let dirt = dirt_step();
        let stone = stone_step();
        let wood = wood_step();
        let water = water_step();
        assert!(stone.len() < wood.len() && wood.len() < dirt.len());
        let rises = |wave: &[i16]| {
            wave.windows(2)
                .filter(|pair| pair[0] < 0 && pair[1] >= 0)
                .count()
        };
        assert!(rises(&stone) > rises(&dirt));
        let jitter = |wave: &[i16]| {
            wave.windows(2)
                .map(|pair| (i32::from(pair[1]) - i32::from(pair[0])).unsigned_abs())
                .sum::<u32>()
                / wave.len() as u32
        };
        assert!(jitter(&water) > jitter(&stone));
        let rain = rain_hiss();
        let peak = |wave: &[i16]| {
            wave.iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap_or(0)
        };
        assert!(peak(&rain) < peak(&stone));
        let bite = bite();
        let mut run = 0u32;
        let mut gap = 0u32;
        for sample in &bite {
            if *sample == 0 {
                run += 1;
                gap = gap.max(run);
            } else {
                run = 0;
            }
        }
        assert!(gap > 200, "a bite has a gap between crunches");
        assert!(peak(&sip()) < peak(&water));
    }

    #[test]
    fn a_splash_marks_the_water_and_a_stroke_is_not_a_step() {
        assert_eq!(water_edge(false, true), Some(WaterEdge::Enter));
        assert_eq!(water_edge(true, false), Some(WaterEdge::Exit));
        assert!(water_edge(false, false).is_none());
        assert!(water_edge(true, true).is_none());
        assert_eq!(WaterEdge::Enter.sound(), Sound::Splash);
        assert_eq!(WaterEdge::Exit.sound(), Sound::SplashOut);
        let step = water_step();
        let splash = splash_wave();
        let splash_out = splash_out_wave();
        let swim = swim_wave();
        let stone = stone_step();
        let peak = |wave: &[i16]| {
            wave.iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap_or(0)
        };
        assert!(splash.len() > step.len());
        assert!(
            peak(&splash) > peak(&step),
            "entering water is louder than a step"
        );
        assert!(
            peak(&splash) > peak(&splash_out),
            "entering is louder than leaving"
        );
        assert_ne!(swim, step);
        assert_ne!(swim, stone);
        let jitter = |wave: &[i16]| {
            wave.windows(2)
                .map(|pair| (i32::from(pair[1]) - i32::from(pair[0])).unsigned_abs())
                .sum::<u32>()
                / wave.len() as u32
        };
        assert!(
            jitter(&swim) > jitter(&stone),
            "a stroke is noise, not a click"
        );
        let mut audio = Audio::new();
        assert!(!audio.stroke(false, 1.0));
        assert!(audio.stroke(true, 0.1));
        assert!(!audio.stroke(true, 0.2));
        audio.play(Sound::Splash);
        audio.play(Sound::Swim);
        assert_eq!(audio.played, vec![Sound::Splash, Sound::Swim]);
    }

    #[test]
    fn a_note_block_steps_up_the_scale_and_wraps() {
        assert!(note_hz(1) > note_hz(0));
        assert!((note_hz(12) / note_hz(0) - 2.0).abs() < 1.0e-3);
        assert_eq!(next_pitch(0), 1);
        assert_eq!(next_pitch(15), 0);
        assert_eq!(note_name(0), "C");
        assert_eq!(note_name(15), "D#5");
        let low = thud(note_hz(0), 0.05, 1.0);
        let high = thud(note_hz(12), 0.05, 1.0);
        let rises = |wave: &[i16]| {
            wave.windows(2)
                .filter(|pair| pair[0] < 0 && pair[1] >= 0)
                .count()
        };
        assert!(
            rises(&high) > rises(&low),
            "an octave up should cross zero more often"
        );
        let mut audio = Audio::new();
        audio.play(Sound::Note(0));
        audio.play(Sound::Note(12));
        assert_eq!(audio.played, vec![Sound::Note(0), Sound::Note(12)]);
        audio.play(Sound::Horn);
        audio.play(Sound::Mace);
        assert_eq!(audio.played.last(), Some(&Sound::Mace));
        let tap = thud(note_hz(0), 0.22, 0.34);
        assert!(horn_call().len() > tap.len());
        assert_eq!(horn_blow(0.0), Some(HORN_WAIT));
        assert_eq!(horn_blow(0.4), None);
        assert_eq!(horn_cool(HORN_WAIT, 0.5), 1.5);
        assert_eq!(horn_cool(0.2, 1.0), 0.0);
    }

    #[test]
    fn the_right_ear_hears_the_right_side_and_a_wall_muffles_both() {
        let forward = [0.0, 0.0, 1.0];
        let right = [1.0, 0.0, 0.0];
        let open_air = |_: i32, _: i32, _: i32| false;
        let near = hear(ahead(10.0), eye(), forward, right, open_air);
        let far = hear(ahead(20.0), eye(), forward, right, open_air);
        assert!(
            near.left + near.right > far.left + far.right,
            "a nearer sound is louder"
        );

        let side = hear([14.0, 1.5, 10.0], eye(), forward, right, open_air);
        assert!(
            side.right > side.left * 1.4,
            "a sound on the right is louder in the right ear: {side:?}"
        );

        let clear = hear(ahead(14.0), eye(), forward, right, open_air);
        let walled = hear(ahead(14.0), eye(), forward, right, |x, y, z| {
            (6..11).contains(&x) && (0..4).contains(&y) && (10..13).contains(&z)
        });
        assert!(
            walled.left + walled.right < (clear.left + clear.right) * 0.5,
            "a wall between the ears and the sound drops both channels"
        );

        let left_block = hear(ahead(14.0), eye(), forward, right, |x, y, z| {
            x == 7 && y == 1 && z == 10
        });
        assert!(
            left_block.right > left_block.left,
            "a block in front of the left ear leaves the right channel open"
        );
        assert!(
            left_block.left_open < left_block.right_open,
            "the left path is the occluded one"
        );
    }
}
