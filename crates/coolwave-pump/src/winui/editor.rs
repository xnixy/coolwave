//! The Widgets tab: a live preview of the pump screen where readings are dragged into place,
//! resized from a corner and styled from the side panel, with undo and saving.
//!
//! The screen is drawn at the pump's own 720 points inside a Viewbox, so pointer positions and
//! shapes on it are in screen pixels. The picture under the widgets is the same drawing the pump
//! gets; what moves while you drag (outlines, the handle, guides) is drawn with WinUI's shapes.

use super::parts::{self, danger, label, labeled, switch_line};
use super::{metric_label, metrics_of, missing_reason, Canvas, Message as PageMessage, PumpCore, PumpPage};
use crate::edit::{self, Drag, Guide};
use crate::engine::Controller;
use crate::layout::{self, Backdrop, Layout, Style, Widget, SCREEN};
use crate::settings::{Settings, Source};
use crate::stats::{self, Metric};
use coolwave_ui::*;
use std::rc::Rc;
use std::time::Duration;
use windows_reactor::*;

/// While a widget is dragged, the screen is drawn again at most this often.
pub const DRAG_FRAME: Duration = Duration::from_millis(60);
/// How close to the selected widget's corner, in screen pixels, grabs the resize handle.
const HANDLE: f32 = 22.0;
const UNDO_LIMIT: usize = 60;
/// The widest the editor's screen grows, and the column beside it.
const SCREEN_MAX: f64 = 640.0;
const SIDE_COLUMN: f64 = 340.0;
/// The two layouts, as the switch at the top lists them.
const CANVASES: [(Canvas, &str); 2] = [(Canvas::Picture, "On your picture"), (Canvas::Dashboard, "Dashboard")];

/// What the pointer is over while nothing is dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    Nothing,
    Widget(usize),
    Handle,
}

pub struct Editor {
    pub canvas: Canvas,
    pub selected: Option<usize>,
    pub snap: bool,
    drag: Option<Drag>,
    guides: Vec<Guide>,
    hover: Hover,
    /// The layout as it was when the current drag or slider gesture began.
    gesture: Option<(Canvas, Layout)>,
    /// Layouts before each change, newest last.
    undo: Vec<(Canvas, Layout)>,
}

#[derive(Clone)]
pub enum Message {
    Canvas(Option<usize>),
    Pressed { x: f32, y: f32, right: bool },
    Moved { x: f32, y: f32, alt: bool },
    Released,
    Hover(Hover),
    Key { key: u32, shift: bool, ctrl: bool },
    Menu(String),
    Snap(bool),
    Undo,
    Arrange,
    RemoveAll,
    Style(Option<usize>),
    Metric(Option<usize>),
    Width(f64),
    Height(f64),
    Color([u8; 3]),
    Caption(bool),
    Backdrop(bool),
    Center,
    CenterAcross,
    Duplicate,
    Front,
    Back,
    Remove,
    Background(Option<usize>),
    BackgroundColor([u8; 3]),
    FollowEffect(bool),
    Add(Metric),
    TurnOnPictureWidgets,
    ShowDashboard,
    /// A slider gesture rested.
    Settle,
}

/// What an update did, so the page knows when to draw and save.
#[derive(Default)]
pub struct Outcome {
    /// The layout changed: draw the screen again.
    pub redraw: bool,
    /// The change is finished (a drag let go): draw now, it was saved.
    pub settled: bool,
    /// A slider moved: settle once it rests.
    pub settle_later: bool,
}

impl Outcome {
    fn changed(changed: bool) -> Self {
        Self { redraw: changed, ..Self::default() }
    }

    fn done() -> Self {
        Self { redraw: true, settled: true, settle_later: false }
    }
}

