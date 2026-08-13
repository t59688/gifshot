//! Windows-standard per-user storage locations for config, logs, and recordings.

use directories::{BaseDirs, UserDirs};
use std::path::PathBuf;

const APP_DIR: &str = "GifShot";

/// `%APPDATA%\\GifShot\\config.json` on Windows.
pub fn config_file() -> PathBuf {
    BaseDirs::new()
        .map(|p| p.config_dir().join(APP_DIR).join("config.json"))
        .unwrap_or_else(|| PathBuf::from(APP_DIR).join("config.json"))
}

/// `%LOCALAPPDATA%\\GifShot\\logs` on Windows.
pub fn log_dir() -> PathBuf {
    BaseDirs::new()
        .map(|p| p.data_local_dir().join(APP_DIR).join("logs"))
        .unwrap_or_else(|| PathBuf::from(APP_DIR).join("logs"))
}

/// `%USERPROFILE%\\Pictures\\GifShot` when the Pictures known folder exists.
pub fn default_capture_dir() -> PathBuf {
    UserDirs::new()
        .and_then(|u| u.picture_dir().map(|p| p.join(APP_DIR)))
        .unwrap_or_else(|| PathBuf::from(APP_DIR))
}
