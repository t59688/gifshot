#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![allow(unsafe_op_in_unsafe_fn)]

//! GifShot native entry point.
//!
//! The executable is intentionally UI-framework-free. The npm package is only the
//! distribution/launcher layer; all hotkey, selection, capture, encoding, clipboard,
//! and tray behavior lives in this native binary.

mod app;
mod capture;
mod clipboard;
mod config;
mod encoder;
mod hotkey;
mod hotkey_capture;
mod hud;
mod logging;
mod messages;
mod paths;
mod selection;
mod tray;
mod types;
mod win32;

fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if arg == "capture-hotkey" || arg == "--capture-hotkey" {
        // Avoid starting the resident logger/UI path; this mode is owned by
        // `gifshot settings` and must print a single hotkey line to stdout.
        match hotkey_capture::run() {
            Ok(hotkey) => {
                println!("{hotkey}");
            }
            Err(hotkey_capture::CaptureError::Cancelled) => {
                std::process::exit(1);
            }
            Err(hotkey_capture::CaptureError::Failed(message)) => {
                eprintln!("gifshot: {message}");
                std::process::exit(2);
            }
        }
        return;
    }

    let _log_guard = logging::init();
    let action = app::StartupAction::from_args();
    if let Err(err) = app::run(action) {
        tracing::error!(error = %err, "GifShot terminated with an error");
        show_fatal(&err);
    }
}

#[cfg(target_os = "windows")]
fn show_fatal(message: &str) {
    use crate::win32::wide;
    use std::ptr;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let message = wide(message);
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            windows_sys::w!("GifShot"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn show_fatal(message: &str) {
    eprintln!("GifShot: {message}");
}
