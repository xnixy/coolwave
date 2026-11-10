//! Picking a file: the system dialog on Windows, a typed path elsewhere, so the
//! Linux build of the egui page needs no dialog library.

use coolwave_ui::egui;
#[cfg(not(windows))]
use coolwave_ui::widgets;
use std::path::PathBuf;

/// What the file is for. Decides the dialog's filter.
#[derive(Clone, Copy)]
pub enum Kind {
    Picture,
    Firmware,
}

/// Paths typed into the fields that stand in for a file dialog.
#[derive(Default)]
pub struct TypedPaths {
    #[cfg(not(windows))]
    picture: String,
    #[cfg(not(windows))]
    firmware: String,
}

#[cfg(not(windows))]
impl TypedPaths {
    fn field(&mut self, kind: Kind) -> &mut String {
        match kind {
            Kind::Picture => &mut self.picture,
            Kind::Firmware => &mut self.firmware,
        }
    }
}

#[cfg(windows)]
const PICTURE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "mp4", "mov", "avi", "mkv", "webm",
];

/// Smallest the path field gets, however long the button's label is.
#[cfg(not(windows))]
const MIN_FIELD_WIDTH: f32 = 80.0;

/// A button (Windows) or a path field with a button (elsewhere).
/// Returns the file once one is chosen.
pub fn choose(
    ui: &mut egui::Ui,
    typed: &mut TypedPaths,
    kind: Kind,
    label: &str,
    enabled: bool,
) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = typed;
        ui.add_enabled(enabled, egui::Button::new(label))
            .clicked()
            .then(|| dialog(kind))
            .flatten()
    }
    #[cfg(not(windows))]
    {
        let text = typed.field(kind);
        let mut chosen = None;
        ui.horizontal(|ui| {
            let font = egui::TextStyle::Button.resolve(ui.style()).size;
            let button_room = widgets::text_width(ui, label, font)
                + ui.spacing().button_padding.x * 2.0
                + ui.spacing().item_spacing.x;
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text("Path to the file")
                    .desired_width((ui.available_width() - button_room).max(MIN_FIELD_WIDTH)),
            );
            let path = PathBuf::from(text.trim());
            if ui
                .add_enabled(enabled && path.is_file(), egui::Button::new(label))
                .clicked()
            {
                chosen = Some(path);
            }
        });
        chosen
    }
}

#[cfg(windows)]
fn dialog(kind: Kind) -> Option<PathBuf> {
    let dialog = match kind {
        Kind::Picture => rfd::FileDialog::new().add_filter("Photos and video", PICTURE_EXTENSIONS),
        Kind::Firmware => rfd::FileDialog::new().add_filter("ASUS firmware package", &["zip"]),
    };
    dialog.pick_file()
}
