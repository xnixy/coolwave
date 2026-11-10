//! Two cards side by side on a wide window, stacked on a narrow one.

use eframe::egui::{self, Align, Layout, Ui};

/// Below this width the two columns would be too cramped to read.
const MIN_WIDTH_FOR_TWO: f32 = 640.0;
const GAP: f32 = 12.0;

/// Lays out `left` and `right`, giving the left column `left_share` of the width.
/// Both draw with the same `state`, which they take turns borrowing.
pub fn two<S>(ui: &mut Ui, left_share: f32, state: &mut S, left: impl FnOnce(&mut Ui, &mut S), right: impl FnOnce(&mut Ui, &mut S)) {
    if ui.available_width() < MIN_WIDTH_FOR_TWO {
        left(ui, state);
        ui.add_space(GAP);
        right(ui, state);
        return;
    }
    let usable = ui.available_width() - GAP;
    let left_width = usable * left_share;
    let right_width = usable - left_width;
    // Top-aligned, so cards of different heights still start level.
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.allocate_ui_with_layout(egui::vec2(left_width, 0.0), Layout::top_down(Align::LEFT), |ui| left(ui, state));
        ui.allocate_ui_with_layout(egui::vec2(right_width, 0.0), Layout::top_down(Align::LEFT), |ui| right(ui, state));
    });
}
