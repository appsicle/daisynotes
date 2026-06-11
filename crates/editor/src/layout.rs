//! Paragraph splitting, the per-paragraph shaped-line cache, and the layout
//! snapshot that maps byte offsets ⇄ pixels.
//!
//! PERFORMANCE INVARIANT (PLAN §5/§10): an edit re-shapes only the edited
//! paragraph(s). [`reuse_paragraphs`] aligns the old paragraph list against
//! the new text from both ends and carries shaped lines across for every
//! paragraph whose text is unchanged; the style signature check in the
//! element then re-shapes only paragraphs whose tiles/voice/width/palette
//! actually changed.

use std::ops::Range;

use gpui::{Bounds, Pixels, Point, ShapedLine, SharedString, WrappedLine, point, px};

/// Content line height as a multiple of the voice size (PLAN §8).
pub(crate) const LINE_HEIGHT_FACTOR: f32 = 1.65;

/// One rectangle of a selection highlight, in content coordinates. Rects for
/// a selection tile vertically with no gaps; only the outer silhouette is
/// rounded (`round_top` on the first rect, `round_bottom` on the last).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SelectionRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub round_top: bool,
    pub round_bottom: bool,
}

/// One newline-delimited paragraph. `range` covers the paragraph **including
/// its trailing newline byte** (the newline belongs to the paragraph it
/// ends); `text_end` is where the visible text stops (before the newline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParaSpan {
    pub range: Range<usize>,
    pub text_end: usize,
}

impl ParaSpan {
    /// The visible (shaped) byte range, excluding the trailing newline.
    pub fn visible(&self) -> Range<usize> {
        self.range.start..self.text_end
    }
}

/// Split `text` into paragraphs. Always yields at least one (possibly
/// empty) paragraph; a trailing newline produces a final empty paragraph,
/// which is exactly where the caret lives after typing Return at the end.
pub(crate) fn split_paragraphs(text: &str) -> Vec<ParaSpan> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (idx, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            spans.push(ParaSpan {
                range: start..idx + 1,
                text_end: idx,
            });
            start = idx + 1;
        }
    }
    spans.push(ParaSpan {
        range: start..text.len(),
        text_end: text.len(),
    });
    spans
}

/// One paragraph plus its cached shaping. `shaped` is `None` until the
/// element shapes it; `sig` records the inputs (style tiles, voice, wrap
/// width, palette, marked overlap) of the cached shape so style-only changes
/// invalidate without a text change.
pub(crate) struct ParaRec {
    pub span: ParaSpan,
    pub text: SharedString,
    pub sig: u64,
    pub shaped: Option<WrappedLine>,
    pub rows: usize,
}

impl ParaRec {
    fn fresh(span: ParaSpan, text: SharedString) -> Self {
        Self {
            span,
            text,
            sig: 0,
            shaped: None,
            rows: 1,
        }
    }
}

/// Rebuild the paragraph list for `text`, reusing shaped lines from `old`
/// for every paragraph whose visible text is unchanged. Matching runs from
/// the front and the back of the list, so a single-paragraph edit re-shapes
/// only that paragraph.
pub(crate) fn reuse_paragraphs(mut old: Vec<ParaRec>, text: &str) -> Vec<ParaRec> {
    let spans = split_paragraphs(text);
    let mut prefix = 0usize;
    while prefix < old.len()
        && prefix < spans.len()
        && old[prefix].text.as_ref() == &text[spans[prefix].visible()]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < old.len() - prefix
        && suffix < spans.len() - prefix
        && old[old.len() - 1 - suffix].text.as_ref()
            == &text[spans[spans.len() - 1 - suffix].visible()]
    {
        suffix += 1;
    }

    let total = spans.len();
    let tail = old.split_off(old.len() - suffix);
    old.truncate(prefix);
    let mut head_iter = old.into_iter();
    let mut tail_iter = tail.into_iter();

    let mut out = Vec::with_capacity(total);
    for (idx, span) in spans.into_iter().enumerate() {
        let reused = if idx < prefix {
            head_iter.next()
        } else if idx >= total - suffix {
            tail_iter.next()
        } else {
            None
        };
        match reused {
            Some(mut rec) => {
                rec.span = span;
                out.push(rec);
            }
            None => {
                let slice = &text[span.visible()];
                out.push(ParaRec::fresh(span, SharedString::from(slice.to_string())));
            }
        }
    }
    out
}

