//! Recording heads-up display.
//!
//! The red frame is deliberately positioned *outside* the capture rectangle, so it
//! cannot leak into the GIF even if Windows capture-exclusion is unavailable. The
//! timer is also outside the capture rectangle and is marked WDA_EXCLUDEFROMCAPTURE
//! as an additional safety net.

use crate::{
    messages::{WM_GIFSHOT_STOP, WM_HUD_MAX_DURATION},
    types::ScreenRect,
    win32,
};
use std::{ptr, time::Instant};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_GUI_FONT, DeleteObject, DrawTextW,
        Ellipse, EndPaint, FillRect, GetStockObject, InvalidateRect, SelectObject, SetBkMode,
        SetTextColor, SetWindowRgn, PAINTSTRUCT, TRANSPARENT, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
            GetWindowLongPtrW, HWND_TOPMOST, IDC_HAND, KillTimer, LWA_ALPHA, LoadCursorW,
            PostMessageW, RegisterClassExW, SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW,
            SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity, SetWindowLongPtrW,
            SetWindowPos, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WM_ERASEBKGND, WM_LBUTTONUP,
            WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
};

const BORDER_THICKNESS_DIP: i32 = 3;
const TIMER_WIDTH_DIP: i32 = 118;
const TIMER_HEIGHT_DIP: i32 = 32;
const TIMER_GAP_DIP: i32 = 8;
const TIMER_ID: usize = 1;

pub struct RecordingHud {
    border_hwnd: HWND,
    timer_hwnd: HWND,
}

struct TimerState {
    main_hwnd: HWND,
    started_at: Instant,
    max_duration_secs: u64,
    max_posted: bool,
    dpi: u32,
}

#[derive(Clone, Copy)]
struct HudMetrics {
    border: i32,
    timer_width: i32,
    timer_height: i32,
    timer_gap: i32,
}

