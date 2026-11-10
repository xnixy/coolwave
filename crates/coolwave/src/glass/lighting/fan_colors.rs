//! Fixed colors for each fan, used instead of the effect while it is Static.

use super::{colors, devices::identify_button};
use crate::glass::Shared;
use coolwave_core::Rgb;
use coolwave_core::engine::{FanPort, Identify};
use coolwave_core::eurux::FanColors;
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::{CollapsingHeader, RichText, Ui};

/// Colors for every fan of every port, in layout order.
pub type Grid = Vec<Vec<FanColors>>;

/// A grid with one entry per fan in `layout`, keeping what `old` already has.
pub fn fitted(old: Option<&Grid>, layout: &[FanPort], fill: Rgb) -> Grid {
    layout
        .iter()
        .enumerate()
        .map(|(port, p)| {
            (0..p.fans)
                .map(|fan| old.and_then(|grid| grid.get(port)?.get(fan).copied()).unwrap_or(FanColors::uniform(fill)))
                .collect()
        })
        .collect()
}

/// The card. `remembered` keeps the colors while the switch is off, so they come back when it is turned on again.
pub fn card(ui: &mut Ui, shared: &mut Shared, remembered: &mut Option<Grid>) {
    let config = shared.engine.config();
    let layout = shared.settings.fan_layout.clone();
    let mut custom = config.eurux_custom.clone();
    let mut changed = false;
    theme::card(ui, |ui| {
        widgets::section_label(ui, "Per-fan colors");
        let mut on = custom.is_some();
        if widgets::switch_row(ui, &mut on, "Use per-fan colors", "Replaces the effect on the fans while the effect is Static.") {
            changed = true;
            if on {
                custom = Some(fitted(remembered.as_ref(), &layout, config.params.primary));
            } else {
                *remembered = custom.take();
            }
        }
        if let Some(grid) = custom.as_mut() {
            *grid = fitted(Some(grid), &layout, config.params.primary);
            ui.add_space(4.0);
            for (port, fans) in layout.iter().zip(grid.iter_mut()) {
                changed |= port_section(ui, shared, port, fans);
            }
        }
    });
    if changed {
        shared.engine.update(|config| config.eurux_custom = custom);
    }
}

fn port_section(ui: &mut Ui, shared: &Shared, port: &FanPort, fans: &mut [FanColors]) -> bool {
    let mut changed = false;
    let title = format!("{} · port {} · {} fan{}", port.name, port.port + 1, port.fans, if port.fans == 1 { "" } else { "s" });
    CollapsingHeader::new(RichText::new(title).color(theme::colors().text)).id_salt(("fan-colors", port.port)).show(ui, |ui| {
        for (number, fan) in fans.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(theme::muted(format!("Fan {}", number + 1)));
                for (name, color) in [("Left", &mut fan.left), ("Middle", &mut fan.middle), ("Right", &mut fan.right)] {
                    changed |= colors::picker(ui, color);
                    ui.label(theme::muted(name).small());
                }
            });
        }
        identify_button(ui, shared, Identify::EuruxPort(port.port));
    });
    changed
}
