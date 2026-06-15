//! daisynotes-clipboard — the one place that talks to the macOS pasteboard and
//! AppKit's attributed-string machinery.
//!
//! Copy writes several flavors at once so every target app finds one it speaks:
//! our own lossless [`DocFragment`] JSON (for self-paste), `public.rtf` (Apple
//! Notes, Mail, TextEdit, Pages), and plain text (everyone else). Paste reads
//! them back in priority order: our fragment first, then RTF, then plain.
//!
//! The neutral model in the middle is [`DocFragment`]. Each external format is
//! one translation against it — adding a *format* is a new adapter; adding a
//! *formatting feature* is a field on the fragment plus its mapping here, never
//! a per-feature branch at the call sites.

use daisynotes_core::DocFragment;

/// The result of reading the pasteboard.
pub enum Paste {
    /// Our own fragment, decoded losslessly (a Daisy Notes → Daisy Notes paste).
    Fragment(DocFragment),
    /// Rich text from another app, parsed into a fragment via RTF.
    External(DocFragment),
    /// Only plain text was available; the caller inserts it as-is.
    Plain(String),
    /// Nothing usable on the pasteboard.
    Empty,
}

/// Write `fragment` to the system pasteboard in every flavor we can produce.
#[cfg(target_os = "macos")]
pub fn write_fragment(fragment: &DocFragment) {
    mac::write(fragment);
}

/// Read the system pasteboard, preferring the richest flavor we understand.
#[cfg(target_os = "macos")]
pub fn read() -> Paste {
    mac::read()
}

#[cfg(not(target_os = "macos"))]
pub fn write_fragment(_fragment: &DocFragment) {}

#[cfg(not(target_os = "macos"))]
pub fn read() -> Paste {
    Paste::Empty
}

// ── UTF-8 ↔ UTF-16 offset mapping ────────────────────────────────────────────
// Our fragment offsets are UTF-8 byte indices; NSAttributedString ranges are
// UTF-16 code-unit indices. These convert between them against a known string.

