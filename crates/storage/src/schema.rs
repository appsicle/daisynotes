//! Schema DDL and idempotent, versioned migrations.
//!
//! Migration state lives in `PRAGMA user_version`: the number of migrations
//! already applied. Opening a store runs every migration past that point, each
//! inside its own transaction, so a database is always either at a known
//! version or untouched.

use rusqlite::Connection;

use crate::error::Result;

/// Ordered migration list. Index `i` brings the database to version `i + 1`.
/// Append-only: never edit a shipped migration, add a new one.
const MIGRATIONS: &[&str] = &[
    // v1 — initial schema. Sync-ready per PLAN §7: `rev`/`synced_rev` exist
    // before the sync engine does.
    "CREATE TABLE entries (
        id          TEXT PRIMARY KEY,
        title       TEXT NOT NULL,
        preview     TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        touched_at  INTEGER NOT NULL,
        deleted_at  INTEGER,
        rev         INTEGER NOT NULL DEFAULT 0,
        synced_rev  INTEGER NOT NULL DEFAULT -1
    );
    CREATE INDEX idx_entries_touched_at ON entries(touched_at DESC);
    CREATE TABLE bodies (
        entry_id  TEXT PRIMARY KEY REFERENCES entries(id),
        doc       TEXT NOT NULL,
        plain     TEXT NOT NULL
    );
    CREATE TABLE agent_notes (
        entry_id  TEXT PRIMARY KEY,
        notes     TEXT NOT NULL
    );
    CREATE TABLE dismissed (
        entry_id    TEXT NOT NULL,
        digest      TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        PRIMARY KEY (entry_id, digest)
    );
    CREATE TABLE meta (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );",
    // v2 — content-addressed image blobs. `id` is the u64 content hash stored
    // as an i64 (SQLite has no unsigned); documents reference blobs by id.
    "CREATE TABLE blobs (
        id     INTEGER PRIMARY KEY,
        mime   TEXT NOT NULL,
        bytes  BLOB NOT NULL
    );",
];

/// Apply every migration the database hasn't seen yet. Idempotent: a second
/// call (or a second open of the same file) is a no-op.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (idx, ddl) in MIGRATIONS.iter().enumerate() {
        let version = idx as i64 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(ddl)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::debug!(version, "applied schema migration");
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::migrate;
    use crate::testutil::mem;

    #[test]
    fn migrate_twice_on_same_connection_is_a_noop() {
        let store = mem();
        let mut conn = store.conn.lock();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version as usize, super::MIGRATIONS.len());
        migrate(&mut conn).unwrap();
        let again: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, again);
    }
}