fn layout_mut(settings: &mut Settings, canvas: Canvas) -> &mut Layout {
    match canvas {
        Canvas::Dashboard => settings.dashboard_layout.get_or_insert_with(Layout::default),
        Canvas::Picture => settings.picture_layout.get_or_insert_with(Layout::default),
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            canvas: Canvas::default(),
            selected: None,
            snap: true,
            drag: None,
            guides: Vec::new(),
            hover: Hover::Nothing,
            gesture: None,
            undo: Vec::new(),
        }
    }

    pub fn switch_canvas(&mut self, canvas: Canvas) {
        self.canvas = canvas;
        self.selected = None;
        self.drag = None;
        self.guides.clear();
        self.hover = Hover::Nothing;
    }

    /// Change the layout. A change is one step to undo, however many updates its gesture takes;
    /// `settled` ends the gesture and saves.
    fn edit(&mut self, core: &mut Controller, settled: bool, change: impl FnOnce(&mut Layout, &mut Self) -> bool) -> Outcome {
        let canvas = self.canvas;
        let layout = layout_mut(&mut core.settings, canvas);
        let before = layout.clone();
        let changed = change(layout, self);
        if changed && self.gesture.is_none() {
            self.gesture = Some((canvas, before));
        }
        if settled {
            self.settle(core);
            return Outcome::done();
        }
        Outcome::changed(changed)
    }

    /// A finished change: one step to undo, and saved.
    fn settle(&mut self, core: &mut Controller) {
        if let Some((canvas, start)) = self.gesture.take() {
            if Some(&start) != Some(&*layout_mut(&mut core.settings, canvas)) {
                self.undo.push((canvas, start));
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
            }
        }
        core.persist();
    }

    /// Edit the selected widget, finished at once.
    fn edit_selected(&mut self, core: &mut Controller, change: impl FnOnce(&mut Widget)) -> Outcome {
        let Some(index) = self.selected else {
            return Outcome::default();
        };
        self.edit(core, true, |layout, _| {
            let Some(widget) = layout.widgets.get_mut(index) else {
                return false;
            };
            let before = widget.clone();
            change(widget);
            *widget != before
        })
    }

    fn can_undo(&self) -> bool {
        self.undo.iter().any(|(on, _)| *on == self.canvas)
    }

    fn undo_last(&mut self, core: &mut Controller) -> Outcome {
        if self.gesture.is_some() {
            return Outcome::default();
        }
        let Some(index) = self.undo.iter().rposition(|(on, _)| *on == self.canvas) else {
            return Outcome::default();
        };
        let (_, previous) = self.undo.remove(index);
        *layout_mut(&mut core.settings, self.canvas) = previous;
        self.selected = None;
        core.persist();
        Outcome::done()
    }

    pub fn update(&mut self, message: Message, core: &mut Controller) -> Outcome {
        let canvas = self.canvas;
        if self.selected.is_some_and(|index| index >= layout_mut(&mut core.settings, canvas).widgets.len()) {
            self.selected = None;
        }
        match message {
            Message::Canvas(Some(index)) => {
                let wanted = CANVASES[index.min(1)].0;
                if wanted != self.canvas {
                    self.switch_canvas(wanted);
                }
                Outcome::changed(true)
            }
            Message::Canvas(None) | Message::Style(None) | Message::Metric(None) | Message::Background(None) => {
                Outcome::default()
            }
            Message::Pressed { x, y, right } => {
                let layout = layout_mut(&mut core.settings, canvas);
                if right {
                    // The right-click menu acts on the widget under the pointer.
                    self.selected = edit::hit(layout, x, y);
                } else {
                    let (selected, drag) = edit::press(layout, self.selected, x, y, HANDLE);
                    self.selected = selected;
                    self.drag = drag;
                }
                Outcome::default()
            }
            Message::Moved { x, y, alt } => {
                let (Some(drag), Some(index)) = (self.drag, self.selected) else {
                    return Outcome::default();
                };
                let snap = self.snap && !alt;
                self.edit(core, false, |layout, editor| {
                    let before = layout.widgets.get(index).cloned();
                    editor.guides = edit::drag(layout, index, drag, x, y, snap);
                    layout.widgets.get(index) != before.as_ref()
                })
            }
            Message::Released => {
                self.guides.clear();
                if self.drag.take().is_some() {
                    self.settle(core);
                    Outcome::done()
                } else {
                    Outcome::default()
                }
            }
            Message::Hover(hover) => {
                self.hover = hover;
                Outcome::default()
            }
            Message::Key { key, shift, ctrl } => self.key(core, key, shift, ctrl),
            Message::Menu(key) => match key.as_str() {
                "duplicate" => self.update(Message::Duplicate, core),
                "front" => self.update(Message::Front, core),
                "back" => self.update(Message::Back, core),
                "remove" => self.update(Message::Remove, core),
                _ => Outcome::default(),
            },
            Message::Snap(on) => {
                self.snap = on;
                Outcome::default()
            }
            Message::Undo => self.undo_last(core),
            Message::Arrange => self.edit(core, true, |layout, editor| {
                let metrics = metrics_of(&layout.widgets);
                let background = layout.background;
                *layout = match canvas {
                    Canvas::Dashboard => layout::arrange_dashboard(&metrics, background),
                    Canvas::Picture => layout::arrange_band(&metrics, false),
                };
                editor.selected = None;
                true
            }),
            Message::RemoveAll => self.edit(core, true, |layout, editor| {
                editor.selected = None;
                let any = !layout.widgets.is_empty();
                layout.widgets.clear();
                any
            }),
            Message::Style(Some(index)) => {
                let style = Style::ALL[index.min(Style::ALL.len() - 1)];
                self.edit_selected(core, |widget| {
                    if widget.style != style {
                        widget.set_style(style);
                    }
                })
            }
            Message::Metric(Some(index)) => {
                let Some(metric) = stats::ALL.get(index).copied() else {
                    return Outcome::default();
                };
                self.edit_selected(core, |widget| widget.metric = metric)
            }
            Message::Width(value) => {
                let Some(index) = self.selected else {
                    return Outcome::default();
                };
                let w = value.round().clamp(layout::MIN_SIZE as f64, SCREEN as f64) as f32;
                let mut outcome = self.edit(core, false, |layout, _| {
                    let Some(widget) = layout.widgets.get_mut(index) else { return false };
                    if widget.w == w {
                        return false;
                    }
                    edit::set_width(widget, w);
                    true
                });
                outcome.settle_later = outcome.redraw;
                outcome
            }
            Message::Height(value) => {
                let Some(index) = self.selected else {
                    return Outcome::default();
                };
                let h = value.round().clamp(layout::MIN_SIZE as f64, SCREEN as f64) as f32;
                let mut outcome = self.edit(core, false, |layout, _| {
                    let Some(widget) = layout.widgets.get_mut(index) else { return false };
                    if widget.h == h {
                        return false;
                    }
                    edit::set_height(widget, h);
                    true
                });
                outcome.settle_later = outcome.redraw;
                outcome
            }
            Message::Settle => {
                if self.gesture.is_some() && self.drag.is_none() {
                    self.settle(core);
                    Outcome::done()
                } else {
                    Outcome::default()
                }
            }
            Message::Color(color) => self.edit_selected(core, |widget| widget.color = color),
            Message::Caption(on) => self.edit_selected(core, |widget| widget.caption = on),
            Message::Backdrop(on) => self.edit_selected(core, |widget| widget.backdrop = on),
            Message::Center => self.edit_selected(core, |widget| {
                widget.x = (SCREEN - widget.w) / 2.0;
                widget.y = (SCREEN - widget.h) / 2.0;
            }),
            Message::CenterAcross => self.edit_selected(core, |widget| widget.x = (SCREEN - widget.w) / 2.0),
            Message::Duplicate => self.reorder(core, edit::duplicate),
            Message::Front => self.reorder(core, edit::bring_to_front),
            Message::Back => self.reorder(core, edit::send_to_back),
            Message::Remove => {
                let Some(index) = self.selected else {
                    return Outcome::default();
                };
                self.edit(core, true, |layout, editor| {
                    editor.selected = None;
                    if index < layout.widgets.len() {
                        layout.widgets.remove(index);
                        true
                    } else {
                        false
                    }
                })
            }
            Message::Background(Some(index)) => {
                let style = Backdrop::ALL[index.min(Backdrop::ALL.len() - 1)];
                self.edit(core, true, |layout, _| {
                    let changed = layout.background.style != style;
                    layout.background.style = style;
                    changed
                })
            }
            Message::BackgroundColor(color) => self.edit(core, true, |layout, _| {
                let changed = layout.background.color != color;
                layout.background.color = color;
                changed
            }),
            Message::FollowEffect(on) => self.edit(core, true, |layout, _| {
                let changed = layout.background.follow_effect != on;
                layout.background.follow_effect = on;
                changed
            }),
            Message::Add(metric) => {
                let readings = core.stats.readings();
                let available = layout::is_clock(metric) || readings.as_ref().is_none_or(|readings| readings.has(metric));
                self.edit(core, true, |layout, editor| {
                    if !available || layout.widgets.len() >= layout::MAX_WIDGETS {
                        return false;
                    }
                    let (cx, cy) = layout.next_spot();
                    let style = match (canvas, layout::is_clock(metric)) {
                        (_, true) => Style::Text,
                        (Canvas::Dashboard, false) => Style::Tile,
                        (Canvas::Picture, false) => Style::Text,
                    };
                    let mut widget = Widget::new(metric, style, cx, cy);
                    widget.backdrop = canvas == Canvas::Picture;
                    layout.widgets.push(widget);
                    editor.selected = Some(layout.widgets.len() - 1);
                    true
                })
            }
            Message::TurnOnPictureWidgets => {
                if !core.settings.stats_on_picture {
                    core.settings.stats_on_picture = true;
                    core.persist();
                }
                Outcome::default()
            }
            Message::ShowDashboard => {
                if !core.busy {
                    core.show_dashboard();
                }
                Outcome::default()
            }
        }
    }

    /// Duplicate or reorder the selected widget, which stays selected where it went.
    fn reorder(&mut self, core: &mut Controller, change: fn(&mut Layout, usize) -> Option<usize>) -> Outcome {
        let Some(index) = self.selected else {
            return Outcome::default();
        };
        self.edit(core, true, |layout, editor| match change(layout, index) {
            Some(moved) => {
                editor.selected = Some(moved);
                true
            }
            None => false,
        })
    }

    /// Arrow keys nudge (Shift further), Delete removes, Ctrl+D duplicates, Ctrl+Z undoes.
    fn key(&mut self, core: &mut Controller, key: u32, shift: bool, ctrl: bool) -> Outcome {
        if ctrl && key == VirtualKey::Z.0 {
            return self.undo_last(core);
        }
        let Some(index) = self.selected else {
            return Outcome::default();
        };
        if ctrl && key == VirtualKey::D.0 {
            return self.reorder(core, edit::duplicate);
        }
        if key == VirtualKey::DELETE.0 || key == VirtualKey::BACK.0 {
            return self.update(Message::Remove, core);
        }
        let step = if shift { 10.0 } else { 1.0 };
        let (dx, dy) = match key {
            k if k == VirtualKey::LEFT.0 => (-step, 0.0),
            k if k == VirtualKey::RIGHT.0 => (step, 0.0),
            k if k == VirtualKey::UP.0 => (0.0, -step),
            k if k == VirtualKey::DOWN.0 => (0.0, step),
            _ => return Outcome::default(),
        };
        self.edit(core, true, |layout, _| {
            let before = layout.widgets.get(index).cloned();
            edit::nudge(layout, index, dx, dy);
            layout.widgets.get(index) != before.as_ref()
        })
    }
}

