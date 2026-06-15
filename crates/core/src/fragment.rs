//! `DocFragment` — a portable slice of a document: text plus everything that
//! styles it (inline runs, list paragraphs, image blocks), with all offsets
//! rebased so the slice starts at 0.
//!
//! This is the one neutral shape copy/paste round-trips through. Self-paste
//! serializes it to JSON on the clipboard (lossless); cross-app bridges
//! (RTF/HTML) translate *to and from* this same type, so a new formatting
//! feature is added in one place — the fragment + [`Document::slice_fragment`]
//! / [`Document::splice_fragment`] — and every paste path inherits it. No
//! per-feature case at the call sites.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::style::{ImageBlock, InlineStyle, ListAttr};

/// Current fragment format version (the clipboard metadata schema).
const VERSION: u64 = 1;

/// One non-plain inline run; offsets are bytes into [`DocFragment::text`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentRun {
    pub start: usize,
    pub end: usize,
    pub style: InlineStyle,
}

/// One list paragraph; `at` is the paragraph start within the fragment text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentList {
    pub at: usize,
    pub attr: ListAttr,
}

/// One embedded image; `at` is its paragraph start within the fragment text.
/// The block carries the blob `id`; bytes do not live in the fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentImage {
    pub at: usize,
    pub block: ImageBlock,
}

/// A self-contained, offset-rebased slice of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocFragment {
    /// Schema version; checked on decode.
    #[serde(default)]
    pub v: u64,
    pub text: String,
    #[serde(default)]
    pub runs: Vec<FragmentRun>,
    #[serde(default)]
    pub lists: Vec<FragmentList>,
    #[serde(default)]
    pub images: Vec<FragmentImage>,
}

impl DocFragment {
    /// Build from rebased parts (offsets already relative to the slice).
    pub fn new(
        text: String,
        runs: Vec<(Range<usize>, InlineStyle)>,
        lists: Vec<(usize, ListAttr)>,
        images: Vec<(usize, ImageBlock)>,
    ) -> Self {
        Self {
            v: VERSION,
            text,
            runs: runs
                .into_iter()
                .filter(|(range, style)| range.start < range.end && !style.is_plain())
                .map(|(range, style)| FragmentRun {
                    start: range.start,
                    end: range.end,
                    style,
                })
                .collect(),
            lists: lists
                .into_iter()
                .map(|(at, attr)| FragmentList { at, attr })
                .collect(),
            images: images
                .into_iter()
                .map(|(at, block)| FragmentImage { at, block })
                .collect(),
        }
    }

    /// True when the fragment carries only plain text (no rich structure) — the
    /// caller can then treat it as a plain insert.
    pub fn is_plain(&self) -> bool {
        self.runs.is_empty() && self.lists.is_empty() && self.images.is_empty()
    }

    /// Serialize for the clipboard metadata slot.
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// Decode and validate. Returns `None` for foreign metadata, a version
    /// mismatch, or any offset that doesn't lie on a char boundary of the
    /// carried text — paste then falls back to plain text.
    pub fn from_json(json: &str) -> Option<Self> {
        let frag: DocFragment = serde_json::from_str(json).ok()?;
        if frag.v != VERSION {
            return None;
        }
        let len = frag.text.len();
        let boundary = |at: usize| at <= len && frag.text.is_char_boundary(at);
        for run in &frag.runs {
            if run.start > run.end || !boundary(run.start) || !boundary(run.end) {
                return None;
            }
        }
        for list in &frag.lists {
            if !boundary(list.at) {
                return None;
            }
        }
        for image in &frag.images {
            if !boundary(image.at) {
                return None;
            }
        }
        Some(frag)
    }
}
