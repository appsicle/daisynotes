//! `Document` — one entry's words, voice, styles, history, and anchors.
//!
//! Every mutation flows through [`Document::apply_edit`], which keeps the
//! rope, the span set, and the anchors in lockstep and is the only place
//! state changes. Public editing methods wrap it with undo recording.

use std::ops::Range;

use ropey::Rope;

use crate::EntryId;
use crate::anchor::{AnchorId, AnchorMap};
use crate::history::{EditOp, History, Tiles};
use crate::images::ImageSet;
use crate::json::{DocDto, DocError};
use crate::nav;
use crate::paras::ParaList;
use crate::spans::SpanSet;
use crate::style::{ImageBlock, InlineStyle, ListAttr, StyleToggle, Voice, MAX_LIST_INDENT};

/// Where the selection should land after an undo or redo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    /// Byte range to select (collapsed range = caret position).
    pub caret: Range<usize>,
}

/// The document model for a single entry.
#[derive(Debug, Clone)]
pub struct Document {
    id: EntryId,
    rope: Rope,
    voice: Voice,
    spans: SpanSet,
    paras: ParaList,
    images: ImageSet,
    version: u64,
    history: History,
    anchors: AnchorMap,
}

impl Document {
    /// An empty document with the default voice.
    pub fn new(id: EntryId) -> Self {
        Self {
            id,
            rope: Rope::new(),
            voice: Voice::default(),
            spans: SpanSet::new(),
            paras: ParaList::new(),
            images: ImageSet::new(),
            version: 0,
            history: History::default(),
            anchors: AnchorMap::default(),
        }
    }

    /// The entry id this document belongs to.
    pub fn id(&self) -> EntryId {
        self.id
    }

    /// Monotonic counter; bumps on every mutation (including undo/redo).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Text length in bytes.
    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    /// True when the document has no text.
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// The underlying rope (read-only; mutate through editing methods).
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// The entry-level voice.
    pub fn voice(&self) -> Voice {
        self.voice
    }

    /// The range-level style runs.
    pub fn spans(&self) -> &SpanSet {
        &self.spans
    }

    /// The text within `range` (clamped) as an owned string.
    pub fn slice(&self, range: Range<usize>) -> String {
        let range = self.clamp_range(range);
        self.rope.byte_slice(range).to_string()
    }

    /// The whole text as an owned string.
    pub fn plain_text(&self) -> String {
        self.rope.to_string()
    }

    /// First non-empty line, trimmed; `"Untitled"` when there is none.
    pub fn title(&self) -> String {
        match self.title_line() {
            Some(idx) => self.rope.line(idx).to_string().trim().to_string(),
            None => "Untitled".to_string(),
        }
    }

    /// Text after the title line, whitespace-collapsed, at most 120 chars.
    pub fn preview(&self) -> String {
        const MAX_CHARS: usize = 120;
        let Some(title_idx) = self.title_line() else {
            return String::new();
        };
        if title_idx + 1 >= self.rope.len_lines() {
            return String::new();
        }
        let start = self.rope.line_to_byte(title_idx + 1);
        let mut out = String::new();
        let mut count = 0usize;
        let mut pending_space = false;
        for ch in self.rope.byte_slice(start..self.len()).chars() {
            if ch.is_whitespace() {
                pending_space = !out.is_empty();
                continue;
            }
            if pending_space {
                if count + 1 >= MAX_CHARS {
                    break;
                }
                out.push(' ');
                count += 1;
                pending_space = false;
            }
            out.push(ch);
            count += 1;
            if count >= MAX_CHARS {
                break;
            }
        }
        out
    }

    // ── Editing ────────────────────────────────────────────────────────────

    /// Inserts `text` at byte offset `at` (clamped). The inserted range takes
    /// [`Document::style_for_insertion`] so typing continues the active style.
    pub fn insert(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = self.clamp(at);
        let style = self.style_for_insertion(at);
        let op = EditOp::Insert {
            at,
            text: text.to_string(),
            tiles: vec![(at..at + text.len(), style)],
        };
        self.apply_edit(&op);
        self.history.record_insert(op, at, text.len());
        self.version += 1;
    }

