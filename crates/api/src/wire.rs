//! Wire-format details of the Messages API: endpoint constants, response
//! parsing, and HTTP status → [`ApiError`] mapping.
//!
//! Everything here is pure (no I/O) so it can be tested against fixtures
//! without touching the network.

use serde::Deserialize;

use crate::error::ApiError;
use crate::types::ClaudeReply;

/// Messages API endpoint.
pub(crate) const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";

/// Pinned `anthropic-version` header value.
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Separator used when a response carries multiple text blocks.
const TEXT_JOIN: &str = "\n";

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    content: Vec<WireBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum WireBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    // Future block kinds (e.g. "thinking") must not break parsing.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireErrorEnvelope {
    #[serde(default)]
    error: Option<WireErrorDetail>,
}

#[derive(Deserialize)]
struct WireErrorDetail {
    #[serde(default)]
    message: Option<String>,
}

/// Parses a 2xx Messages API body into a [`ClaudeReply`].
///
/// Text blocks are collected in order and joined with `"\n"`; the FIRST
/// `tool_use` block (if any) supplies `tool_name`/`tool_input`. Unknown block
/// kinds are ignored.
pub(crate) fn parse_reply(body: &str) -> Result<ClaudeReply, ApiError> {
    let wire: WireResponse =
        serde_json::from_str(body).map_err(|err| ApiError::Parse(err.to_string()))?;

    let mut texts: Vec<&str> = Vec::new();
    let mut tool_name = None;
    let mut tool_input = None;
    for block in &wire.content {
        match block {
            WireBlock::Text { text } => texts.push(text),
            WireBlock::ToolUse { name, input } => {
                if tool_name.is_none() {
                    tool_name = Some(name.clone());
                    tool_input = Some(input.clone());
                }
            }
            WireBlock::Other => {}
        }
    }

    let text = if texts.is_empty() {
        None
    } else {
        Some(texts.join(TEXT_JOIN))
    };

    Ok(ClaudeReply {
        text,
        tool_name,
        tool_input,
        stop_reason: wire.stop_reason,
    })
}

/// Whether a non-success status is worth exactly one retry (429/529/5xx).
pub(crate) fn is_retryable(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// Maps a non-2xx status + body to an [`ApiError`].
///
/// 401 becomes [`ApiError::MissingKey`]; everything else becomes
/// [`ApiError::Api`] carrying `error.message` from the body when parseable,
/// falling back to the raw body, then to `HTTP <status>`.
pub(crate) fn status_error(status: u16, body: &str) -> ApiError {
    if status == 401 {
        return ApiError::MissingKey;
    }
    let message = serde_json::from_str::<WireErrorEnvelope>(body)
        .ok()
        .and_then(|envelope| envelope.error)
        .and_then(|detail| detail.message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                format!("HTTP {status}")
            } else {
                trimmed.to_string()
            }
        });
    ApiError::Api { status, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MIXED_FIXTURE: &str = r#"{
        "id": "msg_01ABC",
        "type": "message",
        "role": "assistant",
        "model": "claude-fable-5",
        "content": [
            { "type": "text", "text": "Reading this passage," },
            {
                "type": "tool_use",
                "id": "toolu_01",
                "name": "leave_notes",
                "input": {
                    "notes": [
                        { "quote": "the sea was glass", "kind": "insight", "body": "This image carries the whole paragraph." }
                    ]
                }
            },
            { "type": "text", "text": "that's all." },
            { "type": "tool_use", "id": "toolu_02", "name": "respond", "input": { "body": "ignored" } },
            { "type": "thinking", "thinking": "unknown block kinds are skipped" }
        ],
        "stop_reason": "tool_use",
        "usage": { "input_tokens": 12, "output_tokens": 34 }
    }"#;

    #[test]
    fn parses_mixed_text_and_tool_use_blocks() {
        let reply = parse_reply(MIXED_FIXTURE).expect("fixture must parse");
        assert_eq!(
            reply.text.as_deref(),
            Some("Reading this passage,\nthat's all.")
        );
        assert_eq!(reply.tool_name.as_deref(), Some("leave_notes"));
        assert_eq!(
            reply.tool_input,
            Some(json!({
                "notes": [
                    { "quote": "the sea was glass", "kind": "insight", "body": "This image carries the whole paragraph." }
                ]
            }))
        );
        assert_eq!(reply.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn tool_only_response_has_no_text() {
        let body = r#"{
            "content": [
                { "type": "tool_use", "id": "toolu_01", "name": "pass", "input": { "reason": "nothing to add" } }
            ],
            "stop_reason": "tool_use"
        }"#;
        let reply = parse_reply(body).expect("fixture must parse");
        assert_eq!(reply.text, None);
        assert_eq!(reply.tool_name.as_deref(), Some("pass"));
        assert_eq!(
            reply.tool_input,
            Some(json!({ "reason": "nothing to add" }))
        );
    }

    #[test]
    fn text_only_response_has_no_tool() {
        let body = r#"{
            "content": [ { "type": "text", "text": "Just words." } ],
            "stop_reason": "end_turn"
        }"#;
        let reply = parse_reply(body).expect("fixture must parse");
        assert_eq!(reply.text.as_deref(), Some("Just words."));
        assert_eq!(reply.tool_name, None);
        assert_eq!(reply.tool_input, None);
        assert_eq!(reply.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn malformed_body_is_a_parse_error() {
        let err = parse_reply("not json").expect_err("must fail");
        assert!(matches!(err, ApiError::Parse(_)));
    }

    #[test]
    fn error_fixture_maps_to_api_error_with_message() {
        let body = r#"{
            "type": "error",
            "error": { "type": "overloaded_error", "message": "Overloaded" },
            "request_id": "req_011CSHo"
        }"#;
        assert_eq!(
            status_error(529, body),
            ApiError::Api {
                status: 529,
                message: "Overloaded".to_string(),
            }
        );
    }

    #[test]
    fn http_401_maps_to_missing_key() {
        let body = r#"{ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } }"#;
        assert_eq!(status_error(401, body), ApiError::MissingKey);
    }

    #[test]
    fn unparseable_error_body_falls_back_to_raw_body() {
        assert_eq!(
            status_error(500, "boom"),
            ApiError::Api {
                status: 500,
                message: "boom".to_string(),
            }
        );
    }

    #[test]
    fn empty_error_body_falls_back_to_status_placeholder() {
        assert_eq!(
            status_error(503, "  "),
            ApiError::Api {
                status: 503,
                message: "HTTP 503".to_string(),
            }
        );
    }

    #[test]
    fn retry_policy_covers_429_529_and_5xx_only() {
        assert!(is_retryable(429));
        assert!(is_retryable(500));
        assert!(is_retryable(529));
        assert!(is_retryable(599));
        assert!(!is_retryable(200));
        assert!(!is_retryable(400));
        assert!(!is_retryable(401));
        assert!(!is_retryable(404));
    }
}
