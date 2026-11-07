#!/bin/bash
# Bouwt "FM27 Trainer.app" in ./dist (release, ad-hoc ondertekend).
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p fm27-trainer
# module voor in FM27: universeel (arm64 + x86_64), want FM27 is universeel
cargo build --release -p fm27-agent --target aarch64-apple-darwin
cargo build --release -p fm27-agent --target x86_64-apple-darwin
mkdir -p target/release
lipo -create -output target/release/libfm27_agent.dylib \
  target/aarch64-apple-darwin/release/libfm27_agent.dylib \
  target/x86_64-apple-darwin/release/libfm27_agent.dylib
codesign --force --sign - target/release/libfm27_agent.dylib
APP="dist/FM27 Trainer.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/fm27-trainer "$APP/Contents/MacOS/FM27 Trainer"
cp target/release/libfm27_agent.dylib "$APP/Contents/MacOS/libfm27_agent.dylib"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>FM27 Trainer</string>
  <key>CFBundleDisplayName</key><string>FM27 Trainer</string>
  <key>CFBundleIdentifier</key><string>local.fm27.trainer</string>
  <key>CFBundleExecutable</key><string>FM27 Trainer</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleDevelopmentRegion</key><string>nl</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSScreenCaptureUsageDescription</key><string>FM27 Trainer leest het Tactical Effectiveness-scherm van Football Manager uit.</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$APP"
echo "Klaar: $APP"