    /// Deletes `range` (clamped), capturing the removed text and its styles
    /// so undo restores them byte-exactly.
    pub fn delete(&mut self, range: Range<usize>) {
        let range = self.clamp_range(range);
        if range.is_empty() {
            return;
        }
        let single_backward = nav::prev_grapheme_boundary(&self.rope, range.end) == range.start;
        let op = self.delete_op(range.clone());
        self.apply_edit(&op);
        self.history.record_delete(op, range, single_backward);
        self.version += 1;
    }

    /// Replaces `range` (clamped) with `text` as a single undo group.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let range = self.clamp_range(range);
        if range.is_empty() && text.is_empty() {
            return;
        }
        let mut ops = Vec::with_capacity(2);
        if !range.is_empty() {
            let op = self.delete_op(range.clone());
            self.apply_edit(&op);
            ops.push(op);
        }
        let mut caret_after = range.start..range.start;
        if !text.is_empty() {
            let at = range.start;
            let style = self.style_for_insertion(at);
            let op = EditOp::Insert {
                at,
                text: text.to_string(),
                tiles: vec![(at..at + text.len(), style)],
            };
            self.apply_edit(&op);
            ops.push(op);
            let end = at + text.len();
            caret_after = end..end;
        }
        self.history.record_other(ops, range, caret_after);
        self.version += 1;
    }

    /// Applies `toggle` to `range` (clamped).
    ///
    /// Bold/italic/underline/strike: removed when the **entire** range
    /// already has the attribute, applied to all of it otherwise.
    /// `Ink(Some(i))` paints the range; `Ink(None)` clears ink.
    pub fn toggle_style(&mut self, range: Range<usize>, toggle: StyleToggle) {
        let range = self.clamp_range(range);
        if range.is_empty() {
            return;
        }
        let before = self.spans.runs_in(range.clone());
        let after = toggled_tiles(&before, toggle);
        if after == before {
            return;
        }
        let op = EditOp::Restyle {
            range: range.clone(),
            before,
            after,
        };
        self.apply_edit(&op);
        self.history.record_other(vec![op], range.clone(), range);
        self.version += 1;
    }

    /// Sets the entry-level voice (undoable).
    pub fn set_voice(&mut self, voice: Voice) {
        if voice == self.voice {
            return;
        }
        let op = EditOp::Voice {
            before: self.voice,
            after: voice,
        };
        self.apply_edit(&op);
        self.history.record_other(vec![op], 0..0, 0..0);
        self.version += 1;
    }

    /// The style newly typed text takes at `at`: the style of the character
    /// before it (so bold continues while typing), plain at offset 0.
    pub fn style_for_insertion(&self, at: usize) -> InlineStyle {
        let at = self.clamp(at);
        if at == 0 {
            return InlineStyle::default();
        }
        let char_idx = self.rope.byte_to_char(at);
        let prev = self.rope.char_to_byte(char_idx - 1);
        self.spans.style_at(prev)
    }

    // ── Undo / redo ────────────────────────────────────────────────────────

    /// Undoes the most recent group. Returns where the selection should land.
    pub fn undo(&mut self) -> Option<UndoOutcome> {
        let group = self.history.pop_undo()?;
        for op in group.ops.iter().rev() {
            self.apply_edit(&op.inverted());
        }
        let caret = group.caret_before.clone();
        self.history.push_redo(group);
        self.version += 1;
        Some(UndoOutcome { caret })
    }

    /// Re-applies the most recently undone group.
    pub fn redo(&mut self) -> Option<UndoOutcome> {
        let group = self.history.pop_redo()?;
        for op in &group.ops {
            self.apply_edit(op);
        }
        let caret = group.caret_after.clone();
        self.history.push_undo_closed(group);
        self.version += 1;
        Some(UndoOutcome { caret })
    }

    /// Closes the open undo group (the editor calls this on pauses).
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// True when there is something to undo.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// True when there is something to redo.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    // ── Navigation ─────────────────────────────────────────────────────────

    /// Clamps to the text length and snaps down to a `char` boundary.
    pub fn clamp(&self, offset: usize) -> usize {
        nav::clamp(&self.rope, offset)
    }

    /// The next grapheme-cluster boundary after `offset`.
    pub fn next_grapheme(&self, offset: usize) -> usize {
        nav::next_grapheme_boundary(&self.rope, offset)
    }

    /// The previous grapheme-cluster boundary before `offset`.
    pub fn prev_grapheme(&self, offset: usize) -> usize {
        nav::prev_grapheme_boundary(&self.rope, offset)
    }

    /// End of the word at-or-after `offset` (Option+Right).
    pub fn next_word(&self, offset: usize) -> usize {
        nav::next_word_boundary(&self.rope, offset)
    }

    /// Start of the word at-or-before `offset` (Option+Left).
    pub fn prev_word(&self, offset: usize) -> usize {
        nav::prev_word_boundary(&self.rope, offset)
    }

    /// The word-bound segment containing `offset` (double-click).
    pub fn word_range_at(&self, offset: usize) -> Range<usize> {
        nav::word_range_at(&self.rope, offset)
    }

    /// The newline-delimited line containing `offset`, excluding its trailing
    /// line break (triple-click).
    pub fn paragraph_range_at(&self, offset: usize) -> Range<usize> {
        nav::paragraph_range_at(&self.rope, offset)
    }

    // ── Anchors ────────────────────────────────────────────────────────────

    /// Registers `range` (clamped) as an anchor that tracks through edits.
    pub fn anchor(&mut self, range: Range<usize>) -> AnchorId {
        let range = self.clamp_range(range);
        self.anchors.register(range)
    }

    /// The anchor's current range, or `None` once destroyed or released.
    pub fn anchor_range(&self, id: AnchorId) -> Option<Range<usize>> {
        self.anchors.get(id)
    }

    /// Forgets an anchor.
    pub fn release_anchor(&mut self, id: AnchorId) {
        self.anchors.release(id);
    }

    // ── List paragraphs ────────────────────────────────────────────────────

    /// The list attribute of the paragraph that *starts* at `para_start`, if
    /// any. `para_start` should be a line start (see [`Document::paragraph_range_at`]).
    pub fn para_attr(&self, para_start: usize) -> Option<ListAttr> {
        self.paras.get(para_start)
    }

    /// The list attribute of the paragraph containing `offset`, if any.
    pub fn list_attr_at(&self, offset: usize) -> Option<ListAttr> {
        self.paras.get(self.paragraph_range_at(offset).start)
    }

    /// True when no paragraph is a list item.
    pub fn has_lists(&self) -> bool {
        !self.paras.is_empty()
    }

    /// Sets (or clears, with `None`) the list attribute of the paragraph
    /// starting at `para_start`. Undoable; a no-op when unchanged.
    pub fn set_para_list(&mut self, para_start: usize, attr: Option<ListAttr>) {
        let at = self.clamp(para_start);
        let before = self.paras.get(at);
        if before == attr {
            return;
        }
        let op = EditOp::SetList {
            at,
            before,
            after: attr,
        };
        self.apply_edit(&op);
        self.history.record_other(vec![op], at..at, at..at);
        self.version += 1;
    }

    /// Like [`Document::set_para_list`], but folds the attribute change into the
    /// open undo group so the `- `/`N. ` marker delete and the list attribute
    /// reverse together on one Cmd-Z (instead of leaving a bare empty line).
    pub fn set_para_list_grouped(&mut self, para_start: usize, attr: Option<ListAttr>) {
        let at = self.clamp(para_start);
        let before = self.paras.get(at);
        if before == attr {
            return;
        }
        let op = EditOp::SetList {
            at,
            before,
            after: attr,
        };
        self.apply_edit(&op);
        self.history.record_into_open(vec![op], at..at);
        self.version += 1;
    }

    // ── Image blocks ───────────────────────────────────────────────────────

    /// The image of the paragraph starting at `para_start`, if any.
    pub fn image_at(&self, para_start: usize) -> Option<ImageBlock> {
        self.images.get(para_start)
    }

    /// The image of the paragraph containing `offset`, if any.
    pub fn image_at_offset(&self, offset: usize) -> Option<ImageBlock> {
        self.images.get(self.paragraph_range_at(offset).start)
    }

    /// True when any paragraph holds an image.
    pub fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    /// Every image block in the document (for the app to load their blobs).
    pub fn image_blocks(&self) -> Vec<ImageBlock> {
        self.images.iter().map(|(_, block)| block).collect()
    }

    /// Sets (or clears) the image of the paragraph starting at `para_start`.
    /// Undoable; a no-op when unchanged. The caller is responsible for the
    /// paragraph itself being empty.
    pub fn set_image(&mut self, para_start: usize, block: Option<ImageBlock>) {
        let at = self.clamp(para_start);
        let before = self.images.get(at);
        if before == block {
            return;
        }
        let op = EditOp::SetImage {
            at,
            before,
            after: block,
        };
        self.apply_edit(&op);
        self.history.record_other(vec![op], at..at, at..at);
        self.version += 1;
    }

    /// Like [`Document::set_image`], but folds into the open undo group so a
    /// pasted image and the newline that created its line reverse together on
    /// one Cmd-Z (instead of leaving a stray blank line behind).
    pub fn set_image_grouped(&mut self, para_start: usize, block: Option<ImageBlock>) {
        let at = self.clamp(para_start);
        let before = self.images.get(at);
        if before == block {
            return;
        }
        let op = EditOp::SetImage {
            at,
            before,
            after: block,
        };
        self.apply_edit(&op);
        self.history.record_into_open(vec![op], at..at);
        self.version += 1;
    }

    /// Remove the image at `para_start` and its (empty) line, as one undoable
    /// group: a `SetImage` clearing the block, then a `Delete` of the line's
    /// trailing newline. Undo restores both byte-exactly.
    pub fn remove_image(&mut self, para_start: usize) {
        let at = self.clamp(para_start);
        let Some(block) = self.images.get(at) else {
            return;
        };
        let mut ops = Vec::with_capacity(2);
        let clear = EditOp::SetImage {
            at,
            before: Some(block),
            after: None,
        };
        self.apply_edit(&clear);
        ops.push(clear);
        // Drop the newline that forms the image's empty line, if present.
        // Use `byte` (not `byte_slice`) so a stray non-empty line never slices
        // a multi-byte char boundary and panics.
        if at < self.rope.len_bytes() && self.rope.byte(at) == b'\n' {
            let del = self.delete_op(at..at + 1);
            self.apply_edit(&del);
            ops.push(del);
        }
        self.history.record_other(ops, at..at, at..at);
        self.version += 1;
    }

    // ── Serialization ──────────────────────────────────────────────────────

    /// Serializes as versioned JSON: `{"v":1,"voice":…,"text":…,"spans":[…]}`
    /// plus an optional `paras` array (omitted when there are no lists).
    pub fn to_json(&self) -> String {
        DocDto::encode(
            self.voice,
            self.rope.to_string(),
            &self.spans,
            &self.paras,
            &self.images,
        )
    }

    /// Reads a document serialized by [`Document::to_json`]. History and
    /// anchors start fresh; `version` starts at 0.
    pub fn from_json(id: EntryId, json: &str) -> Result<Self, DocError> {
        let dto = DocDto::decode(json)?;
        let mut spans = SpanSet::new();
        for span in &dto.spans {
            let range = DocDto::validate_span(&dto.text, span.start, span.end)?;
            if !range.is_empty() && !span.style.is_plain() {
                spans.splice(range.clone(), &[(range, span.style)]);
            }
        }
        let mut paras = ParaList::new();
        for para in &dto.paras {
            if DocDto::valid_para(&dto.text, para.at) {
                paras.set(
                    para.at,
                    Some(ListAttr {
                        kind: para.kind,
                        // Clamp a corrupt or newer-version indent so it can't
                        // push the text column off-screen or grow the marker
                        // counter unbounded at render.
                        indent: para.indent.min(MAX_LIST_INDENT),
                    }),
                );
            }
        }
        let mut images = ImageSet::new();
        for image in &dto.images {
            if DocDto::valid_image(&dto.text, image.at) {
                images.set(
                    image.at,
                    Some(ImageBlock {
                        id: image.id,
                        w: image.w,
                        h: image.h,
                        width: image.width,
                    }),
                );
            }
        }
        Ok(Self {
            id,
            rope: Rope::from_str(&dto.text),
            voice: dto.voice,
            spans,
            paras,
            images,
            version: 0,
            history: History::default(),
            anchors: AnchorMap::default(),
        })
    }

    // ── Internals ──────────────────────────────────────────────────────────

    /// Builds a delete op for `range`, capturing text and styles for undo.
    fn delete_op(&self, range: Range<usize>) -> EditOp {
        EditOp::Delete {
            at: range.start,
            text: self.rope.byte_slice(range.clone()).to_string(),
            tiles: self.spans.runs_in(range),
        }
    }

    /// The single choke point: applies one op to rope + spans + anchors.
    fn apply_edit(&mut self, op: &EditOp) {
        match op {
            EditOp::Insert { at, text, tiles } => {
                let char_at = self.rope.byte_to_char(*at);
                self.rope.insert(char_at, text);
                self.spans.transform_insert(*at, text.len());
                // Splicing the recorded tiles (continuation style on user
                // inserts, captured tiles on undo-of-delete) is what makes
                // undo restore spans byte-exactly.
                self.spans.splice(*at..*at + text.len(), tiles);
                self.paras.transform_insert(*at, text.len());
                self.images.transform_insert(*at, text.len());
                self.anchors.transform_insert(*at, text.len());
            }
            EditOp::Delete { at, text, .. } => {
                let range = *at..*at + text.len();
                let char_start = self.rope.byte_to_char(range.start);
                let char_end = self.rope.byte_to_char(range.end);
                self.rope.remove(char_start..char_end);
                self.spans.transform_delete(range.clone());
                self.paras.transform_delete(range.clone());
                self.images.transform_delete(range.clone());
                self.anchors.transform_delete(range);
            }
            EditOp::Restyle { range, after, .. } => {
                self.spans.splice(range.clone(), after);
            }
            EditOp::Voice { after, .. } => {
                self.voice = *after;
            }
            EditOp::SetList { at, after, .. } => {
                self.paras.set(*at, *after);
            }
            EditOp::SetImage { at, after, .. } => {
                self.images.set(*at, *after);
            }
        }
    }

    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let a = self.clamp(range.start);
        let b = self.clamp(range.end);
        if a <= b { a..b } else { b..a }
    }

    /// Index of the first non-empty line, if any.
    fn title_line(&self) -> Option<usize> {
        (0..self.rope.len_lines())
            .find(|&idx| self.rope.line(idx).chars().any(|c| !c.is_whitespace()))
    }
}

