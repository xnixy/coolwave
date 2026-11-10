//! The Settings page: how Coolwave behaves when closed and at sign-in.

use super::{Shared, autostart};
use coolwave_ui::theme;
use coolwave_ui::widgets;
use eframe::egui::Ui;

#[derive(PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
}

pub struct SettingsPage {
    start_at_sign_in: bool,
}

impl SettingsPage {
    pub fn new() -> Self {
        Self { start_at_sign_in: autostart::enabled() }
    }

    pub fn ui(&mut self, ui: &mut Ui, shared: &mut Shared) -> Action {
        let mut action = Action::None;
        widgets::page_header(ui, "Settings", "How Coolwave runs in the background.", |_| {});
        theme::card(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::switch_row(
                ui,
                &mut shared.settings.run_in_background,
                "Keep running when closed",
                "Effects, fan curves and the pump screen carry on from the tray.",
            );
            ui.separator();
            if widgets::switch_row(ui, &mut self.start_at_sign_in, "Start at sign-in", "Coolwave starts in the tray when you sign in.") {
                if let Err(error) = autostart::set(self.start_at_sign_in) {
                    self.start_at_sign_in = !self.start_at_sign_in;
                    shared.toast(format!("Autostart: {error}"));
                }
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.add(widgets::danger_button("Quit Coolwave")).clicked() {
                action = Action::Quit;
            }
            ui.label(theme::muted("Stops the effects and leaves the tray."));
        });
        action
    }
}
