//! The curve drawn as a graph, with points to drag and the live temperature. It is drawn at a
//! fixed size on a Canvas inside a Viewbox, which scales it to the card; pointer positions arrive
//! in the Canvas's own units, so the mapping below works at any window size.

use super::curve::{self, Point};
use coolwave_core::engine::curve_duty;
use coolwave_ui::nothing;
use windows_reactor::*;

/// The graph's own size; the Viewbox scales it.
pub const WIDTH: f64 = 600.0;
pub const HEIGHT: f64 = 250.0;
/// Room around the plot for the axis labels.
const PAD_LEFT: f64 = 44.0;
const PAD_RIGHT: f64 = 16.0;
const PAD_TOP: f64 = 14.0;
const PAD_BOTTOM: f64 = 26.0;
const GRID_STEP: f64 = 25.0;
const POINT_RADIUS: f64 = 7.0;
/// Grabbing a point works from a bit further than it is drawn.
const GRAB_RADIUS: f64 = 16.0;
const LABEL_SIZE: f64 = 11.0;

/// The temperature the curve follows and the duty the fan is at now.
pub struct Live {
    pub temp: Option<f32>,
    pub duty: Option<u8>,
}

/// Where the plot sits inside the graph, and how values map onto it.
struct Plot;

impl Plot {
    const LEFT: f64 = PAD_LEFT;
    const TOP: f64 = PAD_TOP;
    const RIGHT: f64 = WIDTH - PAD_RIGHT;
    const BOTTOM: f64 = HEIGHT - PAD_BOTTOM;

    fn width() -> f64 {
        Self::RIGHT - Self::LEFT
    }

    fn height() -> f64 {
        Self::BOTTOM - Self::TOP
    }

    fn x(temp: f64) -> f64 {
        Self::LEFT + temp.clamp(curve::MIN_TEMP as f64, curve::MAX_TEMP as f64) / curve::MAX_TEMP as f64 * Self::width()
    }

    fn y(duty: f64) -> f64 {
        Self::BOTTOM - duty.clamp(0.0, 100.0) / 100.0 * Self::height()
    }

    fn at(point: Point) -> (f64, f64) {
        (Self::x(point.0 as f64), Self::y(point.1 as f64))
    }
}

/// The temperature and duty at a spot on the graph.
pub fn value_at(x: f64, y: f64) -> (f32, f32) {
    let temp = (x - Plot::LEFT) / Plot::width() * curve::MAX_TEMP as f64;
    let duty = (Plot::BOTTOM - y) / Plot::height() * 100.0;
    (temp as f32, duty as f32)
}

