//! The three lighting devices: a switch, what each is doing now, and the GPU's own animations.

use super::rows;
use crate::glass::Shared;
use coolwave_core::ene::HARDWARE_MODES;
use coolwave_core::engine::{DeviceStatus, GpuHardware, Identify};
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::{self, Align, ComboBox, Layout, Ui};

pub const IDENTIFY_SECONDS: u64 = 5;
const SYNCED_LABEL: &str = "Synced with the effect";
const ANIMATION_SPEEDS: [(u8, &str, &str); 5] = [
    (4, "Slowest", ""),
    (3, "Slow", ""),
    (2, "Normal", ""),
    (1, "Fast", ""),
    (0, "Fastest", ""),
];
const DEFAULT_ANIMATION_SPEED: u8 = 2;

/// A small button that blinks one zone white so it can be found in the case.
pub fn identify_button(ui: &mut Ui, shared: &Shared, zone: Identify) {
    let tip = format!("Blink this zone white for {IDENTIFY_SECONDS} seconds");
    if ui.button("Identify").on_hover_text(tip).clicked() {
        shared.engine.identify(zone, IDENTIFY_SECONDS);
    }
}

fn state_text(status: &DeviceStatus) -> String {
    if status.state.starts_with("Connected") && status.fps > 0.0 {
        format!("{} · {:.0} fps", status.state, status.fps)
    } else {
        status.state.clone()
    }
}

pub fn card(ui: &mut Ui, shared: &mut Shared) {
    let status = shared.engine.status();
    let config = shared.engine.config();
    let (mut eurux, mut aura, mut gpu) = (config.eurux_enabled, config.aura_enabled, config.gpu_enabled);
    theme::card(ui, |ui| {
        widgets::section_label(ui, "Devices");
        let eurux_changed = device_row(ui, &mut eurux, "ROG EURUX fans", &status.eurux, None, shared);
        ui.separator();
        let aura_changed = device_row(ui, &mut aura, "Motherboard, AIO and Polymo panel", &status.aura, None, shared);
        ui.separator();
        let gpu_changed = device_row(ui, &mut gpu, "ROG Astral RTX 5090", &status.gpu, Some(Identify::Gpu), shared);
        if eurux_changed || aura_changed || gpu_changed {
            shared.engine.update(|config| {
                config.eurux_enabled = eurux;
                config.aura_enabled = aura;
                config.gpu_enabled = gpu;
            });
        }
        ui.add_space(6.0);
        gpu_animation(ui, shared, config.gpu_hardware);
    });
}

fn device_row(ui: &mut Ui, on: &mut bool, title: &str, status: &DeviceStatus, identify: Option<Identify>, shared: &Shared) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = widgets::switch_row(ui, on, title, &state_text(status));
        if let Some(zone) = identify {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| identify_button(ui, shared, zone));
        }
    });
    changed
}

/// Choose between the effect played by Coolwave and one of the card's own animations.
fn gpu_animation(ui: &mut Ui, shared: &mut Shared, current: Option<GpuHardware>) {
    let mut choice = current;
    rows::labeled(ui, "GPU animation", |ui| {
        let selected = current.and_then(|hw| mode_name(hw.mode)).unwrap_or(SYNCED_LABEL);
        ComboBox::from_id_salt("gpu-animation").selected_text(selected).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), SYNCED_LABEL).clicked() {
                choice = None;
            }
            for (mode, name) in HARDWARE_MODES {
                if ui.selectable_label(current.is_some_and(|hw| hw.mode == *mode), *name).clicked() {
                    let speed = current.map_or(DEFAULT_ANIMATION_SPEED, |hw| hw.speed);
                    choice = Some(GpuHardware { mode: *mode, speed, direction: 0 });
                }
            }
        });
    });
    if let Some(hw) = choice.as_mut() {
        rows::labeled(ui, "Speed", |ui| {
            widgets::segmented(ui, &mut hw.speed, &ANIMATION_SPEEDS);
        });
    }
    ui.label(egui::RichText::new("Built-in animations run on the card itself, which is smoothest.").small().color(theme::colors().muted));
    if choice != current {
        shared.engine.update(|config| config.gpu_hardware = choice);
    }
}

fn mode_name(mode: u8) -> Option<&'static str> {
    HARDWARE_MODES.iter().find(|(m, _)| *m == mode).map(|(_, name)| *name)
}
