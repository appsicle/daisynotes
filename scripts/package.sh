#!/bin/bash
# Package Daisy Notes as a shippable macOS app: release build → DaisyNotes.app bundle
# (Info.plist, icns) → ad-hoc codesign → notarization-shaped zip + DMG.
#
# Usage: scripts/package.sh [--skip-build]
# Output: dist/DaisyNotes.app, dist/DaisyNotes.dmg, dist/DaisyNotes.zip

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
DIST=dist
APP="$DIST/DaisyNotes.app"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "── building release binary ──"
  cargo build --release
fi

echo "── assembling bundle ──"
rm -rf "$APP" "$DIST/DaisyNotes.dmg" "$DIST/DaisyNotes.zip"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/daisynotes "$APP/Contents/MacOS/DaisyNotes"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>               <string>Daisy Notes</string>
  <key>CFBundleDisplayName</key>        <string>Daisy Notes</string>
  <key>CFBundleIdentifier</key>         <string>com.daisynotes.editor</string>
  <key>CFBundleVersion</key>            <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key> <string>${VERSION}</string>
  <key>CFBundleExecutable</key>         <string>DaisyNotes</string>
  <key>CFBundleIconFile</key>           <string>DaisyNotes</string>
  <key>CFBundlePackageType</key>        <string>APPL</string>
  <key>LSMinimumSystemVersion</key>     <string>13.0</string>
  <key>LSApplicationCategoryType</key>  <string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key>    <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key> <true/>
  <key>NSHumanReadableCopyright</key>   <string>© 2026 Daisy Notes</string>
</dict>
</plist>
PLIST

echo "── rendering icon ──"
ICON_TMP=$(mktemp -d)
swift scripts/make-icon.swift "$ICON_TMP"
iconutil -c icns "$ICON_TMP/DaisyNotes.iconset" -o "$APP/Contents/Resources/DaisyNotes.icns"
rm -rf "$ICON_TMP"

# Sign with Developer ID when one is in the keychain (hardened runtime,
# required for notarization); fall back to ad-hoc for local builds.
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
  | grep -m1 -o '"Developer ID Application[^"]*"' | tr -d '"' || true)
if [[ -n "$IDENTITY" ]]; then
  echo "── signing ($IDENTITY) ──"
  codesign --force --deep --options runtime --timestamp --sign "$IDENTITY" "$APP"
else
  echo "── signing (ad-hoc; no Developer ID in keychain) ──"
  codesign --force --deep --sign - "$APP"
fi
codesign --verify --strict "$APP"

# Notarize + staple when credentials are stored. Prefer the daisynotes-notary
# keychain profile; fall back to the legacy muse-notary profile from before the
# rename (one-time setup:
#   xcrun notarytool store-credentials daisynotes-notary --apple-id … --team-id … --password …)
NOTARY_PROFILE=""
if [[ -n "$IDENTITY" ]]; then
  for _p in daisynotes-notary muse-notary; do
    if xcrun notarytool history --keychain-profile "$_p" >/dev/null 2>&1; then
      NOTARY_PROFILE="$_p"
      break
    fi
  done
fi

if [[ -n "$NOTARY_PROFILE" ]]; then
  echo "── notarizing app ($NOTARY_PROFILE) ──"
  ditto -c -k --keepParent "$APP" "$DIST/DaisyNotes-notarize.zip"
  xcrun notarytool submit "$DIST/DaisyNotes-notarize.zip" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP"
  rm -f "$DIST/DaisyNotes-notarize.zip"
fi

echo "── archiving ──"
ditto -c -k --keepParent "$APP" "$DIST/DaisyNotes.zip"

# The installer DMG: DaisyNotes.app beside /Applications on the styled paper
# background (glossy blue arrow, "Drag Daisy Notes into Applications"). The app is baked
# in at image-creation time (macOS 26 denies copying bundles onto mounted
# images), then style-dmg.py writes the .DS_Store onto the mounted volume.
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
swift scripts/make-dmg-background.swift "$STAGE/.background.png"

RW=$(mktemp -d)/DaisyNotes-rw.dmg
hdiutil create -volname "Daisy Notes" -srcfolder "$STAGE" -ov -format UDRW -quiet "$RW"
MOUNT=$(hdiutil attach "$RW" -readwrite -noverify -noautoopen | grep -o '/Volumes/.*')

# ds_store + mac_alias live in the dmgbuild pipx venv (pipx install dmgbuild).
STYLER=$(ls "$HOME/.local/pipx/venvs/dmgbuild/bin/python" 2>/dev/null || true)
if [[ -n "$STYLER" ]]; then
  "$STYLER" scripts/style-dmg.py "$MOUNT" || echo "(DMG styling failed; plain layout)"
else
  echo "(pipx install dmgbuild for the styled DMG window; plain layout)"
fi

sync
hdiutil detach "$MOUNT" -quiet
hdiutil convert "$RW" -format UDZO -o "$DIST/DaisyNotes.dmg" -ov -quiet
rm -rf "$STAGE" "$(dirname "$RW")"

# Staple the DMG itself: the app inside is already notarized, but notarizing
# the final image and attaching its own ticket lets the downloaded .dmg clear
# Gatekeeper offline (no round-trip to Apple on first open).
if [[ -n "$NOTARY_PROFILE" ]]; then
  echo "── notarizing dmg ($NOTARY_PROFILE) ──"
  xcrun notarytool submit "$DIST/DaisyNotes.dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DIST/DaisyNotes.dmg"
fi

echo
echo "shipped:"
du -sh "$APP" "$DIST/DaisyNotes.dmg" "$DIST/DaisyNotes.zip"
