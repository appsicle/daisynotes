//! Tolerant parsing of the model's single tool call into an [`AgentDecision`].
//!
//! The model is forced to call exactly one of `pass`, `leave_notes`, or
//! `respond`. Anything malformed degrades to [`AgentDecision::Pass`] with a
//! diagnostic reason — a bad reply must never surface as a bad note.

use serde_json::Value;

use crate::types::{AgentDecision, NoteDraft, NoteKind, REGISTERS};

/// Maximum notes per consideration; extras are dropped.
const MAX_NOTES: usize = 3;
/// Maximum quote length in chars; longer quotes are rejected.
const MAX_QUOTE_CHARS: usize = 300;

/// Interpret one Claude reply as a decision.
///
/// Tolerant by design: an unknown tool, a missing tool call, or malformed
/// input yields `Pass` with a diagnostic reason (and a `tracing` warning),
/// never an error. Notes are capped at two; notes with empty or over-long
/// quotes (or empty bodies) are dropped.
#[must_use]
pub fn parse_decision(reply: &daisynotes_api::ClaudeReply) -> AgentDecision {
    let Some(tool) = reply.tool_name.as_deref() else {
        tracing::warn!(stop_reason = ?reply.stop_reason, "reply carried no tool call");
        return diagnostic_pass("reply carried no tool call");
    };
    let input = reply.tool_input.clone().unwrap_or(Value::Null);
    match tool {
        "pass" => parse_pass(&input),
        "leave_notes" => parse_notes(&input),
        "respond" => parse_respond(&input),
        other => {
            tracing::warn!(tool = other, "unknown tool in reply");
            diagnostic_pass(&format!("unknown tool {other:?}"))
        }
    }
}

fn diagnostic_pass(reason: &str) -> AgentDecision {
    AgentDecision::Pass {
        reason: reason.to_string(),
    }
}

fn parse_pass(input: &Value) -> AgentDecision {
    let reason = input
        .get("reason")
        .and_then(Value::as_str)
        .map_or_else(|| "(no reason given)".to_string(), str::to_string);
    AgentDecision::Pass { reason }
}

fn parse_notes(input: &Value) -> AgentDecision {
    let Some(items) = input.get("notes").and_then(Value::as_array) else {
        tracing::warn!("leave_notes input had no notes array");
        return diagnostic_pass("leave_notes input had no notes array");
    };
    let mut drafts: Vec<NoteDraft> = Vec::new();
    for item in items {
        if drafts.len() == MAX_NOTES {
            tracing::warn!(extra = items.len() - MAX_NOTES, "extra notes dropped");
            break;
        }
        if let Some(draft) = parse_note(item) {
            drafts.push(draft);
        }
    }
    if drafts.is_empty() {
        tracing::warn!("leave_notes carried no usable notes");
        diagnostic_pass("leave_notes carried no usable notes")
    } else {
        AgentDecision::Notes(drafts)
    }
}

fn parse_note(item: &Value) -> Option<NoteDraft> {
    let quote = item.get("quote").and_then(Value::as_str).unwrap_or("");
    if quote.trim().is_empty() {
        tracing::warn!("note dropped: empty quote");
        return None;
    }
    if quote.chars().count() > MAX_QUOTE_CHARS {
        tracing::warn!(
            chars = quote.chars().count(),
            "note dropped: quote too long"
        );
        return None;
    }
    let mut emoji = item
        .get("emoji")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|e| crate::types::REACTION_EMOJI.contains(e))
        .map(str::to_string);
    let body = item.get("body").and_then(Value::as_str).unwrap_or("");
    // A note is one of two things: a written note (a body) or a pure
    // reaction (an emoji, no body). If the model sent both, the body wins —
    // words it bothered to write outrank a decorative emoji, and a reaction
    // must stay bodiless so dismissing it never flashes an empty card.
    if !body.trim().is_empty() {
        emoji = None;
    } else if emoji.is_none() {
        tracing::warn!("note dropped: empty body and no reaction");
        return None;
    }
    if looks_degenerate(body) {
        tracing::warn!("note dropped: degenerate body");
        return None;
    }
    let kind = item
        .get("kind")
        .and_then(Value::as_str)
        .map_or(NoteKind::Insight, |s| s.parse().unwrap_or_default());
    Some(NoteDraft {
        quote: quote.to_string(),
        prefix: string_field(item, "prefix"),
        suffix: string_field(item, "suffix"),
        kind,
        body: body.to_string(),
        emoji,
    })
}

