//! Color pickers for the engine's `Rgb`.

use coolwave_core::Rgb;
use eframe::egui::Ui;

/// A color button that edits `color` in place. True when the color changed.
pub fn picker(ui: &mut Ui, color: &mut Rgb) -> bool {
    let mut srgb = [color.r, color.g, color.b];
    let changed = ui.color_edit_button_srgb(&mut srgb).changed();
    if changed {
        *color = Rgb::new(srgb[0], srgb[1], srgb[2]);
    }
    changed
}
