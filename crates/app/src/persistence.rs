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

/// The little daisy that opens the welcome note — the image-embed feature,
/// shown in place. Stored as a blob so the editor loads it like any pasted
/// image; the id is a fixed key (not a content hash; this image never collides
/// with a user's pasted ones).
const WELCOME_IMAGE: &[u8] = include_bytes!("../../../assets/icons/daisynotes-mark.png");
const WELCOME_IMAGE_ID: u64 = 0xDA15_0000_0000_0001;

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
            Some("occasional") => Chattiness::Occasional,
            // New writers start chatty; the muse earns its keep by speaking.
            _ => Chattiness::Chatty,
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

    /// Seed the very first entry with a hand-written welcome that shows the app
    /// off *in place*: real bold/italic/ink, two real lists, and invitations to
    /// try things. No separate onboarding screen — the welcome is just a note
    /// you can read, edit, or clear. Returns its id; used once, on first launch.
    pub(crate) fn seed_welcome_entry(&self) -> String {
        let ulid = Ulid::new();
        let doc = welcome_document(ulid);
        let plain = doc.plain_text();
        let entry = SavedEntry {
            id: ulid.to_string(),
            title: first_line_title(&plain),
            preview: doc.preview(),
            doc_json: doc.to_json(),
            plain,
            touched_at: now_unix_ms(),
        };
        if let Err(err) = self.store.upsert_entry(&entry) {
            tracing::error!(%err, "failed to seed welcome entry");
        }
        // The welcome image blob (the doc already references it) and the muse's
        // opening notes, so both features show on first open.
        if let Err(err) = self.store.put_blob(WELCOME_IMAGE_ID, "image/png", WELCOME_IMAGE) {
            tracing::error!(%err, "failed to store welcome image");
        }
        if let Ok(json) = serde_json::to_string(&welcome_notes())
            && let Err(err) = self.store.save_notes(&entry.id, &json)
        {
            tracing::error!(%err, "failed to seed welcome notes");
        }
        entry.id
    }
}

/// Build the welcome document: the hand-written first note that shows the app
/// off in place. Pure (no storage), so its formatting/list offsets are unit
/// tested.
pub(crate) fn welcome_document(id: daisynotes_core::EntryId) -> Document {
    use daisynotes_core::{ImageBlock, Ink, ListAttr, ListKind, StyleToggle};

    let mut doc = Document::new(id);

    // List items sit on their own lines with no markers in the text — the
    // paragraph attribute draws the bullet/number. Styles and list marks
    // are applied by searching this exact string, so the offsets stay put
    // (neither toggle_style nor set_para_list changes the text).
    let text = "Welcome to Daisy Notes\n\
\n\
You write, and a friend reads along — now and then leaving a small note out in \
the margin. Never loud, never in the way. That friend is the muse.\n\
\n\
A few things worth knowing:\n\
Your words stay on your machine, in a plain file on disk. Nothing leaves unless \
you send it.\n\
The muse can think on-device — a small model that runs right on your Mac. No \
account, no cloud.\n\
Prefer the cloud? Drop a Claude key into Settings and it'll think there instead.\n\
Make the words yours: bold, italics, a touch of color, and whatever font feels \
right.\n\
\n\
Try a few things, right here:\n\
Type a dash and a space to begin a list. A number and a dot start a numbered one.\n\
Paste an image — press Command-V — and it lands on the page, right where you are.\n\
Press Command-comma for Settings: themes, the muse's mood, your key.\n\
\n\
When something new ships, Daisy Notes updates itself quietly — a small Update tag \
appears up top when it's ready.\n\
\n\
This page is yours. Clear it, or keep it. Either way — start writing.";

        doc.insert(0, text);

        // Style the first occurrence of a phrase, so the welcome demonstrates
        // each kind of formatting on the word that names it.
        let style = |doc: &mut Document, phrase: &str, toggle: StyleToggle| {
            if let Some(at) = text.find(phrase) {
                doc.toggle_style(at..at + phrase.len(), toggle);
            }
        };
        style(&mut doc, "Welcome to Daisy Notes", StyleToggle::Bold);
        style(&mut doc, "the muse", StyleToggle::Ink(Some(Ink::Lavender)));
        style(&mut doc, "bold", StyleToggle::Bold);
        style(&mut doc, "italics", StyleToggle::Italic);
        style(&mut doc, "a touch of color", StyleToggle::Ink(Some(Ink::Rose)));

        // Turn the two clusters of lines into real lists.
        let list = |doc: &mut Document, line: &str, kind: ListKind| {
            if let Some(at) = text.find(line) {
                doc.set_para_list(at, Some(ListAttr { kind, indent: 0 }));
            }
        };
        list(&mut doc, "Your words stay on your machine", ListKind::Bullet);
        list(&mut doc, "The muse can think on-device", ListKind::Bullet);
        list(&mut doc, "Prefer the cloud?", ListKind::Bullet);
        list(&mut doc, "Make the words yours:", ListKind::Bullet);
        list(&mut doc, "Type a dash and a space", ListKind::Number);
        list(&mut doc, "Paste an image", ListKind::Number);
        list(&mut doc, "Press Command-comma for Settings", ListKind::Number);

    // A daisy on its own line, right under the title — the image-embed feature
    // shown in place. It sits on the blank paragraph after the title; the blob
    // itself is written by `seed_welcome_entry`.
    if let Some(blank) = text.find("\n\n") {
        doc.set_image(
            blank + 1,
            Some(ImageBlock {
                id: WELCOME_IMAGE_ID,
                w: 0,
                h: 0,
                width: 88,
            }),
        );
    }

    doc
}

