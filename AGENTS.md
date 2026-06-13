# AGENTS.md

Guidance for AI coding agents working in this repo (Codex, Claude, and others).
See **CLAUDE.md** for the project overview and dev commands, and **RELEASE.md**
for the full release runbook.

## Releases are LOCAL, not CI

Stable releases (Developer-ID signed + Apple-notarized) are cut on a local Mac
with `scripts/release.sh`. Signing and notarization need the Developer ID
certificate and the `daisynotes-notary` keychain profile (legacy fallback:
`muse-notary`), which exist only in the local login keychain — **never expect
CI to sign or notarize.** CI (`.github/workflows/release.yml`) intentionally
ad-hoc signs and publishes testing **pre-releases** only.

If asked to "release", "notarize", or "ship", do it locally with
`scripts/release.sh`. Do not try to make CI notarize unless explicitly asked to
wire up signing secrets (the steps are in RELEASE.md).

## Naming: do not "fix" the muse references

The product is **Daisy Notes**; the in-app companion is still **"the muse."**
Symbols like `muse_flow`, `tokens.muse`, `MuseNow`, `muse_muted`, and the
`"You are Muse."` system prompt are intentional and must stay. Only the brand
was renamed from Muse → Daisy Notes — leave the companion concept alone.

## Build & test

```sh
cargo test                          # full suite
cargo clippy --workspace --all-targets
```

Crate package names are `daisynotes-*`; the binary is `daisynotes`. Theme colors
(accent is light blue `#1E90FF`, light mode default) live in `crates/theme` —
nothing reads hex literals elsewhere.
