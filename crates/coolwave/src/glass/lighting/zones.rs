//! The Aura motherboard headers: which are in use and how many LEDs each has.

use super::devices::identify_button;
use crate::glass::Shared;
use coolwave_core::engine::{AuraZoneInfo, Identify, ZoneCfg};
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::{self, Align, DragValue, Layout, Ui};

const MIN_LEDS: u16 = 1;
const MAX_LEDS: u16 = 500;

pub fn card(ui: &mut Ui, shared: &mut Shared) {
    let zones = shared.engine.status().aura_zones;
    theme::card(ui, |ui| {
        widgets::section_label(ui, "Motherboard headers");
        ui.add(egui::Label::new(theme::muted("Turn off headers with nothing connected and set the real LED count. Fewer LEDs make smoother effects.").small()).wrap());
        ui.add_space(4.0);
        if zones.is_empty() {
            ui.label(theme::muted("The headers appear once the motherboard controller is connected."));
        }
        for (i, zone) in zones.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            zone_row(ui, shared, zone);
        }
    });
}

fn zone_row(ui: &mut Ui, shared: &mut Shared, zone: &AuraZoneInfo) {
    let saved = shared.engine.config().aura_zones.get(&zone.index).copied().unwrap_or_default();
    let mut enabled = saved.enabled;
    let mut leds = saved.leds.unwrap_or(zone.reported_leds).clamp(MIN_LEDS, MAX_LEDS);
    let detail = format!("Header {} · controller reports {} LEDs", zone.index + 1, zone.reported_leds);
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= widgets::switch_row(ui, &mut enabled, &zone.name, &detail);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            identify_button(ui, shared, Identify::Aura(zone.index));
            ui.label(theme::muted("LEDs"));
            changed |= ui.add(DragValue::new(&mut leds).range(MIN_LEDS..=MAX_LEDS)).changed();
        });
    });
    if changed {
        let cfg = ZoneCfg { enabled, leds: Some(leds) };
        shared.engine.update(|config| {
            config.aura_zones.insert(zone.index, cfg);
        });
    }
}
