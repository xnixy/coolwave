//! The list of fan ports with their live speed.

use coolwave_core::engine::{FanControl, FanPort, Status};
use coolwave_ui::theme::{ACCENT, colors};
use eframe::egui::{self, Align2, CornerRadius, CursorIcon, FontId, Pos2, Sense, Ui};

const ROW_HEIGHT: f32 = 54.0;
const ROW_PADDING: f32 = 12.0;

/// Short name of how a port is driven, for under its title.
pub fn mode_name(control: &FanControl) -> &'static str {
    match control {
        FanControl::Default => "Controller default",
        FanControl::Fixed { .. } => "Fixed speed",
        FanControl::Curve { .. } => "Curve",
    }
}

/// One line of live numbers, or why there are none.
pub fn live_text(status: &Status, port: usize) -> String {
    if !status.eurux.state.starts_with("Connected") {
        return status.eurux.state.clone();
    }
    let rpm = status.eurux_rpm.get(port).copied().unwrap_or(0);
    match status.eurux_duty.get(port).copied().flatten() {
        Some(duty) => format!("{rpm} RPM · {duty} %"),
        None => format!("{rpm} RPM"),
    }
}

/// Draws the rows. Returns the port number that was clicked.
pub fn list(ui: &mut Ui, layout: &[FanPort], controls: &[FanControl], status: &Status, selected: usize) -> Option<usize> {
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 4.0;
    for port in layout {
        let control = controls.get(port.port).unwrap_or(&FanControl::Default);
        let subtitle = format!("{} · {}", super::fan_count(port), mode_name(control));
        if row(ui, &port.name, &subtitle, &live_text(status, port.port), port.port == selected) {
            clicked = Some(port.port);
        }
    }
    clicked
}

fn row(ui: &mut Ui, title: &str, subtitle: &str, live: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let palette = colors();
    let painter = ui.painter();
    let fill = if selected {
        palette.row_selected
    } else if response.hovered() {
        palette.row_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    if selected {
        painter.rect_filled(egui::Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 10.0), egui::vec2(3.0, rect.height() - 20.0)), CornerRadius::same(2), ACCENT);
    }
    let left = rect.left() + ROW_PADDING;
    painter.text(Pos2::new(left, rect.center().y - 9.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), palette.text);
    painter.text(Pos2::new(left, rect.center().y + 10.0), Align2::LEFT_CENTER, subtitle, FontId::proportional(12.0), palette.muted);
    painter.text(Pos2::new(rect.right() - ROW_PADDING, rect.center().y), Align2::RIGHT_CENTER, live, FontId::proportional(13.0), palette.muted);
    response.clicked()
}
