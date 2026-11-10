//! A two-minute day. Spring noon uses the old fixed sun. The sun rises in the
//! east and sets in the west. Summer climbs higher and stays up longer; winter
//! stays lower and sets sooner. The moon is opposite the sun on that same arc.

use std::f32::consts::TAU;

/// One full day, midnight to midnight, in seconds.
pub const DAY_SECONDS: f32 = 120.0;
/// How many days pass before the season label repeats.
pub const SEASON_DAYS: f32 = 4.0;
/// Sun at noon, before it is normalized: up, from the north, a little from the east.
pub const NOON_SUN: [f32; 3] = [0.25, 1.0, -0.72];

const MOONLIGHT: f32 = 0.22;
#[cfg(test)]
const SUN_FLOOR: f32 = 0.34;
#[cfg(test)]
const CAST_SHADOW: f32 = 0.42;

/// South keeps noon in the north. North mirrors that arc so noon stands in the south.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hemisphere {
    South,
    North,
}

impl Hemisphere {
    pub fn label(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::North => "north",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::South => Self::North,
            Self::North => Self::South,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spring => "Spring",
            Self::Summer => "Summer",
            Self::Autumn => "Autumn",
            Self::Winter => "Winter",
        }
    }

    /// Declination for a southern year. The noon sun already stands in the
    /// north, so summer (negative) lifts it and winter drops it.
    fn declination(self) -> f32 {
        const TILT: f32 = 23.44 * std::f32::consts::PI / 180.0;
        match self {
            Self::Spring | Self::Autumn => 0.0,
            Self::Summer => -TILT,
            Self::Winter => TILT,
        }
    }
}

/// Seconds since the start of spring, wrapping every four days.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    seconds: f32,
}

impl Clock {
    /// Noon on the first day, so the window opens on the old sun.
    pub fn noon() -> Self {
        Self {
            seconds: DAY_SECONDS * 0.5,
        }
    }

    #[cfg(test)]
    pub fn at(seconds: f32) -> Self {
        Self {
            seconds: seconds.rem_euclid(DAY_SECONDS * SEASON_DAYS),
        }
    }

    /// Advance the sky. `dawn` and `dusk` say which marks this step crossed.
    ///
    /// Dawn is a quarter of the way through the day and dusk is three quarters.
    /// A step that skips several of one mark still reports that mark once.
    pub fn tick(&mut self, dt: f32) -> SkyStep {
        if dt <= 0.0 {
            return SkyStep {
                dawn: false,
                dusk: false,
            };
        }
        let step = SkyStep {
            dawn: crosses_mark(self.seconds, dt, 0.25),
            dusk: crosses_mark(self.seconds, dt, 0.75),
        };
        self.seconds = (self.seconds + dt).rem_euclid(DAY_SECONDS * SEASON_DAYS);
        step
    }

    /// Day of the four-day year. `0` is spring and `3` is winter.
    pub fn year_day(self) -> i32 {
        (self.seconds / DAY_SECONDS).floor() as i32
    }

    /// Keep the current day and set the time. `0` is midnight and `0.5` is noon.
    pub fn set_time_of_day(&mut self, time: f32) {
        let day = (self.seconds / DAY_SECONDS).floor();
        let time = time.clamp(0.0, 0.999);
        self.seconds =
            (day * DAY_SECONDS + time * DAY_SECONDS).rem_euclid(DAY_SECONDS * SEASON_DAYS);
    }

    /// `0` midnight, `0.25` dawn, `0.5` noon, `0.75` dusk.
    pub fn time_of_day(self) -> f32 {
        (self.seconds / DAY_SECONDS).fract()
    }

