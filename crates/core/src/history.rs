//! Undo history: invertible edit ops, merge-aware groups, undo/redo stacks.
//!
//! Grouping rules: consecutive insert-at-caret ops merge while each starts
//! where the previous ended; consecutive single-grapheme backward deletes
//! merge while each ends where the previous started. Anything else — and
//! [`History::break_group`] — closes the open group.

use std::ops::Range;

use crate::style::{ImageBlock, InlineStyle, ListAttr, Voice};

/// Styles tiling a contiguous byte range, in absolute offsets.
pub(crate) type Tiles = Vec<(Range<usize>, InlineStyle)>;

/// One invertible primitive edit. Each carries enough to undo itself exactly.
#[derive(Debug, Clone)]
pub(crate) enum EditOp {
    /// `text` inserted at `at`, then `tiles` spliced over the inserted range and
    /// any `paras`/`images` re-attached. `paras`/`images` are empty for ordinary
    /// typing; they carry structure only when this insert is the inverse of a
    /// delete that removed list paragraphs or image blocks.
    Insert {
        at: usize,
        text: String,
        tiles: Tiles,
        paras: Vec<(usize, ListAttr)>,
        images: Vec<(usize, ImageBlock)>,
    },
    /// `text` removed from `at..at + text.len()`; `tiles` captured the styles
    /// (including plain gaps) and `paras`/`images` the list/image structure of
    /// that range immediately before removal, so the inverse insert restores
    /// them byte-exactly.
    Delete {
        at: usize,
        text: String,
        tiles: Tiles,
        paras: Vec<(usize, ListAttr)>,
        images: Vec<(usize, ImageBlock)>,
    },
    /// Styles over `range` changed from `before` tiles to `after` tiles.
    Restyle {
        range: Range<usize>,
        before: Tiles,
        after: Tiles,
    },
    /// The entry voice changed.
    Voice { before: Voice, after: Voice },
    /// The list attribute of the paragraph starting at `at` changed.
    SetList {
        at: usize,
        before: Option<ListAttr>,
        after: Option<ListAttr>,
    },
    /// The image block of the paragraph starting at `at` changed.
    SetImage {
        at: usize,
        before: Option<ImageBlock>,
        after: Option<ImageBlock>,
    },
}

impl EditOp {
    pub(crate) fn inverted(&self) -> EditOp {
        match self {
            EditOp::Insert {
                at,
                text,
                tiles,
                paras,
                images,
            } => EditOp::Delete {
                at: *at,
                text: text.clone(),
                tiles: tiles.clone(),
                paras: paras.clone(),
                images: images.clone(),
            },
            EditOp::Delete {
                at,
                text,
                tiles,
                paras,
                images,
            } => EditOp::Insert {
                at: *at,
                text: text.clone(),
                tiles: tiles.clone(),
                paras: paras.clone(),
                images: images.clone(),
            },
            EditOp::Restyle {
                range,
                before,
                after,
            } => EditOp::Restyle {
                range: range.clone(),
                before: after.clone(),
                after: before.clone(),
            },
            EditOp::Voice { before, after } => EditOp::Voice {
                before: *after,
                after: *before,
            },
            EditOp::SetList { at, before, after } => EditOp::SetList {
                at: *at,
                before: *after,
                after: *before,
            },
            EditOp::SetImage { at, before, after } => EditOp::SetImage {
                at: *at,
                before: *after,
                after: *before,
            },
        }
    }
}

/// How a group may absorb the next op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Merge {
    /// A typing run; next insert must start at this offset.
    Typing { end: usize },
    /// A backspace run; next single-grapheme delete must end at this offset.
    Backspace { start: usize },
    /// Never merges.
    None,
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub(crate) ops: Vec<EditOp>,
    merge: Merge,
    pub(crate) caret_before: Range<usize>,
    pub(crate) caret_after: Range<usize>,
}

