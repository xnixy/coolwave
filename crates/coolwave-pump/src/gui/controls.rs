//! Small controls the pump page needs that the shared toolkit does not have.

use crate::media::Crop;
use coolwave_ui::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Response, RichText,
    Sense, Stroke, StrokeKind, TextureHandle, Ui,
};
use coolwave_ui::theme::{self, colors};
use coolwave_ui::widgets;

/// The widest the picture in the crop view grows.
const CROP_VIEW_MAX: f32 = 280.0;
/// Outline of the crop square: a pale rose that shows on any picture.
const CROP_OUTLINE: Color32 = Color32::from_rgb(255, 228, 232);
const CROP_SHADE_ALPHA: u8 = 140;
const SWATCH_SIZE: f32 = 24.0;
const SWATCH_RADIUS: f32 = 9.0;
const ROW_LABEL_WIDTH: f32 = 96.0;
const CARD_TITLE_SIZE: f32 = 16.0;

pub fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// The heading at the top of a card.
pub fn card_title(text: &str) -> RichText {
    RichText::new(text)
        .size(CARD_TITLE_SIZE)
        .color(colors().text)
}

/// A slider row that hands back the slider's response, so callers can save
/// when the drag ends instead of on every step.
pub fn slider_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    let mut response = None;
    widgets::slider_row(ui, label, |ui| {
        let slider = add(ui);
        response = Some(slider.clone());
        slider
    });
    response.expect("slider_row runs its closure")
}

/// True when a slider's value should be saved: the drag ended or the field lost focus.
pub fn settled(response: &Response) -> bool {
    response.drag_stopped() || response.lost_focus()
}

/// The name at the start of a row, as wide as the one beside a slider.
pub fn row_label(ui: &mut Ui, label: &str) {
    let width = ROW_LABEL_WIDTH.min(ui.available_width() * 0.35);
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.label(theme::muted(label));
        },
    );
}

/// A rounded label in a color, such as "On the pump".
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.22))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(11.5));
        });
}

/// A row of color dots. Returns the one clicked.
pub fn swatches(ui: &mut Ui, current: [u8; 3], palette: &[[u8; 3]]) -> Option<[u8; 3]> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for &rgb in palette {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(SWATCH_SIZE, SWATCH_SIZE), Sense::click());
            let response = response.on_hover_cursor(CursorIcon::PointingHand);
            let painter = ui.painter();
            painter.circle_filled(rect.center(), SWATCH_RADIUS, color32(rgb));
            let ring = SWATCH_RADIUS + 2.5;
            if rgb == current {
                painter.circle_stroke(rect.center(), ring, Stroke::new(2.0_f32, colors().text));
            } else if response.hovered() {
                painter.circle_stroke(
                    rect.center(),
                    ring,
                    Stroke::new(1.0_f32, Color32::from_white_alpha(90)),
                );
            }
            if response.clicked() {
                picked = Some(rgb);
            }
        }
    });
    picked
}

/// The chosen picture with the crop square over it. Drag to pan. True when a drag ends.
pub fn crop_view(
    ui: &mut Ui,
    texture: &TextureHandle,
    source: (u32, u32),
    crop: &mut Crop,
) -> bool {
    let source_width = source.0.max(1) as f32;
    let source_height = source.1.max(1) as f32;
    let max_w = ui.available_width().min(CROP_VIEW_MAX);
    let aspect = source_width / source_height;
    let (disp_w, disp_h) = if aspect >= 1.0 {
        (max_w, max_w / aspect)
    } else {
        (max_w * aspect, max_w)
    };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(disp_w, disp_h), Sense::click_and_drag());
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter()
        .image(texture.id(), rect, whole, Color32::WHITE);
    let (side, x, y) = crop.square(source.0, source.1);
    let sx = disp_w / source_width;
    let sy = disp_h / source_height;
    let square = egui::Rect::from_min_size(
        rect.min + egui::vec2(x as f32 * sx, y as f32 * sy),
        egui::vec2(side as f32 * sx, side as f32 * sy),
    );
    shade_outside(ui, rect, square);
    ui.painter().rect_stroke(
        square,
        2.0,
        Stroke::new(2.0_f32, CROP_OUTLINE),
        StrokeKind::Inside,
    );
    if response.dragged() {
        let delta = response.drag_delta();
        let max_x = (source_width - side as f32).max(1.0);
        let max_y = (source_height - side as f32).max(1.0);
        crop.pan_x = (crop.pan_x + delta.x / (max_x * sx) * 2.0).clamp(-1.0, 1.0);
        crop.pan_y = (crop.pan_y + delta.y / (max_y * sy) * 2.0).clamp(-1.0, 1.0);
    }
    response.drag_stopped()
}

/// Darkens the four strips of `outer` that the crop square leaves out.
fn shade_outside(ui: &Ui, outer: egui::Rect, square: egui::Rect) {
    let shade = Color32::from_black_alpha(CROP_SHADE_ALPHA);
    let strips = [
        egui::Rect::from_min_max(outer.min, egui::pos2(outer.max.x, square.min.y)),
        egui::Rect::from_min_max(egui::pos2(outer.min.x, square.max.y), outer.max),
        egui::Rect::from_min_max(egui::pos2(outer.min.x, square.min.y), square.left_bottom()),
        egui::Rect::from_min_max(square.right_top(), egui::pos2(outer.max.x, square.max.y)),
    ];
    for strip in strips {
        ui.painter().rect_filled(strip, 0.0, shade);
    }
}