    /// Skip to the next dawn. Evening reaches tomorrow morning, so the season
    /// advances with the day. After midnight, dawn is still this morning.
    /// Daytime stays put. Returns whether the clock moved.
    pub fn sleep(&mut self) -> bool {
        let time = self.time_of_day();
        if (0.25..0.75).contains(&time) {
            return false;
        }
        if time >= 0.75 {
            let advance = (1.0 - time + 0.25) * DAY_SECONDS;
            self.seconds = (self.seconds + advance).rem_euclid(DAY_SECONDS * SEASON_DAYS);
        } else {
            self.set_time_of_day(0.25);
        }
        true
    }

    pub fn season(self) -> Season {
        let day = (self.seconds / DAY_SECONDS).floor() as i32;
        match day.rem_euclid(4) {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        }
    }
}

/// Which daylight marks one sky step crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkyStep {
    pub dawn: bool,
    pub dusk: bool,
}

/// True when moving `dt` seconds forward from `start` reaches `mark`.
///
/// `mark` is a fraction of the day, `0.25` for dawn and `0.75` for dusk.
/// Sitting on the mark waits for the next one.
fn crosses_mark(start: f32, dt: f32, mark: f32) -> bool {
    let day = DAY_SECONDS;
    let cursor = start.rem_euclid(day);
    let mut into = mark * day - cursor;
    if into <= 0.0 {
        into += day;
    }
    dt >= into
}

/// Pixel offset of a held clock's mark. `+x` is right and `+y` is up.
///
/// Noon sits at the top. The mark then travels clockwise through dusk, midnight, and dawn.
pub fn clock_mark(time: f32) -> [f32; 2] {
    const REACH: f32 = 12.0;
    let angle = (time.rem_euclid(1.0) - 0.5) * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    [sin * REACH, cos * REACH]
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub direction: [f32; 3],
    pub brightness: f32,
    pub shadow: f32,
    pub sky: [f32; 3],
    /// Paler band the distance fog fades toward. The clear color stays `sky`.
    pub horizon: [f32; 3],
    pub phase: &'static str,
    pub season: Season,
}

pub fn sample(clock: Clock) -> Light {
    sample_at(clock, Hemisphere::South)
}

pub fn sample_at(clock: Clock, hemisphere: Hemisphere) -> Light {
    let time = clock.time_of_day();
    let sun = sun_on(time, clock.season(), hemisphere);
    let moon = moon_on(time, clock.season(), hemisphere);
    let day = smoothstep(-0.02, 0.45, sun[1]);
    let direction = if sun[1] > 0.0 { sun } else { moon };
    let shadow = if sun[1] > 0.0 {
        smoothstep(0.08, 0.35, sun[1])
    } else {
        0.55 * smoothstep(0.02, 0.40, moon[1])
    };
    Light {
        direction,
        brightness: MOONLIGHT + (1.0 - MOONLIGHT) * day,
        shadow,
        sky: sky(sun[1]),
        horizon: horizon(sun[1]),
        phase: phase(sun[1], time),
        season: clock.season(),
    }
}

/// [`sun_direction`] on the southern arc, or that arc mirrored so noon stands in the south.
pub fn sun_on(time_of_day: f32, season: Season, hemisphere: Hemisphere) -> [f32; 3] {
    let sun = sun_direction(time_of_day, season);
    match hemisphere {
        Hemisphere::South => sun,
        Hemisphere::North => [sun[0], sun[1], -sun[2]],
    }
}

/// Direction from the ground toward the sun.
///
/// Spring and autumn noon match [`NOON_SUN`]. The path is the real daily arc
/// at the latitude that puts that noon sun in the north: hour angle zero at
/// noon, rising in the east. `time_of_day` is `0` at midnight and `0.5` at noon.
pub fn sun_direction(time_of_day: f32, season: Season) -> [f32; 3] {
    let time = time_of_day.rem_euclid(1.0);
    let (phi, yaw) = horizon_frame();
    let dec = season.declination();
    let hour = (time - 0.5) * TAU;
    let (sphi, cphi) = phi.sin_cos();
    let (sdec, cdec) = dec.sin_cos();
    let (sh, ch) = hour.sin_cos();
    // Positive hour is afternoon, so the sun stands in the west.
    let east = -cdec * sh;
    let north = sdec * cphi - cdec * sphi * ch;
    let up = sdec * sphi + cdec * cphi * ch;
    let (sy, cy) = yaw.sin_cos();
    let east_y = east * cy + north * sy;
    let north_y = -east * sy + north * cy;
    normalize([east_y, up, -north_y])
}

