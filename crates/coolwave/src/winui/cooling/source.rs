//! Which temperature a curve follows, and what to tell when it cannot be read.

use coolwave_core::hwmon::TempSensor;

/// The chip HWiNFO readings are listed under.
#[cfg(windows)]
pub const HWINFO_CHIP: &str = coolwave_core::hwmon::CHIP_HWINFO;
#[cfg(not(windows))]
pub const HWINFO_CHIP: &str = "hwinfo";

const GPU_CHIP: &str = "nvidia";
/// CPU temperature chips, best first.
const CPU_CHIPS: [&str; 2] = ["k10temp", "coretemp"];
/// Words in a HWiNFO label that mark the CPU temperature.
const CPU_WORDS: [&str; 2] = ["cpu", "tctl"];

/// A sensor a curve can follow, named by chip and label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub chip: String,
    pub label: String,
}

impl Source {
    pub fn of(sensor: &TempSensor) -> Self {
        Self { chip: sensor.chip.clone(), label: sensor.label.clone() }
    }

    pub fn is_hwinfo(&self) -> bool {
        self.chip == HWINFO_CHIP
    }

    pub fn name(&self) -> String {
        if self.label.is_empty() { "no sensor".to_string() } else { format!("{} · {}", self.chip, self.label) }
    }
}

/// The sensor a new curve follows: the CPU if one can be read, then the GPU, then anything.
pub fn default_for(sensors: &[TempSensor]) -> Option<Source> {
    let hwinfo_cpu = sensors.iter().find(|s| s.chip == HWINFO_CHIP && is_cpu_label(&s.label));
    let cpu_chip = || CPU_CHIPS.iter().find_map(|chip| sensors.iter().find(|s| s.chip == *chip));
    let gpu = || sensors.iter().find(|s| s.chip == GPU_CHIP);
    hwinfo_cpu.or_else(cpu_chip).or_else(gpu).or(sensors.first()).map(Source::of)
}

fn is_cpu_label(label: &str) -> bool {
    let label = label.to_lowercase();
    CPU_WORDS.iter().any(|word| label.contains(word))
}

/// Current reading of `source`, if it can be read now.
pub fn reading(sensors: &[TempSensor], source: &Source) -> Option<f32> {
    sensors.iter().find(|s| s.chip == source.chip && s.label == source.label).map(|s| s.celsius)
}

/// Whether the HWiNFO hint is worth showing: it is listed, or the curve relies on it.
pub fn needs_hwinfo_hint(sensors: &[TempSensor], source: Option<&Source>) -> bool {
    sensors.iter().any(|s| s.chip == HWINFO_CHIP) || source.is_some_and(Source::is_hwinfo)
}

/// The warning for a curve whose sensor cannot be read, if it has that problem.
pub fn missing_warning(sensors: &[TempSensor], source: &Source) -> Option<String> {
    if reading(sensors, source).is_some() {
        return None;
    }
    let what = if source.label.is_empty() { "has no temperature source".to_string() } else { format!("cannot read {}", source.name()) };
    Some(format!("This curve {what}, so the fans run at full speed until it is back."))
}

/// All sensors as choices, with the saved source added when it is not among them.
pub fn choices(sensors: &[TempSensor], saved: &Source) -> Vec<Source> {
    let mut all: Vec<Source> = sensors.iter().map(Source::of).collect();
    if !saved.label.is_empty() && !all.contains(saved) {
        all.push(saved.clone());
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensor(chip: &str, label: &str) -> TempSensor {
        TempSensor { chip: chip.into(), label: label.into(), celsius: 40.0 }
    }

    #[test]
    fn linux_prefers_the_cpu_chip() {
        let sensors = [sensor("nvme", "Composite"), sensor("k10temp", "Tctl")];
        assert_eq!(default_for(&sensors).unwrap().chip, "k10temp");
    }

    #[test]
    fn hwinfo_cpu_beats_the_gpu() {
        let sensors = [sensor(GPU_CHIP, "GPU"), sensor(HWINFO_CHIP, "Motherboard"), sensor(HWINFO_CHIP, "CPU Package")];
        assert_eq!(default_for(&sensors).unwrap().label, "CPU Package");
    }

    #[test]
    fn the_gpu_is_the_fallback() {
        let sensors = [sensor(GPU_CHIP, "GPU"), sensor(HWINFO_CHIP, "Motherboard")];
        assert_eq!(default_for(&sensors).unwrap().chip, GPU_CHIP);
    }

    #[test]
    fn no_sensors_means_no_default() {
        assert_eq!(default_for(&[]), None);
    }

    #[test]
    fn a_missing_sensor_is_warned_about() {
        let sensors = [sensor(GPU_CHIP, "GPU")];
        let gone = Source { chip: HWINFO_CHIP.into(), label: "CPU".into() };
        assert!(missing_warning(&sensors, &gone).is_some());
        assert!(missing_warning(&sensors, &Source::of(&sensors[0])).is_none());
    }

    #[test]
    fn the_hint_follows_hwinfo() {
        let gpu_only = [sensor(GPU_CHIP, "GPU")];
        assert!(!needs_hwinfo_hint(&gpu_only, None));
        assert!(needs_hwinfo_hint(&[sensor(HWINFO_CHIP, "CPU")], None));
        let saved = Source { chip: HWINFO_CHIP.into(), label: "CPU".into() };
        assert!(needs_hwinfo_hint(&gpu_only, Some(&saved)));
    }

    #[test]
    fn a_saved_missing_source_stays_selectable() {
        let sensors = [sensor(GPU_CHIP, "GPU")];
        let saved = Source { chip: "k10temp".into(), label: "Tctl".into() };
        assert_eq!(choices(&sensors, &saved).len(), 2);
    }
}
