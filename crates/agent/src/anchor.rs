//! Quote anchoring and dismissal memory.
//!
//! The model quotes the entry verbatim; [`locate_quote`] turns that quote
//! (plus a few characters of surrounding context) into a byte range the app
//! registers as a `muse_core` anchor. [`dismissal_digest`] fingerprints a
//! dismissed note so the same ground is never retrod.

use std::ops::Range;

use crate::types::NoteKind;

/// Find the byte range of `quote` inside `text`.
///
/// Exact match only — the model is instructed to quote verbatim. When the
/// quote occurs more than once, the occurrence whose surrounding text best
/// matches `prefix` (the characters immediately before) and `suffix` (the
/// characters immediately after) wins; ties go to the first occurrence.
/// Returns `None` for an empty quote or when the quote does not occur.
#[must_use]
pub fn locate_quote(text: &str, quote: &str, prefix: &str, suffix: &str) -> Option<Range<usize>> {
    if quote.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize)> = None; // (score, byte start)
    for (start, _) in text.match_indices(quote) {
        let score = context_score(text, start, quote.len(), prefix, suffix);
        let beaten = best.is_some_and(|(best_score, _)| best_score >= score);
        if !beaten {
            best = Some((score, start));
        }
    }
    best.map(|(_, start)| start..start + quote.len())
}

/// How well the text around an occurrence matches the claimed context:
/// the number of chars of `prefix` that line up walking backwards from the
/// occurrence, plus the chars of `suffix` that line up walking forwards.
fn context_score(text: &str, start: usize, quote_len: usize, prefix: &str, suffix: &str) -> usize {
    // `start` and `start + quote_len` are boundaries of an exact substring
    // match, so both slices below are char-aligned.
    let before = &text[..start];
    let after = &text[start + quote_len..];
    let prefix_overlap = before
        .chars()
        .rev()
        .zip(prefix.chars().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix_overlap = after
        .chars()
        .zip(suffix.chars())
        .take_while(|(a, b)| a == b)
        .count();
    prefix_overlap + suffix_overlap
}

/// Stable fingerprint of a dismissed note: FNV-1a (64-bit, lowercase hex)
/// over the note kind and the whitespace-collapsed, lowercased quote.
/// Two dismissals of "the same note on the same words" always collide,
/// regardless of incidental whitespace or casing drift.
#[must_use]
pub fn dismissal_digest(quote: &str, kind: NoteKind) -> String {
    let mut normalized = String::with_capacity(quote.len());
    for (i, word) in quote.split_whitespace().enumerate() {
        if i > 0 {
            normalized.push(' ');
        }
        for ch in word.chars() {
            normalized.extend(ch.to_lowercase());
        }
    }

    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    let bytes = kind
        .as_str()
        .bytes()
        .chain(std::iter::once(b':'))
        .chain(normalized.bytes());
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_unique_quote() {
        let text = "The harbor was empty by noon.";
        assert_eq!(locate_quote(text, "empty by noon", "", ""), Some(15..28));
    }

    #[test]
    fn empty_or_missing_quote_is_none() {
        let text = "Some words here.";
        assert_eq!(locate_quote(text, "", "Some", "words"), None);
        assert_eq!(locate_quote(text, "absent", "", ""), None);
    }

    #[test]
    fn repeated_phrase_disambiguated_by_prefix() {
        let text = "I was tired. She said I was tired too, and laughed.";
        let range = locate_quote(text, "I was tired", "She said ", " too").map(|r| r.start);
        assert_eq!(range, Some(22));
        let first = locate_quote(text, "I was tired", "", ". She").map(|r| r.start);
        assert_eq!(first, Some(0));
    }

    #[test]
    fn repeated_phrase_disambiguated_by_suffix_alone() {
        let text = "the light, the light again";
        let range = locate_quote(text, "the light", "", " again");
        assert_eq!(range, Some(11..20));
    }

    #[test]
    fn no_context_falls_back_to_first_occurrence() {
        let text = "echo echo echo";
        assert_eq!(locate_quote(text, "echo", "", ""), Some(0..4));
    }

    #[test]
    fn handles_unicode_text_and_context() {
        let text = "Le café était vide. Le café était plein hier — étrange.";
        // Both occurrences of "Le café était"; the suffix picks the second.
        let range = locate_quote(text, "Le café était", "", " plein");
        let start = "Le café était vide. ".len();
        assert_eq!(range, Some(start..start + "Le café était".len()));
        // Returned range slices cleanly.
        let r = range.expect("range exists");
        assert_eq!(&text[r], "Le café était");
    }

    #[test]
    fn handles_emoji_quotes() {
        let text = "morning 🌊 swim, evening 🌊 swim";
        let range = locate_quote(text, "🌊 swim", "evening ", "");
        let start = "morning 🌊 swim, evening ".len();
        assert_eq!(range, Some(start..start + "🌊 swim".len()));
    }

    #[test]
    fn partial_context_overlap_still_scores() {
        // Model's prefix is slightly off at its far end but the near
        // characters match; nearest-character overlap should still win.
        let text = "a red door. a red door at the end.";
        let range = locate_quote(text, "a red door", "", " at the");
        assert_eq!(range, Some(12..22));
    }

    #[test]
    fn digest_is_stable_against_a_golden_value() {
        // FNV-1a 64 over "insight:hello world" — locked so persisted
        // dismissals survive refactors.
        assert_eq!(
            dismissal_digest("hello world", NoteKind::Insight),
            "c8ef96652b9038b9"
        );
        assert_eq!(
            dismissal_digest("hello world", NoteKind::Question),
            "0116d9a10fa1d67f"
        );
    }

    #[test]
    fn digest_normalizes_whitespace_and_case() {
        let a = dismissal_digest("Hello   World", NoteKind::Insight);
        let b = dismissal_digest("  hello\nworld ", NoteKind::Insight);
        let c = dismissal_digest("\thello world\t", NoteKind::Insight);
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn digest_separates_kinds_and_quotes() {
        let base = dismissal_digest("hello world", NoteKind::Insight);
        assert_ne!(base, dismissal_digest("hello world", NoteKind::Reference));
        assert_ne!(base, dismissal_digest("hello worlds", NoteKind::Insight));
        assert_eq!(base.len(), 16);
        assert!(base.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
