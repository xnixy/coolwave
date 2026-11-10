//! The Show, Screen and Settings tabs.

use super::controls::{self, card_title, pill, settled, slider_row};
use super::editor::Canvas;
use super::files::{self, Kind};
use super::{PumpPage, Tab};
use crate::session::Setting;
use crate::settings::Source;
use coolwave_ui::egui::{self, Align, CornerRadius, Layout, RichText, Sense, Stroke, Ui};
use coolwave_ui::theme::{self, colors, ACCENT};
use coolwave_ui::widgets;

const SOURCE_CARD_HEIGHT: f32 = 86.0;
const SOURCE_CARD_RADIUS: u8 = 14;
const PICTURE_COLUMN: f32 = 300.0;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 1.0..=4.0;
const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.25..=2.0;
const DASHBOARD_PREVIEW: f32 = 240.0;
const ROTATIONS: [(u16, &str, &str); 4] = [
    (0, "0°", ""),
    (90, "90°", ""),
    (180, "180°", ""),
    (270, "270°", ""),
];
const USB_ID: &str = "0B05:1DE7";

impl PumpPage {
    pub(super) fn show_tab(&mut self, ui: &mut Ui) {
        ui.columns_const(|[picture, dashboard, animation]| {
            self.source_card(
                picture,
                Source::Picture,
                "Your photo or video",
                "Any picture, GIF, or short clip, cropped to the screen.",
            );
            self.source_card(
                dashboard,
                Source::Dashboard,
                "Stats dashboard",
                "Live readings you arrange on the Widgets tab.",
            );
            self.source_card(
                animation,
                Source::Animation,
                "Pump animation",
                "The clip stored in the pump. Keeps playing after Coolwave quits.",
            );
        });
        ui.add_space(14.0);
        match self.source_tab {
            Source::Picture => self.picture_section(ui),
            Source::Dashboard => self.dashboard_section(ui),
            Source::Animation => self.animation_section(ui),
        }
    }

