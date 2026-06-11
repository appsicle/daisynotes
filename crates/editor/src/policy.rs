//! Pure editing policies: selection normalization, pending caret style,
//! undo-group timing, and voice-size stepping. No gpui types — everything
//! here is unit-testable without a window.

use std::ops::Range;
use std::time::Duration;

use muse_core::{InlineStyle, SIZE_STEPS, StyleToggle};

/// Gap between edits after which the open undo group is closed, so a pause
/// in typing becomes an undo boundary (PLAN §5).
pub(crate) const UNDO_GAP: Duration = Duration::from_millis(750);

/// True when enough time has passed since the previous edit that the next
/// edit should start a fresh undo group.
pub(crate) fn should_break_undo_group(since_last_edit: Option<Duration>) -> bool {
    match since_last_edit {
        Some(gap) => gap > UNDO_GAP,
        // First edit of the session: nothing to split from.
        None => false,
    }
}

/// The selection: `head` is the moving end (where the caret blinks), `anchor`
/// is the fixed end. `head` may precede `anchor` while selecting backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    pub head: usize,
    pub anchor: usize,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            head: offset,
            anchor: offset,
        }
    }

    /// The selection as a forward byte range (`start <= end`).
    pub fn range(&self) -> Range<usize> {
        if self.head <= self.anchor {
            self.head..self.anchor
        } else {
            self.anchor..self.head
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.anchor
    }

    /// True when the head is the left edge (selection made right-to-left).
    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }
}

/// A style waiting at a collapsed caret: set by toggling a style with no
/// selection, consumed by the next insertion at exactly that offset, and
/// cleared by any caret movement (standard macOS behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingStyle {
    /// The caret offset the style was staged at.
    pub at: usize,
    /// The full style newly typed text should take.
    pub style: InlineStyle,
}

impl PendingStyle {
    /// Stage a toggle at a collapsed caret. `base` is the style typing would
    /// otherwise continue with ([`muse_core::Document::style_for_insertion`]).
    pub fn stage(existing: Option<PendingStyle>, at: usize, base: InlineStyle, toggle: StyleToggle) -> PendingStyle {
        let mut style = match existing {
            Some(pending) if pending.at == at => pending.style,
            _ => base,
        };
        apply_toggle(&mut style, toggle);
        PendingStyle { at, style }
    }

    /// The toggles that turn `continuation` (the style an insertion actually
    /// took) into the staged style. Empty when they already match.
    pub fn toggles_against(&self, continuation: InlineStyle) -> Vec<StyleToggle> {
        let mut toggles = Vec::new();
        if self.style.bold != continuation.bold {
            toggles.push(StyleToggle::Bold);
        }
        if self.style.italic != continuation.italic {
            toggles.push(StyleToggle::Italic);
        }
        if self.style.underline != continuation.underline {
            toggles.push(StyleToggle::Underline);
        }
        if self.style.strike != continuation.strike {
            toggles.push(StyleToggle::Strike);
        }
        if self.style.ink != continuation.ink {
            toggles.push(StyleToggle::Ink(self.style.ink));
        }
        toggles
    }
}

/// Flip one attribute of `style` the way [`muse_core::Document::toggle_style`]
/// would for a uniform range.
fn apply_toggle(style: &mut InlineStyle, toggle: StyleToggle) {
    match toggle {
        StyleToggle::Bold => style.bold = !style.bold,
        StyleToggle::Italic => style.italic = !style.italic,
        StyleToggle::Underline => style.underline = !style.underline,
        StyleToggle::Strike => style.strike = !style.strike,
        StyleToggle::Ink(ink) => style.ink = ink,
    }
}

/// The next size step above `size`, or `size` when already at the top.
pub(crate) fn step_size_up(size: f32) -> f32 {
    let idx = nearest_step(size);
    SIZE_STEPS[(idx + 1).min(SIZE_STEPS.len() - 1)]
}

