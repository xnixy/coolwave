//! Fan curve rules: the points a curve may have and how they move.
//!
//! A curve is (°C, %) points, hotter to the right. These rules keep any edit
//! safe: fans never stall, never slow down as it gets hotter, and are at full
//! speed by the last point.

use coolwave_core::eurux::MIN_DUTY;

pub type Point = (f32, u8);

pub const MIN_TEMP: f32 = 0.0;
pub const MAX_TEMP: f32 = 100.0;
pub const MAX_DUTY: u8 = 100;

/// What a new curve starts as.
pub const DEFAULT: [Point; 5] = [(30.0, 30), (45.0, 40), (60.0, 55), (75.0, 80), (85.0, 100)];

pub const PRESETS: [(&str, [Point; 5]); 3] = [
    ("Silent", [(30.0, 20), (45.0, 25), (60.0, 40), (75.0, 65), (90.0, 100)]),
    ("Balanced", DEFAULT),
    ("Performance", [(30.0, 45), (45.0, 60), (60.0, 75), (70.0, 90), (80.0, 100)]),
];

/// Make any list of points a valid curve. An empty list becomes the default.
pub fn normalize(points: &mut Vec<Point>) {
    if points.is_empty() {
        points.extend(DEFAULT);
    }
    for point in points.iter_mut() {
        point.0 = point.0.clamp(MIN_TEMP, MAX_TEMP);
        point.1 = point.1.clamp(MIN_DUTY, MAX_DUTY);
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut floor = MIN_DUTY;
    for point in points.iter_mut() {
        point.1 = point.1.max(floor);
        floor = point.1;
    }
    if let Some(last) = points.last_mut() {
        last.1 = MAX_DUTY;
    }
}

/// Move point `index` to the wanted temperature and duty, stopping at its
/// neighbours so the order and the rising speed are kept. The last point
/// can only move sideways.
pub fn move_point(points: &mut [Point], index: usize, temp: f32, duty: f32) {
    let Some(&(_, _)) = points.get(index) else { return };
    let last = index + 1 == points.len();
    let temp_min = if index == 0 { MIN_TEMP } else { points[index - 1].0 };
    let temp_max = points.get(index + 1).map_or(MAX_TEMP, |next| next.0);
    let duty_min = if index == 0 { MIN_DUTY } else { points[index - 1].1 };
    let duty_max = points.get(index + 1).map_or(MAX_DUTY, |next| next.1);
    points[index].0 = temp.round().clamp(temp_min, temp_max);
    points[index].1 = if last { MAX_DUTY } else { (duty.round() as i32).clamp(duty_min as i32, duty_max as i32) as u8 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_sorts_and_clamps() {
        let mut points = vec![(80.0, 100), (-5.0, 3), (50.0, 60)];
        normalize(&mut points);
        assert_eq!(points, vec![(0.0, MIN_DUTY), (50.0, 60), (80.0, 100)]);
    }

    #[test]
    fn normalize_never_lets_speed_fall() {
        let mut points = vec![(30.0, 50), (50.0, 30), (70.0, 40), (90.0, 20)];
        normalize(&mut points);
        let duties: Vec<u8> = points.iter().map(|p| p.1).collect();
        assert_eq!(duties, vec![50, 50, 50, 100]);
    }

    #[test]
    fn normalize_ends_at_full_speed() {
        let mut points = vec![(30.0, 30), (80.0, 70)];
        normalize(&mut points);
        assert_eq!(points.last(), Some(&(80.0, 100)));
    }

    #[test]
    fn normalize_replaces_an_empty_curve() {
        let mut points = Vec::new();
        normalize(&mut points);
        assert_eq!(points, DEFAULT.to_vec());
    }

    #[test]
    fn presets_are_already_valid() {
        for (_, preset) in PRESETS {
            let mut points = preset.to_vec();
            normalize(&mut points);
            assert_eq!(points, preset.to_vec());
        }
    }

    #[test]
    fn a_point_stops_at_its_neighbours() {
        let mut points = DEFAULT.to_vec();
        move_point(&mut points, 2, 90.0, 10.0);
        assert_eq!(points[2], (75.0, 40));
        move_point(&mut points, 2, 0.0, 99.0);
        assert_eq!(points[2], (45.0, 80));
    }

    #[test]
    fn the_first_point_keeps_the_minimum_duty() {
        let mut points = DEFAULT.to_vec();
        move_point(&mut points, 0, -10.0, 0.0);
        assert_eq!(points[0], (0.0, MIN_DUTY));
    }

    #[test]
    fn the_last_point_only_moves_sideways() {
        let mut points = DEFAULT.to_vec();
        move_point(&mut points, 4, 95.0, 30.0);
        assert_eq!(points[4], (95.0, MAX_DUTY));
    }
}
