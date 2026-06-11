//! Entry persistence: the sidebar list, document bodies, soft delete, purge.

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::{Store, now_millis};

/// One row of the sidebar list: metadata only, no body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    /// Entry id (a ulid string).
    pub id: String,
    /// First line of the entry (Apple Notes convention).
    pub title: String,
    /// Short plain-text excerpt for the sidebar row.
    pub preview: String,
    /// Unix milliseconds of the first save.
    pub created_at: i64,
    /// Unix milliseconds of the last edit; the sidebar sorts by this.
    pub touched_at: i64,
}

/// A full entry as handed to [`Store::upsert_entry`] by the autosave flush.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedEntry {
    /// Entry id (a ulid string).
    pub id: String,
    /// First line of the entry.
    pub title: String,
    /// Short plain-text excerpt for the sidebar row.
    pub preview: String,
    /// The serialized document (opaque JSON; this crate never parses it).
    pub doc_json: String,
    /// Plain-text mirror of the document, kept for future FTS.
    pub plain: String,
    /// Unix milliseconds of the edit being flushed.
    pub touched_at: i64,
}

impl Store {
    /// All live (not soft-deleted) entries, most recently touched first.
    pub fn list_entries(&self) -> Result<Vec<EntrySummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, preview, created_at, touched_at FROM entries \
             WHERE deleted_at IS NULL ORDER BY touched_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(EntrySummary {
                id: row.get(0)?,
                title: row.get(1)?,
                preview: row.get(2)?,
                created_at: row.get(3)?,
                touched_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The serialized document for `id`, or `None` if no body exists.
    /// Soft-deleted entries still load (the undo toast needs them).
    pub fn load_doc(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row("SELECT doc FROM bodies WHERE entry_id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Insert or update an entry — metadata and body in one transaction, so a
    /// kill mid-flush can never tear them apart.
    ///
    /// First insert sets `created_at` (to the entry's `touched_at`) and leaves
    /// `rev` at 0; every later upsert preserves `created_at` and bumps `rev`
    /// for the future sync engine.
    pub fn upsert_entry(&self, e: &SavedEntry) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO entries (id, title, preview, created_at, touched_at) \
             VALUES (?1, ?2, ?3, ?4, ?4) \
             ON CONFLICT(id) DO UPDATE SET \
               title = excluded.title, \
               preview = excluded.preview, \
               touched_at = excluded.touched_at, \
               rev = rev + 1",
            params![e.id, e.title, e.preview, e.touched_at],
        )?;
        tx.execute(
            "INSERT INTO bodies (entry_id, doc, plain) VALUES (?1, ?2, ?3) \
             ON CONFLICT(entry_id) DO UPDATE SET \
               doc = excluded.doc, plain = excluded.plain",
            params![e.id, e.doc_json, e.plain],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Soft-delete: hide the entry from [`Store::list_entries`] but keep every
    /// byte (words are sacred; the undo toast restores). Idempotent — deleting
    /// an already-deleted entry doesn't move its timestamp.
    pub fn soft_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE entries SET deleted_at = ?2, rev = rev + 1 \
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, now_millis()],
        )?;
        if changed == 0 {
            tracing::warn!(id, "soft_delete: no live entry with this id");
        }
        Ok(())
    }

    /// Undo a soft delete: the entry reappears in [`Store::list_entries`].
    pub fn restore(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE entries SET deleted_at = NULL, rev = rev + 1 \
             WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
        )?;
        if changed == 0 {
            tracing::warn!(id, "restore: no soft-deleted entry with this id");
        }
        Ok(())
    }

