//! Caret motion and blink: a critically damped 2-D spring plus a smooth
//! opacity cycle. Pure math over `f32` so it tests without a window.

use std::time::Duration;

/// How long the caret stays fully opaque after an edit or caret move before
/// the blink cycle resumes.
pub(crate) const BLINK_HOLD: Duration = Duration::from_millis(300);

/// One full fade-out/fade-in blink period.
pub(crate) const BLINK_PERIOD: Duration = Duration::from_millis(1100);

/// Stiffness of the caret spring, chosen so a critically damped system
/// settles within ~the `daisynotes_theme::motion::CARET` duration (60ms): a
/// critically damped spring reaches 1% of its travel at ω·t ≈ 6.6.
const OMEGA: f32 = 110.0;

/// Positions closer than this (logical px) snap to the target.
const SETTLE_DIST: f32 = 0.05;
const SETTLE_VEL: f32 = 2.0;

/// A critically damped spring tracking a 2-D target. The caret's pixel
/// position eases through this; the glyphs it trails never wait for it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretSpring {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    tx: f32,
    ty: f32,
    settled: bool,
}

impl CaretSpring {
    /// A spring resting at the given point.
    pub fn resting(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            tx: x,
            ty: y,
            settled: true,
        }
    }

    /// Jump to a point with no animation (entry switches, first layout).
    pub fn snap_to(&mut self, x: f32, y: f32) {
        *self = Self::resting(x, y);
    }

    /// Aim at a new target, keeping current position and velocity so the
    /// motion stays continuous when the target moves mid-flight.
    pub fn retarget(&mut self, x: f32, y: f32) {
        if (x - self.tx).abs() > f32::EPSILON || (y - self.ty).abs() > f32::EPSILON {
            self.tx = x;
            self.ty = y;
            self.settled = false;
        }
    }

    /// Advance by `dt` seconds using the closed-form critically damped
    /// solution: x(t) = target + (q + (v + ωq)·t)·e^(−ωt).
    pub fn step(&mut self, dt: f32) {
        if self.settled {
            return;
        }
        let dt = dt.clamp(0.0, 0.05);
        let decay = (-OMEGA * dt).exp();

        let qx = self.x - self.tx;
        let cx = self.vx + OMEGA * qx;
        self.x = self.tx + (qx + cx * dt) * decay;
        self.vx = (self.vx - cx * OMEGA * dt) * decay;

        let qy = self.y - self.ty;
        let cy = self.vy + OMEGA * qy;
        self.y = self.ty + (qy + cy * dt) * decay;
        self.vy = (self.vy - cy * OMEGA * dt) * decay;

        if (self.x - self.tx).abs() < SETTLE_DIST
            && (self.y - self.ty).abs() < SETTLE_DIST
            && self.vx.abs() < SETTLE_VEL
            && self.vy.abs() < SETTLE_VEL
        {
            self.snap_to(self.tx, self.ty);
        }
    }

    /// True once the spring has reached its target.
    pub fn settled(&self) -> bool {
        self.settled
    }
}

/// Caret opacity `elapsed` after the last edit/caret move: fully opaque
/// during the hold window, then a smooth cosine fade cycle — never a hard
/// toggle.
pub(crate) fn blink_opacity(elapsed: Duration) -> f32 {
    if elapsed <= BLINK_HOLD {
        return 1.0;
    }
    let cycle = (elapsed - BLINK_HOLD).as_secs_f32() % BLINK_PERIOD.as_secs_f32();
    let phase = cycle / BLINK_PERIOD.as_secs_f32();
    // Starts at 1, dips to 0 mid-period, returns to 1: cos eased.
    0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_settles_near_caret_duration() {
        let mut spring = CaretSpring::resting(0.0, 0.0);
        spring.retarget(100.0, 40.0);
        let mut t = 0.0;
        while !spring.settled() && t < 1.0 {
            spring.step(1.0 / 120.0);
            t += 1.0 / 120.0;
        }
        assert!(spring.settled(), "spring never settled");
        assert!(t < 0.150, "settled too slowly: {t}s");
        assert!((spring.x - 100.0).abs() < 0.1);
        assert!((spring.y - 40.0).abs() < 0.1);
    }

    #[test]
    fn spring_never_overshoots() {
        let mut spring = CaretSpring::resting(0.0, 0.0);
        spring.retarget(50.0, 0.0);
        for _ in 0..240 {
            spring.step(1.0 / 120.0);
            assert!(spring.x <= 50.0 + 0.11, "overshoot: {}", spring.x);
        }
    }

    #[test]
    fn blink_holds_then_cycles() {
        assert!((blink_opacity(Duration::ZERO) - 1.0).abs() < f32::EPSILON);
        assert!((blink_opacity(Duration::from_millis(299)) - 1.0).abs() < f32::EPSILON);
        // Mid-cycle is fully faded.
        let mid = blink_opacity(BLINK_HOLD + BLINK_PERIOD / 2);
        assert!(mid < 0.01, "mid-cycle should be faded out, got {mid}");
        // A full period later it is opaque again.
        let full = blink_opacity(BLINK_HOLD + BLINK_PERIOD);
        assert!(full > 0.99, "full period should be opaque, got {full}");
    }
}
