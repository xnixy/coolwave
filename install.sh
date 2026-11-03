#!/bin/sh
# Coolwave installer.
#
#   curl -fsSL https://raw.githubusercontent.com/xnixy/coolwave/main/install.sh | sh
#
# or, from a checkout:  ./install.sh
#
# Builds Coolwave from source, installs it for the current user (~/.local), and
# installs the small root part (device access rules, fan helper, polkit policy).
# Environment: COOLWAVE_SRC (checkout dir), COOLWAVE_REF (git branch/tag),
#              COOLWAVE_AUTOSTART=1|0 (skip the question), COOLWAVE_NO_DEPS=1,
#              COOLWAVE_NO_SYSTEM=1 (skip the root part, e.g. when it is already installed).
set -eu

REPO_URL="https://github.com/xnixy/coolwave.git"
SRC="${COOLWAVE_SRC:-$HOME/.local/src/coolwave}"
REF="${COOLWAVE_REF:-main}"
BIN_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"
APP_ID="io.github.xnixy.Coolwave"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# Questions must read the terminal even when this script arrives through a pipe.
ask_yes() {
    prompt="$1"; default="$2"
    if [ ! -r /dev/tty ]; then [ "$default" = y ]; return; fi
    printf '%s ' "$prompt" > /dev/tty
    read -r answer < /dev/tty || answer=""
    case "${answer:-$default}" in [Yy]*) return 0 ;; *) return 1 ;; esac
}

[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not root (it asks for sudo when needed)"
[ "$(uname -s)" = Linux ] || die "Coolwave runs on Linux only"

if have sudo; then SUDO="sudo"; elif have pkexec; then SUDO="pkexec"; else die "need sudo or pkexec for the system part"; fi

# ------------------------------------------------------------------ build dependencies
install_deps() {
    if have dnf; then
        $SUDO dnf install -y gcc pkgconf-pkg-config gtk4-devel libadwaita-devel git ffmpeg-free || \
        $SUDO dnf install -y gcc pkgconf-pkg-config gtk4-devel libadwaita-devel git
    elif have apt-get; then
        $SUDO apt-get update
        $SUDO apt-get install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev git ffmpeg
    elif have pacman; then
        $SUDO pacman -S --needed --noconfirm base-devel pkgconf gtk4 libadwaita git ffmpeg
    else
        die "unknown package manager: install a C compiler, pkg-config, GTK 4.14+ and libadwaita 1.5+ dev packages, then rerun"
    fi
}

deps_ok() {
    have cc && have pkg-config && have git &&
        pkg-config --atleast-version=4.14 gtk4 2>/dev/null &&
        pkg-config --atleast-version=1.5 libadwaita-1 2>/dev/null
}

if [ "${COOLWAVE_NO_DEPS:-0}" != 1 ] && ! deps_ok; then
    say "Installing build dependencies (GTK 4, libadwaita, compiler)"
    install_deps
fi
deps_ok || die "GTK 4.14+ and libadwaita 1.5+ development files are required"
have ffmpeg || warn "ffmpeg not found: the pump screen needs it for photos and videos"

if ! have cargo; then
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! have cargo; then
    say "Installing Rust (rustup, current user only)"
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
fi

# ------------------------------------------------------------------ source
script_dir=""
case "$0" in */*) script_dir=$(cd "$(dirname "$0")" && pwd) ;; esac
if [ -n "$script_dir" ] && [ -f "$script_dir/Cargo.toml" ] && [ -d "$script_dir/crates/coolwave" ]; then
    SRC="$script_dir"
    say "Using the checkout in $SRC"
elif [ -d "$SRC/.git" ]; then
    say "Updating $SRC"
    git -C "$SRC" fetch --depth 1 origin "$REF"
    git -C "$SRC" checkout -q FETCH_HEAD
else
    say "Downloading Coolwave into $SRC"
    mkdir -p "$(dirname "$SRC")"
    git clone --depth 1 --branch "$REF" "$REPO_URL" "$SRC"
fi
cd "$SRC"

# ------------------------------------------------------------------ build
say "Building (release) — the first build takes a few minutes"
cargo build --release --locked -p coolwave -p coolwave-fanctl

# ------------------------------------------------------------------ install for this user
say "Installing Coolwave to $BIN_DIR"
# Ask a running instance to quit so the binary can be replaced cleanly.
if have gdbus && gdbus call --session --dest "$APP_ID" --object-path /io/github/xnixy/Coolwave \
        --method org.gtk.Actions.Activate quit '[]' '{}' >/dev/null 2>&1; then
    sleep 1
fi
install -Dm755 target/release/coolwave "$BIN_DIR/coolwave"

install -Dm644 "data/$APP_ID.svg" "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
for png in data/icons/coolwave-*.png; do
    size=${png##*-}; size=${size%.png}
    install -Dm644 "$png" "$DATA_DIR/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
mkdir -p "$DATA_DIR/applications"
sed "s|^Exec=.*|Exec=\"$BIN_DIR/coolwave\"|" "data/$APP_ID.desktop" > "$DATA_DIR/applications/$APP_ID.desktop"
have update-desktop-database && update-desktop-database -q "$DATA_DIR/applications" || true
have gtk-update-icon-cache && gtk-update-icon-cache -q -t "$DATA_DIR/icons/hicolor" 2>/dev/null || true

# ------------------------------------------------------------------ system part (root)
if [ "${COOLWAVE_NO_SYSTEM:-0}" != 1 ]; then
    say "Installing device access rules and the fan helper (needs $SUDO)"
    $SUDO sh "$SRC/scripts/install-system.sh" "$SRC/target/release/coolwave-fanctl"
fi

# ------------------------------------------------------------------ autostart
autostart="$CONFIG_DIR/autostart/$APP_ID.desktop"
want_autostart=${COOLWAVE_AUTOSTART:-}
if [ -z "$want_autostart" ]; then
    if ask_yes "Start Coolwave at login (hidden in the tray)? [Y/n]" y; then want_autostart=1; else want_autostart=0; fi
fi
if [ "$want_autostart" = 1 ]; then
    mkdir -p "$(dirname "$autostart")"
    printf '[Desktop Entry]\nType=Application\nName=Coolwave\nComment=Lighting and cooling control\nExec="%s" --background\nIcon=%s\nX-GNOME-Autostart-enabled=true\n' \
        "$BIN_DIR/coolwave" "$APP_ID" > "$autostart"
fi

case ":$PATH:" in *":$BIN_DIR:"*) ;; *) warn "$BIN_DIR is not on your PATH; start it with $BIN_DIR/coolwave" ;; esac
say "Done. Start Coolwave from your app launcher or run: coolwave"
say "If a device is not detected, unplug/replug it or reboot once so the new access rules apply."
