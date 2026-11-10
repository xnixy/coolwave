//! The pump screen's HID connection, on Coolwave's own HID layer. Same
//! semantics as hidapi: `write` sends the buffer as-is (byte 0 = report ID),
//! `read_timeout` returns 0 when nothing arrived in time.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub struct HidDevice {
    device: std::cell::RefCell<coolwave_core::hid::HidDevice>,
}

impl HidDevice {
    pub fn open(path: &PathBuf) -> io::Result<Self> {
        Ok(Self { device: coolwave_core::hid::HidDevice::open(path.clone())?.into() })
    }

    pub fn write(&self, data: &[u8]) -> io::Result<usize> {
        self.device.borrow_mut().write_report(data)?;
        Ok(data.len())
    }

    pub fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> io::Result<usize> {
        let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
        Ok(self.device.borrow_mut().read_report(buf, timeout)?.unwrap_or(0))
    }
}

/// The pump screen's vendor interface: usage page 0xFF00 on 0b05:1de7.
pub fn find(vendor: u16, product: u16, usage_page: u16) -> Option<PathBuf> {
    coolwave_core::hid::find_usage_page(vendor, product, usage_page)
}
