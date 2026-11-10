//! Temperatures on Windows, without a kernel driver.
//!
//! The graphics card's temperature comes from NVML, which ships with the
//! NVIDIA driver. Everything else (CPU, coolant, motherboard) is only
//! available while HWiNFO runs with "Shared Memory Support" turned on.

use super::TempSensor;
use crate::hwinfo::{self, Kind};
use std::ffi::c_void;
use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

/// Chip names used in saved fan curves.
pub const CHIP_GPU: &str = "nvidia";
pub const CHIP_HWINFO: &str = "hwinfo";
pub const LABEL_GPU: &str = "GPU";

/// HWiNFO labels for the CPU temperature, best match first.
const CPU_LABELS: &[&str] = &["tctl", "cpu package", "cpu (", "cpu"];

type NvmlDevice = *mut c_void;
type InitFn = unsafe extern "C" fn() -> i32;
type ByIndexFn = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type TemperatureFn = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;

struct Nvml {
    device: NvmlDevice,
    temperature: TemperatureFn,
}

// SAFETY: NVML is thread-safe and the device handle is a plain identifier.
unsafe impl Send for Nvml {}
unsafe impl Sync for Nvml {}

fn load_nvml() -> Option<Nvml> {
    let name: Vec<u16> = "nvml.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: the library is only looked for in System32, where the driver installs it,
    // and each symbol is cast to its signature from NVML's public header.
    unsafe {
        let library = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if library.is_null() {
            return None;
        }
        let init: InitFn = std::mem::transmute(GetProcAddress(library, c"nvmlInit_v2".as_ptr().cast())?);
        let by_index: ByIndexFn = std::mem::transmute(GetProcAddress(library, c"nvmlDeviceGetHandleByIndex_v2".as_ptr().cast())?);
        let temperature: TemperatureFn = std::mem::transmute(GetProcAddress(library, c"nvmlDeviceGetTemperature".as_ptr().cast())?);
        if init() != NVML_SUCCESS {
            return None;
        }
        let mut device = std::ptr::null_mut();
        (by_index(0, &mut device) == NVML_SUCCESS).then_some(Nvml { device, temperature })
    }
}

fn gpu_temperature() -> Option<f32> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    let nvml = NVML.get_or_init(load_nvml).as_ref()?;
    let mut celsius = 0u32;
    // SAFETY: `device` came from NVML and `celsius` is a valid out pointer.
    let status = unsafe { (nvml.temperature)(nvml.device, NVML_TEMPERATURE_GPU, &mut celsius) };
    (status == NVML_SUCCESS).then_some(celsius as f32)
}

/// Every temperature that can be read right now.
pub fn temperatures() -> Vec<TempSensor> {
    let mut out = Vec::new();
    if let Some(celsius) = gpu_temperature() {
        out.push(TempSensor { chip: CHIP_GPU.into(), label: LABEL_GPU.into(), celsius });
    }
    for reading in hwinfo::read().unwrap_or_default() {
        let celsius = reading.value as f32;
        if reading.kind == Kind::Temperature && (1.0..=150.0).contains(&celsius) {
            out.push(TempSensor { chip: CHIP_HWINFO.into(), label: reading.label, celsius });
        }
    }
    out
}

/// CPU temperature, which only HWiNFO can supply.
pub fn cpu_temperature() -> Option<f32> {
    let readings = hwinfo::read()?;
    hwinfo::find(&readings, Kind::Temperature, CPU_LABELS).map(|celsius| celsius as f32)
}
