//! coolwave-fanctl: minimal privileged helper for motherboard fan control.
//!
//! Runs as root via pkexec. It only ever writes to the PWM attributes of the
//! Super I/O fan chip (hwmon device named `nct*`) and validates every value, so
//! it cannot be used to write arbitrary sysfs files.
//!
//! Usage:
//!   coolwave-fanctl curve  <channel> <temp_c>:<pwm> ...   (exactly one pair per curve point)
//!   coolwave-fanctl source <channel> <temp_index>
//!   coolwave-fanctl auto   <channel>                        (hardware curve mode)
//!   coolwave-fanctl full   <channel>                        (100 %)
//!   coolwave-fanctl manual <channel> <pwm>                  (fixed duty, >= 20 %)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MIN_MANUAL_PWM: u32 = 51; // 20 %
const MAX_CURVE_TEMP: u32 = 100;

fn fan_chip() -> Result<PathBuf, String> {
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon")
        .map_err(|e| format!("cannot list hwmon: {e}"))?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .find(|d| fs::read_to_string(d.join("name")).is_ok_and(|n| n.trim().starts_with("nct")))
        .ok_or_else(|| "no nct67xx fan controller (load the nct6775 driver)".into())
}

fn write(chip: &Path, attr: &str, value: u32) -> Result<(), String> {
    let p = chip.join(attr);
    fs::write(&p, value.to_string()).map_err(|e| format!("write {}: {e}", p.display()))
}

fn channel(chip: &Path, s: &str) -> Result<u32, String> {
    let ch: u32 = s.parse().map_err(|_| format!("invalid channel {s:?}"))?;
    if !(1..=16).contains(&ch) || !chip.join(format!("pwm{ch}")).exists() {
        return Err(format!("no PWM channel {ch}"));
    }
    Ok(ch)
}

fn curve_points(chip: &Path, ch: u32) -> usize {
    (1..=8).take_while(|p| chip.join(format!("pwm{ch}_auto_point{p}_temp")).exists()).count()
}

fn parse_points(args: &[String]) -> Result<Vec<(u32, u32)>, String> {
    args.iter()
        .map(|a| {
            let (t, p) = a.split_once(':').ok_or_else(|| format!("point {a:?} must be temp:pwm"))?;
            let t: u32 = t.parse().map_err(|_| format!("invalid temperature in {a:?}"))?;
            let p: u32 = p.parse().map_err(|_| format!("invalid pwm in {a:?}"))?;
            Ok((t, p))
        })
        .collect()
}

/// Safety rules: temperatures strictly increasing and <= 100 °C, PWM 0..=255 and
/// non-decreasing, last point at 100 % so the fan always ends at full speed.
pub fn validate_curve(points: &[(u32, u32)], expected: usize) -> Result<(), String> {
    if points.len() != expected {
        return Err(format!("this header needs exactly {expected} points, got {}", points.len()));
    }
    for w in points.windows(2) {
        if w[1].0 <= w[0].0 {
            return Err("temperatures must strictly increase".into());
        }
        if w[1].1 < w[0].1 {
            return Err("fan speed must not decrease as temperature rises".into());
        }
    }
    if points.iter().any(|&(t, p)| t > MAX_CURVE_TEMP || p > 255) {
        return Err(format!("temperatures must be <= {MAX_CURVE_TEMP} °C and pwm <= 255"));
    }
    if points.last().map(|p| p.1) != Some(255) {
        return Err("the last point must be 100 % (pwm 255)".into());
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let chip = fan_chip()?;
    let cmd = args.first().map(String::as_str).ok_or("missing command")?;
    let ch = channel(&chip, args.get(1).ok_or("missing channel")?)?;
    match cmd {
        "curve" => {
            let points = parse_points(&args[2..])?;
            validate_curve(&points, curve_points(&chip, ch))?;
            for (i, (t, p)) in points.iter().enumerate() {
                write(&chip, &format!("pwm{ch}_auto_point{}_temp", i + 1), t * 1000)?;
                write(&chip, &format!("pwm{ch}_auto_point{}_pwm", i + 1), *p)?;
            }
            write(&chip, &format!("pwm{ch}_enable"), 5)
        }
        "source" => {
            let idx: u32 = args.get(2).ok_or("missing temp index")?.parse().map_err(|_| "invalid temp index")?;
            if !chip.join(format!("temp{idx}_input")).exists() {
                return Err(format!("no temperature input {idx}"));
            }
            write(&chip, &format!("pwm{ch}_temp_sel"), idx)
        }
        "auto" => write(&chip, &format!("pwm{ch}_enable"), 5),
        "full" => write(&chip, &format!("pwm{ch}_enable"), 0),
        "manual" => {
            let v: u32 = args.get(2).ok_or("missing pwm")?.parse().map_err(|_| "invalid pwm")?;
            if !(MIN_MANUAL_PWM..=255).contains(&v) {
                return Err(format!("manual pwm must be {MIN_MANUAL_PWM}..=255"));
            }
            write(&chip, &format!("pwm{ch}_enable"), 1)?;
            write(&chip, &format!("pwm{ch}"), v)
        }
        other => Err(format!("unknown command {other:?}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("coolwave-fanctl: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_curve;

    #[test]
    fn accepts_sane_curve() {
        assert!(validate_curve(&[(20, 51), (30, 66), (40, 86), (50, 114), (90, 255)], 5).is_ok());
    }

    #[test]
    fn rejects_unsafe_curves() {
        assert!(validate_curve(&[(20, 51), (30, 66), (40, 86), (50, 114), (90, 200)], 5).is_err()); // not ending at 100 %
        assert!(validate_curve(&[(20, 100), (30, 66), (40, 86), (50, 114), (90, 255)], 5).is_err()); // decreasing
        assert!(validate_curve(&[(20, 51), (20, 66), (40, 86), (50, 114), (90, 255)], 5).is_err()); // temp not increasing
        assert!(validate_curve(&[(20, 51), (30, 66), (40, 86), (50, 114), (120, 255)], 5).is_err()); // too hot
        assert!(validate_curve(&[(20, 51), (90, 255)], 5).is_err()); // wrong count
    }
}
