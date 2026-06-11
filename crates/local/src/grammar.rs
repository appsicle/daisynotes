//! GBNF grammar pinning generation to exactly one tool-call JSON object.
//!
//! Mirrors the three tool schemas in `crates/agent/src/prompts.rs`: `pass`,
//! `leave_notes` (1–2 notes), `respond`. Register and kind enums are
//! hardcoded to match `muse_agent::types::REGISTERS` and the note kinds.

/// Root rule name passed to `LlamaSampler::grammar`.
pub const GRAMMAR_ROOT: &str = "root";

/// The grammar itself. JSON strings allow standard escapes; whitespace is
/// fixed (no gaps) to keep the smallest models on rails.
pub const GRAMMAR: &str = r#"
root ::= pass | leavenotes | respond

pass ::= "{\"tool\":\"pass\",\"reason\":" reason "}"

leavenotes ::= "{\"tool\":\"leave_notes\",\"register\":" register ",\"notes\":[" note ("," note)? "]}"

note ::= "{\"quote\":" quote ",\"prefix\":" context ",\"suffix\":" context ",\"kind\":" kind ",\"body\":" body ("," "\"emoji\":" emoji)? "}"

emoji ::= "\"❗\"" | "\"😄\"" | "\"😂\"" | "\"❤️\""

respond ::= "{\"tool\":\"respond\",\"register\":" register ",\"body\":" longbody "}"

register ::= "\"essay\"" | "\"journal\"" | "\"story\"" | "\"math\"" | "\"letter\"" | "\"notes\""

kind ::= "\"insight\"" | "\"question\"" | "\"encouragement\"" | "\"correction\"" | "\"reference\""

reason ::= "\"" char{1,160} "\""

quote ::= "\"" char{1,300} "\""

context ::= "\"" char{0,24} "\""

body ::= "\"" char{0,300} "\""

longbody ::= "\"" char{1,640} "\""

char ::= [^"\\\x7F\x00-\x1F] | "\\" (["\\bfnrt] | "u" hex hex hex hex)

hex ::= [0-9a-fA-F]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_referenced_is_defined() {
        let defined: Vec<&str> = GRAMMAR
            .lines()
            .filter_map(|l| l.split_once("::=").map(|(name, _)| name.trim()))
            .collect();
        for rule in [
            "root",
            "pass",
            "leavenotes",
            "respond",
            "note",
            "register",
            "kind",
            "emoji",
            "reason",
            "quote",
            "context",
            "body",
            "longbody",
            "char",
            "hex",
        ] {
            assert!(defined.contains(&rule), "rule {rule} missing");
        }
        assert!(defined.contains(&GRAMMAR_ROOT));
    }

    #[test]
    fn grammar_covers_all_tools_registers_and_kinds() {
        for needle in [
            r#"\"pass\""#,
            r#"\"leave_notes\""#,
            r#"\"respond\""#,
            r#"\"essay\""#,
            r#"\"journal\""#,
            r#"\"story\""#,
            r#"\"math\""#,
            r#"\"letter\""#,
            r#"\"notes\""#,
            r#"\"insight\""#,
            r#"\"question\""#,
            r#"\"encouragement\""#,
            r#"\"correction\""#,
            r#"\"reference\""#,
        ] {
            assert!(GRAMMAR.contains(needle), "grammar missing {needle}");
        }
    }

    #[test]
    fn notes_are_capped_at_two() {
        // One mandatory note plus at most one optional repeat.
        assert!(GRAMMAR.contains(r#"note ("," note)?"#));
    }

    #[test]
    fn grammar_has_no_nul_bytes_and_is_ascii_clean_enough() {
        assert!(!GRAMMAR.contains('\0'));
        assert!(GRAMMAR.trim().starts_with("root ::="));
    }
}
