//! Coolwave hardware layer: lighting and cooling drivers.

pub mod aura;
pub mod color;
pub mod effects;
pub mod ene;
pub mod engine;
pub mod eurux;
pub mod hid;
pub mod hwinfo;
pub mod hwmon;
#[cfg(windows)]
mod session;
pub mod settings;

pub use color::Rgb;
