//! daisynotes-storage — SQLite local truth: schema, entry store, autosave flushes.
//! The launch path reads from here synchronously; the network never blocks it.
//!
//! This crate owns:
//! - the on-disk schema and its idempotent migrations (versioned via
//!   `PRAGMA user_version`), sync-ready from day one (`rev` / `synced_rev`
//!   live on `entries` even though sync lands in a later milestone);
//! - [`Store`], the handle every other layer uses to read and write entries,
//!   workspace state, settings, and agent persistence (notes + dismissal
//!   memory);
//! - the data directory location ([`data_dir`]).
//!
//! It must not know about:
//! - the document model (`daisynotes-core`): entry ids cross this boundary as plain
//!   ulid `&str`/`String`, documents as opaque JSON strings plus a plain-text
//!   mirror for future FTS;
//! - the network: sync will later read `rev`/`synced_rev` from its own crate;
//! - anything UI. No pixels, no spinners — a read here is the launch path.

mod agent;
mod blobs;
mod entries;
mod error;
mod kv;
mod paths;
mod schema;
mod store;

pub use blobs::Blob;
pub use entries::{EntrySummary, SavedEntry};
pub use error::{Result, StoreError};
pub use paths::data_dir;
pub use store::Store;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod testutil {
    use crate::{SavedEntry, Store};

    /// Fresh in-memory store with migrations applied.
    pub(crate) fn mem() -> Store {
        Store::open_in_memory().unwrap()
    }

    /// A minimal saved entry for tests.
    pub(crate) fn saved(id: &str, title: &str, touched_at: i64) -> SavedEntry {
        SavedEntry {
            id: id.to_owned(),
            title: title.to_owned(),
            preview: format!("{title} preview"),
            doc_json: format!("{{\"v\":1,\"text\":\"{title}\"}}"),
            plain: title.to_owned(),
            touched_at,
        }
    }
}
