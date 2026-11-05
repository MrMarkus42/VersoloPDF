#!/usr/bin/env bash
# Builds a release for Windows (x86_64) on Linux with MinGW and packs it into
# dist/VersoloPDF-<version>-windows-x64.zip
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGET=x86_64-pc-windows-gnu
NAME="VersoloPDF-$VERSION-windows-x64"
OUT="dist/$NAME"

[ -f vendor/pdfium/win-x64/bin/pdfium.dll ] || scripts/fetch-pdfium.sh win-x64
cargo build --release --target $TARGET

rm -rf "$OUT" "dist/$NAME.zip"
mkdir -p "$OUT/licenses/pdfium"
cp "target/$TARGET/release/versolopdf.exe" "$OUT/VersoloPDF.exe"
cp vendor/pdfium/win-x64/bin/pdfium.dll "$OUT/"
cp README.md LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md SECURITY.md "$OUT/"
cp vendor/pdfium/win-x64/LICENSE "$OUT/licenses/pdfium/LICENSE"
cp vendor/pdfium/win-x64/licenses/* "$OUT/licenses/pdfium/"
(cd dist && python3 -c "import shutil; shutil.make_archive('$NAME', 'zip', '.', '$NAME')")
sha256sum "dist/$NAME.zip" | tee "dist/$NAME.zip.sha256"
