//! The Widgets tab: the layout editor with its side panels, undo, and saving.

use super::controls::card_title;
use super::editor::{Canvas, Edit, Editor};
use super::PumpPage;
use crate::layout::{self, Layout, Widget};
use crate::media::Crop;
use crate::settings::Source;
use crate::stats::Metric;
use coolwave_ui::egui::{self, Ui};
use coolwave_ui::theme::{self, colors};
use coolwave_ui::widgets;

/// The column beside the editor's screen.
const SIDE_COLUMN: f32 = 340.0;
/// Width a card adds around its content: margins, stroke, and outer margin.
const CARD_CHROME: f32 = 40.0;
/// Room kept for the page header, banner and the toolbar under the screen.
const HEIGHT_RESERVED: f32 = 150.0;
const SCREEN_SIZE_RANGE: std::ops::RangeInclusive<f32> = 300.0..=640.0;
const UNDO_LIMIT: usize = 60;

/// The part of the preview picture that the crop square covers, as texture coordinates.
fn cropped<'a>(
    preview: Option<&'a egui::TextureHandle>,
    width: u32,
    height: u32,
    crop: &Crop,
) -> Option<(&'a egui::TextureHandle, egui::Rect)> {
    let texture = preview?;
    let (width, height) = (width.max(1), height.max(1));
    let (side, x, y) = crop.square(width, height);
    let (w, h) = (width as f32, height as f32);
    Some((
        texture,
        egui::Rect::from_min_max(
            egui::pos2(x as f32 / w, y as f32 / h),
            egui::pos2((x + side) as f32 / w, (y + side) as f32 / h),
        ),
    ))
}

/// Readings in the order they first appear, for laying a screen out again.
fn metrics_of(widgets: &[Widget]) -> Vec<Metric> {
    let mut metrics = Vec::new();
    for widget in widgets {
        if !metrics.contains(&widget.metric) {
            metrics.push(widget.metric);
        }
    }
    metrics
}

