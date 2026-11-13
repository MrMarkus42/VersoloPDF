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
RELEASE="chromium/8152"
# (also runs with the old bash 3 of macOS: no associative arrays)
sha256_of_platform() {
  case "$1" in
    linux-x64) echo 384ab5c51882408a143a21f9c14dcd2caf1f75280ae41c554d0489b46c3dba9f ;;
    linux-arm64) echo c6ed1a91234aede0cfafdc09c401b83bd44ac69deafc4599fda75d511c36403e ;;
    win-x64) echo cee2b0e9990bc67c0721774932961b9dbf2766046c10eec925acd4cfba6fdfe3 ;;
    win-arm64) echo 8614b10c6a780acc3ae9fdd425d6b61f9d22ae84301b19e1ac82f573930c217c ;;
    mac-arm64) echo 30b0c3f55448840f0947b54bedbe35185bbaccbadd35b8cd960270d6fa6caf80 ;;
    mac-x64) echo acb975e637ffa1f41430205c62baacac70e34b60811e22fc0e0274dfcd601fcc ;;
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
