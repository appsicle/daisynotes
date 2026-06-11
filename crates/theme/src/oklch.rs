//! Hand-rolled sRGB ↔ linear ↔ OKLab ↔ OKLCH conversion and interpolation.
//!
//! Theme crossfades interpolate every token in OKLCH so midpoints stay
//! perceptually clean (no muddy grays halfway between warm palettes).
//!
//! Reference: Björn Ottosson, "A perceptual color space for image
//! processing" (<https://bottosson.github.io/posts/oklab/>). The two 3×3
//! matrices below (sRGB-linear → LMS and L'M'S' → OKLab) and their inverses
//! are copied verbatim from that post.

use gpui::{Hsla, Rgba};

/// A color in OKLCH: perceptual lightness, chroma, hue (radians), alpha.
#[derive(Clone, Copy, Debug)]
struct Oklch {
    l: f32,
    c: f32,
    /// Hue angle in radians. Meaningless when `c` is (near) zero.
    h: f32,
    alpha: f32,
}

/// Below this chroma a color is treated as achromatic and its hue is
/// borrowed from the other interpolation endpoint.
const ACHROMATIC_CHROMA: f32 = 1e-5;

/// Interpolate two colors in OKLCH. `t = 0.0` yields `a`, `t = 1.0` yields
/// `b`. Hue takes the shortest path around the circle; alpha interpolates
/// linearly. This is the primitive behind [`crate::lerp_tokens`].
#[must_use]
pub fn lerp_hsla(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    let ca = oklch_from_hsla(a);
    let cb = oklch_from_hsla(b);

    // An achromatic endpoint has no meaningful hue: spinning through one
    // would tint the midpoints, so borrow the chromatic side's hue.
    let (ha, hb) = if ca.c < ACHROMATIC_CHROMA && cb.c >= ACHROMATIC_CHROMA {
        (cb.h, cb.h)
    } else if cb.c < ACHROMATIC_CHROMA && ca.c >= ACHROMATIC_CHROMA {
        (ca.h, ca.h)
    } else {
        (ca.h, shortest_hue_target(ca.h, cb.h))
    };

    let mixed = Oklch {
        l: lerp(ca.l, cb.l, t),
        c: lerp(ca.c, cb.c, t),
        h: lerp(ha, hb, t),
        alpha: lerp(ca.alpha, cb.alpha, t),
    };
    hsla_from_oklch(mixed)
}

/// The OKLCH perceptual lightness of a color, in `0.0..=1.0`.
pub(crate) fn lightness(color: Hsla) -> f32 {
    oklch_from_hsla(color).l
}

/// The color with its OKLCH lightness replaced (chroma and hue kept).
pub(crate) fn with_lightness(color: Hsla, l: f32) -> Hsla {
    let mut c = oklch_from_hsla(color);
    c.l = l.clamp(0.0, 1.0);
    hsla_from_oklch(c)
}

/// The color with its OKLCH hue rotated by `degrees`.
pub(crate) fn with_hue_rotated(color: Hsla, degrees: f32) -> Hsla {
    let mut c = oklch_from_hsla(color);
    c.h += degrees.to_radians();
    hsla_from_oklch(c)
}

