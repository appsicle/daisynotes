//! daisynotes-local — on-device Muse: llama.cpp inference over quantized Gemma
//! models, downloaded once into Application Support. The default brain when
//! no API key is present. Speaks the same request/reply types as daisynotes-api,
//! so the agent pipeline cannot tell the brains apart.
//!
//! Layout:
//! - [`worker`] — the `"daisynotes-local"` thread, command loop, download driver;
//! - [`engine`] — llama.cpp backend/model/context and the decode loop;
//! - [`prompt`] — Gemma chat-format prompt assembly + middle truncation;
//! - [`grammar`] — the GBNF grammar constraining output to one tool JSON;
//! - [`reply`] — model text → [`daisynotes_api::ClaudeReply`] mapping.
//!
//! Invariants mirrored from daisynotes-api: a receiver handed out by
//! [`LocalHandle::request`] always resolves (the `ReplyGuard` drop pattern),
//! and nothing in this crate panics on engine failure — every llama error
//! maps to [`LocalError::Engine`].

mod engine;
mod grammar;
mod prompt;
mod reply;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::oneshot;
use daisynotes_api::{ClaudeReply, ClaudeRequest};
use parking_lot::Mutex;

/// The on-device models Daisy Notes can run, smallest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalModel {
    /// Gemma 3 1B instruct, Q4_K_M (~0.8 GB). Quick, lighter machines.
    Light,
    /// Gemma 3 4B instruct, Q4_K_M (~2.3 GB). The default recommendation.
    Standard,
}

impl LocalModel {
    /// Human name for the settings pane.
    pub fn display_name(self) -> &'static str {
        match self {
            LocalModel::Light => "Gemma 3 1B",
            LocalModel::Standard => "Gemma 3 4B",
        }
    }

    /// Download size, for the settings pane.
    pub fn size_label(self) -> &'static str {
        match self {
            LocalModel::Light => "0.8 GB",
            LocalModel::Standard => "2.3 GB",
        }
    }

    /// On-disk file name under [`models_dir`].
    pub fn file_name(self) -> &'static str {
        match self {
            LocalModel::Light => "gemma-3-1b-it-Q4_K_M.gguf",
            LocalModel::Standard => "gemma-3-4b-it-Q4_K_M.gguf",
        }
    }

    /// Public, ungated download URL (verified anonymously reachable).
    pub fn url(self) -> &'static str {
        match self {
            LocalModel::Light => {
                "https://huggingface.co/ggml-org/gemma-3-1b-it-GGUF/resolve/main/gemma-3-1b-it-Q4_K_M.gguf"
            }
            LocalModel::Standard => {
                "https://huggingface.co/ggml-org/gemma-3-4b-it-GGUF/resolve/main/gemma-3-4b-it-Q4_K_M.gguf"
            }
        }
    }
}

/// Where models live: `~/Library/Application Support/DaisyNotes/models`.
pub fn models_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Library/Application Support/DaisyNotes/models")
}

/// The on-disk path for a model.
pub fn model_path(model: LocalModel) -> PathBuf {
    models_dir().join(model.file_name())
}

/// The best fully-downloaded model, if any (Standard preferred).
pub fn installed_model() -> Option<LocalModel> {
    [LocalModel::Standard, LocalModel::Light]
        .into_iter()
        .find(|model| model_path(*model).is_file())
}

/// Where a download stands. `Idle` + [`installed_model`] together describe
/// the full picture; a finished download simply becomes installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    /// No download in flight.
    Idle,
    /// A model is downloading.
    Downloading {
        /// Which model.
        model: LocalModel,
        /// Bytes received so far.
        received: u64,
        /// Total bytes, when the server said.
        total: Option<u64>,
    },
    /// The last download failed (cleared on retry).
    Failed {
        /// Which model.
        model: LocalModel,
        /// Short human-readable cause, for logs; the UI shows a retry.
        error: String,
    },
}

/// Errors the local brain can produce.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    /// No model is downloaded yet.
    #[error("no on-device model installed")]
    NotInstalled,
    /// The inference engine failed.
    #[error("local inference failed: {0}")]
    Engine(String),
    /// The worker thread is gone.
    #[error("local worker unavailable")]
    Channel,
}

/// Cheap-to-clone handle onto the local inference worker thread.
#[derive(Clone)]
pub struct LocalHandle {
    tx: tokio::sync::mpsc::UnboundedSender<worker::Command>,
    download: Arc<Mutex<DownloadState>>,
}

