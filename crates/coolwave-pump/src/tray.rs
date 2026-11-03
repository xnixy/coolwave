//! In Coolwave the app-wide tray icon (and its menu) replaces the pump screen's own.
//! This keeps the controller's interface: `available` says the app keeps running
//! after the window closes, and `TrayAction`s are sent by Coolwave if ever needed.

use std::sync::Arc;

/// Called from another thread so the window looks at `tray_rx` soon.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[allow(dead_code)]
pub enum TrayAction {
    Show,
    Stop,
    Quit,
}

pub struct TrayMenu {
    pub available: bool,
}

impl TrayMenu {
    pub fn build(_tx: std::sync::mpsc::SyncSender<TrayAction>, _wake: Wake) -> Self {
        // Coolwave always has a tray icon and keeps running in the background.
        Self { available: true }
    }
}
