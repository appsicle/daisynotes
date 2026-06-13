//! daisynotes-core — the document model: rope text, style spans, ops, undo, anchors.
//! Owns the words. Knows nothing about pixels, persistence, or the network.
//!
//! All offsets in the public API are **global byte offsets** into the UTF-8
//! text, always aligned to `char` boundaries. Conversions to ropey's char
//! indices happen internally and never leak.

mod anchor;
mod document;
mod history;
mod json;
mod nav;
mod spans;
mod style;

pub use anchor::AnchorId;
pub use document::{Document, UndoOutcome};
pub use json::DocError;
pub use spans::SpanSet;
pub use style::{FontFamily, Ink, InlineStyle, SIZE_STEPS, StyleToggle, Voice};

/// Stable, time-sortable identifier for an entry (and its document).
pub type EntryId = ulid::Ulid;
