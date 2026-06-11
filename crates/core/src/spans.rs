//! `SpanSet` — sorted, coalesced, non-overlapping runs of [`InlineStyle`].
//!
//! Invariants (held after every operation, property-tested):
//! - runs are sorted by `start` and non-overlapping,
//! - adjacent runs with equal styles are coalesced,
//! - every stored run is non-empty and non-plain,
//! - all runs lie within the text the set was last transformed against.

use std::ops::Range;

use crate::style::InlineStyle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) style: InlineStyle,
}

/// Style runs over the document text, keyed by global byte offsets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanSet {
    runs: Vec<Run>,
}

impl SpanSet {
    /// An empty set: every offset reads as plain.
    pub fn new() -> Self {
        Self::default()
    }

    /// The style in effect at `offset` (plain when no run covers it).
    pub fn style_at(&self, offset: usize) -> InlineStyle {
        let idx = self.runs.partition_point(|r| r.start <= offset);
        if idx > 0 && self.runs[idx - 1].end > offset {
            self.runs[idx - 1].style
        } else {
            InlineStyle::default()
        }
    }

    /// Tiles the whole `range` — including plain gaps — in order.
    /// Returns an empty vec for an empty range.
    pub fn runs_in(&self, range: Range<usize>) -> Vec<(Range<usize>, InlineStyle)> {
        let mut tiles = Vec::new();
        if range.start >= range.end {
            return tiles;
        }
        let mut cursor = range.start;
        for run in &self.runs {
            if run.end <= range.start {
                continue;
            }
            if run.start >= range.end {
                break;
            }
            let start = run.start.max(range.start);
            let end = run.end.min(range.end);
            if start > cursor {
                tiles.push((cursor..start, InlineStyle::default()));
            }
            tiles.push((start..end, run.style));
            cursor = end;
        }
        if cursor < range.end {
            tiles.push((cursor..range.end, InlineStyle::default()));
        }
        tiles
    }

    /// Iterates every stored (non-plain) run in order.
    pub fn iter(&self) -> impl Iterator<Item = (Range<usize>, InlineStyle)> + '_ {
        self.runs.iter().map(|r| (r.start..r.end, r.style))
    }

    /// True when no styled run is stored (all text is plain).
    pub fn is_plain(&self) -> bool {
        self.runs.is_empty()
    }

    /// Replaces the styles covering `range` with `tiles`.
    ///
    /// `tiles` must be sorted, non-overlapping, and contained in `range`;
    /// any part of `range` not covered by a tile becomes plain. Plain tiles
    /// are accepted and simply not stored.
    pub(crate) fn splice(&mut self, range: Range<usize>, tiles: &[(Range<usize>, InlineStyle)]) {
        if range.start >= range.end {
            return;
        }
        let mut next = Vec::with_capacity(self.runs.len() + tiles.len() + 2);
        // Keep everything before the range, clipping a run that straddles it.
        for run in &self.runs {
            if run.end <= range.start {
                next.push(run.clone());
            } else if run.start < range.start {
                next.push(Run {
                    start: run.start,
                    end: range.start,
                    style: run.style,
                });
            }
        }
        for (tile, style) in tiles {
            let start = tile.start.max(range.start);
            let end = tile.end.min(range.end);
            if start < end && !style.is_plain() {
                next.push(Run {
                    start,
                    end,
                    style: *style,
                });
            }
        }
        // Keep everything after the range, clipping a run that straddles it.
        for run in &self.runs {
            if run.start >= range.end {
                next.push(run.clone());
            } else if run.end > range.end {
                next.push(Run {
                    start: range.end,
                    end: run.end,
                    style: run.style,
                });
            }
        }
        self.runs = coalesced(next);
    }

    /// Transform for `len` bytes inserted at `at`: runs at/after `at` shift
    /// right; a run with `at` strictly inside extends.
    pub(crate) fn transform_insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for run in &mut self.runs {
            if run.start >= at {
                run.start += len;
                run.end += len;
            } else if run.end > at {
                run.end += len;
            }
        }
    }

    /// Transform for `range` deleted: later runs shift left; overlapping runs
    /// clamp; runs fully inside the deletion vanish.
    pub(crate) fn transform_delete(&mut self, range: Range<usize>) {
        let len = range.end.saturating_sub(range.start);
        if len == 0 {
            return;
        }
        let mut next = Vec::with_capacity(self.runs.len());
        for run in &self.runs {
            let (start, end) = if run.end <= range.start {
                (run.start, run.end)
            } else if run.start >= range.end {
                (run.start - len, run.end - len)
            } else {
                let start = run.start.min(range.start);
                let end = if run.end >= range.end {
                    run.end - len
                } else {
                    range.start
                };
                (start, end)
            };
            if start < end {
                next.push(Run {
                    start,
                    end,
                    style: run.style,
                });
            }
        }
        self.runs = coalesced(next);
    }

    /// Checks every invariant against a text of `text_len` bytes.
    #[cfg(test)]
    pub(crate) fn assert_invariants(&self, text_len: usize) {
        let mut prev_end = 0usize;
        let mut prev_style: Option<InlineStyle> = None;
        for run in &self.runs {
            assert!(run.start < run.end, "empty run {run:?}");
            assert!(run.end <= text_len, "run {run:?} exceeds len {text_len}");
            assert!(!run.style.is_plain(), "plain run stored: {run:?}");
            assert!(run.start >= prev_end, "overlap/disorder at {run:?}");
            if run.start == prev_end {
                assert!(
                    prev_style != Some(run.style),
                    "uncoalesced equal-adjacent runs at {run:?}"
                );
            }
            prev_end = run.end;
            prev_style = Some(run.style);
        }
    }
}

