#!/usr/bin/env bash
# Downloads prebuilt PDFium libraries (https://github.com/bblanchon/pdfium-binaries)
# into vendor/pdfium/<platform>/ and verifies their SHA-256 checksums.
#
# Usage: scripts/fetch-pdfium.sh [platform...]
#        platforms: linux-x64 linux-arm64 win-x64 win-arm64 mac-x64 mac-arm64
#        (default: linux-x64 win-x64 mac-x64 mac-arm64)
set -euo pipefail

# Keep this current (security fixes in the PDF parser): the newest release
# that is at least 14 days old. Checksums from the GitHub release digests.
RELEASE="chromium/8111"
# (also runs with the old bash 3 of macOS: no associative arrays)
sha256_of_platform() {
  case "$1" in
    linux-x64) echo bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2 ;;
    linux-arm64) echo 042de247138f96341534cccf24ed21c2550a7245b936bb4a60d2340120000f05 ;;
    win-x64) echo 3452d11031050ec65e57f180215b57b9e6d465e3fbedafa8a77ad90885c7d23e ;;
    win-arm64) echo 99ec8b76d5ac30602a8488da745397393ff6cddc824ef36f398146f6b4f76d4e ;;
    mac-arm64) echo 46c674298c60123f1195c2a03c65ee08c4783c84874ce4f5b3a054abb3ecaf30 ;;
    mac-x64) echo e7fece4693f65a7e47f84fa38099068d573d4dcbd46b1a3c8d2c26581c308466 ;;
  esac
}
sha256_of_file() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

cd "$(dirname "$0")/.."
platforms=("$@")
if [ ${#platforms[@]} -eq 0 ]; then
  platforms=(linux-x64 win-x64 mac-x64 mac-arm64)
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for p in "${platforms[@]}"; do
  expected=$(sha256_of_platform "$p")
  if [ -z "$expected" ]; then
    echo "unknown platform: $p" >&2
    exit 1
  fi
  archive="$tmp/pdfium-$p.tgz"
  echo "downloading PDFium $RELEASE for $p"
  curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 \
    -o "$archive" "https://github.com/bblanchon/pdfium-binaries/releases/download/$RELEASE/pdfium-$p.tgz"
  actual=$(sha256_of_file "$archive")
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for $p: expected $expected, got $actual" >&2
    exit 1
  fi
  rm -rf "vendor/pdfium/$p"
  mkdir -p "vendor/pdfium/$p"
  tar -xzf "$archive" -C "vendor/pdfium/$p" --no-same-owner
  echo "ok: vendor/pdfium/$p"
done
