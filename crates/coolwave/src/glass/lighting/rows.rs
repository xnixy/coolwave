//! Rows shared by the lighting cards.

use coolwave_ui::theme;
use eframe::egui::{self, Align, Layout, Ui};

/// Width of the label column, matching the sliders from `widgets::slider_row`.
const LABEL_WIDTH: f32 = 104.0;

/// A muted label on the left and whatever `add` draws after it.
pub fn labeled(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(LABEL_WIDTH);
                ui.label(theme::muted(label));
            },
        );
        add(ui);
    });
}
