//! The `"muse-local"` worker thread: a tokio runtime hosting the command
//! loop (inference, serialized) and download tasks (streamed off the loop).
//!
//! Inference runs inline in the command loop — one consideration at a time,
//! which the upstream poll cadence expects. Downloads are spawned onto the
//! runtime's worker threads, so they keep streaming while inference runs.

use std::sync::Arc;

use futures::channel::oneshot;
use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use muse_api::{ClaudeReply, ClaudeRequest};

use crate::engine::Engine;
use crate::{DownloadState, LocalError, LocalModel, installed_model, model_path, models_dir};
use crate::{prompt, reply};

/// Update the shared download state roughly every this many bytes.
const PROGRESS_STRIDE: u64 = 2 * 1024 * 1024;

/// What the handle asks of the worker.
pub enum Command {
    /// Run one consideration.
    Infer {
        /// The request (boxed: it carries the whole entry).
        req: Box<ClaudeRequest>,
        /// Where the answer goes; resolves even if the worker dies.
        reply: ReplyGuard,
    },
    /// Download a model in the background.
    Download {
        /// Which model.
        model: LocalModel,
    },
}

/// Carrier for the caller's oneshot sender (mirrors muse-api's ReplyGuard).
///
/// Invariant: a receiver handed out by `LocalHandle::request` always
/// resolves. If this guard is dropped before [`ReplyGuard::fulfill`], `Drop`
/// converts the would-be cancellation into `Err(LocalError::Channel)`.
pub struct ReplyGuard(Option<oneshot::Sender<Result<ClaudeReply, LocalError>>>);

impl ReplyGuard {
    /// Wrap a sender.
    pub fn new(tx: oneshot::Sender<Result<ClaudeReply, LocalError>>) -> ReplyGuard {
        ReplyGuard(Some(tx))
    }

    /// Send the result (at most once); a dropped receiver is fine.
    pub fn fulfill(mut self, result: Result<ClaudeReply, LocalError>) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(result);
        }
    }
}

impl Drop for ReplyGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(Err(LocalError::Channel));
        }
    }
}

/// Entry point of the `"muse-local"` thread.
pub fn worker_main(rx: mpsc::UnboundedReceiver<Command>, state: Arc<Mutex<DownloadState>>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("muse-local-io")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            // Dropping `rx` resolves every queued and future request to
            // Err(LocalError::Channel) via ReplyGuard.
            tracing::error!(%err, "muse-local: failed to build tokio runtime");
            return;
        }
    };
    runtime.block_on(command_loop(rx, state));
}

async fn command_loop(
    mut rx: mpsc::UnboundedReceiver<Command>,
    state: Arc<Mutex<DownloadState>>,
) {
    // The resident engine; loaded on first inference, reloaded when a better
    // model lands on disk.
    let mut engine: Option<Engine> = None;

    while let Some(command) = rx.recv().await {
        match command {
            Command::Infer { req, reply } => {
                let result = run_inference(&mut engine, &req);
                reply.fulfill(result);
            }
            Command::Download { model } => {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    run_download(model, state).await;
                });
            }
        }
    }
    // All handles dropped; in-flight downloads are abandoned with the
    // runtime. Mark any live download as failed so the state is honest.
    let mut download = state.lock();
    if let DownloadState::Downloading { model, .. } = *download {
        *download = DownloadState::Failed {
            model,
            error: "worker shut down".to_string(),
        };
    }
}

/// Ensure the right model is resident, then run one generation.
fn run_inference(
    engine: &mut Option<Engine>,
    req: &ClaudeRequest,
) -> Result<ClaudeReply, LocalError> {
    let Some(wanted) = installed_model() else {
        engine.take(); // drop a resident model whose file vanished
        return Err(LocalError::NotInstalled);
    };
    if engine.as_ref().is_none_or(|e| e.which != wanted) {
        if engine.is_some() {
            tracing::info!(
                model = wanted.display_name(),
                "muse-local: better model installed; reloading"
            );
            engine.take(); // free the old model before loading the new one
        }
        *engine = Some(Engine::load(wanted)?);
    }
    let Some(engine) = engine.as_ref() else {
        return Err(LocalError::Engine("engine missing after load".to_string()));
    };
    let prompt_text = prompt::build_prompt(req);
    let raw = engine.generate(&prompt_text)?;
    Ok(reply::reply_from_output(&raw))
}

/// Stream one model to `<path>.part`, then atomically rename into place.
/// Progress lands in `state` every ~2 MB; failure sets `Failed` and removes
/// the partial file. (Resume via Range is intentionally skipped: failures
/// clean up their .part, so there is never anything to resume.)
async fn run_download(model: LocalModel, state: Arc<Mutex<DownloadState>>) {
    let fail = |error: String| {
        tracing::warn!(model = model.display_name(), %error, "muse-local: download failed");
        *state.lock() = DownloadState::Failed { model, error };
    };
    if let Err(err) = tokio::fs::create_dir_all(models_dir()).await {
        return fail(format!("create models dir: {err}"));
    }
    let final_path = model_path(model);
    let part_path = final_path.with_extension("part");

    let result = stream_to_part(model, &part_path, &state).await;
    match result {
        Ok(()) => {
            if let Err(err) = tokio::fs::rename(&part_path, &final_path).await {
                let _ = tokio::fs::remove_file(&part_path).await;
                return fail(format!("rename into place: {err}"));
            }
            tracing::info!(model = model.display_name(), "muse-local: download complete");
            *state.lock() = DownloadState::Idle;
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&part_path).await;
            fail(error);
        }
    }
}

/// The fallible body of a download: GET → stream chunks → flush.
async fn stream_to_part(
    model: LocalModel,
    part_path: &std::path::Path,
    state: &Mutex<DownloadState>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|err| format!("http client: {err}"))?;
    let response = client
        .get(model.url())
        .send()
        .await
        .map_err(|err| format!("request: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("http status {}", response.status().as_u16()));
    }
    let total = response.content_length();
    *state.lock() = DownloadState::Downloading {
        model,
        received: 0,
        total,
    };

    let mut file = tokio::fs::File::create(part_path)
        .await
        .map_err(|err| format!("create part file: {err}"))?;
    let mut received: u64 = 0;
    let mut last_reported: u64 = 0;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("stream: {err}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|err| format!("write: {err}"))?;
        received += chunk.len() as u64;
        if received - last_reported >= PROGRESS_STRIDE {
            last_reported = received;
            *state.lock() = DownloadState::Downloading {
                model,
                received,
                total,
            };
        }
    }
    file.flush().await.map_err(|err| format!("flush: {err}"))?;
    if let Some(total) = total
        && received != total
    {
        return Err(format!("truncated: {received} of {total} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_reply_guard_resolves_to_channel_error() {
        let (tx, rx) = oneshot::channel();
        drop(ReplyGuard::new(tx));
        let result = futures::executor::block_on(rx).expect("receiver must resolve");
        assert_eq!(result, Err(LocalError::Channel));
    }

    #[test]
    fn fulfilled_guard_does_not_double_send() {
        let (tx, rx) = oneshot::channel();
        let guard = ReplyGuard::new(tx);
        guard.fulfill(Ok(ClaudeReply::default()));
        let result = futures::executor::block_on(rx).expect("receiver must resolve");
        assert_eq!(result, Ok(ClaudeReply::default()));
    }

    #[test]
    fn progress_stride_throttles_to_about_two_megabytes() {
        assert_eq!(PROGRESS_STRIDE, 2 * 1024 * 1024);
    }
}