    /// One of the three things the pump can show. Clicking it opens its controls below.
    fn source_card(&mut self, ui: &mut Ui, source: Source, title: &str, detail: &str) {
        let c = colors();
        let selected = self.source_tab == source;
        let live = self.core.settings.source == source;
        let frame = egui::Frame::new()
            .fill(if selected { c.row_selected } else { c.well })
            .stroke(Stroke::new(
                if selected { 1.5_f32 } else { 1.0 },
                if selected { ACCENT } else { c.hairline },
            ))
            .corner_radius(CornerRadius::same(SOURCE_CARD_RADIUS))
            .inner_margin(egui::Margin::same(14));
        let shown = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(SOURCE_CARD_HEIGHT);
            ui.horizontal(|ui| {
                ui.label(card_title(title));
                if live {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        pill(ui, "On the pump", c.ok);
                    });
                }
            });
            ui.add(egui::Label::new(theme::muted(detail).small()).wrap());
        });
        let response = ui
            .interact(
                shown.response.rect,
                ui.id().with(("source", source as u8)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() && !selected {
            ui.painter().rect_stroke(
                shown.response.rect,
                CornerRadius::same(SOURCE_CARD_RADIUS),
                Stroke::new(1.0_f32, c.text.gamma_multiply(0.3)),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            self.source_tab = source;
        }
    }

    fn picture_section(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.horizontal_top(|ui| {
                let left = PICTURE_COLUMN.min(ui.available_width() * 0.45);
                ui.allocate_ui_with_layout(
                    egui::vec2(left, 0.0),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.set_width(left);
                        self.crop_area(ui, left);
                    },
                );
                ui.add_space(12.0);
                ui.vertical(|ui| self.picture_controls(ui));
            });
            ui.add_space(10.0);
            self.picture_actions(ui);
        });
    }

    /// The preview with its crop square, or a placeholder until a file is chosen.
    fn crop_area(&mut self, ui: &mut Ui, width: f32) {
        let Some(preview) = &self.preview else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, width * 0.7), Sense::hover());
            ui.painter().rect(
                rect,
                CornerRadius::same(12),
                colors().well,
                Stroke::new(1.0_f32, colors().hairline),
                egui::StrokeKind::Inside,
            );
            let message = if self.core.loading_preview() {
                "Loading..."
            } else {
                "No photo or video chosen"
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                message,
                egui::FontId::proportional(15.0),
                colors().muted,
            );
            return;
        };
        let source = (self.core.source_width, self.core.source_height);
        let mut crop = self.core.crop();
        let dropped = controls::crop_view(ui, preview, source, &mut crop);
        self.core.pan_x = crop.pan_x;
        self.core.pan_y = crop.pan_y;
        if dropped {
            self.core.persist();
        }
        ui.label(theme::muted("Drag the square to crop.").small());
    }

    fn picture_controls(&mut self, ui: &mut Ui) {
        match self.core.chosen_name() {
            Some(name) => {
                ui.add(egui::Label::new(card_title(&name)).truncate())
                    .on_hover_text(
                        self.core
                            .chosen
                            .as_ref()
                            .map_or(String::new(), |p| p.display().to_string()),
                    );
            }
            None => {
                ui.label(theme::muted("No file chosen"));
            }
        }
        let busy = self.core.busy;
        if let Some(path) =
            files::choose(ui, &mut self.typed, Kind::Picture, "Choose file...", !busy)
        {
            self.preview = None;
            self.core.choose(path);
        }
        ui.add_space(6.0);
        let zoom = slider_row(ui, "Zoom", |ui| {
            ui.add(
                egui::Slider::new(&mut self.core.zoom, ZOOM_RANGE)
                    .fixed_decimals(2)
                    .suffix("×"),
            )
        });
        if settled(&zoom) {
            self.core.persist();
        }
        if self.core.is_video() {
            self.speed_row(ui);
        }
        if ui.button("Reset crop").clicked() {
            self.core.reset_crop();
            self.core.persist();
        }
        ui.add_space(6.0);
        let mut on_picture = self.core.settings.stats_on_picture;
        if widgets::switch_row(
            ui,
            &mut on_picture,
            "Show widgets on the picture",
            "Place live readings over it on the Widgets tab.",
        ) {
            self.core.settings.stats_on_picture = on_picture;
            self.core.persist();
        }
        if on_picture && ui.button("Edit widgets").clicked() {
            self.open_editor(Canvas::Picture);
        }
    }

    fn speed_row(&mut self, ui: &mut Ui) {
        let speed = slider_row(ui, "Speed", |ui| {
            ui.add(
                egui::Slider::new(&mut self.core.speed, SPEED_RANGE)
                    .fixed_decimals(2)
                    .suffix("×"),
            )
        });
        if speed.changed() {
            let value = self.core.speed;
            self.core.set_speed(value);
        }
        if settled(&speed) {
            self.core.persist();
        }
        ui.label(theme::muted("1× matches the video. Speed changes while it plays.").small());
    }

    fn picture_actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let can_send = self.core.chosen.is_some() && !self.core.busy;
            if ui
                .add_enabled(can_send, theme::primary_button("Show on pump"))
                .clicked()
            {
                self.core.send_chosen();
            }
            if self.core.is_video() && ui.button("Stop video").clicked() {
                self.core.stop_video();
            }
            ui.label(theme::muted("Crop and zoom apply the next time you press Show.").small());
        });
    }

    fn dashboard_section(&mut self, ui: &mut Ui) {
        let readings = self.core.stats.readings();
        theme::card(ui, |ui| {
            ui.horizontal_top(|ui| {
                let layout = self.core.settings.dashboard().clone();
                self.editor.preview(
                    ui,
                    DASHBOARD_PREVIEW,
                    Canvas::Dashboard,
                    &layout,
                    readings.as_ref(),
                    None,
                );
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.label(card_title("Stats dashboard"));
                    ui.add(
                        egui::Label::new(theme::muted(
                            "Live readings on a dark background. Coolwave redraws it once a \
                             second, so it stops updating when Coolwave quits.",
                        ))
                        .wrap(),
                    );
                    ui.add_space(8.0);
                    if self.core.settings.source == Source::Dashboard {
                        pill(ui, "On the pump now", colors().ok);
                    } else if ui
                        .add_enabled(
                            !self.core.busy,
                            theme::primary_button("Show dashboard on pump"),
                        )
                        .clicked()
                    {
                        self.core.show_dashboard();
                    }
                    ui.add_space(4.0);
                    if ui.button("Edit widgets").clicked() {
                        self.open_editor(Canvas::Dashboard);
                    }
                });
            });
        });
    }

    fn animation_section(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.label(card_title("Pump animation"));
            ui.add(
                egui::Label::new(theme::muted(
                    "The animation stored in the pump. Nothing is sent from this computer, so \
                     it keeps playing after Coolwave quits.",
                ))
                .wrap(),
            );
            ui.add_space(8.0);
            if self.core.settings.source == Source::Animation {
                pill(ui, "On the pump now", colors().ok);
            } else if ui
                .add_enabled(
                    !self.core.busy,
                    theme::primary_button("Show pump animation"),
                )
                .clicked()
            {
                self.core.show_animation();
            }
        });
    }

    /// Opens the Widgets tab on one of its two screens.
    fn open_editor(&mut self, canvas: Canvas) {
        self.tab = Tab::Widgets;
        self.editor.canvas = canvas;
    }

    pub(super) fn screen_tab(&mut self, ui: &mut Ui) {
        ui.columns_const(|[left, right]| {
            theme::card(left, |ui| self.display_card(ui));
            theme::card(right, |ui| self.pump_card(ui));
        });
    }

    fn display_card(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(card_title("Display"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.core.busy, egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.core.refresh();
                }
            });
        });
        ui.add_space(6.0);
        let screen_on = self
            .core
            .snapshot
            .as_ref()
            .is_none_or(|shot| shot.screen_on);
        let slider = slider_row(ui, "Brightness", |ui| {
            ui.add_enabled(
                screen_on,
                egui::Slider::new(&mut self.core.brightness, 0..=100).suffix("%"),
            )
        });
        if settled(&slider) && !self.core.busy {
            self.core.apply_brightness();
        }
        self.rotation_row(ui);
        ui.add_space(8.0);
        self.power_switches(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let idle = !self.core.busy;
            if ui
                .add_enabled(idle, egui::Button::new("Wake"))
                .on_hover_text("Wake the screen if it went to sleep.")
                .clicked()
            {
                self.core.apply_power("resume");
            }
            if ui
                .add_enabled(idle, egui::Button::new("Sleep"))
                .on_hover_text("Put the screen to sleep, like when the computer sleeps.")
                .clicked()
            {
                self.core.apply_power("suspend");
            }
        });
    }

    fn rotation_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            controls::row_label(ui, "Rotation");
            let current = self.core.snapshot.as_ref().map(|shot| shot.rotation);
            let mut picked = current.unwrap_or(u16::MAX);
            ui.add_enabled_ui(!self.core.busy && current.is_some(), |ui| {
                if widgets::segmented(ui, &mut picked, &ROTATIONS) {
                    self.core.apply_rotation(picked);
                }
            });
        });
    }

    fn power_switches(&mut self, ui: &mut Ui) {
        let Some(shot) = self.core.snapshot.clone() else {
            ui.label(theme::muted("Connect the pump to change these."));
            return;
        };
        ui.add_enabled_ui(!self.core.busy, |ui| {
            let mut on = shot.screen_on;
            if widgets::switch_row(
                ui,
                &mut on,
                "Screen on",
                "Off keeps the panel dark until you turn it back on.",
            ) {
                self.core.apply_setting(Setting::ScreenOn(on));
            }
            let mut standby = shot.display_in_sleep;
            if widgets::switch_row(
                ui,
                &mut standby,
                "Standby clip while the computer sleeps",
                "Off turns the panel off while the computer sleeps.",
            ) {
                self.core.apply_setting(Setting::DisplayInSleep(standby));
            }
        });
    }

    fn pump_card(&mut self, ui: &mut Ui) {
        ui.label(card_title("This pump"));
        ui.add_space(6.0);
        let shot = self.core.snapshot.clone();
        let from_shot = |read: fn(&crate::engine::Snapshot) -> String| {
            shot.as_ref().map_or("-".to_string(), read)
        };
        let rows = [
            ("Model", "ROG Strix LC IV".to_string()),
            ("Serial", from_shot(|shot| shot.serial.clone())),
            ("Firmware", from_shot(|shot| shot.firmware.clone())),
            ("Free space", from_shot(|shot| shot.free_space.clone())),
            ("USB", USB_ID.to_string()),
        ];
        egui::Grid::new("pump-facts")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                for (name, value) in rows {
                    ui.label(theme::muted(name));
                    ui.label(RichText::new(value).color(colors().text));
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                theme::muted(
                    "The pump and fans run from the motherboard. Coolwave only drives the screen.",
                )
                .small(),
            )
            .wrap(),
        );
        ui.add_space(8.0);
        self.firmware_section(ui);
    }

    fn firmware_section(&mut self, ui: &mut Ui) {
        widgets::section_label(ui, "Firmware");
        ui.add(
            egui::Label::new(theme::muted(
                "Install a firmware package downloaded from ASUS. It is checked first, then sent \
                 the same way the ASUS app sends it.",
            ))
            .wrap(),
        );
        let ready = self.core.snapshot.is_some() && !self.core.busy && !self.core.asus_running;
        if let Some(path) = files::choose(
            ui,
            &mut self.typed,
            Kind::Firmware,
            "Update firmware...",
            ready,
        ) {
            self.core.offer_firmware(&path);
        }
    }

    pub(super) fn settings_tab(&mut self, ui: &mut Ui) {
        ui.columns_const(|[left, right]| {
            theme::card(left, |ui| {
                ui.label(card_title("Startup"));
                ui.add_space(6.0);
                let mut resume = self.core.settings.resume_last;
                if widgets::switch_row(
                    ui,
                    &mut resume,
                    "Play the last picture when Coolwave starts",
                    "Sends the last photo or video again on its own.",
                ) {
                    self.core.settings.resume_last = resume;
                    self.core.persist();
                }
                ui.add_space(8.0);
                let mut follow = self.core.settings.sleep_when_locked;
                if widgets::switch_row(
                    ui,
                    &mut follow,
                    "Sleep the screen while locked",
                    "Wakes it again when you unlock.",
                ) {
                    self.core.set_sleep_when_locked(follow);
                }
            });
            theme::card(right, |ui| self.readings_card(ui));
        });
    }

    fn readings_card(&mut self, ui: &mut Ui) {
        ui.label(card_title("Readings"));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            controls::row_label(ui, "Temperature");
            let mut fahrenheit = self.core.settings.fahrenheit;
            if widgets::segmented(ui, &mut fahrenheit, &[(false, "°C", ""), (true, "°F", "")]) {
                self.core.settings.fahrenheit = fahrenheit;
                self.core.persist();
            }
        });
        ui.add_space(4.0);
        let text = if cfg!(windows) {
            "CPU temperature and power, pump speed, coolant, and fan speeds come from HWiNFO. \
             Run it with Shared Memory Support turned on in its settings. GPU readings need an \
             NVIDIA card."
        } else {
            "CPU temperature comes from the kernel. GPU readings need an NVIDIA card."
        };
        ui.add(egui::Label::new(theme::muted(text)).wrap());
        ui.add_space(6.0);
        ui.label(theme::muted("Settings file").small());
        ui.add(egui::Label::new(theme::muted(&self.settings_file).small()).truncate())
            .on_hover_text(&self.settings_file);
    }
}
