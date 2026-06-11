//! Builds gpui [`TextRun`]s for one paragraph from the document's style
//! tiles, the entry voice, the ink palette, and the IME marked range.
//! Pure construction — testable without a window.

use std::ops::Range;

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Hsla, SharedString, TextRun, UnderlineStyle, px};
use muse_core::{FontFamily, Ink, InlineStyle, Voice};
use muse_theme::fonts;

/// Bold adds this much to the voice's base weight (capped). With a 400 base
/// that lands on 700; heavier voices cap at 800 so bold stays distinct
/// without clotting.
const BOLD_DELTA: f32 = 300.0;
const MAX_WEIGHT: f32 = 800.0;

/// The gpui font family name for an entry voice family.
pub(crate) fn family_name(family: FontFamily) -> &'static str {
    match family {
        FontFamily::Literata => fonts::FONT_SERIF,
        FontFamily::Inter => fonts::FONT_SANS,
        FontFamily::Quattro => fonts::FONT_QUATTRO,
        FontFamily::Mono => fonts::FONT_MONO,
    }
}

/// The ink palette color for an optional ink, given
/// `tokens.ink_palette()` = [ink, rose, lavender, moss].
pub(crate) fn ink_color(ink: Option<Ink>, palette: [Hsla; 4]) -> Hsla {
    match ink {
        None => palette[0],
        Some(Ink::Rose) => palette[1],
        Some(Ink::Lavender) => palette[2],
        Some(Ink::Moss) => palette[3],
    }
}

/// The font for one styled run of the entry voice.
pub(crate) fn run_font(voice: Voice, style: InlineStyle) -> Font {
    let base = f32::from(voice.weight);
    let weight = if style.bold {
        (base + BOLD_DELTA).min(MAX_WEIGHT)
    } else {
        base
    };
    Font {
        family: SharedString::new_static(family_name(voice.family)),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight(weight),
        style: if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

/// Build the [`TextRun`]s for one paragraph.
///
/// `tiles` are the style tiles covering the paragraph's visible text
/// (offsets relative to the paragraph start, gaps included, as produced by
/// `SpanSet::runs_in` after rebasing). `marked` is the IME composition range
/// in the same paragraph-relative offsets; its overlap renders underlined.
pub(crate) fn paragraph_runs(
    tiles: &[(Range<usize>, InlineStyle)],
    visible_len: usize,
    voice: Voice,
    marked: Option<Range<usize>>,
    palette: [Hsla; 4],
) -> Vec<TextRun> {
    let mut runs = Vec::with_capacity(tiles.len() + 2);
    for (range, style) in tiles {
        let range = range.start.min(visible_len)..range.end.min(visible_len);
        if range.start >= range.end {
            continue;
        }
        match &marked {
            Some(marked) if marked.start < range.end && marked.end > range.start => {
                // Split the tile at the marked-range boundaries; the overlap
                // gets the composition underline.
                let cuts = [range.start, marked.start.max(range.start), marked.end.min(range.end), range.end];
                for window in cuts.windows(2) {
                    let (a, b) = (window[0], window[1]);
                    if a >= b {
                        continue;
                    }
                    let in_marked = a >= marked.start && b <= marked.end;
                    runs.push(make_run(b - a, *style, voice, palette, in_marked));
                }
            }
            _ => runs.push(make_run(range.end - range.start, *style, voice, palette, false)),
        }
    }
    runs
}

fn make_run(len: usize, style: InlineStyle, voice: Voice, palette: [Hsla; 4], marked: bool) -> TextRun {
    let color = ink_color(style.ink, palette);
    let underline = if style.underline || marked {
        Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        })
    } else {
        None
    };
    let strikethrough = if style.strike {
        Some(gpui::StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        })
    } else {
        None
    };
    TextRun {
        len,
        font: run_font(voice, style),
        color,
        background_color: None,
        underline,
        strikethrough,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use muse_core::{Document, EntryId, StyleToggle};
    use muse_theme::paper;

    fn doc_with(text: &str) -> Document {
        let mut doc = Document::new(EntryId::default());
        doc.insert(0, text);
        doc
    }

    #[test]
    fn runs_tile_a_styled_paragraph() {
        let mut doc = doc_with("plain bold italic");
        doc.toggle_style(6..10, StyleToggle::Bold);
        doc.toggle_style(11..17, StyleToggle::Italic);
        doc.toggle_style(11..17, StyleToggle::Ink(Some(muse_core::Ink::Moss)));

        let palette = paper().ink_palette();
        let voice = Voice::default();
        let tiles = doc.spans().runs_in(0..17);
        let runs = paragraph_runs(&tiles, 17, voice, None, palette);

        // plain | bold | plain(space) | italic+moss
        assert_eq!(runs.len(), 4);
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), 17);
        assert_eq!(runs[0].font.weight, FontWeight(400.0));
        assert_eq!(runs[1].font.weight, FontWeight(700.0));
        assert_eq!(runs[3].font.style, FontStyle::Italic);
        assert_eq!(runs[3].color, palette[3]);
        // The default voice is the Literata serif.
        assert_eq!(runs[0].font.family.as_ref(), "Literata");
    }

    #[test]
    fn bold_weight_caps_for_heavy_voices() {
        let voice = Voice {
            weight: 700,
            ..Voice::default()
        };
        let style = InlineStyle {
            bold: true,
            ..InlineStyle::default()
        };
        assert_eq!(run_font(voice, style).weight, FontWeight(800.0));
    }

    #[test]
    fn underline_and_strike_carry_the_run_color() {
        let mut doc = doc_with("under struck");
        doc.toggle_style(0..5, StyleToggle::Underline);
        doc.toggle_style(6..12, StyleToggle::Strike);
        doc.toggle_style(6..12, StyleToggle::Ink(Some(muse_core::Ink::Rose)));

        let palette = paper().ink_palette();
        let tiles = doc.spans().runs_in(0..12);
        let runs = paragraph_runs(&tiles, 12, Voice::default(), None, palette);
        assert_eq!(runs[0].underline.as_ref().unwrap().color, Some(palette[0]));
        let strike = runs.last().unwrap().strikethrough.as_ref().unwrap();
        assert_eq!(strike.color, Some(palette[1]));
    }

    #[test]
    fn marked_range_splits_and_underlines() {
        let doc = doc_with("composing here");
        let palette = paper().ink_palette();
        let tiles = doc.spans().runs_in(0..14);
        let runs = paragraph_runs(&tiles, 14, Voice::default(), Some(4..9), palette);

        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), 14);
        assert_eq!(runs.len(), 3);
        assert!(runs[0].underline.is_none());
        assert!(runs[1].underline.is_some(), "marked overlap must underline");
        assert!(runs[2].underline.is_none());
    }

    #[test]
    fn empty_paragraph_yields_no_runs() {
        let doc = Document::new(EntryId::default());
        let tiles = doc.spans().runs_in(0..0);
        let runs = paragraph_runs(&tiles, 0, Voice::default(), None, paper().ink_palette());
        assert!(runs.is_empty());
    }
}
