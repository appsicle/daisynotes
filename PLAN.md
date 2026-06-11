# Muse

*A quiet place to write, with a companion in the margin.*

> Working codename. "Muse" fits the product (the agent **is** the muse, living in the margin) — run a trademark check before shipping. Alternates considered: Marginalia, Vellum, Petal.

---

## 1. Vision

A native macOS writing app that feels like Apple made it — one centered page, a handful of beautiful fonts, and an agent that reads alongside you and leaves thoughtful notes in the margin. It adapts to *what* you're writing: it critiques an essay, reflects on a journal entry, checks a derivation, responds to a letter.

**It is not a work tool.** No toolbars of forty buttons, no collaboration cursors, no "workspace." It is closer to a beautiful paper notebook than to Google Docs. iMessage, not Google Chat. Slack, not Teams. Soft, warm, precise.

### Product principles (every decision is tested against these)

1. **Instant, always.** No loading spinners exist anywhere in the app. Every pixel renders from local state; the network is invisible background machinery.
2. **Nothing ever shifts.** Text never jumps. Chrome never resizes itself. Transient UI lives in overlays. The only layout changes are ones the user explicitly initiates, and those are animated.
3. **Calm motion.** Animation explains, never decorates. Springs, fades, blooms — 150–320ms, then stillness.
4. **Opinionated.** One column. One accent color. Four fonts. Font family and size apply to the whole entry (its "voice"); bold/italic/color apply to selections. Fewer choices, better defaults.
5. **The margin is alive.** The agent is a presence (a small breathing orb), not a chatbot panel. It mostly says nothing. When it speaks, it's worth reading.
6. **Words are sacred.** Autosave always, sync never loses data, conflicts fork rather than overwrite, delete is soft with undo.

---

## 2. Product spec (v1)

### The page
- Single centered column, max 640px at default size (~66ch), generous top padding (96px) so the first line sits calmly.
- A small tertiary-colored date label ("Tuesday, June 10") sits above the entry inside the column — fixed height, part of the content, never shifts.
- No separate title field. The first line *is* the title (Apple Notes convention) and names the entry in the sidebar.

### Writing & formatting
- **Entry-level "voice":** font family, base size (13–28pt stepper), base weight (variable-font slider 300–700) — set from the topbar `Aa` popover, applies to the whole entry.
- **Range-level styling:** bold, italic, underline, strikethrough, and one of four ink colors — applied via a floating pill toolbar that blooms above any selection (Medium-style), and via ⌘B/⌘I/⌘U.
- Full text fundamentals: click/drag selection with autoscroll, double-click word, triple-click paragraph, shift-click extend, ⌘A, copy/cut/paste (plain + RTF flavors), undo/redo with smart grouping, IME & dictation support, emoji.
- Smooth animated caret (critically-damped spring, ~60ms settle), gentle fading blink. Rounded-corner selection highlight (iMessage-like).

