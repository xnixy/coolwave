//! The native window: its icon, the frosted backdrop, and going to the tray
//! and back.

use eframe::egui;

const ICON_PNG: &[u8] = include_bytes!("../../../../data/icons/coolwave-256.png");

pub fn icon() -> egui::IconData {
    match image::load_from_memory(ICON_PNG) {
        Ok(image) => {
            let image = image.to_rgba8();
            let (width, height) = image.dimensions();
            egui::IconData { rgba: image.into_raw(), width, height }
        }
        Err(_) => egui::IconData::default(),
    }
}

#[cfg(windows)]
pub use windows::Window;

#[cfg(not(windows))]
pub use plain::Window;

#[cfg(windows)]
mod windows {
    use eframe::egui;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GWL_EXSTYLE, GetForegroundWindow, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
        GetWindowThreadProcessId, IsIconic, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_APPWINDOW,
        WS_EX_TOOLWINDOW,
    };

    /// How far past the edge of every monitor a parked window sits.
    const PARK_MARGIN: i32 = 80;
    const MOVE_ONLY: u32 = SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED;

    pub struct Window {
        hwnd: HWND,
    }

    impl Window {
        pub fn of(cc: &eframe::CreationContext<'_>) -> Self {
            let hwnd = match cc.window_handle().map(|handle| handle.as_raw()) {
                Ok(RawWindowHandle::Win32(win32)) => win32.hwnd.get() as HWND,
                _ => std::ptr::null_mut(),
            };
            Self { hwnd }
        }

        /// True when Windows gave the window its acrylic backdrop.
        pub fn frost(&self) -> bool {
            !self.hwnd.is_null() && coolwave_ui::window::frost(self.hwnd)
        }

        pub fn paint_frame(&self, light: bool) {
            if !self.hwnd.is_null() {
                coolwave_ui::window::paint_frame(self.hwnd, light);
            }
        }

        fn rect(&self) -> RECT {
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: `hwnd` is this app's window and `rect` is a valid out pointer.
            unsafe { GetWindowRect(self.hwnd, &mut rect) };
            rect
        }

        fn move_to(&self, x: i32, y: i32) {
            // SAFETY: moves this app's own window.
            unsafe { SetWindowPos(self.hwnd, std::ptr::null_mut(), x, y, 0, 0, MOVE_ONLY) };
        }

        /// The window's center is on some monitor.
        fn on_screen(&self) -> bool {
            let rect = self.rect();
            let (x, y) = (rect.left.saturating_add(rect.right) / 2, rect.top.saturating_add(rect.bottom) / 2);
            // SAFETY: plain system metric queries.
            let (left, top, width, height) = unsafe {
                (
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                )
            };
            x >= left && y >= top && x < left.saturating_add(width) && y < top.saturating_add(height)
        }

        /// A tool window has no taskbar button.
        fn is_tool_window(&self) -> bool {
            // SAFETY: reads a style of this app's window.
            let style = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
            style & WS_EX_TOOLWINDOW != 0 && style & WS_EX_APPWINDOW == 0
        }

        fn set_taskbar_button(&self, show: bool) {
            // SAFETY: changes a style of this app's window.
            unsafe {
                let mut style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
                if show {
                    style = (style & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW;
                } else {
                    style = (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
                }
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style as isize);
            }
        }

        /// Move the window off every monitor without hiding it. True when it is parked.
        ///
        /// `ShowWindow(SW_HIDE)` makes Windows drop `WM_PAINT`. eframe then
        /// leaves its event loop spinning after a missed redraw, which burns a
        /// core, and the tray's commands are never read.
        pub fn park(&self, _ctx: &egui::Context) -> bool {
            if self.hwnd.is_null() {
                return false;
            }
            if !self.on_screen() && self.is_tool_window() {
                return true;
            }
            self.set_taskbar_button(false);
            let rect = self.rect();
            let (width, height) = ((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
            // SAFETY: plain system metric queries, then show commands for this app's window.
            unsafe {
                let x = GetSystemMetrics(SM_XVIRTUALSCREEN).saturating_sub(width).saturating_sub(PARK_MARGIN);
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN).saturating_sub(height).saturating_sub(PARK_MARGIN);
                self.move_to(x, y);
                // Hide and show again so the taskbar drops its button, then leave
                // the window visible so paints still arrive.
                ShowWindow(self.hwnd, SW_HIDE);
                ShowWindow(self.hwnd, SW_SHOWNA);
            }
            true
        }

        /// Keep a parked window parked: Windows puts it back on the taskbar
        /// unless the tool style is applied again. False when something
        /// brought the window back on screen.
        pub fn stay_parked(&self, ctx: &egui::Context) -> bool {
            if self.on_screen() {
                return false;
            }
            self.park(ctx)
        }

        /// Put the window back on the primary monitor, on the taskbar, and in front.
        pub fn reveal(&self, ctx: &egui::Context) {
            if self.hwnd.is_null() {
                return;
            }
            self.set_taskbar_button(true);
            if !self.on_screen() {
                let rect = self.rect();
                let (width, height) = ((rect.right - rect.left).max(200), (rect.bottom - rect.top).max(200));
                // SAFETY: plain system metric queries.
                let (screen_width, screen_height) = unsafe { (GetSystemMetrics(SM_CXSCREEN).max(width), GetSystemMetrics(SM_CYSCREEN).max(height)) };
                self.move_to((screen_width - width) / 2, (screen_height - height) / 2);
            }
            // SAFETY: all calls act on this app's window. Input is attached to
            // the foreground thread only for the moment Windows needs to let
            // this window take focus.
            unsafe {
                ShowWindow(self.hwnd, if IsIconic(self.hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
                let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
                let this_thread = GetCurrentThreadId();
                let borrow_focus = foreground_thread != 0 && foreground_thread != this_thread;
                if borrow_focus {
                    AttachThreadInput(foreground_thread, this_thread, 1);
                }
                SetForegroundWindow(self.hwnd);
                BringWindowToTop(self.hwnd);
                if borrow_focus {
                    AttachThreadInput(foreground_thread, this_thread, 0);
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
}

/// Without Windows there is no backdrop and no tray to park in.
#[cfg(not(windows))]
mod plain {
    use eframe::egui;

    pub struct Window;

    impl Window {
        pub fn of(_cc: &eframe::CreationContext<'_>) -> Self {
            Self
        }

        pub fn frost(&self) -> bool {
            false
        }

        pub fn paint_frame(&self, _light: bool) {}

        pub fn park(&self, ctx: &egui::Context) -> bool {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            true
        }

        pub fn stay_parked(&self, _ctx: &egui::Context) -> bool {
            true
        }

        pub fn reveal(&self, ctx: &egui::Context) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
}
