//! Private Win32 messages shared by the application's windows.

use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_GIFSHOT_TOGGLE: u32 = WM_APP + 1;
pub const WM_GIFSHOT_STOP: u32 = WM_APP + 2;
pub const WM_GIFSHOT_QUIT: u32 = WM_APP + 3;
pub const WM_GIFSHOT_OPEN_CAPTURES: u32 = WM_APP + 4;
pub const WM_GIFSHOT_OPEN_CONFIG: u32 = WM_APP + 5;
pub const WM_GIFSHOT_SHUTDOWN: u32 = WM_APP + 6;
pub const WM_GIFSHOT_RELOAD_CONFIG: u32 = WM_APP + 7;
pub const WM_SELECTION_COMPLETE: u32 = WM_APP + 10;
pub const WM_SELECTION_CANCELLED: u32 = WM_APP + 11;
pub const WM_RECORDING_FINISHED: u32 = WM_APP + 20;
pub const WM_HUD_MAX_DURATION: u32 = WM_APP + 21;
pub const WM_TRAY_CALLBACK: u32 = WM_APP + 30;
