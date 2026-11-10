//! One Coolwave at a time. A second launch asks the first to show its window
//! and exits.

use super::Wake;
use std::sync::atomic::{AtomicBool, Ordering};

static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

/// True once after another launch asked for the window.
pub fn take_show_request() -> bool {
    SHOW_REQUESTED.swap(false, Ordering::SeqCst)
}

/// True when this is the only Coolwave. Otherwise the running one was told to
/// show its window, and this process should exit.
#[cfg(windows)]
pub fn claim(wake: Wake) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
    };

    let wide = |name: &str| -> Vec<u16> { name.encode_utf16().chain(Some(0)).collect() };
    let mutex_name = wide("Local\\coolwave");
    let event_name = wide("Local\\coolwave-show");
    // SAFETY: the names are valid, 0-terminated wide strings. The mutex and
    // event handles are kept for the life of the process on purpose.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let event = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
            if !event.is_null() {
                SetEvent(event);
            }
            return false;
        }
        // Auto-reset: each launch wakes the listener once.
        let event = CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr());
        if !event.is_null() {
            let event = event as usize;
            std::thread::spawn(move || {
                while WaitForSingleObject(event as _, INFINITE) == WAIT_OBJECT_0 {
                    SHOW_REQUESTED.store(true, Ordering::SeqCst);
                    wake();
                }
            });
        }
    }
    true
}

#[cfg(not(windows))]
pub fn claim(_wake: Wake) -> bool {
    true
}
