//! `ImageSet` — image blocks keyed by their (empty) paragraph's start byte
//! offset, transformed across edits exactly like [`crate::paras::ParaList`].
//!
//! An image lives on its own paragraph (no rope sentinel): the paragraph's
//! start offset is the key, and the block carries the blob id and natural
//! pixel size. The same paragraph-start transform rules as `ParaList` keep
//! the key aligned as surrounding text shifts; a deletion that swallows the
//! paragraph start drops the block.

use crate::style::ImageBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageEntry {
    pub(crate) at: usize,
    pub(crate) block: ImageBlock,
}

/// Image blocks for the paragraphs that hold one, sorted by `at`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageSet {
    entries: Vec<ImageEntry>,
}

impl ImageSet {
    /// An empty set: no paragraph holds an image.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no paragraph holds an image.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The image of the paragraph starting at `at`, if any.
    pub fn get(&self, at: usize) -> Option<ImageBlock> {
        self.entries.iter().find(|e| e.at == at).map(|e| e.block)
    }

    /// Set (or clear, with `None`) the image of the paragraph at `at`.
    pub(crate) fn set(&mut self, at: usize, block: Option<ImageBlock>) {
        let pos = self.entries.iter().position(|e| e.at == at);
        match (pos, block) {
            (Some(i), Some(block)) => self.entries[i].block = block,
            (Some(i), None) => {
                self.entries.remove(i);
            }
            (None, Some(block)) => {
                let insert_at = self.entries.partition_point(|e| e.at < at);
                self.entries.insert(insert_at, ImageEntry { at, block });
            }
            (None, None) => {}
        }
    }

    /// Iterate every (paragraph-start, block) pair in order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, ImageBlock)> + '_ {
        self.entries.iter().map(|e| (e.at, e.block))
    }

    /// Transform for `len` bytes inserted at `at`. An image is a newline at
    /// `entry.at`; inserting *at or before* that newline slides the image (and
    /// its line) to the right, so an insert exactly at the image's offset moves
    /// it rather than leaving the block stranded on the new text. This is the
    /// inverse of `transform_delete` and lets the editor type text onto an
    /// image's line by pushing the image down to its own paragraph.
    pub(crate) fn transform_insert(&mut self, at: usize, len: usize) {
        if len == 0 {
            return;
        }
        for entry in &mut self.entries {
            if entry.at >= at {
                entry.at += len;
            }
        }
    }

    /// Transform for `range` deleted. An image is a newline at `entry.at`, and
    /// the newline that *separates* it from the previous line sits at
    /// `entry.at - 1`. The block survives only when neither is touched:
    /// - `at < start` — entirely before the delete; kept, unmoved.
    /// - `at > end` — entirely after; the separator at `at-1` is also clear
    ///   (`at-1 >= end`), so the image keeps its own line; shifted left.
    /// - `start <= at <= end` — the image's own newline (`at`) or its separator
    ///   (`at-1`, deleted whenever `at == end`) is swallowed, so its line is
    ///   gone; dropped.
    ///
    /// The `at < start` lower bound is why Select-All (`0..len`) removes the
    /// image at offset 0 (the old `at <= start` kept it); the `at > end` upper
    /// bound is why deleting text up *to* an image's line drops the merged-away
    /// image rather than stranding it mid-line.
    pub(crate) fn transform_delete(&mut self, range: std::ops::Range<usize>) {
        let len = range.end.saturating_sub(range.start);
        if len == 0 {
            return;
        }
        self.entries.retain_mut(|entry| {
            if entry.at < range.start {
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

    fn block(id: u64) -> ImageBlock {
        ImageBlock {
            id,
            w: 0,
            h: 0,
            width: 0,
        }
    }

    #[test]
    fn select_all_delete_drops_every_image() {
        // Three images pasted on their own lines: "\n\n\n", blocks at 0,1,2.
        let mut s = ImageSet::new();
        s.set(0, Some(block(10)));
        s.set(1, Some(block(11)));
        s.set(2, Some(block(12)));
        // Cmd-A then delete removes the whole document.
        s.transform_delete(0..3);
        assert!(s.is_empty(), "the image at offset 0 must be deleted too");
    }

    #[test]
    fn delete_text_strictly_before_image_shifts_it() {
        let mut s = ImageSet::new();
        s.set(4, Some(block(7)));
        // Deleting text that stops before the image's separator (offset 3)
        // keeps it, shifted left; its own line stays intact.
        s.transform_delete(0..3);
        assert_eq!(s.get(1), Some(block(7)));
    }

    #[test]
    fn delete_up_to_image_line_drops_it() {
        let mut s = ImageSet::new();
        s.set(4, Some(block(7)));
        // A delete ending at the image's offset swallows its separator newline
        // (offset 3), merging its line away — the image drops rather than
        // stranding mid-line.
        s.transform_delete(0..4);
        assert!(s.is_empty());
    }

    #[test]
    fn insert_at_image_offset_slides_it_down() {
        let mut s = ImageSet::new();
        s.set(0, Some(block(5)));
        // Typing "h\n" onto the image's own line pushes the image to the next
        // paragraph rather than leaving it stranded over the new text.
        s.transform_insert(0, 2);
        assert_eq!(s.get(0), None);
        assert_eq!(s.get(2), Some(block(5)));
    }

    #[test]
    fn insert_before_image_shifts_it() {
        let mut s = ImageSet::new();
        s.set(6, Some(block(9)));
        s.transform_insert(2, 3);
        assert_eq!(s.get(9), Some(block(9)));
    }

    #[test]
    fn delete_span_drops_inside_and_separator_consumed_images() {
        // Blocks at 0,1,2,3 (consecutive empty lines); delete [1,3).
        let mut s = ImageSet::new();
        for (at, id) in [(0, 1u64), (1, 2), (2, 3), (3, 4)] {
            s.set(at, Some(block(id)));
        }
        s.transform_delete(1..3);
        // 0 kept; 1,2 are inside the range; 3's separator (offset 2) is
        // consumed — all three go, leaving only the leading image.
        assert_eq!(s.get(0), Some(block(1)));
        assert_eq!(s.iter().count(), 1);
    }
}