/// Computes the post-toggle tiles for [`Document::toggle_style`].
fn toggled_tiles(before: &Tiles, toggle: StyleToggle) -> Tiles {
    let entire = |get: fn(&InlineStyle) -> bool| before.iter().all(|(_, s)| get(s));
    let map = |f: &dyn Fn(&mut InlineStyle)| {
        before
            .iter()
            .map(|(range, style)| {
                let mut style = *style;
                f(&mut style);
                (range.clone(), style)
            })
            .collect::<Tiles>()
    };
    match toggle {
        StyleToggle::Bold => {
            let on = !entire(|s| s.bold);
            map(&|s| s.bold = on)
        }
        StyleToggle::Italic => {
            let on = !entire(|s| s.italic);
            map(&|s| s.italic = on)
        }
        StyleToggle::Underline => {
            let on = !entire(|s| s.underline);
            map(&|s| s.underline = on)
        }
        StyleToggle::Strike => {
            let on = !entire(|s| s.strike);
            map(&|s| s.strike = on)
        }
        StyleToggle::Ink(ink) => map(&|s| s.ink = ink),
    }
}

#[cfg(test)]
mod undo_group_tests {
    use super::*;
    use crate::style::{ListAttr, ListKind};

    fn doc() -> Document {
        Document::new(ulid::Ulid::nil())
    }