/// Latitude and a fixed yaw that makes equinox noon land on [`NOON_SUN`].
fn horizon_frame() -> (f32, f32) {
    let noon = normalize(NOON_SUN);
    let phi = -noon[1].acos();
    let yaw = noon[0].atan2(-noon[2]);
    (phi, yaw)
}

/// The moon sits opposite the sun on the same seasonal arc.
#[cfg_attr(not(test), allow(dead_code))]
pub fn moon_direction(time_of_day: f32, season: Season) -> [f32; 3] {
    sun_direction(time_of_day + 0.5, season)
}

pub fn moon_on(time_of_day: f32, season: Season, hemisphere: Hemisphere) -> [f32; 3] {
    sun_on(time_of_day + 0.5, season, hemisphere)
}

/// Cells toward the light, nearest first. Noon starts at `[0, -1]`.
///
/// The list grows as the light drops toward the horizon, and it is empty when
/// the light is at or below the horizon. A block still shades the grass it
/// stands on.
pub fn shadow_lookups(dir: [f32; 3]) -> Vec<[i32; 2]> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len <= 1.0e-8 || dir[1] <= 0.0 {
        return Vec::new();
    }
    let horiz = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    if horiz <= 1.0e-4 {
        return Vec::new();
    }
    let elev = (dir[1] / len).clamp(0.0, 1.0);
    let length = (1 + ((1.0 - elev) * 4.0).round() as i32).clamp(1, 5);
    let dx = dir[0] / horiz;
    let dz = dir[2] / horiz;
    let mut cells = Vec::new();
    for i in 1..=length {
        let x = (dx * i as f32).round() as i32;
        let z = (dz * i as f32).round() as i32;
        if (x != 0 || z != 0) && !cells.contains(&[x, z]) {
            cells.push([x, z]);
        }
    }
    if cells.is_empty() {
        if dir[0].abs() >= dir[2].abs() {
            cells.push([if dir[0] >= 0.0 { 1 } else { -1 }, 0]);
        } else {
            cells.push([0, if dir[2] >= 0.0 { 1 } else { -1 }]);
        }
    }
    cells
}

/// Share of the ambient term that remains. Block light is added after this.
#[cfg(test)]
pub fn ambient_factor(normal: [f32; 3], light: &Light, shadow: bool) -> f32 {
    let normal = normalize(normal);
    let sun = light.direction;
    let ndotl = (normal[0] * sun[0] + normal[1] * sun[1] + normal[2] * sun[2]).max(0.0);
    let facing = SUN_FLOOR + (1.0 - SUN_FLOOR) * ndotl;
    let shadow_mul = if shadow {
        1.0 - (1.0 - CAST_SHADOW) * light.shadow
    } else {
        1.0
    };
    facing * light.brightness * shadow_mul
}

fn sky(sun_y: f32) -> [f32; 3] {
    let day = [0.53, 0.75, 0.90];
    let night = [0.015, 0.02, 0.06];
    let dusk = [0.90, 0.40, 0.26];
    let day_w = smoothstep(-0.05, 0.55, sun_y);
    let dusk_w = (1.0 - smoothstep(0.0, 0.35, sun_y.abs())) * smoothstep(-0.35, 0.05, sun_y);
    mix(mix(night, day, day_w), dusk, dusk_w)
}

