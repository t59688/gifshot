//! Hardware-accelerated monitor capture using Windows.Graphics.Capture via
//! `windows-capture`, with CPU readback restricted to the selected crop only.

use crate::encoder::{self, EncodeSummary, EncoderMessage, EncoderOptions, RawFrame};
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use thiserror::Error;
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl},
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub monitor_handle: isize,
    pub crop: CropRect,
    pub fps: u32,
    pub capture_cursor: bool,
    pub output_dir: std::path::PathBuf,
    pub scale_percent: u32,
    pub max_colors: usize,
    pub quantizer_speed: i32,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("frame buffer error: {0}")]
    Frame(String),
    #[error("encoder queue disconnected")]
    EncoderDisconnected,
}

#[derive(Clone)]
pub struct CaptureFlags {
    crop: CropRect,
    fps: u32,
    tx: Sender<EncoderMessage>,
    eviction_rx: Receiver<EncoderMessage>,
    dropped: Arc<AtomicU64>,
    accepting_frames: Arc<AtomicBool>,
}

pub struct CaptureHandler {
    flags: CaptureFlags,
    pacer: FramePacer,
    scratch: Vec<u8>,
}

#[derive(Debug)]
struct FramePacer {
    interval: Duration,
    next_due: Option<Instant>,
}

impl FramePacer {
    fn new(fps: u32) -> Self {
        Self {
            interval: Duration::from_secs_f64(1.0 / f64::from(fps.max(1))),
            next_due: None,
        }
    }

    /// Samples against an absolute target timeline rather than measuring from the
    /// previous accepted callback. That distinction matters on a 60 Hz source:
    /// naive 24 FPS gating accepts every third callback (~20 FPS), while advancing
    /// the target phase produces the correct 50/33 ms cadence and 24 FPS average.
    fn should_sample(&mut self, now: Instant) -> bool {
        let Some(mut due) = self.next_due else {
            self.next_due = Some(now + self.interval);
            return true;
        };
        if now < due {
            return false;
        }

        // Do not spend an unbounded loop catching up after sleep/resume or a long
        // GPU stall. Re-anchor after a large discontinuity; otherwise preserve the
        // target phase so refresh-rate quantization does not bias the average FPS.
        if now.saturating_duration_since(due) > Duration::from_secs(1) {
            due = now + self.interval;
        } else {
            while due <= now {
                due += self.interval;
            }
        }
        self.next_due = Some(due);
        true
    }
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let pacer = FramePacer::new(ctx.flags.fps);
        Ok(Self { flags: ctx.flags, pacer, scratch: Vec::new() })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Stopping is two-phase: the UI thread closes this gate immediately,
        // then a finalizer thread performs the potentially blocking WGC shutdown.
        // This keeps frames captured during API teardown out of the final GIF.
        if !self.flags.accepting_frames.load(Ordering::Acquire) {
            return Ok(());
        }

        let now = Instant::now();
        if !self.pacer.should_sample(now) {
            return Ok(());
        }

        let crop = self.flags.crop;
        if crop.x.saturating_add(crop.width) > frame.width()
            || crop.y.saturating_add(crop.height) > frame.height()
        {
            return Err(CaptureError::Frame(format!(
                "crop {}x{}+{},{} exceeds frame {}x{}",
                crop.width, crop.height, crop.x, crop.y, frame.width(), frame.height()
            )));
        }

        let buffer = frame
            .buffer_crop(crop.x, crop.y, crop.x + crop.width, crop.y + crop.height)
            .map_err(|e| CaptureError::Frame(e.to_string()))?;
        let bytes = buffer.as_nopadding_buffer(&mut self.scratch);
        if !self.flags.accepting_frames.load(Ordering::Acquire) {
            return Ok(());
        }
        let message = EncoderMessage::Frame(RawFrame { bgra: bytes.to_vec(), captured_at: now });

        match self.flags.tx.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(message)) => {
                // Prefer fresh UI state over stale queued frames. This bounds memory
                // while preventing a slow GIF quantizer from making output trail
                // noticeably behind what the user was doing on screen.
                match self.flags.eviction_rx.try_recv() {
                    Ok(EncoderMessage::Frame(_)) => {
                        self.flags.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(EncoderMessage::Finish(_)) => {
                        // Finish is only sent after capture is stopped, so reaching
                        // this branch would violate the session lifecycle invariant.
                        return Err(CaptureError::EncoderDisconnected);
                    }
                    Err(TryRecvError::Disconnected) => return Err(CaptureError::EncoderDisconnected),
                    Err(TryRecvError::Empty) => {}
                }
                match self.flags.tx.try_send(message) {
                    Ok(()) => Ok(()),
                    Err(TrySendError::Full(_)) => {
                        self.flags.dropped.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(TrySendError::Disconnected(_)) => Err(CaptureError::EncoderDisconnected),
                }
            }
            Err(TrySendError::Disconnected(_)) => Err(CaptureError::EncoderDisconnected),
        }
    }
}

