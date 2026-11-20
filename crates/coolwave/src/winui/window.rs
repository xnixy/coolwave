//! The window: what it keeps, what changes it, and the frame every page sits in (the title bar
//! and the sidebar, with what the lights are doing at its top).

use super::cooling::CoolingState;
use super::core::{Core, Shared};
use super::lighting::FanColorGrid;
use super::{WindowInput, autostart, icon_file};
use coolwave_core::effects::Effect;
use coolwave_core::engine::Identify;
use coolwave_pump::{PumpInput, PumpPage};
use coolwave_ui::*;
use serde::{Deserialize, Serialize};
use std::cell::{Ref, RefMut};
use std::time::Duration;
use windows_reactor::*;

/// The title bar's icon.
const ICON_PNG: &[u8] = include_bytes!("../../../../data/icons/coolwave-32.png");
/// How long a short message stays at the bottom of the window.
const TOAST_FOR: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
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

    fn key(self) -> &'static str {
        match self {
            Page::Lighting => "lighting",
            Page::Cooling => "cooling",
            Page::PumpScreen => "pump",
            Page::Settings => "settings",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Lighting => "\u{E790}",
            Page::Cooling => "\u{E9CA}",
            Page::PumpScreen => "\u{E7F4}",
            Page::Settings => "\u{E713}",
        }
    }

    /// The page named in `COOLWAVE_UI_PAGE`, to open straight onto it while working on it.
    fn from_env() -> Option<Page> {
        let name = std::env::var("COOLWAVE_UI_PAGE").ok()?.to_ascii_lowercase();
        Page::ALL.into_iter().find(|page| page.key().starts_with(&name) || page.label().to_ascii_lowercase().starts_with(&name))
    }
}

/// The three lighting devices, for their switches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Eurux,
    Aura,
    Gpu,
}

/// Which of a fan's three zones a color is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FanZone {
    Left,
    Middle,
    Right,
}

pub struct MainWindow {
    pub(super) core: Shared,
    pub(super) page: Page,
    pub(super) cooling: CoolingState,
    /// Per-fan colors kept while that option is switched off.
    pub(super) remembered_fan_colors: Option<FanColorGrid>,
    pub(super) start_at_sign_in: bool,
    pub(super) toast: Option<String>,
    pub(super) app_icon: EncodedImage,
    /// Where the window opened (its last place, if still on a monitor) and where it is now.
    start_placement: Option<WindowPlacement>,
    placement: Option<WindowPlacement>,
    _toast: Option<ComponentTimer>,
}

#[derive(Clone)]
pub enum Message {
    Page(Page),
    Placement(WindowPlacement),
    ToastDone,
    ToggleLights,
    // Lighting.
    Effect(Effect),
    Primary(Color),
    Secondary(Color),
    Speed(f64),
    Brightness(f64),
    Device(Device, bool),
    Identify(Identify),
    GpuAnimation(Option<usize>),
    GpuSpeed(Option<usize>),
    ZoneOn(u8, bool),
    ZoneLeds(u8, Option<f64>),
    FanColorsOn(bool),
    FanColor(usize, usize, FanZone, Color),
    // Cooling.
    SelectPort(usize),
    Mode(Option<usize>),
    Fixed(f64),
    Source(Option<usize>),
    CurvePress(f64, f64),
    CurveMove(f64, f64),
    CurveRelease,
    PointTemp(usize, Option<f64>),
    PointDuty(usize, Option<f64>),
    Preset(usize),
    // Settings.
    RunInBackground(bool),
    StartAtSignIn(bool),
    Quit,
}

impl Component for MainWindow {
    type Input = WindowInput;
    type Message = Message;

    fn create(input: &WindowInput, _context: &ComponentContext<Self>) -> Self {
        let core = input.core.clone();
        core.0.borrow_mut().window_open = true;
        let cooling = CoolingState::new(&core.0.borrow());
        Self {
            core,
            page: Page::from_env().unwrap_or(Page::Lighting),
            cooling,
            remembered_fan_colors: None,
            start_at_sign_in: autostart::enabled(),
            toast: None,
            app_icon: EncodedImage::from_static(ICON_PNG),
            start_placement: saved_placement(),
            placement: None,
            _toast: None,
        }
    }

