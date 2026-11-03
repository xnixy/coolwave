//! ASUS ENE RGB controller over I2C/SMBus (ROG GPUs). Found on this system:
//! ROG Astral RTX 5090 at NVIDIA i2c bus, address 0x67, firmware "AUMA0-E6K5-1113",
//! 30 LEDs (config[0x03]), v2 direct registers at 0x8100, colors in R,B,G order.
//! Register access: SMBus write-word 0x00 = byte-swapped register address, then
//! read-byte 0x81 / write-byte 0x01 / write-block 0x03.

use crate::color::Rgb;
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;

const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const SMBUS_READ: u8 = 1;
const SMBUS_WRITE: u8 = 0;
const SIZE_BYTE: u32 = 1;
const SIZE_BYTE_DATA: u32 = 2;
const SIZE_WORD_DATA: u32 = 3;
const SIZE_BLOCK_DATA: u32 = 5;

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

#[repr(C)]
union SmbusData {
    byte: u8,
    word: u16,
    block: [u8; 34],
}

#[repr(C)]
struct SmbusIoctl {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut SmbusData,
}

struct Bus {
    fd: OwnedFd,
}

impl Bus {
    fn open(bus: u32, addr: u16) -> io::Result<Self> {
        let f = fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(format!("/dev/i2c-{bus}"))?;
        let fd: OwnedFd = f.into();
        // SAFETY: valid fd, I2C_SLAVE takes the address by value.
        if unsafe { libc::ioctl(fd.as_raw_fd(), I2C_SLAVE, addr as libc::c_ulong) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    fn xfer(&self, rw: u8, cmd: u8, size: u32, data: &mut SmbusData) -> io::Result<()> {
        let mut args = SmbusIoctl { read_write: rw, command: cmd, size, data };
        // SAFETY: args points to valid, live memory for the duration of the call.
        if unsafe { libc::ioctl(self.fd.as_raw_fd(), I2C_SMBUS, &mut args) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_byte(&self) -> io::Result<u8> {
        let mut d = SmbusData { byte: 0 };
        self.xfer(SMBUS_READ, 0, SIZE_BYTE, &mut d)?;
        // SAFETY: the kernel filled the byte member.
        Ok(unsafe { d.byte })
    }

    fn read_byte_data(&self, cmd: u8) -> io::Result<u8> {
        let mut d = SmbusData { byte: 0 };
        self.xfer(SMBUS_READ, cmd, SIZE_BYTE_DATA, &mut d)?;
        Ok(unsafe { d.byte })
    }

    fn write_byte_data(&self, cmd: u8, v: u8) -> io::Result<()> {
        let mut d = SmbusData { byte: v };
        self.xfer(SMBUS_WRITE, cmd, SIZE_BYTE_DATA, &mut d)
    }

    fn write_word_data(&self, cmd: u8, v: u16) -> io::Result<()> {
        let mut d = SmbusData { word: v };
        self.xfer(SMBUS_WRITE, cmd, SIZE_WORD_DATA, &mut d)
    }

    fn write_block_data(&self, cmd: u8, data: &[u8]) -> io::Result<()> {
        let mut block = [0u8; 34];
        block[0] = data.len() as u8;
        block[1..=data.len()].copy_from_slice(data);
        let mut d = SmbusData { block };
        self.xfer(SMBUS_WRITE, cmd, SIZE_BLOCK_DATA, &mut d)
    }
}

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

    /// Find the ENE controller on the NVIDIA GPU I2C buses.
    pub fn find_gpu() -> io::Result<Self> {
        let mut buses: Vec<u32> = fs::read_dir("/sys/bus/i2c/devices")?
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().strip_prefix("i2c-")?.parse::<u32>().ok()?;
                let name = fs::read_to_string(e.path().join("name")).ok()?;
                name.starts_with("NVIDIA i2c adapter").then_some(n)
            })
            .collect();
        buses.sort();
        for b in buses {
            if let Ok(e) = Self::probe(b, GPU_ADDR) {
                return Ok(e);
            }
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

    /// Write direct colors, only touching LEDs that changed (I2C is slow).
    pub fn set_direct(&mut self, colors: &[Rgb]) -> io::Result<()> {
        let n = colors.len().min(self.led_count);
        if self.last.len() != self.led_count {
            self.last = vec![Rgb::new(1, 2, 3); self.led_count];
        }
        for (i, &c) in colors.iter().enumerate().take(n) {
            if self.last[i] != c {
                self.write_block(self.direct_reg + 3 * i as u16, &[c.r, c.b, c.g])?;
                self.last[i] = c;
            }
        }
        Ok(())
    }

    /// Colors used by the hardware animations.
    pub fn set_effect_colors(&mut self, colors: &[Rgb]) -> io::Result<()> {
        for (i, c) in colors.iter().enumerate().take(self.led_count) {
            self.write_block(self.effect_reg + 3 * i as u16, &[c.r, c.b, c.g])?;
        }
        self.write_reg(REG_APPLY, APPLY)
    }

    pub fn read_mode(&self) -> io::Result<u8> {
        self.read_reg(REG_MODE)
    }
}