impl PumpPage {
    pub(super) fn widgets_tab(&mut self, ui: &mut Ui, room: egui::Vec2) {
        let canvas = self.pick_canvas(ui);
        self.canvas_banner(ui, canvas);
        let readings = self.core.stats.readings();
        let side = (room.x - SIDE_COLUMN - CARD_CHROME - 20.0)
            .min(room.y - HEIGHT_RESERVED)
            .clamp(*SCREEN_SIZE_RANGE.start(), *SCREEN_SIZE_RANGE.end());
        let crop = self.core.crop();
        let preview = self.preview.take();
        let picture = cropped(
            preview.as_ref(),
            self.core.source_width,
            self.core.source_height,
            &crop,
        );
        let layout = match canvas {
            Canvas::Dashboard => self
                .core
                .settings
                .dashboard_layout
                .get_or_insert_with(Layout::default),
            Canvas::Picture => self
                .core
                .settings
                .picture_layout
                .get_or_insert_with(Layout::default),
        };
        let before = layout.clone();
        let can_undo = !self.undo.is_empty();
        let mut edit = Edit::default();
        let mut undo = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(side + CARD_CHROME, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    theme::card(ui, |ui| {
                        edit.merge(
                            self.editor
                                .edit(ui, side, layout, readings.as_ref(), picture),
                        );
                        ui.add_space(14.0);
                        let (toolbar, wants_undo) =
                            toolbar(ui, &mut self.editor, canvas, layout, can_undo);
                        edit.merge(toolbar);
                        undo = wants_undo;
                    });
                },
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(SIDE_COLUMN.min(ui.available_width()));
                edit.merge(self.editor.inspector(ui, layout, readings.as_ref()));
                ui.add_space(10.0);
                if canvas == Canvas::Dashboard {
                    edit.merge(self.editor.background_card(ui, layout));
                    ui.add_space(10.0);
                }
                edit.merge(self.editor.library(ui, layout, readings.as_ref()));
            });
        });
        undo |= ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Z))
            && !ui.ctx().wants_keyboard_input();
        self.preview = preview;
        self.record_edit(canvas, edit, before);
        if undo && self.gesture.is_none() {
            self.undo_last(canvas);
        }
    }

    /// The switch between the two layouts, kept apart from the page's own tabs.
    fn pick_canvas(&mut self, ui: &mut Ui) -> Canvas {
        let mut canvas = self.editor.canvas;
        ui.horizontal(|ui| {
            ui.label(card_title("Editing"));
            widgets::segmented(
                ui,
                &mut canvas,
                &[
                    (
                        Canvas::Picture,
                        "On your picture",
                        "Readings over your photo or video",
                    ),
                    (
                        Canvas::Dashboard,
                        "Dashboard",
                        "Readings on the dark dashboard",
                    ),
                ],
            );
        });
        ui.add_space(8.0);
        if canvas != self.editor.canvas {
            self.editor.canvas = canvas;
            self.editor.selected = None;
        }
        canvas
    }

    /// Says when the screen being edited is not the one the pump shows.
    fn canvas_banner(&mut self, ui: &mut Ui, canvas: Canvas) {
        let source = self.core.settings.source;
        match canvas {
            Canvas::Picture if !self.core.settings.stats_on_picture => {
                widgets::banner(ui, colors().warn, |ui| {
                    ui.label(
                        egui::RichText::new("Widgets on your picture are off.")
                            .color(colors().text),
                    );
                    if ui.button("Turn them on").clicked() {
                        self.core.settings.stats_on_picture = true;
                        self.core.persist();
                    }
                });
            }
            Canvas::Picture if source != Source::Picture => {
                widgets::banner(ui, colors().muted, |ui| {
                    ui.label(
                        egui::RichText::new("The pump is not showing your picture right now.")
                            .color(colors().text),
                    );
                });
            }
            Canvas::Dashboard if source != Source::Dashboard => {
                widgets::banner(ui, colors().muted, |ui| {
                    ui.label(
                        egui::RichText::new("The pump is not showing the dashboard right now.")
                            .color(colors().text),
                    );
                    if ui
                        .add_enabled(!self.core.busy, egui::Button::new("Show it"))
                        .clicked()
                    {
                        self.core.show_dashboard();
                    }
                });
            }
            _ => {}
        }
    }

    /// A drag is one step to undo, however many frames it took. Saves once it settles.
    fn record_edit(&mut self, canvas: Canvas, edit: Edit, before: Layout) {
        if edit.changed && self.gesture.is_none() {
            self.gesture = Some(before);
        }
        if !edit.settled {
            return;
        }
        if let Some(start) = self.gesture.take() {
            self.undo.push((canvas, start));
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.core.persist();
    }

    fn undo_last(&mut self, canvas: Canvas) {
        let Some(index) = self.undo.iter().rposition(|(on, _)| *on == canvas) else {
            return;
        };
        let (_, previous) = self.undo.remove(index);
        match canvas {
            Canvas::Dashboard => self.core.settings.dashboard_layout = Some(previous),
            Canvas::Picture => self.core.settings.picture_layout = Some(previous),
        }
        self.editor.selected = None;
        self.core.persist();
    }
}

/// Snap, undo, tidy up and clear, under the screen. Returns the edit and whether undo was asked for.
fn toolbar(
    ui: &mut Ui,
    editor: &mut Editor,
    canvas: Canvas,
    layout: &mut Layout,
    can_undo: bool,
) -> (Edit, bool) {
    let mut edit = Edit::default();
    let mut undo = false;
    ui.horizontal_wrapped(|ui| {
        widgets::switch(ui, &mut editor.snap)
            .on_hover_text("Snap to the middle, the edges, and other widgets.");
        ui.label(theme::muted("Snap"));
        ui.add_space(8.0);
        undo = ui
            .add_enabled(can_undo, egui::Button::new("Undo"))
            .on_hover_text("Ctrl+Z")
            .clicked();
        if ui
            .button("Arrange for me")
            .on_hover_text("Lay the same readings out in tidy rows.")
            .clicked()
        {
            let metrics = metrics_of(&layout.widgets);
            let background = layout.background;
            *layout = match canvas {
                Canvas::Dashboard => layout::arrange_dashboard(&metrics, background),
                Canvas::Picture => layout::arrange_band(&metrics, false),
            };
            editor.selected = None;
            edit.merge(Edit::done());
        }
        if ui
            .add_enabled(
                !layout.widgets.is_empty(),
                widgets::danger_button("Remove all"),
            )
            .clicked()
        {
            layout.widgets.clear();
            editor.selected = None;
            edit.merge(Edit::done());
        }
    });
    (edit, undo)
}
