//! Error types at this crate's boundary.

use std::path::PathBuf;

/// Errors produced by [`crate::Store`] operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The underlying SQLite call failed.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Creating the database's directory failed.
    #[error("could not create directory {path}: {source}")]
    Io {
        /// The directory we attempted to create.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Crate-wide result alias; the error type defaults to [`StoreError`].
pub type Result<T, E = StoreError> = std::result::Result<T, E>;
