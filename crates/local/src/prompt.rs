//! Gemma 3 chat-format prompt assembly.
//!
//! Gemma has no system role, so the system prompt is folded into the single
//! user turn, followed by the conversation content and a strict output
//! instruction. The template tokens match llama.cpp's `gemma` chat template:
//! `<start_of_turn>user\n…<end_of_turn>\n<start_of_turn>model\n` (the `<bos>`
//! token is added by the tokenizer, not by us).

use daisynotes_api::{ClaudeRequest, Role};

/// Soft cap on prompt size in characters. Gemma's tokenizer averages well
/// over 3 chars/token on English prose, so ~24k chars stays comfortably
/// inside an 8192-token context with ~450 tokens reserved for generation.
pub const MAX_PROMPT_CHARS: usize = 24_000;

/// Marker spliced into the middle of an over-long entry.
const ELLIPSIS: &str = "\n[…]\n";

/// The instruction that pins the model to exactly one tool-call JSON object.
const OUTPUT_INSTRUCTION: &str = r#"Decide now. Answer with EXACTLY ONE JSON object and nothing else — no prose, no markdown fences. You add brief factual notes only: facts, definitions, corrections, sources, factual questions. No praise, no warmth, no opinions. leave_notes is your default. It must be one of these three shapes:

{"tool":"leave_notes","register":"<essay|journal|story|math|letter|notes>","notes":[{"quote":"<exact characters copied from the entry, a short phrase — never a whole paragraph>","prefix":"<up to 20 chars before the quote, or empty>","suffix":"<up to 20 chars after the quote, or empty>","kind":"<insight|question|correction|reference>","body":"<one fact, correction, definition, source, or factual question — a fragment or one short sentence>"}]}

{"tool":"respond","register":"<essay|journal|story|math|letter|notes>","body":"<one short factual paragraph about the whole entry>"}

{"tool":"pass","reason":"<one short line on why there is nothing to add>"}

Choose leave_notes whenever there is information to add — a fact, a correction, a definition, a source, or a factual question. Use respond only when the information is about the whole entry. Use pass only when the page is nearly empty, mid-keystroke, or there is nothing to correct, define, source, or extend.

Rules: one to three notes; quotes must appear verbatim in the entry and stay short; every note is terse — a fragment or one short sentence. Information only — no praise, no encouragement, no opinions about the writing, no emotion. When a pronoun is unavoidable, use "you" — never "she", "her", or "the writer"."#;

/// Build the full Gemma-format prompt string for one request.
///
/// The system prompt and the user message(s) are folded into one user turn;
/// if the result would blow past [`MAX_PROMPT_CHARS`], the middle of the
/// `<entry>…</entry>` fence inside the user content is cut out and replaced
/// with `[…]`.
pub fn build_prompt(req: &ClaudeRequest) -> String {
    let mut user_content = String::new();
    for message in &req.messages {
        if !user_content.is_empty() {
            user_content.push_str("\n\n");
        }
        match message.role {
            Role::User => user_content.push_str(&message.content),
            // The pipeline sends a single user turn in practice; if history
            // ever appears, label assistant turns so they read coherently.
            Role::Assistant => {
                user_content.push_str("(you said earlier:) ");
                user_content.push_str(&message.content);
            }
        }
    }

    let overhead = req.system.chars().count()
        + OUTPUT_INSTRUCTION.chars().count()
        + 64; // template tokens + joiners
    let budget = MAX_PROMPT_CHARS.saturating_sub(overhead);
    let user_content = truncate_entry_middle(&user_content, budget);

    let mut out = String::with_capacity(req.system.len() + user_content.len() + 1024);
    out.push_str("<start_of_turn>user\n");
    out.push_str(&req.system);
    out.push_str("\n\n");
    out.push_str(&user_content);
    out.push_str("\n\n");
    out.push_str(OUTPUT_INSTRUCTION);
    out.push_str("<end_of_turn>\n<start_of_turn>model\n");
    out
}

