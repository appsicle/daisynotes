//! Value types shared across the agent pipeline: snapshots in, decisions out,
//! and the persistence shape for margin notes.

use std::convert::Infallible;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The registers Muse reads for. Order matters only for the tool schema enum.
pub const REGISTERS: [&str; 6] = ["essay", "journal", "story", "math", "letter", "notes"];

/// The reactions Muse can leave on a highlighted passage, iMessage-style.
pub const REACTION_EMOJI: [&str; 4] = ["❗", "😄", "😂", "❤️"];

/// Everything the agent needs to consider one entry — captured by the app at
/// trigger time so the consideration runs on an immutable view.
#[derive(Debug, Clone, Default)]
pub struct DocSnapshot {
    /// Entry id this snapshot was taken from (the app keys replies off it).
    pub entry_id: String,
    /// Full plain text of the entry.
    pub text: String,
    /// Register guessed on a previous consideration, e.g. `"journal"`.
    pub register_hint: Option<String>,
    /// Notes currently visible in the margin.
    pub active_notes: Vec<NoteRecord>,
    /// Digests of notes the writer dismissed (see [`crate::dismissal_digest`]).
    pub dismissed_digests: Vec<String>,
    /// The reflective response currently sitting under the entry, if any.
    pub last_response: Option<String>,
}

/// What flavor of note this is. Unknown strings fall back to [`Self::Insight`]
/// via [`FromStr`]; serde uses the strict lowercase names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    /// An observation about the writing itself.
    #[default]
    Insight,
    /// A question worth sitting with.
    Question,
    /// A moment that lands and deserves saying so (used sparingly).
    Encouragement,
    /// Something checkably wrong — a step, a fact, a contradiction.
    Correction,
    /// A source or connection worth chasing.
    Reference,
}

impl NoteKind {
    /// The lowercase wire name — identical to the serde representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insight => "insight",
            Self::Question => "question",
            Self::Encouragement => "encouragement",
            Self::Correction => "correction",
            Self::Reference => "reference",
        }
    }
}

impl FromStr for NoteKind {
    type Err = Infallible;

    /// Tolerant parse: case-insensitive, trims whitespace, and maps anything
    /// unrecognized to [`Self::Insight`] rather than failing.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "question" => Self::Question,
            "encouragement" => Self::Encouragement,
            "correction" => Self::Correction,
            "reference" => Self::Reference,
            _ => Self::Insight,
        })
    }
}

/// A note as persisted and shown in the margin. Also the storage shape:
/// `save_notes`/`load_notes` round-trip `Vec<NoteRecord>` as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    /// App-assigned id, unique per entry.
    pub id: u64,
    /// The verbatim passage the note is anchored to.
    pub quote: String,
    /// What flavor of note this is.
    pub kind: NoteKind,
    /// The note text itself.
    pub body: String,
    /// A reaction emoji (one of [`REACTION_EMOJI`]). When present the note
    /// renders as an iMessage-style highlight reaction rather than a card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

/// A note as proposed by the model, before the app anchors and assigns it an
/// id. `prefix`/`suffix` are short context windows used by
/// [`crate::locate_quote`] to disambiguate repeated phrases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDraft {
    /// Verbatim text from the entry (non-empty, at most 200 chars).
    pub quote: String,
    /// Up to ~20 chars immediately before the quote; may be empty.
    pub prefix: String,
    /// Up to ~20 chars immediately after the quote; may be empty.
    pub suffix: String,
    /// What flavor of note this is.
    pub kind: NoteKind,
    /// The note text itself. May be empty for a pure emoji reaction.
    pub body: String,
    /// A reaction emoji; presentation becomes a highlight reaction.
    pub emoji: Option<String>,
}

/// The outcome of one consideration — exactly one of silence, margin notes,
/// or an end-of-entry response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentDecision {
    /// Muse chose silence (the expected default). `reason` is diagnostic
    /// only — it is logged, never shown to the writer.
    Pass {
        /// Why silence was right (or why the reply could not be used).
        reason: String,
    },
    /// Up to two margin notes to anchor and bloom.
    Notes(Vec<NoteDraft>),
    /// A single reflective response for the end of the entry.
    Respond {
        /// The response text.
        body: String,
        /// The register the model read, lowercased, if it was one of
        /// [`REGISTERS`]; the app caches it as the next `register_hint`.
        register: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_kind_serde_is_lowercase() {
        let json = serde_json::to_string(&NoteKind::Encouragement).expect("serialize kind");
        assert_eq!(json, "\"encouragement\"");
        let back: NoteKind = serde_json::from_str("\"reference\"").expect("deserialize kind");
        assert_eq!(back, NoteKind::Reference);
    }

    #[test]
    fn note_kind_from_str_is_tolerant() {
        assert_eq!("question".parse(), Ok(NoteKind::Question));
        assert_eq!("  Correction ".parse(), Ok(NoteKind::Correction));
        assert_eq!("ENCOURAGEMENT".parse(), Ok(NoteKind::Encouragement));
        assert_eq!("vibe-check".parse(), Ok(NoteKind::Insight));
        assert_eq!("".parse(), Ok(NoteKind::Insight));
    }

    #[test]
    fn note_record_round_trips_through_json() {
        let notes = vec![
            NoteRecord {
                id: 7,
                quote: "the sea was loud — café loud".to_string(),
                kind: NoteKind::Question,
                body: "what made it loud that night?".to_string(),
                emoji: None,
            },
            NoteRecord {
                id: 8,
                quote: "x² + y²".to_string(),
                kind: NoteKind::Correction,
                body: "the second term drops a sign".to_string(),
                emoji: Some("❤️".to_string()),
            },
        ];
        let json = serde_json::to_string(&notes).expect("serialize notes");
        assert!(json.contains("\"kind\":\"question\""));
        let back: Vec<NoteRecord> = serde_json::from_str(&json).expect("deserialize notes");
        assert_eq!(back, notes);
    }
}
