//! Fan, PWM and temperature readings.
//!
//! Linux reads the kernel's hwmon tree. Windows has no such thing without a
//! kernel driver, so there the temperatures come from the NVIDIA driver and,
//! when it is running, from HWiNFO.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[derive(Clone, Debug)]
pub struct CurvePoint {
    pub temp_c: f32,
    pub pwm: u8,
}

#[derive(Clone, Debug)]
pub struct PwmChannel {
    pub index: u32,
    pub rpm: Option<u32>,
    pub pwm: u8,
    /// hwmon pwmN_enable: 1 = manual, 5 = SmartFan IV (hardware curve), ...
    pub mode: u8,
    pub temp_source: Option<u32>,
    pub curve: Vec<CurvePoint>,
}

#[derive(Clone, Debug)]
pub struct TempSensor {
    pub chip: String,
    pub label: String,
    pub celsius: f32,
}

/// Current value of a temperature identified by chip name and label.
pub fn temperature(chip: &str, label: &str) -> Option<f32> {
    temperatures().into_iter().find(|t| t.chip == chip && t.label == label).map(|t| t.celsius)
}
