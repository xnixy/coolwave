//! The GTK 4 / libadwaita app for Linux.

mod autostart;
mod cooling;
mod desktop;
mod lighting;
mod tray;

use adw::prelude::*;
use coolwave_core::effects::Effect;
use coolwave_core::engine::Engine;
use coolwave_core::settings::Settings;
use gtk::{gio, glib};
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

const APP_ID: &str = "io.github.xnixy.Coolwave";

struct AppState {
    engine: Rc<Engine>,
    settings: RefCell<Settings>,
    last_saved: RefCell<String>,
    _hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    tray: RefCell<Option<ksni::blocking::Handle<tray::CoolTray>>>,
    /// The one main window. It is hidden, never destroyed, so the pump screen
    /// page (which keeps feeding the LCD) lives as long as the app.
    window: RefCell<Option<adw::ApplicationWindow>>,
    pump: RefCell<Option<coolwave_pump::PumpPage>>,
    /// Effect to restore when lighting is switched back on from the tray.
    effect_before_off: RefCell<Effect>,
}

thread_local! {
    static STATE: OnceCell<Rc<AppState>> = const { OnceCell::new() };
    static START_HIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn state() -> Rc<AppState> {
    STATE.with(|s| s.get().expect("app started").clone())
}

/// Hide two known-harmless GTK/GDK warnings (expander icon baselines, frame
/// timings). GTK logs structurally, so this needs a writer function.
fn quiet_known_gtk_warnings() {
    glib::log_set_writer_func(|level, fields| {
        let noisy = fields.iter().any(|f| {
            f.key() == "MESSAGE"
                && f.value_str().is_some_and(|m| m.contains("reported baselines") || m.contains("gdk_frame_timings_presented"))
        });
        if noisy {
            glib::LogWriterOutput::Handled
        } else {
            glib::log_writer_default(level, fields)
        }
    });
}

pub fn run() -> glib::ExitCode {
    quiet_known_gtk_warnings();
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.add_main_option("background", glib::Char::from(b'b'), glib::OptionFlags::NONE, glib::OptionArg::None, "Start without a window (effects keep running)", None);

    app.connect_startup(startup);
    app.connect_handle_local_options(|app, opts| {
        let _ = app;
        if opts.contains("background") {
            // First activation skips the window; later launches show it.
            START_HIDDEN.with(|h| h.set(true));
        }
        std::ops::ControlFlow::Continue(())
    });
    app.connect_activate(|app| {
        let hidden = START_HIDDEN.with(|h| h.replace(false));
        if !hidden {
            present_window(app);
        }
    });
    app.run()
}

fn startup(app: &adw::Application) {
    if let Err(e) = desktop::install() {
        eprintln!("coolwave: could not install icon/launcher: {e}");
    }
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(desktop::icon_theme_dir());
    }
    gtk::Window::set_default_icon_name(desktop::ICON_NAME);

    let settings = Settings::load();
    let engine = Rc::new(Engine::start(settings.fan_layout.clone(), settings.lighting.clone()));
    let hold = settings.run_in_background.then(|| app.hold());
    let st = Rc::new(AppState {
        engine,
        // Empty when no file exists yet, so the first autosave writes one.
        last_saved: RefCell::new(if coolwave_core::settings::path().exists() { toml_of(&settings) } else { String::new() }),
        effect_before_off: RefCell::new(match settings.lighting.params.effect {
            Effect::Off => Effect::Aurora,
            e => e,
        }),
        settings: RefCell::new(settings),
        _hold: RefCell::new(hold),
        tray: RefCell::new(None),
        window: RefCell::new(None),
        pump: RefCell::new(None),
    });
    STATE.with(|s| s.set(st.clone()).ok());

    let effect = st.engine.config().params.effect;
    *st.tray.borrow_mut() = tray::spawn(tray::CoolTray { icons: desktop::tray_pixmaps(), effect, lighting_on: effect != Effect::Off, on_command: on_tray });

    if std::env::var_os("COOLWAVE_DEBUG").is_some() {
        glib::timeout_add_seconds_local(3, || {
            let s = state().engine.status();
            eprintln!(
                "coolwave: eurux [{} {:.0}fps] aura [{} {:.0}fps] gpu [{} {:.0}fps]",
                s.eurux.state, s.eurux.fps, s.aura.state, s.aura.fps, s.gpu.state, s.gpu.fps
            );
            glib::ControlFlow::Continue
        });
    }

    // Autosave lighting changes every 2 s if anything changed.
    glib::timeout_add_seconds_local(2, move || {
        let st = state();
        st.settings.borrow_mut().lighting = st.engine.config();
        // Keep the tray menu in sync with changes made in the window.
        let effect = st.engine.config().params.effect;
        if let Some(h) = st.tray.borrow().as_ref() {
            h.update(|t| {
                if effect != Effect::Off {
                    t.effect = effect;
                }
                t.lighting_on = effect != Effect::Off;
            });
        }
        let text = toml_of(&st.settings.borrow());
        if *st.last_saved.borrow() != text {
            if let Err(e) = st.settings.borrow().save() {
                eprintln!("coolwave: failed to save settings: {e}");
            } else {
                *st.last_saved.borrow_mut() = text;
            }
        }
        glib::ControlFlow::Continue
    });

    // Build the window now (hidden) so the pump screen runs even when Coolwave
    // starts in the background.
    build_window(app);

    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            let st = state();
            st.settings.borrow_mut().lighting = st.engine.config();
            let _ = st.settings.borrow().save();
            st._hold.borrow_mut().take();
            if let Some(h) = st.tray.borrow_mut().take() {
                let _ = h.shutdown();
            }
            app.quit();
        })
        .build();
    app.add_action_entries([quit]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn on_tray(cmd: tray::Command) {
    let Some(app) = gio::Application::default().and_downcast::<adw::Application>() else { return };
    let st = state();
    match cmd {
        tray::Command::Show => present_window(&app),
        tray::Command::SetEffect(e) => {
            *st.effect_before_off.borrow_mut() = e;
            st.engine.update(|c| c.params.effect = e);
        }
        tray::Command::SetLighting(on) => {
            let restore = *st.effect_before_off.borrow();
            let current = st.engine.config().params.effect;
            if !on && current != Effect::Off {
                *st.effect_before_off.borrow_mut() = current;
            }
            st.engine.update(|c| c.params.effect = if on { restore } else { Effect::Off });
        }
        tray::Command::Quit => app.activate_action("quit", None),
    }
}

fn toml_of(s: &Settings) -> String {
    toml::to_string(s).unwrap_or_default()
}

fn present_window(_app: &adw::Application) {
    if let Some(w) = state().window.borrow().as_ref() {
        w.present();
    }
}

fn build_window(app: &adw::Application) {
    let st = state();
    if st.window.borrow().is_some() {
        return;
    }
    let layout = st.settings.borrow().fan_layout.clone();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Coolwave")
        .default_width(1100)
        .default_height(820)
        .hide_on_close(true)
        .build();

    let toasts = adw::ToastOverlay::new();
    let stack = adw::ViewStack::new();
    stack.add_titled_with_icon(&lighting::build(st.engine.clone(), &layout), Some("lighting"), "Lighting", "display-brightness-symbolic");
    stack.add_titled_with_icon(&cooling::build(&toasts, &st.engine, &layout), Some("cooling"), "Cooling", "weather-few-clouds-symbolic");
    let pump = coolwave_pump::page(&window);
    stack.add_titled_with_icon(&pump.widget, Some("pump"), "Pump screen", "video-display-symbolic");
    *st.pump.borrow_mut() = Some(pump);
    toasts.set_child(Some(&stack));

    let header = adw::HeaderBar::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    header.set_title_widget(Some(&switcher));
    header.pack_end(&menu_button(app, &toasts));

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&toasts));
    window.set_content(Some(&view));

    // Without background mode, closing the window quits the app.
    window.connect_close_request(glib::clone!(#[weak] app, #[upgrade_or] glib::Propagation::Proceed, move |_| {
        if !state().settings.borrow().run_in_background {
            app.activate_action("quit", None);
        }
        glib::Propagation::Proceed
    }));
    *st.window.borrow_mut() = Some(window);
}