/// A lighter, slightly warmer band under the zenith. Fog fades terrain into this.
fn horizon(sun_y: f32) -> [f32; 3] {
    let day = [0.72, 0.84, 0.96];
    let night = [0.04, 0.05, 0.09];
    let dusk = [0.96, 0.50, 0.32];
    let day_w = smoothstep(-0.05, 0.55, sun_y);
    let dusk_w = (1.0 - smoothstep(0.0, 0.35, sun_y.abs())) * smoothstep(-0.35, 0.05, sun_y);
    mix(mix(night, day, day_w), dusk, dusk_w)
}

/// How thick the afternoon fog is. `0` is a clear sky, `1` is the gray spell.
///
/// It rises after noon and is gone again by dusk. Noon and morning stay clear.
pub fn fog_mood(time_of_day: f32) -> f32 {
    let time = time_of_day.rem_euclid(1.0);
    let dist = (time - 0.70).abs();
    if dist >= 0.12 {
        return 0.0;
    }
    let t = 1.0 - dist / 0.12;
    t * t * (3.0 - 2.0 * t)
}

/// How thick the night fog is. `0` is day, `1` is full night.
///
/// It rises after the afternoon gray spell and is gone again by dawn.
/// Noon, morning, and the gray spell itself stay at `0`.
pub fn night_fog(time_of_day: f32) -> f32 {
    let time = time_of_day.rem_euclid(1.0);
    if time >= 0.82 {
        smoothstep(0.82, 0.90, time)
    } else if time <= 0.22 {
        1.0 - smoothstep(0.10, 0.22, time)
    } else {
        0.0
    }
}

/// Soft dawn bird chorus amount. `0` elsewhere, peaks just after dawn.
///
/// Rises as night fog leaves and fades by mid-morning so noon stays quiet.
pub fn dawn_chorus(time_of_day: f32) -> f32 {
    let time = time_of_day.rem_euclid(1.0);
    if !(0.18..=0.42).contains(&time) {
        return 0.0;
    }
    if time < 0.28 {
        smoothstep(0.18, 0.26, time)
    } else {
        1.0 - smoothstep(0.32, 0.42, time)
    }
}

/// Where the distance fog starts and ends, in blocks.
///
/// Day stays long. The afternoon spell shortens it. Night shortens it again
/// and is the closer of the two.
pub fn fog_span(brightness: f32, time_of_day: f32) -> (f32, f32) {
    let mood = fog_mood(time_of_day);
    let night = night_fog(time_of_day);
    let start = (20.0 + 24.0 * brightness) * (1.0 - mood * 0.62) * (1.0 - night * 0.55);
    let end = (64.0 + 48.0 * brightness) * (1.0 - mood * 0.50) * (1.0 - night * 0.48);
    (start, end)
}

/// Pull `color` toward `gray` by `amount`. `0` leaves it, `1` is fully gray.
pub fn foggy(color: [f32; 3], gray: [f32; 3], amount: f32) -> [f32; 3] {
    mix(color, gray, amount.clamp(0.0, 1.0))
}