/// Start the local worker and return its handle (alias of
/// [`LocalHandle::spawn`]).
pub fn spawn() -> LocalHandle {
    LocalHandle::spawn()
}

impl LocalHandle {
    /// Start the worker thread. The model loads lazily on first request.
    ///
    /// Never panics: if the thread cannot be spawned, every subsequent
    /// [`LocalHandle::request`] resolves to `Err(LocalError::Channel)`.
    pub fn spawn() -> LocalHandle {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let download = Arc::new(Mutex::new(DownloadState::Idle));
        let state = Arc::clone(&download);
        let thread = std::thread::Builder::new().name("daisynotes-local".to_string());
        if let Err(err) = thread.spawn(move || worker::worker_main(rx, state)) {
            tracing::error!(%err, "daisynotes-local: failed to spawn worker thread");
        }
        LocalHandle { tx, download }
    }

    /// Run one consideration on the local model. Mirrors
    /// `daisynotes_api::ApiHandle::request`: never hangs — the receiver always
    /// resolves.
    pub fn request(
        &self,
        req: ClaudeRequest,
    ) -> oneshot::Receiver<Result<ClaudeReply, LocalError>> {
        let (tx, rx) = oneshot::channel();
        let command = worker::Command::Infer {
            req: Box::new(req),
            reply: worker::ReplyGuard::new(tx),
        };
        if let Err(unsent) = self.tx.send(command) {
            tracing::warn!("daisynotes-local: worker thread is gone; failing request");
            if let worker::Command::Infer { reply, .. } = unsent.0 {
                reply.fulfill(Err(LocalError::Channel));
            }
        }
        rx
    }

    /// Begin downloading `model` in the background. No-op if a download is
    /// already in flight.
    pub fn start_download(&self, model: LocalModel) {
        {
            let mut state = self.download.lock();
            if matches!(*state, DownloadState::Downloading { .. }) {
                tracing::debug!("daisynotes-local: download already in flight; ignoring");
                return;
            }
            *state = DownloadState::Downloading {
                model,
                received: 0,
                total: None,
            };
        }
        if self.tx.send(worker::Command::Download { model }).is_err() {
            tracing::warn!("daisynotes-local: worker thread is gone; failing download");
            *self.download.lock() = DownloadState::Failed {
                model,
                error: "local worker unavailable".to_string(),
            };
        }
    }

    /// The current download state; the settings pane polls this while open.
    pub fn download_state(&self) -> DownloadState {
        self.download.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_traits<T: Clone + Send + Sync>() {}

    #[test]
    fn handle_is_clone_send_sync() {
        assert_traits::<LocalHandle>();
    }

    #[test]
    fn request_resolves_to_channel_error_when_worker_is_gone() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx); // simulate a dead worker thread
        let handle = LocalHandle {
            tx,
            download: Arc::new(Mutex::new(DownloadState::Idle)),
        };
        let receiver = handle.request(ClaudeRequest::default());
        let result = futures::executor::block_on(receiver).expect("receiver must resolve");
        assert_eq!(result, Err(LocalError::Channel));
    }

    #[test]
    fn start_download_with_dead_worker_sets_failed() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let handle = LocalHandle {
            tx,
            download: Arc::new(Mutex::new(DownloadState::Idle)),
        };
        handle.start_download(LocalModel::Light);
        assert!(matches!(
            handle.download_state(),
            DownloadState::Failed {
                model: LocalModel::Light,
                ..
            }
        ));
    }

    #[test]
    fn start_download_is_ignored_while_one_is_in_flight() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = LocalHandle {
            tx,
            download: Arc::new(Mutex::new(DownloadState::Downloading {
                model: LocalModel::Standard,
                received: 5,
                total: None,
            })),
        };
        handle.start_download(LocalModel::Light);
        // Still the original download, and no command was queued.
        assert!(matches!(
            handle.download_state(),
            DownloadState::Downloading {
                model: LocalModel::Standard,
                received: 5,
                ..
            }
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn paths_follow_home() {
        let dir = models_dir();
        assert!(dir.ends_with("Library/Application Support/DaisyNotes/models"));
        let path = model_path(LocalModel::Light);
        assert_eq!(path, dir.join("gemma-3-1b-it-Q4_K_M.gguf"));
        assert!(
            model_path(LocalModel::Standard)
                .to_string_lossy()
                .contains("gemma-3-4b-it")
        );
    }
}
