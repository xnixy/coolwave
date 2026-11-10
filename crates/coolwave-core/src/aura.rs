//! ASUS Aura USB motherboard controller (0b05:1bed, firmware "AULA3-AR36-*").
//!
//! 65-byte HID reports with report ID 0xEC. Verified reads on this board:
//! 0x82 firmware string, 0xB0 config table. Header records start at reply[7],
//! 6 bytes each: [present, kind, count_hi, count_lo, ?, ?]; reply[6] = header count.
//! Kinds seen: 0 = ARGB strip, 5 = AIO Q-Connector, 0x83 = Polymo panel (count bytes
//! look like width x height). Direct mode (from the ASUS HAL and OpenRGB):
//! effect mode 0xFF via `[EC 35 effect_ch 00 00 FF]`, then
//! `[EC 40 ch|0x80 last|0x10 offset>255, offset, n<=20, rgb...]`.
//! ARGB header i -> direct channel i; mainboard LEDs -> direct channel 6 (AR36).

use crate::color::Rgb;
use crate::hid::{self, HidDevice};
use std::io;
use std::time::Duration;

pub const VID: u16 = 0x0B05;
pub const PID: u16 = 0x1BED;
pub const MAINBOARD_DIRECT_CHANNEL: u8 = 6;
const REPORT_LEN: usize = 65;
const LEDS_PER_PACKET: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Argb,
    AioQConnector,
    Panel { width: u8, height: u8 },
    Other(u8),
}

#[derive(Clone, Debug)]
pub struct Header {
    pub index: u8,
    pub kind: HeaderKind,
    /// LED count reported by the controller (ARGB headers default to 120).
    pub leds: u16,
}

impl Header {
    pub fn name(&self) -> String {
        match self.kind {
            HeaderKind::Argb => format!("ARGB header {}", self.index + 1),
            HeaderKind::AioQConnector => "AIO (Q-Connector)".into(),
            HeaderKind::Panel { .. } => "Polymo panel".into(),
            HeaderKind::Other(k) => format!("Header {} (type {k:#x})", self.index + 1),
        }
    }
}

pub struct Aura {
    dev: HidDevice,
    pub firmware: String,
    pub headers: Vec<Header>,
}

impl Aura {
    pub fn open() -> io::Result<Self> {
        let path = hid::find_usage_page(VID, PID, 0xFF72)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Aura LED controller not found"))?;
        let mut a = Self { dev: HidDevice::open(path)?, firmware: String::new(), headers: Vec::new() };
        let fw = a.query(0x82)?;
        a.firmware = String::from_utf8_lossy(&fw[2..18]).trim_end_matches('\0').to_string();
        let cfg = a.query(0xB0)?;
        a.headers = parse_headers(&cfg);
        Ok(a)
    }

    fn send(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut p = [0u8; REPORT_LEN];
        p[..payload.len()].copy_from_slice(payload);
        self.dev.write_report(&p)
    }

    fn query(&mut self, cmd: u8) -> io::Result<[u8; REPORT_LEN]> {
        self.send(&[0xEC, cmd])?;
        let mut buf = [0u8; REPORT_LEN];
        for _ in 0..20 {
            match self.dev.read_report(&mut buf, Duration::from_millis(300))? {
                Some(n) if n > 1 && buf[1] == cmd & 0x7F => return Ok(buf),
                Some(_) => continue,
                None => break,
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, format!("no reply to Aura command {cmd:#x}")))
    }

    /// Switch every effect channel to direct (software) control. Not committed.
    pub fn enter_direct(&mut self) -> io::Result<()> {
        for ch in 0..7u8 {
            self.send(&[0xEC, 0x35, ch, 0x00, 0x00, 0xFF])?;
        }
        Ok(())
    }

    /// Stream colors to a direct channel.
    pub fn set_direct(&mut self, channel: u8, colors: &[Rgb]) -> io::Result<()> {
        let n = colors.len();
        let mut off = 0usize;
        while off < n {
            let cnt = LEDS_PER_PACKET.min(n - off);
            let mut flag = channel;
            if off + cnt >= n {
                flag |= 0x80;
            }
            if off > 255 {
                flag |= 0x10;
            }
            let mut p = Vec::with_capacity(5 + cnt * 3);
            p.extend_from_slice(&[0xEC, 0x40, flag, (off & 0xFF) as u8, cnt as u8]);
            for c in &colors[off..off + cnt] {
                p.extend_from_slice(&[c.r, c.g, c.b]);
            }
            self.send(&p)?;
            off += cnt;
        }
        Ok(())
    }
}

pub fn parse_headers(reply: &[u8]) -> Vec<Header> {
    let count = reply.get(6).copied().unwrap_or(0).min(8) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let Some(r) = reply.get(7 + 6 * i..7 + 6 * i + 6) else { break };
        if r[0] == 0 {
            continue;
        }
        let (kind, leds) = match r[1] {
            0 => (HeaderKind::Argb, u16::from_be_bytes([r[2], r[3]])),
            5 => (HeaderKind::AioQConnector, u16::from_be_bytes([r[2], r[3]])),
            0x83 => (HeaderKind::Panel { width: r[3], height: r[2] }, r[2] as u16 * r[3] as u16),
            k => (HeaderKind::Other(k), u16::from_be_bytes([r[2], r[3]])),
        };
        let leds = if leds == 0 { 120 } else { leds.min(500) };
        out.push(Header { index: i as u8, kind, leds });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_this_boards_config() {
        let raw = "ec 30 00 38 10 1f 05 01 00 00 78 3c 00 01 00 00 78 3c 00 01 00 00 78 3c 00 01 83 0a 0f 3c 00 01 05 00 30 3c 00";
        let b: Vec<u8> = raw.split(' ').map(|h| u8::from_str_radix(h, 16).unwrap()).collect();
        let h = parse_headers(&b);
        assert_eq!(h.len(), 5);
        assert_eq!(h[0].kind, HeaderKind::Argb);
        assert_eq!(h[0].leds, 120);
        assert_eq!(h[3].kind, HeaderKind::Panel { width: 15, height: 10 });
        assert_eq!(h[3].leds, 150);
        assert_eq!(h[4].kind, HeaderKind::AioQConnector);
        assert_eq!(h[4].leds, 48);
    }
}
