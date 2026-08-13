//! Thin Win32 helpers. Keeping unsafe FFI concentrated here makes the rest of the
//! application easier to reason about and audit.

use crate::types::ScreenRect;
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::{
        CombineRgn, CreateBitmap, CreateRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromPoint,
        SetWindowRgn, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_DIFF,
    },
    Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            CreateIconIndirect, GetSystemMetrics, ICONINFO, SM_CXCURSOR, SM_CXVIRTUALSCREEN,
            SM_CYCURSOR, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNORMAL,
            HCURSOR,
        },
    },
};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub fn virtual_screen() -> ScreenRect {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        ScreenRect::new(
            left,
            top,
            left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

pub fn point_from_lparam(lparam: isize) -> (i32, i32) {
    let x = (lparam as u32 & 0xffff) as u16 as i16 as i32;
    let y = ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

pub fn monitor_at_point(x: i32, y: i32) -> Result<(isize, ScreenRect), String> {
    unsafe {
        let monitor: HMONITOR = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return Err("MonitorFromPoint returned null".into());
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err("GetMonitorInfoW failed".into());
        }
        Ok((
            monitor as isize,
            ScreenRect::new(info.rcMonitor.left, info.rcMonitor.top, info.rcMonitor.right, info.rcMonitor.bottom),
        ))
    }
}

pub fn monitor_rect(handle: isize) -> Result<ScreenRect, String> {
    unsafe {
        let monitor = handle as HMONITOR;
        if monitor.is_null() {
            return Err("invalid monitor handle".into());
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err("GetMonitorInfoW failed".into());
        }
        Ok(ScreenRect::new(
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ))
    }
}

pub fn set_hole_region(hwnd: HWND, full_width: i32, full_height: i32, hole: Option<ScreenRect>) {
    unsafe {
        let outer = CreateRectRgn(0, 0, full_width, full_height);
        if outer.is_null() {
            return;
        }
        if let Some(hole) = hole {
            let inner = CreateRectRgn(hole.left, hole.top, hole.right, hole.bottom);
            if !inner.is_null() {
                CombineRgn(outer, outer, inner, RGN_DIFF);
                DeleteObject(inner as _);
            }
        }
        // On success Windows owns the region handle.
        if SetWindowRgn(hwnd, outer, 1) == 0 {
            DeleteObject(outer as _);
        }
    }
}

pub fn set_border_region(hwnd: HWND, width: i32, height: i32, thickness: i32) {
    unsafe {
        let outer = CreateRectRgn(0, 0, width, height);
        let inner = CreateRectRgn(thickness, thickness, width - thickness, height - thickness);
        if outer.is_null() || inner.is_null() {
            if !outer.is_null() { DeleteObject(outer as _); }
            if !inner.is_null() { DeleteObject(inner as _); }
            return;
        }
        CombineRgn(outer, outer, inner, RGN_DIFF);
        DeleteObject(inner as _);
        if SetWindowRgn(hwnd, outer, 1) == 0 {
            DeleteObject(outer as _);
        }
    }
}

pub fn atomic_replace(source: &std::path::Path, destination: &std::path::Path) -> Result<(), String> {
    let source = wide(source.as_os_str());
    let destination = wide(destination.as_os_str());
    unsafe {
        if MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) == 0
        {
            return Err("MoveFileExW atomic replace failed".into());
        }
    }
    Ok(())
}

pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    let path = wide(path.as_os_str());
    unsafe {
        let result = ShellExecuteW(
            ptr::null_mut(),
            windows_sys::w!("open"),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        );
        if result as isize <= 32 {
            return Err(format!("ShellExecuteW failed with code {}", result as isize));
        }
    }
    Ok(())
}

/// Open an interactive CLI session (`settings` / `help`) in a new console window.
/// Writes a tiny `.cmd` launcher and opens it with ShellExecute so Windows creates
/// a normal interactive console (GUI-subsystem parents cannot reliably give Node a TTY).
pub fn open_interactive_cli(command: &str, title: &str) -> Result<(), String> {
    let script = resolve_cli_script().ok_or_else(|| {
        "未找到 bin/gifshot.js。请在项目目录终端运行: node bin/gifshot.js settings".to_string()
    })?;
    let node = resolve_node_exe().ok_or_else(|| {
        "未找到 node.exe。请确认 Node.js 在 PATH 中，或在终端运行: gifshot settings".to_string()
    })?;

    let bat_path = std::env::temp_dir().join(format!("gifshot-{command}-launch.cmd"));
    // Batch cannot safely consume verbatim `\\?\` paths from canonicalize(); strip them.
    let node_s = path_for_batch(&node);
    let script_s = path_for_batch(&script);
    let bat = format!(
        "@echo off\r\n\
         setlocal\r\n\
         chcp 65001 >nul\r\n\
         title {title}\r\n\
         \"{node_s}\" \"{script_s}\" {command} --pause-after\r\n\
         if errorlevel 1 (\r\n\
         echo.\r\n\
         echo [GifShot] 启动失败\r\n\
         pause\r\n\
         )\r\n"
    );
    std::fs::write(&bat_path, bat).map_err(|error| format!("无法写入启动脚本: {error}"))?;
    open_path(&bat_path)
}

