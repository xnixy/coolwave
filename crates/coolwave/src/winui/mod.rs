//! Coolwave's window and tray icon on Windows: native WinUI 3 through Microsoft's windows-reactor
//! (vendor/windows-reactor), in Blurman's look. One Mica surface, a sidebar with what the lights
//! are doing at its top, and Windows Settings-style rows in cards.
//!
//! The engine and the pump screen keep running while the window is closed to the tray, so they
//! live with the application ([`core::Core`], [`coolwave_pump::PumpCore`]) and the window, made
//! again each time it opens, works on them through shared handles. The application's own clock
//! (a timer on [`bridge::Bridge`]'s hidden window) ticks the pump, saves the settings and has the
//! window draw its live readings again.

mod autostart;
mod bridge;
mod cooling;
mod core;
mod instance;
mod lighting;
mod settings;
mod window;

use self::core::{Core, Shared, preview};
use coolwave_core::effects::Effect;
use coolwave_pump::{PumpCore, PumpHandle};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use windows_reactor::*;

/// The app's icon, for the window, the taskbar and the tray.
const APP_ICO: &[u8] = include_bytes!("../../../../data/coolwave.ico");
/// How often changed settings are written to disk.
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// How often the clock ticks: while the pump worker is busy, while the window shows live
/// readings, and while Coolwave only sits in the tray.
const TICK_BUSY_MS: u32 = 200;
const TICK_OPEN_MS: u32 = 500;
const TICK_TRAY_MS: u32 = 2000;

/// Run the window and the tray icon until Quit, or until the window closes while Coolwave does
/// not keep to the tray. With `in_tray`, start in the tray.
pub fn run(in_tray: bool) -> Result<(), String> {
    if !preview() && !instance::claim(bridge::ask_to_show) {
        // Coolwave is already running and was asked to show its window.
        return Ok(());
    }
    App::run_application::<Coolwave>(in_tray).map_err(|error| format!("the window could not open: {error}"))
}

/// Say why Coolwave could not start: there is no console to print it to.
pub fn report(error: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let text = wide(&format!("Coolwave could not start: {error}"));
    let caption = wide("Coolwave");
    // SAFETY: both strings are valid, 0-terminated wide strings that outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR) };
}

/// The icon file Windows wants by path (the window's and the tray's), written once to the
/// temp folder.
pub(crate) fn icon_file() -> &'static str {
    static FILE: OnceLock<String> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::temp_dir().join("coolwave-app.ico");
        if std::fs::read(&path).ok().as_deref() != Some(APP_ICO) {
            let _ = std::fs::write(&path, APP_ICO);
        }
        path.to_string_lossy().into_owned()
    })
}

/// What the window gets: the shared state, a number that changes on every tick so it draws the
/// live readings again, and how to quit.
#[derive(Clone, PartialEq)]
pub struct WindowInput {
    pub core: Shared,
    pub pump: PumpHandle,
    pub revision: u64,
    pub quit: Callback<()>,
}

/// The application: one reopenable window, and the tray icon while Coolwave keeps to the tray.
struct Coolwave {
    core: Shared,
    pump: PumpHandle,
    bridge: bridge::Bridge,
    /// Started at sign-in: lives in the tray whatever the setting says.
    started_in_tray: bool,
    revision: u64,
    saved_at: Instant,
}

#[derive(Clone)]
pub enum AppMessage {
    Tick,
    /// Show the window: a second launch, or a click on the tray icon.
    Show,
    Menu(Key),
    Quit,
}

impl Coolwave {
    /// Closing the window keeps Coolwave in the tray.
    fn keeps_to_tray(&self) -> bool {
        self.started_in_tray || self.core.0.borrow().settings.run_in_background
    }

    fn tick(&mut self) {
        let open = self.core.0.borrow().window_open;
        let mut pump = self.pump.0.borrow_mut();
        pump.tick(open);
        let waiting = pump.waiting();
        drop(pump);
        if self.saved_at.elapsed() >= SAVE_EVERY {
            self.saved_at = Instant::now();
            self.core.0.borrow_mut().save();
        }
        if open {
            self.revision += 1;
        }
        self.bridge.tick_every(match (waiting, open) {
            (true, _) => TICK_BUSY_MS,
            (false, true) => TICK_OPEN_MS,
            (false, false) => TICK_TRAY_MS,
        });
    }

