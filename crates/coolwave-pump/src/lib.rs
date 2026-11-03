//! Pump screen support for Coolwave: drives the 720x720 LCD on the ROG Strix LC IV
//! (USB 0b05:1de7). Merged from strix-lc-screen 0.4.0; its own tray, single-instance
//! lock and autostart are replaced by Coolwave's.

mod engine;
mod firmware;
mod frame;
mod gpu;
mod hid;
mod hwinfo;
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
mod ui;

pub use ui::{page, PumpPage};