/// The keys the screen takes for itself while it has focus.
fn editor_key(key: u32, ctrl: bool, selected: bool) -> bool {
    let arrows = [VirtualKey::LEFT.0, VirtualKey::RIGHT.0, VirtualKey::UP.0, VirtualKey::DOWN.0];
    (ctrl && key == VirtualKey::Z.0)
        || (selected && ((ctrl && key == VirtualKey::D.0) || key == VirtualKey::DELETE.0 || key == VirtualKey::BACK.0 || arrows.contains(&key)))
}

fn send(forward: &Callback<PageMessage>, message: Message) {
    forward.call(PageMessage::Editor(message));
}

impl PumpPage {
    pub(super) fn widgets_tab(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let canvas = self.editor.canvas;
        let layout = Self::layout_of(&core.settings, canvas);
        let readings = core.stats.readings();
        let picker = line(
            12.0,
            (
                strong("Editing").vertical_alignment(VerticalAlignment::Center),
                RadioButtons::new()
                    .automation_name("Layout to edit")
                    .items_source(CANVASES.iter().map(|(_, name)| *name))
                    .width(320.0)
                    .max_columns(2)
                    .selected_index(CANVASES.iter().position(|(on, _)| *on == canvas))
                    .on_selection_changed(context.callback(|index| PageMessage::Editor(Message::Canvas(index)))),
            ),
        );
        let screen_card = padded_card(
            StackPanel::new().spacing(14.0).children((self.editor_screen(pump, &layout, context), self.toolbar(&layout, context))),
        )
        .max_width(SCREEN_MAX + 40.0)
        .horizontal_alignment(HorizontalAlignment::Left);
        let mut side: Vec<View> = vec![self.inspector(&layout, readings.as_ref(), context)];
        if canvas == Canvas::Dashboard {
            side.push(self.background_card(&layout, context));
        }
        side.push(self.library(&layout, readings.as_ref(), context));
        let mut parts: Vec<View> = vec![picker.into()];
        if let Some(banner) = self.canvas_banner(core, context) {
            parts.push(banner);
        }
        parts.push(
            Grid::new()
                .columns([GridLength::STAR, GridLength::Pixel(SIDE_COLUMN)])
                .column_spacing(12.0)
                .children((
                    Border::new().vertical_alignment(VerticalAlignment::Top).content(screen_card),
                    StackPanel::new().grid_column(1).spacing(12.0).children(side),
                ))
                .into(),
        );
        StackPanel::new().spacing(12.0).children(parts).into()
    }

