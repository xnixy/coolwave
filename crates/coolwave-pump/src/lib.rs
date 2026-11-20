//! Pump screen support for Coolwave: drives the 720x720 LCD on the ROG Strix LC IV
//! (USB 0b05:1de7). Merged from strix-lc-screen 0.4.0; its own tray, single-instance
//! lock and autostart are replaced by Coolwave's.

// The widget editor's rules; only the WinUI page edits layouts this way.
#[cfg_attr(not(windows), allow(dead_code))]
mod edit;
mod engine;
mod firmware;
mod frame;
mod gpu;
mod hid;
use coolwave_core::hwinfo;
mod layout;
mod lock;
mod media;
mod msg;
mod panel;
mod sensors;
mod session;
mod settings;
mod stats;
mod tray;
#[cfg(not(windows))]
mod ui;
#[cfg(windows)]
mod winui;

#[cfg(not(windows))]
pub use ui::{page, PumpPage};
#[cfg(windows)]
pub use winui::{PumpCore, PumpHandle, PumpInput, PumpPage};
