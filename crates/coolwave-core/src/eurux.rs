//! ROG EURUX Controller (0b05:1d98) lighting.
//!
//! Protocol (firmware 90.36.32.00.0E, verified on hardware for static mode):
//! HID output report 0x90 with a 99-byte payload; payload[0] is the command.
//! - 0x02 colors: `[02, channel, 10 x (r, b, g)]`
//! - 0x01 mode:   `[01, mask_hi, mask_lo, mode, speed, direction, brightness_shift]`
//!
//! Each port has two logical channels: even = fan middle (one color slot per
//! fan, slot `f`), odd = fan sides (right = slot `2f`, left = slot `2f + 1`).
//! Color slots wrap modulo 5 (middle) and 10 (sides), so up to 5 fans per port.
//! Fans (verified reading RPM on hardware):
//! - 0x05 query: `[05, sel]` fills input report 0x90 (read with GET_REPORT):
//!   sel 0 -> `[F0, fw version 5 bytes]`, sel 1 -> `[F1, rpm_be x4]` per port.
//! - 0x04 fixed duty: `[04, d1, d2, d3, d4]` percent per port, 0 = leave unchanged.
//! - 0x07 RPM target loop (not used yet).
//!
//! Modes 0x60-0x62 are configuration commands and are not used.

use crate::color::Rgb;
use crate::hidraw::{self, HidDevice};
use std::io;

pub const VID: u16 = 0x0B05;
pub const PID: u16 = 0x1D98;
pub const PORTS: usize = 4;
pub const MAX_FANS_PER_PORT: usize = 5;

const REPORT_ID: u8 = 0x90;
const PAYLOAD_LEN: usize = 99;
const CMD_MODE: u8 = 0x01;
const CMD_COLORS: u8 = 0x02;
const MODE_STATIC: u8 = 0x03;
const SLOTS: usize = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FanColors {
    pub middle: Rgb,
    pub left: Rgb,
    pub right: Rgb,
}

impl FanColors {
    pub const fn uniform(c: Rgb) -> Self {
        Self { middle: c, left: c, right: c }
    }
}

/// Static colors for every fan on one port.
#[derive(Clone, Debug, Default)]
pub struct PortStatic {
    pub fans: Vec<FanColors>,
    /// 0 = full brightness, each step halves it (0..=7).
    pub brightness_shift: u8,
}

fn packet(payload: &[u8]) -> [u8; PAYLOAD_LEN + 1] {
    assert!(payload.len() <= PAYLOAD_LEN);
    let mut p = [0u8; PAYLOAD_LEN + 1];
    p[0] = REPORT_ID;
    p[1..=payload.len()].copy_from_slice(payload);
    p
}

fn push_color(buf: &mut Vec<u8>, c: Rgb) {
    // Wire order verified on hardware: R, B, G.
    buf.extend_from_slice(&[c.r, c.b, c.g]);
}

/// Only the two color-slot reports for a port (no mode change) — used for animation
/// once the port is in static mode, since static mode re-renders the slots live.
pub fn color_packets(port: usize, fans: &[FanColors]) -> [[u8; PAYLOAD_LEN + 1]; 2] {
    let p = static_port_packets(port, &PortStatic { fans: fans.to_vec(), brightness_shift: 0 });
    [p[0], p[1]]
}

/// Build the reports that set static colors on a port (0-based).
pub fn static_port_packets(port: usize, cfg: &PortStatic) -> Vec<[u8; PAYLOAD_LEN + 1]> {
    assert!(port < PORTS);
    let fans = &cfg.fans[..cfg.fans.len().min(MAX_FANS_PER_PORT)];
    let mid_ch = (port * 2) as u8;
    let side_ch = mid_ch + 1;

    let mut middle = [Rgb::default(); SLOTS];
    let mut sides = [Rgb::default(); SLOTS];
    for (f, fan) in fans.iter().enumerate() {
        middle[f] = fan.middle;
        middle[f + 5] = fan.middle; // middle slots wrap mod 5
        sides[2 * f] = fan.right;
        sides[2 * f + 1] = fan.left;
    }

    let mut out = Vec::with_capacity(3);
    for (ch, slots) in [(mid_ch, &middle), (side_ch, &sides)] {
        let mut pl = vec![CMD_COLORS, ch];
        for c in slots.iter() {
            push_color(&mut pl, *c);
        }
        out.push(packet(&pl));
    }
    let mask: u16 = (1 << mid_ch) | (1 << side_ch);
    out.push(packet(&[
        CMD_MODE,
        (mask >> 8) as u8,
        mask as u8,
        MODE_STATIC,
        0,
        0,
        cfg.brightness_shift.min(7),
    ]));
    out
}

pub const MIN_DUTY: u8 = 20;

pub struct Eurux {
    dev: HidDevice,
}

impl Eurux {
    pub fn open() -> io::Result<Self> {
        let path = hidraw::find(VID, PID)
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ROG EURUX controller not found"))?;
        Ok(Self { dev: HidDevice::open(path)? })
    }

    pub fn set_port_static(&mut self, port: usize, cfg: &PortStatic) -> io::Result<()> {
        for p in static_port_packets(port, cfg) {
            self.dev.write_report(&p)?;
        }
        Ok(())
    }

    /// Fan speed per port (RPM), from the controller's tachometers.
    pub fn read_rpm(&mut self) -> io::Result<[u16; PORTS]> {
        self.dev.write_report(&packet(&[0x05, 0x01]))?;
        let mut buf = [0u8; 65];
        buf[0] = REPORT_ID;
        self.dev.get_input_report(&mut buf)?;
        if buf[1] != 0xF1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected EURUX fan report"));
        }
        let mut rpm = [0u16; PORTS];
        for (i, r) in rpm.iter_mut().enumerate() {
            *r = u16::from_be_bytes([buf[2 + 2 * i], buf[3 + 2 * i]]);
        }
        Ok(rpm)
    }

    /// Set fixed duty (percent) per port; `None` leaves a port unchanged.
    /// Values are clamped to MIN_DUTY..=100 so fans never stall.
    pub fn set_duty(&mut self, duty: [Option<u8>; PORTS]) -> io::Result<()> {
        let mut pl = vec![0x04];
        pl.extend(duty.iter().map(|d| d.map(|v| v.clamp(MIN_DUTY, 100)).unwrap_or(0)));
        self.dev.write_report(&packet(&pl))
    }

    /// Animation frame: update color slots only.
    pub fn set_port_colors(&mut self, port: usize, fans: &[FanColors]) -> io::Result<()> {
        for p in color_packets(port, fans) {
            self.dev.write_report(&p)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_verified_test_packets() {
        // Port 1, three fans: middle red, right green, left blue (seen on hardware).
        let red = Rgb::new(255, 0, 0);
        let green = Rgb::new(0, 255, 0);
        let blue = Rgb::new(0, 0, 255);
        let cfg = PortStatic {
            fans: vec![FanColors { middle: red, left: blue, right: green }; 3],
            brightness_shift: 0,
        };
        let p = static_port_packets(0, &cfg);
        assert_eq!(p.len(), 3);
        assert_eq!(&p[0][..5], &[0x90, 0x02, 0x00, 0xff, 0x00]);
        // sides: slot0 = right (green -> r,b,g = 00 00 ff), slot1 = left (blue -> 00 ff 00)
        assert_eq!(&p[1][..9], &[0x90, 0x02, 0x01, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00]);
        assert_eq!(&p[2][..8], &[0x90, 0x01, 0x00, 0x03, 0x03, 0x00, 0x00, 0x00]);
    }
}
