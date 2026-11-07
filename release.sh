#!/bin/bash
# Release voor macOS: universele app (Apple silicon + Intel), ondertekend, genotariseerd, in een .dmg.
#
#   ./release.sh                 bouwt dist/FM27-Trainer-<versie>.dmg
#
# Ondertekenen gebeurt met het eerste "Developer ID Application"-certificaat in je sleutelhanger
# (of FM27_SIGN_IDENTITY). Zonder certificaat: ad-hoc (werkt alleen op je eigen Mac).
# Notariseren gebeurt als er een notarytool-profiel is (FM27_NOTARY_PROFILE, standaard
# "fm27-notary"); eenmalig aanmaken met:
#   xcrun notarytool store-credentials fm27-notary --apple-id <e-mail> --team-id <TEAMID>
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

VERSION=$(grep -m1 '^version' app/Cargo.toml | cut -d'"' -f2)
IDENTITY="${FM27_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | grep -m1 'Developer ID Application' | sed -E 's/.*"(.*)"/\1/' || true)}"
PROFILE="${FM27_NOTARY_PROFILE:-fm27-notary}"
# Bouwen en ondertekenen buiten iCloud/File Provider-mappen: die plakken Finder-kenmerken op
# bestanden, en codesign weigert die ("resource fork, Finder information … not allowed").
WORK="${TMPDIR:-/tmp}fm27-release"
APP="$WORK/FM27 Trainer.app"
mkdir -p dist
DMG="$PWD/dist/FM27-Trainer-$VERSION.dmg"

echo "▸ FM27 Trainer $VERSION"
for T in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release -p fm27-trainer -p fm27-agent --target "$T"
done
mkdir -p target/universal
lipo -create -output target/universal/fm27-trainer \
  target/aarch64-apple-darwin/release/fm27-trainer target/x86_64-apple-darwin/release/fm27-trainer
lipo -create -output target/universal/libfm27_agent.dylib \
  target/aarch64-apple-darwin/release/libfm27_agent.dylib target/x86_64-apple-darwin/release/libfm27_agent.dylib

echo "▸ app-bundel"
rm -rf "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/universal/fm27-trainer "$APP/Contents/MacOS/FM27 Trainer"
cp target/universal/libfm27_agent.dylib "$APP/Contents/MacOS/libfm27_agent.dylib"
cp app/assets/icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>FM27 Trainer</string>
  <key>CFBundleDisplayName</key><string>FM27 Trainer</string>
  <key>CFBundleIdentifier</key><string>app.fm27trainer</string>
  <key>CFBundleExecutable</key><string>FM27 Trainer</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>nl</string></array>
  <key>LSApplicationCategoryType</key><string>public.app-category.sports</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSScreenCaptureUsageDescription</key><string>FM27 Trainer reads the Tactical Effectiveness screen of Football Manager.</string>
</dict></plist>
PLIST

echo "▸ ondertekenen"
xattr -cr "$APP"   # extra bestandskenmerken (Finder-info, herkomst) blokkeren ondertekenen
if [ -n "$IDENTITY" ]; then
  echo "  met: $IDENTITY"
  codesign --force --timestamp --options runtime --sign "$IDENTITY" "$APP/Contents/MacOS/libfm27_agent.dylib"
  codesign --force --timestamp --options runtime --sign "$IDENTITY" "$APP"
else
  echo "  geen Developer ID-certificaat gevonden: ad-hoc (alleen voor deze Mac)"
  codesign --force --sign - "$APP/Contents/MacOS/libfm27_agent.dylib"
  codesign --force --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"

echo "▸ dmg"
STAGE="$WORK/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "FM27 Trainer" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"

if [ -n "$IDENTITY" ]; then
  codesign --force --timestamp --sign "$IDENTITY" "$DMG"
  if xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then
    echo "▸ notariseren (kan een paar minuten duren)"
    xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
    xcrun stapler staple "$DMG"
    spctl -a -t open --context context:primary-signature -v "$DMG" || true
  else
    echo "  geen notarytool-profiel '$PROFILE': niet genotariseerd (zie bovenaan dit script)"
  fi
fi
rm -rf "$WORK"
echo "✓ $DMG"
