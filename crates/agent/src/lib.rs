//! daisynotes-agent — the companion's brain: trigger heuristics, register-aware
//! prompts, decision parsing, quote anchoring, and dismissal memory.
//!
//! What this crate owns:
//! - [`TriggerEngine`]: zero-cost local heuristics deciding *when* a
//!   consideration should run (idle, accumulated delta, hourly budgets);
//! - [`build_request`]: turning a [`DocSnapshot`] into the one Claude call
//!   that does everything (the system prompt here *is* the product voice);
//! - [`parse_decision`]: tolerantly reading the model's single tool call
//!   back into an [`AgentDecision`];
//! - [`locate_quote`] / [`dismissal_digest`]: anchoring note quotes to byte
//!   ranges and remembering what the writer dismissed.
//!
//! What it must not know about: gpui, pixels, timers, threads, or the
//! network. All time is injected as `now_ms`; all I/O belongs to the app and
//! the api crate. Everything here is unit-testable without a window.

mod anchor;
mod decision;
mod prompts;
mod trigger;
mod types;

pub use anchor::{dismissal_digest, locate_quote};
pub use decision::parse_decision;
pub use prompts::build_request;
pub use trigger::{Chattiness, TriggerEngine};
pub use types::{AgentDecision, DocSnapshot, NoteDraft, NoteKind, NoteRecord, REGISTERS};