    /// Says when the screen being edited is not the one the pump shows.
    fn canvas_banner(&self, core: &Controller, context: &ViewContext<Self>) -> Option<View> {
        let source = core.settings.source;
        Some(match self.editor.canvas {
            Canvas::Picture if !core.settings.stats_on_picture => parts::banner_with(
                InfoBarSeverity::Warning,
                "Widgets on your picture are off.",
                Some(
                    Button::new()
                        .content("Turn them on")
                        .on_click(context.message(PageMessage::Editor(Message::TurnOnPictureWidgets)))
                        .into(),
                ),
            ),
            Canvas::Picture if source != Source::Picture => {
                parts::banner_with(InfoBarSeverity::Informational, "The pump is not showing your picture right now.", None)
            }
            Canvas::Dashboard if source != Source::Dashboard => parts::banner_with(
                InfoBarSeverity::Informational,
                "The pump is not showing the dashboard right now.",
                Some(
                    Button::new()
                        .content("Show it")
                        .is_enabled(!core.busy)
                        .on_click(context.message(PageMessage::Editor(Message::ShowDashboard)))
                        .into(),
                ),
            ),
            _ => return None,
        })
    }

    /// The editable screen. Click to select, drag to move, drag the corner dot to resize.
    fn editor_screen(&self, pump: &PumpCore, layout: &Layout, context: &ViewContext<Self>) -> View {
        let editor = &self.editor;
        let forward = context.forward();
        let boxes: Rc<Vec<[f32; 4]>> = Rc::new(layout.widgets.iter().map(|w| [w.x, w.y, w.w, w.h]).collect());
        let selected = editor.selected.filter(|index| *index < layout.widgets.len());
        let hover = editor.hover;

        let pressed = {
            let forward = forward.clone();
            Callback::new(move |info: PointerEventInfo| {
                send(&forward, Message::Pressed { x: info.x as f32, y: info.y as f32, right: info.is_right_button_pressed });
            })
        };
        let moved = {
            let forward = forward.clone();
            let boxes = boxes.clone();
            Callback::new(move |info: PointerEventInfo| {
                let (x, y) = (info.x as f32, info.y as f32);
                if info.is_left_button_pressed {
                    send(&forward, Message::Moved { x, y, alt: info.modifiers.contains(InputModifiers::ALT) });
                    return;
                }
                let on_handle = selected.and_then(|index| boxes.get(index)).is_some_and(|b| {
                    (x - (b[0] + b[2])).abs() <= HANDLE && (y - (b[1] + b[3])).abs() <= HANDLE
                });
                let now = if on_handle {
                    Hover::Handle
                } else {
                    boxes
                        .iter()
                        .rposition(|b| x >= b[0] && x <= b[0] + b[2] && y >= b[1] && y <= b[1] + b[3])
                        .map_or(Hover::Nothing, Hover::Widget)
                };
                if now != hover {
                    send(&forward, Message::Hover(now));
                }
            })
        };
        let released = {
            let forward = forward.clone();
            Callback::new(move |_: PointerEventInfo| send(&forward, Message::Released))
        };
        let exited = {
            let forward = forward.clone();
            Callback::new(move |_: PointerEventInfo| {
                if hover != Hover::Nothing {
                    send(&forward, Message::Hover(Hover::Nothing));
                }
            })
        };
        let keys = {
            let forward = forward.clone();
            let has_selection = selected.is_some();
            RoutedCallback::new(move |info: KeyEventInfo| {
                let ctrl = info.modifiers.contains(InputModifiers::CONTROL);
                if !editor_key(info.key.0, ctrl, has_selection) {
                    return false;
                }
                send(
                    &forward,
                    Message::Key { key: info.key.0, shift: info.modifiers.contains(InputModifiers::SHIFT), ctrl },
                );
                true
            })
        };
        let cursor = match (editor.drag, hover) {
            (Some(Drag::Resize), _) | (None, Hover::Handle) => InputSystemCursorShape::SizeNorthwestSoutheast,
            (Some(Drag::Move { .. }), _) | (None, Hover::Widget(_)) => InputSystemCursorShape::SizeAll,
            _ => InputSystemCursorShape::Arrow,
        };
        let menu_items = if selected.is_some() {
            vec![
                MenuItem::item("duplicate", "Duplicate"),
                MenuItem::item("front", "Bring to front"),
                MenuItem::item("back", "Send to back"),
                MenuItem::separator("s1"),
                MenuItem::item("remove", "Remove"),
            ]
        } else {
            vec![MenuItem::disabled("hint", "Right-click a widget to change it.")]
        };

        let mut layers: Vec<View> = vec![self.screen_frames(self.editor.canvas, context)];
        if self.editor.canvas == Canvas::Picture && pump.preview.is_none() {
            layers.push(
                TextBlock::new()
                    .text("Your photo or video goes here")
                    .font_size(30.0)
                    .foreground(Brush::Solid(Color::argb(110, 255, 255, 255)))
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .vertical_alignment(VerticalAlignment::Center)
                    .into(),
            );
        }
        layers.push(self.marks(layout, selected).into());

        let screen = Border::new()
            .width(SCREEN as f64)
            .height(SCREEN as f64)
            .background(Brush::Solid(Color::rgb(0, 0, 0)))
            .capture_pointer_on_press(true)
            .focus_on_pointer_release(true)
            .is_tab_stop(true)
            .pointer_cursor(cursor)
            .automation_name("Pump screen layout")
            .on_pointer_pressed(pressed)
            .on_pointer_moved(moved)
            .on_pointer_released(released)
            .on_pointer_exited(exited)
            .on_preview_key_down(keys)
            .content(Grid::new().children(layers))
            .menu(Menu::new(menu_items, context.callback(|key: Key| PageMessage::Editor(Message::Menu(key.as_str().unwrap_or_default().to_string())))));
        parts::bezel(Viewbox::new().stretch(Stretch::Uniform).content(screen)).max_width(SCREEN_MAX).into()
    }