    fn input_changed(&mut self, _input: &WindowInput, context: &ComponentContext<Self>) {
        let notice = self.core_mut().notice.take();
        if let Some(text) = notice {
            self.notify(context, text);
        }
        if self.page == Page::Cooling {
            self.cooling.refresh_sensors();
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Page(page) => {
                self.page = page;
                if page == Page::Cooling {
                    self.cooling.refresh_sensors();
                }
            }
            // Saved as it changes: a closing window gets no last word on where it was.
            Message::Placement(placement) => {
                self.placement = Some(placement);
                save_placement(placement);
            }
            Message::ToastDone => self.toast = None,
            Message::ToggleLights => {
                let mut core = self.core_mut();
                let on = core.lighting_on();
                core.set_lighting(!on);
            }
            Message::RunInBackground(on) => self.core_mut().settings.run_in_background = on,
            Message::StartAtSignIn(on) => {
                if on != self.start_at_sign_in {
                    if let Err(error) = autostart::set(on) {
                        self.notify(context, format!("Could not change start at sign-in: {error}"));
                    }
                    self.start_at_sign_in = autostart::enabled();
                }
            }
            Message::Quit => context.input().quit.call(()),
            message @ (Message::Effect(_)
            | Message::Primary(_)
            | Message::Secondary(_)
            | Message::Speed(_)
            | Message::Brightness(_)
            | Message::Device(..)
            | Message::Identify(_)
            | Message::GpuAnimation(_)
            | Message::GpuSpeed(_)
            | Message::ZoneOn(..)
            | Message::ZoneLeds(..)
            | Message::FanColorsOn(_)
            | Message::FanColor(..)) => self.lighting_update(message),
            message => self.cooling_update(message),
        }
    }

    fn view(&self, input: &WindowInput, context: &mut ViewContext<Self>) -> View {
        context.window_title("Coolwave");
        let mut visuals = WindowVisuals::new()
            .client_size(1180.0, 800.0)
            .constraints(WindowConstraints { min_width: Some(1000.0), min_height: Some(660.0), ..Default::default() })
            .backdrop(WindowBackdrop::Mica)
            .theme(WindowTheme::System)
            .icon(icon_file());
        if let Some(placement) = self.start_placement {
            visuals = visuals.initial_placement(placement);
        }
        context.window_visuals(visuals);
        context.on_window_placement(context.callback(Message::Placement));

        let page: View = match self.page {
            Page::Lighting => self.lighting_page(context),
            Page::Cooling => self.cooling_page(context),
            Page::PumpScreen => View::component::<PumpPage>(PumpInput { pump: input.pump.clone(), revision: input.revision }),
            Page::Settings => self.settings_page(context),
        };
        let toast: View = match &self.toast {
            Some(text) => InfoBar::new()
                .message(text.clone())
                .severity(InfoBarSeverity::Informational)
                .is_open(true)
                .is_closable(true)
                .on_closed(context.message(Message::ToastDone))
                .horizontal_alignment(HorizontalAlignment::Center)
                .vertical_alignment(VerticalAlignment::Bottom)
                .max_width(560.0)
                .margin(Thickness::new(16.0, 0.0, 16.0, 16.0))
                .into(),
            None => nothing(),
        };
        let title_bar = TitleBar::new().title("Coolwave").icon(Icon::image_data(self.app_icon.clone())).grid_row(0);
        let body = Grid::new().columns([GridLength::Auto, GridLength::STAR]).grid_row(1).children((
            self.sidebar(context),
            Grid::new().grid_column(1).children((Grid::new().keyed_children([keyed(self.page.key(), page)]), toast)),
        ));
        Grid::new().rows([GridLength::Auto, GridLength::STAR]).children((title_bar, body)).into()
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        // Closing, to the tray or for good: what changed is written now.
        if let Ok(mut core) = self.core.0.try_borrow_mut() {
            core.window_open = false;
            core.save();
        }
        if let Some(placement) = self.placement {
            save_placement(placement);
        }
    }
}