/// A paragraph as placed by the last layout pass. `y` is in content
/// coordinates (y = 0 at the very top of the scrolled content).
pub(crate) struct SnapPara {
    pub span: ParaSpan,
    pub y: f32,
    pub height: f32,
    pub line: Option<WrappedLine>,
}

/// A margin dot as placed by the last layout pass (content coordinates of
/// its center).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SnapDot {
    pub id: u64,
    pub center: (f32, f32),
}

/// The coda block as placed by the last layout pass.
pub(crate) struct CodaSnap {
    /// Content y of the hairline divider.
    pub divider_y: f32,
    /// Draw-in progress of the divider, eased, `0.0..=1.0`.
    pub divider_frac: f32,
    /// Content y of the first body line.
    pub body_y: f32,
    /// The coda's own line height (voice size − 2, × 1.65).
    pub line_height: f32,
    /// Shaped body lines (one per body paragraph), revealed so far.
    pub lines: Vec<WrappedLine>,
}

/// Everything the editor needs to map between bytes and pixels: built every
/// prepaint, read by mouse handlers, action handlers, and overlay
/// positioning (which reads the previous frame's snapshot).
pub(crate) struct Snapshot {
    /// Element bounds in window coordinates.
    pub bounds: Bounds<Pixels>,
    /// X of the column's left edge, relative to `bounds`.
    pub column_x: f32,
    pub wrap_width: f32,
    pub line_height: f32,
    /// The scroll offset this layout was computed with.
    pub scroll: f32,
    pub paras: Vec<SnapPara>,
    pub content_height: f32,
    pub dots: Vec<SnapDot>,
    /// The shaped date label and its content y (space is reserved whether
    /// or not a label is present).
    pub date: Option<(ShapedLine, f32)>,
    pub coda: Option<CodaSnap>,
}

impl Snapshot {
    /// Content coordinates → window coordinates.
    pub fn to_window(&self, content: (f32, f32)) -> Point<Pixels> {
        point(
            self.bounds.origin.x + px(self.column_x + content.0),
            self.bounds.origin.y + px(content.1 - self.scroll),
        )
    }

