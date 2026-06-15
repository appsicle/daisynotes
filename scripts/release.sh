#!/bin/bash
# Cut a release: package the app, tag the version, and publish a GitHub
# release with the DMG + zip attached. The landing page's download button
# points at .../releases/latest/download/DaisyNotes.dmg, so each release goes
# live the moment this finishes (on a public repo).
#
# Usage: scripts/release.sh [--skip-build]

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
TAG="v${VERSION}"

scripts/package.sh "${1:-}"

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
  git tag -a "$TAG" -m "Daisy Notes $VERSION"
  git push origin "$TAG"
fi

if gh release view "$TAG" >/dev/null 2>&1; then
  echo "── release $TAG exists; replacing assets ──"
  gh release upload "$TAG" dist/DaisyNotes.dmg dist/DaisyNotes.zip --clobber
else
  gh release create "$TAG" dist/DaisyNotes.dmg dist/DaisyNotes.zip \
    --title "Daisy Notes $VERSION" \
    --generate-notes
fi

# ── Sparkle update feed ─────────────────────────────────────────────────────
# Sign the notarized zip, build a one-item appcast, and publish both to a
# dedicated, never-"latest" `updates` release that serves as the Sparkle feed.
# The shipped app's Info.plist SUFeedURL points at
# .../releases/download/updates/appcast.xml, so v0.1.2+ users get a seamless
# in-app update. sign_update reads the EdDSA private key from the keychain
# (foreground only, like codesign).
echo "── building Sparkle appcast ──"
SPARKLE_SIG=$(third_party/Sparkle/bin/sign_update dist/DaisyNotes.zip)
ZIP_NAME="DaisyNotes-${VERSION}.zip"
cp dist/DaisyNotes.zip "dist/${ZIP_NAME}"
ENCLOSURE="https://github.com/appsicle/daisynotes/releases/download/updates/${ZIP_NAME}"
MIN_OS=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" \
  dist/DaisyNotes.app/Contents/Info.plist 2>/dev/null || echo 13.0)
PUBDATE=$(date -u "+%a, %d %b %Y %H:%M:%S +0000")

cat > dist/appcast.xml <<XML
<?xml version="1.0" encoding="utf-8"?>
<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" version="2.0">
  <channel>
    <title>Daisy Notes</title>
    <item>
      <title>Daisy Notes ${VERSION}</title>
      <pubDate>${PUBDATE}</pubDate>
      <sparkle:version>${VERSION}</sparkle:version>
      <sparkle:shortVersionString>${VERSION}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>${MIN_OS}</sparkle:minimumSystemVersion>
      <sparkle:releaseNotesLink>https://github.com/appsicle/daisynotes/releases/tag/${TAG}</sparkle:releaseNotesLink>
      <enclosure url="${ENCLOSURE}" type="application/octet-stream" ${SPARKLE_SIG} />
    </item>
  </channel>
</rss>
XML

echo "── publishing the updates feed ──"
if ! gh release view updates >/dev/null 2>&1; then
  gh release create updates --title "Update feed" --latest=false \
    --notes "Sparkle appcast + update archives. Managed by scripts/release.sh — do not delete."
fi
gh release upload updates "dist/${ZIP_NAME}" dist/appcast.xml --clobber

echo
echo "release: $(gh release view "$TAG" --json url --jq .url)"
echo "button:  https://github.com/appsicle/daisynotes/releases/latest/download/DaisyNotes.dmg"
echo "appcast: https://github.com/appsicle/daisynotes/releases/download/updates/appcast.xml"
