//! HID device lookup and report I/O.
//!
//! Both backends share one convention, the same as hidapi: byte 0 of every
//! buffer written is the report ID (0 for devices without report IDs), and a
//! report read from a device without report IDs carries no ID byte.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
