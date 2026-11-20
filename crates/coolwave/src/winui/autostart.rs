//! Start at sign-in: an entry in the user's Run key.

use std::io;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "Coolwave";

pub fn enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|run| run.get_value::<String, _>(VALUE))
        .is_ok()
}

pub fn set(on: bool) -> io::Result<()> {
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
    if on {
        // Sign-in starts Coolwave in the tray.
        let exe = std::env::current_exe()?;
        run.set_value(VALUE, &format!("\"{}\" --background", exe.display()))
    } else {
        match run.delete_value(VALUE) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
