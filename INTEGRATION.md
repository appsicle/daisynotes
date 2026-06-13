# INTEGRATION.md — assembling crates/app

The composition root. Every other crate is built, green, and tested. This document
is the authoritative wiring spec, compiled from all nine crate reports. Where it
conflicts with memory of the crates, READ THE CRATE SOURCE — it is the truth.

## Startup sequence (main.rs)

1. `tracing_subscriber` init (file appender under `daisynotes_storage::data_dir()/logs` or stderr; keep simple, env-filter).
2. `Application::new().with_assets(daisynotes_ui::assets::DaisyNotesAssets)` — REQUIRED or `svg().path("icons/…")` fails.
3. Inside `run`: `cx.text_system().add_fonts(daisynotes_ui::fonts::all())` BEFORE any window — fonts must exist before first frame (no-shift doctrine). NOTE: the Quattro family registers as **"iA Writer Quattro V"** (theme const already corrected).
4. Open `daisynotes_storage::Store::open_default()`. Read settings: `appearance` ("paper"/"dusk", default paper), `chattiness`, `muse_muted`, `consent`.
5. `cx.set_global(daisynotes_theme::Theme::new(appearance))` BEFORE any view renders — `cx.theme()` panics otherwise.
6. `cx.bind_keys(daisynotes_commands::keybindings())`, `cx.set_menus(daisynotes_commands::app_menus())`.
7. `daisynotes_api::spawn()` → ApiHandle (cheap, do it at startup; requests resolve the key per-call, so a key added later works after relaunch only — fine).
8. Open window: `WindowOptions { titlebar: Some(TitlebarOptions { title: "Daisy Notes", appears_transparent: true, traffic_light_position: Some(point(px(12.), px(18.))) }), window_bounds: centered 1100×760, window_min_size if the field exists (verify in gpui source; target 720×480), ..Default }`.
9. Root entity: `Workspace`. `cx.activate(true)`.
10. `cx.on_app_quit` (verify exact API) → flush pending autosave synchronously.

## Workspace entity (the only stateful glue)

State: `store: Arc<Store>` (or Rc — single-threaded use; Store is Send+Sync, Arc is fine),
`api: ApiHandle`, `editor: Entity<Editor>`, `sidebar: Entity<Sidebar>`, `topbar: Entity<Topbar>`,
`entries_open: bool` + sidebar width animation state, theme crossfade animation state,
`current_entry: String` (ulid string), autosave state (dirty flag + debounce generation),
`engines: HashMap<String, daisynotes_agent::TriggerEngine>`, `notes: Vec<daisynotes_agent::NoteRecord>` (active entry),
`next_note_id: u64`, `muted: bool`, `chattiness: Chattiness`, `consented: bool`,
toast state (enum: None | Plain{msg} | Undo{msg, entry_id}, with auto-dismiss generation).

### Layout (render)

- Root: `div` full-size, `bg(tokens.bg)`, `.key_context(WORKSPACE_CONTEXT)`, all workspace `on_action` handlers, horizontal flex:
  - **Sidebar slot**: width animates 0 ↔ 260 (manual animation: store `(from, to, start: Instant)`; eased with `motion::spring` over `motion::MOVE`; while animating `window.request_animation_frame()`). Inside the slot: fixed-260px-wide Sidebar view, clipped (overflow hidden) so it slides rather than squishes. This is the ONE sanctioned layout animation.
  - **Main column** (flex_1, vertical): Topbar view (fixed TOPBAR_H), Editor view (flex_1).
- Overlay layers (deferred/anchored or absolute, NEVER affecting layout): toast (bottom-center, fade in/out over FADE, auto-dismiss 5s), consent card (centered, only pre-consent).
- Traffic lights occupy the window's top-left; topbar already leaves 80px clear; sidebar has its own top padding. The window is draggable via the transparent titlebar region by default — verify; if not, leave as-is for v1 and note it.

### Theme crossfade (ToggleTheme handler)

Capture `from = cx.theme().tokens`, `target = appearance.toggled()`; persist setting immediately;
animate t: 0→1 over `motion::THEME_FADE` with `motion::ease_in_out`, each frame
`cx.set_global(Theme { appearance: target, tokens: lerp_tokens(&from, &target.tokens(), t) })`
then notify/refresh (verify the cheapest correct way to repaint the window on global change —
`cx.refresh_windows()` or notify the root; check gpui source). Final frame sets exact target tokens.
Call `topbar.set_appearance(target)`.

### Entries flow

