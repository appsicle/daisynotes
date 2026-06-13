//! Storage-facing glue: boot settings, the autosave debounce/flush cycle,
//! entry records, and the small pure helpers (clocks, titles, date labels)
//! those writes need.
//!
//! Charter: every `daisynotes_storage::Store` access made after `main`'s launch
//! reads goes through here or `muse_flow`. This module must not know about
//! the agent pipeline or layout.

use std::time::Duration;

use gpui::{Context, SharedString};
use daisynotes_agent::Chattiness;
use daisynotes_core::Document;
use daisynotes_entries::SyncGlyph;
use daisynotes_storage::{SavedEntry, Store};
use daisynotes_theme::{Appearance, ThemePair, derive_tokens, hsla_from_hex, presets};
use ulid::Ulid;

use crate::workspace::Workspace;

/// Autosave flushes at most this long after the last edit.
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// How long the sidebar's "saved" check lingers after a flush.
const GLYPH_LINGER: Duration = Duration::from_secs(2);

/// Settings read once at launch, before the window opens.
#[derive(Clone, Copy, Debug)]
pub struct Boot {
    /// Paper or Dusk, from the `appearance` setting (default Paper).
    pub appearance: Appearance,
    /// How often Muse may speak (default Occasional).
    pub chattiness: Chattiness,
    /// The writer's stored mute preference.
    pub muted: bool,
    /// The light/dark palettes from `theme.preset` / `theme.custom.*`.
    pub pair: ThemePair,
}

impl Boot {
    /// Read the boot settings; unreadable or missing values fall back to
    /// defaults (the launch path never blocks on a bad row).
    pub fn load(store: &Store) -> Boot {
        let setting = |key: &str| {
            store.setting(key).unwrap_or_else(|err| {
                tracing::error!(%err, key, "failed to read setting");
                None
            })
        };
        let appearance = match setting("appearance").as_deref() {
            Some("dusk") => Appearance::Dusk,
            _ => Appearance::Paper,
        };
        let chattiness = match setting("chattiness").as_deref() {
            Some("quiet") => Chattiness::Quiet,
            Some("chatty") => Chattiness::Chatty,
            _ => Chattiness::Occasional,
        };
        let muted = matches!(setting("muse_muted").as_deref(), Some("true"));
        let pair = pair_from_settings(
            setting("theme.preset").as_deref(),
            setting("theme.custom.light").as_deref(),
            setting("theme.custom.dark").as_deref(),
        );
        Boot {
            appearance,
            chattiness,
            muted,
            pair,
        }
    }
}

/// Resolve the persisted theme choice to a token pair: a named preset, a
/// pair of `accent,bg,fg` hex triplets for `"custom"`, or the Paper & Dusk
/// defaults when anything is missing or fails to parse.
pub(crate) fn pair_from_settings(
    preset: Option<&str>,
    custom_light: Option<&str>,
    custom_dark: Option<&str>,
) -> ThemePair {
    match preset {
        Some("custom") => {
            if let (Some(light), Some(dark)) = (
                custom_light.and_then(tokens_from_triplet),
                custom_dark.and_then(tokens_from_triplet),
            ) {
                ThemePair { light, dark }
            } else {
                tracing::warn!("custom theme settings failed to parse; using defaults");
                ThemePair::default()
            }
        }
        Some(name) => presets()
            .iter()
            .find(|preset| preset.name == name)
            .map_or_else(ThemePair::default, daisynotes_theme::ThemePreset::pair),
        None => ThemePair::default(),
    }
}

/// Parse one persisted `accent,bg,fg` hex triplet into derived tokens.
fn tokens_from_triplet(triplet: &str) -> Option<daisynotes_theme::Tokens> {
    let mut parts = triplet.split(',').map(str::trim).map(hsla_from_hex);
    let accent = parts.next()??;
    let bg = parts.next()??;
    let fg = parts.next()??;
    Some(derive_tokens(accent, bg, fg))
}

/// Wall-clock unix milliseconds, for storage timestamps.
pub(crate) fn now_unix_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// Wall-clock milliseconds as the trigger engine's clock.
pub(crate) fn now_ms() -> u64 {
    u64::try_from(now_unix_ms()).unwrap_or(0)
}

/// "Tuesday, June 10" for the tertiary label above the entry's first line.
pub(crate) fn date_label(created_at_ms: i64) -> Option<SharedString> {
    let ts = jiff::Timestamp::from_millisecond(created_at_ms).ok()?;
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    jiff::fmt::strtime::format("%A, %B %-d", &zoned)
        .ok()
        .map(SharedString::from)
}

