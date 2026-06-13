# Daisy Notes

A quiet place to write, with a friend in the margin.

Daisy Notes is a native macOS writing app — Rust + [GPUI](https://www.gpui.rs) (Zed's
GPU-accelerated UI framework) — with a companion who reads alongside you and,
when there's genuinely something to say, leaves a small note pinned to your
exact words or an iMessage-style reaction on the line that earned it.

- **On-device by default** — Gemma 3 via llama.cpp on Metal; nothing leaves
  your Mac. Add a Claude API key in Settings for the smartest margin friend.
- **Local-first** — every word in SQLite on disk, saved on every pause.
- **Native, the real kind** — no web view; Core Text shaping, Metal rendering,
  sub-second cold launch.

## Develop

```sh
cargo run            # debug build, logs to /tmp/daisynotes-debug.log
cargo test           # ~240 tests across 12 crates
cargo clippy --workspace --all-targets
DAISYNOTES_LOCAL_E2E=1 cargo test -p daisynotes-local --test e2e -- --ignored   # real on-device generation (needs a downloaded model)
```

## Ship

```sh
scripts/package.sh   # release build → dist/DaisyNotes.app + DaisyNotes.dmg + DaisyNotes.zip
```

The script assembles the bundle (Info.plist + icns rendered by
`scripts/make-icon.swift`), ad-hoc signs it, and produces a DMG and a
notarization-shaped zip. For distribution beyond your own machines, replace
the ad-hoc signature with a Developer ID certificate and notarize:

```sh
codesign --force --deep --options runtime --sign "Developer ID Application: …" dist/DaisyNotes.app
xcrun notarytool submit dist/DaisyNotes.zip --keychain-profile … --wait
xcrun stapler staple dist/DaisyNotes.app
```

## Layout

| crate | owns |
|---|---|
| `app` | composition root, workspace, settings, agent orchestration |
| `core` | document model, spans, undo, anchors |
| `editor` | the writing surface: layout, paint, overlays, annotations |
| `agent` | trigger engine, prompts, decision parsing, quote anchoring |
| `api` | Claude worker thread |
| `local` | on-device worker: llama.cpp, GBNF grammar, model downloads |
| `entries` | sidebar |
| `topbar` | window chrome |
| `commands` | actions, keymap, menus |
| `storage` | SQLite (WAL) |
| `theme` | tokens, OKLCH machinery, presets |
| `ui` | shared widgets, icons, fonts |

`marketing/` holds the landing page (a single static `index.html`).
