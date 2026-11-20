//! The widget editor's rules, without a window: what a press lands on, how a drag moves or
//! resizes a widget and snaps it to lines, and the small edits (nudge, duplicate, reorder).
//! Positions are in screen pixels, 0 to 720, whatever size the editor draws the screen at.

use crate::layout::{self, snap_axis, stops, Layout, Style, Widget, SCREEN};

/// A drag in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Moving a widget. `grab` is where the pointer holds it, from its top left corner.
    Move { grab_x: f32, grab_y: f32 },
    /// Pulling its bottom right corner.
    Resize,
}

/// A line a dragged widget lined up with: vertical (at an x) or horizontal (at a y).
pub type Guide = (bool, f32);

/// The topmost widget under a point.
pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<usize> {
    layout.widgets.iter().rposition(|widget| widget.contains(x, y))
}

/// Whether a point is on the selected widget's resize handle, `radius` screen pixels around its
/// bottom right corner.
pub fn on_handle(layout: &Layout, selected: Option<usize>, x: f32, y: f32, radius: f32) -> bool {
    selected.and_then(|index| layout.widgets.get(index)).is_some_and(|widget| {
        (x - (widget.x + widget.w)).abs() <= radius && (y - (widget.y + widget.h)).abs() <= radius
    })
}

/// What a press at a point selects and starts: the selected widget's handle resizes it, a widget
/// is picked up, and empty space clears the selection.
pub fn press(
    layout: &Layout,
    selected: Option<usize>,
    x: f32,
    y: f32,
    handle_radius: f32,
) -> (Option<usize>, Option<Drag>) {
    if on_handle(layout, selected, x, y, handle_radius) {
        return (selected, Some(Drag::Resize));
    }
    match hit(layout, x, y) {
        Some(index) => {
            let widget = &layout.widgets[index];
            (
                Some(index),
                Some(Drag::Move {
                    grab_x: x - widget.x,
                    grab_y: y - widget.y,
                }),
            )
        }
        None => (None, None),
    }
}

/// Follow the pointer at `(px, py)` with widget `index`. With `snap`, its edges and middle line
/// up with the screen's lines and other widgets'; the lines it lined up with come back.
pub fn drag(
    layout: &mut Layout,
    index: usize,
    drag: Drag,
    px: f32,
    py: f32,
    snap: bool,
) -> Vec<Guide> {
    let mut guides = Vec::new();
    if index >= layout.widgets.len() {
        return guides;
    }
    let others: Vec<Widget> = layout
        .widgets
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(_, widget)| widget.clone())
        .collect();
    let widget = &mut layout.widgets[index];
    match drag {
        Drag::Move { grab_x, grab_y } => {
            widget.x = px - grab_x;
            widget.y = py - grab_y;
            widget.keep_on_screen();
            if snap {
                let (dx, gx) = snap_axis(
                    [widget.x, widget.x + widget.w / 2.0, widget.x + widget.w],
                    &stops(&others, true),
                );
                let (dy, gy) = snap_axis(
                    [widget.y, widget.y + widget.h / 2.0, widget.y + widget.h],
                    &stops(&others, false),
                );
                widget.x += dx;
                widget.y += dy;
                widget.keep_on_screen();
                guides.extend(gx.map(|x| (true, x)));
                guides.extend(gy.map(|y| (false, y)));
            }
        }
        Drag::Resize => {
            let (mut right, mut bottom) = (px, py);
            if snap {
                let (dx, gx) = snap_axis([right], &stops(&others, true));
                let (dy, gy) = snap_axis([bottom], &stops(&others, false));
                right += dx;
                bottom += dy;
                guides.extend(gx.map(|x| (true, x)));
                guides.extend(gy.map(|y| (false, y)));
            }
            let right = right.min(SCREEN);
            let bottom = bottom.min(SCREEN);
            widget.w = (right - widget.x).max(layout::MIN_SIZE);
            widget.h = (bottom - widget.y).max(layout::MIN_SIZE);
            if widget.style == Style::Ring {
                let side = widget
                    .w
                    .max(widget.h)
                    .min(SCREEN - widget.x)
                    .min(SCREEN - widget.y);
                widget.w = side;
                widget.h = side;
            }
            widget.keep_on_screen();
        }
    }
    guides
}

/// Move a widget by whole pixels, from the arrow keys.
pub fn nudge(layout: &mut Layout, index: usize, dx: f32, dy: f32) {
    if let Some(widget) = layout.widgets.get_mut(index) {
        widget.x += dx;
        widget.y += dy;
        widget.keep_on_screen();
    }
}

/// A copy of a widget a little down and to the right, on top. Its index, unless the screen is full.
pub fn duplicate(layout: &mut Layout, index: usize) -> Option<usize> {
    if layout.widgets.len() >= layout::MAX_WIDGETS {
        return None;
    }
    let mut copy = layout.widgets.get(index)?.clone();
    copy.x += 24.0;
    copy.y += 24.0;
    copy.keep_on_screen();
    layout.widgets.push(copy);
    Some(layout.widgets.len() - 1)
}

/// Draw a widget over all the others. Its new index.
pub fn bring_to_front(layout: &mut Layout, index: usize) -> Option<usize> {
    if index >= layout.widgets.len() {
        return None;
    }
    let widget = layout.widgets.remove(index);
    layout.widgets.push(widget);
    Some(layout.widgets.len() - 1)
}

