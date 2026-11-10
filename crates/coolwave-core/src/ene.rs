//! ASUS ENE RGB controller over I2C/SMBus (ROG GPUs). Found on this system:
//! ROG Astral RTX 5090 at NVIDIA i2c bus, address 0x67, firmware "AUMA0-E6K5-1113",
//! 30 LEDs (config[0x03]), v2 direct registers at 0x8100, colors in R,B,G order.
//! Register access: SMBus write-word 0x00 = byte-swapped register address, then
//! read-byte 0x81 / write-byte 0x01 / write-block 0x03.

use crate::color::Rgb;
use std::io;

// The SMBus transport is the only part that differs per system.
#[cfg_attr(target_os = "linux", path = "ene/i2c_dev.rs")]
#[cfg_attr(windows, path = "ene/nvapi.rs")]
mod bus;
use bus::Bus;


const REG_DEVICE_NAME: u16 = 0x1000;
const REG_CONFIG_TABLE: u16 = 0x1C00;
const REG_DIRECT_V1: u16 = 0x8000;
const REG_DIRECT_V2: u16 = 0x8100;
const REG_EFFECT_V1: u16 = 0x8010;
const REG_EFFECT_V2: u16 = 0x8160;
const REG_DIRECT_ENABLE: u16 = 0x8020;
const REG_MODE: u16 = 0x8021;
const REG_SPEED: u16 = 0x8022;
const REG_DIRECTION: u16 = 0x8023;
const REG_APPLY: u16 = 0x80A0;
const APPLY: u8 = 0x01;
pub const GPU_ADDR: u16 = 0x67;
/// Largest direct-color block per SMBus write (verified on AUMA0-E6K5-1113: 30 bytes = 10 LEDs).
const BLOCK_BYTES: usize = 30;

/// Built-in ENE animations (mode number, name).
pub const HARDWARE_MODES: &[(u8, &str)] = &[
    (1, "Static"),
    (2, "Breathing"),
    (3, "Flashing"),
    (4, "Spectrum cycle"),
    (5, "Rainbow"),
    (6, "Spectrum breathing"),
    (7, "Chase fade"),
    (8, "Spectrum chase fade"),
    (9, "Chase"),
    (10, "Spectrum chase"),
    (11, "Spectrum wave"),
    (12, "Chase rainbow pulse"),
    (13, "Random flicker"),
];

pub struct Ene {
    bus: Bus,
    pub bus_number: u32,
    pub firmware: String,
    pub led_count: usize,
    direct_reg: u16,
    effect_reg: u16,
    last: Vec<Rgb>,
}

impl Ene {
    fn select(&self, reg: u16) -> io::Result<()> {
        self.bus.write_word_data(0x00, reg.swap_bytes())
    }

    fn read_reg(&self, reg: u16) -> io::Result<u8> {
        self.select(reg)?;
        self.bus.read_byte_data(0x81)
    }

    fn write_reg(&self, reg: u16, v: u8) -> io::Result<()> {
        self.select(reg)?;
        self.bus.write_byte_data(0x01, v)
    }

    fn write_block(&self, reg: u16, data: &[u8]) -> io::Result<()> {
        self.select(reg)?;
        if self.bus.write_block_data(0x03, data).is_err() {
            for &b in data {
                self.bus.write_byte_data(0x01, b)?;
            }
        }
        Ok(())
    }

