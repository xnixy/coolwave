//! The Cooling page: speed and curves for the EURUX fan ports.
//!
//! Motherboard fan headers and the AIO pump are not driven from here on Windows, so the page
//! says where they are set instead.

mod curve;
mod draft;
mod graph;
mod source;

use super::core::{Core, preview};
use super::window::{MainWindow, Message};
use coolwave_core::engine::{FanControl, FanPort, Status};
use coolwave_core::eurux::{self, MIN_DUTY};
use coolwave_core::hwmon::{self, TempSensor};
use coolwave_ui::*;
use curve::Point;
use draft::Draft;
use source::Source;
use std::time::{Duration, Instant};
use windows_reactor::*;

/// Reading every sensor can be slow (NVML, shared memory), so not on every tick.
const SENSOR_REFRESH: Duration = Duration::from_secs(1);
const BIOS_NOTE: &str = "Motherboard fan headers and the AIO pump follow the fan curves set in the BIOS.";
const FIXED: Range = Range::new(MIN_DUTY as f64, curve::MAX_DUTY as f64, 1.0);
const MODES: [&str; 3] = ["Controller default", "Fixed speed", "Curve"];

/// What the page keeps while the window is open.
pub struct CoolingState {
    /// The port whose settings show.
    pub selected: usize,
    /// What each port remembers of the modes it is not in.
    pub drafts: Vec<Draft>,
    pub sensors: Vec<TempSensor>,
    sensors_read: Instant,
    /// The curve point being dragged.
    pub dragging: Option<usize>,
}

impl CoolingState {
    pub fn new(core: &Core) -> Self {
        let controls = core.engine.config().eurux_fans;
        let drafts = (0..eurux::PORTS)
            .map(|port| {
                let mut draft = Draft::default();
                if let Some(control) = controls.get(port) {
                    draft.remember(control);
                }
                draft
            })
            .collect();
        let mut state = Self {
            selected: core.settings.fan_layout.first().map_or(0, |port| port.port),
            drafts,
            sensors: hwmon::temperatures(),
            sensors_read: Instant::now(),
            dragging: None,
        };
        state.preview_mode(core);
        state
    }

    /// With `COOLWAVE_UI_FAN_MODE` set to `fixed` or `curve` in preview mode, the first port
    /// starts in that mode.
    fn preview_mode(&mut self, core: &Core) {
        let Some(first) = core.settings.fan_layout.first().filter(|_| preview()) else { return };
        let control = match std::env::var("COOLWAVE_UI_FAN_MODE").as_deref() {
            Ok("fixed") => self.drafts[first.port].fixed_control(),
            Ok("curve") => self.drafts[first.port].curve_control(&self.sensors),
            _ => return,
        };
        self.apply(core, first.port, control);
    }

    pub fn refresh_sensors(&mut self) {
        if self.sensors_read.elapsed() >= SENSOR_REFRESH {
            self.sensors = hwmon::temperatures();
            self.sensors_read = Instant::now();
        }
    }

    /// Store a port's new control in the engine and remember it for later mode switches.
    fn apply(&mut self, core: &Core, port: usize, control: FanControl) {
        if let Some(draft) = self.drafts.get_mut(port) {
            draft.remember(&control);
        }
        core.engine.update(|config| {
            if config.eurux_fans.len() <= port {
                config.eurux_fans.resize(port + 1, FanControl::Default);
            }
            config.eurux_fans[port] = control;
        });
    }
}

/// "3 fans", or "1 fan".
pub fn fan_count(port: &FanPort) -> String {
    format!("{} fan{}", port.fans, if port.fans == 1 { "" } else { "s" })
}

/// Short name of how a port is driven, for under its title.
fn mode_name(control: &FanControl) -> &'static str {
    match control {
        FanControl::Default => MODES[0],
        FanControl::Fixed { .. } => MODES[1],
        FanControl::Curve { .. } => MODES[2],
    }
}

/// One line of live numbers, or why there are none.
fn live_text(status: &Status, port: usize) -> String {
    if !status.eurux.state.starts_with("Connected") {
        return status.eurux.state.clone();
    }
    let rpm = status.eurux_rpm.get(port).copied().unwrap_or(0);
    match status.eurux_duty.get(port).copied().flatten() {
        Some(duty) => format!("{rpm} RPM · {duty} %"),
        None => format!("{rpm} RPM"),
    }
}

