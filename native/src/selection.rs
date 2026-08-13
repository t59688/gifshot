//! Snipping-Tool-style selection overlay and FPS chooser.
//!
//! No webview or GUI framework is used. The dimmer is a layered Win32 window whose
//! region is cut around the selection, and the FPS chooser is a tiny custom-painted
//! popup. Dragging is constrained to the monitor where it started; this makes the
//! capture crop unambiguous even on mixed-DPI/multi-GPU systems.

use crate::{
    messages::{WM_SELECTION_CANCELLED, WM_SELECTION_COMPLETE},
    types::{GifQuality, ScreenRect, SelectionResult},
    win32,
};
use std::ptr;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_GUI_FONT, DeleteObject, DrawTextW, EndPaint,
        FillRect, GetStockObject, InvalidateRect, RoundRect, SelectObject, SetBkMode, SetTextColor,
        SetWindowRgn, PAINTSTRUCT, TRANSPARENT, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE},
        WindowsAndMessaging::{
            CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
            GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, IDC_ARROW, LWA_ALPHA,
            LoadCursorW, PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOW, SetCursor,
            SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
            ShowWindow, WNDCLASSEXW, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN,
            WM_SETCURSOR, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_NOACTIVATE,
            WS_EX_TRANSPARENT, WS_POPUP, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        },
    },
};

const BORDER: i32 = 2;
const MIN_SELECTION: i32 = 16;
const ROW_HEIGHT_DIP: i32 = 44;
const FPS_BUTTON_WIDTH_DIP: i32 = 52;
const QUALITY_BUTTON_WIDTH_DIP: i32 = 56;
const POPUP_PADDING_DIP: i32 = 8;

pub struct SelectionController {
    main_hwnd: HWND,
    overlay_hwnd: HWND,
    border_hwnd: HWND,
    blocker_hwnd: HWND,
    fps_hwnd: HWND,
    virtual_rect: ScreenRect,
    monitor_rect: Option<ScreenRect>,
    monitor: isize,
    drag_start: Option<(i32, i32)>,
    selection: Option<ScreenRect>,
    fps_options: Vec<u32>,
    selected_fps: u32,
    selected_quality: GifQuality,
    hover_fps: Option<usize>,
    hover_quality: Option<usize>,
    fps_dpi: u32,
    committed: bool,
}

impl SelectionController {
    unsafe fn destroy_aux_windows(&mut self) {
        // FPS/blocker WndProcs point back to this controller. Clear that pointer
        // before DestroyWindow because destruction synchronously re-enters them.
        if !self.fps_hwnd.is_null() {
            SetWindowLongPtrW(self.fps_hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.fps_hwnd);
            self.fps_hwnd = ptr::null_mut();
        }
        if !self.blocker_hwnd.is_null() {
            SetWindowLongPtrW(self.blocker_hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.blocker_hwnd);
            self.blocker_hwnd = ptr::null_mut();
        }
        if !self.border_hwnd.is_null() {
            DestroyWindow(self.border_hwnd);
            self.border_hwnd = ptr::null_mut();
        }
    }
}

impl Drop for SelectionController {
    fn drop(&mut self) {
        unsafe {
            self.destroy_aux_windows();
        }
    }
}

pub fn show(
    main_hwnd: HWND,
    fps_options: Vec<u32>,
    default_fps: u32,
    default_quality: GifQuality,
    dim_opacity: u8,
) -> Result<HWND, String> {
    register_classes()?;
    let virtual_rect = win32::virtual_screen();
    let controller = Box::new(SelectionController {
        main_hwnd,
        overlay_hwnd: ptr::null_mut(),
        border_hwnd: ptr::null_mut(),
        blocker_hwnd: ptr::null_mut(),
        fps_hwnd: ptr::null_mut(),
        virtual_rect,
        monitor_rect: None,
        monitor: 0,
        drag_start: None,
        selection: None,
        fps_options,
        selected_fps: default_fps,
        selected_quality: default_quality,
        hover_fps: None,
        hover_quality: None,
        fps_dpi: 96,
        committed: false,
    });
    let raw = Box::into_raw(controller);

    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            windows_sys::w!("GifShot.SelectionOverlay"),
            windows_sys::w!("GifShot Selection"),
            WS_POPUP,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if hwnd.is_null() {
            drop(Box::from_raw(raw));
            return Err("CreateWindowExW(selection overlay) failed".into());
        }
        (*raw).overlay_hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
        SetLayeredWindowAttributes(hwnd, 0, dim_opacity, LWA_ALPHA);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
            SWP_SHOWWINDOW,
        );
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        Ok(hwnd)
    }
}

