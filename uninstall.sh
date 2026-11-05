#!/bin/sh
# Remove Coolwave.
#
#   curl -fsSL https://raw.githubusercontent.com/xnixy/coolwave/main/uninstall.sh | sh
#
# Settings in ~/.config/coolwave are kept unless you pass --purge.
set -eu

APP_ID="io.github.xnixy.Coolwave"
BIN_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"
say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

if have gdbus; then
    gdbus call --session --dest "$APP_ID" --object-path /io/github/xnixy/Coolwave \
        --method org.gtk.Actions.Activate quit '[]' '{}' >/dev/null 2>&1 && sleep 1 || true
fi

say "Removing Coolwave from your user account"
rm -f "$BIN_DIR/coolwave" \
      "$DATA_DIR/applications/$APP_ID.desktop" \
      "$CONFIG_DIR/autostart/$APP_ID.desktop"
find "$DATA_DIR/icons/hicolor" -name "$APP_ID.*" -delete 2>/dev/null || true

if [ "${1:-}" = "--purge" ]; then
    say "Removing settings ($CONFIG_DIR/coolwave) and the build checkout"
    rm -rf "$CONFIG_DIR/coolwave" "$HOME/.local/src/coolwave"
fi

if have sudo; then SUDO="sudo"; else SUDO="pkexec"; fi
say "Removing system files (needs $SUDO)"
$SUDO sh -c '
rm -f /usr/local/libexec/coolwave-fanctl \
      /usr/share/polkit-1/actions/io.github.xnixy.coolwave.fanctl.policy \
      /etc/udev/rules.d/60-coolwave.rules \
      /etc/modules-load.d/coolwave-i2c.conf
udevadm control --reload || true'
say "Coolwave removed."
