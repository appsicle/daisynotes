//! Model output → [`ClaudeReply`] mapping.
//!
//! The grammar makes well-formed output overwhelmingly likely, but nothing
//! here trusts that: fences are stripped, the first balanced JSON object is
//! extracted, and anything unparseable degrades to a clean `pass` so the
//! agent's `parse_decision` always has something sensible to chew on.

use daisynotes_api::ClaudeReply;
use serde_json::Value;

/// Reason carried by the synthetic pass when output cannot be parsed.
const UNPARSEABLE: &str = "local model output unparseable";

/// Interpret raw generated text as one tool call.
pub fn reply_from_output(raw: &str) -> ClaudeReply {
    match extract_tool_call(raw) {
        Some((tool, input)) => ClaudeReply {
            text: None,
            tool_name: Some(tool),
            tool_input: Some(input),
            stop_reason: Some("tool_use".to_string()),
        },
        None => {
            tracing::warn!(len = raw.len(), "daisynotes-local: unparseable model output");
            synthetic_pass()
        }
    }
}

/// The clean `pass` produced when the model's output is unusable.
pub fn synthetic_pass() -> ClaudeReply {
    ClaudeReply {
        text: None,
        tool_name: Some("pass".to_string()),
        tool_input: Some(serde_json::json!({ "reason": UNPARSEABLE })),
        stop_reason: Some("tool_use".to_string()),
    }
}

/// Pull `(tool, rest-of-object)` out of the raw text, tolerating markdown
/// fences and surrounding prose.
fn extract_tool_call(raw: &str) -> Option<(String, Value)> {
    let value = first_json_object(raw)?;
    let Value::Object(mut map) = value else {
        return None;
    };
    let tool = match map.remove("tool") {
        Some(Value::String(tool)) if !tool.is_empty() => tool,
        _ => return None,
    };
    Some((tool, Value::Object(map)))
}

/// Find and parse the first balanced top-level JSON object in `raw`,
/// skipping markdown fences and any leading/trailing prose.
fn first_json_object(raw: &str) -> Option<Value> {
    let start = raw.find('{')?;
    let bytes = raw.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return serde_json::from_str(&raw[start..=i]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pass_maps_cleanly() {
        let reply = reply_from_output(r#"{"tool":"pass","reason":"mid-thought"}"#);
        assert_eq!(reply.tool_name.as_deref(), Some("pass"));
        assert_eq!(reply.tool_input, Some(json!({ "reason": "mid-thought" })));
        assert_eq!(reply.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(reply.text, None);
    }

    #[test]
    fn leave_notes_keeps_everything_but_the_tool_key() {
        let raw = r#"{"tool":"leave_notes","register":"essay","notes":[{"quote":"the harbor","prefix":"","suffix":" at noon","kind":"question","body":"why here?"}]}"#;
        let reply = reply_from_output(raw);
        assert_eq!(reply.tool_name.as_deref(), Some("leave_notes"));
        let input = reply.tool_input.expect("input");
        assert_eq!(input["register"], "essay");
        assert_eq!(input["notes"][0]["quote"], "the harbor");
        assert_eq!(input["notes"][0]["kind"], "question");
        assert!(input.get("tool").is_none());
    }

    #[test]
    fn respond_maps_cleanly() {
        let raw = r#"{"tool":"respond","register":"journal","body":"That line stayed with me."}"#;
        let reply = reply_from_output(raw);
        assert_eq!(reply.tool_name.as_deref(), Some("respond"));
        assert_eq!(
            reply.tool_input,
            Some(json!({ "register": "journal", "body": "That line stayed with me." }))
        );
    }

    #[test]
    fn markdown_fences_and_prose_are_tolerated() {
        let raw = "Sure! Here's my decision:\n```json\n{\"tool\":\"pass\",\"reason\":\"ok\"}\n```\nthanks";
        let reply = reply_from_output(raw);
        assert_eq!(reply.tool_name.as_deref(), Some("pass"));
        assert_eq!(reply.tool_input, Some(json!({ "reason": "ok" })));
    }

    #[test]
    fn braces_inside_strings_do_not_confuse_the_scanner() {
        let raw = r#"{"tool":"respond","register":"story","body":"a brace } and { quote \" inside"}"#;
        let reply = reply_from_output(raw);
        assert_eq!(reply.tool_name.as_deref(), Some("respond"));
        let input = reply.tool_input.expect("input");
        assert_eq!(input["body"], "a brace } and { quote \" inside");
    }

    #[test]
    fn malformed_output_becomes_a_synthetic_pass() {
        for raw in [
            "",
            "no json here",
            "{ broken",
            r#"{"reason":"missing tool key"}"#,
            r#"{"tool":42}"#,
            r#"[1,2,3]"#,
        ] {
            let reply = reply_from_output(raw);
            assert_eq!(reply.tool_name.as_deref(), Some("pass"), "raw: {raw:?}");
            let input = reply.tool_input.expect("input");
            assert_eq!(input["reason"], UNPARSEABLE);
            assert_eq!(reply.stop_reason.as_deref(), Some("tool_use"));
        }
    }
}
