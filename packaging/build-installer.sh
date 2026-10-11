#!/bin/bash
# Build dist/study-timer-installer.run: one file with the compiled app only (no source code).
set -euo pipefail
cd "$(dirname "$0")/.."

CARGO="${CARGO:-$(command -v cargo || echo "$HOME/.cargo/bin/cargo")}"
"$CARGO" build --release

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp target/release/study-timer assets/study-timer.svg assets/study-timer.desktop "$STAGE/"

mkdir -p dist
OUT=dist/study-timer-installer.run
cat packaging/installer-header.sh > "$OUT"
tar -czf - -C "$STAGE" . >> "$OUT"
chmod +x "$OUT"
echo "Built $OUT ($(du -h "$OUT" | cut -f1))"
