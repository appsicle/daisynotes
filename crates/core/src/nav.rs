//! Grapheme, word, and paragraph navigation over a rope, in byte offsets.
//!
//! Grapheme boundaries stream rope chunks through `GraphemeCursor` so no
//! full-text copy is ever made. Word segmentation materializes one line at a
//! time (words never cross newlines), which bounds allocation to a paragraph.

use std::ops::Range;

use ropey::Rope;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};

/// Clamps to `len` and snaps down to the nearest `char` boundary.
pub(crate) fn clamp(rope: &Rope, offset: usize) -> usize {
    let offset = offset.min(rope.len_bytes());
    // byte_to_char floors into the containing char; mapping back snaps down.
    rope.char_to_byte(rope.byte_to_char(offset))
}

/// The next grapheme-cluster boundary after `offset` (or `len` at the end).
pub(crate) fn next_grapheme_boundary(rope: &Rope, offset: usize) -> usize {
    let len = rope.len_bytes();
    let offset = clamp(rope, offset);
    if offset >= len {
        return len;
    }
    let mut cursor = GraphemeCursor::new(offset, len, true);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(offset);
    loop {
        match cursor.next_boundary(chunk, chunk_start) {
            Ok(Some(boundary)) => return boundary,
            Ok(None) => return len,
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_start += chunk.len();
                let (next, _, _, _) = rope.chunk_at_byte(chunk_start);
                chunk = next;
            }
            Err(GraphemeIncomplete::PreContext(idx)) => {
                let (ctx, ctx_start) = pre_context(rope, idx);
                cursor.provide_context(ctx, ctx_start);
            }
            // InvalidOffset cannot occur: `offset` is boundary-aligned.
            Err(_) => return next_char_boundary(rope, offset),
        }
    }
}

/// The previous grapheme-cluster boundary before `offset` (or 0).
pub(crate) fn prev_grapheme_boundary(rope: &Rope, offset: usize) -> usize {
    let offset = clamp(rope, offset);
    if offset == 0 {
        return 0;
    }
    let mut cursor = GraphemeCursor::new(offset, rope.len_bytes(), true);
    let (mut chunk, mut chunk_start, _, _) = rope.chunk_at_byte(offset);
    loop {
        match cursor.prev_boundary(chunk, chunk_start) {
            Ok(Some(boundary)) => return boundary,
            Ok(None) => return 0,
            Err(GraphemeIncomplete::PrevChunk) => {
                let (prev, start, _, _) = rope.chunk_at_byte(chunk_start.saturating_sub(1));
                chunk = prev;
                chunk_start = start;
            }
            Err(GraphemeIncomplete::PreContext(idx)) => {
                let (ctx, ctx_start) = pre_context(rope, idx);
                cursor.provide_context(ctx, ctx_start);
            }
            // InvalidOffset cannot occur: `offset` is boundary-aligned.
            Err(_) => return prev_char_boundary(rope, offset),
        }
    }
}

/// End of the word at-or-after `offset` (Option+Right). `len` when none.
pub(crate) fn next_word_boundary(rope: &Rope, offset: usize) -> usize {
    let len = rope.len_bytes();
    let offset = clamp(rope, offset);
    if offset >= len {
        return len;
    }
    let mut line_idx = rope.byte_to_line(offset);
    while line_idx < rope.len_lines() {
        let line_start = rope.line_to_byte(line_idx);
        let line = rope.line(line_idx).to_string();
        for (i, seg) in line.split_word_bound_indices() {
            let end = line_start + i + seg.len();
            if end > offset && is_word(seg) {
                return end;
            }
        }
        line_idx += 1;
    }
    len
}

/// Start of the word at-or-before `offset` (Option+Left). 0 when none.
pub(crate) fn prev_word_boundary(rope: &Rope, offset: usize) -> usize {
    let offset = clamp(rope, offset);
    if offset == 0 {
        return 0;
    }
    let mut line_idx = rope.byte_to_line(offset);
    loop {
        let line_start = rope.line_to_byte(line_idx);
        let line = rope.line(line_idx).to_string();
        let mut best: Option<usize> = None;
        for (i, seg) in line.split_word_bound_indices() {
            let start = line_start + i;
            if start >= offset {
                break;
            }
            if is_word(seg) {
                best = Some(start);
            }
        }
        if let Some(start) = best {
            return start;
        }
        if line_idx == 0 {
            return 0;
        }
        line_idx -= 1;
    }
}