    fn bullet() -> ListAttr {
        ListAttr {
            kind: ListKind::Bullet,
            indent: 0,
        }
    }

    #[test]
    fn list_trigger_undo_is_one_step() {
        let mut d = doc();
        d.insert(0, "- "); // the typed marker (its own undo group)
        d.break_undo_group();
        d.delete(0..2); // consume the marker — leaves the group open
        d.set_para_list_grouped(0, Some(bullet()));
        assert_eq!(d.para_attr(0), Some(bullet()));
        assert_eq!(d.rope().to_string(), "");
        // One undo restores the marker text AND clears the bullet together.
        d.undo();
        assert_eq!(d.rope().to_string(), "- ");
        assert_eq!(d.para_attr(0), None);
    }

    #[test]
    fn image_paste_undo_is_one_step() {
        let mut d = doc();
        d.break_undo_group();
        d.insert(0, "\n"); // the image's line — leaves the group open
        d.set_image_grouped(
            0,
            Some(ImageBlock {
                id: 7,
                w: 0,
                h: 0,
                width: 0,
            }),
        );
        assert!(d.image_at(0).is_some());
        assert_eq!(d.rope().to_string(), "\n");
        // One undo removes the image AND the line it created together.
        d.undo();
        assert_eq!(d.rope().to_string(), "");
        assert!(d.image_at(0).is_none());
    }
}
