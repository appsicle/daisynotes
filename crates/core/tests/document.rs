//! Behavioral tests for `daisynotes_core::Document` through the public API:
//! titles/previews, toggle semantics, undo grouping, anchors, navigation,
//! and the golden serialization format.
#![allow(clippy::unwrap_used)]

use std::ops::Range;

use daisynotes_core::{Document, EntryId, FontFamily, Ink, InlineStyle, StyleToggle, Voice};

fn doc() -> Document {
    Document::new(EntryId::new())
}

fn doc_with(text: &str) -> Document {
    let mut d = doc();
    d.insert(0, text);
    d.break_undo_group();
    d
}

fn bold() -> InlineStyle {
    InlineStyle {
        bold: true,
        ..InlineStyle::default()
    }
}

fn spans_of(d: &Document) -> Vec<(Range<usize>, InlineStyle)> {
    d.spans().iter().collect()
}

// ── Title & preview ─────────────────────────────────────────────────────────

#[test]
fn title_is_first_nonempty_line_trimmed() {
    assert_eq!(doc().title(), "Untitled");
    assert_eq!(doc_with("   \n\n  Dear June  \nbody").title(), "Dear June");
    assert_eq!(doc_with("Only line").title(), "Only line");
    assert_eq!(doc_with(" \n \n ").title(), "Untitled");
}

#[test]
fn preview_collapses_whitespace_after_title() {
    assert_eq!(doc().preview(), "");
    assert_eq!(doc_with("Title only").preview(), "");
    let d = doc_with("Title\n  first   line\n\n\tsecond line  ");
    assert_eq!(d.preview(), "first line second line");
    // Blank lines before the title don't count as body.
    let d = doc_with("\n\nTitle\nbody words");
    assert_eq!(d.preview(), "body words");
}

#[test]
fn preview_caps_at_120_chars() {
    let body = "word ".repeat(60);
    let d = doc_with(&format!("Title\n{body}"));
    let p = d.preview();
    assert!(p.chars().count() <= 120, "len = {}", p.chars().count());
    assert!(p.starts_with("word word"));
}

// ── Editing basics ──────────────────────────────────────────────────────────

#[test]
fn insert_delete_replace_slice() {
    let mut d = doc();
    assert!(d.is_empty());
    d.insert(0, "hello world");
    assert_eq!(d.len(), 11);
    assert!(!d.is_empty());
    d.delete(5..11);
    assert_eq!(d.plain_text(), "hello");
    d.replace(0..5, "goodbye");
    assert_eq!(d.plain_text(), "goodbye");
    assert_eq!(d.slice(0..4), "good");
    // Out-of-bounds ranges clamp instead of panicking.
    assert_eq!(d.slice(4..999), "bye");
    d.delete(900..999);
    assert_eq!(d.plain_text(), "goodbye");
}

#[test]
fn version_bumps_on_every_mutation() {
    let mut d = doc();
    let v0 = d.version();
    d.insert(0, "ab");
    let v1 = d.version();
    assert!(v1 > v0);
    d.toggle_style(0..2, StyleToggle::Bold);
    let v2 = d.version();
    assert!(v2 > v1);
    d.set_voice(Voice {
        family: FontFamily::Inter,
        size: 18.0,
        weight: 500,
    });
    let v3 = d.version();
    assert!(v3 > v2);
    d.undo();
    assert!(d.version() > v3);
    // No-ops do not bump.
    let v = d.version();
    d.insert(0, "");
    d.delete(1..1);
    assert_eq!(d.version(), v);
}

#[test]
fn style_for_insertion_continues_the_previous_char() {
    let mut d = doc_with("plain bold");
    d.toggle_style(6..10, StyleToggle::Bold);
    assert!(d.style_for_insertion(0).is_plain());
    assert!(d.style_for_insertion(6).is_plain()); // before the bold run
    assert_eq!(d.style_for_insertion(7), bold()); // inside it
    assert_eq!(d.style_for_insertion(10), bold()); // right after it
    // Typing at the end of a bold run continues bold.
    d.insert(10, "er");
    assert_eq!(spans_of(&d), vec![(6..12, bold())]);
}