/// The muse notes seeded onto the welcome — one quiet card and one reaction —
/// so the margin companion is there the moment it opens. Each anchors to a
/// verbatim quote in the welcome text (see `welcome_document`).
fn welcome_notes() -> Vec<daisynotes_agent::NoteRecord> {
    use daisynotes_agent::{NoteKind, NoteRecord};
    vec![
        NoteRecord {
            id: 1,
            quote: "That friend is the muse.".to_string(),
            kind: NoteKind::Encouragement,
            body: "Hi — that's me. I stay out here in the margin, and only ever a word or two."
                .to_string(),
            emoji: None,
        },
        NoteRecord {
            id: 2,
            quote: "start writing".to_string(),
            kind: NoteKind::Encouragement,
            body: String::new(),
            emoji: Some("❤️".to_string()),
        },
    ]
}

impl Workspace {
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
        assert_eq!(boot.chattiness, Chattiness::Chatty);
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

#[cfg(test)]
mod welcome_tests {
    use super::{WELCOME_IMAGE_ID, welcome_document, welcome_notes};
    use daisynotes_core::{Document, EntryId, Ink, ListKind};

    fn raw(doc: &Document) -> String {
        doc.slice(0..doc.len())
    }

    #[test]
    fn welcome_structure_and_round_trip() {
        let doc = welcome_document(EntryId::nil());
        let text = raw(&doc);
        assert!(text.starts_with("Welcome to Daisy Notes"));

        // A bullet line and a numbered line carry the right list attribute.
        let bullet = text.find("Your words stay on your machine").unwrap();
        assert_eq!(doc.para_attr(bullet).map(|a| a.kind), Some(ListKind::Bullet));
        let number = text.find("Paste an image").unwrap();
        assert_eq!(doc.para_attr(number).map(|a| a.kind), Some(ListKind::Number));

        // "the muse" is inked in the muse hue; "Welcome…" is bold.
        let muse = text.find("the muse").unwrap();
        assert_eq!(doc.spans().style_at(muse).ink, Some(Ink::Lavender));
        assert!(doc.spans().style_at(0).bold);

        // The hero image sits on the blank paragraph after the title.
        let img_at = text.find("\n\n").unwrap() + 1;
        assert_eq!(doc.image_at(img_at).map(|b| b.id), Some(WELCOME_IMAGE_ID));

        // Every seeded muse note anchors to a verbatim quote present in the
        // welcome (plain_text is the raw rope, so these offsets are real).
        for note in welcome_notes() {
            assert!(text.contains(&note.quote), "note quote missing: {}", note.quote);
        }

        // Everything survives a JSON round-trip (how it's actually persisted).
        let restored = Document::from_json(EntryId::nil(), &doc.to_json()).unwrap();
        assert_eq!(raw(&restored), text);
        assert_eq!(restored.para_attr(bullet).map(|a| a.kind), Some(ListKind::Bullet));
        assert_eq!(restored.spans().style_at(muse).ink, Some(Ink::Lavender));
        assert_eq!(restored.image_at(img_at).map(|b| b.id), Some(WELCOME_IMAGE_ID));
    }
}
