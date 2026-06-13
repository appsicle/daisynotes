//! The [`Store`] handle: connection ownership, open paths, pragmas.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::{Result, StoreError};
use crate::paths::data_dir;
use crate::schema;

/// Handle to Daisy Notes's local SQLite database.
///
/// Wraps a single connection in a `parking_lot::Mutex`, so `Store` is
/// `Send + Sync` and can be shared across threads (the UI thread's synchronous
/// launch-path reads, the autosave flush, the agent's persistence) without any
/// further coordination. Every method takes `&self`.
pub struct Store {
    pub(crate) conn: Mutex<Connection>,
}

impl Store {
    /// Open (or create) the database at `path`, creating its parent directory
    /// and applying schema migrations idempotently.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Self::init(Connection::open(path)?)
    }

    /// Open the default database, `data_dir()/daisynotes.sqlite3` — the app's
    /// launch-path store.
    pub fn open_default() -> Result<Store> {
        Self::open(&data_dir().join("daisynotes.sqlite3"))
    }

    /// Open a fresh in-memory database (tests).
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        apply_pragmas(&conn)?;
        schema::migrate(&mut conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }
}

/// PLAN §7 pragmas: WAL journaling, `synchronous=NORMAL`, foreign keys on.
fn apply_pragmas(conn: &Connection) -> Result<()> {
    // `journal_mode` returns the resulting mode as a row, so it can't go
    // through `pragma_update`; in-memory databases report "memory", which is
    // equally fine.
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

/// Current wall-clock time in unix milliseconds (via jiff).
pub(crate) fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Store;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn store_is_send_and_sync() {
        assert_send_sync::<Store>();
    }

    #[test]
    fn open_twice_is_idempotent_and_preserves_data() {
        let path =
            std::env::temp_dir().join(format!("daisynotes-storage-test-{}.sqlite3", ulid::Ulid::new()));
        {
            let store = Store::open(&path).unwrap();
            store.set_setting("theme", "dusk").unwrap();
        }
        {
            // Second open re-runs the migration path; it must be a no-op that
            // keeps existing rows.
            let store = Store::open(&path).unwrap();
            assert_eq!(store.setting("theme").unwrap().as_deref(), Some("dusk"));
        }
        for suffix in ["", "-wal", "-shm"] {
            let mut sidecar = path.clone().into_os_string();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(sidecar);
        }
    }

    #[test]
    fn open_in_memory_applies_schema() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.list_entries().unwrap().is_empty());
    }
}
