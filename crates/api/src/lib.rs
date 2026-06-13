//! daisynotes-api — Claude Messages API client on a dedicated tokio thread.
//! Talks to the app exclusively through channels. Zero UI dependencies.
//!
//! What this crate owns:
//! - the wire protocol for `POST https://api.anthropic.com/v1/messages`
//!   (non-streaming): request serialization, response parsing, error mapping,
//!   the 60s timeout, and the single-retry policy for transient statuses;
//! - the `"daisynotes-api"` worker thread and the tokio runtime that lives on it;
//! - API-key resolution (`ANTHROPIC_API_KEY` env var, then macOS Keychain).
//!
//! What it must not know about: gpui, documents, prompts, or any UI state.
//! Callers obtain an [`ApiHandle`] via [`spawn`], submit a [`ClaudeRequest`],
//! and await the returned `futures` oneshot receiver on their own executor.
//! A receiver handed out by [`ApiHandle::request`] always resolves — if the
//! worker thread is gone it resolves to `Err(ApiError::Channel)`, never hangs.
//!
//! Privacy invariant: this crate never logs message content or API keys; only
//! models, counts, and statuses reach `tracing`.

mod error;
mod handle;
mod keys;
mod types;
mod wire;

pub use error::ApiError;
pub use handle::{ApiHandle, spawn};
pub use keys::{resolve_api_key, store_api_key};
pub use types::{ChatMessage, ClaudeReply, ClaudeRequest, DEFAULT_MODEL, Role};
