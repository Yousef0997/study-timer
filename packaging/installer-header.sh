#!/bin/bash
# Study Timer installer. Installs for the current user only; no root needed.
#   ./study-timer-installer.run              install (or update)
#   ./study-timer-installer.run --uninstall  remove the app (your schedule data is kept)
set -euo pipefail

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="$HOME/.local/bin"
APP="$BIN/study-timer"
DESKTOP="$DATA_HOME/applications/study-timer.desktop"
ICON="$DATA_HOME/icons/hicolor/scalable/apps/study-timer.svg"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/study-timer.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    pkill -x study-timer 2>/dev/null || true
    rm -f "$APP" "$DESKTOP" "$ICON" "$AUTOSTART"
    echo "Study Timer was removed."
    echo "Your schedule and history are still in $DATA_HOME/study-timer (delete that folder to remove them too)."
    exit 0
fi

if [ "$(uname -m)" != "x86_64" ]; then
    echo "Sorry, this installer is for 64-bit Intel/AMD PCs (x86_64)." >&2
    exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
PAYLOAD_LINE=$(awk '/^__PAYLOAD_BELOW__$/ { print NR + 1; exit }' "$0")
tail -n +"$PAYLOAD_LINE" "$0" | tar -xz -C "$TMP"

mkdir -p "$BIN" "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
pkill -x study-timer 2>/dev/null || true
install -m 755 "$TMP/study-timer" "$APP"
install -m 644 "$TMP/study-timer.svg" "$ICON"
sed "s|^Exec=study-timer|Exec=$APP|" "$TMP/study-timer.desktop" > "$DESKTOP"
update-desktop-database "$(dirname "$DESKTOP")" 2>/dev/null || true
gtk-update-icon-cache -q "$DATA_HOME/icons/hicolor" 2>/dev/null || true

echo "Study Timer is installed."
echo "Open it from your applications menu (search \"Study Timer\"), then turn on"
echo "\"Start Study Timer when I log in\" in Settings so alarms ring on time."
command -v notify-send >/dev/null || echo "Tip: install 'notify-send' (libnotify) to get desktop notifications."
command -v espeak-ng >/dev/null || command -v spd-say >/dev/null || echo "Tip: install 'espeak-ng' to hear spoken alerts."
exit 0
__PAYLOAD_BELOW__
