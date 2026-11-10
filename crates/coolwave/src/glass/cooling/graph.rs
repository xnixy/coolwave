//! The curve drawn as a graph, with points to drag and the live temperature.

use super::curve::{self, Point};
use coolwave_core::engine::curve_duty;
use coolwave_ui::theme::{ACCENT, colors};
use eframe::egui::{self, Align2, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};

const HEIGHT: f32 = 230.0;
/// Room around the plot for the axis labels.
const PAD_LEFT: f32 = 40.0;
const PAD_RIGHT: f32 = 14.0;
const PAD_TOP: f32 = 12.0;
const PAD_BOTTOM: f32 = 24.0;
const GRID_STEP: f32 = 25.0;
const POINT_RADIUS: f32 = 6.0;
/// Grabbing a point works from a bit further than it is drawn.
const GRAB_RADIUS: f32 = 12.0;
const LABEL_SIZE: f32 = 11.0;

/// The temperature the curve follows and the duty the fan is at now.
pub struct Live {
    pub temp: Option<f32>,
    pub duty: Option<u8>,
}

/// Where the plot sits inside the graph's rectangle, and how values map onto it.
struct Plot {
    area: Rect,
}

impl Plot {
    fn new(outer: Rect) -> Self {
        let min = outer.min + egui::vec2(PAD_LEFT, PAD_TOP);
        let max = outer.max - egui::vec2(PAD_RIGHT, PAD_BOTTOM);
        Self { area: Rect::from_min_max(min, max) }
    }

    fn x(&self, temp: f32) -> f32 {
        self.area.left() + temp.clamp(curve::MIN_TEMP, curve::MAX_TEMP) / curve::MAX_TEMP * self.area.width()
    }

    fn y(&self, duty: f32) -> f32 {
        self.area.bottom() - duty.clamp(0.0, 100.0) / 100.0 * self.area.height()
    }

    fn at(&self, point: Point) -> Pos2 {
        Pos2::new(self.x(point.0), self.y(point.1 as f32))
    }

    fn temp_at(&self, x: f32) -> f32 {
        (x - self.area.left()) / self.area.width() * curve::MAX_TEMP
    }

    fn duty_at(&self, y: f32) -> f32 {
        (self.area.bottom() - y) / self.area.height() * 100.0
    }
}

/// Draws the graph and lets the points be dragged. True when the curve changed.
pub fn show(ui: &mut Ui, points: &mut [Point], live: &Live) -> bool {
    let width = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::hover());
    let plot = Plot::new(outer);
    let changed = drag_points(ui, &plot, points);
    draw_grid(ui, outer, &plot);
    draw_curve(ui, &plot, points);
    draw_live(ui, &plot, points, live);
    changed
}

fn drag_points(ui: &mut Ui, plot: &Plot, points: &mut [Point]) -> bool {
    let mut changed = false;
    for index in 0..points.len() {
        let center = plot.at(points[index]);
        let area = Rect::from_center_size(center, egui::vec2(GRAB_RADIUS * 2.0, GRAB_RADIUS * 2.0));
        let response = ui.interact(area, ui.id().with(("curve-point", index)), Sense::drag());
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let before = points[index];
                curve::move_point(points, index, plot.temp_at(pointer.x), plot.duty_at(pointer.y));
                changed |= points[index] != before;
            }
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if response.hovered() || response.dragged() {
            let (temp, duty) = points[index];
            response.on_hover_text(format!("{temp:.0} °C → {duty} %"));
        }
    }
    changed
}

fn draw_grid(ui: &Ui, outer: Rect, plot: &Plot) {
    let painter = ui.painter();
    let palette = colors();
    painter.rect(plot.area, CornerRadius::same(6), palette.well, Stroke::new(1.0_f32, palette.hairline), StrokeKind::Inside);
    let line = Stroke::new(1.0_f32, palette.hairline);
    let font = FontId::proportional(LABEL_SIZE);
    let mut value = 0.0;
    while value <= 100.0 {
        painter.line_segment([Pos2::new(plot.area.left(), plot.y(value)), Pos2::new(plot.area.right(), plot.y(value))], line);
        painter.line_segment([Pos2::new(plot.x(value), plot.area.top()), Pos2::new(plot.x(value), plot.area.bottom())], line);
        painter.text(Pos2::new(outer.left() + PAD_LEFT - 6.0, plot.y(value)), Align2::RIGHT_CENTER, format!("{value:.0}%"), font.clone(), palette.muted);
        painter.text(Pos2::new(plot.x(value), plot.area.bottom() + 6.0), Align2::CENTER_TOP, format!("{value:.0}°"), font.clone(), palette.muted);
        value += GRID_STEP;
    }
}

fn draw_curve(ui: &Ui, plot: &Plot, points: &[Point]) {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
    let mut path = vec![Pos2::new(plot.x(curve::MIN_TEMP), plot.y(first.1 as f32))];
    path.extend(points.iter().map(|&point| plot.at(point)));
    path.push(Pos2::new(plot.x(curve::MAX_TEMP), plot.y(last.1 as f32)));
    let painter = ui.painter();
    painter.add(Shape::line(path, Stroke::new(2.5_f32, ACCENT)));
    let rim = Stroke::new(2.0_f32, colors().text);
    for &point in points {
        painter.circle(plot.at(point), POINT_RADIUS, ACCENT, rim);
    }
}

/// A line at the current temperature, and a dot where the fan is on the curve.
fn draw_live(ui: &Ui, plot: &Plot, points: &[Point], live: &Live) {
    let Some(temp) = live.temp else { return };
    let painter = ui.painter();
    let color = colors().ok;
    let x = plot.x(temp);
    painter.line_segment([Pos2::new(x, plot.area.top()), Pos2::new(x, plot.area.bottom())], Stroke::new(1.5_f32, color.gamma_multiply(0.7)));
    let duty = live.duty.or_else(|| curve_duty(points, temp)).unwrap_or(0);
    painter.circle_filled(Pos2::new(x, plot.y(duty as f32)), POINT_RADIUS - 1.0, color);
    let on_left_half = temp < curve::MAX_TEMP / 2.0;
    let (anchor, dx) = if on_left_half { (Align2::LEFT_TOP, 6.0) } else { (Align2::RIGHT_TOP, -6.0) };
    painter.text(Pos2::new(x + dx, plot.area.top() + 4.0), anchor, format!("{temp:.0} °C"), FontId::proportional(LABEL_SIZE), color);
}
