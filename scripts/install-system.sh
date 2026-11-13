#!/bin/sh
# Root part of the Coolwave install: device access, fan helper, polkit policy, nct6775.
# Usage (as root): sh scripts/install-system.sh <path-to-coolwave-fanctl>
set -eu
cd "$(dirname "$0")/.."
FANCTL=${1:-target/release/coolwave-fanctl}

install -Dm755 "$FANCTL" /usr/local/libexec/coolwave-fanctl
install -Dm644 data/io.github.xnixy.coolwave.fanctl.policy /usr/share/polkit-1/actions/io.github.xnixy.coolwave.fanctl.policy
install -Dm644 data/60-coolwave.rules /etc/udev/rules.d/60-coolwave.rules
echo nct6775 > /etc/modules-load.d/nct6775.conf
echo i2c-dev > /etc/modules-load.d/coolwave-i2c.conf
modprobe nct6775 2>/dev/null || true
modprobe i2c-dev 2>/dev/null || true
udevadm control --reload
udevadm trigger --attr-match=idVendor=0b05 || true
# The hidraw nodes carry no idVendor of their own, so the line above skips them.
udevadm trigger --subsystem-match=hidraw || true
udevadm trigger --subsystem-match=i2c-dev || true
echo "Coolwave system files installed."