/// If `content` exceeds `budget` chars, cut characters out of the middle of
/// the `<entry>…</entry>` fence (keeping head and tail) and splice in
/// `[…]`. Falls back to truncating the middle of the whole content when no
/// fence is present. Always cuts on char boundaries.
pub fn truncate_entry_middle(content: &str, budget: usize) -> String {
    let len = content.chars().count();
    if len <= budget {
        return content.to_string();
    }
    let excess = len - budget + ELLIPSIS.chars().count();

    let (open, close) = ("<entry>\n", "\n</entry>");
    if let (Some(start), Some(end)) = (content.find(open), content.rfind(close)) {
        let entry_start = start + open.len();
        if entry_start < end {
            let entry = &content[entry_start..end];
            let entry_len = entry.chars().count();
            if entry_len > excess + 64 {
                let keep = entry_len - excess;
                let head: String = entry.chars().take(keep / 2).collect();
                let tail: String = entry.chars().skip(entry_len - (keep - keep / 2)).collect();
                let mut out = String::with_capacity(content.len());
                out.push_str(&content[..entry_start]);
                out.push_str(&head);
                out.push_str(ELLIPSIS);
                out.push_str(&tail);
                out.push_str(&content[end..]);
                return out;
            }
        }
    }

    // No fence (or the fence is too small to absorb the cut): truncate the
    // middle of the whole content instead.
    let keep = budget.saturating_sub(ELLIPSIS.chars().count()).max(2);
    let head: String = content.chars().take(keep / 2).collect();
    let tail: String = content.chars().skip(len - (keep - keep / 2)).collect();
    format!("{head}{ELLIPSIS}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use daisynotes_api::ChatMessage;

    fn request(system: &str, user: &str) -> ClaudeRequest {
        ClaudeRequest {
            system: system.to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: user.to_string(),
            }],
            ..ClaudeRequest::default()
        }
    }

    #[test]
    fn template_tokens_in_the_right_places() {
        let req = request("You live in the margin.", "Here is the entry.");
        let prompt = build_prompt(&req);
        assert!(prompt.starts_with("<start_of_turn>user\n"));
        assert!(prompt.ends_with("<end_of_turn>\n<start_of_turn>model\n"));
        // The system prompt is folded in before the user content.
        let sys = prompt.find("You live in the margin.").expect("system");
        let user = prompt.find("Here is the entry.").expect("user");
        let instruction = prompt.find("EXACTLY ONE JSON object").expect("instruction");
        assert!(sys < user && user < instruction);
        // Exactly one user turn, no <bos> in the text (tokenizer adds it).
        assert_eq!(prompt.matches("<start_of_turn>user").count(), 1);
        assert!(!prompt.contains("<bos>"));
    }

    #[test]
    fn instruction_names_all_three_tools() {
        let prompt = build_prompt(&request("s", "u"));
        for tool in ["\"pass\"", "\"leave_notes\"", "\"respond\""] {
            assert!(prompt.contains(tool), "missing {tool}");
        }
    }

    #[test]
    fn short_prompts_are_untouched() {
        let user = "before <entry>\nshort entry\n</entry> after".to_string();
        let req = request("sys", &user);
        let prompt = build_prompt(&req);
        assert!(prompt.contains("short entry"));
        assert!(!prompt.contains("[…]"));
    }

    #[test]
    fn long_entries_lose_their_middle_not_their_edges() {
        let body = "x".repeat(60_000);
        let user = format!("intro\n<entry>\nHEAD {body} TAIL\n</entry>\noutro");
        let req = request("sys", &user);
        let prompt = build_prompt(&req);
        assert!(prompt.chars().count() <= MAX_PROMPT_CHARS + 200);
        assert!(prompt.contains("HEAD"));
        assert!(prompt.contains("TAIL"));
        assert!(prompt.contains("[…]"));
        // The fence and the surrounding context survive.
        assert!(prompt.contains("intro\n<entry>\n"));
        assert!(prompt.contains("\n</entry>\noutro"));
    }

    #[test]
    fn truncation_is_char_boundary_safe() {
        let body = "é🦀".repeat(20_000);
        let out = truncate_entry_middle(&format!("<entry>\n{body}\n</entry>"), 1000);
        assert!(out.chars().count() <= 1010);
        assert!(out.contains("[…]"));
    }

    #[test]
    fn content_without_fence_still_truncates() {
        let content = "y".repeat(5000);
        let out = truncate_entry_middle(&content, 100);
        assert!(out.chars().count() <= 100);
        assert!(out.contains("[…]"));
        assert!(out.starts_with('y') && out.ends_with('y'));
    }
}