/// First non-empty line, trimmed; empty when the entry has no words yet so
/// the sidebar can render its "New entry" placeholder in tertiary ink.
pub(crate) fn first_line_title(plain: &str) -> String {
    plain
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

impl Workspace {
    /// Debounced autosave: each edit bumps the generation; the matching
    /// timer flushes only if no newer edit superseded it.
    pub(crate) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        self.save_generation = self.save_generation.wrapping_add(1);
        let generation = self.save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.save_generation == generation {
                    this.flush(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Synchronous flush: one transactional row write, then the sidebar
    /// refreshes from storage. A failed write keeps the dirty flag so the
    /// next edit retries.
    pub(crate) fn flush(&mut self, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        let entry = {
            let doc = self.editor.read(cx).document();
            let plain = doc.plain_text();
            SavedEntry {
                id: self.current_entry.clone(),
                title: first_line_title(&plain),
                preview: doc.preview(),
                doc_json: doc.to_json(),
                plain,
                touched_at: now_unix_ms(),
            }
        };
        if let Err(err) = self.store.upsert_entry(&entry) {
            tracing::error!(%err, "autosave flush failed");
            return;
        }
        self.dirty = false;
        self.refresh_sidebar(cx);
        self.show_saved_glyph(cx);
    }

    /// Push a fresh entry list into the sidebar.
    pub(crate) fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        match self.store.list_entries() {
            Ok(entries) => self.sidebar.update(cx, |sidebar, cx| {
                sidebar.set_entries(entries, cx);
            }),
            Err(err) => tracing::error!(%err, "failed to list entries"),
        }
    }

    /// Mint and persist an empty entry record, returning its id. The
    /// sidebar shows it as soon as the next refresh runs.
    pub(crate) fn create_entry_record(&self) -> String {
        let ulid = Ulid::new();
        let doc = Document::new(ulid);
        let entry = SavedEntry {
            id: ulid.to_string(),
            title: String::new(),
            preview: String::new(),
            doc_json: doc.to_json(),
            plain: String::new(),
            touched_at: now_unix_ms(),
        };
        if let Err(err) = self.store.upsert_entry(&entry) {
            tracing::error!(%err, "failed to create entry record");
        }
        entry.id
    }

    /// Write a setting, logging (never surfacing) failures.
    pub(crate) fn persist_setting(&self, key: &str, value: &str) {
        if let Err(err) = self.store.set_setting(key, value) {
            tracing::error!(%err, key, "failed to persist setting");
        }
    }

    /// Briefly show the passive "saved" check in the sidebar footer.
    fn show_saved_glyph(&mut self, cx: &mut Context<Self>) {
        self.glyph_generation = self.glyph_generation.wrapping_add(1);
        let generation = self.glyph_generation;
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_sync_glyph(SyncGlyph::Saved, cx);
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GLYPH_LINGER).await;
            this.update(cx, |this, cx| {
                if this.glyph_generation == generation {
                    this.sidebar.update(cx, |sidebar, cx| {
                        sidebar.set_sync_glyph(SyncGlyph::Local, cx);
                    });
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn boot_defaults_when_settings_are_absent() {
        let store = Store::open_in_memory().unwrap();
        let boot = Boot::load(&store);
        assert_eq!(boot.appearance, Appearance::Paper);
        assert_eq!(boot.chattiness, Chattiness::Occasional);
        assert!(!boot.muted);
        assert_eq!(boot.pair, ThemePair::default());
    }

    #[test]
    fn boot_reads_stored_settings() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("appearance", "dusk").unwrap();
        store.set_setting("chattiness", "quiet").unwrap();
        store.set_setting("muse_muted", "true").unwrap();
        let boot = Boot::load(&store);
        assert_eq!(boot.appearance, Appearance::Dusk);
        assert_eq!(boot.chattiness, Chattiness::Quiet);
        assert!(boot.muted);
    }

    #[test]
    fn boot_resolves_named_preset_and_custom_triplets() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("theme.preset", "Codex").unwrap();
        let boot = Boot::load(&store);
        let codex = presets()
            .into_iter()
            .find(|p| p.name == "Codex")
            .unwrap()
            .pair();
        assert_eq!(boot.pair, codex);

        store.set_setting("theme.preset", "custom").unwrap();
        store
            .set_setting("theme.custom.light", "#B86450,#FAF8F5,#26221C")
            .unwrap();
        store
            .set_setting("theme.custom.dark", "#E0907C,#171512,#EDE9E2")
            .unwrap();
        let boot = Boot::load(&store);
        assert_ne!(boot.pair, ThemePair::default());

        // A torn custom setting falls back to the defaults.
        store.set_setting("theme.custom.dark", "nonsense").unwrap();
        assert_eq!(Boot::load(&store).pair, ThemePair::default());
    }

    #[test]
    fn date_label_is_weekday_month_day_without_padding() {
        // 2026-06-09 12:00:00 UTC — a Tuesday everywhere near UTC; only the
        // shape is asserted to stay timezone-independent.
        let label = date_label(1_780_660_800_000).expect("label formats");
        let label = label.to_string();
        assert!(label.contains(", "), "missing comma: {label}");
        assert!(!label.contains(" 0"), "zero-padded day: {label}");
        let day: String = label
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        assert!(!day.is_empty(), "label has no day number: {label}");
    }

    #[test]
    fn first_line_title_skips_blank_lines_and_trims() {
        assert_eq!(first_line_title(""), "");
        assert_eq!(first_line_title("\n\n  \n"), "");
        assert_eq!(first_line_title("  Dear June  \nbody"), "Dear June");
        assert_eq!(first_line_title("\n\n  late title"), "late title");
    }
}
