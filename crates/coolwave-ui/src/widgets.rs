//! The small controls the pages are built from.

use crate::theme::{self, colors, ACCENT};
use egui::{
    Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, Ui,
};

/// An on/off switch. Animated, and it reads as on or off at a glance where a checkbox does not.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = egui::vec2(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let c = colors();
        let track = c.track.lerp_to_gamma(ACCENT, t);
        let strength = if response.hovered() { 60 } else { 30 };
        let edge = if c.light {
            Color32::from_black_alpha(strength)
        } else {
            Color32::from_white_alpha(strength)
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(11),
            track,
            Stroke::new(1.0_f32, edge),
            StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
        painter.circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    }
    response
}

/// A switch with a label beside it, as one row.
pub fn switch_row(ui: &mut Ui, on: &mut bool, title: &str, detail: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = switch(ui, on).changed();
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).color(colors().text));
            if !detail.is_empty() {
                ui.add(egui::Label::new(theme::muted(detail).small()).wrap());
            }
        });
    });
    changed
}

/// One choice out of a few, drawn as joined buttons. Returns whether the choice changed.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    let c = colors();
    Frame::new()
        .fill(c.well)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(option, label, tip) in options {
                    let selected = *value == option;
                    let text = if selected {
                        RichText::new(label).color(Color32::WHITE)
                    } else {
                        RichText::new(label).color(c.muted)
                    };
                    let button = egui::Button::new(text)
                        .fill(if selected {
                            ACCENT
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 28.0));
                    let mut response = ui.add(button);
                    if !tip.is_empty() {
                        response = response.on_hover_text(tip);
                    }
                    if response.clicked() && !selected {
                        *value = option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A small upper-case heading over a group of rows.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(colors().muted.gamma_multiply(0.85)),
    );
    ui.add_space(2.0);
}

/// The page title and its one-line explanation, with room on the right for controls.
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(title)
                    .size(22.0)
                    .strong()
                    .color(colors().text),
            );
            ui.label(theme::muted(subtitle));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(10.0);
}

/// A label on the left and a slider filling the rest of the row, its number kept inside.
pub fn slider_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> Response) -> bool {
    let budget = ui.available_width();
    let mut changed = false;
    ui.allocate_ui_with_layout(
        egui::vec2(budget, 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(budget);
            let label_w = 104.0_f32.min(budget * 0.35);
            ui.allocate_ui_with_layout(
                egui::vec2(label_w, ui.spacing().interact_size.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(label_w);
                    ui.label(theme::muted(label));
                },
            );
            ui.spacing_mut().slider_width = slider_rail_width(ui);
            changed = add(ui).changed();
        },
    );
    changed
}

/// Width of the number button beside a slider, measured for the widest value.
///
/// `slider_width` is only the rail. The value button is drawn after it, so a rail
/// that uses the whole remaining row paints the number outside the window.
fn slider_value_width(ui: &Ui) -> f32 {
    let padded = text_width(ui, "100%", egui::TextStyle::Button.resolve(ui.style()).size)
        + ui.spacing().button_padding.x * 2.0;
    padded.max(ui.spacing().interact_size.x) + 4.0
}

fn slider_rail_width(ui: &Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    (ui.available_width() - gap - slider_value_width(ui)).max(0.0)
}

pub fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            Color32::WHITE,
        )
        .size()
        .x
}

/// A label and a one-line text field that fills the row. True once you leave the field.
pub fn text_row(ui: &mut Ui, label: &str, text: &mut String, hint: &str) -> bool {
    let mut done = false;
    let row_w = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_width(row_w);
        let label_w = 104.0_f32.min(row_w * 0.35);
        ui.allocate_ui_with_layout(
            egui::vec2(label_w, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(label_w);
                ui.label(theme::muted(label));
            },
        );
        let field = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        done = field.lost_focus();
    });
    done
}

/// The sidebar's pages, each with a small drawn icon so nothing depends on an icon font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavIcon {
    Lighting,
    Cooling,
    PumpScreen,
    Settings,
}

pub fn nav_item(ui: &mut Ui, icon: NavIcon, label: &str, selected: bool) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = colors();
        let fill = if selected {
            c.row_selected
        } else if response.hovered() {
            c.row_hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(10), fill);
        if selected {
            let bar = Rect::from_min_size(
                rect.left_top() + egui::vec2(0.0, 10.0),
                egui::vec2(3.0, rect.height() - 20.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), ACCENT);
        }
        let color = if selected { c.text } else { c.muted };
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        draw_nav_icon(
            painter,
            icon_rect,
            icon,
            if selected { ACCENT } else { color },
        );
        painter.text(
            egui::pos2(rect.left() + 44.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(15.0),
            color,
        );
    }
    response
}

fn draw_nav_icon(painter: &egui::Painter, r: Rect, icon: NavIcon, color: Color32) {
    let stroke = Stroke::new(1.6_f32, color);
    let c = r.center();
    match icon {
        // A sun: a disc with eight rays.
        NavIcon::Lighting => {
            painter.circle_stroke(c, r.width() * 0.22, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = egui::vec2(a.cos(), a.sin());
                painter.line_segment([c + dir * (r.width() * 0.38), c + dir * (r.width() * 0.52)], stroke);
            }
        }
        // A fan: a ring, a hub, and three swept blades.
        NavIcon::Cooling => {
            painter.circle_stroke(c, r.width() / 2.0, stroke);
            for i in 0..3 {
                let a = i as f32 * std::f32::consts::TAU / 3.0 - 0.5;
                let root = c + egui::vec2(a.cos(), a.sin()) * 1.5;
                let tip = c + egui::vec2((a + 0.7).cos(), (a + 0.7).sin()) * (r.width() * 0.36);
                painter.line_segment([root, tip], Stroke::new(2.4_f32, color));
            }
            painter.circle_filled(c, 1.6, color);
        }
        // The pump cap: a round screen in a rounded housing.
        NavIcon::PumpScreen => {
            painter.rect_stroke(r, CornerRadius::same(4), stroke, StrokeKind::Inside);
            painter.circle_stroke(c, r.width() * 0.26, stroke);
        }
        NavIcon::Settings => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = egui::vec2(a.cos(), a.sin());
                painter.line_segment(
                    [c + dir * (r.width() * 0.32), c + dir * (r.width() * 0.5)],
                    Stroke::new(2.2_f32, color),
                );
            }
            painter.circle_stroke(c, r.width() * 0.32, stroke);
            painter.circle_filled(c, 1.8, color);
        }
    }
}

/// An app's icon, or the first letter of its name on a tinted tile.
pub fn toast(ctx: &egui::Context, text: &str, opacity: f32) {
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
        .interactable(false)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            let c = colors();
            Frame::new()
                .fill(c.popup)
                .stroke(Stroke::new(1.0_f32, c.hairline))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(16, 10))
                .shadow(egui::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(if c.light { 40 } else { 90 }),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.0, c.ok);
                        ui.label(RichText::new(text).color(c.text));
                    });
                });
        });
}

/// A banner across the top of a page.
pub fn banner(ui: &mut Ui, color: Color32, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(color.gamma_multiply(0.2))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add_contents);
        });
    ui.add_space(8.0);
}

/// A plain button drawn in the accent red, for things that cannot be undone.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(colors().danger_text))
        .fill(ACCENT.gamma_multiply(0.35))
        .stroke(Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.8)))
}