fn menu_button(app: &adw::Application, toasts: &adw::ToastOverlay) -> gtk::MenuButton {
    let st = state();
    let bg = gio::SimpleAction::new_stateful("run-in-background", None, &st.settings.borrow().run_in_background.to_variant());
    bg.connect_activate(glib::clone!(#[weak] app, move |a, _| {
        let on = !a.state().and_then(|v| v.get::<bool>()).unwrap_or(false);
        a.set_state(&on.to_variant());
        let st = state();
        st.settings.borrow_mut().run_in_background = on;
        *st._hold.borrow_mut() = on.then(|| app.hold());
        let _ = st.settings.borrow().save();
    }));
    let login = gio::SimpleAction::new_stateful("start-at-login", None, &autostart::enabled().to_variant());
    login.connect_activate(glib::clone!(#[weak] toasts, move |a, _| {
        let on = !a.state().and_then(|v| v.get::<bool>()).unwrap_or(false);
        match autostart::set(on) {
            Ok(()) => a.set_state(&on.to_variant()),
            Err(e) => toasts.add_toast(adw::Toast::new(&format!("Autostart: {e}"))),
        }
    }));
    app.add_action(&bg);
    app.add_action(&login);

    let menu = gio::Menu::new();
    menu.append(Some("Keep effects running when closed"), Some("app.run-in-background"));
    menu.append(Some("Start at login"), Some("app.start-at-login"));
    menu.append(Some("Quit"), Some("app.quit"));
    gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).build()
}
