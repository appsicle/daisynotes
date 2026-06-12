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

echo "── signing (ad-hoc) ──"
codesign --force --deep --sign - "$APP"
codesign --verify --strict "$APP"

echo "── archiving ──"
ditto -c -k --keepParent "$APP" "$DIST/Muse.zip"
hdiutil create -volname "Muse" -srcfolder "$APP" -ov -format UDZO -quiet "$DIST/Muse.dmg"

echo
echo "shipped:"
du -sh "$APP" "$DIST/Muse.dmg" "$DIST/Muse.zip"
