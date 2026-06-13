//! The styled-clipboard envelope: a JSON description of the copied text and
//! its inline styles, carried as metadata on the plain-text clipboard entry
//! (`ClipboardItem::new_string_with_metadata`). Paste prefers the envelope
//! only when its text matches the clipboard's plain text, so edits made in
//! another app safely fall back to plain.
//!
//! Serialization is hand-rolled over `serde_json::Value` (this crate
//! depends on `serde_json` but not on `serde` itself); `InlineStyle`'s own
//! serde impls from daisynotes-core do the style legwork.

use std::ops::Range;

use daisynotes_core::InlineStyle;
use serde_json::{Value, json};

/// Current envelope format version.
const VERSION: u64 = 1;

/// One styled run within the envelope; offsets are bytes into
/// [`Envelope::text`]. Plain runs are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvelopeRun {
    pub start: usize,
    pub end: usize,
    pub style: InlineStyle,
}

/// The clipboard payload Muse round-trips styled text through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Envelope {
    pub text: String,
    pub runs: Vec<EnvelopeRun>,
}

impl Envelope {
    /// Build an envelope from copied text and the style tiles covering it
    /// (tile offsets relative to the copied text). Plain tiles are dropped.
    pub fn new(text: String, tiles: &[(Range<usize>, InlineStyle)]) -> Self {
        let runs = tiles
            .iter()
            .filter(|(range, style)| !style.is_plain() && range.start < range.end)
            .map(|(range, style)| EnvelopeRun {
                start: range.start,
                end: range.end,
                style: *style,
            })
            .collect();
        Self { text, runs }
    }

    /// Decode and validate an envelope. Returns `None` for foreign metadata,
    /// version mismatches, or runs that don't lie on char boundaries of the
    /// carried text — paste then falls back to plain text.
    pub fn decode(json: &str) -> Option<Envelope> {
        let value: Value = serde_json::from_str(json).ok()?;
        if value.get("v")?.as_u64()? != VERSION {
            return None;
        }
        let text = value.get("text")?.as_str()?.to_string();
        let mut runs = Vec::new();
        for entry in value.get("runs")?.as_array()? {
            let start = usize::try_from(entry.get("start")?.as_u64()?).ok()?;
            let end = usize::try_from(entry.get("end")?.as_u64()?).ok()?;
            let style: InlineStyle =
                serde_json::from_value(entry.get("style")?.clone()).ok()?;
            if start > end
                || end > text.len()
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return None;
            }
            runs.push(EnvelopeRun { start, end, style });
        }
        Some(Envelope { text, runs })
    }

    /// Serialize for the clipboard metadata slot.
    pub fn encode(&self) -> Option<String> {
        let runs: Vec<Value> = self
            .runs
            .iter()
            .map(|run| {
                Some(json!({
                    "start": run.start,
                    "end": run.end,
                    "style": serde_json::to_value(run.style).ok()?,
                }))
            })
            .collect::<Option<_>>()?;
        Some(
            json!({
                "v": VERSION,
                "text": self.text,
                "runs": runs,
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use daisynotes_core::Ink;

    fn styled(bold: bool, ink: Option<Ink>) -> InlineStyle {
        InlineStyle {
            bold,
            ink,
            ..InlineStyle::default()
        }
    }

    #[test]
    fn envelope_round_trips() {
        let tiles = vec![
            (0..5, styled(true, None)),
            (5..7, InlineStyle::default()),
            (7..12, styled(false, Some(Ink::Lavender))),
        ];
        let envelope = Envelope::new("hello, world".to_string(), &tiles);
        // The plain middle tile is not stored.
        assert_eq!(envelope.runs.len(), 2);

        let json = envelope.encode().unwrap();
        let decoded = Envelope::decode(&json).unwrap();
        assert_eq!(decoded, envelope);
        assert_eq!(decoded.runs[1].style.ink, Some(Ink::Lavender));
    }

    #[test]
    fn decode_rejects_bad_payloads() {
        assert!(Envelope::decode("not json").is_none());
        assert!(Envelope::decode("{\"v\":9,\"text\":\"x\",\"runs\":[]}").is_none());
        // Run past the end of the text.
        let bad = Envelope {
            text: "ab".into(),
            runs: vec![EnvelopeRun {
                start: 0,
                end: 9,
                style: styled(true, None),
            }],
        };
        assert!(Envelope::decode(&bad.encode().unwrap()).is_none());
        // Run splitting a multi-byte char.
        let bad = Envelope {
            text: "é".into(),
            runs: vec![EnvelopeRun {
                start: 0,
                end: 1,
                style: styled(true, None),
            }],
        };
        assert!(Envelope::decode(&bad.encode().unwrap()).is_none());
    }

    #[test]
    fn empty_selection_encodes_cleanly() {
        let envelope = Envelope::new(String::new(), &[]);
        let decoded = Envelope::decode(&envelope.encode().unwrap()).unwrap();
        assert!(decoded.text.is_empty() && decoded.runs.is_empty());
    }
}
