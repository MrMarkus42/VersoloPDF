#!/bin/bash
# Runs the mouse tour (tests/mouse.rs) in the background; log in /tmp/mouse.log,
# reports in target/mouse/.
# Usage: bash scripts/mouse.sh [Tab,Tab,...]   (no argument: all tabs)
# Needs VERSOLOPDF_PDFIUM_DIR when PDFium is not installed system-wide.
cd "$(dirname "$0")/.."
rm -rf target/mouse
export VERSOLOPDF_MOUSE_TABS="$1"
# No file manager, browser or mail program from the clicks (they would
# keep running after the test).
export VERSOLOPDF_NO_EXTERNAL=1
# Without a desktop session the system file dialog would wait for input.
nohup env DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus timeout 5400 \
    cargo test --release --test mouse -- --ignored --nocapture > /tmp/mouse.log 2>&1 &
echo started
