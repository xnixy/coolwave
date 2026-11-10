//! The Cooling page: speed and curves for the EURUX fan ports.
//!
//! Motherboard fan headers and the AIO pump are not driven from here on
//! Windows, so the page says where they are set instead.

mod curve;
mod draft;
mod graph;
mod port;
mod ports;
mod source;

use super::{Shared, columns};
use coolwave_core::engine::{FanControl, FanPort};
use coolwave_core::eurux;
use coolwave_core::hwmon::{self, TempSensor};
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::{self, Ui};
use std::time::{Duration, Instant};

/// The port list takes this share of the width, the selected port the rest.
const LIST_SHARE: f32 = 0.36;
const CARD_GAP: f32 = 12.0;
/// Reading every sensor can be slow (NVML, shared memory), so not on every frame.
const SENSOR_REFRESH: Duration = Duration::from_secs(1);
const BIOS_NOTE: &str = "Motherboard fan headers and the AIO pump follow the fan curves set in the BIOS.";

/// With `COOLWAVE_UI_FAN_MODE` set to `fixed` or `curve` in preview mode, the first port starts in that mode.
fn preview_mode() -> Option<String> {
    if !super::preview() {
        return None;
    }
    std::env::var("COOLWAVE_UI_FAN_MODE").ok()
}

pub struct CoolingPage {
    selected: usize,
    drafts: Vec<draft::Draft>,
    sensors: Vec<TempSensor>,
    sensors_read: Instant,
}

impl CoolingPage {
    pub fn new(shared: &Shared) -> Self {
        let sensors = hwmon::temperatures();
        let controls = shared.engine.config().eurux_fans;
        let drafts = (0..eurux::PORTS)
            .map(|port| {
                let mut draft = draft::Draft::default();
                if let Some(control) = controls.get(port) {
                    draft.remember(control);
                }
                draft
            })
            .collect();
        let mut page = Self {
            selected: shared.settings.fan_layout.first().map_or(0, |port| port.port),
            drafts,
            sensors,
            sensors_read: Instant::now(),
        };
        if let (Some(mode), Some(first)) = (preview_mode(), shared.settings.fan_layout.first()) {
            let control = match mode.as_str() {
                "fixed" => Some(page.drafts[first.port].fixed_control()),
                "curve" => Some(page.drafts[first.port].curve_control(&page.sensors)),
                _ => None,
            };
            if let Some(control) = control {
                page.apply(shared, first.port, control);
            }
        }
        page
    }

    pub fn ui(&mut self, ui: &mut Ui, shared: &mut Shared) {
        widgets::page_header(ui, "Cooling", "Speed and curves for the ROG EURUX fan ports.", |_| {});
        self.refresh_sensors();
        egui::ScrollArea::vertical().id_salt("cooling").auto_shrink([false, false]).show(ui, |ui| {
            let mut state = (&mut *self, &mut *shared);
            columns::two(ui, LIST_SHARE, &mut state, |ui, (page, shared)| page.port_list(ui, shared), |ui, (page, shared)| page.port_settings(ui, shared));
            ui.add_space(CARD_GAP);
            theme::card(ui, |ui| {
                ui.add(egui::Label::new(theme::muted(BIOS_NOTE)).wrap());
            });
            ui.add_space(CARD_GAP);
        });
    }

    fn refresh_sensors(&mut self) {
        if self.sensors_read.elapsed() >= SENSOR_REFRESH {
            self.sensors = hwmon::temperatures();
            self.sensors_read = Instant::now();
        }
    }

    fn port_list(&mut self, ui: &mut Ui, shared: &Shared) {
        let status = shared.engine.status();
        let controls = shared.engine.config().eurux_fans;
        theme::card(ui, |ui| {
            widgets::section_label(ui, "Fan ports");
            if let Some(clicked) = ports::list(ui, &shared.settings.fan_layout, &controls, &status, self.selected) {
                self.selected = clicked;
            }
        });
    }

    fn port_settings(&mut self, ui: &mut Ui, shared: &mut Shared) {
        let layout = shared.settings.fan_layout.clone();
        let Some(port) = layout.iter().find(|p| p.port == self.selected).or(layout.first()) else {
            theme::card(ui, |ui| {
                ui.label(theme::muted("No fan ports are set up."));
            });
            return;
        };
        let config = shared.engine.config();
        let control = config.eurux_fans.get(port.port).cloned().unwrap_or_default();
        let status = shared.engine.status();
        let draft = self.drafts.get(port.port).cloned().unwrap_or_default();
        let view = port::View { port, control: &control, draft: &draft, sensors: &self.sensors, status: &status, fans_enabled: config.eurux_enabled };
        let edited = theme::card(ui, |ui| port::show(ui, &view));
        if let Some(control) = edited {
            self.apply(shared, port.port, control);
        }
    }

    /// Store a port's new control in the engine and remember it for later mode switches.
    fn apply(&mut self, shared: &Shared, port: usize, control: FanControl) {
        if let Some(draft) = self.drafts.get_mut(port) {
            draft.remember(&control);
        }
        shared.engine.update(|config| {
            if config.eurux_fans.len() <= port {
                config.eurux_fans.resize(port + 1, FanControl::Default);
            }
            config.eurux_fans[port] = control;
        });
    }
}

/// "3 fans", or "1 fan".
fn fan_count(port: &FanPort) -> String {
    format!("{} fan{}", port.fans, if port.fans == 1 { "" } else { "s" })
}
