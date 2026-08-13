//! Small, platform-agnostic value types used across UI and capture modules.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    pub const fn width(self) -> i32 {
        self.right - self.left
    }

    pub const fn height(self) -> i32 {
        self.bottom - self.top
    }

    pub fn normalized(self) -> Self {
        Self {
            left: self.left.min(self.right),
            top: self.top.min(self.bottom),
            right: self.left.max(self.right),
            bottom: self.top.max(self.bottom),
        }
    }

    pub fn clamp_to(self, bounds: Self) -> Self {
        Self {
            left: self.left.clamp(bounds.left, bounds.right),
            top: self.top.clamp(bounds.top, bounds.bottom),
            right: self.right.clamp(bounds.left, bounds.right),
            bottom: self.bottom.clamp(bounds.top, bounds.bottom),
        }
        .normalized()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GifQuality {
    Low,
    #[default]
    Medium,
    High,
    Original,
}

impl GifQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Original];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
            Self::Original => "原始",
        }
    }

    /// Output scale relative to the captured region (100 = native resolution).
    pub const fn scale_percent(self) -> u32 {
        match self {
            Self::Low => 50,
            Self::Medium => 70,
            Self::High => 85,
            Self::Original => 100,
        }
    }

    /// Local GIF palette size. Fewer colors shrinks files and softens gradients.
    pub const fn max_colors(self) -> usize {
        match self {
            Self::Low => 64,
            Self::Medium => 128,
            Self::High => 192,
            Self::Original => 256,
        }
    }

    /// Maps to NeuQuant sample factor (1 = best/slowest, 30 = fastest).
    pub const fn quantizer_speed(self) -> i32 {
        match self {
            Self::Low => 20,
            Self::Medium => 10,
            Self::High => 5,
            Self::Original => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionResult {
    pub rect: ScreenRect,
    /// Raw HMONITOR value stored as an integer so it can safely cross module boundaries.
    pub monitor: isize,
    pub fps: u32,
    pub quality: GifQuality,
}

#[derive(Debug, Clone)]
pub struct RecordingResult {
    pub output_path: Option<std::path::PathBuf>,
    pub copied_to_clipboard: bool,
    pub frames_written: u64,
    pub frames_dropped: u64,
    pub duration_ms: u128,
    /// Non-fatal issue such as clipboard contention. The GIF is still valid.
    pub warning: Option<String>,
    /// Fatal capture/encoding error.
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_reverse_drag() {
        assert_eq!(
            ScreenRect::new(300, 250, 100, 50).normalized(),
            ScreenRect::new(100, 50, 300, 250)
        );
    }

    #[test]
    fn clamps_selection_to_monitor_bounds() {
        let monitor = ScreenRect::new(-1920, 0, 0, 1080);
        let selection = ScreenRect::new(-2000, -20, 100, 1200);
        assert_eq!(selection.clamp_to(monitor), monitor);
    }
}