/// True when `text` collapses into a runaway loop: any 1–4 char pattern
/// repeated six or more times in a row (the "hjhjhj…" failure mode of small
/// models filling a grammar-bounded string).
fn looks_degenerate(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for width in 1..=4usize {
        if chars.len() < width * 6 {
            continue;
        }
        let mut run = 1usize;
        let mut start = width;
        while start + width <= chars.len() {
            if chars[start..start + width] == chars[start - width..start] {
                run += 1;
                if run >= 6 {
                    return true;
                }
            } else {
                run = 1;
            }
            start += width;
        }
    }
    false
}

fn string_field(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn parse_respond(input: &Value) -> AgentDecision {
    let body = input.get("body").and_then(Value::as_str).unwrap_or("");
    if body.trim().is_empty() {
        tracing::warn!("respond input had no body");
        return diagnostic_pass("respond input had no body");
    }
    let register = input
        .get("register")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .filter(|r| REGISTERS.contains(&r.as_str()));
    AgentDecision::Respond {
        body: body.to_string(),
        register,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daisynotes_api::ClaudeReply;
    use serde_json::json;

    fn reply(tool: &str, input: Value) -> ClaudeReply {
        ClaudeReply {
            text: None,
            tool_name: Some(tool.to_string()),
            tool_input: Some(input),
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn note(quote: &str, kind: &str, body: &str) -> Value {
        json!({ "quote": quote, "prefix": "", "suffix": "", "kind": kind, "body": body })
    }

    #[test]
    fn pass_with_reason() {
        let decision = parse_decision(&reply("pass", json!({ "reason": "mid-thought" })));
        assert_eq!(
            decision,
            AgentDecision::Pass {
                reason: "mid-thought".to_string()
            }
        );
    }

    #[test]
    fn pass_with_malformed_input_still_passes() {
        let decision = parse_decision(&reply("pass", json!(42)));
        assert!(matches!(decision, AgentDecision::Pass { .. }));
    }

    #[test]
    fn leave_notes_parses_drafts() {
        let input = json!({
            "register": "essay",
            "notes": [{
                "quote": "the harbor",
                "prefix": "down to ",
                "suffix": " at noon",
                "kind": "question",
                "body": "why the harbor, of all places?"
            }]
        });
        let decision = parse_decision(&reply("leave_notes", input));
        let AgentDecision::Notes(drafts) = decision else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].quote, "the harbor");
        assert_eq!(drafts[0].prefix, "down to ");
        assert_eq!(drafts[0].suffix, " at noon");
        assert_eq!(drafts[0].kind, NoteKind::Question);
        assert_eq!(drafts[0].body, "why the harbor, of all places?");
    }

    #[test]
    fn notes_capped_at_three() {
        let input = json!({
            "register": "essay",
            "notes": [
                note("one", "insight", "a"),
                note("two", "insight", "b"),
                note("three", "insight", "c"),
                note("four", "insight", "d"),
            ]
        });
        let AgentDecision::Notes(drafts) = parse_decision(&reply("leave_notes", input)) else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 3);
        assert_eq!(drafts[0].quote, "one");
        assert_eq!(drafts[2].quote, "three");
    }

    #[test]
    fn empty_and_overlong_quotes_rejected() {
        let long_quote = "é".repeat(301);
        let ok_quote = "é".repeat(300);
        let input = json!({
            "register": "essay",
            "notes": [
                note("", "insight", "no quote"),
                note("   ", "insight", "blank quote"),
                note(&long_quote, "insight", "too long"),
                note(&ok_quote, "insight", "exactly at the cap"),
            ]
        });
        let AgentDecision::Notes(drafts) = parse_decision(&reply("leave_notes", input)) else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].quote, ok_quote);
    }

    #[test]
    fn degenerate_bodies_rejected() {
        assert!(looks_degenerate("hjhjhjhjhjhjhjhjhjhj"));
        assert!(looks_degenerate("aaaaaaaa"));
        assert!(looks_degenerate("abcabcabcabcabcabcabc"));
        assert!(!looks_degenerate("this might just be me, but the harbor line sings"));
        assert!(!looks_degenerate("what made it loud that night?"));
        assert!(!looks_degenerate(""));
        let input = json!({
            "register": "journal",
            "notes": [note("real quote", "insight", "hjhjhjhjhjhjhjhjhjhjhjhj")]
        });
        assert!(matches!(
            parse_decision(&reply("leave_notes", input)),
            AgentDecision::Pass { .. }
        ));
    }

    #[test]
    fn empty_body_rejected_and_unknown_kind_falls_back() {
        let input = json!({
            "register": "story",
            "notes": [
                note("real quote", "vibes", "kind is unknown"),
                note("another quote", "insight", ""),
            ]
        });
        let AgentDecision::Notes(drafts) = parse_decision(&reply("leave_notes", input)) else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, NoteKind::Insight);
    }

    #[test]
    fn bodiless_emoji_is_a_pure_reaction() {
        let input = json!({
            "register": "story",
            "notes": [{
                "quote": "the harbor",
                "prefix": "",
                "suffix": "",
                "kind": "insight",
                "emoji": "❤️"
            }]
        });
        let AgentDecision::Notes(drafts) = parse_decision(&reply("leave_notes", input)) else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].emoji.as_deref(), Some("❤️"));
        assert!(drafts[0].body.is_empty());
    }

    #[test]
    fn body_wins_over_a_stray_emoji() {
        // A note that carries both is a written note; the emoji is dropped so
        // it renders as a card, not a bodiless margin reaction.
        let input = json!({
            "register": "story",
            "notes": [{
                "quote": "the harbor",
                "prefix": "",
                "suffix": "",
                "kind": "question",
                "body": "why the harbor, of all places?",
                "emoji": "❤️"
            }]
        });
        let AgentDecision::Notes(drafts) = parse_decision(&reply("leave_notes", input)) else {
            panic!("expected notes");
        };
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].body, "why the harbor, of all places?");
        assert_eq!(drafts[0].emoji, None);
    }

    #[test]
    fn all_notes_invalid_degrades_to_pass() {
        let input = json!({ "register": "essay", "notes": [note("", "insight", "x")] });
        assert!(matches!(
            parse_decision(&reply("leave_notes", input)),
            AgentDecision::Pass { .. }
        ));
    }

    #[test]
    fn leave_notes_without_array_degrades_to_pass() {
        let decision = parse_decision(&reply("leave_notes", json!({ "notes": "not an array" })));
        assert!(matches!(decision, AgentDecision::Pass { .. }));
    }

    #[test]
    fn respond_parses_body_and_register() {
        let input =
            json!({ "register": "journal", "body": "That last line — you almost said it." });
        let decision = parse_decision(&reply("respond", input));
        assert_eq!(
            decision,
            AgentDecision::Respond {
                body: "That last line — you almost said it.".to_string(),
                register: Some("journal".to_string()),
            }
        );
    }

    #[test]
    fn respond_with_unknown_register_keeps_body() {
        let input = json!({ "register": "manifesto", "body": "still a response" });
        let decision = parse_decision(&reply("respond", input));
        assert_eq!(
            decision,
            AgentDecision::Respond {
                body: "still a response".to_string(),
                register: None,
            }
        );
    }

    #[test]
    fn respond_without_body_degrades_to_pass() {
        let decision = parse_decision(&reply("respond", json!({ "register": "journal" })));
        assert!(matches!(decision, AgentDecision::Pass { .. }));
        let decision = parse_decision(&reply("respond", json!({ "body": "   " })));
        assert!(matches!(decision, AgentDecision::Pass { .. }));
    }

    #[test]
    fn unknown_tool_degrades_to_pass() {
        let decision = parse_decision(&reply("write_essay_for_her", json!({})));
        let AgentDecision::Pass { reason } = decision else {
            panic!("expected pass");
        };
        assert!(reason.contains("write_essay_for_her"));
    }

    #[test]
    fn missing_tool_degrades_to_pass() {
        let decision = parse_decision(&ClaudeReply::default());
        assert!(matches!(decision, AgentDecision::Pass { .. }));
    }

    #[test]
    fn missing_tool_input_is_tolerated() {
        let r = ClaudeReply {
            tool_name: Some("pass".to_string()),
            ..ClaudeReply::default()
        };
        assert!(matches!(parse_decision(&r), AgentDecision::Pass { .. }));
    }
}
