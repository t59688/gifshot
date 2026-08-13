//! Application orchestration and Win32 message loop.
//!
//! The runtime is a single-instance, event-driven process. UI work stays on the
//! Win32 thread; capture runs on the Windows.Graphics.Capture worker supplied by
//! `windows-capture`; GIF encoding runs on a dedicated bounded-queue worker.

use crate::{
    capture::{CaptureOptions, CaptureSession, CropRect},
    clipboard,
    config::Config,
    hotkey::Hotkey,
    hud::RecordingHud,
    messages::*,
    paths,
    selection,
    tray::{self, TrayIcon},
    types::{RecordingResult, SelectionResult},
    win32,
};
use std::{
    fs,
    mem,
    path::PathBuf,
    ptr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tracing::{error, info, warn};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM, ERROR_ALREADY_EXISTS},
    System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey},
        WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
            FindWindowW, GWLP_USERDATA, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
            IMAGE_ICON, KillTimer, LoadIconW, LoadImageW, MSG, PostMessageW, PostQuitMessage,
            RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SetTimer,
            SetWindowLongPtrW, TranslateMessage, WNDCLASSEXW, WM_CLOSE, WM_DESTROY, WM_HOTKEY,
            WM_LBUTTONDBLCLK, WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONUP, WM_TIMER, WS_EX_TOOLWINDOW,
            WS_OVERLAPPED,
        },
    },
};

const MAIN_CLASS: &str = "GifShot.MainWindow";
const HOTKEY_ID: i32 = 0x4746; // 'GF'
const HEALTH_TIMER_ID: usize = 0x4746;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupAction {
    /// Start the resident process but do not open the selector.
    Background,
    Toggle,
    Stop,
    Quit,
    OpenCaptures,
    OpenConfig,
    ReloadConfig,
}

impl StartupAction {
    pub fn from_args() -> Self {
        let arg = std::env::args().nth(1).unwrap_or_default().to_ascii_lowercase();
        match arg.as_str() {
            "--background" | "background" | "start" => Self::Background,
            "stop" | "--stop" => Self::Stop,
            "quit" | "--quit" => Self::Quit,
            "open" | "captures" | "--open" => Self::OpenCaptures,
            "config" | "--config" => Self::OpenConfig,
            "reload" | "--reload" | "reload-config" => Self::ReloadConfig,
            "capture" | "--capture" | "" => Self::Toggle,
            _ => Self::Toggle,
        }
    }

    fn message(self) -> Option<u32> {
        match self {
            Self::Background => None,
            Self::Toggle => Some(WM_GIFSHOT_TOGGLE),
            Self::Stop => Some(WM_GIFSHOT_STOP),
            Self::Quit => Some(WM_GIFSHOT_QUIT),
            Self::OpenCaptures => Some(WM_GIFSHOT_OPEN_CAPTURES),
            Self::OpenConfig => Some(WM_GIFSHOT_OPEN_CONFIG),
            Self::ReloadConfig => Some(WM_GIFSHOT_RELOAD_CONFIG),
        }
    }
}

enum RuntimeState {
    Idle,
    Selecting { hwnd: HWND },
    Recording {
        session: CaptureSession,
        hud: RecordingHud,
        rect: crate::types::ScreenRect,
        fps: u32,
    },
    Encoding,
}

impl RuntimeState {
    fn is_recording(&self) -> bool {
        matches!(self, Self::Recording { .. })
    }
}

struct InstanceMutex(HANDLE);

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CloseHandle(self.0);
                self.0 = ptr::null_mut();
            }
        }
    }
}

struct App {
    hwnd: HWND,
    config: Config,
    state: RuntimeState,
    tray: Option<TrayIcon>,
    registered_hotkey: Option<Hotkey>,
    registered_hotkey_label: String,
    quit_after_finish: bool,
    taskbar_created_msg: u32,
    _instance_mutex: InstanceMutex,
}

