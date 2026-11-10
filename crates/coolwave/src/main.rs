// A windowed program on Windows: no console opens behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(egui_ui)]
mod glass;
#[cfg(not(egui_ui))]
mod linux;

#[cfg(not(egui_ui))]
fn main() -> gtk::glib::ExitCode {
    linux::run()
}

#[cfg(egui_ui)]
fn main() {
    // `--background` (or `-b`) starts in the tray, as the autostart entry does.
    let in_tray = std::env::args().skip(1).any(|arg| arg == "--background" || arg == "-b");
    if let Err(error) = glass::run(in_tray) {
        eprintln!("coolwave: {error}");
        std::process::exit(1);
    }
}
