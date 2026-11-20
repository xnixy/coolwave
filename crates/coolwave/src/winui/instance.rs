//! One Coolwave at a time. A second launch asks the first to show its window and exits.

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

/// True when this is the only Coolwave; `show` then runs (on another thread) each time a later
/// launch asks for the window. Otherwise the running one was told to show its window, and this
/// process should exit.
pub fn claim(show: impl Fn() + Send + 'static) -> bool {
    let wide = |name: &str| -> Vec<u16> { name.encode_utf16().chain(Some(0)).collect() };
    let mutex_name = wide("Local\\coolwave");
    let event_name = wide("Local\\coolwave-show");
    // SAFETY: the names are valid, 0-terminated wide strings. The mutex and event handles are
    // kept for the life of the process on purpose.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Windows lets only the program in front bring a window forward, and that is this
            // launch: hand the right to the running Coolwave.
            AllowSetForegroundWindow(ASFW_ANY);
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
                    show();
                }
            });
        }
    }
    true
}
