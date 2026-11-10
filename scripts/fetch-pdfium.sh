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
RELEASE="chromium/8125"
# (also runs with the old bash 3 of macOS: no associative arrays)
sha256_of_platform() {
  case "$1" in
    linux-x64) echo e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659 ;;
    linux-arm64) echo 8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa ;;
    win-x64) echo 9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b ;;
    win-arm64) echo d394ba61991fb53f4d87f70988035f337d9e73f2bd2c3c1b966df8c43fcabb05 ;;
    mac-arm64) echo 4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d ;;
    mac-x64) echo 8a5193535b80c7215d8932f6647ae5097465b6dba2811602c21e120c2af112b0 ;;
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