/// Byte offset (UTF-8) → UTF-16 code-unit offset within `text`.
#[cfg(target_os = "macos")]
fn utf8_to_utf16(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// UTF-16 code-unit offset → byte offset (UTF-8) within `text`.
#[cfg(target_os = "macos")]
fn utf16_to_utf8(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

#[cfg(target_os = "macos")]
mod mac {
    use daisynotes_core::{DocFragment, Ink, InlineStyle};

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_app_kit::{
        NSAttributedStringAppKitDocumentFormats, NSColor, NSColorType, NSFont,
        NSFontAttributeName, NSFontDescriptorSymbolicTraits, NSForegroundColorAttributeName,
        NSPasteboard, NSPasteboardTypeRTF, NSPasteboardTypeString,
        NSStrikethroughStyleAttributeName, NSUnderlineStyleAttributeName,
    };
    use objc2_foundation::{
        NSAttributedString, NSData, NSDictionary, NSMutableAttributedString, NSNumber, NSRange,
        NSString,
    };

    use super::{Paste, utf16_to_utf8, utf8_to_utf16};

    /// Our private pasteboard flavor carrying the lossless fragment JSON.
    const FRAGMENT_UTI: &str = "app.daisynotes.fragment";

    // ── Public entry points ──────────────────────────────────────────────────

    pub(super) fn write(fragment: &DocFragment) {
        // Safety: standard AppKit pasteboard writes, all on the main thread
        // (copy is a user action dispatched on the UI thread).
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();

        // Plain text — the universal fallback.
        let plain = NSString::from_str(&fragment.text);
        pb.setString_forType(&plain, unsafe { NSPasteboardTypeString });

        // Our lossless fragment, for a Daisy Notes → Daisy Notes paste.
        if let Some(json) = fragment.to_json() {
            let uti = NSString::from_str(FRAGMENT_UTI);
            let value = NSString::from_str(&json);
            pb.setString_forType(&value, &uti);
        }

        // RTF, for Apple Notes / native rich-text apps.
        if let Some(rtf) = fragment_to_rtf(fragment) {
            pb.setData_forType(Some(&rtf), unsafe { NSPasteboardTypeRTF });
        }
    }

    pub(super) fn read() -> Paste {
        let pb = NSPasteboard::generalPasteboard();

        // 1. Our own fragment (lossless).
        let uti = NSString::from_str(FRAGMENT_UTI);
        if let Some(json) = pb.stringForType(&uti)
            && let Some(fragment) = DocFragment::from_json(&json.to_string())
        {
            return Paste::Fragment(fragment);
        }

        // 2. Rich text from another app, via RTF.
        if let Some(data) = pb.dataForType(unsafe { NSPasteboardTypeRTF })
            && let Some(fragment) = rtf_to_fragment(&data)
        {
            return Paste::External(fragment);
        }

        // 3. Plain text.
        if let Some(s) = pb.stringForType(unsafe { NSPasteboardTypeString }) {
            return Paste::Plain(s.to_string());
        }

        Paste::Empty
    }

    // ── DocFragment → RTF ─────────────────────────────────────────────────────

    /// Build an `NSAttributedString` from the fragment's text + inline styles
    /// and serialize it to RTF bytes. Lists and images are not yet emitted to
    /// RTF (self-paste carries them losslessly via the fragment flavor).
    pub(crate) fn fragment_to_rtf(fragment: &DocFragment) -> Option<Retained<NSData>> {
        let text = &fragment.text;
        let ns = NSString::from_str(text);
        let attributed =
            NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &ns);

        for run in &fragment.runs {
            let start = utf8_to_utf16(text, run.start);
            let end = utf8_to_utf16(text, run.end);
            if end <= start {
                continue;
            }
            let range = NSRange {
                location: start,
                length: end - start,
            };
            apply_style(&attributed, &run.style, range);
        }

        let full = NSRange {
            location: 0,
            length: attributed.length(),
        };
        let doc_attrs = NSDictionary::new();
        unsafe { attributed.RTFFromRange_documentAttributes(full, &doc_attrs) }
    }

    /// Apply one inline style over a UTF-16 range of the attributed string.
    fn apply_style(attributed: &NSMutableAttributedString, style: &InlineStyle, range: NSRange) {
        if style.bold || style.italic {
            let font = styled_font(style.bold, style.italic);
            unsafe {
                attributed.addAttribute_value_range(NSFontAttributeName, &font, range);
            }
        }
        if style.underline {
            let one = NSNumber::numberWithInt(1);
            unsafe {
                attributed.addAttribute_value_range(NSUnderlineStyleAttributeName, &one, range);
            }
        }
        if style.strike {
            let one = NSNumber::numberWithInt(1);
            unsafe {
                attributed.addAttribute_value_range(NSStrikethroughStyleAttributeName, &one, range);
            }
        }
        if let Some(ink) = style.ink {
            let (r, g, b) = ink_rgb(ink);
            let color =
                NSColor::colorWithSRGBRed_green_blue_alpha(r as f64, g as f64, b as f64, 1.0);
            unsafe {
                attributed.addAttribute_value_range(NSForegroundColorAttributeName, &color, range);
            }
        }
    }

    /// A system font carrying the requested bold/italic traits, built through a
    /// font descriptor so no `MainThreadMarker` (NSFontManager) is needed.
    fn styled_font(bold: bool, italic: bool) -> Retained<NSFont> {
        let base = NSFont::systemFontOfSize(12.0);
        let mut traits = NSFontDescriptorSymbolicTraits(0);
        if bold {
            traits |= NSFontDescriptorSymbolicTraits::TraitBold;
        }
        if italic {
            traits |= NSFontDescriptorSymbolicTraits::TraitItalic;
        }
        let descriptor = base.fontDescriptor().fontDescriptorWithSymbolicTraits(traits);
        NSFont::fontWithDescriptor_size(&descriptor, 12.0).unwrap_or(base)
    }

    // ── RTF → DocFragment ─────────────────────────────────────────────────────

    /// Parse RTF bytes into a fragment via `NSAttributedString`, reading inline
    /// styles back off each attribute run.
    pub(crate) fn rtf_to_fragment(data: &NSData) -> Option<DocFragment> {
        let attributed = unsafe {
            NSAttributedString::initWithRTF_documentAttributes(
                NSAttributedString::alloc(),
                data,
                None,
            )
        }?;

        let text = attributed.string().to_string();
        let len = attributed.length();

        let mut runs: Vec<(std::ops::Range<usize>, InlineStyle)> = Vec::new();
        let mut i = 0usize;
        while i < len {
            let mut eff = NSRange {
                location: 0,
                length: 0,
            };
            let attrs = unsafe { attributed.attributesAtIndex_effectiveRange(i, &mut eff) };
            let style = style_from_attrs(&attrs);
            if !style.is_plain() {
                let start = utf16_to_utf8(&text, eff.location);
                let end = utf16_to_utf8(&text, eff.location + eff.length);
                if start < end {
                    runs.push((start..end, style));
                }
            }
            let next = eff.location + eff.length;
            i = if next > i { next } else { i + 1 };
        }

        Some(DocFragment::new(text, runs, Vec::new(), Vec::new()))
    }

    /// Read an [`InlineStyle`] out of one attribute-run dictionary.
    fn style_from_attrs(attrs: &NSDictionary<NSString, AnyObject>) -> InlineStyle {
        let mut style = InlineStyle::default();

        if let Some(font) = unsafe { attrs.objectForKey(NSFontAttributeName) }
            && let Ok(font) = font.downcast::<NSFont>()
        {
            let traits = font.fontDescriptor().symbolicTraits();
            style.bold = traits.contains(NSFontDescriptorSymbolicTraits::TraitBold);
            style.italic = traits.contains(NSFontDescriptorSymbolicTraits::TraitItalic);
        }
        if let Some(num) = unsafe { attrs.objectForKey(NSUnderlineStyleAttributeName) }
            && let Ok(num) = num.downcast::<NSNumber>()
        {
            style.underline = num.intValue() != 0;
        }
        if let Some(num) = unsafe { attrs.objectForKey(NSStrikethroughStyleAttributeName) }
            && let Ok(num) = num.downcast::<NSNumber>()
        {
            style.strike = num.intValue() != 0;
        }
        if let Some(color) = unsafe { attrs.objectForKey(NSForegroundColorAttributeName) }
            && let Ok(color) = color.downcast::<NSColor>()
        {
            style.ink = nearest_ink(&color);
        }
        style
    }

    // ── Ink ↔ RGB ─────────────────────────────────────────────────────────────

    /// Representative sRGB for each ink on export (0..=1 components).
    fn ink_rgb(ink: Ink) -> (f32, f32, f32) {
        match ink {
            // Rose maps to the accent (#1E90FF), lavender to the muse hue, moss
            // to #5F7A5A. Exact fidelity is not required — self-paste is lossless
            // via the fragment flavor; this is the cross-app approximation.
            Ink::Rose => (0x1E as f32 / 255.0, 0x90 as f32 / 255.0, 0xFF as f32 / 255.0),
            Ink::Lavender => (0x9B as f32 / 255.0, 0x8C as f32 / 255.0, 0xEC as f32 / 255.0),
            Ink::Moss => (0x5F as f32 / 255.0, 0x7A as f32 / 255.0, 0x5A as f32 / 255.0),
        }
    }

    /// Snap an arbitrary NSColor to the nearest ink, or `None` if it is closer
    /// to plain text (near-black/near-white) than to any ink.
    fn nearest_ink(color: &NSColor) -> Option<Ink> {
        let srgb = color.colorUsingType(NSColorType::ComponentBased)?;
        let r = srgb.redComponent() as f32;
        let g = srgb.greenComponent() as f32;
        let b = srgb.blueComponent() as f32;
        let candidates = [
            (Ink::Rose, ink_rgb(Ink::Rose)),
            (Ink::Lavender, ink_rgb(Ink::Lavender)),
            (Ink::Moss, ink_rgb(Ink::Moss)),
        ];
        let dist = |a: (f32, f32, f32)| {
            (a.0 - r).powi(2) + (a.1 - g).powi(2) + (a.2 - b).powi(2)
        };
        // Plain text (default ink) sits near black; if the color is closer to
        // black than to any ink, treat it as no ink.
        let black = (0.0, 0.0, 0.0);
        let best = candidates
            .into_iter()
            .min_by(|x, y| dist(x.1).partial_cmp(&dist(y.1)).unwrap_or(std::cmp::Ordering::Equal))?;
        if dist(black) < dist(best.1) {
            None
        } else {
            Some(best.0)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn rtf_round_trips_inline_styles() {
            // bold "Hello", plain " ", italic+underline "world".
            let runs = vec![
                (
                    0..5,
                    InlineStyle {
                        bold: true,
                        ..InlineStyle::default()
                    },
                ),
                (
                    6..11,
                    InlineStyle {
                        italic: true,
                        underline: true,
                        ..InlineStyle::default()
                    },
                ),
            ];
            let frag = DocFragment::new("Hello world".to_string(), runs, Vec::new(), Vec::new());

            let rtf = fragment_to_rtf(&frag).expect("export RTF");
            let back = rtf_to_fragment(&rtf).expect("import RTF");

            assert_eq!(back.text, "Hello world");
            // "Hello" is bold.
            let bold = back
                .runs
                .iter()
                .find(|r| r.start == 0)
                .expect("bold run");
            assert!(bold.style.bold);
            // "world" is italic + underline.
            let ital = back
                .runs
                .iter()
                .find(|r| r.start == 6)
                .expect("italic run");
            assert!(ital.style.italic);
            assert!(ital.style.underline);
        }

        #[test]
        #[ignore = "writes to the real system pasteboard"]
        fn pasteboard_round_trips_fragment() {
            let runs = vec![(
                0..5,
                InlineStyle {
                    bold: true,
                    ..InlineStyle::default()
                },
            )];
            let frag = DocFragment::new("Hello world".to_string(), runs, Vec::new(), Vec::new());
            crate::write_fragment(&frag);
            match crate::read() {
                Paste::Fragment(f) => {
                    assert_eq!(f.text, "Hello world");
                    assert!(f.runs.iter().any(|r| r.style.bold), "bold survives");
                }
                _ => panic!("expected our own fragment back from the pasteboard"),
            }
        }

        #[test]
        fn rtf_round_trips_color() {
            use daisynotes_core::Ink;
            let runs = vec![(
                0..4,
                InlineStyle {
                    ink: Some(Ink::Moss),
                    ..InlineStyle::default()
                },
            )];
            let frag = DocFragment::new("leaf".to_string(), runs, Vec::new(), Vec::new());
            let rtf = fragment_to_rtf(&frag).expect("export RTF");
            let back = rtf_to_fragment(&rtf).expect("import RTF");
            let run = back.runs.iter().find(|r| r.start == 0).expect("color run");
            assert_eq!(run.style.ink, Some(Ink::Moss));
        }
    }
}
