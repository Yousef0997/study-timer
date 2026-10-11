#!/bin/bash
# Build Study Timer and install it for the current user (no root needed).
set -euo pipefail
cd "$(dirname "$0")"

CARGO="${CARGO:-$(command -v cargo || echo "$HOME/.cargo/bin/cargo")}"
"$CARGO" build --release

BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
mkdir -p "$BIN" "$APPS" "$ICONS"

install -m 755 target/release/study-timer "$BIN/study-timer"
install -m 644 assets/study-timer.svg "$ICONS/study-timer.svg"
sed "s|^Exec=study-timer|Exec=$BIN/study-timer|" assets/study-timer.desktop > "$APPS/study-timer.desktop"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -q "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" 2>/dev/null || true

echo "Installed Study Timer to $BIN/study-timer"
echo "Open it from the Activities menu (search \"Study Timer\") or run: study-timer"
