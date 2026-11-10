//! What a port remembers while another mode is selected, so switching back
//! to Curve or Fixed speed brings the last settings with it.

use super::curve::{self, Point};
use super::source::{self, Source};
use coolwave_core::engine::FanControl;
use coolwave_core::hwmon::TempSensor;

/// Speed a port is set to the first time it is switched to a fixed speed.
const FIRST_FIXED_PERCENT: u8 = 60;

#[derive(Clone, Debug)]
pub struct Draft {
    pub fixed: u8,
    pub curve: Vec<Point>,
    pub source: Option<Source>,
}

impl Default for Draft {
    fn default() -> Self {
        Self { fixed: FIRST_FIXED_PERCENT, curve: curve::DEFAULT.to_vec(), source: None }
    }
}

impl Draft {
    /// Take over the settings of the mode the port is in.
    pub fn remember(&mut self, control: &FanControl) {
        match control {
            FanControl::Default => {}
            FanControl::Fixed { percent } => self.fixed = *percent,
            FanControl::Curve { chip, label, points } => {
                self.curve = points.clone();
                self.source = Some(Source { chip: chip.clone(), label: label.clone() });
            }
        }
    }

    pub fn fixed_control(&self) -> FanControl {
        FanControl::Fixed { percent: self.fixed }
    }

    /// A curve over the remembered points. A source that is gone is replaced by the default one.
    pub fn curve_control(&self, sensors: &[TempSensor]) -> FanControl {
        let source = self.source.clone().or_else(|| source::default_for(sensors)).unwrap_or(Source { chip: String::new(), label: String::new() });
        let mut points = self.curve.clone();
        curve::normalize(&mut points);
        FanControl::Curve { chip: source.chip, label: source.label, points }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_without_a_saved_source_takes_the_default() {
        let sensors = [TempSensor { chip: "nvidia".into(), label: "GPU".into(), celsius: 50.0 }];
        match Draft::default().curve_control(&sensors) {
            FanControl::Curve { chip, .. } => assert_eq!(chip, "nvidia"),
            other => panic!("expected a curve, got {other:?}"),
        }
    }

    #[test]
    fn remembers_the_last_curve() {
        let mut draft = Draft::default();
        draft.remember(&FanControl::Curve { chip: "k10temp".into(), label: "Tctl".into(), points: vec![(40.0, 50), (80.0, 100)] });
        draft.remember(&FanControl::Default);
        match draft.curve_control(&[]) {
            FanControl::Curve { chip, points, .. } => {
                assert_eq!(chip, "k10temp");
                assert_eq!(points.len(), 2);
            }
            other => panic!("expected a curve, got {other:?}"),
        }
    }
}
