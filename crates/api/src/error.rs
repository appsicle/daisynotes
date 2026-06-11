//! Typed errors at the crate boundary.

use thiserror::Error;

/// Errors surfaced by the Claude client.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApiError {
    /// No usable API key (none in env/Keychain, or the server rejected it
    /// with HTTP 401).
    #[error("no usable Anthropic API key")]
    MissingKey,
    /// Transport-level failure: DNS, TLS, connect, timeout, reset.
    #[error("network error: {0}")]
    Network(String),
    /// The API answered with a non-success status other than 401.
    #[error("api error {status}: {message}")]
    Api {
        /// HTTP status code returned by the API.
        status: u16,
        /// `error.message` from the response body when parseable, otherwise
        /// the raw body or a generic `HTTP <status>` placeholder.
        message: String,
    },
    /// A 2xx response body could not be understood.
    #[error("parse error: {0}")]
    Parse(String),
    /// The api worker thread is gone; the request can never complete.
    #[error("api channel closed")]
    Channel,
}
