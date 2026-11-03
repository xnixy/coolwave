//! Minimal hidraw access for the pump screen (replaces the hidapi C library, which
//! needs libudev headers). Same semantics as hidapi's Linux hidraw backend:
//! `write` sends the buffer as-is (byte 0 = report id), `read_timeout` returns 0
//! when nothing arrived in time.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

pub struct HidDevice {
    file: File,
}

impl HidDevice {
    pub fn open(path: &PathBuf) -> io::Result<Self> {
        Ok(Self { file: OpenOptions::new().read(true).write(true).open(path)? })
    }

    pub fn write(&self, data: &[u8]) -> io::Result<usize> {
        (&self.file).write(data)
    }

    pub fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> io::Result<usize> {
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: pfd is a valid pollfd for the duration of the call.
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        if r == 0 {
            return Ok(0);
        }
        (&self.file).read(buf)
    }
}

/// The pump screen's vendor interface: usage page 0xFF00 on 0b05:1de7.
pub fn find(vendor: u16, product: u16, usage_page: u16) -> Option<PathBuf> {
    coolwave_core::hidraw::find_usage_page(vendor, product, usage_page)
}
