# Coolwave

Lighting, cooling and pump-screen control for ASUS ROG hardware on Linux — one GTK 4 /
libadwaita app instead of Armoury Crate, OpenRGB and the ASUS LCD tool.

- **Lighting** — one effect engine drives every device in sync: ROG EURUX fans (three zones
  per fan), Aura motherboard ARGB headers, the AIO (Q-Connector), the Polymo Lighting II panel,
  and ASUS ROG graphics cards. 18 effects, including zone-aware ones (Aurora, Reactor, Spin,
  Scanner, Meteor shower, Ripple, Dual tone), plus the GPU's own built-in animations.
- **Cooling** — live RPM and temperatures, 5-point hardware fan curves for motherboard headers
  and the AIO pump (stored in the fan chip), and software curves for EURUX fan ports.
- **Pump screen** — photos, GIFs, videos and live stats widgets on the ROG Strix LC IV
  720×720 LCD, brightness, rotation, standby, and firmware updates.
- Tray icon, runs in the background, optional start at login. Settings in `~/.config/coolwave/`.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/xnixy/coolwave/main/install.sh | sh
```

The installer:

1. installs build dependencies if needed (GTK 4.14+, libadwaita 1.5+, a C compiler; Fedora,
   Debian/Ubuntu and Arch are supported) and Rust via rustup if `cargo` is missing,
2. downloads the source to `~/.local/src/coolwave` and builds it,
3. installs the app to `~/.local/bin/coolwave` with its launcher entry and icon,
4. installs the root part with `sudo`: udev rules so your user can talk to the devices, the
   `coolwave-fanctl` fan helper with a polkit policy (no password prompt for the logged-in
   user), and `nct6775` autoload for motherboard fan sensors,
5. asks whether Coolwave should start at login (hidden in the tray).

Run the same command again to update. From a checkout you can run `./install.sh` instead.
Replug devices or reboot once afterwards so the new access rules apply.

Uninstall (add `--purge` to also delete settings):

```sh
curl -fsSL https://raw.githubusercontent.com/xnixy/coolwave/main/uninstall.sh | sh
```

`ffmpeg` is needed for photos and videos on the pump screen.

## Supported hardware

Developed and tested on a ROG Crosshair X870E Dark Hero system.

| Device | How Coolwave talks to it |
|---|---|
| ROG EURUX fan controller `0b05:1d98` | HID report `0x90`: static/animated colors per fan zone, RPM readback, fan duty |
| Aura motherboard controller `0b05:1bed` (`AULA3-AR36-*`) | HID report `0xEC`, direct mode: ARGB headers, Polymo panel, AIO Q-Connector |
| ASUS ROG GPUs (ENE controller, e.g. ROG Astral RTX 5090) | SMBus on the GPU's I2C bus, address `0x67`: direct colors and built-in animations |
| ROG Strix LC IV pump screen `0b05:1de7` | HID: live JPEG frames, settings, firmware |
| Motherboard fans and pump | `nct6775` hwmon driver (NCT67xx Super I/O) |

Other ASUS boards with the same controllers may work; header layouts are read from the
controller. Use **Identify** (target icon) on the Lighting page to find which header is which,
then switch off empty headers and set real LED counts — fewer LEDs make Aura effects smoother.

## Safety

The fan helper only writes PWM attributes of the `nct*` hwmon chip and rejects unsafe curves
(the last point must be 100 %, speeds may not drop as temperature rises, max 100 °C). EURUX
software curves go to 100 % if their temperature sensor disappears, and never below 20 %.
The UI keeps the pump header at 40 % or more.

## Development

```sh
cargo build --release
cargo test --workspace
COOLWAVE_DEBUG=1 ./target/release/coolwave          # device status and frame rates
cargo run --release -p coolwave-core --example showcase   # cycle through effects headless
```

Crates: `coolwave-core` (device drivers, effects engine, settings), `coolwave` (the GTK app),
`coolwave-fanctl` (root fan helper), `coolwave-pump` (pump screen, merged from
[strix-lc-screen](https://github.com/xnixy/strix-lc-screen)). Protocol notes are in the doc
comments at the top of `crates/coolwave-core/src/{eurux,aura,ene}.rs`.

## Disclaimer

Not affiliated with or endorsed by ASUS. ROG, Aura and Armoury Crate are trademarks of ASUSTeK.
The device protocols were worked out for interoperability; use at your own risk.

## License

MIT — see [LICENSE](LICENSE). The Rajdhani fonts are under the SIL Open Font License
(`crates/coolwave-pump/assets/fonts/OFL.txt`).