/// The point under a spot on the graph, if one is close enough to grab.
pub fn point_at(points: &[Point], x: f64, y: f64) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let (px, py) = Plot::at(*point);
            (index, (px - x).hypot(py - y))
        })
        .filter(|(_, distance)| *distance <= GRAB_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

fn text(value: String, left: f64, top: f64, brush: impl Into<Brush>) -> View {
    TextBlock::new().text(value).font_size(LABEL_SIZE).foreground(brush).canvas_left(left).canvas_top(top).into()
}

fn segment(from: (f64, f64), to: (f64, f64), brush: impl Into<Brush>, thickness: f64) -> View {
    Line::new().x1(from.0).y1(from.1).x2(to.0).y2(to.1).stroke(brush).stroke_thickness(thickness).into()
}

/// The graph. `dragging` is the point being moved, which gets its values written beside it.
pub fn view(points: &[Point], live: &Live, dragging: Option<usize>, pressed: Callback<PointerEventInfo>, moved: Callback<PointerEventInfo>, released: Callback<PointerEventInfo>) -> View {
    let mut shapes: Vec<View> = vec![Rectangle::new()
        .width(Plot::width())
        .height(Plot::height())
        .radius_x(6.0)
        .radius_y(6.0)
        .fill(ThemeBrush::SubtleSecondary)
        .stroke(ThemeBrush::CardStroke)
        .stroke_thickness(1.0)
        .canvas_left(Plot::LEFT)
        .canvas_top(Plot::TOP)
        .into()];
    let mut value = 0.0;
    while value <= 100.0 {
        shapes.push(segment((Plot::LEFT, Plot::y(value)), (Plot::RIGHT, Plot::y(value)), ThemeBrush::CardStroke, 1.0));
        shapes.push(segment((Plot::x(value), Plot::TOP), (Plot::x(value), Plot::BOTTOM), ThemeBrush::CardStroke, 1.0));
        shapes.push(text(format!("{value:.0}%"), 4.0, Plot::y(value) - 8.0, ThemeBrush::SecondaryText));
        shapes.push(text(format!("{value:.0}°"), Plot::x(value) - 9.0, Plot::BOTTOM + 6.0, ThemeBrush::SecondaryText));
        value += GRID_STEP;
    }
    if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
        // Flat before the first point and after the last, as the fans run.
        let mut path = vec![(Plot::x(curve::MIN_TEMP as f64), Plot::y(first.1 as f64))];
        path.extend(points.iter().map(|point| Plot::at(*point)));
        path.push((Plot::x(curve::MAX_TEMP as f64), Plot::y(last.1 as f64)));
        for pair in path.windows(2) {
            shapes.push(segment(pair[0], pair[1], ThemeBrush::Accent, 2.5));
        }
    }
    if let Some(temp) = live.temp {
        let x = Plot::x(temp as f64);
        shapes.push(segment((x, Plot::TOP), (x, Plot::BOTTOM), ThemeBrush::SystemSuccess, 1.5));
        let duty = live.duty.or_else(|| curve_duty(points, temp)).unwrap_or(0);
        let r = POINT_RADIUS - 1.5;
        shapes.push(Ellipse::new().width(r * 2.0).height(r * 2.0).fill(ThemeBrush::SystemSuccess).canvas_left(x - r).canvas_top(Plot::y(duty as f64) - r).into());
        let on_left_half = temp < curve::MAX_TEMP / 2.0;
        let left = if on_left_half { x + 6.0 } else { x - 44.0 };
        shapes.push(text(format!("{temp:.0} °C"), left, Plot::TOP + 4.0, ThemeBrush::SystemSuccess));
    }
    for (index, point) in points.iter().enumerate() {
        let (x, y) = Plot::at(*point);
        let r = if dragging == Some(index) { POINT_RADIUS + 1.5 } else { POINT_RADIUS };
        shapes.push(
            Ellipse::new()
                .width(r * 2.0)
                .height(r * 2.0)
                .fill(ThemeBrush::Accent)
                .stroke(ThemeBrush::PrimaryText)
                .stroke_thickness(2.0)
                .canvas_left(x - r)
                .canvas_top(y - r)
                .into(),
        );
    }
    let tag: View = match dragging.and_then(|index| points.get(index)) {
        Some(&(temp, duty)) => {
            let (x, y) = Plot::at((temp, duty));
            let left = if x > WIDTH - 120.0 { x - 112.0 } else { x + 12.0 };
            Border::new()
                .background(ThemeBrush::SolidBackground)
                .border_brush(ThemeBrush::CardStroke)
                .border_thickness(1.0)
                .corner_radius(4.0)
                .padding(Thickness::new(6.0, 2.0, 6.0, 3.0))
                .canvas_left(left)
                .canvas_top((y - 30.0).max(0.0))
                .content(TextBlock::new().text(format!("{temp:.0} °C → {duty} %")).font_size(LABEL_SIZE))
                .into()
        }
        None => nothing(),
    };
    shapes.push(tag);
    Viewbox::new()
        .stretch(Stretch::Uniform)
        .max_height(HEIGHT * 1.4)
        .content(
            Border::new()
                .width(WIDTH)
                .height(HEIGHT)
                // A fill, so the whole graph takes the pointer, not only its lines.
                .background(Brush::Solid(Color::transparent()))
                .capture_pointer_on_press(true)
                .automation_name("Fan curve. Drag a point to change it, or set the numbers below.")
                .on_pointer_pressed(pressed)
                .on_pointer_moved(moved)
                .on_pointer_released(released)
                .content(Canvas::new().width(WIDTH).height(HEIGHT).children(shapes)),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_found_where_it_is_drawn() {
        let points = curve::DEFAULT.to_vec();
        let (x, y) = Plot::at(points[2]);
        assert_eq!(point_at(&points, x + 3.0, y - 3.0), Some(2));
        assert_eq!(point_at(&points, x + 40.0, y), None);
    }

    #[test]
    fn values_map_back_from_the_plot() {
        let (x, y) = Plot::at((60.0, 55));
        let (temp, duty) = value_at(x, y);
        assert!((temp - 60.0).abs() < 0.01 && (duty - 55.0).abs() < 0.01);
    }
}
