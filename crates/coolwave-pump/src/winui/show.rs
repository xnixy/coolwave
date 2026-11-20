//! The Show tab: what the pump shows, and the controls of each kind of picture.

use super::parts::{self, labeled, switch_line};
use super::{Canvas, Kind, Message, PumpCore, PumpPage};
use crate::engine::Controller;
use crate::settings::Source;
use coolwave_ui::*;
use windows_reactor::*;

/// The widest the picture in the crop view grows.
const CROP_VIEW_MAX: f64 = 280.0;
const CROP_SHADE: Color = Color::argb(140, 0, 0, 0);
const DASHBOARD_PREVIEW: f64 = 240.0;
const ZOOM: Range = Range::new(1.0, 4.0, 0.01);
const SPEED: Range = Range::new(0.25, 2.0, 0.05);

/// The three things the pump can show, with what each is.
const SOURCES: [(Source, &str, &str); 3] = [
    (Source::Picture, "Your photo or video", "Any picture, GIF, or short clip, cropped to the screen."),
    (Source::Dashboard, "Stats dashboard", "Live readings you arrange on the Widgets tab."),
    (Source::Animation, "Pump animation", "The clip stored in the pump. Keeps playing after Coolwave quits."),
];

/// The size the crop view draws a picture of this size at.
fn display_size(width: u32, height: u32) -> (f64, f64) {
    let aspect = width.max(1) as f64 / height.max(1) as f64;
    if aspect >= 1.0 {
        (CROP_VIEW_MAX, CROP_VIEW_MAX / aspect)
    } else {
        (CROP_VIEW_MAX * aspect, CROP_VIEW_MAX)
    }
}

/// Move the crop square by a drag of `(dx, dy)` in the crop view.
pub(super) fn pan(core: &mut Controller, (width, height): (u32, u32), dx: f64, dy: f64) {
    if width == 0 || height == 0 {
        return;
    }
    let (display_w, display_h) = display_size(width, height);
    let (side, _, _) = core.crop().square(width, height);
    let sx = display_w / width as f64;
    let sy = display_h / height as f64;
    let max_x = (width as f64 - side as f64).max(1.0);
    let max_y = (height as f64 - side as f64).max(1.0);
    core.pan_x = (core.pan_x as f64 + dx / (max_x * sx) * 2.0).clamp(-1.0, 1.0) as f32;
    core.pan_y = (core.pan_y as f64 + dy / (max_y * sy) * 2.0).clamp(-1.0, 1.0) as f32;
}

impl PumpPage {
    pub(super) fn show_tab(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let cards: Vec<View> = SOURCES
            .iter()
            .enumerate()
            .map(|(index, (source, title, detail))| {
                self.source_card(*source, title, detail, core.settings.source == *source, context).grid_column(index as i32).into()
            })
            .collect();
        let section = match self.source_tab {
            Source::Picture => self.picture_section(pump, context),
            Source::Dashboard => self.dashboard_section(core, context),
            Source::Animation => animation_section(core, context),
        };
        StackPanel::new()
            .spacing(14.0)
            .children((
                Grid::new()
                    .columns([GridLength::STAR, GridLength::STAR, GridLength::STAR])
                    .column_spacing(12.0)
                    .children(cards),
                section,
            ))
            .into()
    }

    /// One of the three things the pump can show. Clicking it opens its controls below.
    fn source_card(&self, source: Source, title: &str, detail: &str, live: bool, context: &ViewContext<Self>) -> Grid {
        let selected = self.source_tab == source;
        let mut words: Vec<View> = vec![strong(title).into(), caption(detail).into()];
        if live {
            words.push(Border::new().margin(Thickness::new(0.0, 4.0, 0.0, 0.0)).content(pill("On the pump", Tone::Good)).into());
        }
        let mut button = Button::new()
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .vertical_alignment(VerticalAlignment::Stretch)
            .horizontal_content_alignment(HorizontalAlignment::Stretch)
            .vertical_content_alignment(VerticalAlignment::Top)
            .automation_name(title)
            .on_click(context.message(Message::SourceTab(source)))
            .content(StackPanel::new().spacing(4.0).margin(Thickness::new(4.0, 8.0, 4.0, 8.0)).children(words));
        if selected {
            button = button.background(ThemeBrush::SubtleSecondary);
        }
        let mark: View = if selected {
            Border::new()
                .width(3.0)
                .height(28.0)
                .corner_radius(2.0)
                .background(ThemeBrush::Accent)
                .horizontal_alignment(HorizontalAlignment::Left)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        } else {
            nothing()
        };
        Grid::new().min_height(96.0).children((button, mark))
    }

