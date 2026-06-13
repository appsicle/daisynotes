//! The `"daisynotes-api"` worker thread and the [`ApiHandle`] the app talks to.
//!
//! One std thread owns a 2-worker tokio runtime and drains an mpsc command
//! queue; each command becomes a concurrent task that performs one HTTP call
//! and answers over a `futures` oneshot channel.

use std::time::Duration;

use futures::channel::oneshot;
use tokio::sync::mpsc;

use crate::error::ApiError;
use crate::keys::resolve_api_key;
use crate::types::{ClaudeReply, ClaudeRequest};
use crate::wire;

/// Whole-request timeout for one HTTP attempt.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Pause before the single retry on a transient status.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// A cheap-to-clone, thread-safe handle to the `"daisynotes-api"` worker thread.
///
/// Obtain one via [`spawn`] (or [`ApiHandle::spawn`]); clones share the same
/// worker. The worker shuts down on its own once every handle is dropped.
#[derive(Clone)]
pub struct ApiHandle {
    tx: mpsc::UnboundedSender<Command>,
}

/// Starts the dedicated `"daisynotes-api"` thread and returns a handle to it.
///
/// Convenience alias for [`ApiHandle::spawn`].
pub fn spawn() -> ApiHandle {
    ApiHandle::spawn()
}

impl ApiHandle {
    /// Starts a std thread named `"daisynotes-api"` running a tokio multi-thread
    /// runtime (2 workers) with an mpsc command loop, and returns the handle.
    ///
    /// Never panics: if the thread cannot be spawned, every subsequent
    /// [`ApiHandle::request`] resolves to `Err(ApiError::Channel)`.
    pub fn spawn() -> ApiHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let thread = std::thread::Builder::new().name("daisynotes-api".to_string());
        if let Err(err) = thread.spawn(move || worker_main(rx)) {
            tracing::error!(%err, "daisynotes-api: failed to spawn worker thread");
        }
        ApiHandle { tx }
    }

    /// Submits a request to the worker thread.
    ///
    /// The returned receiver is awaited by the caller on its own executor and
    /// always resolves: with the reply, with a typed [`ApiError`], or with
    /// `Err(ApiError::Channel)` if the worker thread is gone. It never hangs.
    pub fn request(&self, req: ClaudeRequest) -> oneshot::Receiver<Result<ClaudeReply, ApiError>> {
        let (tx, rx) = oneshot::channel();
        let command = Command {
            req,
            reply: ReplyGuard(Some(tx)),
        };
        if let Err(unsent) = self.tx.send(command) {
            tracing::warn!("daisynotes-api: worker thread is gone; failing request");
            unsent.0.reply.fulfill(Err(ApiError::Channel));
        }
        rx
    }
}

struct Command {
    req: ClaudeRequest,
    reply: ReplyGuard,
}

/// Carrier for the caller's oneshot sender.
///
/// Invariant: a receiver handed out by [`ApiHandle::request`] always
/// resolves. If this guard is dropped before [`ReplyGuard::fulfill`] — the
/// worker died, the runtime aborted the task, or the queue was discarded —
/// `Drop` converts the would-be oneshot cancellation into
/// `Err(ApiError::Channel)`.
struct ReplyGuard(Option<oneshot::Sender<Result<ClaudeReply, ApiError>>>);

impl ReplyGuard {
    fn fulfill(mut self, result: Result<ClaudeReply, ApiError>) {
        if let Some(tx) = self.0.take() {
            // The caller may have dropped the receiver; that's fine.
            let _ = tx.send(result);
        }
    }
}

impl Drop for ReplyGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(Err(ApiError::Channel));
        }
    }
}

