//! The Settings page: how the app starts and runs.

use super::Ui;
use crate::settings;
use adw::prelude::*;
use std::rc::Rc;

pub fn page(ui: &Rc<Ui>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();

    // Tray, background running and start at login are Coolwave-wide (header menu).
    let startup = adw::PreferencesGroup::builder()
        .title("Startup")
        .description("Background running and start at login are in Coolwave's main menu.")
        .build();
    let resume = adw::SwitchRow::builder()
        .title("Play the last picture when Coolwave starts")
        .subtitle("Sends the last photo or video again on its own.")
        .build();
    startup.add(&resume);
    {
        let ui = Rc::clone(ui);
        resume.connect_active_notify(move |row| {
            let on = row.is_active();
            ui.act(|core| {
                core.settings.resume_last = on;
                core.persist();
            });
        });
    }
    page.add(&startup);

    let locking = adw::PreferencesGroup::builder().title("Locking").build();
    let follow = adw::SwitchRow::builder()
        .title("Sleep the screen while locked")
        .subtitle("Wakes it again when you unlock.")
        .build();
    {
        let ui = Rc::clone(ui);
        follow.connect_active_notify(move |row| {
            let on = row.is_active();
            ui.act(|core| core.set_sleep_when_locked(on));
        });
    }
    locking.add(&follow);
    page.add(&locking);

    let readings = adw::PreferencesGroup::builder()
        .title("Readings")
        .description("CPU temperature comes from the kernel. GPU readings need an NVIDIA card.")
        .build();
    let units = adw::ComboRow::builder()
        .title("Temperature")
        .model(&gtk::StringList::new(&["°C", "°F"]))
        .build();
    {
        let ui = Rc::clone(ui);
        units.connect_selected_notify(move |row| {
            let fahrenheit = row.selected() == 1;
            ui.act(|core| {
                core.settings.fahrenheit = fahrenheit;
                core.persist();
            });
        });
    }
    readings.add(&units);
    let file = adw::ActionRow::builder()
        .title("Settings file")
        .subtitle(settings::settings_path().display().to_string())
        .subtitle_selectable(true)
        .build();
    file.add_css_class("property");
    readings.add(&file);
    page.add(&readings);

    ui.on_refresh(move |ui| {
        let core = ui.core.borrow();
        let settings = &core.settings;
        resume.set_active(settings.resume_last);
        follow.set_active(settings.sleep_when_locked);
        units.set_selected(u32::from(settings.fahrenheit));
    });
    page.upcast()
}
