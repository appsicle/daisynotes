#!/bin/bash
# Package Muse as a shippable macOS app: release build → Muse.app bundle
# (Info.plist, icns) → ad-hoc codesign → notarization-shaped zip + DMG.
#
# Usage: scripts/package.sh [--skip-build]
# Output: dist/Muse.app, dist/Muse.dmg, dist/Muse.zip

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
DIST=dist
APP="$DIST/Muse.app"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "── building release binary ──"
  cargo build --release
fi

echo "── assembling bundle ──"
rm -rf "$APP" "$DIST/Muse.dmg" "$DIST/Muse.zip"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/muse "$APP/Contents/MacOS/Muse"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>               <string>Muse</string>
  <key>CFBundleDisplayName</key>        <string>Muse</string>
  <key>CFBundleIdentifier</key>         <string>com.muse.editor</string>
  <key>CFBundleVersion</key>            <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key> <string>${VERSION}</string>
  <key>CFBundleExecutable</key>         <string>Muse</string>
  <key>CFBundleIconFile</key>           <string>Muse</string>
  <key>CFBundlePackageType</key>        <string>APPL</string>
  <key>LSMinimumSystemVersion</key>     <string>13.0</string>
  <key>LSApplicationCategoryType</key>  <string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key>    <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key> <true/>
  <key>NSHumanReadableCopyright</key>   <string>© 2026 Muse</string>
</dict>
</plist>
PLIST

echo "── rendering icon ──"
ICON_TMP=$(mktemp -d)
swift scripts/make-icon.swift "$ICON_TMP"
iconutil -c icns "$ICON_TMP/Muse.iconset" -o "$APP/Contents/Resources/Muse.icns"
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

# Notarize + staple when credentials are stored (one-time:
#   xcrun notarytool store-credentials muse-notary --apple-id … --team-id … --password …)
if [[ -n "$IDENTITY" ]] && xcrun notarytool history --keychain-profile muse-notary >/dev/null 2>&1; then
  echo "── notarizing ──"
  ditto -c -k --keepParent "$APP" "$DIST/Muse-notarize.zip"
  xcrun notarytool submit "$DIST/Muse-notarize.zip" --keychain-profile muse-notary --wait
  xcrun stapler staple "$APP"
  rm -f "$DIST/Muse-notarize.zip"
fi

echo "── archiving ──"
ditto -c -k --keepParent "$APP" "$DIST/Muse.zip"

# The installer DMG: Muse.app beside /Applications on the styled paper
# background (rose arrow, "drag Muse into Applications"). The app is baked
# in at image-creation time (macOS 26 denies copying bundles onto mounted
# images), then style-dmg.py writes the .DS_Store onto the mounted volume.
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
swift scripts/make-dmg-background.swift "$STAGE/.background.png"

RW=$(mktemp -d)/Muse-rw.dmg
hdiutil create -volname "Muse" -srcfolder "$STAGE" -ov -format UDRW -quiet "$RW"
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
hdiutil convert "$RW" -format UDZO -o "$DIST/Muse.dmg" -ov -quiet
rm -rf "$STAGE" "$(dirname "$RW")"

echo
echo "shipped:"
du -sh "$APP" "$DIST/Muse.dmg" "$DIST/Muse.zip"
