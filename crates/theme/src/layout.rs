//! Fixed layout metrics (PLAN §2, §8). All values are logical pixels.
//!
//! Chrome dimensions are constants by design: "nothing ever shifts" is
//! enforced by never computing chrome sizes from content.

/// Topbar height.
pub const TOPBAR_H: f32 = 52.0;

/// Sidebar width when open.
pub const SIDEBAR_W: f32 = 260.0;

/// Maximum width of the centered writing column (~66ch at default size).
pub const COLUMN_MAX_W: f32 = 640.0;

/// Top padding above the first line of an entry.
pub const COLUMN_TOP_PAD: f32 = 96.0;

/// Corner radius for small controls (buttons, swatches).
pub const RADIUS_SM: f32 = 6.0;

/// Corner radius for cards and popovers.
pub const RADIUS_MD: f32 = 10.0;

/// Corner radius for floating pills and sidebar panels.
pub const RADIUS_LG: f32 = 14.0;

// UI type scale. Sk-Modernist ships in Regular only, so chrome hierarchy
// comes entirely from these sizes plus ink color tiers — never weight.

/// Pane titles ("Settings") — anything that heads a surface.
pub const UI_TITLE: f32 = 15.0;

/// Primary interactive text: sidebar entry titles, settings control
/// labels, buttons, toast text.
pub const UI_BODY: f32 = 14.0;

/// Secondary controls: text-field content, segmented controls, the "Aa"
/// glyph.
pub const UI_TEXT: f32 = 13.0;

/// Metadata and subtext: entry ages, sync glyph captions.
pub const UI_SMALL: f32 = 12.0;

/// Section headers ("ENTRIES", settings section labels): UPPERCASE at
/// tertiary ink for a small-caps-label feel (gpui's text API has no
/// letter-spacing knob).
pub const UI_HEADER: f32 = 11.0;
