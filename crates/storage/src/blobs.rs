//! Content-addressed binary blobs (pasted images). Keyed by the u64 content
//! hash of the encoded bytes, stored as an i64 (SQLite has no unsigned), so
//! the same image pasted twice dedupes to one row.

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::Store;

/// An image blob: its MIME type and encoded bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl Store {
    /// Insert a blob if its id isn't already present (idempotent — the id is
    /// the content hash, so re-inserting the same bytes is a no-op).
    pub fn put_blob(&self, id: u64, mime: &str, bytes: &[u8]) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO blobs (id, mime, bytes) VALUES (?1, ?2, ?3)",
            params![id as i64, mime, bytes],
        )?;
        Ok(())
    }

    /// Fetch a blob by content-hash id.
    pub fn get_blob(&self, id: u64) -> Result<Option<Blob>> {
        let conn = self.conn.lock();
        let blob = conn
            .query_row(
                "SELECT mime, bytes FROM blobs WHERE id = ?1",
                params![id as i64],
                |row| {
                    Ok(Blob {
                        mime: row.get(0)?,
                        bytes: row.get(1)?,
                    })
                },
            )
            .optional()?;
        Ok(blob)
    }
}