// ── toggle_style semantics ──────────────────────────────────────────────────

#[test]
fn toggle_applies_when_range_is_not_entirely_styled() {
    let mut d = doc_with("abcdef");
    d.toggle_style(0..3, StyleToggle::Bold);
    assert_eq!(spans_of(&d), vec![(0..3, bold())]);
    // Mixed range: applies to all of it.
    d.toggle_style(0..6, StyleToggle::Bold);
    assert_eq!(spans_of(&d), vec![(0..6, bold())]);
    // Entirely bold now: removes.
    d.toggle_style(0..6, StyleToggle::Bold);
    assert!(d.spans().is_plain());
}

#[test]
fn toggle_preserves_other_attributes() {
    let mut d = doc_with("abcdef");
    d.toggle_style(0..6, StyleToggle::Italic);
    d.toggle_style(2..4, StyleToggle::Bold);
    let both = InlineStyle {
        bold: true,
        italic: true,
        ..InlineStyle::default()
    };
    let italic = InlineStyle {
        italic: true,
        ..InlineStyle::default()
    };
    assert_eq!(
        spans_of(&d),
        vec![(0..2, italic), (2..4, both), (4..6, italic)]
    );
    // Removing italic across the whole range keeps the bold island.
    d.toggle_style(0..6, StyleToggle::Italic);
    assert_eq!(spans_of(&d), vec![(2..4, bold())]);
}

#[test]
fn ink_sets_and_clears() {
    let mut d = doc_with("abcdef");
    d.toggle_style(0..4, StyleToggle::Ink(Some(Ink::Rose)));
    assert_eq!(d.spans().style_at(0).ink, Some(Ink::Rose));
    // Painting a different ink over part of the range replaces it there.
    d.toggle_style(2..6, StyleToggle::Ink(Some(Ink::Moss)));
    assert_eq!(d.spans().style_at(1).ink, Some(Ink::Rose));
    assert_eq!(d.spans().style_at(2).ink, Some(Ink::Moss));
    assert_eq!(d.spans().style_at(5).ink, Some(Ink::Moss));
    // Ink(None) clears ink but keeps other attributes.
    d.toggle_style(0..6, StyleToggle::Bold);
    d.toggle_style(0..6, StyleToggle::Ink(None));
    assert_eq!(spans_of(&d), vec![(0..6, bold())]);
}

#[test]
fn runs_in_tiles_plain_gaps() {
    let mut d = doc_with("abcdef");
    d.toggle_style(2..4, StyleToggle::Bold);
    assert_eq!(
        d.spans().runs_in(0..6),
        vec![
            (0..2, InlineStyle::default()),
            (2..4, bold()),
            (4..6, InlineStyle::default()),
        ]
    );
    assert_eq!(d.spans().runs_in(3..3), vec![]);
}

// ── Undo / redo ─────────────────────────────────────────────────────────────

#[test]
fn typing_run_undoes_as_one_group() {
    let mut d = doc();
    for (i, ch) in ["h", "e", "y"].iter().enumerate() {
        d.insert(i, ch);
    }
    assert_eq!(d.plain_text(), "hey");
    let outcome = d.undo().unwrap();
    assert_eq!(d.plain_text(), "");
    assert_eq!(outcome.caret, 0..0);
    assert!(!d.can_undo());
    let outcome = d.redo().unwrap();
    assert_eq!(d.plain_text(), "hey");
    assert_eq!(outcome.caret, 3..3);
}

#[test]
fn break_undo_group_splits_typing_runs() {
    let mut d = doc();
    d.insert(0, "one");
    d.break_undo_group();
    d.insert(3, " two");
    d.undo();
    assert_eq!(d.plain_text(), "one");
    d.undo();
    assert_eq!(d.plain_text(), "");
}

#[test]
fn non_adjacent_insert_breaks_the_group() {
    let mut d = doc();
    d.insert(0, "ab");
    d.insert(0, "x"); // not at the caret (offset 2): new group
    assert_eq!(d.plain_text(), "xab");
    d.undo();
    assert_eq!(d.plain_text(), "ab");
    d.undo();
    assert_eq!(d.plain_text(), "");
}