impl MainWindow {
    pub(super) fn core(&self) -> Ref<'_, Core> {
        self.core.0.borrow()
    }

    pub(super) fn core_mut(&self) -> RefMut<'_, Core> {
        self.core.0.borrow_mut()
    }

    pub(super) fn notify(&mut self, context: &ComponentContext<Self>, text: String) {
        self.toast = Some(text);
        self._toast = Some(context.set_local_timeout(TOAST_FOR, || Message::ToastDone));
    }

    /// Closing keeps Coolwave in the tray.
    pub(super) fn closes_to_tray(&self) -> bool {
        self.core().settings.run_in_background
    }

    fn sidebar(&self, context: &ViewContext<Self>) -> View {
        let mut top: Vec<View> = vec![self.status_card(context)];
        for page in [Page::Lighting, Page::Cooling, Page::PumpScreen] {
            top.push(self.nav(context, page));
        }
        let note = if self.closes_to_tray() { "Closing keeps Coolwave in the tray." } else { "Closing stops the effects." };
        Grid::new()
            .width(248.0)
            .margin(Thickness::new(12.0, 0.0, 12.0, 16.0))
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .children((
                StackPanel::new().spacing(2.0).children(top),
                StackPanel::new().grid_row(2).spacing(10.0).children((
                    self.nav(context, Page::Settings),
                    StackPanel::new()
                        .margin(Thickness::new(12.0, 0.0, 12.0, 0.0))
                        .children((caption(note), caption(&format!("Version {}", env!("CARGO_PKG_VERSION"))))),
                )),
            ))
            .into()
    }

    /// What the lights are doing right now, with a switch for them.
    fn status_card(&self, context: &ViewContext<Self>) -> View {
        let core = self.core();
        let on = core.lighting_on();
        let dot: View = if on {
            Ellipse::new().width(8.0).height(8.0).fill(ThemeBrush::Accent).vertical_alignment(VerticalAlignment::Center).into()
        } else {
            Ellipse::new()
                .width(8.0)
                .height(8.0)
                .stroke(ThemeBrush::SecondaryText)
                .stroke_thickness(1.5)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        };
        let heading = if on { core.chosen_effect().name().to_string() } else { "Lights off".to_string() };
        let words = if on {
            "Playing in sync on every device you turned on."
        } else {
            "Every light is dark. The effect comes back when you turn them on."
        };
        let label = if on { "Lights off" } else { "Lights on" };
        let mut button = Button::new()
            .automation_name(label)
            .on_click(context.message(Message::ToggleLights))
            .content(with_glyph(if on { "\u{E708}" } else { "\u{E706}" }, label));
        if !on {
            button = button.style(ButtonStyle::Accent);
        }
        card(
            Thickness::new(14.0, 12.0, 14.0, 12.0),
            StackPanel::new().spacing(10.0).children((line(8.0, (dot, strong(&heading))), caption(words), button)),
        )
        .margin(Thickness::new(0.0, 4.0, 0.0, 14.0))
        .into()
    }

    fn nav(&self, context: &ViewContext<Self>, page: Page) -> View {
        let selected = self.page == page;
        let mut button = Button::new()
            .style(ButtonStyle::Subtle)
            .height(40.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .horizontal_content_alignment(HorizontalAlignment::Left)
            .automation_name(page.label())
            .on_click(context.message(Message::Page(page)))
            .content(line(
                14.0,
                (
                    glyph(page.glyph(), 16.0).vertical_alignment(VerticalAlignment::Center),
                    TextBlock::new().text(page.label()).vertical_alignment(VerticalAlignment::Center),
                ),
            ));
        if selected {
            button = button.background(ThemeBrush::SubtleSecondary);
        }
        let pill: View = if selected {
            Border::new()
                .width(3.0)
                .height(16.0)
                .corner_radius(2.0)
                .background(ThemeBrush::Accent)
                .horizontal_alignment(HorizontalAlignment::Left)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        } else {
            nothing()
        };
        Grid::new().children((button, pill)).into()
    }
}

/// The color WinUI uses and the engine's.
pub(super) fn rgb_of(color: Color) -> coolwave_core::Rgb {
    coolwave_core::Rgb::new(color.r, color.g, color.b)
}

pub(super) fn color_of_rgb(rgb: coolwave_core::Rgb) -> Color {
    Color::rgb(rgb.r, rgb.g, rgb.b)
}

// -------------------------------------------------------------------------------------------
// Where the window was.

/// The window's place, as it is kept on disk between runs.
#[derive(Serialize, Deserialize)]
struct SavedPlacement {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    maximized: bool,
}

fn placement_path() -> std::path::PathBuf {
    coolwave_core::settings::config_dir().join("window.toml")
}

/// The window's last place, if it is still on a monitor: one unplugged since, or a resolution
/// change, opens it where Windows puts new windows instead.
fn saved_placement() -> Option<WindowPlacement> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    let text = std::fs::read_to_string(placement_path()).ok()?;
    let saved: SavedPlacement = toml::from_str(&text).ok()?;
    if saved.width < 200 || saved.height < 200 {
        return None;
    }
    let bounds = RECT { left: saved.x, top: saved.y, right: saved.x + saved.width, bottom: saved.y + saved.height };
    // SAFETY: a plain query on a rectangle that lives across the call.
    if unsafe { MonitorFromRect(&bounds, MONITOR_DEFAULTTONULL) }.is_null() {
        return None;
    }
    Some(WindowPlacement { x: saved.x, y: saved.y, width: saved.width, height: saved.height, maximized: saved.maximized })
}

fn save_placement(placement: WindowPlacement) {
    if super::core::preview() {
        return;
    }
    let saved = SavedPlacement {
        x: placement.x,
        y: placement.y,
        width: placement.width,
        height: placement.height,
        maximized: placement.maximized,
    };
    let path = placement_path();
    if let (Some(folder), Ok(text)) = (path.parent(), toml::to_string(&saved)) {
        let _ = std::fs::create_dir_all(folder);
        let _ = std::fs::write(&path, text);
    }
}
