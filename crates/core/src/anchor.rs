//! Anchors — byte ranges that survive edits. Agent notes pin to these.
//!
//! Transform rules:
//! - insert at or before the start shifts both ends right,
//! - insert strictly inside extends the end,
//! - delete entirely covering the anchor destroys it,
//! - partial overlap clamps the surviving side.

use std::ops::Range;

/// Opaque handle to a registered anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnchorId(u64);

#[derive(Debug, Clone, Default)]
pub(crate) struct AnchorMap {
    entries: Vec<(AnchorId, Range<usize>)>,
    next: u64,
}

impl AnchorMap {
    pub(crate) fn register(&mut self, range: Range<usize>) -> AnchorId {
        let id = AnchorId(self.next);
        self.next += 1;
        self.entries.push((id, range));
        id
    }

    pub(crate) fn get(&self, id: AnchorId) -> Option<Range<usize>> {
        self.entries
            .iter()
            .find(|(aid, _)| *aid == id)
            .map(|(_, r)| r.clone())
    }

    pub(crate) fn release(&mut self, id: AnchorId) {
        self.entries.retain(|(aid, _)| *aid != id);
    }

    pub(crate) fn transform_insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for (_, range) in &mut self.entries {
            if at <= range.start {
                range.start += len;
                range.end += len;
            } else if at < range.end {
                range.end += len;
            }
        }
    }

    pub(crate) fn transform_delete(&mut self, deleted: Range<usize>) {
        let len = deleted.end.saturating_sub(deleted.start);
        if len == 0 {
            return;
        }
        self.entries.retain_mut(|(_, range)| {
            if deleted.start <= range.start && range.end <= deleted.end {
                // Fully covered: the anchored text is gone. Destroy.
                return false;
            }
            if deleted.end <= range.start {
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
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_before_shifts_inside_extends_after_ignores() {
        let mut m = AnchorMap::default();
        let id = m.register(5..10);
        m.transform_insert(2, 3); // before
        assert_eq!(m.get(id), Some(8..13));
        m.transform_insert(10, 2); // strictly inside
        assert_eq!(m.get(id), Some(8..15));
        m.transform_insert(15, 4); // at end: not inside
        assert_eq!(m.get(id), Some(8..15));
        m.transform_insert(8, 1); // exactly at start: shifts
        assert_eq!(m.get(id), Some(9..16));
    }

    #[test]
    fn delete_covering_destroys() {
        let mut m = AnchorMap::default();
        let id = m.register(5..10);
        m.transform_delete(5..10);
        assert_eq!(m.get(id), None);
        let id2 = m.register(5..10);
        m.transform_delete(4..11);
        assert_eq!(m.get(id2), None);
    }

    #[test]
    fn delete_partial_clamps() {
        let mut m = AnchorMap::default();
        let head = m.register(5..10);
        m.transform_delete(3..7); // overlaps head
        assert_eq!(m.get(head), Some(3..6));

        let mut m = AnchorMap::default();
        let tail = m.register(5..10);
        m.transform_delete(8..12); // overlaps tail
        assert_eq!(m.get(tail), Some(5..8));

        let mut m = AnchorMap::default();
        let around = m.register(5..10);
        m.transform_delete(6..8); // inside
        assert_eq!(m.get(around), Some(5..8));

        let mut m = AnchorMap::default();
        let after = m.register(5..10);
        m.transform_delete(0..3); // before
        assert_eq!(m.get(after), Some(2..7));
    }

    #[test]
    fn release_forgets() {
        let mut m = AnchorMap::default();
        let id = m.register(1..2);
        m.release(id);
        assert_eq!(m.get(id), None);
    }
}
