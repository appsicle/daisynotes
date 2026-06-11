//! Entry-level "voice" (family / size / weight) and range-level inline styles.

use serde::{Deserialize, Serialize};

/// The four content font families an entry's voice can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontFamily {
    /// Literata — serif, the default. Warm, bookish, made for long-form.
    #[default]
    Literata,
    /// Inter — clean humanist sans.
    Inter,
    /// iA Writer Quattro — soft semi-mono; the journal voice.
    Quattro,
    /// JetBrains Mono — true mono for notes and technical writing.
    Mono,
}

/// The entry-level voice: font family, base size, and base weight.
/// Applies to the whole entry, never to a sub-range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    /// Content font family for the entire entry.
    pub family: FontFamily,
    /// Base point size; one of [`SIZE_STEPS`] in practice.
    pub size: f32,
    /// Base weight, 300–700 (variable axis).
    pub weight: u16,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            family: FontFamily::Literata,
            size: 16.0,
            weight: 400,
        }
    }
}

/// The sanctioned base-size steps for the `Aa` popover stepper, in points.
pub const SIZE_STEPS: [f32; 8] = [13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 28.0];

/// The curated selected-text ink colors (besides the default ink).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ink {
    /// Rose clay — the accent hue.
    Rose,
    /// Lavender — the muse hue.
    Lavender,
    /// Moss green.
    Moss,
}

/// Range-level styling: the only inline attributes Muse supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InlineStyle {
    /// Bold.
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Italic.
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Underline.
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Strikethrough.
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Optional ink color; `None` means the default ink.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ink: Option<Ink>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // signature dictated by serde
fn is_false(b: &bool) -> bool {
    !*b
}

impl InlineStyle {
    /// True when no attribute is set — the style of unadorned text.
    /// Plain runs are never stored in a [`crate::SpanSet`].
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// A single style mutation applied to a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleToggle {
    /// Toggle bold: removed if the entire range is bold, applied otherwise.
    Bold,
    /// Toggle italic (same whole-range semantics as bold).
    Italic,
    /// Toggle underline (same whole-range semantics as bold).
    Underline,
    /// Toggle strikethrough (same whole-range semantics as bold).
    Strike,
    /// `Some(ink)` paints the range with that ink; `None` clears ink.
    Ink(Option<Ink>),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let v = Voice::default();
        assert_eq!(v.family, FontFamily::Literata);
        assert!((v.size - 16.0).abs() < f32::EPSILON);
        assert_eq!(v.weight, 400);
        assert!(InlineStyle::default().is_plain());
    }

    #[test]
    fn size_steps_are_sorted() {
        assert!(SIZE_STEPS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn font_family_serializes_as_lowercase_string() {
        let families = [
            (FontFamily::Literata, "\"literata\""),
            (FontFamily::Inter, "\"inter\""),
            (FontFamily::Quattro, "\"quattro\""),
            (FontFamily::Mono, "\"mono\""),
        ];
        for (family, json) in families {
            assert_eq!(serde_json::to_string(&family).unwrap(), json);
        }
        assert_eq!(serde_json::to_string(&Ink::Rose).unwrap(), "\"rose\"");
    }

    #[test]
    fn non_plain_style() {
        let style = InlineStyle {
            ink: Some(Ink::Moss),
            ..InlineStyle::default()
        };
        assert!(!style.is_plain());
    }
}
