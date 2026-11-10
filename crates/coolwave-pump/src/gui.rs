//! The pump screen page of the egui window. What the buttons do lives in
//! `engine::Controller`; this draws its state and passes the clicks on.

use crate::engine::{demo, Controller};
use coolwave_ui::egui;
use coolwave_ui::theme::{self, colors};
use coolwave_ui::widgets;
use std::sync::Arc;
use std::time::Duration;

mod controls;
mod dialogs;
mod editor;
mod files;
mod pages;
mod widgets_page;

/// Called from other threads to make the window repaint soon.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// How often to look at the worker while it is busy.
const BUSY_POLL: Duration = Duration::from_millis(200);
/// Environment variable that opens the page on a given tab, to work on it.
const TAB_ENV: &str = "COOLWAVE_UI_PUMP_TAB";

/// The sub-pages of the pump screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Show,
    Widgets,
    Screen,
    Settings,
}

impl Tab {
    const ALL: [(Tab, &'static str, &'static str); 4] = [
        (Tab::Show, "Show", "What the pump screen displays"),
        (
            Tab::Widgets,
            "Widgets",
            "Arrange live readings on the screen",
        ),
        (
            Tab::Screen,
            "Screen",
            "Brightness, rotation, power and firmware",
        ),
        (Tab::Settings, "Settings", "How the pump screen behaves"),
    ];

    /// The tab named in `COOLWAVE_UI_PUMP_TAB`.
    fn from_env() -> Option<Tab> {
        let name = std::env::var(TAB_ENV).ok()?.to_ascii_lowercase();
        Tab::ALL
            .iter()
            .find(|(_, label, _)| label.to_ascii_lowercase() == name)
            .map(|(tab, _, _)| *tab)
    }
}

pub struct PumpPage {
    core: Controller,
    preview: Option<egui::TextureHandle>,
    tab: Tab,
    /// The source whose controls the Show tab has open. What the pump shows is `settings.source`.
    source_tab: crate::settings::Source,
    editor: editor::Editor,
    /// Widget layouts before each change, newest last.
    undo: Vec<(editor::Canvas, crate::layout::Layout)>,
    /// The layout as it was when the current drag began.
    gesture: Option<crate::layout::Layout>,
    /// Paths typed in where no file dialog is available.
    typed: files::TypedPaths,
    /// Where the settings live, shown on the Settings tab.
    settings_file: String,
}

impl PumpPage {
    /// Starts the pump screen worker. `wake` asks the window for a repaint.
    pub fn new(wake: Wake) -> Self {
        let core = Controller::new(wake);
        let source_tab = core.settings.source;
        Self {
            core,
            preview: None,
            tab: Tab::from_env().unwrap_or(Tab::Show),
            source_tab,
            editor: editor::Editor::new(),
            undo: Vec::new(),
            gesture: None,
            typed: files::TypedPaths::default(),
            settings_file: crate::settings::settings_path().display().to_string(),
        }
    }

    /// Runs once per frame, also while the window sits in the tray. True when
    /// the page wants another frame soon.
    pub fn tick(&mut self, ctx: &egui::Context, window_open: bool) -> bool {
        self.core.window_open = window_open;
        // Coolwave owns the tray; drain anything queued for the old one.
        while self.core.tray_rx.try_recv().is_ok() {}
        let mut wants_frame = self.core.tick();
        wants_frame |= self.poll_preview(ctx);
        if self.core.waiting() {
            ctx.request_repaint_after(BUSY_POLL);
        }
        wants_frame
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        widgets::page_header(
            ui,
            "Pump screen",
            "The 720 x 720 screen on the ROG Strix LC IV.",
            |ui| self.connection_pill(ui),
        );
        let mut tab = self.tab;
        widgets::segmented(ui, &mut tab, &Tab::ALL);
        self.tab = tab;
        ui.add_space(10.0);
        self.banners(ui);
        let room = ui.available_size();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match self.tab {
                    Tab::Show => self.show_tab(ui),
                    Tab::Widgets => self.widgets_tab(ui, room),
                    Tab::Screen => self.screen_tab(ui),
                    Tab::Settings => self.settings_tab(ui),
                }
                ui.add_space(8.0);
                ui.label(theme::muted(&self.core.status).small());
            });
        self.firmware_dialog(ui.ctx());
    }

    fn poll_preview(&mut self, ctx: &egui::Context) -> bool {
        let Some(preview) = self.core.take_preview() else {
            return false;
        };
        match image::load_from_memory(&preview.jpeg) {
            Ok(image) => {
                let decoded = image.to_rgb8();
                let color = egui::ColorImage::from_rgb(
                    [decoded.width() as usize, decoded.height() as usize],
                    decoded.as_raw(),
                );
                self.preview =
                    Some(ctx.load_texture("pump-preview", color, egui::TextureOptions::LINEAR));
            }
            Err(error) => {
                self.core.status = format!("could not show a preview: {error}");
            }
        }
        true
    }

    /// Whether the pump is reachable, at the right of the page header.
    fn connection_pill(&mut self, ui: &mut egui::Ui) {
        let c = colors();
        let screen_off = self
            .core
            .snapshot
            .as_ref()
            .is_some_and(|shot| !shot.screen_on);
        let (text, color) = if demo() {
            ("Preview, no pump", c.muted)
        } else if self.core.asus_running {
            ("ASUS app is open", c.warn)
        } else if screen_off {
            ("Screen is off", c.warn)
        } else if self.core.snapshot.is_some() {
            ("Connected", c.ok)
        } else if self.core.busy {
            ("Connecting...", c.warn)
        } else {
            ("Not connected", c.muted)
        };
        controls::pill(ui, text, color);
    }

    fn banners(&mut self, ui: &mut egui::Ui) {
        if self.core.asus_running {
            widgets::banner(ui, colors().warn, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Quit ROG STRIX LC & SLC IV Series. It is holding the pump's USB connection.",
                    )
                    .color(colors().text),
                );
            });
        }
    }
}
