//! The Lighting page: the effect, its look, and the devices that play it.

mod colors;
mod devices;
mod effect;
mod fan_colors;
mod rows;
mod zones;

use super::{Shared, columns};
use coolwave_ui::widgets;
use eframe::egui::{self, Ui};

/// The effect list gets a little less than the look beside it.
const EFFECT_SHARE: f32 = 0.5;
const CARD_GAP: f32 = 12.0;

pub struct LightingPage {
    /// Per-fan colors kept while that option is switched off.
    remembered_fan_colors: Option<fan_colors::Grid>,
}

impl LightingPage {
    pub fn new(_shared: &Shared) -> Self {
        Self { remembered_fan_colors: None }
    }

    pub fn ui(&mut self, ui: &mut Ui, shared: &mut Shared) {
        widgets::page_header(ui, "Lighting", "One effect, played in sync on every device you turn on.", |_| {});
        egui::ScrollArea::vertical().id_salt("lighting").auto_shrink([false, false]).show(ui, |ui| {
            columns::two(ui, EFFECT_SHARE, shared, effect::choose, effect::look);
            ui.add_space(CARD_GAP);
            devices::card(ui, shared);
            ui.add_space(CARD_GAP);
            zones::card(ui, shared);
            ui.add_space(CARD_GAP);
            fan_colors::card(ui, shared, &mut self.remembered_fan_colors);
            ui.add_space(CARD_GAP);
        });
    }
}
