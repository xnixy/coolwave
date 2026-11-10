//! HID device lookup and I/O through the Windows HID class driver.
//!
//! No extra driver is involved: these are the same calls hidapi makes.

use std::ffi::{OsString, c_void};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_ListW, CR_SUCCESS,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetHidGuid, HidD_GetInputReport, HidD_GetPreparsedData,
    HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

/// A write that has not finished after this long means the device is gone or stuck.
const WRITE_TIMEOUT_MS: u32 = 1000;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Device paths of every HID interface that is plugged in right now.
fn present_interfaces() -> Vec<PathBuf> {
    let mut guid = unsafe { std::mem::zeroed() };
    // SAFETY: HidD_GetHidGuid only fills the GUID it is given.
    unsafe { HidD_GetHidGuid(&mut guid) };
    // The list can grow between asking for its size and reading it, so try a few times.
    for _ in 0..3 {
        let mut len = 0u32;
        // SAFETY: both calls get valid pointers, and `list` is `len` characters long.
        unsafe {
            if CM_Get_Device_Interface_List_SizeW(&mut len, &guid, std::ptr::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
                != CR_SUCCESS
            {
                return Vec::new();
            }
            let mut list = vec![0u16; len as usize];
            if CM_Get_Device_Interface_ListW(&guid, std::ptr::null(), list.as_mut_ptr(), len, CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
                == CR_SUCCESS
            {
                // One string after another, each ending in 0, with an empty string last.
                return list.split(|c| *c == 0).filter(|s| !s.is_empty()).map(|s| PathBuf::from(OsString::from_wide(s))).collect();
            }
        }
    }
    Vec::new()
}

/// Report sizes and usage page of one interface. All lengths include the report ID byte.
struct Caps {
    usage_page: u16,
    input_len: usize,
    output_len: usize,
}

fn caps_of(handle: HANDLE) -> io::Result<Caps> {
    let mut parsed = 0;
    // SAFETY: `handle` is an open HID device. The parsed data is freed below.
    unsafe {
        if HidD_GetPreparsedData(handle, &mut parsed) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let status = HidP_GetCaps(parsed, &mut caps);
        HidD_FreePreparsedData(parsed);
        if status != HIDP_STATUS_SUCCESS {
            return Err(io::Error::other("could not read HID capabilities"));
        }
        Ok(Caps {
            usage_page: caps.UsagePage,
            input_len: caps.InputReportByteLength as usize,
            output_len: caps.OutputReportByteLength as usize,
        })
    }
}

fn open_handle(path: &Path, access: u32) -> io::Result<HANDLE> {
    let name = wide(path);
    // SAFETY: `name` is a valid, 0-terminated wide string.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE { Err(io::Error::last_os_error()) } else { Ok(handle) }
}

/// HID interfaces of a USB vendor/product, in a stable order.
pub fn find(vid: u16, pid: u16) -> Vec<PathBuf> {
    let needle = format!("vid_{vid:04x}&pid_{pid:04x}");
    let mut out: Vec<PathBuf> =
        present_interfaces().into_iter().filter(|p| p.to_string_lossy().to_ascii_lowercase().contains(&needle)).collect();
    out.sort();
    out
}

/// First interface of the device whose top-level usage page is `page`.
pub fn find_usage_page(vid: u16, pid: u16, page: u16) -> Option<PathBuf> {
    find(vid, pid).into_iter().find(|path| {
        // Access 0 only asks about the device, so this works even while another program has it open.
        let Ok(handle) = open_handle(path, 0) else { return false };
        let caps = caps_of(handle);
        // SAFETY: `handle` was opened just above and is not used again.
        unsafe { CloseHandle(handle) };
        caps.is_ok_and(|c| c.usage_page == page)
    })
}

pub struct HidDevice {
    handle: HANDLE,
    pub path: PathBuf,
    caps: Caps,
    /// Windows only takes writes of exactly the device's output report length.
    write_buf: Vec<u8>,
    write_event: HANDLE,
    /// A read stays queued across timeouts, so its buffer and state must outlive the call.
    read_buf: Vec<u8>,
    read_state: Box<OVERLAPPED>,
    read_event: HANDLE,
    read_pending: bool,
}

// SAFETY: the handles are owned by this struct alone and may be used from any one thread.
unsafe impl Send for HidDevice {}

fn new_event() -> io::Result<HANDLE> {
    // SAFETY: an unnamed manual-reset event with default security.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() { Err(io::Error::last_os_error()) } else { Ok(event) }
}

impl HidDevice {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let handle = open_handle(&path, GENERIC_READ | GENERIC_WRITE)?;
        let made = caps_of(handle).and_then(|caps| Ok((caps, new_event()?, new_event()?)));
        let (caps, write_event, read_event) = match made {
            Ok(parts) => parts,
            Err(e) => {
                // SAFETY: `handle` was opened above and is dropped here.
                unsafe { CloseHandle(handle) };
                return Err(e);
            }
        };
        Ok(Self {
            handle,
            path,
            write_buf: vec![0; caps.output_len],
            read_buf: vec![0; caps.input_len],
            caps,
            write_event,
            // SAFETY: OVERLAPPED is plain data and all-zero is its initial state.
            read_state: Box::new(unsafe { std::mem::zeroed() }),
            read_event,
            read_pending: false,
        })
    }

    /// Write one report; `data[0]` must be the report ID.
    pub fn write_report(&mut self, data: &[u8]) -> io::Result<()> {
        if data.len() > self.write_buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "report is longer than the device accepts"));
        }
        self.write_buf.fill(0);
        self.write_buf[..data.len()].copy_from_slice(data);
        // SAFETY: the buffer and `state` live until the write has finished or was cancelled.
        unsafe {
            let mut state: OVERLAPPED = std::mem::zeroed();
            state.hEvent = self.write_event;
            ResetEvent(self.write_event);
            let started = WriteFile(self.handle, self.write_buf.as_ptr(), self.write_buf.len() as u32, std::ptr::null_mut(), &mut state);
            if started == 0 {
                if GetLastError() != ERROR_IO_PENDING {
                    return Err(io::Error::last_os_error());
                }
                if WaitForSingleObject(self.write_event, WRITE_TIMEOUT_MS) != WAIT_OBJECT_0 {
                    CancelIoEx(self.handle, &state);
                    // Wait for the cancel to land before `state` goes away.
                    let mut ignored = 0;
                    GetOverlappedResult(self.handle, &state, &mut ignored, 1);
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "HID write timed out"));
                }
            }
            let mut written = 0;
            if GetOverlappedResult(self.handle, &state, &mut written, 0) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Read one input report, waiting at most `timeout`.
    pub fn read_report(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<Option<usize>> {
        // SAFETY: `read_buf` and `read_state` are owned by self and stay put while a read is queued.
        let received = unsafe {
            if !self.read_pending {
                *self.read_state = std::mem::zeroed();
                self.read_state.hEvent = self.read_event;
                ResetEvent(self.read_event);
                let started =
                    ReadFile(self.handle, self.read_buf.as_mut_ptr(), self.read_buf.len() as u32, std::ptr::null_mut(), &mut *self.read_state);
                if started == 0 && GetLastError() != ERROR_IO_PENDING {
                    return Err(io::Error::last_os_error());
                }
                self.read_pending = true;
            }
            match WaitForSingleObject(self.read_event, timeout.as_millis().min(u32::MAX as u128 - 1) as u32) {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(None),
                _ => return Err(io::Error::last_os_error()),
            }
            self.read_pending = false;
            let mut received = 0;
            if GetOverlappedResult(self.handle, &*self.read_state, &mut received, 0) == 0 {
                return Err(io::Error::last_os_error());
            }
            received as usize
        };
        // Windows always puts the report ID first, as 0 when the device has none.
        let report = &self.read_buf[..received];
        let report = if report.first() == Some(&0) { &report[1..] } else { report };
        let n = report.len().min(buf.len());
        buf[..n].copy_from_slice(&report[..n]);
        Ok(Some(n))
    }

    /// Fetch an input report over the control endpoint. `buf[0]` = report ID.
    pub fn get_input_report(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // The driver wants a buffer of the device's full input report length.
        let mut report = vec![0u8; self.caps.input_len.max(buf.len())];
        report[0] = buf[0];
        // SAFETY: `report` is valid for the length passed.
        if unsafe { HidD_GetInputReport(self.handle, report.as_mut_ptr().cast::<c_void>(), report.len() as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let n = buf.len().min(report.len());
        buf[..n].copy_from_slice(&report[..n]);
        Ok(n)
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        // SAFETY: the handles are still open. A queued read is cancelled and
        // waited for, so the driver no longer touches `read_buf` afterwards.
        unsafe {
            if self.read_pending {
                CancelIoEx(self.handle, &*self.read_state);
                let mut ignored = 0;
                GetOverlappedResult(self.handle, &*self.read_state, &mut ignored, 1);
            }
            CloseHandle(self.handle);
            CloseHandle(self.read_event);
            CloseHandle(self.write_event);
        }
    }
}