    fn picture_section(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let video = core.is_video();
        let mut actions: Vec<View> = vec![Button::new()
            .style(ButtonStyle::Accent)
            .content("Show on pump")
            .is_enabled(core.chosen.is_some() && !core.busy)
            .on_click(context.message(Message::ShowPicture))
            .into()];
        if video {
            actions.push(Button::new().content("Stop video").on_click(context.message(Message::StopVideo)).into());
        }
        actions.push(caption("Crop and zoom apply the next time you press Show.").vertical_alignment(VerticalAlignment::Center).into());
        padded_card(
            StackPanel::new().spacing(14.0).children((
                Grid::new()
                    .columns([GridLength::Pixel(CROP_VIEW_MAX + 8.0), GridLength::STAR])
                    .column_spacing(18.0)
                    .children((
                        StackPanel::new()
                            .spacing(6.0)
                            .children((self.crop_view(pump, context), caption("Drag the square to crop."))),
                        self.picture_controls(core, context).grid_column(1),
                    )),
                line(10.0, actions),
            )),
        )
        .into()
    }

    /// The preview with its crop square, or a placeholder until a file is chosen.
    fn crop_view(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let Some(preview) = &pump.preview else {
            let message = if core.loading_preview() { "Loading…" } else { "No photo or video chosen" };
            return Border::new()
                .width(CROP_VIEW_MAX)
                .height(CROP_VIEW_MAX * 0.7)
                .corner_radius(12.0)
                .background(ThemeBrush::SubtleSecondary)
                .border_brush(ThemeBrush::CardStroke)
                .border_thickness(1.0)
                .content(muted(message).horizontal_alignment(HorizontalAlignment::Center).vertical_alignment(VerticalAlignment::Center))
                .into();
        };
        let (width, height) = pump.source_size();
        let (display_w, display_h) = display_size(width, height);
        let (side, x, y) = core.crop().square(width.max(1), height.max(1));
        let sx = display_w / width.max(1) as f64;
        let sy = display_h / height.max(1) as f64;
        let (left, top, size_w, size_h) = (x as f64 * sx, y as f64 * sy, side as f64 * sx, side as f64 * sy);
        let shade = |l: f64, t: f64, w: f64, h: f64| -> View {
            Rectangle::new()
                .canvas_left(l)
                .canvas_top(t)
                .width(w.max(0.0))
                .height(h.max(0.0))
                .fill(Brush::Solid(CROP_SHADE))
                .into()
        };
        let marks = windows_reactor::Canvas::new().width(display_w).height(display_h).children((
            shade(0.0, 0.0, display_w, top),
            shade(0.0, top + size_h, display_w, display_h - top - size_h),
            shade(0.0, top, left, size_h),
            shade(left + size_w, top, display_w - left - size_w, size_h),
            Rectangle::new()
                .canvas_left(left)
                .canvas_top(top)
                .width(size_w)
                .height(size_h)
                .stroke(Brush::Solid(parts::CROP_OUTLINE))
                .stroke_thickness(2.0),
        ));
        let forward = context.forward();
        let pressed = {
            let forward = forward.clone();
            Callback::new(move |info: PointerEventInfo| forward.call(Message::CropPressed(info.x, info.y)))
        };
        let moved = {
            let forward = forward.clone();
            let dragging = self.crop_drag.is_some();
            Callback::new(move |info: PointerEventInfo| {
                if dragging || info.is_left_button_pressed {
                    forward.call(Message::CropMoved(info.x, info.y));
                }
            })
        };
        let released = Callback::new(move |_: PointerEventInfo| forward.call(Message::CropReleased));
        Border::new()
            .width(display_w)
            .height(display_h)
            .capture_pointer_on_press(true)
            .pointer_cursor(InputSystemCursorShape::SizeAll)
            .automation_name("Crop")
            .on_pointer_pressed(pressed)
            .on_pointer_moved(moved)
            .on_pointer_released(released)
            .content(Grid::new().children((
                Image::new().source_data(preview.jpeg.clone()).stretch(Stretch::Fill).width(display_w).height(display_h),
                marks,
            )))
            .into()
    }

