# Daisy Notes

A native macOS writing app (Rust + GPUI) with an on-device companion that reads
along as you write and leaves small notes in the margin. Local-first — every
entry lives in SQLite on disk.

## Build & test

```sh
cargo run                       # debug build, logs to /tmp/daisynotes-debug.log
cargo test                      # full suite across the workspace
cargo clippy --workspace --all-targets
DAISYNOTES_LOCAL_E2E=1 cargo test -p daisynotes-local --test e2e -- --ignored   # real on-device generation
```

Workspace crates live in `crates/` (`app`, `core`, `editor`, `agent`, `api`,
`local`, `storage`, `theme`, `topbar`, `ui`, `commands`, `entries`); their
package names are `daisynotes-*` and the binary is `daisynotes`.

## Releasing — DO IT LOCALLY

**Stable releases are cut from a local Mac, never from CI.** Code signing
(Developer ID) and Apple notarization require the certificate and the
`daisynotes-notary` keychain profile, which live only in the local login
keychain. To ship a stable release, run `scripts/release.sh` on your machine —
it builds, signs, notarizes + staples (app and DMG), tags, pushes, and publishes
the GitHub release. CI only produces ad-hoc-signed **pre-releases** for testing;
it does not notarize. Full flow and one-time setup: see **RELEASE.md**.

## Conventions

- **The product is "Daisy Notes"; the in-app companion is "the muse."** Code
  deliberately keeps `muse`-named symbols for the companion concept —
  `muse_flow`, `tokens.muse`, `MuseNow`, `muse_muted`, the lavender "muse hue",
  the system prompt `"You are Muse."`. Do **not** rename these; only the
  product/brand was renamed from Muse → Daisy Notes.
- The accent is the landing-page light blue `#1E90FF`, and Paper (light) is the
  default appearance. Colors live in `crates/theme`; nothing reads hex literals
  elsewhere.
- Annotations vs reactions in the editor: a **note** is a subtle highlight over
  the quoted text that opens a card on hover; a **reaction** is just an emoji in
  a circle out in the right margin (no message, never over the text). Notes have
  a body and no emoji; reactions have an emoji and no body.
- Repo: `github.com/appsicle/daisynotes`. The landing download button points at
  `releases/latest/download/DaisyNotes.dmg`, served by the latest *stable*
  (locally-notarized) release — prereleases never take that slot.
