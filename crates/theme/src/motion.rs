//! The "calm springs" motion system (PLAN §8): durations and easing curves.
//!
//! Every easing here has the shape `Fn(f32) -> f32` over `0.0..=1.0`, which
//! is exactly what [`gpui::Animation::with_easing`] accepts. Motion explains,
//! never decorates: these curves all settle and none overshoot.

use std::time::Duration;

/// Fades: hover states, hairlines, toasts. 160ms ease-out.
pub const FADE: Duration = Duration::from_millis(160);

/// Movement: panels, popovers, the sidebar spring. 280ms, settles cleanly.
pub const MOVE: Duration = Duration::from_millis(280);

/// Theme toggle crossfade, all tokens interpolated in OKLCH. 240ms.
pub const THEME_FADE: Duration = Duration::from_millis(240);

/// Muse margin-note bloom (dot → card, scale 0.96 → 1 + fade). 320ms.
pub const NOTE_BLOOM: Duration = Duration::from_millis(320);

/// Caret travel: critically damped, ~60ms settle. Glyphs never wait for it.
pub const CARET: Duration = Duration::from_millis(60);

/// Quintic ease-out: starts fast, decelerates to a stop. The default fade
/// curve.
#[must_use]
pub fn ease_out_quint(delta: f32) -> f32 {
    1.0 - (1.0 - delta).powi(5)
}

/// Quadratic ease-in-out: slow at both ends, brisk in the middle. For
/// symmetric crossfades (theme toggle, icon morphs).
#[must_use]
pub fn ease_in_out(delta: f32) -> f32 {
    if delta < 0.5 {
        2.0 * delta * delta
    } else {
        let x = -2.0 * delta + 2.0;
        1.0 - x * x / 2.0
    }
}

/// Spring-feel settle: a cubic-bezier(0.32, 0.72, 0.0, 1.0) approximation of
/// a critically-damped spring (response ≈ the animation duration, damping
/// ≈ 0.9). Rises quickly, then glides to rest with no overshoot — the curve
/// for panel movement and the switch knob.
#[must_use]
pub fn spring(delta: f32) -> f32 {
    cubic_bezier(0.32, 0.72, 0.0, 1.0)(delta)
}

/// Build an easing function from a CSS-style cubic Bézier with control
/// points `(x1, y1)` and `(x2, y2)` (endpoints fixed at (0,0) and (1,1)).
///
/// `x1`/`x2` must lie in `0.0..=1.0` so the curve is a function of time.
/// Keeping `y1`/`y2` within `0.0..=1.0` guarantees output in `0.0..=1.0`,
/// which gpui's animation element asserts on.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |delta: f32| {
        let delta = delta.clamp(0.0, 1.0);
        // Solve bezier_x(t) = delta for t with Newton–Raphson; the curve's
        // x component is monotonic because x1 and x2 are clamped to [0, 1].
        let x1 = x1.clamp(0.0, 1.0);
        let x2 = x2.clamp(0.0, 1.0);
        let mut t = delta;
        for _ in 0..8 {
            let x = sample(x1, x2, t) - delta;
            let dx = derivative(x1, x2, t);
            if dx.abs() < 1e-6 {
                break;
            }
            t = (t - x / dx).clamp(0.0, 1.0);
        }
        sample(y1, y2, t).clamp(0.0, 1.0)
    }
}

/// Evaluate one component of the cubic Bézier (endpoints 0 and 1) at `t`.
fn sample(p1: f32, p2: f32, t: f32) -> f32 {
    let omt = 1.0 - t;
    3.0 * omt * omt * t * p1 + 3.0 * omt * t * t * p2 + t * t * t
}

/// Derivative of [`sample`] with respect to `t`.
fn derivative(p1: f32, p2: f32, t: f32) -> f32 {
    let omt = 1.0 - t;
    3.0 * omt * omt * p1 + 6.0 * omt * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_hit_endpoints() {
        for ease in [ease_out_quint as fn(f32) -> f32, ease_in_out, spring] {
            assert!((ease(0.0)).abs() < 1e-4);
            assert!((ease(1.0) - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn easings_stay_in_unit_range_and_monotonic() {
        for ease in [ease_out_quint as fn(f32) -> f32, ease_in_out, spring] {
            let mut prev = 0.0_f32;
            for i in 0..=100 {
                let v = ease(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&v), "easing escaped unit range: {v}");
                assert!(v >= prev - 1e-4, "easing not monotonic at step {i}");
                prev = v;
            }
        }
    }

    #[test]
    fn cubic_bezier_linear_is_identity() {
        let linear = cubic_bezier(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            assert!((linear(t) - t).abs() < 1e-3, "t={t} got {}", linear(t));
        }
    }
}
