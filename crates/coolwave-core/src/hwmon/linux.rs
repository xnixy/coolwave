//! Read-only access to hwmon fan, PWM and temperature data.
//! Writes (PWM, curves) need root and will go through a privileged helper.

use super::{CurvePoint, PwmChannel, TempSensor};
use std::fs;
use std::path::{Path, PathBuf};

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_num<T: std::str::FromStr>(p: &Path) -> Option<T> {
    read_trim(p)?.parse().ok()
}

fn hwmon_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon")
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The motherboard Super I/O fan controller (nct67xx), if the driver is loaded.
pub fn find_fan_chip() -> Option<PathBuf> {
    hwmon_dirs()
        .into_iter()
        .find(|d| read_trim(&d.join("name")).is_some_and(|n| n.starts_with("nct")))
}

pub fn pwm_channels(chip: &Path) -> Vec<PwmChannel> {
    let mut out = Vec::new();
    for i in 1..=16u32 {
        let base = chip.join(format!("pwm{i}"));
        let Some(pwm) = read_num::<u8>(&base) else { continue };
        let curve = (1..=8)
            .map_while(|p| {
                let t = read_num::<i32>(&chip.join(format!("pwm{i}_auto_point{p}_temp")))?;
                let v = read_num::<u8>(&chip.join(format!("pwm{i}_auto_point{p}_pwm")))?;
                Some(CurvePoint { temp_c: t as f32 / 1000.0, pwm: v })
            })
            .collect();
        out.push(PwmChannel {
            index: i,
            rpm: read_num(&chip.join(format!("fan{i}_input"))),
            pwm,
            mode: read_num(&chip.join(format!("pwm{i}_enable"))).unwrap_or(0),
            temp_source: read_num(&chip.join(format!("pwm{i}_temp_sel"))),
            curve,
        });
    }
    out
}

/// Plausible temperatures from all hwmon chips (skips disconnected inputs).
pub fn temperatures() -> Vec<TempSensor> {
    let mut out = Vec::new();
    for d in hwmon_dirs() {
        let chip = read_trim(&d.join("name")).unwrap_or_default();
        for i in 1..=32u32 {
            let Some(milli) = read_num::<i32>(&d.join(format!("temp{i}_input"))) else { continue };
            let c = milli as f32 / 1000.0;
            if !(1.0..=150.0).contains(&c) {
                continue;
            }
            let label = read_trim(&d.join(format!("temp{i}_label"))).unwrap_or_else(|| format!("temp{i}"));
            out.push(TempSensor { chip: chip.clone(), label, celsius: c });
        }
    }
    out
}

/// Temperature inputs of one chip as (index, label, °C) — used to pick a curve's source.
pub fn chip_temps(chip: &Path) -> Vec<(u32, String, f32)> {
    (1..=32u32)
        .filter_map(|i| {
            let milli = read_num::<i32>(&chip.join(format!("temp{i}_input")))?;
            let label = read_trim(&chip.join(format!("temp{i}_label"))).unwrap_or_else(|| format!("temp{i}"));
            Some((i, label, milli as f32 / 1000.0))
        })
        .collect()
}

pub const FANCTL: &str = "/usr/local/libexec/coolwave-fanctl";

pub fn fanctl_installed() -> bool {
    Path::new(FANCTL).exists()
}

/// CPU temperature from the AMD driver, if it is loaded.
pub fn cpu_temperature() -> Option<f32> {
    temperatures().into_iter().find(|t| t.chip == "k10temp").map(|t| t.celsius)
}