/// `Path::canonicalize` on Windows returns `\\?\D:\...`. Inside `.cmd`, `?` is a
/// wildcard and the path collapses (Node then sees only `D:` → EISDIR).
fn path_for_batch(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| {
            text.strip_prefix(r"\\?\")
                .unwrap_or(text.as_ref())
                .to_string()
        });
    text.replace('/', "\\")
}

fn resolve_node_exe() -> Option<std::path::PathBuf> {
    resolve_on_path("node.exe")
        .or_else(|| resolve_on_path("node"))
        .or_else(|| {
            std::env::var_os("NVM_SYMLINK").and_then(|dir| {
                let candidate = std::path::PathBuf::from(dir).join("node.exe");
                candidate.is_file().then_some(candidate)
            })
        })
}

fn resolve_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_cli_script() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let candidates = [
        parent.join("../../bin/gifshot.js"),
        parent.join("../../../bin/gifshot.js"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            // `absolute` keeps a normal `D:\...` path; `canonicalize` adds `\\?\`.
            return std::path::absolute(&candidate).ok().or(Some(candidate));
        }
    }
    None
}

fn message_box(title: &str, text: &str) {
    let title = wide(title);
    let text = wide(text);
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

/// Launch settings/help without blocking the Win32 UI thread (avoids tray freeze).
pub fn open_interactive_cli_async(command: &'static str, title: &'static str) {
    std::thread::spawn(move || {
        if let Err(error) = open_interactive_cli(command, title) {
            message_box("GifShot", &error);
        }
    });
}

/// Monochrome XOR crosshair: pixels invert against whatever is under the cursor,
/// so it stays visible on both light and dark backgrounds (unlike stock IDC_CROSS).
pub fn inverted_cross_cursor() -> Result<HCURSOR, String> {
    static CURSOR: OnceLock<Result<isize, String>> = OnceLock::new();
    let handle = CURSOR.get_or_init(|| unsafe { create_inverted_cross_cursor() }.map(|h| h as isize));
    match handle {
        Ok(ptr) => Ok(*ptr as HCURSOR),
        Err(err) => Err(err.clone()),
    }
}

unsafe fn create_inverted_cross_cursor() -> Result<HCURSOR, String> {
    let width = GetSystemMetrics(SM_CXCURSOR).max(32);
    let height = GetSystemMetrics(SM_CYCURSOR).max(32);
    let stride = ((width + 31) / 32) * 4;
    let plane = (stride * height) as usize;
    // Monochrome cursor mask is AND plane followed by XOR plane.
    let mut bits = vec![0u8; plane * 2];
    // Transparent by default: AND=1, XOR=0.
    bits[..plane].fill(0xff);

    let cx = width / 2;
    let cy = height / 2;
    let arm = (width.min(height) / 2 - 1).max(8);
    let gap = 2;
    let thickness = 1;

    let mut invert = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width || y >= height {
            return;
        }
        let and_index = (y as usize) * stride as usize + (x as usize) / 8;
        let xor_index = plane + and_index;
        let bit = 7 - (x as usize % 8);
        bits[and_index] &= !(1 << bit);
        bits[xor_index] |= 1 << bit;
    };

    for dx in -arm..=arm {
        if dx.abs() <= gap {
            continue;
        }
        for t in -thickness..=thickness {
            invert(cx + dx, cy + t);
            invert(cx + t, cy + dx);
        }
    }

    let mask = CreateBitmap(width, height * 2, 1, 1, bits.as_ptr().cast());
    if mask.is_null() {
        return Err("CreateBitmap(inverted cross cursor) failed".into());
    }

    let info = ICONINFO {
        fIcon: 0,
        xHotspot: cx as u32,
        yHotspot: cy as u32,
        hbmMask: mask,
        hbmColor: ptr::null_mut(),
    };
    let cursor = CreateIconIndirect(&info);
    DeleteObject(mask as _);
    if cursor.is_null() {
        return Err("CreateIconIndirect(inverted cross cursor) failed".into());
    }
    Ok(cursor)
}
