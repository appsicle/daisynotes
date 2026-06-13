//! Property tests over the public `Document` API: span-set invariants under
//! random edit sequences, byte-exact undo/redo round-trips, anchor transform
//! rules against a reference model, serialization round-trips, and grapheme
//! walk symmetry.
#![allow(clippy::unwrap_used)]

use std::ops::Range;

use daisynotes_core::{Document, EntryId, FontFamily, Ink, InlineStyle, SIZE_STEPS, StyleToggle, Voice};
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────────────

/// Text fragments mixing ASCII, multibyte chars, ZWJ emoji, combining marks,
/// and newlines — every UTF-8 hazard the model must survive.
fn arb_fragment() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "a",
        "Hello ",
        "x y\nz",
        "é",
        "👩‍👩‍👧",
        "🦀",
        "e\u{301}",
        "\n",
        "日本語",
        " ",
    ])
}

fn arb_toggle() -> impl Strategy<Value = StyleToggle> {
    prop_oneof![
        Just(StyleToggle::Bold),
        Just(StyleToggle::Italic),
        Just(StyleToggle::Underline),
        Just(StyleToggle::Strike),
        Just(StyleToggle::Ink(None)),
        Just(StyleToggle::Ink(Some(Ink::Rose))),
        Just(StyleToggle::Ink(Some(Ink::Lavender))),
        Just(StyleToggle::Ink(Some(Ink::Moss))),
    ]
}

fn arb_voice() -> impl Strategy<Value = Voice> {
    (
        prop_oneof![
            Just(FontFamily::Literata),
            Just(FontFamily::Inter),
            Just(FontFamily::Quattro),
            Just(FontFamily::Mono),
        ],
        0..SIZE_STEPS.len(),
        300u16..=700,
    )
        .prop_map(|(family, size_idx, weight)| Voice {
            family,
            size: SIZE_STEPS[size_idx],
            weight,
        })
}

#[derive(Debug, Clone)]
enum EditAction {
    Insert {
        at: usize,
        text: &'static str,
    },
    Delete {
        a: usize,
        b: usize,
    },
    Replace {
        a: usize,
        b: usize,
        text: &'static str,
    },
    Toggle {
        a: usize,
        b: usize,
        toggle: StyleToggle,
    },
    SetVoice(Voice),
    Break,
}

fn arb_action() -> impl Strategy<Value = EditAction> {
    // Offsets are intentionally unclamped/unaligned; the document must clamp.
    let pos = 0..256usize;
    prop_oneof![
        3 => (pos.clone(), arb_fragment()).prop_map(|(at, text)| EditAction::Insert { at, text }),
        2 => (pos.clone(), pos.clone()).prop_map(|(a, b)| EditAction::Delete { a, b }),
        2 => (pos.clone(), pos.clone(), arb_fragment())
            .prop_map(|(a, b, text)| EditAction::Replace { a, b, text }),
        2 => (pos.clone(), pos.clone(), arb_toggle())
            .prop_map(|(a, b, toggle)| EditAction::Toggle { a, b, toggle }),
        1 => arb_voice().prop_map(EditAction::SetVoice),
        1 => Just(EditAction::Break),
    ]
}

fn apply(doc: &mut Document, action: &EditAction) {
    match action {
        EditAction::Insert { at, text } => doc.insert(*at, text),
        EditAction::Delete { a, b } => doc.delete(*a.min(b)..*a.max(b)),
        EditAction::Replace { a, b, text } => doc.replace(*a.min(b)..*a.max(b), text),
        EditAction::Toggle { a, b, toggle } => doc.toggle_style(*a.min(b)..*a.max(b), *toggle),
        EditAction::SetVoice(voice) => doc.set_voice(*voice),
        EditAction::Break => doc.break_undo_group(),
    }
}

// ── Checks ──────────────────────────────────────────────────────────────────

