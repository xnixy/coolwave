//! "Start at login" via an XDG autostart entry.

use std::io;
use std::path::PathBuf;

fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart/io.github.xnixy.Coolwave.desktop")
}

pub fn enabled() -> bool {
    path().exists()
}

pub fn set(on: bool) -> io::Result<()> {
    let p = path();
    if !on {
        return match std::fs::remove_file(&p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        };
    }
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(p.parent().expect("has parent"))?;
    std::fs::write(
        &p,
        format!(
            "[Desktop Entry]\nType=Application\nName=Coolwave\nComment=Lighting and cooling control\nExec=\"{}\" --background\nIcon=io.github.xnixy.Coolwave\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        ),
    )
}
