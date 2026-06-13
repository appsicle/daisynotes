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

echo
echo "release: $(gh release view "$TAG" --json url --jq .url)"
echo "button:  https://github.com/appsicle/daisynotes/releases/latest/download/DaisyNotes.dmg"