- Startup: `list_entries()`; if empty create `Document::new(Ulid::new())`, upsert immediately, list again. Open `last_open_entry` if it's in the list, else the first (most recent).
- **Open(id)** (sidebar event, also used at startup): flush autosave for the old entry first; `load_doc(id)` → `Document::from_json(ulid, &json)` (on parse failure: tracing::error, fall back to a fresh document — never crash); `editor.replace_document`; `sidebar.set_selected(Some(id))`; `set_last_open_entry`; `topbar.set_voice(editor.voice())`; date label: format the entry's `created_at` with jiff as e.g. "Tuesday, June 10" → `editor.set_date_label`; restore agent state (below); focus the editor (`window.focus(&editor.focus_handle(cx))` — verify call shape); reset orb to Resting (or muted state).
- **NewEntry** (action + sidebar event → same handler): flush; create doc; upsert (so the sidebar shows it instantly); open it.
- **DeleteRequested(id)**: `soft_delete(id)`; refresh sidebar list; if it was current → open most recent remaining (create fresh if none); show Undo toast "Entry deleted" 5s; Undo → `restore(id)` + refresh list.
- Touch semantics: only edits reorder (upsert sets touched_at) — opening must NOT touch.

### Autosave

- Subscribe to editor `Edited`: set dirty, bump a generation counter, spawn a debounce task (gpui spawn + background timer 400ms); when it fires and generation unchanged → flush.
- `flush()`: if dirty — build `SavedEntry { id, title: doc.title(), preview: doc.preview(), doc_json: doc.to_json(), plain: doc.plain_text(), touched_at: now_ms (jiff) }`, `upsert_entry`, refresh `sidebar.set_entries(list_entries())`, glyph Saving→Saved (brief), clear dirty.
- Flush synchronously on: entry switch, NewEntry, quit, window deactivation if observable cheaply.
- `replace_document` emits SelectionChanged/ScrollChanged, NOT Edited — switching entries must not dirty.

### Editor event wiring

- `Edited` → autosave (above) + agent note_edit (below) + `topbar.set_voice` is NOT needed here (voice events are separate).
- `VoiceChanged` → `topbar.set_voice(editor.voice())` + autosave dirty (style is content).
  CHECK the editor source: VoiceChanged may fire alongside Edited; don't double-dirty (idempotent dirty flag is fine).
- `ScrollChanged` → `topbar.set_scrolled(editor.read(cx).is_scrolled())`.
- `AnnotationDismissed { id }` → find NoteRecord by id → `add_dismissed_digest(entry, dismissal_digest(&quote, kind))` → remove record → `save_notes(entry, json)`.

### Topbar event wiring

- `SetFamily(f)` → dispatch `daisynotes_commands::SetFamily { family: index }` to the editor (or call through an editor action dispatch on the window — the editor handles the action; verify focus is on editor or dispatch directly via `window.dispatch_action` while editor focused; if the popover holds focus, dispatching may miss the editor — SAFEST: the workspace handles TopbarEvent by directly updating through the editor entity: `editor.update(cx, |e, cx| ...)` — but Editor has no public set_voice. Therefore: dispatch the gpui action TO THE EDITOR'S FOCUS via `window.dispatch_action(...)` after focusing the editor, or better, verify whether `dispatch_action` routes to focused element — the popover holds focus while open! Pragmatic decision: on TopbarEvent, call `window.focus(&editor.focus_handle)` first, then `window.dispatch_action(Box::new(action), cx)`. The popover stays open per its design (it re-takes focus? it won't — accept that arrow: clicking a font row applies the change; popover may lose Escape-focus afterward but remains dismissible by click-away. Note whatever you observe in the report.)
- `SetSize(s)` → compare with `editor.read(cx).voice().size`: greater → dispatch `IncreaseSize`, smaller → `DecreaseSize`, equal → ignore.
- `SetWeight(w)` → dispatch `SetWeight { weight: w }`.

### Agent orchestration (the soul — wire with care)

Gate: `consented && !muted && api key present`. If `resolve_api_key()` is None at startup: force `muted = true`, `topbar.set_muted(true)`, show a one-time plain toast: "Muse is asleep — no API key found. Set ANTHROPIC_API_KEY and relaunch." (honest dev-mode copy; VOICE-compliant: quiet, no jargon beyond the necessary).