fn register_classes() -> Result<(), String> {
    use std::sync::OnceLock;
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| unsafe {
            let instance = GetModuleHandleW(ptr::null());

            let dim_brush = CreateSolidBrush(win32::rgb(0, 0, 0));
            let cross = match win32::inverted_cross_cursor() {
                Ok(cursor) => cursor,
                Err(err) => return Err(err),
            };
            let mut overlay: WNDCLASSEXW = std::mem::zeroed();
            overlay.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            overlay.style = CS_HREDRAW | CS_VREDRAW;
            overlay.lpfnWndProc = Some(selection_proc);
            overlay.hInstance = instance;
            overlay.hCursor = cross;
            overlay.hbrBackground = dim_brush;
            overlay.lpszClassName = windows_sys::w!("GifShot.SelectionOverlay");
            if RegisterClassExW(&overlay) == 0 {
                return Err("RegisterClassExW(selection) failed".into());
            }

            let border_brush = CreateSolidBrush(win32::rgb(255, 255, 255));
            let mut border: WNDCLASSEXW = std::mem::zeroed();
            border.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            border.lpfnWndProc = Some(DefWindowProcW);
            border.hInstance = instance;
            border.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
            border.hbrBackground = border_brush;
            border.lpszClassName = windows_sys::w!("GifShot.SelectionBorder");
            if RegisterClassExW(&border) == 0 {
                return Err("RegisterClassExW(selection border) failed".into());
            }

            let blocker_brush = CreateSolidBrush(win32::rgb(0, 0, 0));
            let mut blocker: WNDCLASSEXW = std::mem::zeroed();
            blocker.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            blocker.lpfnWndProc = Some(blocker_proc);
            blocker.hInstance = instance;
            blocker.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
            blocker.hbrBackground = blocker_brush;
            blocker.lpszClassName = windows_sys::w!("GifShot.SelectionBlocker");
            if RegisterClassExW(&blocker) == 0 {
                return Err("RegisterClassExW(selection blocker) failed".into());
            }

            let mut fps: WNDCLASSEXW = std::mem::zeroed();
            fps.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            fps.lpfnWndProc = Some(fps_proc);
            fps.hInstance = instance;
            fps.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
            fps.lpszClassName = windows_sys::w!("GifShot.FpsPopup");
            if RegisterClassExW(&fps) == 0 {
                return Err("RegisterClassExW(FPS popup) failed".into());
            }
            Ok(())
        })
        .clone()
}

