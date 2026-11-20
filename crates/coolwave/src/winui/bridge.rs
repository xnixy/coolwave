//! How other threads reach the WinUI thread, and the app's own clock. Reactor's application has
//! no sender another thread can use and no timer, so a hidden window does both: other threads
//! (a second launch) post it a message, and a Win32 timer on it ticks. Its handler runs on the UI
//! thread, inside Reactor's message loop, and tells the application.

use super::AppMessage;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows_reactor::LocalSender;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, SetTimer, WM_APP, WM_TIMER};

/// Show the window: a second launch asked for it.
const WM_SHOW: u32 = WM_APP + 1;
const TICK_TIMER: usize = 1;

/// The hidden window, for [`ask_to_show`]; 0 until it exists.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// A request to show the window came before the hidden window existed.
static SHOW_PENDING: AtomicBool = AtomicBool::new(false);

pub struct Bridge {
    window: windows_window::Window,
    interval: Cell<u32>,
}

impl Bridge {
    pub fn start(app: LocalSender<AppMessage>) -> windows_window::Result<Self> {
        let window = windows_window::Window::new("Coolwave")
            .style(0)
            .visible(false)
            .quit_on_close(false)
            .on_message(move |_, message, wparam, _| match message {
                WM_SHOW => {
                    let _ = app.send(AppMessage::Show);
                    Some(0)
                }
                WM_TIMER if wparam == TICK_TIMER => {
                    let _ = app.send(AppMessage::Tick);
                    Some(0)
                }
                _ => None,
            })
            .create()?;
        WINDOW.store(window.hwnd() as isize, Ordering::SeqCst);
        Ok(Self { window, interval: Cell::new(0) })
    }

    /// Whether a request to show the window came before the bridge was there to take it.
    pub fn take_early_show(&self) -> bool {
        SHOW_PENDING.swap(false, Ordering::SeqCst)
    }

    /// Tick every `ms` milliseconds. The same interval again leaves the timer as it runs.
    pub fn tick_every(&self, ms: u32) {
        if self.interval.replace(ms) != ms {
            // SAFETY: the window lives as long as the bridge; replacing the timer is allowed.
            unsafe { SetTimer(self.window.hwnd(), TICK_TIMER, ms, None) };
        }
    }
}

/// Ask the UI thread to show the window. Safe from any thread, also before the bridge exists.
pub fn ask_to_show() {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd == 0 {
        SHOW_PENDING.store(true, Ordering::SeqCst);
        return;
    }
    // SAFETY: posting to a window handle is safe; a stale one just fails.
    unsafe { PostMessageW(hwnd as _, WM_SHOW, 0, 0) };
}