/// The next size step below `size`, or `size` when already at the bottom.
pub(crate) fn step_size_down(size: f32) -> f32 {
    let idx = nearest_step(size);
    SIZE_STEPS[idx.saturating_sub(1)]
}

/// Index of the step closest to `size` (ties resolve to the smaller step).
fn nearest_step(size: f32) -> usize {
    let mut best = 0;
    let mut best_dist = f32::MAX;
    for (idx, step) in SIZE_STEPS.iter().enumerate() {
        let dist = (step - size).abs();
        if dist < best_dist {
            best = idx;
            best_dist = dist;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use muse_core::Ink;

    #[test]
    fn selection_normalizes_both_directions() {
        let forward = Selection { head: 9, anchor: 4 };
        assert_eq!(forward.range(), 4..9);
        assert!(!forward.reversed());

        let backward = Selection { head: 4, anchor: 9 };
        assert_eq!(backward.range(), 4..9);
        assert!(backward.reversed());

        let caret = Selection::caret(7);
        assert!(caret.is_empty());
        assert_eq!(caret.range(), 7..7);
    }

    #[test]
    fn undo_group_breaks_only_after_the_gap() {
        assert!(!should_break_undo_group(None));
        assert!(!should_break_undo_group(Some(Duration::from_millis(749))));
        assert!(!should_break_undo_group(Some(UNDO_GAP)));
        assert!(should_break_undo_group(Some(Duration::from_millis(751))));
    }

    #[test]
    fn pending_style_stages_and_diffs() {
        let base = InlineStyle::default();
        // cmd-B at offset 5 stages bold.
        let pending = PendingStyle::stage(None, 5, base, StyleToggle::Bold);
        assert!(pending.style.bold);
        assert_eq!(pending.at, 5);

        // A second toggle at the same caret accumulates onto the staged style.
        let pending = PendingStyle::stage(Some(pending), 5, base, StyleToggle::Ink(Some(Ink::Rose)));
        assert!(pending.style.bold);
        assert_eq!(pending.style.ink, Some(Ink::Rose));

        // A stale pending (different offset) restarts from the base style.
        let pending = PendingStyle::stage(Some(pending), 9, base, StyleToggle::Italic);
        assert!(!pending.style.bold);
        assert!(pending.style.italic);
    }

    #[test]
    fn pending_style_toggle_diff_round_trips() {
        let base = InlineStyle {
            bold: true,
            ..InlineStyle::default()
        };
        // Stage italic on top of bold continuation; then suppose the insert
        // continued with plain text — both bold and italic must be applied.
        let pending = PendingStyle::stage(None, 0, base, StyleToggle::Italic);
        let toggles = pending.toggles_against(InlineStyle::default());
        assert!(toggles.contains(&StyleToggle::Bold));
        assert!(toggles.contains(&StyleToggle::Italic));
        assert_eq!(toggles.len(), 2);
        // Against a matching continuation there is nothing to do.
        assert!(pending.toggles_against(pending.style).is_empty());
    }

    #[test]
    fn double_toggle_cancels() {
        let base = InlineStyle::default();
        let pending = PendingStyle::stage(None, 3, base, StyleToggle::Bold);
        let pending = PendingStyle::stage(Some(pending), 3, base, StyleToggle::Bold);
        assert_eq!(pending.style, base);
        assert!(pending.toggles_against(base).is_empty());
    }

    #[test]
    fn size_steps_walk_the_ladder() {
        assert!((step_size_up(16.0) - 18.0).abs() < f32::EPSILON);
        assert!((step_size_down(16.0) - 15.0).abs() < f32::EPSILON);
        // Clamped at both ends.
        assert!((step_size_up(28.0) - 28.0).abs() < f32::EPSILON);
        assert!((step_size_down(13.0) - 13.0).abs() < f32::EPSILON);
        // Off-ladder sizes snap to the nearest step before stepping.
        assert!((step_size_up(16.9) - 18.0).abs() < f32::EPSILON);
        assert!((step_size_down(26.5) - 24.0).abs() < f32::EPSILON);
    }
}
