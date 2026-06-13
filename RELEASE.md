# Releasing Daisy Notes

> **Signing and notarization happen on a local Mac, not in CI.** The Developer
> ID certificate and the `notarytool` credentials live in the local login
> keychain; GitHub's runners have neither. Cut every *stable* release from your
> own machine with `scripts/release.sh`.

## TL;DR — cut a stable release

```sh
scripts/release.sh
```

That runs the whole chain: build → Developer-ID sign → notarize + staple (the
app **and** the DMG) → tag `v$VERSION` → push the tag → create/replace the
GitHub release with `DaisyNotes.dmg` + `DaisyNotes.zip`. The landing-page button
(`releases/latest/download/DaisyNotes.dmg`) goes live the moment it finishes.

Bump `version` in `Cargo.toml` first if this is a new version.

## Why local, not CI

`scripts/package.sh` only signs and notarizes when two things are present in the
local keychain:

1. a **Developer ID Application** certificate (the hardened-runtime signature), and
2. a **notarytool keychain profile** — `daisynotes-notary` (preferred), or the
   legacy `muse-notary` (fallback from before the Muse → Daisy Notes rename).

CI (`.github/workflows/release.yml`, `macos-14`) has neither, so there it
**ad-hoc signs and skips notarization on purpose.** Two lanes:

| Trigger        | Who builds                       | Signature                 | Becomes `releases/latest`? |
|----------------|----------------------------------|---------------------------|----------------------------|
| push to `main` | CI                               | ad-hoc (testing only)     | no — it's a `--prerelease` |
| `v*` tag       | **you, locally** via `release.sh`| Developer ID + notarized  | yes                        |

`release.sh` creates the notarized release *before* pushing the tag, so when CI
sees the tag it finds the release already exists and leaves its assets alone. A
`v*` tag pushed **without** running `release.sh` first makes CI fall back to an
ad-hoc stable release — avoid that.

## One-time setup on a new machine

1. Install the **Developer ID Application** certificate (Team ID `Z4ZQV988QB`)
   into the login keychain. Verify:
   ```sh
   security find-identity -v -p codesigning | grep "Developer ID Application"
   ```
2. Create an **app-specific password** at
   appleid.apple.com → Sign-In and Security → App-Specific Passwords. It looks
   like `abcd-efgh-ijkl-mnop` — this is **not** your account password.
3. Store the notary credentials once:
   ```sh
   xcrun notarytool store-credentials daisynotes-notary \
     --apple-id "<your-apple-id-email>" \
     --team-id Z4ZQV988QB \
     --password "<app-specific-password>"
   ```
   Confirm it works:
   ```sh
   xcrun notarytool history --keychain-profile daisynotes-notary
   ```

## Verify a built artifact

```sh
xcrun stapler validate dist/DaisyNotes.app
xcrun stapler validate dist/DaisyNotes.dmg
spctl -a -vvv -t install dist/DaisyNotes.app   # want: source=Notarized Developer ID
```

## If you ever want CI to notarize

Add repo secrets — a base64-encoded `.p12` of the Developer ID cert plus its
password, and an App Store Connect API key (issuer id, key id, `.p8`). In the
workflow, import the cert into a temporary keychain and `store-credentials` (or
pass `--apple-id`/`--team-id`/`--password`) before the Package step.
`package.sh`'s existing `NOTARY_PROFILE` detection then fires automatically — no
script change needed.