    /// Window coordinates → content coordinates.
    pub fn to_content(&self, window: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(window.x - self.bounds.origin.x) - self.column_x,
            f32::from(window.y - self.bounds.origin.y) + self.scroll,
        )
    }

    fn para_index_at_offset(&self, offset: usize) -> usize {
        // Last paragraph whose range starts at or before the offset.
        match self
            .paras
            .binary_search_by(|p| p.span.range.start.cmp(&offset))
        {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        }
    }

    /// Byte-index rows of a paragraph: `(start, end)` index pairs relative
    /// to the paragraph's visible text, one per wrapped visual row.
    fn rows(para: &SnapPara) -> Vec<(usize, usize)> {
        let Some(line) = &para.line else {
            // Unshaped paragraph: one row covering all visible text.
            return vec![(0, para.span.visible().len())];
        };
        let len = line.len();
        let mut rows = Vec::with_capacity(line.wrap_boundaries().len() + 1);
        let mut start = 0usize;
        for boundary in line.wrap_boundaries() {
            let glyph = &line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
            rows.push((start, glyph.index));
            start = glyph.index;
        }
        rows.push((start, len));
        rows
    }

    /// X position of `idx` within the row beginning at `row_start`, in
    /// content coordinates relative to the column's left edge.
    fn x_in_row(para: &SnapPara, row_start: usize, idx: usize) -> f32 {
        let Some(line) = &para.line else { return 0.0 };
        let layout = &line.unwrapped_layout;
        f32::from(layout.x_for_index(idx)) - f32::from(layout.x_for_index(row_start))
    }

    /// The caret position for a byte offset: content coordinates of the
    /// glyph's top-left on its visual row.
    pub fn caret_point(&self, offset: usize) -> (f32, f32) {
        if self.paras.is_empty() {
            return (0.0, 0.0);
        }
        let para = &self.paras[self.para_index_at_offset(offset)];
        let rel = offset
            .saturating_sub(para.span.range.start)
            .min(para.span.visible().len());
        let rows = Self::rows(para);
        let (row_idx, row) = Self::row_for_index(&rows, rel);
        let x = Self::x_in_row(para, row.0, rel);
        (x, para.y + row_idx as f32 * self.line_height)
    }

    /// The row containing `rel`: the first row whose end is past it (a caret
    /// exactly on a wrap boundary displays at the start of the next row).
    fn row_for_index(rows: &[(usize, usize)], rel: usize) -> (usize, (usize, usize)) {
        for (idx, row) in rows.iter().enumerate() {
            if rel < row.1 || idx == rows.len() - 1 {
                return (idx, *row);
            }
        }
        (0, rows[0])
    }

    /// The visual row (as a global byte range) containing `offset`; used by
    /// `MoveToLineStart/End` and `DeleteToLineStart`.
    pub fn visual_row_range(&self, offset: usize) -> Range<usize> {
        if self.paras.is_empty() {
            return 0..0;
        }
        let para = &self.paras[self.para_index_at_offset(offset)];
        let rel = offset
            .saturating_sub(para.span.range.start)
            .min(para.span.visible().len());
        let rows = Self::rows(para);
        let (_, row) = Self::row_for_index(&rows, rel);
        para.span.range.start + row.0..para.span.range.start + row.1
    }

    /// The byte offset nearest a content-coordinate point.
    pub fn offset_at(&self, content: (f32, f32)) -> usize {
        let (x, y) = content;
        let Some(first) = self.paras.first() else {
            return 0;
        };
        if y < first.y {
            return 0;
        }
        let Some(last) = self.paras.last() else {
            return 0;
        };
        if y >= last.y + last.height {
            return last.span.range.end;
        }
        // Binary search the paragraph whose vertical band contains y.
        let idx = self
            .paras
            .partition_point(|p| p.y + p.height <= y)
            .min(self.paras.len() - 1);
        let para = &self.paras[idx];
        let Some(line) = &para.line else {
            return para.span.range.start;
        };
        let rel_point = point(px(x.max(0.0)), px((y - para.y).max(0.0)));
        let rel = line
            .closest_index_for_position(rel_point, px(self.line_height))
            .unwrap_or_else(|nearest| nearest)
            .min(para.span.visible().len());
        para.span.range.start + rel
    }

    /// Selection rectangles (content coordinates) for a byte range: one per
    /// visual row, each spanning the FULL line box so consecutive rows touch
    /// with zero vertical gap (including across paragraph boundaries and
    /// empty paragraphs). When the selection continues past a row's end (a
    /// wrap or a selected newline) the rect extends a small tail past the
    /// last glyph; an empty selected line shows a small stub. Only the outer
    /// silhouette is rounded: top corners on the first rect, bottom corners
    /// on the last.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<SelectionRect> {
        let mut rects: Vec<SelectionRect> = Vec::new();
        if range.start >= range.end {
            return rects;
        }
        let em = self.line_height / LINE_HEIGHT_FACTOR;
        let stub = em * 0.4;
        let tail = em * 0.45;
        for para in &self.paras {
            if para.span.range.end <= range.start && para.span.range.end != para.span.range.start {
                continue;
            }
            if para.span.range.start >= range.end {
                break;
            }
            let visible = para.span.visible();
            let sel_start = range.start.max(visible.start);
            let sel_end = range.end.min(visible.end);
            let newline_selected = range.end > para.span.text_end
                && range.start <= para.span.text_end
                && para.span.text_end < para.span.range.end;

            let rows = Self::rows(para);
            for (row_idx, row) in rows.iter().enumerate() {
                let row_start = visible.start + row.0;
                let row_end = visible.start + row.1;
                let y = para.y + row_idx as f32 * self.line_height;
                let s = sel_start.max(row_start);
                let e = sel_end.min(row_end);
                let last_row = row_idx == rows.len() - 1;
                if s > e || (s == e && !(newline_selected && last_row)) {
                    continue;
                }
                let x1 = Self::x_in_row(para, row.0, s - visible.start);
                let mut x2 = Self::x_in_row(para, row.0, e - visible.start);
                // The selection continues past this row's end: a wrap on an
                // inner row, or the trailing newline on the last row.
                let continues = if last_row {
                    newline_selected
                } else {
                    range.end > row_end
                };
                if continues {
                    x2 += tail;
                }
                if x2 - x1 < stub && s == e {
                    x2 = x1 + stub;
                }
                if x2 - x1 < 1.0 {
                    x2 = x1 + 1.0;
                }
                rects.push(SelectionRect {
                    x: x1,
                    y,
                    w: x2 - x1,
                    h: self.line_height,
                    round_top: false,
                    round_bottom: false,
                });
            }
        }
        if let Some(first) = rects.first_mut() {
            first.round_top = true;
        }
        if let Some(last) = rects.last_mut() {
            last.round_bottom = true;
        }
        rects
    }

    /// Maximum scroll offset for a viewport height.
    pub fn max_scroll(&self, viewport_height: f32) -> f32 {
        (self.content_height - viewport_height).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[ParaSpan], text: &str) -> Vec<String> {
        spans
            .iter()
            .map(|s| text[s.visible()].to_string())
            .collect()
    }

    #[test]
    fn splits_with_trailing_newline_owned_by_its_paragraph() {
        let text = "first\nsecond\n";
        let spans = split_paragraphs(text);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].range, 0..6);
        assert_eq!(spans[0].text_end, 5);
        assert_eq!(spans[1].range, 6..13);
        assert_eq!(spans[1].text_end, 12);
        // The trailing newline leaves an empty final paragraph for the caret.
        assert_eq!(spans[2].range, 13..13);
        assert_eq!(spans[2].text_end, 13);
        assert_eq!(texts(&spans, text), ["first", "second", ""]);
    }

    #[test]
    fn splits_empty_and_blank_paragraphs() {
        assert_eq!(split_paragraphs("").len(), 1);
        let text = "a\n\nb";
        let spans = split_paragraphs(text);
        assert_eq!(texts(&spans, text), ["a", "", "b"]);
        assert_eq!(spans[1].range, 2..3);
        assert_eq!(spans[1].text_end, 2);
        // Ranges tile the text exactly.
        assert_eq!(spans.last().map(|s| s.range.end), Some(text.len()));
        for pair in spans.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
    }

    #[test]
    fn reuse_keeps_untouched_paragraphs() {
        let original = "alpha\nbeta\ngamma";
        let mut recs = reuse_paragraphs(Vec::new(), original);
        // Mark each as shaped with a distinctive signature.
        for (idx, rec) in recs.iter_mut().enumerate() {
            rec.sig = idx as u64 + 100;
        }
        // Edit the middle paragraph only.
        let edited = "alpha\nbeta!\ngamma";
        let recs = reuse_paragraphs(recs, edited);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].sig, 100, "untouched head must be reused");
        assert_eq!(recs[1].sig, 0, "edited paragraph must be fresh");
        assert_eq!(recs[2].sig, 102, "untouched tail must be reused");
        // Spans always reflect the new text.
        assert_eq!(recs[1].span.range, 6..12);
        assert_eq!(&edited[recs[1].span.visible()], "beta!");
    }

    #[test]
    fn reuse_handles_paragraph_insertion_and_removal() {
        let mut recs = reuse_paragraphs(Vec::new(), "one\ntwo");
        for rec in &mut recs {
            rec.sig = 7;
        }
        // Split "two" by inserting a newline: head reused, rest fresh or
        // matched from the back.
        let recs = reuse_paragraphs(recs, "one\nt\nwo");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].sig, 7);
        assert_eq!(recs[0].text.as_ref(), "one");
        assert_eq!(recs[1].text.as_ref(), "t");
        assert_eq!(recs[2].text.as_ref(), "wo");

        // Deleting back down re-merges; the head is still reused.
        let mut recs = recs;
        for rec in &mut recs {
            rec.sig = rec.sig.max(1);
        }
        let recs = reuse_paragraphs(recs, "one");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].text.as_ref(), "one");
    }

    /// A snapshot over `text` with one unshaped (single-row) paragraph per
    /// span and a line height of 16.5 (em = 10.0). Unshaped rows measure
    /// zero glyph width, which is enough to exercise the rect geometry.
    fn test_snapshot(text: &str) -> Snapshot {
        let line_height = 16.5;
        let paras = split_paragraphs(text)
            .into_iter()
            .enumerate()
            .map(|(idx, span)| SnapPara {
                span,
                y: idx as f32 * line_height,
                height: line_height,
                line: None,
            })
            .collect::<Vec<_>>();
        let content_height = paras.len() as f32 * line_height;
        Snapshot {
            bounds: Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0))),
            column_x: 0.0,
            wrap_width: 400.0,
            line_height,
            scroll: 0.0,
            paras,
            content_height,
            dots: Vec::new(),
            date: None,
            coda: None,
        }
    }

    #[test]
    fn selection_rects_tile_with_no_vertical_gaps() {
        let snap = test_snapshot("alpha\nbeta\ngamma");
        let rects = snap.selection_rects(0..16);
        assert_eq!(rects.len(), 3);
        for rect in &rects {
            assert!(
                (rect.h - snap.line_height).abs() < f32::EPSILON,
                "every rect spans the full line box"
            );
        }
        for pair in rects.windows(2) {
            assert!(
                (pair[0].y + pair[0].h - pair[1].y).abs() < f32::EPSILON,
                "consecutive rects must touch: {pair:?}"
            );
        }
    }

    #[test]
    fn selection_rects_include_empty_paragraph_stub() {
        let snap = test_snapshot("a\n\nb");
        let rects = snap.selection_rects(0..4);
        assert_eq!(rects.len(), 3, "the empty middle line gets a stub rect");
        let em = snap.line_height / LINE_HEIGHT_FACTOR;
        // The empty line's stub is a small fraction of an em, not a sliver.
        assert!(rects[1].w >= em * 0.35, "stub too narrow: {}", rects[1].w);
        assert!(rects[1].w <= em * 0.6, "stub too wide: {}", rects[1].w);
        // Still gap-free across the empty paragraph.
        for pair in rects.windows(2) {
            assert!((pair[0].y + pair[0].h - pair[1].y).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn selection_rects_add_tail_when_newline_is_selected() {
        let snap = test_snapshot("ab\ncd");
        // Selecting just within the first paragraph: no tail.
        let inner = snap.selection_rects(0..2);
        assert_eq!(inner.len(), 1);
        let no_tail_w = inner[0].w;
        // Selecting across the newline extends the first rect past the text.
        let across = snap.selection_rects(0..4);
        assert_eq!(across.len(), 2);
        let em = snap.line_height / LINE_HEIGHT_FACTOR;
        // Unshaped rows have zero glyph width, so the across-newline rect is
        // (nearly) exactly the tail; the within-line rect is just the
        // minimum sliver.
        assert!(no_tail_w <= 1.0 + f32::EPSILON, "no tail expected: {no_tail_w}");
        assert!(
            across[0].w >= em * 0.4,
            "newline tail missing: {}",
            across[0].w
        );
    }

    #[test]
    fn selection_rects_round_only_the_outer_silhouette() {
        let snap = test_snapshot("one\ntwo\nthree");
        let rects = snap.selection_rects(0..13);
        assert_eq!(rects.len(), 3);
        assert!(rects[0].round_top && !rects[0].round_bottom);
        assert!(!rects[1].round_top && !rects[1].round_bottom);
        assert!(!rects[2].round_top && rects[2].round_bottom);
        // A single-line selection rounds all four corners.
        let single = snap.selection_rects(0..2);
        assert_eq!(single.len(), 1);
        assert!(single[0].round_top && single[0].round_bottom);
        // Empty selection yields nothing.
        assert!(snap.selection_rects(3..3).is_empty());
    }

    #[test]
    fn reuse_of_identical_text_reuses_everything() {
        let text = "a\nb\nc";
        let mut recs = reuse_paragraphs(Vec::new(), text);
        for rec in &mut recs {
            rec.sig = 9;
        }
        let recs = reuse_paragraphs(recs, text);
        assert!(recs.iter().all(|r| r.sig == 9));
    }
}