    /// Outlines, the resize handle, guides and the position tag, in screen pixels.
    fn marks(&self, layout: &Layout, selected: Option<usize>) -> windows_reactor::Canvas {
        let mut shapes: Vec<View> = Vec::new();
        for (vertical, at) in &self.editor.guides {
            let at = *at as f64;
            let (x1, y1, x2, y2) = if *vertical { (at, 0.0, at, SCREEN as f64) } else { (0.0, at, SCREEN as f64, at) };
            shapes.push(Line::new().x1(x1).y1(y1).x2(x2).y2(y2).stroke(Brush::Solid(parts::GUIDE)).stroke_thickness(2.0).into());
        }
        if let Hover::Widget(index) = self.editor.hover {
            if Some(index) != selected && self.editor.drag.is_none() {
                if let Some(widget) = layout.widgets.get(index) {
                    shapes.push(outline(widget, 3.0, Brush::Solid(Color::argb(130, 255, 255, 255)), 2.0));
                }
            }
        }
        if let Some(widget) = selected.and_then(|index| layout.widgets.get(index)) {
            shapes.push(outline(widget, 4.0, Brush::Theme(ThemeBrush::Accent), 4.0));
            let (cx, cy) = ((widget.x + widget.w) as f64, (widget.y + widget.h) as f64);
            shapes.push(
                Ellipse::new()
                    .width(24.0)
                    .height(24.0)
                    .canvas_left(cx - 12.0)
                    .canvas_top(cy - 12.0)
                    .fill(Brush::Solid(Color::rgb(255, 255, 255)))
                    .stroke(ThemeBrush::Accent)
                    .stroke_thickness(4.0)
                    .into(),
            );
            if self.editor.drag.is_some() {
                let tag = format!("{:.0}, {:.0}   {:.0} × {:.0}", widget.x, widget.y, widget.w, widget.h);
                shapes.push(
                    Border::new()
                        .canvas_left(widget.x as f64)
                        .canvas_top((widget.y as f64 - 46.0).max(6.0))
                        .background(Brush::Solid(Color::argb(200, 0, 0, 0)))
                        .corner_radius(8.0)
                        .padding(Thickness::new(10.0, 4.0, 10.0, 5.0))
                        .content(TextBlock::new().text(tag).font_size(20.0).foreground(Brush::Solid(Color::rgb(255, 255, 255))))
                        .into(),
                );
            }
        }
        windows_reactor::Canvas::new().width(SCREEN as f64).height(SCREEN as f64).children(shapes)
    }