    fn quit(&mut self, context: &ApplicationContext<Self>) {
        self.core.0.borrow_mut().save();
        context.exit();
    }

    fn menu(&self, context: &ApplicationViewContext<Self>) -> Menu {
        let core = self.core.0.borrow();
        let on = core.lighting_on();
        let current = core.chosen_effect();
        let effects = Effect::ALL.into_iter().enumerate().filter(|(_, effect)| *effect != Effect::Off).map(|(index, effect)| {
            // The menu has no check marks; the effect playing has one in its label.
            let label = if on && effect == current { format!("✓  {}", effect.name()) } else { format!("     {}", effect.name()) };
            MenuItem::item(format!("effect:{index}"), label)
        });
        Menu::new(
            vec![
                MenuItem::item("show", "Show Coolwave"),
                MenuItem::separator("s1"),
                MenuItem::item("lights", if on { "Turn the lights off" } else { "Turn the lights on" }),
                MenuItem::submenu("effects", "Effect", effects),
                MenuItem::separator("s2"),
                MenuItem::item("quit", "Quit"),
            ],
            context.callback(AppMessage::Menu),
        )
    }

    fn tooltip(&self) -> String {
        let core = self.core.0.borrow();
        if core.lighting_on() { format!("Coolwave: {}", core.chosen_effect().name()) } else { "Coolwave: lights off".to_string() }
    }
}

impl Application for Coolwave {
    type Input = bool;
    type Message = AppMessage;

    fn create(in_tray: &bool, context: &ApplicationContext<Self>) -> Self {
        let bridge = bridge::Bridge::start(context.sender()).expect("the hidden window for the app's clock");
        let core = Shared(Rc::new(RefCell::new(Core::start())));
        let pump = PumpHandle::new(PumpCore::new(Arc::new(bridge::ask_to_show)));
        if !*in_tray || bridge.take_early_show() {
            context.show_window("main");
        }
        bridge.tick_every(TICK_BUSY_MS);
        Self { core, pump, bridge, started_in_tray: *in_tray, revision: 0, saved_at: Instant::now() }
    }

    fn update(&mut self, message: AppMessage, context: &ApplicationContext<Self>) {
        match message {
            AppMessage::Tick => return self.tick(),
            AppMessage::Show => context.show_window("main"),
            AppMessage::Quit => self.quit(context),
            AppMessage::Menu(key) => match key.as_str() {
                Some("show") => context.show_window("main"),
                Some("lights") => {
                    let mut core = self.core.0.borrow_mut();
                    let on = core.lighting_on();
                    core.set_lighting(!on);
                }
                Some("quit") => self.quit(context),
                Some(other) => {
                    let effect = other.strip_prefix("effect:").and_then(|index| index.parse::<usize>().ok()).and_then(|index| Effect::ALL.get(index).copied());
                    if let Some(effect) = effect {
                        self.core.0.borrow_mut().set_effect(effect);
                    }
                }
                None => {}
            },
        }
        // What the tray changed shows in the window at once, not on the next tick.
        self.revision += 1;
    }

    fn view(&self, _input: &bool, context: &ApplicationViewContext<Self>) -> ApplicationView {
        let view = ApplicationView::new().window::<window::MainWindow>(
            "main",
            WindowInput { core: self.core.clone(), pump: self.pump.clone(), revision: self.revision, quit: context.message(AppMessage::Quit) },
        );
        if !self.keeps_to_tray() {
            return view;
        }
        view.notify_icon(
            "tray",
            NotifyIcon::new(icon_file())
                .tooltip(self.tooltip())
                .on_activate(context.callback(|_| AppMessage::Show))
                .menu(self.menu(context)),
        )
    }
}

impl Drop for Coolwave {
    fn drop(&mut self) {
        if let Ok(mut core) = self.core.0.try_borrow_mut() {
            core.save();
        }
    }
}
