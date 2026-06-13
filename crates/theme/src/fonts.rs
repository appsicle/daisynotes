//! Font family names for the four bundled content voices and the UI chrome.
//!
//! All families — the four content voices and the Sk-Modernist UI face —
//! are bundled (see `daisynotes-ui::fonts`) and registered with GPUI's text
//! system before the first frame, so these names always resolve.

/// Literata — serif, the default entry voice. Warm, bookish long-form type.
pub const FONT_SERIF: &str = "Literata";

/// Inter — clean humanist sans.
pub const FONT_SANS: &str = "Inter";

/// iA Writer Quattro — soft semi-mono; the journal voice.
pub const FONT_QUATTRO: &str = "iA Writer Quattro V";

/// JetBrains Mono — true mono for notes and technical writing.
pub const FONT_MONO: &str = "JetBrains Mono";

/// Sk-Modernist — the UI chrome face, bundled (Regular weight only) and
/// registered alongside the content fonts in `daisynotes-ui::fonts`.
///
/// Single-weight by design: chrome hierarchy comes from size and ink
/// color tiers (see `layout`), never from font weight.
pub const FONT_UI: &str = "Sk-Modernist";
