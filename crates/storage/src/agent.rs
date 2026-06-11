//! Agent persistence: margin notes per entry and the dismissal memory that
//! keeps Muse from repeating itself.
//!
//! Notes are stored as an opaque JSON blob per entry (the agent crate owns the
//! shape); dismissals are content digests the agent includes in its prompt as
//! "never say this again".

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::{Store, now_millis};

impl Store {
    /// Persist the agent's notes for an entry, replacing any previous blob.
    pub fn save_notes(&self, entry_id: &str, notes_json: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO agent_notes (entry_id, notes) VALUES (?1, ?2) \
             ON CONFLICT(entry_id) DO UPDATE SET notes = excluded.notes",
            params![entry_id, notes_json],
        )?;
        Ok(())
    }

    /// The agent's persisted notes for an entry, if any.
    pub fn load_notes(&self, entry_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT notes FROM agent_notes WHERE entry_id = ?1",
                [entry_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Remember that the user dismissed a note with this digest. Idempotent —
    /// re-adding an existing digest is a no-op.
    pub fn add_dismissed_digest(&self, entry_id: &str, digest: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO dismissed (entry_id, digest, created_at) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(entry_id, digest) DO NOTHING",
            params![entry_id, digest, now_millis()],
        )?;
        Ok(())
    }

    /// Every digest the user has dismissed for an entry, oldest first.
    pub fn dismissed_digests(&self, entry_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT digest FROM dismissed WHERE entry_id = ?1 \
             ORDER BY created_at ASC, digest ASC",
        )?;
        let rows = stmt.query_map([entry_id], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::testutil::mem;

    #[test]
    fn notes_roundtrip_and_overwrite() {
        let store = mem();
        assert_eq!(store.load_notes("a").unwrap(), None);

        store.save_notes("a", "[{\"kind\":\"question\"}]").unwrap();
        assert_eq!(
            store.load_notes("a").unwrap().as_deref(),
            Some("[{\"kind\":\"question\"}]")
        );

        store.save_notes("a", "[]").unwrap();
        assert_eq!(store.load_notes("a").unwrap().as_deref(), Some("[]"));
    }

    #[test]
    fn dismissed_digests_roundtrip_dedupe_and_isolation() {
        let store = mem();
        assert!(store.dismissed_digests("a").unwrap().is_empty());

        store.add_dismissed_digest("a", "d1").unwrap();
        store.add_dismissed_digest("a", "d2").unwrap();
        store.add_dismissed_digest("a", "d1").unwrap(); // duplicate: no-op
        store.add_dismissed_digest("b", "d3").unwrap();

        assert_eq!(store.dismissed_digests("a").unwrap(), ["d1", "d2"]);
        assert_eq!(store.dismissed_digests("b").unwrap(), ["d3"]);
    }
}
