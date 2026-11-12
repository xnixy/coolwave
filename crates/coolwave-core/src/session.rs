//! Tells the lighting engine when Windows changes something that makes the
//! Aura controller drop direct mode: the display turning off or on, and the
//! user leaving or coming back. Windows calls back on one of its own threads;
//! no window is needed.

use crate::engine::renew_direct_mode;
use std::ffi::c_void;
use std::sync::Once;
use windows_sys::core::GUID;
use windows_sys::Win32::System::Power::{DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerSettingRegisterNotification};
use windows_sys::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;

/// From winnt.h: the console display, this session's display, and whether the user is present.
const SETTINGS: [GUID; 3] = [
    GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47),
    GUID::from_u128(0x2b84c20e_ad23_4ddf_93db_05ffbd7efca5),
    GUID::from_u128(0x3c0f4548_c03f_4c4d_b9f2_237ede686376),
];

unsafe extern "system" fn changed(_context: *const c_void, _kind: u32, _setting: *const c_void) -> u32 {
    renew_direct_mode();
    0
}

/// Start listening. Stays registered for as long as the program runs.
pub fn watch() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Windows keeps the pointer to these for as long as the registration lives, so they are leaked.
        let recipient = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS { Callback: Some(changed), Context: std::ptr::null_mut() }));
        for setting in &SETTINGS {
            let mut registration = std::ptr::null_mut();
            // SAFETY: `setting` and `recipient` are valid, and `recipient` is never freed.
            // A failure only means no early renewal; the slow keepalive still runs.
            unsafe { PowerSettingRegisterNotification(setting, DEVICE_NOTIFY_CALLBACK, std::ptr::from_mut(recipient).cast(), &mut registration) };
        }
    });
}