#[test]
fn backspace_run_undoes_as_one_group() {
    let mut d = doc_with("hey👩‍👩‍👧!");
    let mut caret = d.len();
    // Backspace one grapheme at a time, all the way down.
    while caret > 0 {
        let prev = d.prev_grapheme(caret);
        d.delete(prev..caret);
        caret = prev;
    }
    assert_eq!(d.plain_text(), "");
    let outcome = d.undo().unwrap();
    assert_eq!(d.plain_text(), "hey👩‍👩‍👧!");
    // The whole restored run is reselected (macOS undo behavior).
    assert_eq!(outcome.caret, 0..d.len());
}

#[test]
fn forward_or_multichar_deletes_do_not_merge() {
    let mut d = doc_with("abcdef");
    d.delete(0..1); // forward delete: "a"
    d.delete(0..1); // forward delete: "b" — does not merge (not a backspace run)
    d.undo();
    assert_eq!(d.plain_text(), "bcdef");
    d.undo();
    assert_eq!(d.plain_text(), "abcdef");
}

#[test]
fn undo_restores_deleted_styles_byte_exactly() {
    let mut d = doc_with("abcdef");
    d.toggle_style(1..3, StyleToggle::Bold);
    d.toggle_style(4..6, StyleToggle::Ink(Some(Ink::Lavender)));
    let before_spans = spans_of(&d);
    let before_text = d.plain_text();
    d.break_undo_group();
    d.delete(0..6);
    assert!(d.spans().is_plain());
    d.undo();
    assert_eq!(d.plain_text(), before_text);
    assert_eq!(spans_of(&d), before_spans);
}

#[test]
fn replace_is_one_group_and_restores_selection() {
    let mut d = doc_with("hello world");
    d.replace(0..5, "goodbye");
    assert_eq!(d.plain_text(), "goodbye world");
    let outcome = d.undo().unwrap();
    assert_eq!(d.plain_text(), "hello world");
    assert_eq!(outcome.caret, 0..5); // the replaced selection
    let outcome = d.redo().unwrap();
    assert_eq!(d.plain_text(), "goodbye world");
    assert_eq!(outcome.caret, 7..7); // caret after the inserted text
}

#[test]
fn new_edit_clears_redo() {
    let mut d = doc_with("abc");
    d.delete(0..1);
    d.undo();
    assert!(d.can_redo());
    d.insert(0, "z");
    assert!(!d.can_redo());
}

#[test]
fn voice_change_is_undoable() {
    let mut d = doc();
    let loud = Voice {
        family: FontFamily::Mono,
        size: 24.0,
        weight: 700,
    };
    d.set_voice(loud);
    assert_eq!(d.voice(), loud);
    d.undo();
    assert_eq!(d.voice(), Voice::default());
    d.redo();
    assert_eq!(d.voice(), loud);
    // Setting the same voice again records nothing.
    let v = d.version();
    d.set_voice(loud);
    assert_eq!(d.version(), v);
}

// ── Anchors ─────────────────────────────────────────────────────────────────

#[test]
fn anchors_track_through_edits() {
    let mut d = doc_with("The quick brown fox");
    let id = d.anchor(4..9); // "quick"
    d.insert(0, ">> "); // before: shifts
    assert_eq!(d.anchor_range(id), Some(7..12));
    d.insert(9, "ish"); // strictly inside: extends
    assert_eq!(d.anchor_range(id), Some(7..15));
    d.insert(15, "!"); // at end: not inside
    assert_eq!(d.anchor_range(id), Some(7..15));
    d.delete(0..3); // before: shifts back
    assert_eq!(d.anchor_range(id), Some(4..12));
    d.delete(2..6); // partial overlap: clamps
    assert_eq!(d.anchor_range(id), Some(2..8));
}

#[test]
fn delete_covering_anchor_destroys_it() {
    let mut d = doc_with("a passage to pin");
    let id = d.anchor(2..9);
    d.delete(0..12);
    assert_eq!(d.anchor_range(id), None);
}

