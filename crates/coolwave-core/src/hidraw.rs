//! Minimal hidraw device lookup and I/O (no libudev dependency).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// hidraw nodes for a USB vendor/product, with their raw report descriptors.
pub fn find_with_descriptors(vid: u16, pid: u16) -> Vec<(PathBuf, Vec<u8>)> {
    let needle = format!("HID_ID=0003:{:08X}:{:08X}", vid, pid);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return out;
    };
    for e in entries.flatten() {
        let Ok(uevent) = fs::read_to_string(e.path().join("device/uevent")) else { continue };
        if uevent.lines().any(|l| l == needle) {
            let desc = fs::read(e.path().join("device/report_descriptor")).unwrap_or_default();
            out.push((PathBuf::from("/dev").join(e.file_name()), desc));
        }
    }
    out.sort();
    out
}

/// Find `/dev/hidrawN` nodes whose HID_ID matches a USB vendor/product.
pub fn find(vid: u16, pid: u16) -> Vec<PathBuf> {
    find_with_descriptors(vid, pid).into_iter().map(|(p, _)| p).collect()
}

/// First interface whose descriptor starts with the given vendor usage page.
pub fn find_usage_page(vid: u16, pid: u16, page: u16) -> Option<PathBuf> {
    let prefix = [0x06, page as u8, (page >> 8) as u8];
    find_with_descriptors(vid, pid)
        .into_iter()
        .find(|(_, d)| d.starts_with(&prefix))
        .map(|(p, _)| p)
}

pub struct HidDevice {
    file: File,
    pub path: PathBuf,
}

impl HidDevice {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        Ok(Self { file, path })
    }

    /// Write one report; `data[0]` must be the report ID.
    pub fn write_report(&mut self, data: &[u8]) -> io::Result<()> {
        let n = self.file.write(data)?;
        if n != data.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short HID write"));
        }
        Ok(())
    }

    /// Read one input report, waiting at most `timeout`.
    pub fn read_report(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<usize>> {
        let deadline = Instant::now() + timeout;
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let left = deadline.saturating_duration_since(Instant::now()).as_millis() as i32;
        // SAFETY: pfd is a valid pollfd for the lifetime of the call.
        let r = unsafe { libc::poll(&mut pfd, 1, left) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        if r == 0 {
            return Ok(None);
        }
        Ok(Some(self.file.read(buf)?))
    }
}

impl HidDevice {
    /// HIDIOCGINPUT: fetch an input report over the control endpoint. `buf[0]` = report ID.
    pub fn get_input_report(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // _IOC(_IOC_READ|_IOC_WRITE, 'H', 0x0A, len)
        let req = (3u64 << 30) | ((buf.len() as u64) << 16) | ((b'H' as u64) << 8) | 0x0A;
        // SAFETY: buf is valid for buf.len() bytes, matching the size encoded in req.
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), req as libc::c_ulong, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(r as usize)
    }
}
