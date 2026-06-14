//! `ParaList` — per-paragraph list attributes (bullet/number + indent),
//! keyed by each paragraph's start byte offset and transformed across edits
//! the same way [`crate::spans::SpanSet`] tracks inline styles.
//!
//! A paragraph is a newline-delimited line; its key is its first byte (the
//! byte after the preceding `\n`, or 0). Only list paragraphs are stored —
//! a plain paragraph has no entry. The editor sets and clears attributes
//! explicitly (`- ` / `1. ` triggers, Tab/Shift-Tab, Enter); the transforms
//! here only keep existing keys aligned as surrounding text shifts.

use crate::style::ListAttr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParaEntry {
    /// Byte offset of the paragraph's first character (the list line start).
    pub(crate) at: usize,
    pub(crate) attr: ListAttr,
}

/// List attributes for the paragraphs that are list items, sorted by `at`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaList {
    entries: Vec<ParaEntry>,
}

impl ParaList {
    /// An empty set: every paragraph reads as a plain (non-list) line.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no paragraph carries a list attribute.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The list attribute of the paragraph starting at `at`, if any.
    pub fn get(&self, at: usize) -> Option<ListAttr> {
        self.entries
            .iter()
            .find(|e| e.at == at)
            .map(|e| e.attr)
    }

    /// Set (or clear, with `None`) the list attribute for the paragraph
    /// starting at `at`. Returns the previous attribute.
    pub(crate) fn set(&mut self, at: usize, attr: Option<ListAttr>) -> Option<ListAttr> {
        let pos = self.entries.iter().position(|e| e.at == at);
        let before = pos.map(|i| self.entries[i].attr);
        match (pos, attr) {
            (Some(i), Some(attr)) => self.entries[i].attr = attr,
            (Some(i), None) => {
                self.entries.remove(i);
            }
            (None, Some(attr)) => {
                let insert_at = self.entries.partition_point(|e| e.at < at);
                self.entries.insert(insert_at, ParaEntry { at, attr });
            }
            (None, None) => {}
        }
        before
    }

    /// Iterate every stored (paragraph-start, attribute) pair in order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, ListAttr)> + '_ {
        self.entries.iter().map(|e| (e.at, e.attr))
    }

    /// Transform for `len` bytes inserted at `at`. A paragraph start strictly
    /// after the insertion shifts right; one exactly at the insertion stays
    /// put (the inserted text joins the front of that paragraph).
    pub(crate) fn transform_insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for entry in &mut self.entries {
            if entry.at > at {
                entry.at += len;
            }
        }
    }

    /// Transform for `range` deleted. A paragraph start at or before the cut
    /// is unchanged; one comfortably after it shifts left. A start in
    /// `range.start < at <= range.end` loses the `\n` that preceded it, so its
    /// line merges into the one above and the attribute is dropped (the line
    /// above keeps its own).
    pub(crate) fn transform_delete(&mut self, range: std::ops::Range<usize>) {
        let len = range.end.saturating_sub(range.start);
        if len == 0 {
            return;
        }
        self.entries.retain_mut(|entry| {
            if entry.at <= range.start {
                true
            } else if entry.at > range.end {
                entry.at -= len;
                true
            } else {
                false
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ListKind;

    fn bullet(indent: u8) -> ListAttr {
        ListAttr {
            kind: ListKind::Bullet,
            indent,
        }
    }

    #[test]
    fn set_get_clear() {
        let mut p = ParaList::new();
        assert_eq!(p.set(4, Some(bullet(0))), None);
        assert_eq!(p.get(4), Some(bullet(0)));
        assert_eq!(p.set(4, Some(bullet(1))), Some(bullet(0)));
        assert_eq!(p.get(4), Some(bullet(1)));
        assert_eq!(p.set(4, None), Some(bullet(1)));
        assert!(p.is_empty());
    }

    #[test]
    fn insert_shifts_later_paragraphs_only() {
        let mut p = ParaList::new();
        p.set(0, Some(bullet(0)));
        p.set(6, Some(bullet(0)));
        // Insert inside the first paragraph: only the second start moves.
        p.transform_insert(3, 2);
        assert_eq!(p.get(0), Some(bullet(0)));
        assert_eq!(p.get(8), Some(bullet(0)));
        // Insert exactly at a paragraph start: that start stays.
        p.transform_insert(8, 1);
        assert_eq!(p.get(8), Some(bullet(0)));
    }

    #[test]
    fn delete_shifts_and_merges() {
        let mut p = ParaList::new();
        p.set(0, Some(bullet(0)));
        p.set(6, Some(bullet(1)));
        // Delete the newline that begins the second list line: it merges up,
        // dropping its attribute; nothing after to shift.
        p.transform_delete(5..6);
        assert_eq!(p.get(0), Some(bullet(0)));
        assert_eq!(p.get(5), None);
        assert!(p.iter().count() == 1);
    }
}