fn scale_px(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

fn metrics(dpi: u32) -> HudMetrics {
    HudMetrics {
        border: scale_px(BORDER_THICKNESS_DIP, dpi),
        timer_width: scale_px(TIMER_WIDTH_DIP, dpi),
        timer_height: scale_px(TIMER_HEIGHT_DIP, dpi),
        timer_gap: scale_px(TIMER_GAP_DIP, dpi),
    }
}

impl RecordingHud {
    pub fn show(
        main_hwnd: HWND,
        capture_rect: ScreenRect,
        max_duration_secs: u64,
        started_at: Instant,
    ) -> Result<Self, String> {
        register_classes()?;

        unsafe {
            let instance = GetModuleHandleW(ptr::null());

            // Create at the capture monitor first so the HWND receives that monitor's
            // PMv2 DPI, then size the visual in DIPs converted to physical pixels.
            let border_hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
                windows_sys::w!("GifShot.RecordingBorder"),
                windows_sys::w!(""),
                WS_POPUP,
                capture_rect.left,
                capture_rect.top,
                1,
                1,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if border_hwnd.is_null() {
                return Err("CreateWindowExW(recording border) failed".into());
            }
            let border_dpi = GetDpiForWindow(border_hwnd).max(96);
            let border_metrics = metrics(border_dpi);
            let border_width = capture_rect.width() + border_metrics.border * 2;
            let border_height = capture_rect.height() + border_metrics.border * 2;
            win32::set_border_region(
                border_hwnd,
                border_width,
                border_height,
                border_metrics.border,
            );
            SetLayeredWindowAttributes(border_hwnd, 0, 255, LWA_ALPHA);
            let _ = SetWindowDisplayAffinity(border_hwnd, WDA_EXCLUDEFROMCAPTURE);
            SetWindowPos(
                border_hwnd,
                HWND_TOPMOST,
                capture_rect.left - border_metrics.border,
                capture_rect.top - border_metrics.border,
                border_width,
                border_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            ShowWindow(border_hwnd, SW_SHOW);

            let timer_state = Box::new(TimerState {
                main_hwnd,
                started_at,
                max_duration_secs,
                max_posted: false,
                dpi: 96,
            });
            let raw = Box::into_raw(timer_state);
            let timer_hwnd = CreateWindowExW(
                // Intentionally not WS_EX_TRANSPARENT: the badge must receive clicks
                // so the user can stop recording without using the hotkey.
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                windows_sys::w!("GifShot.RecordingTimer"),
                windows_sys::w!("Recording"),
                WS_POPUP,
                capture_rect.left,
                capture_rect.top,
                1,
                1,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if timer_hwnd.is_null() {
                drop(Box::from_raw(raw));
                DestroyWindow(border_hwnd);
                return Err("CreateWindowExW(recording timer) failed".into());
            }

            let timer_dpi = GetDpiForWindow(timer_hwnd).max(96);
            (*raw).dpi = timer_dpi;
            let timer_metrics = metrics(timer_dpi);
            let desktop = win32::monitor_at_point(capture_rect.left, capture_rect.top)
                .map(|(_, rect)| rect)
                .unwrap_or_else(|_| win32::virtual_screen());
            let edge = scale_px(4, timer_dpi);
            let min_x = desktop.left + edge;
            let max_x = (desktop.right - timer_metrics.timer_width - edge).max(min_x);
            let min_y = desktop.top + edge;
            let max_y = (desktop.bottom - timer_metrics.timer_height - edge).max(min_y);

            // Prefer a location completely outside the capture rectangle. Besides
            // being visually cleaner, this makes correctness independent of capture
            // exclusion support for normal region recordings.
            let above_y = capture_rect.top - timer_metrics.timer_height - timer_metrics.timer_gap;
            let below_y = capture_rect.bottom + timer_metrics.timer_gap;
            let left_x = capture_rect.left - timer_metrics.timer_width - timer_metrics.timer_gap;
            let right_x = capture_rect.right + timer_metrics.timer_gap;
            let (timer_x, timer_y, timer_inside_capture) = if above_y >= desktop.top {
                (capture_rect.left.clamp(min_x, max_x), above_y, false)
            } else if below_y + timer_metrics.timer_height <= desktop.bottom {
                (capture_rect.left.clamp(min_x, max_x), below_y, false)
            } else if left_x >= desktop.left {
                (left_x, capture_rect.top.clamp(min_y, max_y), false)
            } else if right_x + timer_metrics.timer_width <= desktop.right {
                (right_x, capture_rect.top.clamp(min_y, max_y), false)
            } else {
                (
                    capture_rect.left.clamp(min_x, max_x),
                    (capture_rect.top + timer_metrics.timer_gap).clamp(min_y, max_y),
                    true,
                )
            };

            SetWindowLongPtrW(timer_hwnd, GWLP_USERDATA, raw as isize);
            SetLayeredWindowAttributes(timer_hwnd, 0, 245, LWA_ALPHA);
            let timer_excluded = SetWindowDisplayAffinity(timer_hwnd, WDA_EXCLUDEFROMCAPTURE) != 0;
            let corner = scale_px(8, timer_dpi);
            let region = CreateRoundRectRgn(
                0,
                0,
                timer_metrics.timer_width + 1,
                timer_metrics.timer_height + 1,
                corner,
                corner,
            );
            if !region.is_null() {
                SetWindowRgn(timer_hwnd, region, 1);
            }
            SetWindowPos(
                timer_hwnd,
                HWND_TOPMOST,
                timer_x,
                timer_y,
                timer_metrics.timer_width,
                timer_metrics.timer_height,
                SWP_NOACTIVATE,
            );
            // If the selection occupies the whole monitor and Windows refuses
            // WDA_EXCLUDEFROMCAPTURE, hiding the timer is preferable to baking it
            // into the user's GIF. The main health timer still enforces max duration.
            if !timer_inside_capture || timer_excluded {
                ShowWindow(timer_hwnd, SW_SHOW);
            }
            if SetTimer(timer_hwnd, TIMER_ID, 250, None) == 0 {
                // WM_NCDESTROY releases TimerState because ownership has already
                // been attached to GWLP_USERDATA at this point.
                DestroyWindow(timer_hwnd);
                DestroyWindow(border_hwnd);
                return Err("SetTimer(recording HUD) failed".into());
            }

            Ok(Self { border_hwnd, timer_hwnd })
        }
    }
}

impl Drop for RecordingHud {
    fn drop(&mut self) {
        unsafe {
            if !self.timer_hwnd.is_null() {
                DestroyWindow(self.timer_hwnd);
                self.timer_hwnd = ptr::null_mut();
            }
            if !self.border_hwnd.is_null() {
                DestroyWindow(self.border_hwnd);
                self.border_hwnd = ptr::null_mut();
            }
        }
    }
}

fn register_classes() -> Result<(), String> {
    use std::sync::OnceLock;
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

    REGISTERED
        .get_or_init(|| unsafe {
            let instance = GetModuleHandleW(ptr::null());

            let red = CreateSolidBrush(win32::rgb(235, 64, 52));
            if red.is_null() {
                return Err("CreateSolidBrush(recording border) failed".into());
            }
            let mut border: WNDCLASSEXW = std::mem::zeroed();
            border.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            border.lpfnWndProc = Some(DefWindowProcW);
            border.hInstance = instance;
            border.hbrBackground = red;
            border.lpszClassName = windows_sys::w!("GifShot.RecordingBorder");
            if RegisterClassExW(&border) == 0 {
                DeleteObject(red as _);
                return Err("RegisterClassExW(recording border) failed".into());
            }

            let mut timer: WNDCLASSEXW = std::mem::zeroed();
            timer.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            timer.lpfnWndProc = Some(timer_proc);
            timer.hInstance = instance;
            timer.hCursor = LoadCursorW(ptr::null_mut(), IDC_HAND);
            timer.lpszClassName = windows_sys::w!("GifShot.RecordingTimer");
            if RegisterClassExW(&timer) == 0 {
                return Err("RegisterClassExW(recording timer) failed".into());
            }
            Ok(())
        })
        .clone()
}

unsafe extern "system" fn timer_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TimerState;
    if msg == WM_NCDESTROY {
        KillTimer(hwnd, TIMER_ID);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !raw.is_null() {
            // Recover the Box before creating any Rust reference to it. This keeps
            // teardown correct even if future handlers gain synchronous re-entry.
            drop(Box::from_raw(raw));
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    if raw.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *raw;

    match msg {
        WM_TIMER if wparam == TIMER_ID => {
            let elapsed = state.started_at.elapsed().as_secs();
            if !state.max_posted && elapsed >= state.max_duration_secs {
                state.max_posted = true;
                PostMessageW(state.main_hwnd, WM_HUD_MAX_DURATION, 0, 0);
            }
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_LBUTTONUP => {
            PostMessageW(state.main_hwnd, WM_GIFSHOT_STOP, 0, 0);
            0
        }
        WM_PAINT => {
            paint_timer(hwnd, state);
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_timer(hwnd: HWND, state: &TimerState) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        EndPaint(hwnd, &ps);
        return;
    }

    let m = metrics(state.dpi);
    let mut rect = RECT { left: 0, top: 0, right: m.timer_width, bottom: m.timer_height };
    let bg = CreateSolidBrush(win32::rgb(30, 30, 33));
    FillRect(hdc, &rect, bg);
    DeleteObject(bg as _);

    // Red circular recording indicator.
    let red = CreateSolidBrush(win32::rgb(235, 64, 52));
    let old_brush = SelectObject(hdc, red as _);
    let dot_left = scale_px(10, state.dpi);
    let dot_top = scale_px(11, state.dpi);
    let dot_size = scale_px(10, state.dpi);
    Ellipse(hdc, dot_left, dot_top, dot_left + dot_size, dot_top + dot_size);
    SelectObject(hdc, old_brush);
    DeleteObject(red as _);

    let elapsed = state.started_at.elapsed().as_secs();
    let text = format!("REC  {:02}:{:02}", elapsed / 60, elapsed % 60);
    let wide = win32::wide(text);
    rect.left = scale_px(23, state.dpi);
    rect.right = m.timer_width - scale_px(6, state.dpi);
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, win32::rgb(245, 245, 247));
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let old_font = SelectObject(hdc, font);
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(hdc, old_font);
    EndPaint(hwnd, &ps);
}