    fn picture_controls(&self, core: &Controller, context: &ViewContext<Self>) -> StackPanel {
        let mut parts: Vec<View> = Vec::new();
        parts.push(match core.chosen_name() {
            Some(name) => {
                let path = core.chosen.as_ref().map_or(String::new(), |path| path.display().to_string());
                strong(&name).text_trimming(TextTrimming::CharacterEllipsis).tooltip(path)
            }
            None => muted("No file chosen").into(),
        });
        parts.push(
            Button::new()
                .content("Choose file…")
                .is_enabled(!core.busy && !self.picking)
                .on_click(context.message(Message::Choose(Kind::Picture)))
                .into(),
        );
        parts.push(labeled(
            "Zoom",
            slider(
                core.zoom as f64,
                ZOOM,
                format!("{:.2}×", core.zoom),
                "Zoom",
                true,
                context.callback(Message::Zoom),
            ),
        ));
        if core.is_video() {
            parts.push(labeled(
                "Speed",
                slider(
                    core.speed as f64,
                    SPEED,
                    format!("{:.2}×", core.speed),
                    "Speed",
                    true,
                    context.callback(Message::Speed),
                ),
            ));
            parts.push(caption("1× matches the video. Speed changes while it plays.").into());
        }
        parts.push(Button::new().content("Reset crop").on_click(context.message(Message::ResetCrop)).into());
        let on_picture = core.settings.stats_on_picture;
        parts.push(switch_line(
            on_picture,
            true,
            "Show widgets on the picture",
            "Place live readings over it on the Widgets tab.",
            context.callback(Message::StatsOnPicture),
        ));
        if on_picture {
            parts.push(
                Button::new().content("Edit widgets").on_click(context.message(Message::EditWidgets(Canvas::Picture))).into(),
            );
        }
        StackPanel::new().spacing(10.0).children(parts)
    }

    fn dashboard_section(&self, core: &Controller, context: &ViewContext<Self>) -> View {
        let action: View = if core.settings.source == Source::Dashboard {
            pill("On the pump now", Tone::Good)
        } else {
            Button::new()
                .style(ButtonStyle::Accent)
                .content("Show dashboard on pump")
                .is_enabled(!core.busy)
                .on_click(context.message(Message::ShowDashboard))
                .into()
        };
        padded_card(
            Grid::new()
                .columns([GridLength::Pixel(DASHBOARD_PREVIEW + 14.0), GridLength::STAR])
                .column_spacing(18.0)
                .children((
                    parts::bezel(
                        Border::new()
                            .width(DASHBOARD_PREVIEW)
                            .height(DASHBOARD_PREVIEW)
                            .content(self.screen_frames(Canvas::Dashboard, context)),
                    )
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .vertical_alignment(VerticalAlignment::Top),
                    StackPanel::new().grid_column(1).spacing(10.0).children((
                        strong("Stats dashboard"),
                        muted(
                            "Live readings on a dark background. Coolwave redraws it once a second, so it stops \
                             updating when Coolwave quits.",
                        ),
                        action,
                        Button::new()
                            .content("Edit widgets")
                            .on_click(context.message(Message::EditWidgets(Canvas::Dashboard))),
                    )),
                )),
        )
        .into()
    }
}

fn animation_section(core: &Controller, context: &ViewContext<PumpPage>) -> View {
    let action: View = if core.settings.source == Source::Animation {
        pill("On the pump now", Tone::Good)
    } else {
        Button::new()
            .style(ButtonStyle::Accent)
            .content("Show pump animation")
            .is_enabled(!core.busy)
            .on_click(context.message(Message::ShowAnimation))
            .into()
    };
    padded_card(StackPanel::new().spacing(10.0).children((
        strong("Pump animation"),
        muted(
            "The animation stored in the pump. Nothing is sent from this computer, so it keeps playing after \
             Coolwave quits.",
        ),
        action,
    )))
    .into()
}
