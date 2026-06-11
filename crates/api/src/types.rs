//! Public request/reply value types for the Claude Messages API.
//!
//! [`ClaudeRequest`] serializes 1:1 into the wire body of
//! `POST /v1/messages`; [`ClaudeReply`] is the distilled, app-facing shape of
//! a response (joined text, first tool call, stop reason).

use serde::Serialize;

/// Default Claude model used by Muse.
pub const DEFAULT_MODEL: &str = "claude-fable-5";

/// Who authored a [`ChatMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The writer (or the harness speaking on her behalf).
    User,
    /// A prior reply from the model.
    Assistant,
}

/// One turn of conversation history sent to the Messages API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    /// Author of this turn.
    pub role: Role,
    /// Plain-text content of this turn.
    pub content: String,
}

/// A complete, self-contained request against the Claude Messages API.
///
/// Serializes directly into the `POST /v1/messages` body. `tools` and
/// `tool_choice` are forwarded to the API verbatim; optional fields are
/// omitted from the body entirely when `None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaudeRequest {
    /// Model id, e.g. [`DEFAULT_MODEL`].
    pub model: String,
    /// Hard cap on generated tokens.
    pub max_tokens: u32,
    /// System prompt. Always sent as a string (may be empty).
    pub system: String,
    /// Conversation turns, oldest first; the first must be a user turn.
    pub messages: Vec<ChatMessage>,
    /// Tool definitions (a JSON array), forwarded verbatim when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    /// Tool-choice directive (a JSON object), forwarded verbatim when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Sampling temperature; omitted from the body when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl Default for ClaudeRequest {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: 1024,
            system: String::new(),
            messages: Vec::new(),
            tools: None,
            tool_choice: None,
            temperature: None,
        }
    }
}

/// Distilled result of one Messages API call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaudeReply {
    /// All `text` blocks from the response, joined with `"\n"`; `None` when
    /// the model produced no prose.
    pub text: Option<String>,
    /// Name of the first `tool_use` block, if any.
    pub tool_name: Option<String>,
    /// Input of the first `tool_use` block, if any.
    pub tool_input: Option<serde_json::Value>,
    /// Why generation stopped (e.g. `"end_turn"`, `"tool_use"`).
    pub stop_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_body_matches_wire_shape_exactly() {
        let req = ClaudeRequest {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: 1024,
            system: "You are Muse.".to_string(),
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: "Hello".to_string(),
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: "Hi".to_string(),
                },
            ],
            tools: Some(json!([
                {
                    "name": "pass",
                    "description": "Choose silence.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "reason": { "type": "string" } },
                        "required": ["reason"]
                    }
                }
            ])),
            tool_choice: Some(json!({ "type": "any" })),
            temperature: Some(0.5),
        };

        let body = serde_json::to_value(&req).expect("request must serialize");
        let golden = json!({
            "model": "claude-fable-5",
            "max_tokens": 1024,
            "system": "You are Muse.",
            "messages": [
                { "role": "user", "content": "Hello" },
                { "role": "assistant", "content": "Hi" }
            ],
            "tools": [
                {
                    "name": "pass",
                    "description": "Choose silence.",
                    "input_schema": {
                        "type": "object",
                        "properties": { "reason": { "type": "string" } },
                        "required": ["reason"]
                    }
                }
            ],
            "tool_choice": { "type": "any" },
            "temperature": 0.5
        });
        assert_eq!(body, golden);
    }

    #[test]
    fn optional_fields_are_omitted_when_none() {
        let req = ClaudeRequest {
            system: "sys".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hi".to_string(),
            }],
            ..ClaudeRequest::default()
        };
        let body = serde_json::to_value(&req).expect("request must serialize");
        let golden = json!({
            "model": "claude-fable-5",
            "max_tokens": 1024,
            "system": "sys",
            "messages": [{ "role": "user", "content": "hi" }]
        });
        assert_eq!(body, golden);
        let object = body.as_object().expect("body must be an object");
        assert!(!object.contains_key("tools"));
        assert!(!object.contains_key("tool_choice"));
        assert!(!object.contains_key("temperature"));
    }
}