pub struct CaptureSession {
    control: CaptureControl<CaptureHandler, CaptureError>,
    encoder_tx: Sender<EncoderMessage>,
    encoder_join: JoinHandle<Result<EncodeSummary, String>>,
    dropped: Arc<AtomicU64>,
    accepting_frames: Arc<AtomicBool>,
    pub started_at: Instant,
}

pub struct StopSummary {
    pub encoded: Result<EncodeSummary, String>,
    pub capture_error: Option<String>,
    pub frames_dropped: u64,
}

impl CaptureSession {
    pub fn start(options: CaptureOptions) -> Result<Self, String> {
        let dropped = Arc::new(AtomicU64::new(0));
        let accepting_frames = Arc::new(AtomicBool::new(true));
        let (encoder_tx, eviction_rx, encoder_join) = encoder::start(EncoderOptions {
            width: options.crop.width,
            height: options.crop.height,
            output_dir: options.output_dir,
            scale_percent: options.scale_percent,
            max_colors: options.max_colors,
            quantizer_speed: options.quantizer_speed,
        })?;

        let flags = CaptureFlags {
            crop: options.crop,
            fps: options.fps,
            tx: encoder_tx.clone(),
            eviction_rx,
            dropped: dropped.clone(),
            accepting_frames: accepting_frames.clone(),
        };

        let monitor = Monitor::from_raw_hmonitor(options.monitor_handle as *mut _);

        // Cursor/border controls were added to Windows.Graphics.Capture after the
        // base capture API. Production compatibility therefore uses explicit
        // settings only when the OS reports them as supported. In particular,
        // forcing WithoutBorder on Windows 10 builds that lack the property would
        // make the entire capture session fail with BorderConfigUnsupported.
        let cursor_setting = if GraphicsCaptureApi::is_cursor_settings_supported().unwrap_or(false) {
            if options.capture_cursor {
                CursorCaptureSettings::WithCursor
            } else {
                CursorCaptureSettings::WithoutCursor
            }
        } else {
            CursorCaptureSettings::Default
        };
        let border_setting = if GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
            DrawBorderSettings::WithoutBorder
        } else {
            DrawBorderSettings::Default
        };

        let settings = Settings::new(
            monitor,
            cursor_setting,
            border_setting,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );

        let control = CaptureHandler::start_free_threaded(settings).map_err(|e| e.to_string())?;
        Ok(Self {
            control,
            encoder_tx,
            encoder_join,
            dropped,
            accepting_frames,
            started_at: Instant::now(),
        })
    }

    pub fn is_finished(&self) -> bool {
        self.control.is_finished()
    }

    /// Immediately prevents new encoder frames without blocking the caller. The
    /// actual WGC shutdown is intentionally deferred to [`Self::stop`].
    pub fn request_stop(&self) {
        self.accepting_frames.store(false, Ordering::Release);
    }

    /// Blocking shutdown. Call this from a worker thread, never from the Win32 UI thread.
    /// `finished_at` is the user-visible stop instant, not the time WGC teardown returns.
    pub fn stop(self, finished_at: Instant) -> StopSummary {
        self.accepting_frames.store(false, Ordering::Release);
        let capture_error = self.control.stop().err().map(|e| e.to_string());
        let _ = self.encoder_tx.send(EncoderMessage::Finish(finished_at));
        drop(self.encoder_tx);

        let encoded = self
            .encoder_join
            .join()
            .map_err(|_| "GIF encoder thread panicked".to_string())
            .and_then(|r| r);

        StopSummary {
            encoded,
            capture_error,
            frames_dropped: self.dropped.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_locked_pacer_tracks_24fps_on_60hz_callbacks() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(24);
        let samples = (0..60)
            .filter(|tick| {
                let at = start + Duration::from_micros(16_667 * *tick as u64);
                pacer.should_sample(at)
            })
            .count();
        assert!((23..=25).contains(&samples), "sample count was {samples}");
    }

    #[test]
    fn pacer_reanchors_after_long_stall() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(15);
        assert!(pacer.should_sample(start));
        assert!(pacer.should_sample(start + Duration::from_secs(3)));
        assert!(!pacer.should_sample(start + Duration::from_secs(3) + Duration::from_millis(10)));
    }
}