/// Asserts every SpanSet invariant observable through the public API.
fn check_span_invariants(doc: &Document) {
    let text = doc.plain_text();
    let mut prev_end = 0usize;
    let mut prev_style: Option<InlineStyle> = None;
    for (range, style) in doc.spans().iter() {
        assert!(range.start < range.end, "empty run {range:?}");
        assert!(range.end <= doc.len(), "run {range:?} out of bounds");
        assert!(!style.is_plain(), "plain run stored at {range:?}");
        assert!(range.start >= prev_end, "unsorted/overlapping at {range:?}");
        if range.start == prev_end {
            assert_ne!(prev_style, Some(style), "uncoalesced runs at {range:?}");
        }
        assert!(text.is_char_boundary(range.start), "misaligned {range:?}");
        assert!(text.is_char_boundary(range.end), "misaligned {range:?}");
        prev_end = range.end;
        prev_style = Some(style);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    text: String,
    spans: Vec<(Range<usize>, InlineStyle)>,
    voice: Voice,
}

fn snapshot(doc: &Document) -> Snapshot {
    Snapshot {
        text: doc.plain_text(),
        spans: doc.spans().iter().collect(),
        voice: doc.voice(),
    }
}

// ── Reference model for anchor transforms ───────────────────────────────────

/// The anchor transform rules, restated independently of the implementation.
fn model_insert(anchor: &mut Option<Range<usize>>, at: usize, len: usize) {
    if let Some(range) = anchor {
        if at <= range.start {
            range.start += len;
            range.end += len;
        } else if at < range.end {
            range.end += len;
        }
    }
}

fn model_delete(anchor: &mut Option<Range<usize>>, deleted: Range<usize>) {
    let len = deleted.end - deleted.start;
    if len == 0 {
        return;
    }
    let Some(range) = anchor else { return };
    if deleted.start <= range.start && range.end <= deleted.end {
        *anchor = None;
    } else if deleted.end <= range.start {
        range.start -= len;
        range.end -= len;
    } else if deleted.start < range.end {
        let start = range.start.min(deleted.start);
        let end = if range.end >= deleted.end {
            range.end - len
        } else {
            deleted.start
        };
        *range = start..end;
    }
}

// ── Properties ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Span invariants hold after every action in a random edit sequence.
    #[test]
    fn span_invariants_under_random_edits(
        actions in prop::collection::vec(arb_action(), 0..40),
    ) {
        let mut doc = Document::new(EntryId::new());
        for action in &actions {
            apply(&mut doc, action);
            check_span_invariants(&doc);
        }
        // runs_in tiles any window gap-free and in order.
        let len = doc.len();
        let tiles = doc.spans().runs_in(0..len);
        let mut cursor = 0usize;
        for (range, _) in &tiles {
            prop_assert_eq!(range.start, cursor);
            cursor = range.end;
        }
        if len > 0 {
            prop_assert_eq!(cursor, len);
        }
    }

    /// Undoing everything restores the initial state byte-exactly, and
    /// redoing everything restores the final state byte-exactly.
    #[test]
    fn undo_redo_round_trips_exactly(
        seed in prop::collection::vec(arb_fragment(), 0..4),
        actions in prop::collection::vec(arb_action(), 1..30),
    ) {
        let mut doc = Document::new(EntryId::new());
        let initial = snapshot(&doc);
        for fragment in &seed {
            let at = doc.len();
            doc.insert(at, fragment);
        }
        doc.break_undo_group();
        for action in &actions {
            apply(&mut doc, action);
        }
        let fin = snapshot(&doc);

        while doc.undo().is_some() {}
        prop_assert_eq!(snapshot(&doc), initial.clone());
        prop_assert!(!doc.can_undo());

        while doc.redo().is_some() {}
        prop_assert_eq!(snapshot(&doc), fin.clone());
        prop_assert!(!doc.can_redo());

        // A second full cycle still round-trips (stacks stay coherent).
        while doc.undo().is_some() {}
        prop_assert_eq!(snapshot(&doc), initial);
        while doc.redo().is_some() {}
        prop_assert_eq!(snapshot(&doc), fin);
    }

    /// Anchors transform exactly per the spec rules under random edits.
    #[test]
    fn anchors_follow_the_transform_rules(
        seed in prop::collection::vec(arb_fragment(), 1..6),
        anchor_bounds in (0..128usize, 0..128usize),
        actions in prop::collection::vec(arb_action(), 0..25),
    ) {
        let mut doc = Document::new(EntryId::new());
        for fragment in &seed {
            let at = doc.len();
            doc.insert(at, fragment);
        }
        let (a, b) = anchor_bounds;
        let raw = a.min(b)..a.max(b);
        let id = doc.anchor(raw.clone());
        let mut model = Some(doc.clamp(raw.start)..doc.clamp(raw.end));

        for action in &actions {
            // Mirror the document's clamping, then drive the model with the
            // primitive insert/delete decomposition each action performs.
            match action {
                EditAction::Insert { at, text } => {
                    if !text.is_empty() {
                        let at = doc.clamp(*at);
                        model_insert(&mut model, at, text.len());
                    }
                }
                EditAction::Delete { a, b } => {
                    let range = doc.clamp(*a.min(b))..doc.clamp(*a.max(b));
                    if !range.is_empty() {
                        model_delete(&mut model, range);
                    }
                }
                EditAction::Replace { a, b, text } => {
                    let range = doc.clamp(*a.min(b))..doc.clamp(*a.max(b));
                    if !(range.is_empty() && text.is_empty()) {
                        if !range.is_empty() {
                            model_delete(&mut model, range.clone());
                        }
                        if !text.is_empty() {
                            model_insert(&mut model, range.start, text.len());
                        }
                    }
                }
                EditAction::Toggle { .. } | EditAction::SetVoice(_) | EditAction::Break => {}
            }
            apply(&mut doc, action);
            prop_assert_eq!(doc.anchor_range(id), model.clone());
            if let Some(range) = &model {
                prop_assert!(range.end <= doc.len());
            }
        }
    }

    /// to_json → from_json reproduces text, spans, and voice exactly.
    #[test]
    fn serialization_round_trips(
        actions in prop::collection::vec(arb_action(), 0..30),
    ) {
        let mut doc = Document::new(EntryId::new());
        for action in &actions {
            apply(&mut doc, action);
        }
        let json = doc.to_json();
        let restored = Document::from_json(doc.id(), &json).unwrap();
        prop_assert_eq!(snapshot(&restored), snapshot(&doc));
        // Encoding is stable: a second round trip emits identical bytes.
        prop_assert_eq!(restored.to_json(), json);
    }

    /// Forward and backward grapheme walks visit the same boundaries, and
    /// every boundary is a valid char boundary.
    #[test]
    fn grapheme_walks_are_symmetric(
        fragments in prop::collection::vec(arb_fragment(), 0..12),
    ) {
        let text: String = fragments.concat();
        let mut doc = Document::new(EntryId::new());
        doc.insert(0, &text);

        let mut forward = vec![0usize];
        let mut at = 0;
        while at < doc.len() {
            let next = doc.next_grapheme(at);
            prop_assert!(next > at, "no progress at {at}");
            prop_assert!(text.is_char_boundary(next));
            forward.push(next);
            at = next;
        }
        let mut backward = vec![doc.len()];
        let mut at = doc.len();
        while at > 0 {
            let prev = doc.prev_grapheme(at);
            prop_assert!(prev < at, "no progress at {at}");
            prop_assert!(text.is_char_boundary(prev));
            backward.push(prev);
            at = prev;
        }
        backward.reverse();
        prop_assert_eq!(forward, backward);
    }

    /// Word and paragraph ranges are well-formed and contain their offset.
    #[test]
    fn word_and_paragraph_ranges_are_well_formed(
        fragments in prop::collection::vec(arb_fragment(), 0..10),
        offset in 0..256usize,
    ) {
        let text: String = fragments.concat();
        let mut doc = Document::new(EntryId::new());
        doc.insert(0, &text);

        let word = doc.word_range_at(offset);
        prop_assert!(word.start <= word.end);
        prop_assert!(word.end <= doc.len());
        prop_assert!(text.is_char_boundary(word.start));
        prop_assert!(text.is_char_boundary(word.end));

        let para = doc.paragraph_range_at(offset);
        prop_assert!(para.start <= para.end);
        prop_assert!(para.end <= doc.len());
        let clamped = doc.clamp(offset.min(doc.len()));
        if clamped < doc.len() {
            prop_assert!(para.start <= clamped);
        }
        // Paragraphs never contain a newline.
        prop_assert!(!doc.slice(para).contains('\n'));

        // Word motion always progresses or pins at the boundary.
        prop_assert!(doc.next_word(offset) >= doc.clamp(offset));
        prop_assert!(doc.prev_word(offset) <= doc.clamp(offset));
    }
}