/// A curve's points made valid, its source, and whether the control is a curve at all.
fn curve_of(control: &FanControl) -> Option<(Source, Vec<Point>)> {
    match control {
        FanControl::Curve { chip, label, points } => {
            let mut points = points.clone();
            curve::normalize(&mut points);
            Some((Source { chip: chip.clone(), label: label.clone() }, points))
        }
        _ => None,
    }
}

impl MainWindow {
    /// The selected port as the layout has it, or the first one.
    fn selected_port(&self) -> Option<FanPort> {
        let core = self.core();
        let layout = &core.settings.fan_layout;
        layout.iter().find(|port| port.port == self.cooling.selected).or(layout.first()).cloned()
    }

    fn control_of(&self, port: usize) -> FanControl {
        self.core().engine.config().eurux_fans.get(port).cloned().unwrap_or_default()
    }

    /// Give the selected port's curve new points (or keep them) and apply it.
    fn edit_curve(&mut self, change: impl FnOnce(&mut Vec<Point>)) {
        let Some(port) = self.selected_port() else { return };
        let Some((source, mut points)) = curve_of(&self.control_of(port.port)) else { return };
        let before = points.clone();
        change(&mut points);
        curve::normalize(&mut points);
        if points != before {
            let core = self.core.0.borrow();
            self.cooling.apply(&core, port.port, FanControl::Curve { chip: source.chip, label: source.label, points });
        }
    }

    pub(super) fn cooling_update(&mut self, message: Message) {
        let Some(port) = self.selected_port() else { return };
        match message {
            Message::SelectPort(port) => {
                self.cooling.selected = port;
                self.cooling.dragging = None;
            }
            Message::Mode(Some(index)) => {
                let current = self.control_of(port.port);
                if mode_name(&current) == MODES[index.min(2)] {
                    return;
                }
                let draft = self.cooling.drafts.get(port.port).cloned().unwrap_or_default();
                let control = match index {
                    0 => FanControl::Default,
                    1 => draft.fixed_control(),
                    _ => draft.curve_control(&self.cooling.sensors),
                };
                let core = self.core.0.borrow();
                self.cooling.apply(&core, port.port, control);
            }
            Message::Fixed(value) => {
                let percent = FIXED.snap(value) as u8;
                if matches!(self.control_of(port.port), FanControl::Fixed { percent: now } if now != percent) {
                    let core = self.core.0.borrow();
                    self.cooling.apply(&core, port.port, FanControl::Fixed { percent });
                }
            }
            Message::Source(Some(index)) => {
                let Some((saved, points)) = curve_of(&self.control_of(port.port)) else { return };
                let Some(chosen) = source::choices(&self.cooling.sensors, &saved).into_iter().nth(index) else { return };
                if chosen != saved {
                    let core = self.core.0.borrow();
                    self.cooling.apply(&core, port.port, FanControl::Curve { chip: chosen.chip, label: chosen.label, points });
                }
            }
            Message::CurvePress(x, y) => {
                self.cooling.dragging = curve_of(&self.control_of(port.port)).and_then(|(_, points)| graph::point_at(&points, x, y));
            }
            Message::CurveMove(x, y) => {
                if let Some(index) = self.cooling.dragging {
                    let (temp, duty) = graph::value_at(x, y);
                    self.edit_curve(|points| curve::move_point(points, index, temp, duty));
                }
            }
            Message::CurveRelease => self.cooling.dragging = None,
            Message::PointTemp(index, Some(temp)) if temp.is_finite() => {
                self.edit_curve(|points| {
                    let duty = points.get(index).map_or(0.0, |point| point.1 as f32);
                    curve::move_point(points, index, temp as f32, duty);
                });
            }
            Message::PointDuty(index, Some(duty)) if duty.is_finite() => {
                self.edit_curve(|points| {
                    let temp = points.get(index).map_or(0.0, |point| point.0);
                    curve::move_point(points, index, temp, duty as f32);
                });
            }
            Message::Preset(index) => {
                if let Some((_, preset)) = curve::PRESETS.get(index) {
                    self.edit_curve(|points| *points = preset.to_vec());
                }
            }
            _ => {}
        }
    }

