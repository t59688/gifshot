//! Persistent user configuration.
//!
//! The UI intentionally exposes very little. Advanced values live in JSON so the
//! fast capture flow stays as close as possible to Win+Shift+S.

use crate::{hotkey::Hotkey, paths, types::GifQuality, win32};
use serde::{Deserialize, Serialize};
use chrono::Local;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub hotkey: String,
    pub fallback_hotkey: String,
    pub default_fps: u32,
    pub fps_options: Vec<u32>,
    pub default_quality: GifQuality,
    pub capture_cursor: bool,
    pub max_duration_secs: u64,
    pub dim_opacity: u8,
    /// Kept in sync with `default_quality` for advanced manual edits.
    /// Prefer changing `default_quality`; scale/color limits are derived from that enum.
    pub gif_quantizer_speed: i32,
    pub copy_to_clipboard: bool,
    pub show_notifications: bool,
    /// Null means Pictures\\GifShot.
    pub output_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            hotkey: "Win+Shift+G".to_string(),
            fallback_hotkey: "Ctrl+Shift+G".to_string(),
            default_fps: 15,
            fps_options: vec![5, 10, 15, 24],
            default_quality: GifQuality::Medium,
            capture_cursor: true,
            max_duration_secs: 120,
            dim_opacity: 128,
            gif_quantizer_speed: GifQuality::Medium.quantizer_speed(),
            copy_to_clipboard: true,
            show_notifications: true,
            output_dir: None,
        }
    }
}

impl Config {
    pub fn load_or_create() -> io::Result<Self> {
        let path = paths::config_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut cfg = if path.exists() {
            let text = fs::read_to_string(&path)?;
            match serde_json::from_str::<Self>(&text) {
                Ok(cfg) => cfg,
                Err(_) => {
                    // Preserve the user's invalid file for diagnosis instead of silently
                    // destroying it. A normalized default is written below.
                    let stamp = Local::now().format("%Y%m%d-%H%M%S");
                    let backup = path.with_file_name(format!("config.corrupt-{stamp}.json"));
                    let _ = fs::copy(&path, backup);
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        cfg.normalize();
        cfg.save()?;
        Ok(cfg)
    }

    pub fn save(&self) -> io::Result<()> {
        let path = paths::config_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let mut file = File::create(&tmp)?;
        file.write_all(&payload)?;
        file.sync_all()?;
        drop(file);

        if let Err(error) = win32::atomic_replace(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::other(error));
        }
        Ok(())
    }

    pub fn capture_dir(&self) -> PathBuf {
        match &self.output_dir {
            Some(path) if path.is_absolute() => path.clone(),
            Some(path) => paths::config_file()
                .parent()
                .map(|base| base.join(path))
                .unwrap_or_else(|| path.clone()),
            None => paths::default_capture_dir(),
        }
    }

    fn normalize(&mut self) {
        self.schema_version = 1;
        self.fps_options.retain(|v| (1..=30).contains(v));
        self.fps_options.sort_unstable();
        self.fps_options.dedup();
        self.fps_options.truncate(8);
        if self.fps_options.is_empty() {
            self.fps_options = vec![5, 10, 15, 24];
        }
        if !self.fps_options.contains(&self.default_fps) {
            // normalize() guarantees the list is non-empty before this branch.
            if let Some(nearest) = self.fps_options.iter().min_by_key(|fps| fps.abs_diff(15)) {
                self.default_fps = *nearest;
            }
        }
        self.max_duration_secs = self.max_duration_secs.clamp(5, 600);
        self.dim_opacity = self.dim_opacity.clamp(64, 220);
        self.gif_quantizer_speed = self.default_quality.quantizer_speed();
        if Hotkey::parse(&self.hotkey).is_err() {
            self.hotkey = "Win+Shift+G".to_string();
        }
        if Hotkey::parse(&self.fallback_hotkey).is_err() {
            self.fallback_hotkey = "Ctrl+Shift+G".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_invalid_fps_values() {
        let mut cfg = Config {
            fps_options: vec![0, 15, 15, 90],
            default_fps: 24,
            ..Config::default()
        };
        cfg.normalize();
        assert_eq!(cfg.fps_options, vec![15]);
        assert_eq!(cfg.default_fps, 15);
    }
}
