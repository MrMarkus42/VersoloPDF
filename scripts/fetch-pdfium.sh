#!/usr/bin/env bash
# Downloads prebuilt PDFium libraries (https://github.com/bblanchon/pdfium-binaries)
# into vendor/pdfium/<platform>/ and verifies their SHA-256 checksums.
#
# Usage: scripts/fetch-pdfium.sh [platform...]
#        platforms: linux-x64 linux-arm64 win-x64 win-arm64 mac-x64 mac-arm64
#        (default: linux-x64 win-x64 mac-x64 mac-arm64)
set -euo pipefail

RELEASE="chromium/7881"
declare -A SHA256=(
  [linux-x64]=1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d
  [linux-arm64]=ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674
  [win-x64]=73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08
  [win-arm64]=d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84
  [mac-arm64]=52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40
  [mac-x64]=6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b
)

cd "$(dirname "$0")/.."
platforms=("$@")
if [ ${#platforms[@]} -eq 0 ]; then
  platforms=(linux-x64 win-x64 mac-x64 mac-arm64)
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for p in "${platforms[@]}"; do
  expected=${SHA256[$p]:-}
  if [ -z "$expected" ]; then
    echo "unknown platform: $p" >&2
    exit 1
  fi
  archive="$tmp/pdfium-$p.tgz"
  echo "downloading PDFium $RELEASE for $p"
  curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 \
    -o "$archive" "https://github.com/bblanchon/pdfium-binaries/releases/download/$RELEASE/pdfium-$p.tgz"
  actual=$(sha256sum "$archive" | cut -d' ' -f1)
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for $p: expected $expected, got $actual" >&2
    exit 1
  fi
  rm -rf "vendor/pdfium/$p"
  mkdir -p "vendor/pdfium/$p"
  tar -xzf "$archive" -C "vendor/pdfium/$p" --no-same-owner
  echo "ok: vendor/pdfium/$p"
done
