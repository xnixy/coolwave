//! SMBus transfers through the kernel's i2c-dev interface (`/dev/i2c-N`).

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

pub struct Bus {
    fd: OwnedFd,
}

impl Bus {
    pub fn open(bus: u32, addr: u16) -> io::Result<Self> {
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

    pub fn read_byte(&self) -> io::Result<u8> {
        let mut d = SmbusData { byte: 0 };
        self.xfer(SMBUS_READ, 0, SIZE_BYTE, &mut d)?;
        // SAFETY: the kernel filled the byte member.
        Ok(unsafe { d.byte })
    }

    pub fn read_byte_data(&self, cmd: u8) -> io::Result<u8> {
        let mut d = SmbusData { byte: 0 };
        self.xfer(SMBUS_READ, cmd, SIZE_BYTE_DATA, &mut d)?;
        Ok(unsafe { d.byte })
    }

    pub fn write_byte_data(&self, cmd: u8, v: u8) -> io::Result<()> {
        let mut d = SmbusData { byte: v };
        self.xfer(SMBUS_WRITE, cmd, SIZE_BYTE_DATA, &mut d)
    }

    pub fn write_word_data(&self, cmd: u8, v: u16) -> io::Result<()> {
        let mut d = SmbusData { word: v };
        self.xfer(SMBUS_WRITE, cmd, SIZE_WORD_DATA, &mut d)
    }

    pub fn write_block_data(&self, cmd: u8, data: &[u8]) -> io::Result<()> {
        let mut block = [0u8; 34];
        block[0] = data.len() as u8;
        block[1..=data.len()].copy_from_slice(data);
        let mut d = SmbusData { block };
        self.xfer(SMBUS_WRITE, cmd, SIZE_BLOCK_DATA, &mut d)
    }
}

/// Kernel bus numbers of the NVIDIA card's I2C adapters, lowest first.
pub fn gpu_buses() -> io::Result<Vec<u32>> {
    let mut buses: Vec<u32> = fs::read_dir("/sys/bus/i2c/devices")?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().strip_prefix("i2c-")?.parse::<u32>().ok()?;
            let name = fs::read_to_string(e.path().join("name")).ok()?;
            name.starts_with("NVIDIA i2c adapter").then_some(n)
        })
        .collect();
    buses.sort();
    Ok(buses)
}

pub fn no_access_hint(bus: u32) -> String {
    format!("no access to /dev/i2c-{bus} (run install.sh, then reboot)")
}
