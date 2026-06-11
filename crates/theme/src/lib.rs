//! muse-theme — design tokens: Paper/Dusk palettes, type scale, spacing,
//! motion constants, and OKLCH interpolation for animated theme crossfades.
//!
//! This crate owns the visual constants of Muse and nothing else. It knows
//! the two palettes from PLAN §8, the layout grid, the bundled font names,
//! and how to interpolate between palettes perceptually (OKLCH) so the
//! theme toggle is a pure color crossfade. It must not know about views,
//! entries, documents, or the agent.

pub mod fonts;
pub mod layout;
pub mod motion;

mod custom;
mod oklch;
mod tokens;

pub use custom::{ThemePair, ThemePreset, derive_tokens, hex_from_hsla, hsla_from_hex, presets};
pub use fonts::{FONT_MONO, FONT_QUATTRO, FONT_SANS, FONT_SERIF, FONT_UI};
pub use oklch::lerp_hsla;
pub use tokens::{ActiveTheme, Appearance, Theme, Tokens, dusk, lerp_tokens, paper};