fn worker_main(mut rx: mpsc::UnboundedReceiver<Command>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("daisynotes-api-worker")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            // Dropping `rx` resolves every queued and future request to
            // Err(ApiError::Channel) via ReplyGuard.
            tracing::error!(%err, "daisynotes-api: failed to build tokio runtime");
            return;
        }
    };

    runtime.block_on(async move {
        let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
            Ok(client) => client,
            Err(err) => {
                tracing::error!(%err, "daisynotes-api: failed to build http client");
                while let Some(command) = rx.recv().await {
                    command.reply.fulfill(Err(ApiError::Network(format!(
                        "http client unavailable: {err}"
                    ))));
                }
                return;
            }
        };

        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                command = rx.recv() => match command {
                    Some(Command { req, reply }) => {
                        let client = client.clone();
                        tasks.spawn(async move {
                            let result = perform(&client, req).await;
                            reply.fulfill(result);
                        });
                    }
                    None => break,
                },
                Some(joined) = tasks.join_next(), if !tasks.is_empty() => {
                    if let Err(err) = joined {
                        tracing::error!(%err, "daisynotes-api: request task failed");
                    }
                }
            }
        }
        // Every handle is gone; let in-flight requests finish before the
        // runtime (and their reply guards) are torn down.
        while let Some(joined) = tasks.join_next().await {
            if let Err(err) = joined {
                tracing::error!(%err, "daisynotes-api: request task failed");
            }
        }
    });
}

/// Performs one request: resolve key, POST, retry once on 429/529/5xx.
async fn perform(client: &reqwest::Client, req: ClaudeRequest) -> Result<ClaudeReply, ApiError> {
    // Key resolution happens per request so a key set after launch (env at
    // spawn time, or Keychain at any time) works without a restart. The
    // `security` subprocess blocks, so it runs off the async workers.
    let key = tokio::task::spawn_blocking(resolve_api_key)
        .await
        .map_err(|err| ApiError::Network(format!("key lookup failed: {err}")))?;
    let Some(key) = key else {
        return Err(ApiError::MissingKey);
    };

    tracing::debug!(
        model = %req.model,
        turns = req.messages.len(),
        has_tools = req.tools.is_some(),
        "daisynotes-api: dispatching request"
    );

    let mut first_attempt = true;
    loop {
        let response = client
            .post(wire::MESSAGES_URL)
            .header("x-api-key", &key)
            .header("anthropic-version", wire::ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(%err, "daisynotes-api: network error");
                return Err(ApiError::Network(err.to_string()));
            }
        };

        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            let body = response
                .text()
                .await
                .map_err(|err| ApiError::Network(err.to_string()))?;
            let reply = wire::parse_reply(&body)?;
            tracing::debug!(
                stop_reason = reply.stop_reason.as_deref().unwrap_or("none"),
                tool = reply.tool_name.as_deref().unwrap_or("none"),
                "daisynotes-api: reply received"
            );
            return Ok(reply);
        }

        if first_attempt && wire::is_retryable(status) {
            first_attempt = false;
            tracing::debug!(status, "daisynotes-api: transient status, retrying once");
            tokio::time::sleep(RETRY_DELAY).await;
            continue;
        }

        let body = response.text().await.unwrap_or_default();
        let err = wire::status_error(status, &body);
        tracing::warn!(status, "daisynotes-api: api error");
        return Err(err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_traits<T: Clone + Send + Sync>() {}

    #[test]
    fn handle_is_clone_send_sync() {
        assert_traits::<ApiHandle>();
    }

    #[test]
    fn request_resolves_to_channel_error_when_worker_is_gone() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx); // simulate a dead worker thread
        let handle = ApiHandle { tx };
        let receiver = handle.request(ClaudeRequest::default());
        let result = futures::executor::block_on(receiver).expect("receiver must resolve");
        assert_eq!(result, Err(ApiError::Channel));
    }

    #[test]
    fn dropped_reply_guard_resolves_to_channel_error() {
        let (tx, rx) = oneshot::channel();
        drop(ReplyGuard(Some(tx)));
        let result = futures::executor::block_on(rx).expect("receiver must resolve");
        assert_eq!(result, Err(ApiError::Channel));
    }

    #[test]
    fn spawn_returns_a_usable_handle_without_network() {
        // No request is sent: this only verifies the thread + runtime come up
        // and wind down cleanly when the last handle drops.
        let handle = spawn();
        let clone = handle.clone();
        drop(handle);
        drop(clone);
    }
}