pub fn run(action: StartupAction) -> Result<(), String> {
    unsafe {
        // Selection coordinates and WGC pixels must use the same physical coordinate
        // system, especially on mixed-DPI desktops.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // File-opening commands do not need the resident runtime or the singleton
    // mutex. Handling them directly also avoids a startup race where a short-lived
    // `gifshot open` process could temporarily make a capture invocation believe a
    // recorder instance was starting.
    match action {
        StartupAction::OpenCaptures => {
            let config = Config::load_or_create().map_err(|e| e.to_string())?;
            let dir = config.capture_dir();
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            return win32::open_path(&dir);
        }
        StartupAction::OpenConfig => {
            Config::load_or_create().map_err(|e| e.to_string())?;
            return win32::open_path(&paths::config_file());
        }
        _ => {}
    }

    let class = win32::wide(MAIN_CLASS);
    let instance_mutex = unsafe {
        let handle = CreateMutexW(
            ptr::null(),
            0,
            windows_sys::w!("Local\\GifShot.Singleton.v1"),
        );
        if handle.is_null() {
            return Err("CreateMutexW(single instance) failed".into());
        }
        let already_exists = GetLastError() == ERROR_ALREADY_EXISTS;
        (InstanceMutex(handle), already_exists)
    };

    if instance_mutex.1 {
        // The mutex is created before the hidden control window, so a second process
        // can observe the singleton during the first process's startup. Wait briefly
        // for that rendezvous window instead of racing into a second runtime.
        for _ in 0..100 {
            let existing = unsafe { FindWindowW(class.as_ptr(), ptr::null()) };
            if !existing.is_null() {
                if let Some(message) = action.message() {
                    let posted = unsafe { PostMessageW(existing, message, 0, 0) };
                    if posted == 0 {
                        return Err("could not deliver command to the running GifShot instance".into());
                    }
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        return Err("another GifShot instance is starting, but its control window was not found".into());
    }
    let instance_mutex = instance_mutex.0;

    // Stop/quit/reload are idempotent when no resident process exists.
    if matches!(
        action,
        StartupAction::Stop | StartupAction::Quit | StartupAction::ReloadConfig
    ) {
        return Ok(());
    }

    register_main_class()?;
    let config = Config::load_or_create().map_err(|e| e.to_string())?;
    let app = Box::new(App {
        hwnd: ptr::null_mut(),
        config,
        state: RuntimeState::Idle,
        tray: None,
        registered_hotkey: None,
        registered_hotkey_label: String::new(),
        quit_after_finish: false,
        taskbar_created_msg: 0,
        _instance_mutex: instance_mutex,
    });
    let raw = Box::into_raw(app);

    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            windows_sys::w!("GifShot"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if hwnd.is_null() {
            drop(Box::from_raw(raw));
            return Err("CreateWindowExW(main) failed".into());
        }
        (*raw).hwnd = hwnd;
        // Attach owned state only after window creation succeeds. This avoids the
        // ambiguous CreateWindowEx failure path where WM_NCDESTROY may already have
        // run and prevents a potential double-free of the state box.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);

        if let Err(e) = (*raw).initialize() {
            error!(error = %e, "application initialization failed");
            DestroyWindow(hwnd);
            return Err(e);
        }

        if action == StartupAction::Toggle {
            PostMessageW(hwnd, WM_GIFSHOT_TOGGLE, 0, 0);
        }

        let mut msg: MSG = mem::zeroed();
        loop {
            let status = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if status == -1 {
                return Err("GetMessageW failed".into());
            }
            if status == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

fn register_main_class() -> Result<(), String> {
    use std::sync::OnceLock;
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

    REGISTERED
        .get_or_init(|| unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let mut class: WNDCLASSEXW = mem::zeroed();
            class.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
            class.lpfnWndProc = Some(main_proc);
            class.hInstance = instance;
            // Resource ID 1 is the app icon embedded by winresource from assets/gifshot.ico.
            class.hIcon = LoadIconW(instance, 1 as *const u16);
            class.hIconSm = LoadImageW(
                instance,
                1 as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                0,
            ) as _;
            class.lpszClassName = windows_sys::w!("GifShot.MainWindow");
            if RegisterClassExW(&class) == 0 {
                return Err("RegisterClassExW(main) failed".into());
            }
            Ok(())
        })
        .clone()
}

fn append_message(slot: &mut Option<String>, message: String) {
    match slot {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(&message);
        }
        None => *slot = Some(message),
    }
}


fn finalize_recording(
    session: CaptureSession,
    stop_requested_at: std::time::Instant,
    started_at: std::time::Instant,
    copy_to_clipboard: bool,
) -> RecordingResult {
    let summary = session.stop(stop_requested_at);
    let duration_ms = stop_requested_at
        .saturating_duration_since(started_at)
        .as_millis();
    let capture_warning = summary.capture_error;
    let mut warning = None;
    let mut error = None;
    let mut output_path: Option<PathBuf> = None;
    let mut copied_to_clipboard = false;
    let mut frames_written = 0;

    match summary.encoded {
        Ok(encoded) => {
            frames_written = encoded.frames_written;
            output_path = Some(encoded.path.clone());

            // A WGC shutdown error can happen after useful frames have already
            // been encoded. Preserve the valid GIF and surface it as a warning
            // rather than incorrectly classifying the whole recording as failed.
            warning = capture_warning.map(|e| format!("Capture ended with a warning: {e}"));

            if copy_to_clipboard {
                match clipboard::set_file(&encoded.path) {
                    Ok(()) => copied_to_clipboard = true,
                    Err(e) => append_message(&mut warning, format!("Clipboard copy failed: {e}")),
                }
            }
        }
        Err(e) => {
            let mut message = format!("GIF encoding failed: {e}");
            if let Some(capture_error) = capture_warning {
                message.push_str(&format!("; capture error: {capture_error}"));
            }
            error = Some(message);
        }
    }

    RecordingResult {
        output_path,
        copied_to_clipboard,
        frames_written,
        frames_dropped: summary.frames_dropped,
        duration_ms,
        warning,
        error,
    }
}

impl App {
    unsafe fn initialize(&mut self) -> Result<(), String> {
        let (hotkey, label) = self.register_best_hotkey()?;
        self.registered_hotkey = Some(hotkey);
        self.registered_hotkey_label = label;

        self.taskbar_created_msg = RegisterWindowMessageW(windows_sys::w!("TaskbarCreated"));
        let mut tray = TrayIcon::new(self.hwnd, &self.registered_hotkey_label);
        if let Err(e) = tray.restore() {
            warn!(error = %e, "notification-area icon unavailable; will retry after Explorer restarts");
        }
        self.tray = Some(tray);
        if self.registered_hotkey_label != self.config.hotkey {
            self.notify_info(
                "GifShot hotkey",
                &format!(
                    "{} is unavailable; using {}",
                    self.config.hotkey, self.registered_hotkey_label
                ),
            );
        }
        SetTimer(self.hwnd, HEALTH_TIMER_ID, 1000, None);
        info!(hotkey = %self.registered_hotkey_label, "GifShot ready");
        Ok(())
    }

    unsafe fn register_best_hotkey(&self) -> Result<(Hotkey, String), String> {
        let preferred = Hotkey::parse(&self.config.hotkey)?;
        if RegisterHotKey(self.hwnd, HOTKEY_ID, preferred.modifiers, preferred.vk) != 0 {
            return Ok((preferred, self.config.hotkey.clone()));
        }

        let fallback = Hotkey::parse(&self.config.fallback_hotkey)?;
        if RegisterHotKey(self.hwnd, HOTKEY_ID, fallback.modifiers, fallback.vk) != 0 {
            warn!(preferred = %self.config.hotkey, fallback = %self.config.fallback_hotkey, "preferred hotkey unavailable; using fallback");
            return Ok((fallback, self.config.fallback_hotkey.clone()));
        }

        Err(format!(
            "cannot register hotkey '{}' or fallback '{}'",
            self.config.hotkey, self.config.fallback_hotkey
        ))
    }

    unsafe fn toggle(&mut self) {
        match &self.state {
            RuntimeState::Idle => self.start_selection(),
            RuntimeState::Selecting { hwnd } => {
                let hwnd = *hwnd;
                if !hwnd.is_null() {
                    DestroyWindow(hwnd);
                }
                self.state = RuntimeState::Idle;
            }
            RuntimeState::Recording { .. } => self.stop_recording(false),
            RuntimeState::Encoding => self.notify_info("GifShot", "Finishing previous GIF…"),
        }
    }

    unsafe fn start_selection(&mut self) {
        match selection::show(
            self.hwnd,
            self.config.fps_options.clone(),
            self.config.default_fps,
            self.config.default_quality,
            self.config.dim_opacity,
        ) {
            Ok(hwnd) => self.state = RuntimeState::Selecting { hwnd },
            Err(e) => {
                error!(error = %e, "failed to show selector");
                self.state = RuntimeState::Idle;
                self.notify_error("GifShot", &format!("Could not start capture: {e}"));
            }
        }
    }

    unsafe fn selection_complete(&mut self, result: SelectionResult) {
        if let RuntimeState::Selecting { hwnd } = &self.state {
            let hwnd = *hwnd;
            if !hwnd.is_null() {
                // Synchronously remove the dimmer/FPS windows before WGC can deliver
                // the first captured frame.
                DestroyWindow(hwnd);
            }
        }
        self.state = RuntimeState::Idle;

        self.config.default_fps = result.fps;
        self.config.default_quality = result.quality;
        self.config.gif_quantizer_speed = result.quality.quantizer_speed();
        if let Err(e) = self.config.save() {
            warn!(error = %e, "could not persist capture preferences");
        }

        if let Err(e) = self.begin_recording(result) {
            error!(error = %e, "failed to begin recording");
            self.notify_error("GifShot", &format!("Could not start recording: {e}"));
        }
    }

    unsafe fn begin_recording(&mut self, result: SelectionResult) -> Result<(), String> {
        let monitor = win32::monitor_rect(result.monitor)?;
        let rect = result.rect.clamp_to(monitor);
        if rect.width() < 16 || rect.height() < 16 {
            return Err("capture region is too small".into());
        }

        let crop = CropRect {
            x: u32::try_from(rect.left - monitor.left).map_err(|_| "invalid crop x")?,
            y: u32::try_from(rect.top - monitor.top).map_err(|_| "invalid crop y")?,
            width: u32::try_from(rect.width()).map_err(|_| "invalid crop width")?,
            height: u32::try_from(rect.height()).map_err(|_| "invalid crop height")?,
        };
        // GIF dimensions are 16-bit by format definition. Fail explicitly instead
        // of producing a corrupt file on an exotic wall-size display.
        if crop.width > u16::MAX as u32 || crop.height > u16::MAX as u32 {
            return Err("capture region exceeds GIF's 65,535 pixel dimension limit".into());
        }

        let output_dir = self.config.capture_dir();
        fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
        let session = CaptureSession::start(CaptureOptions {
            monitor_handle: result.monitor,
            crop,
            fps: result.fps,
            capture_cursor: self.config.capture_cursor,
            output_dir,
            scale_percent: result.quality.scale_percent(),
            max_colors: result.quality.max_colors(),
            quantizer_speed: result.quality.quantizer_speed(),
        })?;

        let hud = match RecordingHud::show(
            self.hwnd,
            rect,
            self.config.max_duration_secs,
            session.started_at,
        ) {
            Ok(hud) => hud,
            Err(e) => {
                // Capture is already live. This is an exceptional startup failure,
                // so prefer a short synchronous shutdown over risking an invisible
                // recorder if the OS also refuses to create a cleanup thread.
                let stop_at = std::time::Instant::now();
                session.request_stop();
                let _ = session.stop(stop_at);
                return Err(e);
            }
        };

        info!(
            fps = result.fps,
            quality = ?result.quality,
            width = crop.width,
            height = crop.height,
            "recording started"
        );
        self.state = RuntimeState::Recording { session, hud, rect, fps: result.fps };
        Ok(())
    }

    unsafe fn stop_recording(&mut self, quit_after: bool) {
        if quit_after {
            self.quit_after_finish = true;
        }
        let current = mem::replace(&mut self.state, RuntimeState::Encoding);
        let (session, hud, rect, fps) = match current {
            RuntimeState::Recording { session, hud, rect, fps } => (session, hud, rect, fps),
            other => {
                self.state = other;
                if quit_after && !matches!(&self.state, RuntimeState::Encoding) {
                    // Route main-window destruction through the private shutdown
                    // message so this method's mutable App borrow has ended before
                    // DestroyWindow synchronously re-enters main_proc.
                    PostMessageW(self.hwnd, WM_GIFSHOT_SHUTDOWN, 0, 0);
                }
                return;
            }
        };

        let stop_requested_at = std::time::Instant::now();
        session.request_stop();
        drop(hud);
        let main = self.hwnd as isize;
        let copy_to_clipboard = self.config.copy_to_clipboard;
        let started_at = session.started_at;
        info!(fps, width = rect.width(), height = rect.height(), "stopping recording");

        // Keep the session recoverable until the finalizer thread has actually
        // been created. `thread::Builder::spawn` can fail under resource pressure;
        // moving CaptureSession directly into the closure would then drop the only
        // owner without explicitly stopping WGC or joining the encoder.
        let session_cell = Arc::new(Mutex::new(Some(session)));
        let worker_cell = Arc::clone(&session_cell);
        let spawn_result = thread::Builder::new()
            .name("gifshot-finalizer".into())
            .spawn(move || {
                let session = worker_cell
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                let Some(session) = session else {
                    return;
                };
                let result = finalize_recording(
                    session,
                    stop_requested_at,
                    started_at,
                    copy_to_clipboard,
                );
                unsafe {
                    let raw = Box::into_raw(Box::new(result));
                    if PostMessageW(main as HWND, WM_RECORDING_FINISHED, 0, raw as isize) == 0 {
                        drop(Box::from_raw(raw));
                    }
                }
            });
        if let Err(e) = spawn_result {
            error!(error = %e, "failed to spawn finalizer thread; finalizing synchronously");
            // This path is intentionally synchronous. Thread creation failure is
            // exceptional, and correctness (stop capture, join encoder, remove .part)
            // is more important than keeping the UI responsive for this one failure.
            let session = session_cell
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(session) = session {
                let result = finalize_recording(
                    session,
                    stop_requested_at,
                    started_at,
                    copy_to_clipboard,
                );
                self.recording_finished(result);
            } else {
                self.state = RuntimeState::Idle;
                self.notify_error("GifShot", "Could not finalize recording");
            }
        }
    }

    unsafe fn recording_finished(&mut self, result: RecordingResult) {
        self.state = RuntimeState::Idle;
        if let Some(err) = &result.error {
            error!(error = %err, dropped = result.frames_dropped, "recording failed");
            self.notify_error("GifShot recording failed", err);
        } else if let Some(path) = &result.output_path {
            info!(
                path = %path.display(),
                frames = result.frames_written,
                dropped = result.frames_dropped,
                duration_ms = result.duration_ms,
                clipboard = result.copied_to_clipboard,
                "recording saved"
            );
            if let Some(warning) = &result.warning {
                warn!(warning = %warning, "recording completed with warning");
                self.notify_warning("GifShot saved with warning", warning);
            } else {
                let text = if result.copied_to_clipboard {
                    format!("Saved and copied: {}", path.file_name().unwrap_or_default().to_string_lossy())
                } else {
                    format!("Saved: {}", path.file_name().unwrap_or_default().to_string_lossy())
                };
                self.notify_info("GIF ready", &text);
            }
        }

        if self.quit_after_finish {
            PostMessageW(self.hwnd, WM_GIFSHOT_SHUTDOWN, 0, 0);
        }
    }

    unsafe fn open_captures(&self) {
        let dir = self.config.capture_dir();
        if let Err(e) = fs::create_dir_all(&dir).and_then(|_| {
            win32::open_path(&dir).map_err(std::io::Error::other)
        }) {
            self.notify_error("GifShot", &format!("Could not open captures: {e}"));
        }
    }

    unsafe fn open_config(&self) {
        if let Err(e) = self.config.save().and_then(|_| {
            win32::open_path(&paths::config_file()).map_err(std::io::Error::other)
        }) {
            self.notify_error("GifShot", &format!("Could not open config: {e}"));
        }
    }

    unsafe fn open_settings_cli(&self) {
        win32::open_interactive_cli_async("settings", "GifShot 设置");
    }

    unsafe fn open_help_cli(&self) {
        win32::open_interactive_cli_async("help", "GifShot 帮助");
    }

    unsafe fn reload_config(&mut self) {
        match Config::load_or_create() {
            Ok(config) => self.config = config,
            Err(e) => {
                self.notify_error("GifShot", &format!("无法重新加载配置：{e}"));
                return;
            }
        }

        if self.registered_hotkey.is_some() {
            UnregisterHotKey(self.hwnd, HOTKEY_ID);
            self.registered_hotkey = None;
        }

        match self.register_best_hotkey() {
            Ok((hotkey, label)) => {
                self.registered_hotkey = Some(hotkey);
                self.registered_hotkey_label = label.clone();
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_tooltip(&format!("GifShot — {label}"));
                }
                if label != self.config.hotkey {
                    self.notify_info(
                        "GifShot",
                        &format!("{} 不可用，已使用 {}", self.config.hotkey, label),
                    );
                } else {
                    self.notify_info("GifShot", &format!("快捷键已更新为 {label}"));
                }
                info!(hotkey = %label, "config reloaded");
            }
            Err(e) => {
                self.notify_error("GifShot", &format!("快捷键注册失败：{e}"));
            }
        }
    }

    unsafe fn request_quit(&mut self) {
        match &self.state {
            RuntimeState::Recording { .. } => self.stop_recording(true),
            RuntimeState::Encoding => self.quit_after_finish = true,
            RuntimeState::Selecting { hwnd } => {
                let hwnd = *hwnd;
                if !hwnd.is_null() {
                    DestroyWindow(hwnd);
                }
                self.state = RuntimeState::Idle;
                PostMessageW(self.hwnd, WM_GIFSHOT_SHUTDOWN, 0, 0);
            }
            RuntimeState::Idle => {
                PostMessageW(self.hwnd, WM_GIFSHOT_SHUTDOWN, 0, 0);
            }
        }
    }

    fn notify_info(&self, title: &str, text: &str) {
        if self.config.show_notifications {
            if let Some(tray) = &self.tray {
                tray.notify_info(title, text);
            }
        }
    }

    fn notify_warning(&self, title: &str, text: &str) {
        if self.config.show_notifications {
            if let Some(tray) = &self.tray {
                tray.notify_warning(title, text);
            }
        }
    }

    fn notify_error(&self, title: &str, text: &str) {
        if self.config.show_notifications {
            if let Some(tray) = &self.tray {
                tray.notify_error(title, text);
            }
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;

    // DestroyWindow synchronously re-enters the window procedure with WM_DESTROY /
    // WM_NCDESTROY. Handle final teardown before creating an &mut App so there can
    // be no re-entrant mutable alias to the same Rust object.
    if msg == WM_GIFSHOT_SHUTDOWN {
        if raw.is_null() {
            return 0;
        }
        {
            let app = &mut *raw;
            KillTimer(hwnd, HEALTH_TIMER_ID);
            if app.registered_hotkey.is_some() {
                UnregisterHotKey(hwnd, HOTKEY_ID);
                app.registered_hotkey = None;
            }
            app.tray.take();
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        DestroyWindow(hwnd);
        drop(Box::from_raw(raw));
        PostQuitMessage(0);
        return 0;
    }

    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !raw.is_null() {
            drop(Box::from_raw(raw));
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if raw.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let app = &mut *raw;

    if app.taskbar_created_msg != 0 && msg == app.taskbar_created_msg {
        if let Some(tray) = app.tray.as_mut() {
            if let Err(e) = tray.restore() {
                warn!(error = %e, "failed to restore notification-area icon after Explorer restart");
            }
        }
        return 0;
    }

    match msg {
        WM_HOTKEY if wparam as i32 == HOTKEY_ID => {
            app.toggle();
            0
        }
        WM_GIFSHOT_TOGGLE => {
            app.toggle();
            0
        }
        WM_GIFSHOT_STOP | WM_HUD_MAX_DURATION => {
            if matches!(&app.state, RuntimeState::Recording { .. }) {
                app.stop_recording(false);
            }
            0
        }
        WM_GIFSHOT_QUIT => {
            app.request_quit();
            0
        }
        WM_GIFSHOT_OPEN_CAPTURES => {
            app.open_captures();
            0
        }
        WM_GIFSHOT_OPEN_CONFIG => {
            app.open_config();
            0
        }
        WM_GIFSHOT_RELOAD_CONFIG => {
            app.reload_config();
            0
        }
        WM_SELECTION_COMPLETE => {
            if lparam != 0 {
                let result = *Box::from_raw(lparam as *mut SelectionResult);
                app.selection_complete(result);
            }
            0
        }
        WM_SELECTION_CANCELLED => {
            if matches!(&app.state, RuntimeState::Selecting { .. }) {
                app.state = RuntimeState::Idle;
            }
            0
        }
        WM_RECORDING_FINISHED => {
            if lparam != 0 {
                let result = *Box::from_raw(lparam as *mut RecordingResult);
                app.recording_finished(result);
            }
            0
        }
        WM_TIMER if wparam == HEALTH_TIMER_ID => {
            let should_stop = match &app.state {
                RuntimeState::Recording { session, .. } => {
                    session.is_finished()
                        || session.started_at.elapsed().as_secs() >= app.config.max_duration_secs
                }
                _ => false,
            };
            if should_stop {
                app.stop_recording(false);
            }
            0
        }
        WM_TRAY_CALLBACK => {
            let event = lparam as u32;
            if event == WM_RBUTTONUP {
                if let Some(tray_icon) = &app.tray {
                    if let Some(command) = tray_icon.show_menu(app.state.is_recording()) {
                        match command {
                            tray::CMD_CAPTURE_OR_STOP => app.toggle(),
                            tray::CMD_SETTINGS => app.open_settings_cli(),
                            tray::CMD_HELP => app.open_help_cli(),
                            tray::CMD_QUIT => app.request_quit(),
                            _ => {}
                        }
                    }
                }
            } else if event == WM_LBUTTONDBLCLK {
                app.toggle();
            }
            0
        }
        WM_CLOSE => {
            app.request_quit();
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, HEALTH_TIMER_ID);
            if app.registered_hotkey.is_some() {
                UnregisterHotKey(hwnd, HOTKEY_ID);
                app.registered_hotkey = None;
            }
            app.tray.take();
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
