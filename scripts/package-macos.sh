#!/usr/bin/env bash
# Builds the macOS app (one program for Intel and Apple chips) and packs it
# into dist/VersoloPDF-<version>-macos.zip. Runs on a Mac (GitHub Actions:
# .github/workflows/macos.yml), needs the Xcode command line tools.
#
# The app is signed "ad hoc" (no Apple developer account): macOS asks once
# before the first start (see the website / README).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
NAME="VersoloPDF-$VERSION-macos"
APP="dist/$NAME/VersoloPDF.app"
TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
export MACOSX_DEPLOYMENT_TARGET=11.0

for p in mac-arm64 mac-x64; do
  [ -f "vendor/pdfium/$p/lib/libpdfium.dylib" ] || bash scripts/fetch-pdfium.sh "$p"
done
for t in $TARGETS; do
  cargo build --release --locked --target "$t"
done

rm -rf "dist/$NAME" "dist/$NAME.zip"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources/licenses/pdfium"
lipo -create -output "$APP/Contents/MacOS/VersoloPDF" \
  target/aarch64-apple-darwin/release/versolopdf target/x86_64-apple-darwin/release/versolopdf
lipo -create -output "$APP/Contents/MacOS/versolopdf-engine" \
  target/aarch64-apple-darwin/release/versolopdf-engine target/x86_64-apple-darwin/release/versolopdf-engine
lipo -create -output "$APP/Contents/Frameworks/libpdfium.dylib" \
  vendor/pdfium/mac-arm64/lib/libpdfium.dylib vendor/pdfium/mac-x64/lib/libpdfium.dylib
cp README.md LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md SECURITY.md "$APP/Contents/Resources/"
cp vendor/pdfium/mac-arm64/LICENSE "$APP/Contents/Resources/licenses/pdfium/LICENSE"
cp vendor/pdfium/mac-arm64/licenses/* "$APP/Contents/Resources/licenses/pdfium/"

# Program icon from the same drawing as on Windows.
ICONSET=$(mktemp -d)/VersoloPDF.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/versolopdf.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) assets/versolopdf.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/VersoloPDF.icns"

sed "s/@VERSION@/$VERSION/g" assets/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

(cd "dist/$NAME" && ditto -c -k --keepParent VersoloPDF.app "../$NAME.zip")
shasum -a 256 "dist/$NAME.zip" | sed "s|dist/||" | tee "dist/$NAME.zip.sha256"