/// The word-bound segment containing `offset` (double-click selection).
/// Whitespace and punctuation runs select as their own segment, matching
/// macOS behavior. Returns `0..0` for an empty document.
pub(crate) fn word_range_at(rope: &Rope, offset: usize) -> Range<usize> {
    let len = rope.len_bytes();
    if len == 0 {
        return 0..0;
    }
    let mut offset = clamp(rope, offset);
    if offset == len {
        offset = prev_grapheme_boundary(rope, len);
    }
    let line_idx = rope.byte_to_line(offset);
    let line_start = rope.line_to_byte(line_idx);
    let line = rope.line(line_idx).to_string();
    for (i, seg) in line.split_word_bound_indices() {
        let start = line_start + i;
        let end = start + seg.len();
        if start <= offset && offset < end {
            return start..end;
        }
    }
    offset..offset
}

/// The newline-delimited line containing `offset`, excluding its trailing
/// line break (triple-click selection).
pub(crate) fn paragraph_range_at(rope: &Rope, offset: usize) -> Range<usize> {
    let offset = clamp(rope, offset);
    let line_idx = rope.byte_to_line(offset);
    let start = rope.line_to_byte(line_idx);
    let line = rope.line(line_idx);
    start..start + line_len_sans_break(line)
}

/// Byte length of a line slice with any trailing line break removed.
fn line_len_sans_break(line: ropey::RopeSlice<'_>) -> usize {
    let mut len = line.len_bytes();
    let chars = line.len_chars();
    if chars == 0 {
        return len;
    }
    let last = line.char(chars - 1);
    if matches!(
        last,
        '\n' | '\u{000B}' | '\u{000C}' | '\u{000D}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    ) {
        len -= last.len_utf8();
        if last == '\n' && chars > 1 && line.char(chars - 2) == '\r' {
            len -= 1;
        }
    }
    len
}

