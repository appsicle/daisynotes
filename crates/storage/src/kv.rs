//! Workspace state & settings: the `meta` key/value table.
//!
//! Internal workspace keys are namespaced under `state.` and user settings
//! under `setting.`, so a setting named like an internal key can never
//! collide with it.

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::Store;

/// `meta` key for the last-open entry id.
const LAST_OPEN_KEY: &str = "state.last_open_entry";
/// Prefix applied to every settings key.
const SETTING_PREFIX: &str = "setting.";

impl Store {
    /// The id of the entry that was open when the app last quit, if any.
    /// Part of the launch path: window restoration reads this synchronously.
    pub fn last_open_entry(&self) -> Result<Option<String>> {
        self.kv_get(LAST_OPEN_KEY)
    }

    /// Remember `id` as the entry to restore on next launch.
    pub fn set_last_open_entry(&self, id: &str) -> Result<()> {
        self.kv_set(LAST_OPEN_KEY, id)
    }

    /// Read a setting (theme, chattiness, voice defaults, …), if set.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.kv_get(&format!("{SETTING_PREFIX}{key}"))
    }

    /// Write a setting, replacing any previous value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.kv_set(&format!("{SETTING_PREFIX}{key}"), value)
    }

    fn kv_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::testutil::mem;

    #[test]
    fn settings_roundtrip_and_overwrite() {
        let store = mem();
        assert_eq!(store.setting("theme").unwrap(), None);

        store.set_setting("theme", "paper").unwrap();
        assert_eq!(store.setting("theme").unwrap().as_deref(), Some("paper"));

        store.set_setting("theme", "dusk").unwrap();
        assert_eq!(store.setting("theme").unwrap().as_deref(), Some("dusk"));
    }

    #[test]
    fn last_open_entry_roundtrip() {
        let store = mem();
        assert_eq!(store.last_open_entry().unwrap(), None);

        store
            .set_last_open_entry("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap();
        assert_eq!(
            store.last_open_entry().unwrap().as_deref(),
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")
        );
    }

    #[test]
    fn settings_cannot_collide_with_workspace_state() {
        let store = mem();
        store.set_last_open_entry("the-entry").unwrap();
        store
            .set_setting("state.last_open_entry", "imposter")
            .unwrap();
        assert_eq!(
            store.last_open_entry().unwrap().as_deref(),
            Some("the-entry")
        );
    }
}