/// Undo/redo stacks for one document. Session-scoped.
#[derive(Debug, Default, Clone)]
pub(crate) struct History {
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Whether the top undo group may still absorb compatible ops.
    open: bool,
}

impl History {
    /// Records a freshly applied insert, merging into an open typing run when
    /// it continues exactly at the caret.
    pub(crate) fn record_insert(&mut self, op: EditOp, at: usize, len: usize) {
        self.redo.clear();
        let end = at + len;
        if self.open
            && let Some(top) = self.undo.last_mut()
            && top.merge == (Merge::Typing { end: at })
        {
            top.ops.push(op);
            top.merge = Merge::Typing { end };
            top.caret_after = end..end;
            return;
        }
        self.undo.push(Group {
            ops: vec![op],
            merge: Merge::Typing { end },
            caret_before: at..at,
            caret_after: end..end,
        });
        self.open = true;
    }

    /// Records a freshly applied delete of `range`. `single_backward` marks a
    /// one-grapheme deletion eligible for backspace-run merging.
    pub(crate) fn record_delete(&mut self, op: EditOp, range: Range<usize>, single_backward: bool) {
        self.redo.clear();
        if single_backward
            && self.open
            && let Some(top) = self.undo.last_mut()
            && top.merge == (Merge::Backspace { start: range.end })
        {
            top.ops.push(op);
            top.merge = Merge::Backspace { start: range.start };
            // Undo of the whole run reselects everything it restores; the
            // run only ever grows leftward, so start stays a valid pre-group
            // offset while end keeps the first op's pre-group offset.
            top.caret_before = range.start..top.caret_before.end;
            top.caret_after = range.start..range.start;
            return;
        }
        self.undo.push(Group {
            ops: vec![op],
            merge: if single_backward {
                Merge::Backspace { start: range.start }
            } else {
                Merge::None
            },
            caret_before: range.clone(),
            caret_after: range.start..range.start,
        });
        self.open = true;
    }

    /// Records a non-mergeable group (replace, restyle, voice change).
    pub(crate) fn record_other(
        &mut self,
        ops: Vec<EditOp>,
        caret_before: Range<usize>,
        caret_after: Range<usize>,
    ) {
        self.redo.clear();
        self.undo.push(Group {
            ops,
            merge: Merge::None,
            caret_before,
            caret_after,
        });
        self.open = false;
    }

    /// Folds `ops` into the currently open group (so they reverse on the same
    /// Cmd-Z) if one is open, updating its caret-after and marking it
    /// non-mergeable; otherwise records them as a fresh non-mergeable group.
    /// Used to attach a follow-up attribute change (list/image) to the text
    /// edit that triggered it.
    pub(crate) fn record_into_open(&mut self, ops: Vec<EditOp>, caret_after: Range<usize>) {
        self.redo.clear();
        if self.open
            && let Some(top) = self.undo.last_mut()
        {
            top.ops.extend(ops);
            top.caret_after = caret_after;
            top.merge = Merge::None;
        } else {
            self.undo.push(Group {
                ops,
                merge: Merge::None,
                caret_before: caret_after.clone(),
                caret_after,
            });
            self.open = false;
        }
    }

    /// Closes the open group; the next op starts a new one.
    pub(crate) fn break_group(&mut self) {
        self.open = false;
    }

    /// Pops the group to undo. The caller applies each op's inverse in
    /// reverse order, then the group lands on the redo stack via
    /// [`History::push_redo`].
    pub(crate) fn pop_undo(&mut self) -> Option<Group> {
        self.open = false;
        self.undo.pop()
    }

    pub(crate) fn push_redo(&mut self, group: Group) {
        self.redo.push(group);
    }

    /// Pops the group to redo. The caller re-applies its ops in order, then
    /// returns it to the undo stack via [`History::push_undo_closed`].
    pub(crate) fn pop_redo(&mut self) -> Option<Group> {
        self.redo.pop()
    }

    pub(crate) fn push_undo_closed(&mut self, group: Group) {
        self.undo.push(group);
        self.open = false;
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