fn phase(sun_y: f32, time: f32) -> &'static str {
    if sun_y > 0.2 {
        "day"
    } else if sun_y > -0.08 {
        if time < 0.5 { "dawn" } else { "dusk" }
    } else {
        "night"
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1.0e-8 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noon() -> Light {
        sample(Clock::noon())
    }

    #[test]
    fn afternoon_fog_grays_the_sky_and_noon_stays_clear() {
        assert_eq!(fog_mood(0.5), 0.0);
        assert_eq!(fog_mood(0.2), 0.0);
        assert_eq!(fog_mood(0.9), 0.0);
        assert!(fog_mood(0.70) > 0.95);
        let rising = fog_mood(0.64);
        assert!(rising > 0.2 && rising < 0.95, "fog eases in, got {rising}");
        let blue = [0.53, 0.75, 0.90];
        let gray = foggy(blue, [0.55, 0.58, 0.62], 1.0);
        assert!(gray[2] < blue[2]);
        assert!((gray[0] - gray[2]).abs() < 0.1);
        assert_eq!(foggy(blue, [0.0, 0.0, 0.0], 0.0), blue);
    }

    #[test]
    fn dawn_chorus_peaks_after_sunrise_and_is_quiet_at_noon() {
        assert_eq!(dawn_chorus(0.0), 0.0);
        assert_eq!(dawn_chorus(0.5), 0.0);
        assert_eq!(dawn_chorus(0.75), 0.0);
        assert!(dawn_chorus(0.26) > 0.8);
        assert!(dawn_chorus(0.22) > 0.0 && dawn_chorus(0.22) < dawn_chorus(0.26));
        assert!(dawn_chorus(0.38) > 0.0 && dawn_chorus(0.38) < dawn_chorus(0.26));
    }

    #[test]
    fn night_fog_closes_in_and_leaves_the_afternoon_gray() {
        assert!(night_fog(0.0) > 0.95);
        assert_eq!(night_fog(0.5), 0.0);
        assert_eq!(night_fog(0.25), 0.0);
        assert_eq!(night_fog(0.70), 0.0);
        assert!(fog_mood(0.70) > 0.95);
        let easing = night_fog(0.86);
        assert!(
            easing > 0.15 && easing < 0.95,
            "night fog eases in after the gray spell, got {easing}"
        );
        for step in 0..100 {
            let time = step as f32 / 100.0;
            assert!(
                fog_mood(time) < 0.5 || night_fog(time) < 0.5,
                "the two spells overlap at {time}"
            );
        }
        let (day_start, day_end) = fog_span(1.0, 0.5);
        let (night_start, night_end) = fog_span(0.22, 0.0);
        let (gray_start, gray_end) = fog_span(0.85, 0.70);
        assert!(night_end < day_end && night_start < day_start);
        assert!(gray_end < day_end);
        assert!(
            night_end < gray_end,
            "night closes closer than the gray spell"
        );
        let clear = [0.04, 0.05, 0.09];
        let closed = foggy(clear, [0.012, 0.016, 0.04], 0.75);
        let sum = |color: [f32; 3]| color[0] + color[1] + color[2];
        assert!(sum(closed) < sum(clear));
        let gray = foggy(clear, [0.55, 0.58, 0.62], 1.0);
        assert!(gray[0] > closed[0]);
    }

    #[test]
    fn noon_matches_the_old_sun_and_night_is_below_the_horizon() {
        let sun = sun_direction(0.5, Season::Spring);
        let old = normalize(NOON_SUN);
        for axis in 0..3 {
            assert!((sun[axis] - old[axis]).abs() < 1.0e-5);
        }
        assert!(
            sun_direction(0.0, Season::Spring)[1] < 0.0,
            "midnight sun is down"
        );
        assert!(
            sun_direction(0.25, Season::Spring)[1].abs() < 1.0e-4,
            "dawn sits on the horizon"
        );
        assert!(
            sun_direction(0.75, Season::Spring)[1].abs() < 1.0e-4,
            "dusk sits on the horizon"
        );
        assert_eq!(shadow_lookups(sun)[0], [0, -1]);
        let moon = moon_direction(0.0, Season::Spring);
        for axis in 0..3 {
            assert!((moon[axis] - sun[axis]).abs() < 1.0e-5);
        }
        assert!(
            moon_direction(0.5, Season::Spring)[1] < 0.0,
            "the moon is down at noon"
        );
    }

    #[test]
    fn the_day_is_brighter_than_night_and_dusk_goes_red() {
        let day = noon();
        let night = sample(Clock::at(0.0));
        let dusk = sample(Clock::at(DAY_SECONDS * 0.75));
        assert_eq!(day.phase, "day");
        assert_eq!(night.phase, "night");
        assert_eq!(dusk.phase, "dusk");
        assert!(day.brightness > dusk.brightness);
        assert!(dusk.brightness > night.brightness);
        assert!(night.brightness <= MOONLIGHT + 1.0e-4);
        assert!(day.shadow > 0.9);
        assert!(night.shadow > 0.4 && night.shadow < day.shadow);
        assert!(night.sky[2] < day.sky[2]);
        assert!(night.sky[0] < 0.08 && night.sky[1] < 0.08 && night.sky[2] < 0.12);
        assert!(dusk.sky[0] > dusk.sky[2]);
        assert!(day.sky[2] > day.sky[0]);
        assert!(night.direction[1] > 0.0, "the moon is up");
        let day_sky = day.sky[0] + day.sky[1] + day.sky[2];
        let day_horizon = day.horizon[0] + day.horizon[1] + day.horizon[2];
        assert!(
            day_horizon > day_sky,
            "noon horizon is paler than the zenith"
        );
        assert!(night.horizon[2] < 0.2);
        assert!(dusk.horizon[0] > dusk.horizon[2]);
    }

    #[test]
    fn noon_light_faces_the_sun_and_a_lantern_survives_the_night() {
        let day = noon();
        let up = ambient_factor([0.0, 1.0, 0.0], &day, false);
        let north = ambient_factor([0.0, 0.0, -1.0], &day, false);
        let south = ambient_factor([0.0, 0.0, 1.0], &day, false);
        let east = ambient_factor([1.0, 0.0, 0.0], &day, false);
        let west = ambient_factor([-1.0, 0.0, 0.0], &day, false);
        assert!(up > north);
        assert!(north > south);
        assert!(east > west);
        let shaded = ambient_factor([0.0, 1.0, 0.0], &day, true);
        assert!(shaded < up);
        assert!((shaded - up * CAST_SHADOW).abs() < 1.0e-4);

        let night = sample(Clock::at(0.0));
        let night_open = ambient_factor([0.0, 1.0, 0.0], &night, false);
        let night_shade = ambient_factor([0.0, 1.0, 0.0], &night, true);
        assert!(night_shade < night_open, "moonlight casts a shadow");
        assert!(night_shade > night_open * CAST_SHADOW);
        let albedo = [0.45, 0.70, 0.30];
        let unlit = albedo[0] * 0.2 * night_shade;
        let lantern = unlit + albedo[0] * 12.0 / 15.0;
        assert!(lantern > unlit * 4.0);
    }

    #[test]
    fn time_advances_through_seasons_and_the_shadow_follows_the_sun() {
        let mut clock = Clock::noon();
        assert_eq!(clock.season(), Season::Spring);
        assert_eq!(clock.season().label(), "Spring");
        clock.tick(DAY_SECONDS);
        assert_eq!(clock.season(), Season::Summer);
        clock.tick(DAY_SECONDS);
        assert_eq!(clock.season(), Season::Autumn);
        clock.tick(DAY_SECONDS);
        assert_eq!(clock.season(), Season::Winter);
        clock.tick(DAY_SECONDS);
        assert_eq!(clock.season(), Season::Spring);
        clock.tick(0.0);
        clock.tick(-1.0);
        let noon = clock_mark(0.5);
        assert!(noon[1] > 8.0 && noon[0].abs() < 1.0, "noon is at the top");
        let midnight = clock_mark(0.0);
        assert!(
            midnight[1] < -8.0 && midnight[0].abs() < 1.0,
            "midnight is at the bottom"
        );
        let dusk = clock_mark(0.75);
        assert!(dusk[0] > 8.0 && dusk[1].abs() < 1.0, "dusk is on the right");
        let dawn = clock_mark(0.25);
        assert!(dawn[0] < -8.0 && dawn[1].abs() < 1.0, "dawn is on the left");

        let early = shadow_lookups(sun_direction(0.30, Season::Spring));
        let noon_steps = shadow_lookups(sun_direction(0.5, Season::Spring));
        let late = shadow_lookups(sun_direction(0.70, Season::Spring));
        assert_eq!(noon_steps[0], [0, -1]);
        assert!(early[0][0] > 0, "morning sun is in the east: {early:?}");
        assert!(late[0][0] < 0, "afternoon sun is in the west: {late:?}");
        assert_ne!(early, late);
        let low = shadow_lookups(sun_direction(0.27, Season::Spring));
        assert!(
            low.len() > noon_steps.len(),
            "a low sun reaches farther: {low:?} vs {noon_steps:?}"
        );
        let midnight = shadow_lookups(sample(Clock::at(0.0)).direction);
        let later_night = shadow_lookups(sample(Clock::at(DAY_SECONDS * 0.15)).direction);
        assert_eq!(midnight[0], [0, -1]);
        assert_ne!(midnight, later_night);
    }

    #[test]
    fn seasons_raise_the_summer_sun_and_shorten_the_winter_day() {
        let spring = sun_direction(0.5, Season::Spring);
        let summer = sun_direction(0.5, Season::Summer);
        let autumn = sun_direction(0.5, Season::Autumn);
        let winter = sun_direction(0.5, Season::Winter);
        assert!(summer[1] > spring[1] + 0.1, "summer noon is higher");
        assert!(spring[1] > winter[1] + 0.2, "winter noon is lower");
        for axis in 0..3 {
            assert!((autumn[axis] - spring[axis]).abs() < 1.0e-5);
        }
        for sun in [spring, summer, autumn, winter] {
            assert!(sun[2] < 0.0, "noon stays in the north: {sun:?}");
        }

        assert!(sun_direction(0.20, Season::Summer)[1] > 0.0);
        assert!(sun_direction(0.80, Season::Summer)[1] > 0.0);
        assert!(sun_direction(0.20, Season::Spring)[1] < 0.0);
        assert!(sun_direction(0.20, Season::Winter)[1] < 0.0);
        assert!(sun_direction(0.32, Season::Winter)[1] > 0.0);
        assert!(sun_direction(0.70, Season::Winter)[1] < 0.0);

        let summer_rise = azimuth(horizon_sun(Season::Summer, true));
        let spring_rise = azimuth(horizon_sun(Season::Spring, true));
        let winter_rise = azimuth(horizon_sun(Season::Winter, true));
        let autumn_rise = azimuth(horizon_sun(Season::Autumn, true));
        assert!(
            summer_rise > spring_rise + 0.05,
            "summer sunrise swings south: {summer_rise} vs {spring_rise}"
        );
        assert!(
            spring_rise > winter_rise + 0.05,
            "winter sunrise swings north: {winter_rise} vs {spring_rise}"
        );
        assert!((autumn_rise - spring_rise).abs() < 1.0e-4);

        let summer_shadow = shadow_lookups(summer);
        let winter_shadow = shadow_lookups(winter);
        assert!(winter_shadow.len() > shadow_lookups(spring).len());
        assert!(shadow_lookups(spring).len() >= summer_shadow.len());
        assert_eq!(winter_shadow[0][1], -1, "winter noon still shadows south");
        let summer_morning = shadow_lookups(sun_direction(0.35, Season::Summer));
        let winter_morning = shadow_lookups(sun_direction(0.35, Season::Winter));
        assert_ne!(summer_morning, winter_morning);
        assert!(winter_morning.len() > summer_morning.len());

        let sun = sun_direction(0.35, Season::Summer);
        let moon = moon_direction(0.35, Season::Summer);
        let opposite = sun_direction(0.85, Season::Summer);
        for axis in 0..3 {
            assert!((moon[axis] - opposite[axis]).abs() < 1.0e-5);
        }
        assert!(sun[1] > 0.0 && moon[1] < 0.0);
    }

    #[test]
    fn the_northern_arc_puts_noon_in_the_south() {
        let south = sun_direction(0.5, Season::Spring);
        let north = sun_on(0.5, Season::Spring, Hemisphere::North);
        assert!(south[2] < 0.0);
        assert!(
            north[2] > 0.0,
            "northern noon stands in the south: {north:?}"
        );
        assert!((north[1] - south[1]).abs() < 1.0e-5);
        let summer = sun_on(0.5, Season::Summer, Hemisphere::North);
        let winter = sun_on(0.5, Season::Winter, Hemisphere::North);
        assert!(summer[1] > north[1] + 0.1);
        assert!(north[1] > winter[1] + 0.2);
        assert!(sun_on(0.20, Season::Summer, Hemisphere::North)[1] > 0.0);
        assert!(sun_on(0.20, Season::Winter, Hemisphere::North)[1] < 0.0);
        let summer_rise = azimuth(horizon_on(Season::Summer, true));
        let spring_rise = azimuth(horizon_on(Season::Spring, true));
        assert!(
            summer_rise < spring_rise - 0.05,
            "northern summer sunrise swings north: {summer_rise} vs {spring_rise}"
        );
        let shadow = shadow_lookups(north);
        assert_eq!(shadow[0][1], 1, "northern noon shadows toward the south");
        assert!(sample(Clock::noon()).direction[2] < 0.0);
    }

    #[test]
    fn sleep_skips_the_night_and_leaves_a_day_alone() {
        let mut evening = Clock::at(DAY_SECONDS * 0.9);
        assert_eq!(evening.season(), Season::Spring);
        assert!(evening.sleep());
        assert!((evening.time_of_day() - 0.25).abs() < 1.0e-3);
        assert_eq!(evening.season(), Season::Summer);

        let mut after_midnight = Clock::at(DAY_SECONDS * 0.1);
        assert!(after_midnight.sleep());
        assert!((after_midnight.time_of_day() - 0.25).abs() < 1.0e-3);
        assert_eq!(after_midnight.season(), Season::Spring);

        let mut noon = Clock::noon();
        assert!(!noon.sleep());
        assert!((noon.time_of_day() - 0.5).abs() < 1.0e-3);
        assert_eq!(noon.season(), Season::Spring);
    }

    #[test]
    fn the_sky_notices_a_sunrise_and_a_full_day() {
        let mut night = Clock::at(DAY_SECONDS * 0.2);
        assert!(!night.tick(1.0).dawn);
        let morning = night.tick(6.0);
        assert!(morning.dawn);
        assert!(!morning.dusk);
        assert!((night.time_of_day() - 0.25).abs() < 0.02);
        assert_eq!(night.year_day(), 0);

        let mut noon = Clock::noon();
        assert!(!noon.tick(1.0).dusk);
        let evening = noon.tick(40.0);
        assert!(evening.dusk);
        assert!(!evening.dawn);
        assert_eq!(noon.year_day(), 0);

        let mut again = Clock::noon();
        let whole = again.tick(DAY_SECONDS);
        assert!(whole.dawn && whole.dusk);
        assert_eq!(again.year_day(), 1);
    }

    #[test]
    fn the_hemisphere_cycles_and_names_the_north() {
        assert_eq!(Hemisphere::South.cycle(), Hemisphere::North);
        assert_eq!(Hemisphere::North.label(), "north");
    }

    fn horizon_on(season: Season, morning: bool) -> [f32; 3] {
        let (mut lo, mut hi) = if morning { (0.0, 0.5) } else { (0.5, 1.0) };
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            let up = sun_on(mid, season, Hemisphere::North)[1] > 0.0;
            if morning == up {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        sun_on((lo + hi) * 0.5, season, Hemisphere::North)
    }

    fn azimuth(dir: [f32; 3]) -> f32 {
        dir[0].atan2(-dir[2])
    }

    fn horizon_sun(season: Season, morning: bool) -> [f32; 3] {
        let (mut lo, mut hi) = if morning { (0.0, 0.5) } else { (0.5, 1.0) };
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            let up = sun_direction(mid, season)[1] > 0.0;
            if morning == up {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        sun_direction((lo + hi) * 0.5, season)
    }
}
