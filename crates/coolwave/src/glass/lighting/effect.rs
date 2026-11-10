//! The effect choice and what shapes it: colors, speed and brightness.

use super::{colors, rows};
use crate::glass::Shared;
use coolwave_core::effects::Effect;
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::{Slider, Ui};

const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.1..=5.0;
const SPEED_STEP: f64 = 0.1;
const MAX_PERCENT: f32 = 100.0;

/// Every effect as a button. The current one is filled.
pub fn choose(ui: &mut Ui, shared: &mut Shared) {
    let current = shared.engine.config().params.effect;
    theme::card(ui, |ui| {
        widgets::section_label(ui, "Effect");
        ui.horizontal_wrapped(|ui| {
            for effect in Effect::ALL {
                if theme::choice(ui, effect == current, effect.name()).clicked() {
                    shared.set_effect(effect);
                }
            }
        });
    });
}

/// Colors an effect uses, then speed and brightness.
pub fn look(ui: &mut Ui, shared: &mut Shared) {
    let config = shared.engine.config();
    let mut params = config.params;
    // The GPU's built-in animations take both colors whatever the effect is.
    let gpu_animation = config.gpu_enabled && config.gpu_hardware.is_some();
    let (primary, secondary) = params.effect.uses_colors();
    let (primary, secondary) = (primary || gpu_animation, secondary || gpu_animation);

    theme::card(ui, |ui| {
        widgets::section_label(ui, "Look");
        let mut color_changed = false;
        if primary || secondary {
            rows::labeled(ui, color_label(primary, secondary), |ui| {
                if primary {
                    color_changed |= colors::picker(ui, &mut params.primary);
                }
                if secondary {
                    color_changed |= colors::picker(ui, &mut params.secondary);
                }
            });
        } else {
            ui.label(theme::muted("This effect chooses its own colors."));
        }
        let speed_changed = ui
            .add_enabled_ui(!params.effect.is_static(), |ui| {
                widgets::slider_row(ui, "Speed", |ui| {
                    ui.add(Slider::new(&mut params.speed, SPEED_RANGE).step_by(SPEED_STEP).suffix("x"))
                })
            })
            .inner;
        let mut percent = params.brightness * MAX_PERCENT;
        let brightness_changed = widgets::slider_row(ui, "Brightness", |ui| {
            ui.add(Slider::new(&mut percent, 0.0..=MAX_PERCENT).step_by(1.0).suffix("%"))
        });
        params.brightness = percent / MAX_PERCENT;

        if color_changed || speed_changed || brightness_changed {
            shared.engine.update(|config| {
                config.params.primary = params.primary;
                config.params.secondary = params.secondary;
                config.params.speed = params.speed;
                config.params.brightness = params.brightness;
            });
        }
    });
}

fn color_label(primary: bool, secondary: bool) -> &'static str {
    match (primary, secondary) {
        (true, true) => "Colors",
        (true, false) => "Color",
        _ => "Background",
    }
}