    /// Snap, undo, tidy up and clear, under the screen.
    fn toolbar(&self, layout: &Layout, context: &ViewContext<Self>) -> View {
        let editor_message = |message: Message| context.message(PageMessage::Editor(message));
        line(
            8.0,
            (
                line(
                    6.0,
                    (
                        switch(self.editor.snap, "Snap", context.callback(|on| PageMessage::Editor(Message::Snap(on)))),
                        muted("Snap").vertical_alignment(VerticalAlignment::Center),
                    ),
                )
                .vertical_alignment(VerticalAlignment::Center)
                .tooltip("Snap to the middle, the edges, and other widgets. Hold Alt to place freely."),
                Button::new()
                    .content("Undo")
                    .is_enabled(self.editor.can_undo())
                    .on_click(editor_message(Message::Undo))
                    .tooltip("Ctrl+Z"),
                Button::new()
                    .content("Arrange for me")
                    .on_click(editor_message(Message::Arrange))
                    .tooltip("Lay the same readings out in tidy rows."),
                Button::new()
                    .content(danger("Remove all"))
                    .is_enabled(!layout.widgets.is_empty())
                    .on_click(editor_message(Message::RemoveAll)),
            ),
        )
        .into()
    }

    /// The selected widget's style, size, color, and order.
    fn inspector(&self, layout: &Layout, readings: Option<&stats::Readings>, context: &ViewContext<Self>) -> View {
        let editor_message = |message: Message| context.message(PageMessage::Editor(message));
        let Some(widget) = self.editor.selected.and_then(|index| layout.widgets.get(index)) else {
            return padded_card(StackPanel::new().spacing(6.0).children((
                strong("Arrange the screen"),
                muted("Click a widget to select it, then drag it anywhere."),
                muted("Drag the white dot in its corner to resize it."),
                muted("Arrow keys nudge it, Shift moves it further, Delete removes it."),
                muted("It snaps to the middle, the edges, and other widgets. Hold Alt to place it freely."),
                muted("Right-click a widget for more."),
            )))
            .into();
        };
        let ring = widget.style == Style::Ring;
        let size = Range::new(layout::MIN_SIZE as f64, SCREEN as f64, 1.0);
        let value: View = match readings {
            Some(readings) => TextBlock::new()
                .text(readings.text(widget.metric))
                .font_size(17.0)
                .foreground(Brush::Solid(color_of(widget.color)))
                .vertical_alignment(VerticalAlignment::Center)
                .into(),
            None => nothing(),
        };
        let mut sizes: Vec<View> = vec![labeled(
            if ring { "Size" } else { "Width" },
            slider(
                widget.w as f64,
                size,
                format!("{:.0}", widget.w),
                if ring { "Size" } else { "Width" },
                true,
                context.callback(|value| PageMessage::Editor(Message::Width(value))),
            ),
        )];
        if !ring {
            sizes.push(labeled(
                "Height",
                slider(
                    widget.h as f64,
                    size,
                    format!("{:.0}", widget.h),
                    "Height",
                    true,
                    context.callback(|value| PageMessage::Editor(Message::Height(value))),
                ),
            ));
        }
        padded_card(
            StackPanel::new().spacing(6.0).children((
                Grid::new().columns([GridLength::STAR, GridLength::Auto]).children((
                    StackPanel::new().children((caption(widget.metric.group()), subtitle(widget.metric.name()))),
                    Border::new().grid_column(1).content(value),
                )),
                label("Style"),
                ComboBox::new()
                    .automation_name("Style")
                    .min_width(160.0)
                    .items_source(Style::ALL.iter().map(|style| style.name()))
                    .selected_index(Style::ALL.iter().position(|style| *style == widget.style))
                    .on_selection_changed(context.callback(|index| PageMessage::Editor(Message::Style(index)))),
                label("Reading"),
                ComboBox::new()
                    .automation_name("Reading")
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .items_source(stats::ALL.iter().map(|metric| metric_label(*metric)))
                    .selected_index(stats::ALL.iter().position(|metric| *metric == widget.metric))
                    .on_selection_changed(context.callback(|index| PageMessage::Editor(Message::Metric(index)))),
                label("Size"),
                StackPanel::new().spacing(4.0).children(sizes),
                label("Color"),
                swatches(widget.color, &layout::PALETTE, "Widget color", |rgb| editor_message(Message::Color(rgb))),
                switch_line(
                    widget.caption,
                    true,
                    "Show the name",
                    "",
                    context.callback(|on| PageMessage::Editor(Message::Caption(on))),
                ),
                switch_line(
                    widget.backdrop,
                    true,
                    "Dark backdrop",
                    "Keeps it readable over a busy picture.",
                    context.callback(|on| PageMessage::Editor(Message::Backdrop(on))),
                ),
                line(
                    8.0,
                    (
                        Button::new()
                            .content("Center")
                            .on_click(editor_message(Message::Center))
                            .tooltip("Move it to the middle of the screen."),
                        Button::new()
                            .content("Center across")
                            .on_click(editor_message(Message::CenterAcross))
                            .tooltip("Keep its height, and center it left to right."),
                        Button::new().content("Duplicate").on_click(editor_message(Message::Duplicate)).tooltip("Ctrl+D"),
                    ),
                )
                .margin(Thickness::new(0.0, 6.0, 0.0, 0.0)),
                line(
                    8.0,
                    (
                        Button::new().content("Bring to front").on_click(editor_message(Message::Front)),
                        Button::new().content("Send to back").on_click(editor_message(Message::Back)),
                        Button::new().content(danger("Remove")).on_click(editor_message(Message::Remove)).tooltip("Delete"),
                    ),
                ),
            )),
        )
        .into()
    }

