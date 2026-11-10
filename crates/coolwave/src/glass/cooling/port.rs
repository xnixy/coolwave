//! The settings of one fan port: its mode, and a fixed speed or a curve.

use super::curve::{self, Point};
use super::draft::Draft;
use super::graph::{self, Live};
use super::source::{self, Source};
use coolwave_core::engine::{FanControl, FanPort, Status};
use coolwave_core::eurux::MIN_DUTY;
use coolwave_core::hwmon::TempSensor;
use coolwave_ui::theme::{self, colors};
use coolwave_ui::widgets;
use eframe::egui::{self, ComboBox, DragValue, Grid, RichText, Slider, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Default,
    Fixed,
    Curve,
}

impl Mode {
    fn of(control: &FanControl) -> Self {
        match control {
            FanControl::Default => Mode::Default,
            FanControl::Fixed { .. } => Mode::Fixed,
            FanControl::Curve { .. } => Mode::Curve,
        }
    }
}

const MODES: [(Mode, &str, &str); 3] = [
    (Mode::Default, "Controller default", "Leave the fans to the controller's own setting"),
    (Mode::Fixed, "Fixed speed", "Hold one speed"),
    (Mode::Curve, "Curve", "Follow a temperature"),
];

/// Everything the panel needs to show a port.
pub struct View<'a> {
    pub port: &'a FanPort,
    pub control: &'a FanControl,
    pub draft: &'a Draft,
    pub sensors: &'a [TempSensor],
    pub status: &'a Status,
    pub fans_enabled: bool,
}

/// Draws the panel. Gives back the new control when the user changed something.
pub fn show(ui: &mut Ui, view: &View) -> Option<FanControl> {
    ui.label(RichText::new(&view.port.name).size(17.0).strong().color(colors().text));
    ui.label(theme::muted(format!("Port {} · {}", view.port.port + 1, super::fan_count(view.port))));
    ui.add_space(4.0);
    if !view.fans_enabled {
        ui.add(egui::Label::new(theme::muted("Turn on the ROG EURUX fans under Lighting so Coolwave can control them.")).wrap());
    }
    let mut mode = Mode::of(view.control);
    let mut new_control = None;
    if widgets::segmented(ui, &mut mode, &MODES) {
        new_control = Some(match mode {
            Mode::Default => FanControl::Default,
            Mode::Fixed => view.draft.fixed_control(),
            Mode::Curve => view.draft.curve_control(view.sensors),
        });
    }
    ui.add_space(6.0);
    let edited = match view.control {
        FanControl::Default => {
            ui.label(theme::muted("The controller keeps its own speed for this port."));
            None
        }
        FanControl::Fixed { percent } => fixed_speed(ui, *percent),
        FanControl::Curve { chip, label, points } => curve_editor(ui, view, Source { chip: chip.clone(), label: label.clone() }, points),
    };
    new_control.or(edited)
}

fn fixed_speed(ui: &mut Ui, percent: u8) -> Option<FanControl> {
    let mut value = percent.max(MIN_DUTY);
    let changed = widgets::slider_row(ui, "Speed", |ui| ui.add(Slider::new(&mut value, MIN_DUTY..=curve::MAX_DUTY).suffix("%")));
    changed.then_some(FanControl::Fixed { percent: value })
}

fn curve_editor(ui: &mut Ui, view: &View, saved: Source, saved_points: &[Point]) -> Option<FanControl> {
    let mut points = saved_points.to_vec();
    curve::normalize(&mut points);
    let mut changed = points != saved_points;
    let mut selected = saved.clone();

    if let Some(warning) = source::missing_warning(view.sensors, &saved) {
        widgets::banner(ui, colors().warn, |ui| {
            ui.add(egui::Label::new(RichText::new(warning).color(colors().text)).wrap());
        });
    }
    changed |= source_picker(ui, view.sensors, &mut selected);
    if source::needs_hwinfo_hint(view.sensors, Some(&selected)) {
        ui.add(egui::Label::new(theme::muted("HWiNFO readings need HWiNFO running with Shared Memory Support turned on.").small()).wrap());
    }
    ui.add_space(6.0);

    let live = Live {
        temp: source::reading(view.sensors, &selected),
        duty: view.status.eurux_duty.get(view.port.port).copied().flatten(),
    };
    changed |= graph::show(ui, &mut points, &live);
    ui.add_space(4.0);
    changed |= point_fields(ui, &mut points);
    ui.add_space(4.0);
    changed |= presets(ui, &mut points);
    ui.add(egui::Label::new(theme::muted(format!("Fans never go below {MIN_DUTY} %, never slow down as it gets hotter, and the last point is always 100 %.")).small()).wrap());

    changed.then(|| FanControl::Curve { chip: selected.chip, label: selected.label, points })
}

fn source_picker(ui: &mut Ui, sensors: &[TempSensor], selected: &mut Source) -> bool {
    let before = selected.clone();
    let choices = source::choices(sensors, &before);
    widgets_label(ui, "Follows", |ui| {
        let text = if choices.is_empty() { "No sensor found".to_string() } else { before.name() };
        ComboBox::from_id_salt("curve-source").selected_text(text).width(260.0).show_ui(ui, |ui| {
            for choice in &choices {
                let reading = source::reading(sensors, choice).map_or("not available".to_string(), |t| format!("{t:.0} °C"));
                let text = format!("{} · {}", choice.name(), reading);
                if ui.selectable_label(choice == &before, text).clicked() {
                    *selected = choice.clone();
                }
            }
        });
    });
    *selected != before
}

fn widgets_label(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(theme::muted(label));
        add(ui);
    });
}

/// The same points as numbers, for setting them exactly.
fn point_fields(ui: &mut Ui, points: &mut [Point]) -> bool {
    let mut changed = false;
    let last = points.len().saturating_sub(1);
    Grid::new("curve-points").num_columns(points.len() + 1).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(theme::muted("Temp"));
        for index in 0..points.len() {
            let mut temp = points[index].0;
            if ui.add(DragValue::new(&mut temp).range(curve::MIN_TEMP..=curve::MAX_TEMP).suffix(" °C")).changed() {
                curve::move_point(points, index, temp, points[index].1 as f32);
                changed = true;
            }
        }
        ui.end_row();
        ui.label(theme::muted("Speed"));
        for index in 0..points.len() {
            let mut duty = points[index].1;
            let editable = index != last;
            let field = DragValue::new(&mut duty).range(MIN_DUTY..=curve::MAX_DUTY).suffix(" %");
            if ui.add_enabled(editable, field).changed() {
                curve::move_point(points, index, points[index].0, duty as f32);
                changed = true;
            }
        }
        ui.end_row();
    });
    changed
}

fn presets(ui: &mut Ui, points: &mut Vec<Point>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(theme::muted("Presets"));
        for (name, preset) in curve::PRESETS {
            if ui.button(name).clicked() {
                *points = preset.to_vec();
                changed = true;
            }
        }
    });
    changed
}
