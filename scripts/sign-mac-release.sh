#!/usr/bin/env bash
# Signs the macOS package of a published release: GitHub Actions builds and
# uploads "VersoloPDF-<version>-macos.zip"; the signing key is not on GitHub,
# so this script (run where the key is) downloads the package, signs it and
# uploads the ".sig" file.  Usage: scripts/sign-mac-release.sh v1.4.0
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="${1:?usage: sign-mac-release.sh <tag>}"
KEY="${VERSOLOPDF_SIGNING_KEY:-$HOME/.config/versolopdf/update-signing.key}"
[ -f "$KEY" ] || { echo "no signing key at $KEY"; exit 1; }
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
gh release download "$TAG" --pattern '*-macos.zip' --dir "$WORK"
cargo run --quiet --release --example sign_release -- sign "$KEY" "$WORK"/*-macos.zip
gh release upload "$TAG" "$WORK"/*-macos.zip.sig --clobber
cargo run --quiet --release --example sign_release -- verify "$WORK"/*-macos.zip
