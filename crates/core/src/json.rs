//! Versioned document serialization: `{"v":1,"voice":…,"text":…,"spans":[…]}`.
//!
//! The format is written compactly (false/None style fields are omitted) and
//! is golden-tested so storage and sync can rely on byte-stable output. Newer
//! block features (list paragraphs) ride as optional fields that are omitted
//! when empty, so a plain document still round-trips byte-for-byte as v1.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::images::ImageSet;
use crate::paras::ParaList;
use crate::spans::SpanSet;
use crate::style::{InlineStyle, ListKind, Voice};

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
    /// List-paragraph attributes. Omitted (and defaulted) when there are none,
    /// so plain documents stay byte-identical to the original v1 format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) paras: Vec<ParaDto>,
    /// Image blocks. Omitted (and defaulted) when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) images: Vec<ImageDto>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SpanDto {
    pub(crate) start: usize,
    pub(crate) end: usize,
    #[serde(flatten)]
    pub(crate) style: InlineStyle,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ParaDto {
    /// Byte offset of the list paragraph's first character.
    pub(crate) at: usize,
    pub(crate) kind: ListKind,
    pub(crate) indent: u8,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ImageDto {
    /// Byte offset of the image paragraph's start.
    pub(crate) at: usize,
    /// Content hash of the encoded bytes (blob key / GPUI image id).
    pub(crate) id: u64,
    pub(crate) w: u32,
    pub(crate) h: u32,
    /// User-chosen display width in px; omitted (0) means fit the column.
    #[serde(default)]
    pub(crate) width: u32,
}

impl DocDto {
    pub(crate) fn encode(
        voice: Voice,
        text: String,
        spans: &SpanSet,
        paras: &ParaList,
        images: &ImageSet,
    ) -> String {
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
            paras: paras
                .iter()
                .map(|(at, attr)| ParaDto {
                    at,
                    kind: attr.kind,
                    indent: attr.indent,
                })
                .collect(),
            images: images
                .iter()
                .map(|(at, block)| ImageDto {
                    at,
                    id: block.id,
                    w: block.w,
                    h: block.h,
                    width: block.width,
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

    /// A stored list-paragraph offset is kept only if it lands on a char
    /// boundary within the text and at the start of a line; otherwise it is
    /// silently dropped (a corrupt attribute never breaks the whole document).
    pub(crate) fn valid_para(text: &str, at: usize) -> bool {
        let on_boundary = at <= text.len() && text.is_char_boundary(at);
        let line_start = at == 0 || text.as_bytes().get(at.wrapping_sub(1)) == Some(&b'\n');
        on_boundary && line_start
    }

    /// A stored image offset is kept only when it is a valid paragraph start
    /// *and* that paragraph is empty — its first byte is a newline, or it is
    /// the empty final paragraph. This keeps images on their own line: a
    /// corrupt or newer-version offset that points into text is dropped rather
    /// than hiding the text behind the image or slicing a multi-byte char when
    /// the line is later removed.
    pub(crate) fn valid_image(text: &str, at: usize) -> bool {
        Self::valid_para(text, at)
            && (at == text.len() || text.as_bytes().get(at) == Some(&b'\n'))
    }
}
