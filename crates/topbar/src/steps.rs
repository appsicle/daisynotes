//! Pure helpers: size-stepper math and the weight stops. No gpui in
//! here — everything is unit-testable.

use muse_core::SIZE_STEPS;

/// Comparison slop for size steps (sizes are whole points in practice).
const EPS: f32 = 0.01;

/// The five weight stops shown in the `Aa` popover's segmented control.
/// A continuous 300–700 slider is post-v1; these are the curated detents
/// until then.
pub(crate) const WEIGHT_STOPS: [u16; 5] = [300, 400, 500, 600, 700];

/// The size step strictly above `size`, or `size` unchanged at (or beyond)
/// the top of the scale.
pub(crate) fn next_size(size: f32) -> f32 {
    SIZE_STEPS
        .iter()
        .copied()
        .find(|&step| step > size + EPS)
        .unwrap_or(size)
}

/// The size step strictly below `size`, or `size` unchanged at (or beyond)
/// the bottom of the scale.
pub(crate) fn prev_size(size: f32) -> f32 {
    SIZE_STEPS
        .iter()
        .rev()
        .copied()
        .find(|&step| step < size - EPS)
        .unwrap_or(size)
}

/// Whether two sizes are the same step, within stepper slop.
pub(crate) fn same_step(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

/// The weight stop nearest to `weight`; ties resolve to the lighter stop.
pub(crate) fn nearest_weight_stop(weight: u16) -> u16 {
    let mut best = WEIGHT_STOPS[0];
    let mut best_distance = u16::MAX;
    for stop in WEIGHT_STOPS {
        let distance = stop.abs_diff(weight);
        if distance < best_distance {
            best_distance = distance;
            best = stop;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn next_size_walks_the_scale_upward() {
        let mut size = SIZE_STEPS[0];
        for &expected in &SIZE_STEPS[1..] {
            size = next_size(size);
            assert!(close(size, expected), "expected {expected}, got {size}");
        }
    }

    #[test]
    fn prev_size_walks_the_scale_downward() {
        let mut size = SIZE_STEPS[SIZE_STEPS.len() - 1];
        for &expected in SIZE_STEPS[..SIZE_STEPS.len() - 1].iter().rev() {
            size = prev_size(size);
            assert!(close(size, expected), "expected {expected}, got {size}");
        }
    }

    #[test]
    fn stepping_stops_at_the_ends() {
        assert!(close(next_size(28.0), 28.0));
        assert!(close(prev_size(13.0), 13.0));
    }

    #[test]
    fn off_scale_sizes_snap_to_adjacent_steps() {
        assert!(close(next_size(17.0), 18.0));
        assert!(close(prev_size(17.0), 16.0));
        assert!(close(next_size(15.5), 16.0));
        assert!(close(prev_size(15.5), 15.0));
    }

    #[test]
    fn far_out_of_range_sizes_do_not_move() {
        assert!(close(next_size(30.0), 30.0));
        assert!(close(prev_size(10.0), 10.0));
    }

    #[test]
    fn same_step_tolerates_float_noise() {
        assert!(same_step(16.0, 16.000_001));
        assert!(!same_step(16.0, 15.0));
    }

    #[test]
    fn nearest_weight_stop_exact_and_clamped() {
        for stop in WEIGHT_STOPS {
            assert_eq!(nearest_weight_stop(stop), stop);
        }
        assert_eq!(nearest_weight_stop(100), 300);
        assert_eq!(nearest_weight_stop(800), 700);
    }

    #[test]
    fn nearest_weight_stop_ties_go_lighter() {
        assert_eq!(nearest_weight_stop(350), 300);
        assert_eq!(nearest_weight_stop(449), 400);
        assert_eq!(nearest_weight_stop(451), 500);
        assert_eq!(nearest_weight_stop(650), 600);
    }
}