    /// Permanently remove entries soft-deleted more than `days` days ago,
    /// along with their bodies, agent notes, and dismissal memory. Returns the
    /// number of entries purged.
    pub fn purge_deleted_older_than(&self, days: i64) -> Result<u64> {
        let cutoff = now_millis().saturating_sub(days.saturating_mul(86_400_000));
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        // Children first: `bodies` carries the foreign key, and agent state is
        // meaningless without its entry.
        for table in ["bodies", "agent_notes", "dismissed"] {
            tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE entry_id IN \
                     (SELECT id FROM entries \
                      WHERE deleted_at IS NOT NULL AND deleted_at < ?1)"
                ),
                [cutoff],
            )?;
        }
        let purged = tx.execute(
            "DELETE FROM entries WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
            [cutoff],
        )?;
        tx.commit()?;
        Ok(purged as u64)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::testutil::{mem, saved};

    #[test]
    fn upsert_then_list_orders_by_touched_at_desc() {
        let store = mem();
        store.upsert_entry(&saved("a", "older", 1_000)).unwrap();
        store.upsert_entry(&saved("b", "newest", 3_000)).unwrap();
        store.upsert_entry(&saved("c", "middle", 2_000)).unwrap();

        let ids: Vec<_> = store
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["b", "c", "a"]);
    }

    #[test]
    fn load_doc_roundtrips_and_missing_is_none() {
        let store = mem();
        let entry = saved("a", "hello", 1_000);
        store.upsert_entry(&entry).unwrap();
        assert_eq!(store.load_doc("a").unwrap(), Some(entry.doc_json));
        assert_eq!(store.load_doc("nope").unwrap(), None);
    }

    #[test]
    fn created_at_is_preserved_across_upserts() {
        let store = mem();
        store.upsert_entry(&saved("a", "draft", 1_000)).unwrap();
        store.upsert_entry(&saved("a", "edited", 9_000)).unwrap();

        let entries = store.list_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].created_at, 1_000);
        assert_eq!(entries[0].touched_at, 9_000);
        assert_eq!(entries[0].title, "edited");
    }

    #[test]
    fn upsert_bumps_rev_for_sync() {
        let store = mem();
        store.upsert_entry(&saved("a", "one", 1_000)).unwrap();
        store.upsert_entry(&saved("a", "two", 2_000)).unwrap();
        store.upsert_entry(&saved("a", "three", 3_000)).unwrap();

        let (rev, synced_rev): (i64, i64) = store
            .conn
            .lock()
            .query_row(
                "SELECT rev, synced_rev FROM entries WHERE id = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(rev, 2);
        assert_eq!(synced_rev, -1);
    }

    #[test]
    fn soft_delete_hides_and_restore_returns() {
        let store = mem();
        store.upsert_entry(&saved("a", "keep", 1_000)).unwrap();
        store.upsert_entry(&saved("b", "drop", 2_000)).unwrap();

        store.soft_delete("b").unwrap();
        let ids: Vec<_> = store
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["a"]);
        // Body survives a soft delete.
        assert!(store.load_doc("b").unwrap().is_some());

        store.restore("b").unwrap();
        let ids: Vec<_> = store
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn soft_delete_and_restore_of_missing_id_are_noops() {
        let store = mem();
        store.soft_delete("ghost").unwrap();
        store.restore("ghost").unwrap();
        assert!(store.list_entries().unwrap().is_empty());
    }

    #[test]
    fn purge_removes_only_old_deleted_entries_and_their_satellites() {
        let store = mem();
        store.upsert_entry(&saved("old", "old", 1_000)).unwrap();
        store.upsert_entry(&saved("new", "new", 2_000)).unwrap();
        store.upsert_entry(&saved("live", "live", 3_000)).unwrap();
        store.save_notes("old", "[]").unwrap();
        store.add_dismissed_digest("old", "digest-1").unwrap();
        store.soft_delete("old").unwrap();
        store.soft_delete("new").unwrap();

        // Backdate "old"'s deletion 40 days.
        store
            .conn
            .lock()
            .execute(
                "UPDATE entries SET deleted_at = deleted_at - 40 * 86400000 \
                 WHERE id = 'old'",
                [],
            )
            .unwrap();

        let purged = store.purge_deleted_older_than(30).unwrap();
        assert_eq!(purged, 1);

        // "old" is fully gone, satellites included.
        assert_eq!(store.load_doc("old").unwrap(), None);
        assert_eq!(store.load_notes("old").unwrap(), None);
        assert!(store.dismissed_digests("old").unwrap().is_empty());

        // "new" is still recoverable, "live" untouched.
        store.restore("new").unwrap();
        let ids: Vec<_> = store
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["live", "new"]);
    }
}