/// The color with its OKLCH chroma scaled by `factor`.
pub(crate) fn with_chroma_scaled(color: Hsla, factor: f32) -> Hsla {
    let mut c = oklch_from_hsla(color);
    c.c = (c.c * factor).max(0.0);
    hsla_from_oklch(c)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Re-express `to` so that linear interpolation from `from` travels the
/// shorter way around the hue circle.
fn shortest_hue_target(from: f32, to: f32) -> f32 {
    use std::f32::consts::TAU;
    let mut delta = (to - from) % TAU;
    if delta > TAU / 2.0 {
        delta -= TAU;
    } else if delta < -TAU / 2.0 {
        delta += TAU;
    }
    from + delta
}

fn oklch_from_hsla(color: Hsla) -> Oklch {
    let rgba = Rgba::from(color);
    let (l, a, b) = oklab_from_linear(
        srgb_to_linear(rgba.r),
        srgb_to_linear(rgba.g),
        srgb_to_linear(rgba.b),
    );
    Oklch {
        l,
        c: (a * a + b * b).sqrt(),
        h: b.atan2(a),
        alpha: rgba.a,
    }
}

fn hsla_from_oklch(color: Oklch) -> Hsla {
    let a = color.c * color.h.cos();
    let b = color.c * color.h.sin();
    let (lr, lg, lb) = linear_from_oklab(color.l, a, b);
    Rgba {
        r: linear_to_srgb(lr).clamp(0.0, 1.0),
        g: linear_to_srgb(lg).clamp(0.0, 1.0),
        b: linear_to_srgb(lb).clamp(0.0, 1.0),
        a: color.alpha.clamp(0.0, 1.0),
    }
    .into()
}

/// sRGB electro-optical transfer function (IEC 61966-2-1).
fn srgb_to_linear(c: f32) -> f32 {
    if c > 0.040_45 {
        ((c + 0.055) / 1.055).powf(2.4)
    } else {
        c / 12.92
    }
}

/// Inverse of [`srgb_to_linear`].
fn linear_to_srgb(c: f32) -> f32 {
    if c > 0.003_130_8 {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    } else {
        12.92 * c
    }
}

/// Linear sRGB → OKLab (Ottosson's M1 then cube root then M2).
fn oklab_from_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();

    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// OKLab → linear sRGB (inverse M2, cube, inverse M1).
fn linear_from_oklab(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

    let l_ = l_ * l_ * l_;
    let m_ = m_ * m_ * m_;
    let s_ = s_ * s_ * s_;

    (
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Rgba, hsla, rgb};

    fn assert_rgba_close(a: Rgba, b: Rgba, eps: f32) {
        assert!(
            (a.r - b.r).abs() < eps
                && (a.g - b.g).abs() < eps
                && (a.b - b.b).abs() < eps
                && (a.a - b.a).abs() < eps,
            "expected {a:?} ≈ {b:?}"
        );
    }

    #[test]
    fn white_has_unit_lightness() {
        let white = oklch_from_hsla(
            Rgba {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }
            .into(),
        );
        assert!((white.l - 1.0).abs() < 1e-3, "L = {}", white.l);
        assert!(white.c < 1e-3);
    }

    #[test]
    fn srgb_red_matches_reference() {
        // bottosson.github.io reference: sRGB red → OKLab (0.6280, 0.2249, 0.1258).
        let red = oklch_from_hsla(
            Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }
            .into(),
        );
        assert!((red.l - 0.628).abs() < 2e-3, "L = {}", red.l);
        let (expected_c, expected_h) = {
            let (a, b) = (0.2249_f32, 0.1258_f32);
            ((a * a + b * b).sqrt(), b.atan2(a))
        };
        assert!((red.c - expected_c).abs() < 2e-3, "C = {}", red.c);
        assert!((red.h - expected_h).abs() < 2e-2, "h = {}", red.h);
    }

    #[test]
    fn round_trips_palette_colors() {
        for hex in [
            0xFAF8F5, 0x171512, 0xFFFFFF, 0x1F1C18, 0x26221C, 0xEDE9E2, 0xB86450, 0xE0907C,
            0x6E6AA8, 0xA8A4DE, 0x5F7A5A, 0x8FAE89, 0xECE8E1, 0x2A2722,
        ] {
            let original: Hsla = rgb(hex).into();
            let through = hsla_from_oklch(oklch_from_hsla(original));
            assert_rgba_close(Rgba::from(original), Rgba::from(through), 2e-3);
        }
    }

    #[test]
    fn lerp_endpoints_are_exact_enough() {
        let a: Hsla = rgb(0xB86450).into();
        let b: Hsla = rgb(0xE0907C).into();
        assert_rgba_close(Rgba::from(lerp_hsla(a, b, 0.0)), Rgba::from(a), 2e-3);
        assert_rgba_close(Rgba::from(lerp_hsla(a, b, 1.0)), Rgba::from(b), 2e-3);
    }

    #[test]
    fn black_white_midpoint_is_perceptual_gray() {
        let mid = lerp_hsla(gpui::black(), hsla(0.0, 0.0, 1.0, 1.0), 0.5);
        let mid = oklch_from_hsla(mid);
        assert!((mid.l - 0.5).abs() < 1e-2, "L = {}", mid.l);
        assert!(mid.c < 1e-3);
    }

    #[test]
    fn alpha_lerps_linearly() {
        let a: Hsla = hsla(0.1, 0.5, 0.5, 0.0);
        let b: Hsla = hsla(0.1, 0.5, 0.5, 1.0);
        assert!((lerp_hsla(a, b, 0.25).a - 0.25).abs() < 1e-4);
    }

    #[test]
    fn hue_takes_shortest_path() {
        use std::f32::consts::TAU;
        // 350° → 10° should pass through 0°, not 180°.
        let from = 350.0_f32.to_radians();
        let to = 10.0_f32.to_radians();
        let target = shortest_hue_target(from, to);
        let mid = lerp(from, target, 0.5).rem_euclid(TAU);
        assert!(
            mid < 1.0_f32.to_radians() || mid > 359.0_f32.to_radians(),
            "midpoint hue was {}°",
            mid.to_degrees()
        );
    }
}
