//! The egui window: Blurman's look around Coolwave's lighting, cooling and
//! pump screen pages. This is the app on Windows.
//!
//! The engine and the pump screen keep running while the window sits in the
//! tray, so the window is parked instead of closed.

mod autostart;
mod columns;
mod cooling;
mod instance;
mod lighting;
mod settings_page;
mod tray;
mod window;

use coolwave_core::effects::Effect;
use coolwave_core::engine::Engine;
use coolwave_core::settings::Settings;
use coolwave_ui::theme::{self, ACCENT, colors};
use coolwave_ui::widgets::{self, NavIcon};
use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WINDOW_TITLE: &str = "Coolwave";
const WINDOW_SIZE: [f32; 2] = [1180.0, 800.0];
const MIN_WINDOW_SIZE: [f32; 2] = [1000.0, 660.0];
const SIDEBAR_WIDTH: f32 = 232.0;
/// How often changed settings are written to disk.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(2);
/// Live readings (fan speeds, device states) are redrawn this often.
const REFRESH_OPEN: Duration = Duration::from_millis(500);
/// In the tray only the autosave and the tray menu need a turn.
const REFRESH_IN_TRAY: Duration = Duration::from_secs(2);
const TOAST_FOR: Duration = Duration::from_secs(3);
const TOAST_FADE: Duration = Duration::from_millis(400);

/// With `COOLWAVE_UI_PREVIEW` set, the window opens without touching any
/// device or the settings file. For working on the pages and taking pictures.
pub fn preview() -> bool {
    std::env::var_os("COOLWAVE_UI_PREVIEW").is_some()
}

/// Called from other threads to make the window repaint soon.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What every page works on.
pub struct Shared {
    pub engine: Engine,
    pub settings: Settings,
    toast: Option<(String, Instant)>,
    /// Effect to go back to when lighting is switched on again.
    effect_before_off: Effect,
}

impl Shared {
    /// Show a short message at the bottom of the window.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    pub fn lighting_on(&self) -> bool {
        self.engine.config().params.effect != Effect::Off
    }

    pub fn set_lighting(&mut self, on: bool) {
        let current = self.engine.config().params.effect;
        if !on && current != Effect::Off {
            self.effect_before_off = current;
        }
        let effect = if on { self.effect_before_off } else { Effect::Off };
        self.engine.update(|config| config.params.effect = effect);
    }

    pub fn set_effect(&mut self, effect: Effect) {
        if effect != Effect::Off {
            self.effect_before_off = effect;
        }
        self.engine.update(|config| config.params.effect = effect);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Lighting,
    Cooling,
    PumpScreen,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Lighting, Page::Cooling, Page::PumpScreen, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Lighting => "Lighting",
            Page::Cooling => "Cooling",
            Page::PumpScreen => "Pump screen",
            Page::Settings => "Settings",
        }
    }

    /// The page named in `COOLWAVE_UI_PAGE`, to open straight onto it while working on it.
    fn from_env() -> Option<Page> {
        let name = std::env::var("COOLWAVE_UI_PAGE").ok()?;
        Page::ALL.into_iter().find(|page| page.label().to_ascii_lowercase().starts_with(&name.to_ascii_lowercase()))
    }

    fn icon(self) -> NavIcon {
        match self {
            Page::Lighting => NavIcon::Lighting,
            Page::Cooling => NavIcon::Cooling,
            Page::PumpScreen => NavIcon::PumpScreen,
            Page::Settings => NavIcon::Settings,
        }
    }
}