- Engines: `engines.entry(entry_id).or_insert_with(|| TriggerEngine::new(chattiness))`. On open: seed `note_edit(now_ms, doc_len_chars, 0, false)` (doc_len via `rope().len_chars()`).
- On `Edited`: `delta = abs(new_len_chars - last_len_chars)` (track per entry); `paragraph_completed` = char immediately before the selection head is a newline (cheap heuristic, fine); `engine.note_edit(now_ms, len, delta, para)`.
- Poll: a long-lived spawned task ticking every 500ms (gpui timer loop on the workspace entity, weak handle so it dies with the window): if gated-off → skip; if `engine.poll(now_ms)` → `mark_considered(now_ms)` and run a consideration:
  1. `topbar.set_orb(Reading)`.
  2. Snapshot: `DocSnapshot { entry_id, text: editor doc plain_text, register_hint: setting("register.<id>"), active_notes: notes.clone(), dismissed_digests: store.dismissed_digests(id), last_response: setting("response.<id>") }`. Capture `doc.version()` alongside.
  3. `topbar.set_orb(Thinking)`; `api.request(build_request(&snapshot))`; `cx.spawn` awaits the oneshot (never hangs).
  4. On reply → `parse_decision`:
     - `Pass` → orb Resting; `tracing::debug` the reason.
     - `Notes(drafts)` → for each: compute digest, drop if in dismissed set; `locate_quote` against the CURRENT `plain_text()` (doc may have moved on — locate against current text, not the snapshot); if found: `NoteRecord { id: next_note_id++, quote, kind, body }` → push to `notes`, `save_notes`, `editor.add_annotation(Annotation { id, range, tone: map kind 1:1, body })`. Any added → orb HasNote, then schedule Resting after ~8s (generation-guard the timer). None survived → orb Resting.
     - `Respond { body, register }` → `editor.set_coda(Some(body))`; persist `response.<id>`; persist `register.<id>` if Some; orb Resting.
     - `Err(ApiError::MissingKey)` → mute + toast (once); other errors → orb Resting + `tracing::warn` (the writing is never interrupted; no error UI beyond the one key toast).
- `MuseNow` action → if consent + key: `engine.force()` (poll loop picks it up within 500ms; that latency is fine and keeps one code path).
- `ToggleMuseMuted` → flip, persist, `topbar.set_muted`; muted does not clear existing notes.
- Restore on open: `load_notes(id)` → `Vec<NoteRecord>` → for each `locate_quote(current_text, &quote, "", "")` → found ones become annotations (`set_annotations`, ranges against current doc); drop the lost ones from the record list and re-save. Coda: restore from `response.<id>` via `set_coda` (no reveal replay needed — setting Some replays the typewriter; acceptable, or note if distracting).

### Consent (first run)

Pre-consent overlay: centered card (~380px), VOICE.md copy:
"Muse reads what you write here so it can leave you notes — like a friend with a pencil. It never trains on your words, and you can mute it anytime, for one entry or forever."
Below: `switch("consent-muse", true)` labeled "Let Muse read along" + `text_button("begin", "Begin")`.
Begin → set `consent=given`, `muse_muted = !switch_state`, drop the overlay, focus the editor.
The editor is fully usable behind the overlay decision; the overlay blocks nothing else once dismissed. No dark patterns: the switch defaults on but muting is honest and equal-weight.

### Toasts

Bottom-center, max one at a time, `pill()` container, UI_TEXT ink on surface_lifted, fade in/out FADE, 5s auto-dismiss (generation-guarded timer). Undo variant adds `text_button("undo", "Undo")`.

### Workspace action handlers

NewEntry, ToggleSidebar (flip + start width animation), ToggleTheme (crossfade), MuseNow, ToggleMuseMuted, Quit (flush then `cx.quit()`), About (`tracing::info` no-op v1), Cancel at workspace level → `topbar.dismiss_popover` (also editor handles its own Cancel internally — workspace Cancel only fires when editor isn't focused).

## Quality bar

Same as every crate: edition 2024, zero warnings, clippy clean, no unwrap/expect outside tests,
rustdoc on public items, charter at top of each module. main.rs stays thin; workspace.rs owns glue;
split agent orchestration into `muse_flow.rs` (or similar) and persistence into `persistence.rs`.
The binary must `cargo run -p muse` and come up showing the restored entry with zero spinners.

## Verification before reporting

`cargo check -p muse` zero errors/warnings; `cargo clippy -p muse --all-targets` clean;
`cargo build` whole workspace; launch `./target/debug/muse` for ≥5s and confirm it stays alive
(you cannot see the window — confirm no crash/panic in stderr and exit cleanly via kill).
Report: wiring decisions made where this doc said "verify", any deviations, and what remains untested.