    /// The dashboard's background style and color.
    fn background_card(&self, layout: &Layout, context: &ViewContext<Self>) -> View {
        let background = layout.background;
        let mut parts: Vec<View> = vec![
            strong("Background").into(),
            ComboBox::new()
                .automation_name("Background")
                .min_width(160.0)
                .items_source(Backdrop::ALL.iter().map(|style| style.name()))
                .selected_index(Backdrop::ALL.iter().position(|style| *style == background.style))
                .on_selection_changed(context.callback(|index| PageMessage::Editor(Message::Background(index))))
                .into(),
        ];
        if background.style != Backdrop::Plain {
            parts.push(swatches(background.color, &layout::PALETTE, "Background color", |rgb| {
                context.message(PageMessage::Editor(Message::BackgroundColor(rgb)))
            }));
        }
        parts.push(switch_line(
            background.follow_effect,
            true,
            "Follow the lighting effect",
            "Plays the effect chosen under Lighting as the background. With the lights off, the background above is used.",
            context.callback(|on| PageMessage::Editor(Message::FollowEffect(on))),
        ));
        padded_card(StackPanel::new().spacing(8.0).children(parts)).into()
    }

    /// Every reading, grouped, with its live value. Clicking one adds it.
    fn library(&self, layout: &Layout, readings: Option<&stats::Readings>, context: &ViewContext<Self>) -> View {
        let full = layout.widgets.len() >= layout::MAX_WIDGETS;
        let mut rows: Vec<View> = vec![Grid::new()
            .columns([GridLength::STAR, GridLength::Auto])
            .children((
                strong("Add a reading"),
                muted(&format!("{} of {}", layout.widgets.len(), layout::MAX_WIDGETS)).grid_column(1),
            ))
            .into()];
        let mut group = "";
        for metric in stats::ALL {
            if metric.group() != group {
                group = metric.group();
                rows.push(label(group));
            }
            let available = layout::is_clock(metric) || readings.is_none_or(|readings| readings.has(metric));
            let value = readings.filter(|_| available && !layout::is_clock(metric)).map(|readings| readings.text(metric));
            let placed = layout.widgets.iter().any(|widget| widget.metric == metric);
            let tip = if !available {
                missing_reason(metric)
            } else if full {
                "The screen is full. Remove a widget first."
            } else {
                "Add it to the screen."
            };
            let dot: View = if placed {
                Ellipse::new()
                    .width(6.0)
                    .height(6.0)
                    .fill(ThemeBrush::Accent)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2)
                    .into()
            } else {
                nothing()
            };
            rows.push(
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .horizontal_content_alignment(HorizontalAlignment::Stretch)
                    .is_enabled(available && !full)
                    .automation_name(format!("Add {}", metric_label(metric)))
                    .on_click(context.message(PageMessage::Editor(Message::Add(metric))))
                    .content(
                        Grid::new()
                            .columns([GridLength::STAR, GridLength::Auto, GridLength::Auto, GridLength::Auto])
                            .column_spacing(8.0)
                            .children((
                                TextBlock::new().text(metric.name()).vertical_alignment(VerticalAlignment::Center),
                                muted(value.as_deref().unwrap_or("")).grid_column(1).vertical_alignment(VerticalAlignment::Center),
                                dot,
                                glyph("\u{E710}", 12.0).grid_column(3).vertical_alignment(VerticalAlignment::Center),
                            )),
                    )
                    .tooltip(tip),
            );
        }
        padded_card(StackPanel::new().spacing(2.0).children(rows)).into()
    }
}

/// A rounded box just outside a widget.
fn outline(widget: &Widget, gap: f64, brush: Brush, thickness: f64) -> View {
    Rectangle::new()
        .canvas_left(widget.x as f64 - gap)
        .canvas_top(widget.y as f64 - gap)
        .width(widget.w as f64 + gap * 2.0)
        .height(widget.h as f64 + gap * 2.0)
        .radius_x(10.0)
        .radius_y(10.0)
        .stroke(brush)
        .stroke_thickness(thickness)
        .into()
}
