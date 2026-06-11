//! Versioned document serialization: `{"v":1,"voice":…,"text":…,"spans":[…]}`.
//!
//! The format is written compactly (false/None style fields are omitted) and
//! is golden-tested so storage and sync can rely on byte-stable output.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::spans::SpanSet;
use crate::style::{InlineStyle, Voice};

/// Current serialization format version.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Errors produced when reading a serialized document.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    /// The JSON could not be parsed at all.
    #[error("failed to parse document JSON: {0}")]
    Parse(#[from] serde_json::Error),
    /// The document was written by a newer (or unknown) format version.
    #[error("unsupported document format version {0}")]
    UnsupportedVersion(u32),
    /// A span lies outside the text or off a char boundary.
    #[error("span {start}..{end} is out of bounds or misaligned")]
    InvalidSpan {
        /// Span start byte offset as stored.
        start: usize,
        /// Span end byte offset as stored.
        end: usize,
    },
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DocDto {
    pub(crate) v: u32,
    pub(crate) voice: Voice,
    pub(crate) text: String,
    pub(crate) spans: Vec<SpanDto>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SpanDto {
    pub(crate) start: usize,
    pub(crate) end: usize,
    #[serde(flatten)]
    pub(crate) style: InlineStyle,
}

impl DocDto {
    pub(crate) fn encode(voice: Voice, text: String, spans: &SpanSet) -> String {
        let dto = DocDto {
            v: FORMAT_VERSION,
            voice,
            text,
            spans: spans
                .iter()
                .map(|(range, style)| SpanDto {
                    start: range.start,
                    end: range.end,
                    style,
                })
                .collect(),
        };
        // Serializing this DTO cannot fail (plain structs, string keys); the
        // fallback is an empty v1 document rather than a panic path.
        serde_json::to_string(&dto).unwrap_or_else(|_| {
            "{\"v\":1,\"voice\":{\"family\":\"literata\",\"size\":16.0,\"weight\":400},\
             \"text\":\"\",\"spans\":[]}"
                .to_string()
        })
    }

    pub(crate) fn decode(json: &str) -> Result<DocDto, DocError> {
        let dto: DocDto = serde_json::from_str(json)?;
        if dto.v != FORMAT_VERSION {
            return Err(DocError::UnsupportedVersion(dto.v));
        }
        Ok(dto)
    }

    /// Validates one stored span against the decoded text.
    pub(crate) fn validate_span(
        text: &str,
        start: usize,
        end: usize,
    ) -> Result<Range<usize>, DocError> {
        let valid = start <= end
            && end <= text.len()
            && text.is_char_boundary(start)
            && text.is_char_boundary(end);
        if valid {
            Ok(start..end)
        } else {
            Err(DocError::InvalidSpan { start, end })
        }
    }
}
