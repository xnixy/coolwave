//! The firmware confirmation, the one question that stops everything else.

use super::PumpPage;
use crate::firmware;
use coolwave_ui::egui;
use coolwave_ui::theme::{self, colors};

const DIALOG_WIDTH: f32 = 440.0;
const TITLE_SIZE: f32 = 18.0;

impl PumpPage {
    /// Confirm a checked firmware package before it is sent.
    pub(super) fn firmware_dialog(&mut self, ctx: &egui::Context) {
        let Some(package) = &self.core.firmware_offer else {
            return;
        };
        let installed = self
            .core
            .snapshot
            .as_ref()
            .map_or("unknown".to_string(), |shot| shot.firmware.clone());
        let same = firmware::parse_version(&installed) == firmware::parse_version(&package.version);
        let title = format!("Install firmware {}?", package.version);
        let mut install = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("pump-firmware-dialog"))
            .frame(theme::dialog())
            .show(ctx, |ui| {
                ui.set_width(DIALOG_WIDTH);
                ui.label(
                    egui::RichText::new(title)
                        .size(TITLE_SIZE)
                        .color(colors().text),
                );
                ui.add_space(6.0);
                let first = if same {
                    format!("The pump already has {installed}. This installs it again.")
                } else {
                    format!("The pump's firmware is {installed} now.")
                };
                for text in [
                    first.as_str(),
                    "The screen goes dark while the pump installs it and restarts. That takes \
                     about half a minute, and Coolwave reconnects on its own.",
                    "Keep the computer awake and the cooler plugged in until the screen is \
                     back. Cooling is not affected: the pump runs from the motherboard.",
                ] {
                    ui.add(egui::Label::new(theme::muted(text)).wrap());
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(theme::primary_button("Install")).clicked() {
                        install = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if install {
            self.core.install_firmware();
        } else if cancel || modal.should_close() {
            self.core.cancel_firmware();
        }
    }
}
