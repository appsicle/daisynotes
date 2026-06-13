# Daisy Notes

A quiet place to write, with a friend in the margin.

**[daisynotes.app](https://daisynotes.app)** · [Download for macOS](https://github.com/appsicle/daisynotes/releases/latest/download/DaisyNotes.dmg)

Daisy Notes is a native macOS writing app — Rust + [GPUI](https://www.gpui.rs) (Zed's
GPU-accelerated UI framework) — with a companion who reads alongside you and,
when there's genuinely something to say, leaves a small note: a quiet highlight
on the exact passage that opens its card on hover, or a reaction emoji resting
out in the margin. Most of the time it stays silent.

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
scripts/release.sh   # the above, then tag the version and publish the GitHub release
```

`package.sh` assembles the bundle (Info.plist + icns from `scripts/make-icon.swift`).
When a Developer ID certificate and a notary keychain profile are present, it
signs with the Developer ID and **notarizes + staples both the app and the DMG**;
otherwise it ad-hoc signs (fine for local runs).

**Stable releases are cut locally**, not in CI — the certificate and notary
credentials live in the local keychain. `scripts/release.sh` builds, notarizes,
tags `v$VERSION`, and publishes the release whose `DaisyNotes.dmg` the landing
page serves. CI only publishes ad-hoc pre-releases for testing. See
[RELEASE.md](RELEASE.md) for the full runbook and one-time setup.

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

`marketing/` is the landing page — a Vite static site deployed to Cloudflare
Pages at [daisynotes.app](https://daisynotes.app) (see `.github/workflows/marketing.yml`).

The companion concept is still called **"the muse"** throughout the code
(`muse_flow`, `tokens.muse`, the `MuseNow` action) — only the product was
renamed from Muse to Daisy Notes. See [CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md).