#[test]
fn release_anchor_forgets_it() {
    let mut d = doc_with("text");
    let id = d.anchor(0..4);
    assert_eq!(d.anchor_range(id), Some(0..4));
    d.release_anchor(id);
    assert_eq!(d.anchor_range(id), None);
}

// ── Navigation ──────────────────────────────────────────────────────────────

#[test]
fn grapheme_navigation_over_emoji() {
    let d = doc_with("a👩‍👩‍👧b");
    assert_eq!(d.next_grapheme(0), 1);
    assert_eq!(d.next_grapheme(1), 19); // the whole ZWJ family
    assert_eq!(d.prev_grapheme(19), 1);
    assert_eq!(d.prev_grapheme(20), 19);
    assert_eq!(d.clamp(3), 1); // snaps into the cluster's first char
}

#[test]
fn word_and_paragraph_navigation() {
    let d = doc_with("hello world\nnext line");
    assert_eq!(d.next_word(0), 5);
    assert_eq!(d.prev_word(11), 6);
    assert_eq!(d.word_range_at(8), 6..11);
    assert_eq!(d.paragraph_range_at(8), 0..11);
    assert_eq!(d.paragraph_range_at(14), 12..21);
}

// ── Serialization ───────────────────────────────────────────────────────────

#[test]
fn golden_empty_document() {
    let d = doc();
    assert_eq!(
        d.to_json(),
        r#"{"v":1,"voice":{"family":"literata","size":16.0,"weight":400},"text":"","spans":[]}"#
    );
}

#[test]
fn golden_styled_document() {
    let mut d = doc_with("Hello world");
    d.toggle_style(0..5, StyleToggle::Bold);
    d.toggle_style(6..11, StyleToggle::Ink(Some(Ink::Rose)));
    d.set_voice(Voice {
        family: FontFamily::Quattro,
        size: 18.0,
        weight: 500,
    });
    assert_eq!(
        d.to_json(),
        r#"{"v":1,"voice":{"family":"quattro","size":18.0,"weight":500},"text":"Hello world","spans":[{"start":0,"end":5,"bold":true},{"start":6,"end":11,"ink":"rose"}]}"#
    );
}

#[test]
fn from_json_round_trips() {
    let mut d = doc_with("Styled text here");
    d.toggle_style(0..6, StyleToggle::Bold);
    d.toggle_style(3..10, StyleToggle::Italic);
    d.set_voice(Voice {
        family: FontFamily::Inter,
        size: 14.0,
        weight: 300,
    });
    let json = d.to_json();
    let d2 = Document::from_json(d.id(), &json).unwrap();
    assert_eq!(d2.id(), d.id());
    assert_eq!(d2.plain_text(), d.plain_text());
    assert_eq!(spans_of(&d2), spans_of(&d));
    assert_eq!(d2.voice(), d.voice());
    assert_eq!(d2.version(), 0);
    assert!(!d2.can_undo());
}

#[test]
fn from_json_rejects_bad_input() {
    let id = EntryId::new();
    assert!(Document::from_json(id, "not json").is_err());
    let newer =
        r#"{"v":2,"voice":{"family":"literata","size":16.0,"weight":400},"text":"","spans":[]}"#;
    assert!(matches!(
        Document::from_json(id, newer),
        Err(daisynotes_core::DocError::UnsupportedVersion(2))
    ));
    // Span beyond the text.
    let bad_span = r#"{"v":1,"voice":{"family":"literata","size":16.0,"weight":400},"text":"ab","spans":[{"start":0,"end":5,"bold":true}]}"#;
    assert!(matches!(
        Document::from_json(id, bad_span),
        Err(daisynotes_core::DocError::InvalidSpan { .. })
    ));
    // Span off a char boundary ('é' is two bytes).
    let misaligned = r#"{"v":1,"voice":{"family":"literata","size":16.0,"weight":400},"text":"é","spans":[{"start":0,"end":1,"bold":true}]}"#;
    assert!(matches!(
        Document::from_json(id, misaligned),
        Err(daisynotes_core::DocError::InvalidSpan { .. })
    ));
}
