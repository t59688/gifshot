//! Native Windows notification-area integration.
//!
//! The tray is intentionally secondary to the global hotkey. It exposes capture,
//! settings/help (terminal), and quit without introducing a GUI framework.

use crate::{messages::WM_TRAY_CALLBACK, win32};
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, POINT, RECT},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIIF_WARNING, NIM_ADD,
            NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics,
            IDI_APPLICATION, IMAGE_ICON, LoadIconW, LoadImageW, MF_SEPARATOR, MF_STRING,
            PostMessageW, SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, TPM_BOTTOMALIGN,
            TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, HICON, WM_NULL,
        },
    },
};

pub const CMD_CAPTURE_OR_STOP: u32 = 1001;
pub const CMD_SETTINGS: u32 = 1002;
pub const CMD_HELP: u32 = 1003;
pub const CMD_QUIT: u32 = 1004;

/// Resource ID assigned by `winresource` when embedding `assets/gifshot.ico`.
const IDI_GIFSHOT: isize = 1;
const ICON_ID: u32 = 1;

pub struct TrayIcon {
    hwnd: HWND,
    added: bool,
    tooltip: String,
    icon: HICON,
    /// True when `icon` came from `LoadImageW` and must be destroyed.
    owns_icon: bool,
}

impl TrayIcon {
    pub fn new(hwnd: HWND, hotkey_label: &str) -> Self {
        let (icon, owns_icon) = load_tray_icon();
        Self {
            hwnd,
            added: false,
            tooltip: format!("GifShot — {hotkey_label}"),
            icon,
            owns_icon,
        }
    }

    /// Explorer recreates the notification area after a shell restart. The
    /// registered TaskbarCreated message lets the main window call this method
    /// and restore the icon without restarting the recorder.
    pub fn restore(&mut self) -> Result<(), String> {
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.hwnd;
            data.uID = ICON_ID;
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            data.uCallbackMessage = WM_TRAY_CALLBACK;
            data.hIcon = self.icon;
            copy_wide(&mut data.szTip, &self.tooltip);

            if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                self.added = false;
                return Err("Shell_NotifyIconW(NIM_ADD) failed".into());
            }
            self.added = true;
        }
        Ok(())
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.tooltip = tooltip.to_string();
        if !self.added {
            return;
        }
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.hwnd;
            data.uID = ICON_ID;
            data.uFlags = NIF_TIP;
            copy_wide(&mut data.szTip, &self.tooltip);
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    pub fn show_menu(&self, recording: bool) -> Option<u32> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }

            let primary = if recording { "停止录制" } else { "录制 GIF" };
            let primary_w = win32::wide(primary);
            let settings_w = win32::wide("设置");
            let help_w = win32::wide("帮助");
            let quit_w = win32::wide("退出");

            AppendMenuW(menu, MF_STRING, CMD_CAPTURE_OR_STOP as usize, primary_w.as_ptr());
            AppendMenuW(menu, MF_STRING, CMD_SETTINGS as usize, settings_w.as_ptr());
            AppendMenuW(menu, MF_STRING, CMD_HELP as usize, help_w.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
            AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, quit_w.as_ptr());

            let anchor = self.menu_anchor();
            // Required by the Win32 notification-area menu contract so the menu
            // dismisses correctly when the user clicks elsewhere.
            SetForegroundWindow(self.hwnd);
            // Bottom+left align: open above the icon and extend to its right,
            // matching common tray menus (图标右上方).
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                anchor.x,
                anchor.y,
                0,
                self.hwnd,
                ptr::null(),
            ) as u32;
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            (command != 0).then_some(command)
        }
    }

    /// Prefer the tray icon rectangle; fall back to the cursor if Explorer has
    /// not yet published the icon bounds (overflow flyout, shell restart, etc.).
    fn menu_anchor(&self) -> POINT {
        unsafe {
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: self.hwnd,
                uID: ICON_ID,
                guidItem: std::mem::zeroed(),
            };
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if Shell_NotifyIconGetRect(&identifier, &mut rect) >= 0
                && rect.right > rect.left
                && rect.bottom > rect.top
            {
                return POINT {
                    x: rect.left,
                    y: rect.top,
                };
            }

            let mut point = POINT { x: 0, y: 0 };
            GetCursorPos(&mut point);
            point
        }
    }

    pub fn notify_error(&self, title: &str, text: &str) {
        self.notify(title, text, NIIF_ERROR);
    }

    pub fn notify_warning(&self, title: &str, text: &str) {
        self.notify(title, text, NIIF_WARNING);
    }

    pub fn notify_info(&self, title: &str, text: &str) {
        self.notify(title, text, NIIF_INFO);
    }

    fn notify(&self, title: &str, text: &str, icon: u32) {
        if !self.added {
            return;
        }
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.hwnd;
            data.uID = ICON_ID;
            data.uFlags = NIF_INFO;
            copy_wide(&mut data.szInfoTitle, title);
            copy_wide(&mut data.szInfo, text);
            data.dwInfoFlags = icon;
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.added {
            unsafe {
                let mut data: NOTIFYICONDATAW = std::mem::zeroed();
                data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = self.hwnd;
                data.uID = ICON_ID;
                Shell_NotifyIconW(NIM_DELETE, &data);
            }
            self.added = false;
        }
        if self.owns_icon && !self.icon.is_null() {
            unsafe {
                DestroyIcon(self.icon);
            }
            self.icon = ptr::null_mut();
            self.owns_icon = false;
        }
    }
}

fn load_tray_icon() -> (HICON, bool) {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(ptr::null());
        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);
        let icon = LoadImageW(
            instance,
            IDI_GIFSHOT as *const u16,
            IMAGE_ICON,
            cx,
            cy,
            0,
        ) as HICON;
        if icon.is_null() {
            // Shared system icon; DestroyIcon must not be called on it.
            (LoadIconW(ptr::null_mut(), IDI_APPLICATION), false)
        } else {
            (icon, true)
        }
    }
}

fn copy_wide<const N: usize>(dst: &mut [u16; N], text: &str) {
    let mut src = text.encode_utf16();
    for slot in dst.iter_mut().take(N.saturating_sub(1)) {
        if let Some(ch) = src.next() {
            *slot = ch;
        } else {
            break;
        }
    }
    dst[N - 1] = 0;
}
