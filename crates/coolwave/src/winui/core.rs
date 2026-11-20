//! What lives as long as Coolwave does, whether or not the window is open: the lighting and fan
//! engine and the settings. The window is made again each time it opens, so it works on this
//! through a [`Shared`] handle.

use coolwave_core::effects::Effect;
use coolwave_core::engine::Engine;
use coolwave_core::settings::Settings;
use std::cell::RefCell;
use std::rc::Rc;

/// With `COOLWAVE_UI_PREVIEW` set, the window opens without touching any device or the settings
/// file, and next to a running Coolwave. For working on the pages and taking pictures.
pub fn preview() -> bool {
    std::env::var_os("COOLWAVE_UI_PREVIEW").is_some()
}

pub struct Core {
    pub engine: Engine,
    pub settings: Settings,
    /// Effect to go back to when the lights are switched on again.
    effect_before_off: Effect,
    /// The settings file as last written, to save only what changed.
    last_saved: String,
    /// A short message for the window to show, from something done outside it.
    pub notice: Option<String>,
    /// The window is open (it says so as it opens and closes).
    pub window_open: bool,
}

impl Core {
    pub fn start() -> Self {
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
        // Empty when no file exists yet, so the first save writes one.
        let last_saved = if coolwave_core::settings::path().exists() { toml_of(&settings) } else { String::new() };
        Self { engine, settings, effect_before_off, last_saved, notice: None, window_open: false }
    }

    pub fn lighting_on(&self) -> bool {
        self.engine.config().params.effect != Effect::Off
    }

    /// The effect the lights play, or would play once switched on.
    pub fn chosen_effect(&self) -> Effect {
        match self.engine.config().params.effect {
            Effect::Off => self.effect_before_off,
            effect => effect,
        }
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

    /// Write the settings if they changed since the last time.
    pub fn save(&mut self) {
        if preview() {
            return;
        }
        self.settings.lighting = self.engine.config();
        let text = toml_of(&self.settings);
        if text == self.last_saved {
            return;
        }
        match self.settings.save() {
            Ok(()) => self.last_saved = text,
            Err(error) => eprintln!("coolwave: failed to save settings: {error}"),
        }
    }
}

fn toml_of(settings: &Settings) -> String {
    toml::to_string(settings).unwrap_or_default()
}

/// The core, compared by identity so it can be part of the window's input.
#[derive(Clone)]
pub struct Shared(pub Rc<RefCell<Core>>);

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