    pub(super) fn cooling_page(&self, context: &ViewContext<Self>) -> View {
        page_frame(StackPanel::new().spacing(22.0).max_width(1100.0).children((
            page_header("Cooling", "Speed and curves for the ROG EURUX fan ports."),
            columns(0.36, 0.64, self.port_list(context), self.port_settings(context)),
            padded_card(muted(BIOS_NOTE)),
        )))
    }

    /// The ports with their live speed. Clicking one shows its settings beside the list.
    fn port_list(&self, context: &ViewContext<Self>) -> View {
        let core = self.core();
        let status = core.engine.status();
        let controls = core.engine.config().eurux_fans;
        let selected = self.selected_port().map(|port| port.port);
        let rows: Vec<View> = core
            .settings
            .fan_layout
            .iter()
            .map(|port| {
                let control = controls.get(port.port).cloned().unwrap_or_default();
                let chosen = selected == Some(port.port);
                let mut button = Button::new()
                    .style(ButtonStyle::Subtle)
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .horizontal_content_alignment(HorizontalAlignment::Stretch)
                    .min_height(56.0)
                    .automation_name(&port.name)
                    .on_click(context.message(Message::SelectPort(port.port)))
                    .content(
                        Grid::new().columns([GridLength::STAR, GridLength::Auto]).column_spacing(12.0).children((
                            StackPanel::new()
                                .vertical_alignment(VerticalAlignment::Center)
                                .children((TextBlock::new().text(&port.name), caption(&format!("{} · {}", fan_count(port), mode_name(&control))))),
                            caption(&live_text(&status, port.port)).grid_column(1).vertical_alignment(VerticalAlignment::Center),
                        )),
                    );
                if chosen {
                    button = button.background(ThemeBrush::SubtleSecondary);
                }
                let pill: View = if chosen {
                    Border::new()
                        .width(3.0)
                        .height(20.0)
                        .corner_radius(2.0)
                        .background(ThemeBrush::Accent)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .vertical_alignment(VerticalAlignment::Center)
                        .into()
                } else {
                    nothing()
                };
                Grid::new().children((button, pill)).into()
            })
            .collect();
        padded_card(StackPanel::new().spacing(10.0).children((strong("Fan ports"), StackPanel::new().spacing(4.0).children(rows)))).into()
    }

    /// The settings of one fan port: its mode, and a fixed speed or a curve.
    fn port_settings(&self, context: &ViewContext<Self>) -> View {
        let Some(port) = self.selected_port() else {
            return padded_card(muted("No fan ports are set up.")).into();
        };
        let control = self.control_of(port.port);
        let fans_enabled = self.core().engine.config().eurux_enabled;
        let mode = match control {
            FanControl::Default => 0,
            FanControl::Fixed { .. } => 1,
            FanControl::Curve { .. } => 2,
        };
        let mut parts: Vec<View> = vec![StackPanel::new()
            .children((subtitle(&port.name), caption(&format!("Port {} · {}", port.port + 1, fan_count(&port)))))
            .into()];
        if !fans_enabled {
            parts.push(warning("Turn on the ROG EURUX fans under Lighting so Coolwave can control them.").into());
        }
        parts.push(
            RadioButtons::new()
                .automation_name("Mode")
                .items_source(MODES)
                .max_columns(3)
                .selected_index(Some(mode))
                .on_selection_changed(context.callback(Message::Mode))
                .into(),
        );
        match &control {
            FanControl::Default => parts.push(muted("The controller keeps its own speed for this port.").into()),
            FanControl::Fixed { percent } => {
                let value = (*percent).max(MIN_DUTY) as f64;
                parts.push(labeled("Speed", slider(value, FIXED, format!("{value:.0}%"), "Fixed speed", true, context.callback(Message::Fixed))));
            }
            FanControl::Curve { .. } => parts.extend(self.curve_editor(context, &port, &control)),
        }
        padded_card(StackPanel::new().spacing(14.0).children(parts)).into()
    }