pub fn run(start_in_tray: bool) -> Result<(), String> {
    // The tray, a second launch and the pump worker run on other threads. They
    // repaint whichever context the window has once it exists.
    let context: Arc<Mutex<Option<egui::Context>>> = Arc::default();
    let wake: Wake = {
        let context = Arc::clone(&context);
        Arc::new(move || {
            if let Some(ctx) = context.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                ctx.request_repaint();
            }
        })
    };
    if !preview() && !instance::claim(Arc::clone(&wake)) {
        // Coolwave is already running and was asked to show its window.
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_app_id("coolwave")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_transparent(cfg!(windows))
            .with_icon(window::icon()),
        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            theme::set_light(cc.egui_ctx.theme() == egui::Theme::Light);
            *context.lock().unwrap_or_else(|e| e.into_inner()) = Some(cc.egui_ctx.clone());
            Ok(Box::new(App::new(window::Window::of(cc), wake, start_in_tray)))
        }),
    )
    .map_err(|error| error.to_string())
}

struct App {
    shared: Shared,
    page: Page,
    lighting: lighting::LightingPage,
    cooling: cooling::CoolingPage,
    pump: coolwave_pump::PumpPage,
    settings_page: settings_page::SettingsPage,
    tray: tray::Tray,
    window: window::Window,
    /// Windows gave the window its acrylic backdrop, so panels are see-through.
    glass: bool,
    /// Go to the tray as soon as the window exists (started with `--background`).
    hide_once: bool,
    in_tray: bool,
    quitting: bool,
    /// The settings file as last written, to save only what changed.
    last_saved: String,
    save_checked: Instant,
}

impl App {
    fn new(window: window::Window, wake: Wake, start_in_tray: bool) -> Self {
        let mut settings = Settings::load();
        if preview() {
            settings.lighting.eurux_enabled = false;
            settings.lighting.aura_enabled = false;
            settings.lighting.gpu_enabled = false;
        }
        let engine = Engine::start(settings.fan_layout.clone(), settings.lighting.clone());
        let effect_before_off = match settings.lighting.params.effect {
            Effect::Off => Effect::Aurora,
            effect => effect,
        };
        // Empty when no file exists yet, so the first autosave writes one.
        let last_saved = if coolwave_core::settings::path().exists() { toml_of(&settings) } else { String::new() };
        let shared = Shared { engine, settings, toast: None, effect_before_off };
        Self {
            lighting: lighting::LightingPage::new(&shared),
            cooling: cooling::CoolingPage::new(&shared),
            pump: coolwave_pump::PumpPage::new(Arc::clone(&wake)),
            settings_page: settings_page::SettingsPage::new(),
            tray: tray::Tray::build(wake),
            glass: window.frost(),
            window,
            shared,
            page: Page::from_env().unwrap_or(Page::Lighting),
            hide_once: start_in_tray,
            in_tray: false,
            quitting: false,
            last_saved,
            save_checked: Instant::now(),
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.in_tray = false;
        self.hide_once = false;
        self.window.reveal(ctx);
        ctx.request_repaint();
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        self.save_now();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        while let Some(command) = self.tray.next_command() {
            match command {
                tray::Command::Show => self.show_window(ctx),
                tray::Command::ToggleLighting => {
                    let on = self.shared.lighting_on();
                    self.shared.set_lighting(!on);
                }
                tray::Command::SetEffect(effect) => self.shared.set_effect(effect),
                tray::Command::Quit => self.quit(ctx),
            }
        }
        self.tray.sync(self.shared.lighting_on(), self.shared.effect_before_off);
    }

    /// Closing keeps Coolwave in the tray when the setting says so and there is a tray.
    fn closes_to_tray(&self) -> bool {
        self.shared.settings.run_in_background && self.tray.available()
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if self.hide_once {
            if !self.tray.available() {
                self.hide_once = false;
            } else if self.window.park(ctx) {
                self.hide_once = false;
                self.in_tray = true;
            } else {
                // The window is not there yet. Try again next frame.
                ctx.request_repaint();
            }
        }
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && !self.quitting {
            if self.closes_to_tray() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.in_tray = self.window.park(ctx);
            } else {
                self.save_now();
            }
        }
        if self.in_tray && !self.window.stay_parked(ctx) {
            // Something else brought the window back, such as a second launch.
            self.in_tray = false;
        }
    }

