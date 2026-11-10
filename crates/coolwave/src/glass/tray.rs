//! The tray icon and its menu: show the window, switch the lights, pick an
//! effect, quit. Clicks arrive as `Command`s the window reads each frame.

use super::Wake;
use coolwave_core::effects::Effect;

// Only the Windows tray sends these. The other build has no tray.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Command {
    Show,
    /// Flip the lights. The window knows which way.
    ToggleLighting,
    SetEffect(Effect),
    Quit,
}

#[cfg(windows)]
pub use windows::Tray;

#[cfg(not(windows))]
pub use none::Tray;

#[cfg(windows)]
mod windows {
    use super::{Command, Effect, Wake};
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    const ICON_PNG: &[u8] = include_bytes!("../../../../data/icons/coolwave-32.png");
    const ID_SHOW: &str = "show";
    const ID_LIGHTS: &str = "lights";
    const ID_QUIT: &str = "quit";
    const ID_EFFECT_PREFIX: &str = "effect:";
    /// More clicks than this between two frames are dropped.
    const QUEUE: usize = 16;

    pub struct Tray {
        commands: Receiver<Command>,
        lights: CheckMenuItem,
        effects: Vec<(Effect, CheckMenuItem)>,
        /// What the menu shows now, to touch it only on a change.
        shown: Option<(bool, Effect)>,
        /// None when Windows refused the icon. The app then closes like a normal window.
        _icon: Option<TrayIcon>,
    }

    fn left_click(event: &TrayIconEvent) -> bool {
        matches!(
            event,
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
        )
    }

    fn icon() -> Option<tray_icon::Icon> {
        let image = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
        let (width, height) = image.dimensions();
        tray_icon::Icon::from_rgba(image.into_raw(), width, height).ok()
    }

    fn command_for(id: &str) -> Option<Command> {
        match id {
            ID_SHOW => Some(Command::Show),
            ID_LIGHTS => Some(Command::ToggleLighting),
            ID_QUIT => Some(Command::Quit),
            _ => {
                let index: usize = id.strip_prefix(ID_EFFECT_PREFIX)?.parse().ok()?;
                Effect::ALL.get(index).copied().map(Command::SetEffect)
            }
        }
    }

    impl Tray {
        pub fn build(wake: Wake) -> Self {
            let (tx, commands): (SyncSender<Command>, _) = sync_channel(QUEUE);
            let lights = CheckMenuItem::with_id(ID_LIGHTS, "Lights on", true, true, None);
            let effect_menu = Submenu::new("Effect", true);
            let mut effects = Vec::new();
            for (index, effect) in Effect::ALL.into_iter().enumerate().filter(|(_, e)| *e != Effect::Off) {
                let item = CheckMenuItem::with_id(format!("{ID_EFFECT_PREFIX}{index}"), effect.name(), true, false, None);
                let _ = effect_menu.append(&item);
                effects.push((effect, item));
            }
            let menu = Menu::new();
            let _ = menu.append(&MenuItem::with_id(ID_SHOW, "Show Coolwave", true, None));
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&lights);
            let _ = menu.append(&effect_menu);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&MenuItem::with_id(ID_QUIT, "Quit", true, None));

            let (click_tx, click_wake) = (tx.clone(), Arc::clone(&wake));
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if left_click(&event) {
                    let _ = click_tx.try_send(Command::Show);
                    click_wake();
                }
            }));
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(command) = command_for(&event.id.0) {
                    let _ = tx.try_send(command);
                    wake();
                }
            }));
            let icon = icon().and_then(|icon| {
                TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)
                    .with_tooltip("Coolwave")
                    .with_icon(icon)
                    .build()
                    .ok()
            });
            Self { commands, lights, effects, shown: None, _icon: icon }
        }

        pub fn available(&self) -> bool {
            self._icon.is_some()
        }

        pub fn next_command(&mut self) -> Option<Command> {
            let command = self.commands.try_recv().ok()?;
            // A click may have flipped a check mark on its own. Set them all again.
            self.shown = None;
            Some(command)
        }

        /// Make the check marks match what the lights are doing.
        pub fn sync(&mut self, lighting_on: bool, effect: Effect) {
            if self.shown == Some((lighting_on, effect)) {
                return;
            }
            self.shown = Some((lighting_on, effect));
            self.lights.set_checked(lighting_on);
            for (item_effect, item) in &self.effects {
                item.set_checked(lighting_on && *item_effect == effect);
            }
        }
    }
}

/// No tray: closing the window quits.
#[cfg(not(windows))]
mod none {
    use super::{Command, Effect, Wake};

    pub struct Tray;

    impl Tray {
        pub fn build(_wake: Wake) -> Self {
            Self
        }

        pub fn available(&self) -> bool {
            false
        }

        pub fn next_command(&mut self) -> Option<Command> {
            None
        }

        pub fn sync(&mut self, _lighting_on: bool, _effect: Effect) {}
    }
}
