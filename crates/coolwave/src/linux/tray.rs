//! System tray icon (StatusNotifierItem; on GNOME shown by the AppIndicator extension).
//! Menu callbacks run on the tray's D-Bus thread and are forwarded to the GTK main loop.

use coolwave_core::effects::Effect;
use gtk::glib;
use ksni::blocking::TrayMethods;

/// Commands sent from the tray to the GTK main thread.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Show,
    SetEffect(Effect),
    SetLighting(bool),
    Quit,
}

pub struct CoolTray {
    pub icons: Vec<ksni::Icon>,
    pub effect: Effect,
    pub lighting_on: bool,
    pub on_command: fn(Command),
}

impl CoolTray {
    fn send(&self, cmd: Command) {
        let f = self.on_command;
        glib::MainContext::default().invoke(move || f(cmd));
    }
}

impl ksni::Tray for CoolTray {
    fn id(&self) -> String {
        "coolwave".into()
    }

    fn title(&self) -> String {
        "Coolwave".into()
    }

    fn icon_name(&self) -> String {
        // Empty name: hosts must use the pixmaps (no SVG/theme lookup problems).
        if self.icons.is_empty() { super::desktop::ICON_NAME.into() } else { String::new() }
    }

    fn icon_theme_path(&self) -> String {
        super::desktop::icon_theme_dir().display().to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Coolwave".into(),
            description: if self.lighting_on { format!("Lighting: {}", self.effect.name()) } else { "Lighting off".into() },
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let effects: Vec<Effect> = Effect::ALL.iter().copied().filter(|e| *e != Effect::Off).collect();
        let selected = effects.iter().position(|e| *e == self.effect).unwrap_or(0);
        vec![
            StandardItem {
                label: "Open Coolwave".into(),
                icon_name: super::desktop::ICON_NAME.into(),
                activate: Box::new(|t: &mut Self| t.send(Command::Show)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Lighting on".into(),
                checked: self.lighting_on,
                activate: Box::new(|t: &mut Self| {
                    t.lighting_on = !t.lighting_on;
                    t.send(Command::SetLighting(t.lighting_on));
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Effect".into(),
                submenu: vec![RadioGroup {
                    selected,
                    select: Box::new(move |t: &mut Self, i| {
                        if let Some(e) = Effect::ALL.iter().copied().filter(|e| *e != Effect::Off).nth(i) {
                            t.effect = e;
                            t.lighting_on = true;
                            t.send(Command::SetEffect(e));
                        }
                    }),
                    options: effects.iter().map(|e| RadioItem { label: e.name().into(), ..Default::default() }).collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn spawn(tray: CoolTray) -> Option<ksni::blocking::Handle<CoolTray>> {
    match tray.spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("coolwave: tray unavailable: {e}");
            None
        }
    }
}