    fn save_now(&mut self) {
        if preview() {
            return;
        }
        self.shared.settings.lighting = self.shared.engine.config();
        let text = toml_of(&self.shared.settings);
        if text == self.last_saved {
            return;
        }
        match self.shared.settings.save() {
            Ok(()) => self.last_saved = text,
            Err(error) => eprintln!("coolwave: failed to save settings: {error}"),
        }
    }

    fn autosave(&mut self) {
        if self.save_checked.elapsed() >= AUTOSAVE_EVERY {
            self.save_checked = Instant::now();
            self.save_now();
        }
    }

    /// The brand, what the lights are doing with a switch for them, and the pages.
    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 26.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, ACCENT);
            ui.label(RichText::new("Coolwave").size(20.0).strong().color(colors().text));
        });
        ui.add_space(12.0);
        self.status_card(ui);
        ui.add_space(14.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for page in Page::ALL {
            if widgets::nav_item(ui, page.icon(), page.label(), self.page == page).clicked() {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(4.0);
            ui.label(theme::muted(format!("Version {}", env!("CARGO_PKG_VERSION"))).small());
            let note = if self.closes_to_tray() { "Closing keeps Coolwave in the tray." } else { "Closing stops the effects." };
            ui.add(egui::Label::new(theme::muted(note).small()).wrap());
        });
    }

    fn status_card(&mut self, ui: &mut Ui) {
        let on = self.shared.lighting_on();
        let effect = self.shared.engine.config().params.effect;
        theme::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                let dot = if on { colors().ok } else { colors().muted };
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                let text = if on { effect.name().to_string() } else { "Lights off".to_string() };
                ui.label(RichText::new(text).color(colors().text));
            });
            let label = if on { "Lights off" } else { "Lights on" };
            if ui.add_sized([ui.available_width(), 30.0], egui::Button::new(label)).clicked() {
                self.shared.set_lighting(!on);
            }
        });
    }

    /// Draws the toast and says how soon it needs another frame.
    fn draw_toast(&mut self, ctx: &egui::Context) -> Option<Duration> {
        let (text, shown) = self.shared.toast.as_ref()?;
        let age = shown.elapsed();
        if age >= TOAST_FOR {
            self.shared.toast = None;
            return None;
        }
        let left = TOAST_FOR - age;
        let opacity = (left.as_secs_f32() / TOAST_FADE.as_secs_f32()).min(1.0);
        widgets::toast(ctx, text, opacity);
        Some(if left <= TOAST_FADE { Duration::from_millis(16) } else { left - TOAST_FADE })
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.glass { [0.0; 4] } else { colors().bg.to_normalized_gamma_f32() }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // egui follows the system's app mode on its own. The palette and title bar follow egui.
        let light = ctx.theme() == egui::Theme::Light;
        if theme::set_light(light) {
            self.window.paint_frame(light);
        }
        if instance::take_show_request() {
            self.show_window(ctx);
        }
        self.poll_tray(ctx);
        self.handle_close(ctx);
        self.autosave();
        let pump_wants_frame = self.pump.tick(ctx, !self.in_tray);

        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(SIDEBAR_WIDTH)
            .frame(theme::sidebar(self.glass))
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(theme::page(self.glass)).show(ctx, |ui| match self.page {
            Page::Lighting => self.lighting.ui(ui, &mut self.shared),
            Page::Cooling => self.cooling.ui(ui, &mut self.shared),
            Page::PumpScreen => self.pump.ui(ui),
            Page::Settings => {
                if self.settings_page.ui(ui, &mut self.shared) == settings_page::Action::Quit {
                    self.quit(ctx);
                }
            }
        });
        let toast = self.draw_toast(ctx);

        let mut next = if self.in_tray { REFRESH_IN_TRAY } else { REFRESH_OPEN };
        if let Some(toast) = toast {
            next = next.min(toast);
        }
        if pump_wants_frame {
            next = Duration::ZERO;
        }
        ctx.request_repaint_after(next);
    }
}

fn toml_of(settings: &Settings) -> String {
    toml::to_string(settings).unwrap_or_default()
}