/// Sorts nothing (input is in order by construction) but drops empties and
/// merges touching runs with equal styles.
fn coalesced(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.start >= run.end {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.end == run.start
            && last.style == run.style
        {
            last.end = run.end;
            continue;
        }
        out.push(run);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Ink;

    fn bold() -> InlineStyle {
        InlineStyle {
            bold: true,
            ..InlineStyle::default()
        }
    }

    fn italic() -> InlineStyle {
        InlineStyle {
            italic: true,
            ..InlineStyle::default()
        }
    }

    #[test]
    fn style_at_and_gaps() {
        let mut s = SpanSet::new();
        s.splice(2..5, &[(2..5, bold())]);
        assert!(s.style_at(0).is_plain());
        assert_eq!(s.style_at(2), bold());
        assert_eq!(s.style_at(4), bold());
        assert!(s.style_at(5).is_plain());
        s.assert_invariants(10);
    }

    #[test]
    fn runs_in_tiles_whole_range() {
        let mut s = SpanSet::new();
        s.splice(2..4, &[(2..4, bold())]);
        s.splice(6..8, &[(6..8, italic())]);
        let tiles = s.runs_in(0..10);
        assert_eq!(
            tiles,
            vec![
                (0..2, InlineStyle::default()),
                (2..4, bold()),
                (4..6, InlineStyle::default()),
                (6..8, italic()),
                (8..10, InlineStyle::default()),
            ]
        );
        assert_eq!(s.runs_in(3..3), vec![]);
        assert_eq!(s.runs_in(3..7).len(), 3);
    }

    #[test]
    fn splice_overwrites_and_coalesces() {
        let mut s = SpanSet::new();
        s.splice(0..4, &[(0..4, bold())]);
        s.splice(4..8, &[(4..8, bold())]);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(0..8, bold())]);
        // Punch a plain hole in the middle.
        s.splice(2..6, &[(2..6, InlineStyle::default())]);
        assert_eq!(
            s.iter().collect::<Vec<_>>(),
            vec![(0..2, bold()), (6..8, bold())]
        );
        s.assert_invariants(8);
    }

    #[test]
    fn insert_shifts_and_extends() {
        let mut s = SpanSet::new();
        s.splice(2..6, &[(2..6, bold())]);
        // Strictly inside: extends.
        s.transform_insert(4, 3);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(2..9, bold())]);
        // At start: shifts.
        s.transform_insert(2, 1);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(3..10, bold())]);
        // At end: leaves the run alone.
        s.transform_insert(10, 5);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(3..10, bold())]);
        s.assert_invariants(100);
    }

    #[test]
    fn delete_clamps_shifts_and_rejoins() {
        let mut s = SpanSet::new();
        s.splice(0..2, &[(0..2, bold())]);
        s.splice(3..5, &[(3..5, bold())]);
        // Deleting the plain gap rejoins the two bold runs.
        s.transform_delete(2..3);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(0..4, bold())]);
        // Delete fully covering a run removes it.
        let mut t = SpanSet::new();
        t.splice(2..4, &[(2..4, italic())]);
        t.transform_delete(1..5);
        assert!(t.is_plain());
        // Partial overlap clamps.
        let mut u = SpanSet::new();
        u.splice(2..6, &[(2..6, bold())]);
        u.transform_delete(0..4);
        assert_eq!(u.iter().collect::<Vec<_>>(), vec![(0..2, bold())]);
    }

    mod props {
        use proptest::prelude::*;

        use super::*;

        fn arb_style() -> impl Strategy<Value = InlineStyle> {
            (
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
                prop_oneof![
                    Just(None),
                    Just(Some(Ink::Rose)),
                    Just(Some(Ink::Lavender)),
                    Just(Some(Ink::Moss)),
                ],
            )
                .prop_map(|(bold, italic, underline, strike, ink)| InlineStyle {
                    bold,
                    italic,
                    underline,
                    strike,
                    ink,
                })
        }

        #[derive(Debug, Clone)]
        enum SpanOp {
            Style {
                range: Range<usize>,
                style: InlineStyle,
            },
            Insert {
                at: usize,
                len: usize,
            },
            Delete {
                range: Range<usize>,
            },
        }

        fn arb_op(max: usize) -> impl Strategy<Value = SpanOp> {
            prop_oneof![
                ((0..=max, 0..=max), arb_style()).prop_map(|((a, b), style)| SpanOp::Style {
                    range: a.min(b)..a.max(b),
                    style,
                }),
                (0..=max, 1..=8usize).prop_map(|(at, len)| SpanOp::Insert { at, len }),
                (0..=max, 0..=max).prop_map(|(a, b)| SpanOp::Delete {
                    range: a.min(b)..a.max(b),
                }),
            ]
        }

        proptest! {
            /// Invariants hold after any sequence of splices and transforms,
            /// and `runs_in` always tiles a queried range contiguously.
            #[test]
            fn invariants_under_random_ops(
                ops in proptest::collection::vec(arb_op(64), 0..40),
            ) {
                let mut len = 64usize;
                let mut set = SpanSet::new();
                for op in ops {
                    match op {
                        SpanOp::Style { range, style } => {
                            let range = range.start.min(len)..range.end.min(len);
                            set.splice(range.clone(), &[(range, style)]);
                        }
                        SpanOp::Insert { at, len: n } => {
                            let at = at.min(len);
                            set.transform_insert(at, n);
                            len += n;
                        }
                        SpanOp::Delete { range } => {
                            let range = range.start.min(len)..range.end.min(len);
                            set.transform_delete(range.clone());
                            len -= range.end - range.start;
                        }
                    }
                    set.assert_invariants(len);
                    // runs_in tiles the whole queried range, in order, gap-free.
                    let tiles = set.runs_in(0..len);
                    let mut cursor = 0usize;
                    for (range, _) in &tiles {
                        prop_assert_eq!(range.start, cursor);
                        prop_assert!(range.end > range.start);
                        cursor = range.end;
                    }
                    if len > 0 {
                        prop_assert_eq!(cursor, len);
                    }
                    // style_at agrees with the tiling at every boundary.
                    for (range, style) in &tiles {
                        prop_assert_eq!(set.style_at(range.start), *style);
                    }
                }
            }
        }
    }

    #[test]
    fn ink_runs_with_distinct_inks_stay_separate() {
        let rose = InlineStyle {
            ink: Some(Ink::Rose),
            ..InlineStyle::default()
        };
        let moss = InlineStyle {
            ink: Some(Ink::Moss),
            ..InlineStyle::default()
        };
        let mut s = SpanSet::new();
        s.splice(0..2, &[(0..2, rose)]);
        s.splice(2..4, &[(2..4, moss)]);
        assert_eq!(s.iter().count(), 2);
        s.assert_invariants(4);
    }
}