    /// Same check as OpenRGB: device answers and registers 0xA0..0xAF read back 0..15.
    fn probe(bus_number: u32, addr: u16) -> io::Result<Self> {
        let bus = Bus::open(bus_number, addr)?;
        bus.read_byte()?;
        for r in 0xA0..0xB0u8 {
            if bus.read_byte_data(r)? != r - 0xA0 {
                return Err(io::Error::new(io::ErrorKind::NotFound, "not an ENE controller"));
            }
        }
        let mut e = Self { bus, bus_number, firmware: String::new(), led_count: 0, direct_reg: REG_DIRECT_V1, effect_reg: REG_EFFECT_V1, last: Vec::new() };
        let name: Vec<u8> = (0..16).map(|i| e.read_reg(REG_DEVICE_NAME + i)).collect::<io::Result<_>>()?;
        e.firmware = String::from_utf8_lossy(&name).trim_end_matches('\0').to_string();
        let cfg: Vec<u8> = (0..4).map(|i| e.read_reg(REG_CONFIG_TABLE + i)).collect::<io::Result<_>>()?;
        let v2 = ["AUMA0-E6K5-1113", "AUMA0-E6K5-1114"].contains(&e.firmware.as_str());
        let count_at_3 = v2 || e.firmware.ends_with("0107") || e.firmware.ends_with("1107") || e.firmware.ends_with("1110") || e.firmware.ends_with("1111");
        e.led_count = cfg[if count_at_3 { 3 } else { 2 }] as usize;
        if v2 {
            e.direct_reg = REG_DIRECT_V2;
            e.effect_reg = REG_EFFECT_V2;
        }
        Ok(e)
    }

    /// Find the ENE controller on the NVIDIA GPU I2C buses (read-only probing).
    pub fn find_gpu() -> io::Result<Self> {
        let buses = bus::gpu_buses()?;
        let mut denied = None;
        for b in buses {
            match Self::probe(b, GPU_ADDR) {
                Ok(e) => return Ok(e),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => denied = denied.or(Some(b)),
                Err(_) => {}
            }
        }
        // A bus we may not open could be the right one, so "not found" would be a guess.
        if let Some(b) = denied {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, bus::no_access_hint(b)));
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "ASUS GPU lighting controller not found"))
    }

    pub fn enter_direct(&mut self) -> io::Result<()> {
        self.write_reg(REG_DIRECT_ENABLE, 1)?;
        self.write_reg(REG_APPLY, APPLY)?;
        self.last.clear();
        Ok(())
    }

    /// Hardware effect (runs on the card without the app). `mode` uses the ENE mode numbers.
    pub fn set_hardware_mode(&mut self, mode: u8, speed: u8, direction: u8) -> io::Result<()> {
        self.last.clear();
        self.write_reg(REG_DIRECT_ENABLE, 0)?;
        self.write_reg(REG_MODE, mode)?;
        self.write_reg(REG_SPEED, speed)?;
        self.write_reg(REG_DIRECTION, direction)?;
        self.write_reg(REG_APPLY, APPLY)
    }

    /// Write direct colors. I2C through the NVIDIA driver costs ~1.8 ms per
    /// transaction (and busy-waits a CPU core), so only the changed span is sent,
    /// in 30-byte SMBus blocks (10 LEDs): a full frame is 6 transactions, not 60.
    pub fn set_direct(&mut self, colors: &[Rgb]) -> io::Result<()> {
        let n = colors.len().min(self.led_count);
        if self.last.len() != self.led_count {
            self.last = vec![Rgb::new(1, 2, 3); self.led_count];
        }
        let changed: Vec<usize> = (0..n).filter(|&i| self.last[i] != colors[i]).collect();
        let (Some(&first), Some(&last)) = (changed.first(), changed.last()) else {
            return Ok(());
        };
        let bytes: Vec<u8> = colors[first..=last].iter().flat_map(|c| [c.r, c.b, c.g]).collect();
        for (k, chunk) in bytes.chunks(BLOCK_BYTES).enumerate() {
            self.write_block(self.direct_reg + (3 * first + k * BLOCK_BYTES) as u16, chunk)?;
        }
        self.last[first..=last].copy_from_slice(&colors[first..=last]);
        Ok(())
    }

    /// Colors used by the hardware animations (30-byte blocks, like direct mode).
    pub fn set_effect_colors(&mut self, colors: &[Rgb]) -> io::Result<()> {
        let bytes: Vec<u8> = colors.iter().take(self.led_count).flat_map(|c| [c.r, c.b, c.g]).collect();
        for (k, chunk) in bytes.chunks(BLOCK_BYTES).enumerate() {
            self.write_block(self.effect_reg + (k * BLOCK_BYTES) as u16, chunk)?;
        }
        self.write_reg(REG_APPLY, APPLY)
    }

    pub fn read_mode(&self) -> io::Result<u8> {
        self.read_reg(REG_MODE)
    }
}
