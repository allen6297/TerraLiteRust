//! What a registered command does when the player runs it.
//!
//! The id is `namespace:path`. `day` and `night` move the clock. `home` puts
//! the camera back on the stairs. Anything else is still a command, and the
//! caller throws a spark either way.

use crate::camera::Camera;
use crate::daylight::Clock;

pub fn apply(id: &str, clock: &mut Clock, camera: &mut Camera) -> bool {
    match name(id) {
        "day" => {
            clock.set_time_of_day(0.5);
            true
        }
        "night" => {
            clock.set_time_of_day(0.0);
            true
        }
        "home" => {
            *camera = Camera::drop_by_the_stairs();
            true
        }
        _ => false,
    }
}

fn name(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daylight::Season;

    #[test]
    fn day_night_and_home_move_the_clock_and_the_eye() {
        let mut clock = Clock::noon();
        let mut camera = Camera::drop_by_the_stairs();
        camera.eye.x = 40.0;
        assert!(apply("guide:night", &mut clock, &mut camera));
        assert!(clock.time_of_day() < 0.01);
        assert_eq!(clock.season(), Season::Spring);
        assert!(apply("day", &mut clock, &mut camera));
        assert!((clock.time_of_day() - 0.5).abs() < 1.0e-3);
        assert!((camera.eye.x - 40.0).abs() < 1.0e-3);
        assert!(apply("guide:home", &mut clock, &mut camera));
        assert!((camera.eye.x - 8.5).abs() < 1.0e-3);
        assert!(!apply("guide:marker", &mut clock, &mut camera));
    }
}
