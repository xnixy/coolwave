//! The Screen tab (brightness, rotation, power and firmware) and the Settings tab.

use super::parts::{labeled, switch_line};
use super::{Kind, Message, PumpCore, PumpPage};
use coolwave_ui::*;
use windows_reactor::*;

/// The turns the screen can have, as the rotation choice lists them.
pub(super) const ROTATIONS: [u16; 4] = [0, 90, 180, 270];
const USB_ID: &str = "0B05:1DE7";

impl PumpPage {
    pub(super) fn screen_tab(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        columns(1.0, 1.0, self.display_card(pump, context), self.pump_card(pump, context))
    }

    fn display_card(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let idle = !core.busy;
        let shot = core.snapshot.as_ref();
        let screen_on = shot.is_none_or(|shot| shot.screen_on);
        let rotation = shot.map(|shot| shot.rotation);
        let mut parts: Vec<View> = vec![
            Grid::new()
                .columns([GridLength::STAR, GridLength::Auto])
                .children((
                    strong("Display").vertical_alignment(VerticalAlignment::Center),
                    Button::new()
                        .grid_column(1)
                        .content("Refresh")
                        .is_enabled(idle)
                        .on_click(context.message(Message::Refresh)),
                ))
                .into(),
            labeled(
                "Brightness",
                slider(
                    core.brightness as f64,
                    Range::new(0.0, 100.0, 1.0),
                    format!("{}%", core.brightness),
                    "Brightness",
                    screen_on,
                    context.callback(Message::Brightness),
                ),
            ),
            labeled(
                "Rotation",
                ComboBox::new()
                    .automation_name("Rotation")
                    .min_width(120.0)
                    .items_source(ROTATIONS.iter().map(|degrees| format!("{degrees}°")))
                    .is_enabled(idle && rotation.is_some())
                    .selected_index(rotation.and_then(|rotation| ROTATIONS.iter().position(|r| *r == rotation)))
                    .on_selection_changed(context.callback(Message::Rotation)),
            ),
        ];
        match shot {
            None => parts.push(muted("Connect the pump to change these.").into()),
            Some(shot) => {
                parts.push(switch_line(
                    shot.screen_on,
                    idle,
                    "Screen on",
                    "Off keeps the panel dark until you turn it back on.",
                    context.callback(Message::ScreenOn),
                ));
                parts.push(switch_line(
                    shot.display_in_sleep,
                    idle,
                    "Standby clip while the computer sleeps",
                    "Off turns the panel off while the computer sleeps.",
                    context.callback(Message::Standby),
                ));
            }
        }
        parts.push(
            line(
                8.0,
                (
                    Button::new()
                        .content("Wake")
                        .is_enabled(idle)
                        .on_click(context.message(Message::Power("resume")))
                        .tooltip("Wake the screen if it went to sleep."),
                    Button::new()
                        .content("Sleep")
                        .is_enabled(idle)
                        .on_click(context.message(Message::Power("suspend")))
                        .tooltip("Put the screen to sleep, like when the computer sleeps."),
                ),
            )
            .into(),
        );
        padded_card(StackPanel::new().spacing(10.0).children(parts)).into()
    }

    fn pump_card(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let shot = core.snapshot.as_ref();
        let read = |value: fn(&crate::engine::Snapshot) -> String| shot.map_or("-".to_string(), value);
        let facts = [
            ("Model", "ROG Strix LC IV".to_string()),
            ("Serial", read(|shot| shot.serial.clone())),
            ("Firmware", read(|shot| shot.firmware.clone())),
            ("Free space", read(|shot| shot.free_space.clone())),
            ("USB", USB_ID.to_string()),
        ];
        let mut cells: Vec<View> = Vec::new();
        for (row, (name, value)) in facts.iter().enumerate() {
            cells.push(muted(name).grid_row(row as i32).into());
            cells.push(
                TextBlock::new()
                    .text(value)
                    .is_text_selection_enabled(true)
                    .grid_row(row as i32)
                    .grid_column(1)
                    .into(),
            );
        }
        let ready = core.snapshot.is_some() && !core.busy && !core.asus_running && !self.picking;
        padded_card(StackPanel::new().spacing(10.0).children((
            strong("This pump"),
            Grid::new()
                .columns([GridLength::Auto, GridLength::STAR])
                .rows(facts.iter().map(|_| GridLength::Auto))
                .column_spacing(18.0)
                .row_spacing(8.0)
                .children(cells),
            caption("The pump and fans run from the motherboard. Coolwave only drives the screen."),
            strong("Firmware").margin(Thickness::new(0.0, 6.0, 0.0, 0.0)),
            muted(
                "Install a firmware package downloaded from ASUS. It is checked first, then sent the same way the \
                 ASUS app sends it.",
            ),
            Button::new()
                .content("Update firmware…")
                .is_enabled(ready)
                .on_click(context.message(Message::Choose(Kind::Firmware))),
        )))
        .into()
    }

    pub(super) fn settings_tab(&self, pump: &PumpCore, context: &ViewContext<Self>) -> View {
        let core = &pump.core;
        let startup = padded_card(StackPanel::new().spacing(6.0).children((
            strong("Startup"),
            switch_line(
                core.settings.resume_last,
                true,
                "Play the last picture when Coolwave starts",
                "Sends the last photo or video again on its own.",
                context.callback(Message::ResumeLast),
            ),
            switch_line(
                core.settings.sleep_when_locked,
                true,
                "Sleep the screen while locked",
                "Wakes it again when you unlock.",
                context.callback(Message::SleepWhenLocked),
            ),
        )));
        let settings_file = crate::settings::settings_path().display().to_string();
        let readings = padded_card(StackPanel::new().spacing(8.0).children((
            strong("Readings"),
            labeled(
                "Temperature",
                ComboBox::new()
                    .automation_name("Temperature unit")
                    .min_width(120.0)
                    .items_source(["°C", "°F"])
                    .selected_index(Some(usize::from(core.settings.fahrenheit)))
                    .on_selection_changed(context.callback(Message::Fahrenheit)),
            ),
            muted(
                "CPU temperature and power, pump speed, coolant, and fan speeds come from HWiNFO. Run it with \
                 Shared Memory Support turned on in its settings. GPU readings need an NVIDIA card.",
            ),
            caption("Settings file"),
            caption(&settings_file)
                .text_wrapping(TextWrapping::NoWrap)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .is_text_selection_enabled(true)
                .tooltip(settings_file.clone()),
        )));
        columns(1.0, 1.0, startup, readings)
    }
}