unsafe extern "system" fn selection_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SelectionController;
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !raw.is_null() {
            let mut owned = Box::from_raw(raw);
            owned.overlay_hwnd = ptr::null_mut();
            owned.destroy_aux_windows();
            drop(owned);
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    if msg == WM_CLOSE {
        if raw.is_null() {
            DestroyWindow(hwnd);
            return 0;
        }
        {
            let state = &mut *raw;
            if !state.committed {
                PostMessageW(state.main_hwnd, WM_SELECTION_CANCELLED, 0, 0);
            }
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let mut owned = Box::from_raw(raw);
        owned.overlay_hwnd = ptr::null_mut();
        owned.destroy_aux_windows();
        DestroyWindow(hwnd);
        drop(owned);
        return 0;
    }
    if raw.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *raw;

    match msg {
        WM_LBUTTONDOWN => {
            let (cx, cy) = win32::point_from_lparam(lparam);
            let x = state.virtual_rect.left + cx;
            let y = state.virtual_rect.top + cy;
            if let Ok((monitor, monitor_rect)) = win32::monitor_at_point(x, y) {
                state.monitor = monitor;
                state.monitor_rect = Some(monitor_rect);
                state.drag_start = Some((x, y));
                state.selection = None;
                hide_fps(state);
                ensure_border(state);
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if let (Some((sx, sy)), Some(bounds)) = (state.drag_start, state.monitor_rect) {
                let (cx, cy) = win32::point_from_lparam(lparam);
                let x = (state.virtual_rect.left + cx).clamp(bounds.left, bounds.right - 1);
                let y = (state.virtual_rect.top + cy).clamp(bounds.top, bounds.bottom - 1);
                let rect = ScreenRect::new(sx, sy, x, y).normalized();
                if rect.width() > 0 && rect.height() > 0 {
                    state.selection = Some(rect);
                    update_visuals(state, rect);
                }
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            state.drag_start = None;
            if let Some(rect) = state.selection {
                if rect.width() >= MIN_SELECTION && rect.height() >= MIN_SELECTION {
                    show_fps(state, rect);
                } else {
                    reset_selection(state);
                }
            }
            0
        }
        WM_RBUTTONDOWN => {
            cancel(state);
            0
        }
        WM_KEYDOWN if wparam as u16 == VK_ESCAPE => {
            cancel(state);
            0
        }
        WM_SETCURSOR => {
            if let Ok(cursor) = win32::inverted_cross_cursor() {
                SetCursor(cursor);
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            // Paint explicitly rather than relying on class-background erasure. A
            // layered top-level window can otherwise be exposed before a normal
            // erase cycle and momentarily show an undimmed/undefined surface.
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_null() {
                let mut client: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut client);
                let brush = CreateSolidBrush(win32::rgb(0, 0, 0));
                if !brush.is_null() {
                    FillRect(hdc, &client, brush);
                    DeleteObject(brush as _);
                }
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn blocker_proc(
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
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SelectionController;
    if raw.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *raw;
    match msg {
        WM_RBUTTONDOWN => {
            cancel(state);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn fps_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SelectionController;
    if raw.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *raw;

    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = win32::point_from_lparam(lparam);
            let hover_fps = fps_hit_test(state, x, y);
            let hover_quality = quality_hit_test(state, x, y);
            if hover_fps != state.hover_fps || hover_quality != state.hover_quality {
                state.hover_fps = hover_fps;
                state.hover_quality = hover_quality;
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = win32::point_from_lparam(lparam);
            if let Some(index) = quality_hit_test(state, x, y) {
                state.selected_quality = GifQuality::ALL[index];
                InvalidateRect(hwnd, ptr::null(), 0);
            } else if let Some(index) = fps_hit_test(state, x, y) {
                state.selected_fps = state.fps_options[index];
                complete(state);
            }
            0
        }
        WM_KEYDOWN if wparam as u16 == VK_ESCAPE => {
            cancel(state);
            0
        }
        WM_RBUTTONDOWN => {
            cancel(state);
            0
        }
        WM_PAINT => {
            paint_popup(hwnd, state);
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn ensure_border(state: &mut SelectionController) {
    if !state.border_hwnd.is_null() {
        return;
    }
    let instance = GetModuleHandleW(ptr::null());
    state.border_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        windows_sys::w!("GifShot.SelectionBorder"),
        windows_sys::w!(""),
        WS_POPUP,
        0,
        0,
        1,
        1,
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        ptr::null(),
    );
    if !state.border_hwnd.is_null() {
        SetLayeredWindowAttributes(state.border_hwnd, 0, 255, LWA_ALPHA);
    }
}

unsafe fn update_visuals(state: &mut SelectionController, rect: ScreenRect) {
    let local = ScreenRect::new(
        rect.left - state.virtual_rect.left,
        rect.top - state.virtual_rect.top,
        rect.right - state.virtual_rect.left,
        rect.bottom - state.virtual_rect.top,
    );
    win32::set_hole_region(state.overlay_hwnd, state.virtual_rect.width(), state.virtual_rect.height(), Some(local));

    if !state.border_hwnd.is_null() {
        let width = rect.width() + BORDER * 2;
        let height = rect.height() + BORDER * 2;
        win32::set_border_region(state.border_hwnd, width, height, BORDER);
        SetWindowPos(
            state.border_hwnd,
            HWND_TOPMOST,
            rect.left - BORDER,
            rect.top - BORDER,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe fn show_fps(state: &mut SelectionController, rect: ScreenRect) {
    hide_fps(state);

    // The dimmer's window region is cut out around the selection. A nearly
    // transparent no-activate blocker keeps clicks from leaking through to the
    // application underneath while the user chooses an FPS.
    let instance = GetModuleHandleW(ptr::null());
    state.blocker_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        windows_sys::w!("GifShot.SelectionBlocker"),
        windows_sys::w!(""),
        WS_POPUP,
        rect.left,
        rect.top,
        rect.width(),
        rect.height(),
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        state as *mut SelectionController as *const _,
    );
    if state.blocker_hwnd.is_null() {
        // Never leave a bright hole over the underlying application without the
        // click blocker that makes the FPS decision modal. Resetting is safer
        // than allowing the next click to leak into the captured application.
        reset_selection(state);
        return;
    }
    SetLayeredWindowAttributes(state.blocker_hwnd, 0, 1, LWA_ALPHA);
    SetWindowPos(
        state.blocker_hwnd,
        HWND_TOPMOST,
        rect.left,
        rect.top,
        rect.width(),
        rect.height(),
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );

    // Create the popup at a point inside the selected monitor first, then read
    // the HWND's per-monitor DPI before calculating its visual metrics.
    state.fps_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
        windows_sys::w!("GifShot.FpsPopup"),
        windows_sys::w!("FPS"),
        WS_POPUP,
        rect.left,
        rect.top,
        1,
        1,
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        state as *mut SelectionController as *const _,
    );
    if state.fps_hwnd.is_null() {
        // The blocker has already been created. Tear the whole selection state
        // back down so a failed popup creation cannot strand an invisible
        // topmost window over the user's desktop.
        reset_selection(state);
        return;
    }
    state.fps_dpi = GetDpiForWindow(state.fps_hwnd).max(96);
    let metrics = popup_metrics(state);
    let width = metrics.width;
    let height = metrics.height;
    let edge = scale_px(4, state.fps_dpi);
    let gap = scale_px(10, state.fps_dpi);
    let mut x = rect.left + (rect.width() - width) / 2;
    let min_x = state.virtual_rect.left + edge;
    let max_x = (state.virtual_rect.right - width - edge).max(min_x);
    x = x.clamp(min_x, max_x);
    let below = rect.bottom + gap;
    let y = if below + height <= state.virtual_rect.bottom {
        below
    } else {
        (rect.top - height - gap).max(state.virtual_rect.top + edge)
    };

    SetLayeredWindowAttributes(state.fps_hwnd, 0, 248, LWA_ALPHA);
    SetWindowPos(
        state.fps_hwnd,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE,
    );
    let corner = scale_px(12, state.fps_dpi);
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, corner, corner);
    if !region.is_null() {
        SetWindowRgn(state.fps_hwnd, region, 1);
    }
    ShowWindow(state.fps_hwnd, SW_SHOW);
    SetForegroundWindow(state.fps_hwnd);
    SetFocus(state.fps_hwnd);
}

unsafe fn hide_fps(state: &mut SelectionController) {
    state.hover_fps = None;
    state.hover_quality = None;
    if !state.fps_hwnd.is_null() {
        SetWindowLongPtrW(state.fps_hwnd, GWLP_USERDATA, 0);
        DestroyWindow(state.fps_hwnd);
        state.fps_hwnd = ptr::null_mut();
    }
    if !state.blocker_hwnd.is_null() {
        SetWindowLongPtrW(state.blocker_hwnd, GWLP_USERDATA, 0);
        DestroyWindow(state.blocker_hwnd);
        state.blocker_hwnd = ptr::null_mut();
    }
}

unsafe fn reset_selection(state: &mut SelectionController) {
    state.selection = None;
    state.monitor_rect = None;
    state.monitor = 0;
    win32::set_hole_region(state.overlay_hwnd, state.virtual_rect.width(), state.virtual_rect.height(), None);
    if !state.border_hwnd.is_null() {
        ShowWindow(state.border_hwnd, SW_HIDE);
    }
    hide_fps(state);
}

unsafe fn complete(state: &mut SelectionController) {
    let Some(rect) = state.selection else { return };
    let result = Box::new(SelectionResult {
        rect,
        monitor: state.monitor,
        fps: state.selected_fps,
        quality: state.selected_quality,
    });
    let raw = Box::into_raw(result);
    if PostMessageW(state.main_hwnd, WM_SELECTION_COMPLETE, 0, raw as isize) != 0 {
        state.committed = true;
    } else {
        drop(Box::from_raw(raw));
    }
    PostMessageW(state.overlay_hwnd, WM_CLOSE, 0, 0);
}

unsafe fn cancel(state: &mut SelectionController) {
    PostMessageW(state.overlay_hwnd, WM_CLOSE, 0, 0);
}

#[derive(Clone, Copy)]
struct PopupMetrics {
    width: i32,
    height: i32,
    row_height: i32,
    fps_button_width: i32,
    quality_button_width: i32,
    padding: i32,
}

fn scale_px(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

fn popup_metrics(state: &SelectionController) -> PopupMetrics {
    let padding = scale_px(POPUP_PADDING_DIP, state.fps_dpi);
    let row_height = scale_px(ROW_HEIGHT_DIP, state.fps_dpi);
    let fps_button_width = scale_px(FPS_BUTTON_WIDTH_DIP, state.fps_dpi);
    let quality_button_width = scale_px(QUALITY_BUTTON_WIDTH_DIP, state.fps_dpi);
    let fps_count = state.fps_options.len().max(1) as i32;
    let fps_width = fps_count * fps_button_width + padding * 2;
    let quality_width = 4 * quality_button_width + padding * 2;
    PopupMetrics {
        width: fps_width.max(quality_width),
        height: row_height * 2,
        row_height,
        fps_button_width,
        quality_button_width,
        padding,
    }
}

fn fps_hit_test(state: &SelectionController, x: i32, y: i32) -> Option<usize> {
    let metrics = popup_metrics(state);
    if y < 0 || y >= metrics.row_height {
        return None;
    }
    let button_gap = scale_px(4, state.fps_dpi);
    state.fps_options.iter().enumerate().find_map(|(index, _)| {
        let left = metrics.padding + index as i32 * metrics.fps_button_width;
        let right = left + metrics.fps_button_width - button_gap;
        (x >= left && x < right).then_some(index)
    })
}

fn quality_hit_test(state: &SelectionController, x: i32, y: i32) -> Option<usize> {
    let metrics = popup_metrics(state);
    if y < metrics.row_height || y >= metrics.height {
        return None;
    }
    let button_gap = scale_px(4, state.fps_dpi);
    GifQuality::ALL.iter().enumerate().find_map(|(index, _)| {
        let left = metrics.padding + index as i32 * metrics.quality_button_width;
        let right = left + metrics.quality_button_width - button_gap;
        (x >= left && x < right).then_some(index)
    })
}

unsafe fn paint_popup(hwnd: HWND, state: &SelectionController) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        EndPaint(hwnd, &ps);
        return;
    }
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);

    let background = CreateSolidBrush(win32::rgb(31, 31, 34));
    FillRect(hdc, &client, background);
    DeleteObject(background as _);

    let old_font = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(hdc, TRANSPARENT as i32);

    let metrics = popup_metrics(state);
    let vertical_inset = scale_px(7, state.fps_dpi);
    let button_gap = scale_px(4, state.fps_dpi);
    let radius = scale_px(8, state.fps_dpi);

    for (index, fps) in state.fps_options.iter().copied().enumerate() {
        let left = metrics.padding + index as i32 * metrics.fps_button_width;
        let mut rect = RECT {
            left,
            top: vertical_inset,
            right: left + metrics.fps_button_width - button_gap,
            bottom: metrics.row_height - vertical_inset,
        };
        paint_choice_button(
            hdc,
            &mut rect,
            &format!("{fps}"),
            fps == state.selected_fps,
            state.hover_fps == Some(index),
            radius,
        );
    }

    for (index, quality) in GifQuality::ALL.iter().copied().enumerate() {
        let left = metrics.padding + index as i32 * metrics.quality_button_width;
        let mut rect = RECT {
            left,
            top: metrics.row_height + vertical_inset,
            right: left + metrics.quality_button_width - button_gap,
            bottom: metrics.height - vertical_inset,
        };
        paint_choice_button(
            hdc,
            &mut rect,
            quality.label(),
            quality == state.selected_quality,
            state.hover_quality == Some(index),
            radius,
        );
    }

    SelectObject(hdc, old_font);
    EndPaint(hwnd, &ps);
}

unsafe fn paint_choice_button(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &mut RECT,
    label: &str,
    selected: bool,
    hovered: bool,
    radius: i32,
) {
    let color = if hovered {
        win32::rgb(82, 82, 90)
    } else if selected {
        win32::rgb(62, 62, 68)
    } else {
        win32::rgb(43, 43, 47)
    };
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(hdc, brush as _);
    RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius, radius);
    SelectObject(hdc, old_brush);
    DeleteObject(brush as _);

    SetTextColor(hdc, win32::rgb(245, 245, 247));
    let text = win32::wide(label);
    DrawTextW(hdc, text.as_ptr(), -1, rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}