/// Draw a widget under all the others. Its new index.
pub fn send_to_back(layout: &mut Layout, index: usize) -> Option<usize> {
    if index >= layout.widgets.len() {
        return None;
    }
    let widget = layout.widgets.remove(index);
    layout.widgets.insert(0, widget);
    Some(0)
}

/// Resize from the inspector's sliders, keeping the widget's middle where it was. A gauge stays
/// round, so its one size is both.
pub fn set_width(widget: &mut Widget, w: f32) {
    let ring = widget.style == Style::Ring;
    let (cx, cy) = widget.center();
    widget.w = w;
    if ring {
        widget.h = w;
    }
    widget.x = cx - widget.w / 2.0;
    if ring {
        widget.y = cy - widget.h / 2.0;
    }
    widget.keep_on_screen();
}

pub fn set_height(widget: &mut Widget, h: f32) {
    let (_, cy) = widget.center();
    widget.h = h;
    widget.y = cy - h / 2.0;
    widget.keep_on_screen();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Metric;

    const HANDLE: f32 = 8.0;

    fn one_widget() -> Layout {
        let mut widget = Widget::new(Metric::CpuUsage, Style::Text, 0.0, 0.0);
        (widget.x, widget.y, widget.w, widget.h) = (100.0, 100.0, 200.0, 100.0);
        Layout {
            widgets: vec![widget],
            ..Layout::default()
        }
    }

    /// Press at `from`, move to `to` in steps, as a pointer would.
    fn drag_from_to(
        layout: &mut Layout,
        selected: Option<usize>,
        from: (f32, f32),
        to: (f32, f32),
        snap: bool,
    ) -> Option<usize> {
        let (selected, started) = press(layout, selected, from.0, from.1, HANDLE);
        if let (Some(index), Some(started)) = (selected, started) {
            for step in 1..=5 {
                let t = step as f32 / 5.0;
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                drag(layout, index, started, x, y, snap);
            }
        }
        selected
    }

    #[test]
    fn dragging_a_widget_moves_it_with_the_pointer() {
        let mut layout = one_widget();
        let selected = drag_from_to(&mut layout, None, (150.0, 130.0), (263.0, 207.0), false);
        let widget = &layout.widgets[0];
        assert_eq!((widget.x, widget.y), (213.0, 177.0));
        assert_eq!(selected, Some(0));
    }

    #[test]
    fn dragging_the_corner_resizes() {
        let mut layout = one_widget();
        // Select it first, then pull the dot at its bottom right.
        let (selected, _) = press(&layout, None, 150.0, 130.0, HANDLE);
        assert_eq!(selected, Some(0));
        drag_from_to(&mut layout, selected, (300.0, 200.0), (401.0, 251.0), false);
        let widget = &layout.widgets[0];
        assert_eq!((widget.x, widget.y), (100.0, 100.0));
        assert_eq!((widget.w, widget.h), (301.0, 151.0));
    }

    #[test]
    fn a_drag_snaps_to_the_middle_of_the_screen() {
        let mut layout = one_widget();
        // The widget's center lands 4 points left of the middle line.
        drag_from_to(&mut layout, None, (200.0, 150.0), (356.0, 150.0), true);
        let widget = &layout.widgets[0];
        assert_eq!(widget.x + widget.w / 2.0, SCREEN / 2.0);
    }

    #[test]
    fn clicking_empty_space_clears_the_selection() {
        let layout = one_widget();
        let (selected, started) = press(&layout, Some(0), 600.0, 600.0, HANDLE);
        assert_eq!((selected, started), (None, None));
    }

    #[test]
    fn a_snapped_drag_reports_its_guide() {
        let mut layout = one_widget();
        let (_, started) = press(&layout, None, 200.0, 150.0, HANDLE);
        let guides = drag(&mut layout, 0, started.unwrap(), 356.0, 150.0, true);
        assert!(guides.contains(&(true, SCREEN / 2.0)));
    }

    #[test]
    fn duplicates_stop_when_the_screen_is_full() {
        let mut layout = one_widget();
        assert_eq!(duplicate(&mut layout, 0), Some(1));
        assert_eq!((layout.widgets[1].x, layout.widgets[1].y), (124.0, 124.0));
        while layout.widgets.len() < layout::MAX_WIDGETS {
            duplicate(&mut layout, 0);
        }
        assert_eq!(duplicate(&mut layout, 0), None);
    }

    #[test]
    fn nudging_keeps_the_widget_on_screen() {
        let mut layout = one_widget();
        nudge(&mut layout, 0, -500.0, 10.0);
        assert_eq!((layout.widgets[0].x, layout.widgets[0].y), (0.0, 110.0));
    }

    #[test]
    fn reordering_moves_the_widget_to_either_end() {
        let mut layout = one_widget();
        duplicate(&mut layout, 0);
        let first = layout.widgets[0].clone();
        assert_eq!(bring_to_front(&mut layout, 0), Some(1));
        assert_eq!(layout.widgets[1], first);
        assert_eq!(send_to_back(&mut layout, 1), Some(0));
        assert_eq!(layout.widgets[0], first);
    }
}