### Entries (left sidebar)
- Toggleable sidebar (⌘\), 260px, listing entries sorted by **last edit** (opening doesn't reorder).
- Grouped: Today / Yesterday / Previous 7 Days / Earlier. Row = title + relative time.
- ⌘N new entry. Right-click → delete (soft delete, 5s undo toast). Switching entries renders in one frame.

### The agent ("Muse")
- A small orb in the topbar: resting (slow 4s breathe) → reading (shimmer) → thinking (pulse) → has-a-note (accent dot).
- **Proactive:** after a natural pause in writing, Muse may leave 0–2 margin notes anchored to specific passages, or (for journal-like writing) a single reflective **response** below a divider at the end of the entry. Most of the time it chooses silence.
- **Genre-aware:** classifies the register (academic / journal / story / letter / math / list / notes) and adapts both *what* it says and *how* — critique and citations for an essay, warmth and reflection for a journal, step-checking for math.
- Notes live in a right margin layer as soft dots that bloom into cards on hover — they **never** push or reflow the text. Dismissed notes are remembered and never repeated. Notes whose anchor text is deleted "wither" (fade out).
- Streaming: note text writes itself in word by word. The placeholder is a glowing margin dot, never a spinner.
- Chattiness setting: Quiet / Occasional / Chatty. Per-entry mute. Clear first-run consent moment (it reads your writing; here's the off switch).

### Theme
- Light ("Paper") and dark ("Dusk"), both warm-toned. Toggle in topbar: sun⇄moon icon morph + full-window 240ms color crossfade interpolated in OKLCH. System-follow option.

### Cloud
- Everything autosaves locally instantly; a background sync engine mirrors entries to the cloud. A tiny passive glyph indicates sync state. Zero modal sync UI, zero blocking on network, fully usable offline forever.

### Explicitly **not** in v1
Collaboration, comments-by-humans, images/attachments, markdown export (v1.1), tags/folders, search (v1.1, FTS5 is already in the schema), LaTeX/math *rendering* (the agent can still discuss math), RTL/BiDi text (honest limitation, noted in Risks), Windows/Linux.

---

## 3. Stack decision

### UI framework: **GPUI** (Apache-2.0, from Zed Industries)

| Option | Verdict |
|---|---|
| **GPUI** | ✅ **Chosen.** The only Rust UI framework with shipped proof (Zed) of Apple-grade quality: Metal-rendered, Core Text shaping, 120Hz ProMotion, sub-frame input latency, IME support, custom window chrome, native menus. Pure-Rust codebase. |
| Tauri + web editor | Easiest rich text (ProseMirror), but the UI would be TypeScript in a WKWebView — compromises "extremely fast without compromise," invites layout shift, and isn't really a Rust app. |
| Raw AppKit via objc2 | Most native (TextKit 2 gives rich text for free), but the codebase becomes thousands of `unsafe` `msg_send!` calls — the opposite of pristine. |
| Iced / Slint / egui / Floem | None have a credible path to a rich-text editor of this quality; egui's immediate-mode text and font rendering won't hit the bar. |

The honest cost of GPUI: **we build the rich-text editor ourselves** (Zed's editor is monospace-code-specific). That's the project's long pole and is planned in depth in §5. We also accept GPUI's API churn by pinning the version, and weak accessibility for now (see Risks).

Rationale notes:
- GPUI is a hybrid immediate/retained model: views are rebuilt per frame from `Entity` state, layout via flexbox (Taffy), everything GPU-drawn. This makes "never shifts, always 120fps" an engineering target we fully control rather than a fight with someone else's widget.
- Text shaping goes through Core Text, so we get macOS-quality glyph rendering, font fallback, and emoji.
- Pin via crates.io release if current, otherwise a git tag of `zed-industries/zed`; upgrade deliberately, never `*`.

### Async story
GPUI has its own foreground/background executors. Networking (Claude API + sync) runs on a **dedicated tokio thread** owned by the `api` crate; it talks to the app exclusively through channels. UI code never awaits the network.

### Key crates
| Concern | Crate | Why |
|---|---|---|
| UI / windowing / text | `gpui` | See above |
| Rope | `ropey` | Mature, well-tested rope; O(log n) edits |
| Word/grapheme boundaries | `unicode-segmentation` | Correct double-click & caret motion |
| Storage | `rusqlite` (bundled, WAL) | Synchronous, fast, zero-setup local truth |
| Serialization | `serde` / `serde_json` | Document + sync format |
| HTTP / streaming | `reqwest` (rustls) + `eventsource-stream`, `tokio` | Claude SSE streaming, sync |
| Errors | `thiserror` (libs) / `anyhow` (app edges) | Typed errors at boundaries |
| Time | `jiff` | Modern, correct date handling for grouping |
| IDs | `ulid` | Time-sortable entry IDs |
| Color math | hand-rolled OKLCH (tiny) | Theme interpolation |
| Logging | `tracing` + file appender | Debuggability without a console |
| macOS interop | `objc2`, `objc2-app-kit` (narrow use) | NSPasteboard RTF flavors, Keychain, spellcheck later |
| Icons | bundled Lucide SVG subset (ISC) | Thin-stroke, consistent, themeable |

Backend (Milestone 6): `axum` + `sqlx`/Postgres on Fly.io. Kept deliberately tiny (§7).

---

## 4. Architecture & module layout

Cargo workspace; one crate per module; **no cyclic dependencies, enforced by the dependency graph itself.**

```
muse/
├── Cargo.toml                # workspace, shared lints, release profile
├── PLAN.md
├── assets/                   # fonts/, icons/, app icon
└── crates/
    ├── app/                  # main binary: window, menu bar, composition root, settings
    ├── core/                 # THE document model: rope, spans, ops, anchors, undo. Zero UI deps.
    ├── editor/               # text layout, rendering, input, caret/selection, clipboard
    ├── commands/             # command registry, keymap, undo stack orchestration
    ├── entries/              # sidebar UI + entry list state
    ├── topbar/               # chrome: Aa popover, theme toggle, muse orb, sidebar toggle
    ├── agent/                # trigger engine, prompts, anchoring, comment/response state
    ├── api/                  # Claude client (SSE) + sync client + tokio thread. Zero UI deps.
    ├── storage/              # SQLite: schema, autosave, entry store, migrations
    ├── theme/                # design tokens, palettes, OKLCH animation
    └── ui/                   # shared primitives: buttons, popovers, toasts, springs, icons
```

Dependency graph (arrows = "depends on"):

```
app ─→ {editor, entries, topbar, agent, commands, storage, theme, ui}
editor ─→ {core, commands, theme, ui}
agent  ─→ {core, api}          # reads snapshots, emits suggestions; never touches UI directly
entries, topbar ─→ {theme, ui, commands}
commands ─→ core
storage ─→ core
core, api, theme ─→ (leaf crates)
```

### The one law: all mutation flows through commands
Every change to a document — a keystroke, a paste, a ⌘B, a programmatic fix — is a `Command` producing `core::Op`s applied to the document. This gives us, for free: a correct undo stack, a single choke point for autosave dirtying, a single event stream (`DocumentEvent`) that the agent, the sidebar (title/preview/touched-at), and the sync engine all subscribe to. The agent **observes** the document via snapshots and **suggests**; it never holds a reference to UI.

---

## 5. The editor (deep dive — this is the long pole)

### Document model (`core`)
```rust
Document {
    id: Ulid,
    text: ropey::Rope,                  // the words
    base: VoiceStyle,                   // entry-level: family, size, weight
    spans: SpanSet,                     // range-level: bold/italic/underline/strike/color
    version: u64,                       // bumps per op; anchors & agent snapshots key off it
}
```
- `SpanSet`: sorted, coalesced, non-overlapping runs of `InlineStyle` bitflags + optional color. Edits transform span boundaries via the op's delta. Property-tested (insert/delete/style at random; invariants: sorted, coalesced, in-bounds).
- `Op` = `Insert{at, text}` | `Delete{range, deleted}` | `Restyle{range, before, after}` | `Rebase{before, after}` (voice change). Every op carries enough to invert — undo is exact.
- **Anchors**: positions/ranges that survive edits (maintained by applying each op's delta to a registered anchor list). Used by agent comments, selection-restore on undo, and scroll position.
- **Undo**: ops grouped by time gap (>750ms) or boundary (word, command type change). Stores inverse ops + selection before/after. Session-scoped in v1.
- Serialization: `{ base, text, spans }` as compact JSON, plus a plain-text mirror column in SQLite for future FTS. Format versioned from day one (`"v": 1`).

### Layout & rendering (`editor`)
- **Paragraph-keyed shaped-line cache.** Text is shaped (via GPUI/Core Text) per paragraph into positioned glyph runs; an edit invalidates only its paragraph. Mixed inline styles become multiple `TextRun`s within the paragraph.
- **Virtualization with exact heights.** Only visible ± overscan paragraphs are shaped for *drawing*, but paragraph **heights are computed eagerly in idle time and cached** (and persisted alongside the doc). Estimated heights cause scrollbar jumps — that's layout shift, so we don't estimate. A 50k-word entry is ~ thousands of paragraphs; idle measurement finishes in tens of ms and is amortized across edits.
- Caret: drawn in the accent color, 2px rounded; position animated with a critically-damped spring; blink is an opacity ease, not a hard toggle. Caret animation **never** delays the glyph — the character appears in the same frame as the keystroke; only the caret eases.
- Selection: per-line rounded rects (4px radii, accent @ 18%), merged across wrapped lines.
- Scrolling: GPUI-native trackpad momentum; overlay scrollbar (never reserves width).

### Input
- `EntityInputHandler` (GPUI's IME protocol): marked text, dead keys, dictation, emoji picker all flow correctly.
- Keymap lives in `commands`: declarative `keystroke → CommandId` table; native macOS menu bar (GPUI menus) dispatches the same commands, so menus/shortcuts can never diverge.

### Clipboard
- Copy writes **three flavors** via NSPasteboard (`objc2`): plain text, RTF (with our styles mapped), and our native JSON flavor (perfect round-trip within Muse).
- Paste prefers native flavor → RTF (sanitized into our limited style model — foreign fonts/sizes are *dropped*, inline B/I/U/color kept; opinionated) → plain.

### Acceptance bar for the editor milestone
Typing at 120Hz with zero dropped frames in a 50k-word document; keystroke-to-glyph < 8ms; all interactions in §2 work; fuzz tests (random op sequences) hold all invariants; undo round-trips byte-exact.

---

## 6. The agent layer (deep dive)

### Trigger engine (local, zero-cost heuristics — no model call to decide)
Fire a consideration when **all** hold:
- ≥ ~40 chars of meaningful delta since last consideration (or a paragraph was completed),
- 2.5s of idle after typing,
- ≥ 45s since the last consideration for this entry,
- entry length above a small floor, agent not muted, chattiness budget not exhausted.

### Pipeline
```
DocumentEvent stream → TriggerEngine → Snapshot {full text, voice, genre cache,
    active comments, dismissed-comment digests, last response}
  → api::muse (tokio thread) → Claude Messages API (streaming, tool choice)
  → AgentEvent stream → margin UI / response block
```

- **One call does everything.** The model receives the snapshot and a rubric, and must choose exactly one tool:
  - `pass { reason }` — the expected default; rubric says "most of the time, the right move is silence."
  - `leave_notes { notes: [{ quote, prefix, suffix, kind, body }] }` — max 2; `kind ∈ insight | question | encouragement | correction | reference`.
  - `respond { body }` — a single end-of-entry reflective response (journal/letter registers).
  - Genre classification happens inside the same call and is returned for caching.
- Model: `claude-fable-5` default, configurable. Streaming via SSE so note bodies write themselves in.
- **Anchoring** (Hypothesis-style): each note carries an exact quote + 16-char prefix/suffix. Client locates the range, registers a `core` anchor; thereafter edits move it for free. If the quoted text is destroyed, the note withers (fades out, archived). Dismissals store a digest (quote-hash + kind) the model is told never to repeat.
- **Restraint rules in the system prompt:** match the register; never comment on spelling/grammar minutiae; never moralize a journal entry; questions over directives; at most 2 notes; reference prior dismissals as a signal to stay quieter.

### Personality
One voice, warm and brief — a well-read friend, not an assistant. Register adaptation changes *content*, not persona. Sample register behaviors: academic → counterarguments, missing citations, structural notes; journal → reflection, a gentle question, naming a feeling; math → recompute and verify steps, flag the broken one; story → continuity, texture, "what does she want here?"

### Keys, cost, privacy
- v0 (development): `ANTHROPIC_API_KEY` from macOS Keychain or env.
- v1 (product): calls route through our backend proxy (`POST /muse`) — users never handle keys; usage metered server-side.
- Chattiness budgets cap calls/hour/entry. First-run consent card; per-entry mute; TLS everywhere; no training on user content.
- The agent thread can never affect input latency — it shares nothing with the render path except channels.

---

## 7. Storage & sync

### Local (the truth)
SQLite via `rusqlite`, WAL mode, `synchronous=NORMAL`:
```sql
entries(id TEXT PK, title TEXT, preview TEXT, voice JSON,
        created_at INT, touched_at INT, deleted_at INT NULL,
        rev INT, synced_rev INT)
bodies(entry_id TEXT PK, doc JSON, plain TEXT)          -- plain mirrors text for future FTS5
agent_notes(id, entry_id, kind, body, quote, state, created_at)
meta(key, value)                                        -- schema version, device id, sync cursor
```
- **Autosave:** dirty-flag on every command; flush at 400ms debounce, on entry switch, on window blur, on quit. A flush is a single-row write — sub-millisecond.
- **Launch path:** open window → synchronously read entry metadata list + last-open entry body (one indexed read each, <2ms) → first frame is the real app with real words. This is why there are no spinners: the truth is always 2ms away.

### Sync (Milestone 6, but the schema above is sync-ready from day one)
- Engine: push entries where `rev > synced_rev`, debounced 3s after save; pull on launch + window focus, using a server cursor.
- Protocol (tiny REST): `POST /auth/device`, `GET /entries?since=`, `PUT /entries/:id` with `If-Match: rev`, `POST /muse` (agent proxy).
- **Conflict policy: never lose words.** `412 Precondition Failed` → keep the server version under the original entry, fork the local copy as a sibling entry "(edited on this Mac)". Both visible, user reconciles. CRDTs (automerge) only if real multi-device co-editing demand appears.
- Backend: smallest possible Axum + Postgres service on Fly.io. Auth: device token v1 → Sign in with Apple v1.1. Roadmap: E2E encryption (journals are intimate); minimum bar at launch is TLS + per-user isolation + encrypted disks.
- Sync status UI: a tiny cloud-check glyph in the sidebar footer, passively updated. Failures retry with backoff silently; persistent failure shows a quiet, dismissible toast — never a modal, never a blocked save.

---

## 8. Design system

### Mood
A quiet morning desk. Warm paper, soft ink, one rose accent, one lavender voice for the muse. Generous whitespace. Nothing shiny, nothing gray-corporate.

### Color tokens (all UI reads tokens; tokens animate, so theme-switching is a pure crossfade)

| Token | Paper (light) | Dusk (dark) |
|---|---|---|
| `bg` | `#FAF8F5` | `#171512` |
| `surface` | `#FFFFFF` | `#1F1C18` |
| `ink` | `#26221C` | `#EDE9E2` |
| `ink.secondary` | `#6F6A61` | `#A39D92` |
| `ink.tertiary` | `#A8A296` | `#6E695F` |
| `hairline` | `#ECE8E1` | `#2A2722` |
| `accent` (rose clay) | `#B86450` | `#E0907C` |
| `selection` | accent @ 18% | accent @ 22% |
| `muse` (lavender) | `#6E6AA8` | `#A8A4DE` |
| shadow | `0 1px 3px rgba(28,25,20,.06), 0 8px 24px rgba(28,25,20,.05)` | lifted-surface + hairline instead |

Rules: exactly **one** accent + **one** agent hue; ink colors for selected-text are 4 curated swatches (ink, rose, lavender, a moss green `#5F7A5A`/`#8FAE89`); both palettes warm-biased — never pure black/white/gray.

### Typography
- **UI chrome:** SF Pro via system (13pt primary / 11pt secondary) — instant Apple-feel, zero bundling.
- **Content fonts, bundled (all SIL OFL, variable where available):**
  1. **Literata** — serif, the default. Designed for long-form reading; warm, bookish.
  2. **Inter** — clean humanist sans.
  3. **iA Writer Quattro** — soft semi-mono; the journal voice.
  4. **JetBrains Mono** — true mono for notes/technical.
- Content defaults: 16pt, line-height 1.65, 640px column. Size steps: 13 14 15 16 18 20 24 28. Weight slider 300–700 (variable axes; static instances as fallback if GPUI's variable-axis support disappoints).
- Fonts load **before** the first frame (bundled bytes registered at startup) — font-swap shift is impossible by construction.

### Iconography & chrome
- Lucide subset, 1.5px stroke, drawn at token colors. Hidden titlebar, inset traffic lights (12px), 52px topbar, hairline only appears under the topbar once content has scrolled (a fade, not a pop).
- Corner radii: 6px (controls), 10px (cards/popovers), 14px (sidebar panels). Subtle vibrancy/blur behind the floating selection toolbar.

### Motion system ("calm springs")
| Pattern | Spec |
|---|---|
| Movement (panels, popovers, sidebar) | spring: response 280ms, damping 0.9 — settles, never wobbles |
| Fades (hover, hairlines, toasts) | 160ms ease-out |
| Theme toggle | 240ms, all tokens interpolated in **OKLCH** (perceptually clean midpoints), sun⇄moon path morph synced |
| Caret travel | critically-damped, ~60ms; glyphs never wait for it |
| Muse note bloom | 320ms spring, scale 0.96→1 + fade, dot→card |
| Orb | 4s breathing loop at rest; tempo states for reading/thinking |
| Sidebar toggle | the one sanctioned layout animation: column re-centers along the same 280ms spring |

Hard rules: animate **transform/opacity only** (the sidebar spring is the sole exception, and it's user-initiated); nothing animates in the text column while the user is typing except the caret; every animation is interruptible and reversible mid-flight.

---

## 9. Interaction patterns catalog

| Interaction | Behavior |
|---|---|
| Launch | Window appears with last entry's real words. No splash, no skeleton. |
| Type | Glyph same frame; caret springs behind it. |
| Select (drag) | Rounded highlight grows live; autoscroll near edges. |
| Release selection | Format pill blooms above selection (B I U S · 4 ink dots · "ask Muse"). Esc or click-away fades it. |
| ⌘B/I/U, ⌘+/⌘−/⌘0 | Range styles / entry size. Topbar `Aa` popover: font cards with live previews, size stepper, weight slider. |
| New entry (⌘N) | Sidebar row slides in at top; page crossfades to blank; caret breathing on the first line. |
| Switch entry | One-frame swap, gentle 120ms content crossfade. |
| Delete entry | Row collapses (height spring), undo toast 5s. |
| Theme toggle | Icon morph + whole-window OKLCH crossfade, 240ms. |
| Muse considers | Orb shifts to thinking pulse. Usually returns to rest (it passed). |
| Muse notes | Lavender dot fades in at the anchored line's margin; hover/click blooms the card; text streams in. Dismiss = card recedes into the dot, gone. |
| Muse responds (journal) | Hairline divider draws in below the entry; response streams beneath in lavender-tinted italic block. |
| Anchor text deleted | The note withers — 400ms fade, archived. |
| Offline | Nothing changes. Cloud glyph shows a quiet pause icon. |
| Quit/reopen | Exact restoration: entry, scroll position, selection. |

---

## 10. Performance engineering

### Budgets (CI-enforced where measurable, Instruments-verified otherwise)

| Metric | Budget | Target |
|---|---|---|
| Cold launch → first contentful frame | < 250ms | 150ms |
| Keystroke → glyph | < 8ms (one 120Hz frame) | 4ms |
| Scroll, 50k-word entry | 120fps, zero dropped frames | — |
| Entry switch → content | < 16ms | one frame |
| Theme crossfade frame time | < 8.3ms throughout | — |
| Memory, typical session | < 150MB | 100MB |
| Agent impact on input latency | 0 (different thread, channels only) | 0 |

### How the budgets are met
- GPU-drawn UI (Metal), damage-driven redraws, glyph atlas caching (GPUI).
- Paragraph-incremental shaping; exact-height virtualization (§5) — no estimation, no scroll jumps.
- Synchronous 2ms SQLite reads on the launch path; everything network strictly off it.
- Release profile: `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, stripped. Dev profile stays fast to keep iteration joyful.
- `cargo flamegraph` + Instruments sessions are a standing part of every milestone's exit criteria, not a post-hoc rescue.

### The no-spinner / no-shift doctrine (restated as build rules)
1. Local state is always complete enough to render — design every feature's data model so this is true *first*.
2. Overlays for everything transient (toolbars, popovers, notes, toasts).
3. All chrome has fixed dimensions; fonts pre-registered; scrollbars overlay.
4. Network results arrive as *quiet upgrades* to existing pixels (a sync glyph changes; a note blooms), never as something the user waits for.

---

## 11. Codebase standards ("equally pristine")

- **Boundaries:** the dependency graph in §4 is law. `core` and `api` compile with no UI deps. The editor knows nothing about the agent; the agent knows nothing about pixels.
- **Lints:** workspace-level `clippy` with `pedantic` (curated allows), `unwrap_used`/`expect_used` denied outside tests, `rustfmt` enforced. `unsafe` only inside `objc2` interop modules, each block commented with its invariant.
- **Errors:** `thiserror` enums at crate boundaries; `anyhow` only at the app edge; every background failure path ends in a `tracing` event, never a silent drop.
- **Naming/docs:** every crate has a `//!` charter doc stating what it owns and what it must not know about. Public items documented; no `mod.rs` grab-bags.
- **Tests:** property tests for `core` (rope+spans+undo invariants), unit tests for anchoring transforms and trigger heuristics, GPUI `TestAppContext` integration tests for editor input, golden-file tests for serialization. CI (GitHub Actions, macOS runner): fmt + clippy `-D warnings` + tests on every push.
- **Commits:** conventional, small; every milestone lands demoable.

---

## 12. Milestones

| # | Scope | Exit criteria |
|---|---|---|
| **M0 — Foundation** (wk 1) | Workspace scaffold, all crates stubbed with charters, CI green. Window with hidden titlebar + inset traffic lights, topbar shell, theme tokens both palettes (static switch), fonts bundled & registered, Lucide pipeline. | Launches <250ms; 120fps empty scroll; clippy/fmt/CI gates live. |
| **M1 — Text core** (wk 2–4, the long pole) | `core` document model (rope, spans, ops, undo, anchors) + `editor` layout/render/input: typing, caret, mouse selection, clipboard (plain), IME, virtualization. | Editor acceptance bar (§5). Property tests green. |
| **M2 — Rich text** (wk 5) | Inline styles, floating format pill, `Aa` popover (family/size/weight), 4 ink colors, RTF copy/paste, keymap + native menus. | All §2 formatting works; paste from Pages/Notes sanitizes correctly. |
| **M3 — Entries & storage** (wk 6) | SQLite schema, autosave, sidebar (groups, soft delete + undo toast), launch restoration, date label. | Kill -9 mid-typing loses ≤400ms of work; entry switch one frame. |
| **M4 — Theme & motion polish** (wk 7) | OKLCH token animation, sun/moon morph, caret spring, selection polish, orb (visual states only), toasts. | Every §9 interaction matches spec; Instruments shows no frame >8.3ms during crossfade. |
| **M5 — The agent** (wk 8–9) | `api` Claude client (SSE, tokio thread), trigger engine, tool schema, anchored margin notes + wither, response mode, dismissal memory, chattiness, consent card, Keychain key (v0). | Writing an essay vs. a journal entry produces register-appropriate behavior; input latency unaffected (measured). |
| **M6 — Sync** (wk 10–11) | Axum+Postgres service, device auth, push/pull engine, conflict-fork policy, sync glyph, agent proxy endpoint. | Two Macs converge; forced conflict forks and loses zero words; offline indefinitely is fine. |
| **M7 — Ship polish** (wk 12) | App icon, DMG, codesign + notarize, first-run experience (a designed empty state — a single inviting line, not a tutorial), settings popover, crash reporting decision. | Notarized build on a clean Mac feels like the §1 vision. |

Sequencing logic: the editor is the riskiest and most valuable artifact, so it gets built immediately after the visual foundation exists to judge it in. The agent lands only once anchoring infrastructure (`core` anchors, M1) and real persistence (M3) exist. Sync is last because local-first means it's pure addition.

---

## 13. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Rich-text editor underestimated (IME, wrapping, BiDi, edge cases) | **High** | It's 3 of 12 weeks with the strongest acceptance bar; scope fenced hard (entry-level voice + 5 inline attrs, no arbitrary mixed fonts); fuzz/property tests from day one; Zed's editor source as reference for GPUI text idioms. |
| GPUI API churn / docs gaps | Med | Pin exact version; vendor-friendly via git tag; isolate GPUI types behind `ui`/`editor` so churn is localized; Zed source is the documentation. |
| Variable-font axes unsupported in GPUI | Low | Fallback: ship static weight instances (Literata/Inter provide them). |
| Accessibility (VoiceOver) weak in GPUI | Med | Honest roadmap item; track GPUI's accessibility work; structure all text behind a model that can later feed NSAccessibility. |
| Agent feels naggy or creepy | **High (product)** | Restraint rubric + `pass` as default + dismissal memory + chattiness setting + consent moment; tune against real writing in M5 dogfood. |
| RTL/BiDi correctness | Med | Out of scope v1, documented; Core Text shaping handles glyphs, caret logic deferred. |
| Sync data loss | High | Conflict-fork policy (never overwrite), soft deletes, WAL, autosave debounce ≤400ms. |
| "Muse" trademark | Low | Check before ship; rename is a string + folder. |

---

## 14. Open questions (none block M0–M5)

1. **Name** — ship-name & trademark check (M7).
2. **Accounts** — Sign in with Apple timing: M6 or v1.1? (Device-token sync works without it.)
3. **E2E encryption** — at sync launch or fast-follow? Affects backend design slightly (blob storage either way keeps options open).
4. **Crash reporting** — none vs. opt-in (privacy posture says opt-in at most).
5. **Pricing posture** — subscription gating agent usage is the natural shape (backend proxy already meters); not a v1 build concern.

---

## 15. First commands (M0 kickoff)

```bash
cd ~/Desktop/muse
git init
cargo new crates/app --name muse
# workspace Cargo.toml with members = ["crates/*"], shared [workspace.lints], release profile
# add gpui (pinned), scaffold remaining crates with charter docs
# vendor fonts into assets/fonts/ (Literata, Inter, iA Writer Quattro, JetBrains Mono — all SIL OFL)
# .github/workflows/ci.yml: fmt + clippy -D warnings + test on macos-14
```
