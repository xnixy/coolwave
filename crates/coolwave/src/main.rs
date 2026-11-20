// A windowed program on Windows: no console opens behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
mod linux;
#[cfg(windows)]
mod winui;

#[cfg(not(windows))]
fn main() -> gtk::glib::ExitCode {
    linux::run()
}

#[cfg(windows)]
fn main() {
    // `--background` (or `-b`) starts in the tray, as the autostart entry does.
    let in_tray = std::env::args().skip(1).any(|arg| arg == "--background" || arg == "-b");
    if let Err(error) = winui::run(in_tray) {
        winui::report(&error);
        std::process::exit(1);
    }
}