/// The chunk slice ending exactly at `idx`, as `GraphemeCursor` pre-context.
/// `provide_context` asserts `start + chunk.len() == idx`, so the containing
/// chunk must be clipped — handing it over whole panics on multi-chunk ropes.
fn pre_context(rope: &Rope, idx: usize) -> (&str, usize) {
    let (chunk, chunk_start, _, _) = rope.chunk_at_byte(idx.saturating_sub(1));
    (&chunk[..idx - chunk_start], chunk_start)
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

fn next_char_boundary(rope: &Rope, offset: usize) -> usize {
    let char_idx = rope.byte_to_char(offset);
    rope.char_to_byte((char_idx + 1).min(rope.len_chars()))
}

fn prev_char_boundary(rope: &Rope, offset: usize) -> usize {
    let char_idx = rope.byte_to_char(offset);
    rope.char_to_byte(char_idx.saturating_sub(1))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn graphemes_over_emoji_and_combining_marks() {
        // "a" + family emoji (ZWJ sequence) + "e" + combining acute.
        let text = "a\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}e\u{0301}b";
        let rope = Rope::from_str(text);
        let mut boundaries = vec![0];
        let mut at = 0;
        while at < rope.len_bytes() {
            at = next_grapheme_boundary(&rope, at);
            boundaries.push(at);
        }
        // a | family | é | b
        assert_eq!(boundaries.len(), 5);
        assert_eq!(boundaries[1], 1);
        assert_eq!(boundaries[2], 1 + 18); // 3 emoji (4 bytes) + 2 ZWJ (3 bytes)
        assert_eq!(boundaries[3], boundaries[2] + 3); // e + U+0301
        assert_eq!(*boundaries.last().unwrap(), text.len());
        // Walk backwards over the same boundaries.
        let mut back = vec![rope.len_bytes()];
        let mut at = rope.len_bytes();
        while at > 0 {
            at = prev_grapheme_boundary(&rope, at);
            back.push(at);
        }
        back.reverse();
        assert_eq!(back, boundaries);
    }

    #[test]
    fn grapheme_at_edges() {
        let rope = Rope::from_str("hi");
        assert_eq!(prev_grapheme_boundary(&rope, 0), 0);
        assert_eq!(next_grapheme_boundary(&rope, 2), 2);
        assert_eq!(next_grapheme_boundary(&rope, 99), 2);
        let empty = Rope::new();
        assert_eq!(next_grapheme_boundary(&empty, 0), 0);
        assert_eq!(prev_grapheme_boundary(&empty, 0), 0);
    }

    #[test]
    fn word_motion() {
        let rope = Rope::from_str("hello, wide world\nsecond line");
        assert_eq!(next_word_boundary(&rope, 0), 5); // end of "hello"
        assert_eq!(next_word_boundary(&rope, 5), 11); // end of "wide"
        assert_eq!(next_word_boundary(&rope, 12), 17); // end of "world"
        assert_eq!(next_word_boundary(&rope, 17), 24); // "second" on next line
        assert_eq!(prev_word_boundary(&rope, 17), 12); // start of "world"
        assert_eq!(prev_word_boundary(&rope, 18), 12); // from start of line 2
        assert_eq!(prev_word_boundary(&rope, 3), 0);
        assert_eq!(
            next_word_boundary(&rope, rope.len_bytes()),
            rope.len_bytes()
        );
    }

    #[test]
    fn word_range_double_click() {
        let rope = Rope::from_str("don't stop");
        assert_eq!(word_range_at(&rope, 2), 0..5); // apostrophe stays in word
        assert_eq!(word_range_at(&rope, 5), 5..6); // the space itself
        assert_eq!(word_range_at(&rope, 8), 6..10);
        assert_eq!(word_range_at(&rope, 10), 6..10); // end of doc
        assert_eq!(word_range_at(&Rope::new(), 0), 0..0);
    }

    #[test]
    fn paragraph_range_triple_click() {
        let rope = Rope::from_str("first\nsecond para\n\nfourth");
        assert_eq!(paragraph_range_at(&rope, 0), 0..5);
        assert_eq!(paragraph_range_at(&rope, 5), 0..5); // on the newline
        assert_eq!(paragraph_range_at(&rope, 8), 6..17);
        assert_eq!(paragraph_range_at(&rope, 18), 18..18); // empty line
        assert_eq!(paragraph_range_at(&rope, 21), 19..25);
        assert_eq!(paragraph_range_at(&rope, 99), 19..25);
    }

    #[test]
    fn graphemes_across_rope_chunks() {
        // Large enough that ropey splits into multiple chunks; ZWJ emoji
        // sequences throughout force the PreContext path at chunk seams.
        let unit = "word \u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467} e\u{0301} ";
        let text = unit.repeat(200);
        let rope = Rope::from_str(&text);
        assert!(rope.chunks().count() > 1, "test needs a multi-chunk rope");
        let mut expected = vec![0usize];
        expected.extend(text.grapheme_indices(true).skip(1).map(|(i, _)| i));
        expected.push(text.len());
        // Forward walk matches the str-based segmentation exactly.
        let mut walked = vec![0usize];
        let mut pos = 0;
        while pos < text.len() {
            pos = next_grapheme_boundary(&rope, pos);
            walked.push(pos);
        }
        assert_eq!(walked, expected);
        // Backward walk visits the same boundaries.
        let mut back = vec![text.len()];
        let mut pos = text.len();
        while pos > 0 {
            pos = prev_grapheme_boundary(&rope, pos);
            back.push(pos);
        }
        back.reverse();
        assert_eq!(back, expected);
    }

    #[test]
    fn clamp_snaps_to_char_boundary() {
        let rope = Rope::from_str("aé"); // 'é' = 2 bytes at offset 1..3
        assert_eq!(clamp(&rope, 2), 1);
        assert_eq!(clamp(&rope, 3), 3);
        assert_eq!(clamp(&rope, 1000), 3);
    }
}