    fn curve_editor(&self, context: &ViewContext<Self>, port: &FanPort, control: &FanControl) -> Vec<View> {
        let Some((saved, points)) = curve_of(control) else { return Vec::new() };
        let sensors = &self.cooling.sensors;
        let mut parts: Vec<View> = Vec::new();
        if let Some(text) = source::missing_warning(sensors, &saved) {
            parts.push(banner(InfoBarSeverity::Warning, &text).into());
        }
        let choices = source::choices(sensors, &saved);
        let names: Vec<String> = choices
            .iter()
            .map(|choice| {
                let reading = source::reading(sensors, choice).map_or("not available".to_string(), |t| format!("{t:.0} °C"));
                format!("{} · {}", choice.name(), reading)
            })
            .collect();
        let picker: View = if names.is_empty() {
            muted("No sensor found").into()
        } else {
            ComboBox::new()
                .automation_name("Temperature the curve follows")
                .min_width(280.0)
                .items_source(names)
                .selected_index(choices.iter().position(|choice| *choice == saved))
                .on_selection_changed(context.callback(Message::Source))
                .into()
        };
        parts.push(labeled("Follows", picker));
        if source::needs_hwinfo_hint(sensors, Some(&saved)) {
            parts.push(caption("HWiNFO readings need HWiNFO running with Shared Memory Support turned on.").into());
        }
        let live = graph::Live {
            temp: source::reading(sensors, &saved),
            duty: self.core().engine.status().eurux_duty.get(port.port).copied().flatten(),
        };
        parts.push(graph::view(
            &points,
            &live,
            self.cooling.dragging,
            context.callback(|info: PointerEventInfo| Message::CurvePress(info.x, info.y)),
            context.callback(|info: PointerEventInfo| Message::CurveMove(info.x, info.y)),
            context.callback(|_: PointerEventInfo| Message::CurveRelease),
        ));
        parts.push(point_fields(context, &points));
        let presets: Vec<View> = curve::PRESETS
            .iter()
            .enumerate()
            .map(|(index, (name, _))| Button::new().content(*name).on_click(context.message(Message::Preset(index))).into())
            .collect();
        parts.push(labeled("Presets", line(8.0, presets)));
        parts.push(
            caption(&format!("Fans never go below {MIN_DUTY} %, never slow down as it gets hotter, and the last point is always 100 %.")).into(),
        );
        parts
    }
}

/// The same points as numbers, for setting them exactly.
fn point_fields(context: &ViewContext<MainWindow>, points: &[Point]) -> View {
    let last = points.len().saturating_sub(1);
    let mut columns = vec![GridLength::Pixel(56.0)];
    columns.extend(points.iter().map(|_| GridLength::STAR));
    let mut cells: Vec<View> = vec![
        muted("Temp").grid_row(0).vertical_alignment(VerticalAlignment::Center).into(),
        muted("Speed").grid_row(1).vertical_alignment(VerticalAlignment::Center).into(),
    ];
    for (index, &(temp, duty)) in points.iter().enumerate() {
        let column = index as i32 + 1;
        cells.push(
            NumberBox::new()
                .grid_row(0)
                .grid_column(column)
                .automation_name(format!("Point {} temperature, °C", index + 1))
                .minimum(curve::MIN_TEMP as f64)
                .maximum(curve::MAX_TEMP as f64)
                .small_change(1.0)
                .value(Some(temp as f64))
                .on_value_changed(context.callback(move |value| Message::PointTemp(index, value)))
                .into(),
        );
        cells.push(
            NumberBox::new()
                .grid_row(1)
                .grid_column(column)
                .automation_name(format!("Point {} speed, %", index + 1))
                .minimum(MIN_DUTY as f64)
                .maximum(curve::MAX_DUTY as f64)
                .small_change(1.0)
                .is_enabled(index != last)
                .value(Some(duty as f64))
                .on_value_changed(context.callback(move |value| Message::PointDuty(index, value)))
                .into(),
        );
    }
    Grid::new()
        .columns(columns)
        .rows([GridLength::Auto, GridLength::Auto])
        .column_spacing(6.0)
        .row_spacing(6.0)
        .children(cells)
        .into()
}
