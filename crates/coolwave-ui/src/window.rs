//! The Windows 11 acrylic backdrop and a title bar in the palette's colors.

use crate::theme::colors;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMSBT_TRANSIENTWINDOW, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_NCACTIVATE;

/// True when Windows accepted the attribute.
fn set_attribute(hwnd: HWND, attribute: i32, value: u32) -> bool {
    // SAFETY: `value` is a valid u32 for the size passed.
    unsafe { DwmSetWindowAttribute(hwnd, attribute as u32, (&raw const value).cast::<c_void>(), size_of::<u32>() as u32) >= 0 }
}

/// Title bar and border in the palette's mode. Call again when the mode changes.
pub fn paint_frame(hwnd: HWND, light: bool) {
    // COLORREF is 0x00BBGGRR: a plum border and caption, with text that contrasts.
    let (border, caption, text) = if light { (0x00DC_D4DA, 0x00F3_EEF2, 0x0020_181E) } else { (0x0030_2832, 0x0018_1016, 0x00F2_EEF0) };
    set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!light));
    set_attribute(hwnd, DWMWA_BORDER_COLOR, border);
    set_attribute(hwnd, DWMWA_CAPTION_COLOR, caption);
    set_attribute(hwnd, DWMWA_TEXT_COLOR, text);
}

/// Give the window the Windows 11 acrylic backdrop. False on Windows versions
/// without system backdrops, where the window stays opaque.
pub fn frost(hwnd: HWND) -> bool {
    paint_frame(hwnd, colors().light);
    set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND as u32);
    let whole_window = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    // SAFETY: `hwnd` is the caller's live window and the margins outlive the call.
    let frosted = unsafe { DwmExtendFrameIntoClientArea(hwnd, &whole_window) } >= 0
        && set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW as u32);
    if frosted {
        // SAFETY: the subclass procedure is a plain function that lives as long as the program.
        unsafe { SetWindowSubclass(hwnd, Some(stay_frosted), 1, 0) };
    }
    frosted
}

/// Windows swaps the backdrop for a flat color while the window is inactive,
/// judging by whether the frame is drawn active. Always drawing it active
/// keeps the glass. Keyboard focus is tracked through other messages, so the
/// window still knows when it is not focused.
unsafe extern "system" fn stay_frosted(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let wparam = if msg == WM_NCACTIVATE { 1 } else { wparam };
    // SAFETY: forwarding the message this procedure was called with.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
