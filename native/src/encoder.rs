//! Streaming GIF encoder.
//!
//! Frames are encoded while recording instead of accumulated in RAM. A one-frame
//! look-behind lets us merge identical frames and assign their full on-screen
//! duration to a single GIF frame, which is especially effective for UI demos.

use chrono::Local;
use crossbeam_channel::{Receiver, Sender, bounded};
use gif::{Encoder, Frame, Repeat};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct RawFrame {
    pub bgra: Vec<u8>,
    pub captured_at: Instant,
}

#[derive(Debug)]
pub enum EncoderMessage {
    Frame(RawFrame),
    Finish(Instant),
}

#[derive(Debug, Clone)]
pub struct EncoderOptions {
    pub width: u32,
    pub height: u32,
    pub output_dir: PathBuf,
    pub scale_percent: u32,
    pub max_colors: usize,
    pub quantizer_speed: i32,
}

#[derive(Debug, Clone)]
pub struct EncodeSummary {
    pub path: PathBuf,
    pub frames_written: u64,
}

struct PendingFrame {
    bgra: Vec<u8>,
    captured_at: Instant,
    sample_hash: u64,
}

#[derive(Default)]
struct DelayClock {
    accumulated_us: u128,
    emitted_cs: u128,
}

impl DelayClock {
    /// GIF stores delay in centiseconds. Quantizing cumulatively prevents 15/24 FPS
    /// from drifting over long recordings (e.g. 15 FPS naturally alternates 6/7 cs).
    fn quantize(&mut self, duration: Duration) -> u16 {
        self.accumulated_us = self.accumulated_us.saturating_add(duration.as_micros());
        let target_cs = (self.accumulated_us + 5_000) / 10_000;
        let mut delta = target_cs.saturating_sub(self.emitted_cs);
        if delta < 2 {
            // Delays under ~20 ms are inconsistently honored by GIF viewers. Our UI
            // only exposes <=24 FPS, so this floor does not alter normal recordings.
            delta = 2;
        }
        delta = delta.min(u16::MAX as u128);
        self.emitted_cs = self.emitted_cs.saturating_add(delta);
        delta as u16
    }
}

pub fn start(
    options: EncoderOptions,
) -> Result<(Sender<EncoderMessage>, Receiver<EncoderMessage>, JoinHandle<Result<EncodeSummary, String>>), String> {
    fs::create_dir_all(&options.output_dir).map_err(|e| e.to_string())?;
    let (tx, rx) = bounded::<EncoderMessage>(4);
    // A receiver clone is returned only so the capture producer can evict the
    // oldest queued frame on backpressure. It never consumes the Finish marker,
    // which is sent only after the capture producer has stopped.
    let eviction_rx = rx.clone();
    let handle = thread::Builder::new()
        .name("gifshot-gif-encoder".into())
        .spawn(move || encode_loop(rx, options))
        .map_err(|e| e.to_string())?;
    Ok((tx, eviction_rx, handle))
}

fn encode_loop(rx: Receiver<EncoderMessage>, options: EncoderOptions) -> Result<EncodeSummary, String> {
    let final_path = unique_output_path(&options.output_dir);
    let part_path = final_path.with_extension("gif.part");
    let result = (|| {
        let file = File::create(&part_path).map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        let (out_w, out_h) = scaled_size(options.width, options.height, options.scale_percent);
        let width = u16::try_from(out_w).map_err(|_| "capture width exceeds GIF limit".to_string())?;
        let height = u16::try_from(out_h).map_err(|_| "capture height exceeds GIF limit".to_string())?;

        let mut encoder = Encoder::new(&mut writer, width, height, &[]).map_err(|e| e.to_string())?;
        encoder.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;

        let mut pending: Option<PendingFrame> = None;
        let mut delay_clock = DelayClock::default();
        let mut frames_written = 0u64;
        let mut finish_at = None;

        while let Ok(message) = rx.recv() {
            match message {
                EncoderMessage::Frame(raw) => {
                    let hash = sample_hash(&raw.bgra);
                    if let Some(previous) = pending.as_mut() {
                        if previous.sample_hash == hash && previous.bgra == raw.bgra {
                            // Same pixels: keep the older timestamp so the eventual frame
                            // receives the complete unchanged duration.
                            continue;
                        }

                        let duration = raw.captured_at.saturating_duration_since(previous.captured_at);
                        write_frame(
                            &mut encoder,
                            previous,
                            delay_clock.quantize(duration),
                            options.width,
                            options.height,
                            out_w,
                            out_h,
                            options.max_colors,
                            options.quantizer_speed,
                        )?;
                        frames_written += 1;
                    }

                    pending = Some(PendingFrame {
                        bgra: raw.bgra,
                        captured_at: raw.captured_at,
                        sample_hash: hash,
                    });
                }
                EncoderMessage::Finish(at) => {
                    finish_at = Some(at);
                    break;
                }
            }
        }

        let finished_at = finish_at.unwrap_or_else(Instant::now);
        if let Some(previous) = pending.as_mut() {
            let duration = finished_at.saturating_duration_since(previous.captured_at);
            write_frame(
                &mut encoder,
                previous,
                delay_clock.quantize(duration.max(Duration::from_millis(20))),
                options.width,
                options.height,
                out_w,
                out_h,
                options.max_colors,
                options.quantizer_speed,
            )?;
            frames_written += 1;
        }

        if frames_written == 0 {
            return Err("capture ended before the first frame arrived".to_string());
        }

        drop(encoder);
        writer.flush().map_err(|e| e.to_string())?;
        writer.get_ref().sync_all().map_err(|e| e.to_string())?;
        drop(writer);

        crate::win32::atomic_replace(&part_path, &final_path)?;
        Ok(EncodeSummary { path: final_path.clone(), frames_written })
    })();

    if result.is_err() {
        let _ = fs::remove_file(&part_path);
    }
    result
}

fn write_frame<W: Write>(
    encoder: &mut Encoder<W>,
    pending: &mut PendingFrame,
    delay: u16,
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    max_colors: usize,
    quantizer_speed: i32,
) -> Result<(), String> {
    // WGC gives BGRA8. Convert to RGBA for the quantizer, optionally downscaling
    // first so lower quality tiers shrink both file size and encode cost.
    let mut rgba = if src_w == out_w && src_h == out_h {
        let mut rgba = std::mem::take(&mut pending.bgra);
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        rgba
    } else {
        downsample_bgra_to_rgba(&pending.bgra, src_w, src_h, out_w, out_h)
    };

    let width = u16::try_from(out_w).map_err(|_| "scaled width exceeds GIF limit".to_string())?;
    let height = u16::try_from(out_h).map_err(|_| "scaled height exceeds GIF limit".to_string())?;
    let mut frame = quantize_rgba_frame(width, height, &mut rgba, max_colors, quantizer_speed)?;
    frame.delay = delay;
    encoder.write_frame(&frame).map_err(|e| e.to_string())
}

fn scaled_size(width: u32, height: u32, scale_percent: u32) -> (u32, u32) {
    let scale = scale_percent.clamp(25, 100);
    if scale >= 100 {
        return (width.max(1), height.max(1));
    }
    let out_w = ((u64::from(width) * u64::from(scale)) / 100).max(16) as u32;
    let out_h = ((u64::from(height) * u64::from(scale)) / 100).max(16) as u32;
    (out_w.min(width.max(1)), out_h.min(height.max(1)))
}

fn downsample_bgra_to_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        let y0 = (u64::from(y) * u64::from(sh) / u64::from(dh)) as u32;
        let y1 = ((u64::from(y + 1) * u64::from(sh) / u64::from(dh)) as u32)
            .max(y0 + 1)
            .min(sh);
        for x in 0..dw {
            let x0 = (u64::from(x) * u64::from(sw) / u64::from(dw)) as u32;
            let x1 = ((u64::from(x + 1) * u64::from(sw) / u64::from(dw)) as u32)
                .max(x0 + 1)
                .min(sw);
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = ((yy * sw + xx) * 4) as usize;
                    // BGRA -> accumulate as RGB
                    sum[0] += u32::from(src[i + 2]);
                    sum[1] += u32::from(src[i + 1]);
                    sum[2] += u32::from(src[i]);
                    count += 1;
                }
            }
            let o = ((y * dw + x) * 4) as usize;
            out[o] = (sum[0] / count.max(1)) as u8;
            out[o + 1] = (sum[1] / count.max(1)) as u8;
            out[o + 2] = (sum[2] / count.max(1)) as u8;
            out[o + 3] = 255;
        }
    }
    out
}

fn quantize_rgba_frame(
    width: u16,
    height: u16,
    rgba: &mut [u8],
    max_colors: usize,
    quantizer_speed: i32,
) -> Result<Frame<'static>, String> {
    // Always go through NeuQuant with the tier's color budget. The gif crate's
    // exact-palette shortcut would make flat UI recordings ignore max_colors and
    // collapse the size ladder (e.g. "高" larger than "原始").
    let colors = max_colors.clamp(2, 256);
    let speed = quantizer_speed.clamp(1, 30);
    let nq = color_quant::NeuQuant::new(speed, colors, rgba);
    let indices: Vec<u8> = rgba
        .chunks_exact(4)
        .map(|pix| nq.index_of(pix) as u8)
        .collect();
    Ok(Frame::from_palette_pixels(
        width,
        height,
        indices,
        nq.color_map_rgb(),
        None,
    ))
}

fn sample_hash(bytes: &[u8]) -> u64 {
    // Fast rejection hash. Equality is still verified byte-for-byte before merging,
    // so collisions cannot corrupt the output.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes.iter().step_by(64) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ bytes.len() as u64
}

fn unique_output_path(dir: &Path) -> PathBuf {
    let stamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    for suffix in 0..10_000u32 {
        let name = if suffix == 0 {
            format!("GifShot_{stamp}.gif")
        } else {
            format!("GifShot_{stamp}_{suffix}.gif")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("GifShot_{}.gif", Local::now().timestamp_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_delay_does_not_drift_at_15fps() {
        let mut clock = DelayClock::default();
        let sum: u32 = (0..15)
            .map(|_| u32::from(clock.quantize(Duration::from_micros(66_667))))
            .sum();
        assert!((99..=101).contains(&sum));
    }

    #[test]
    fn cumulative_delay_does_not_drift_at_24fps() {
        let mut clock = DelayClock::default();
        let sum: u32 = (0..24)
            .map(|_| u32::from(clock.quantize(Duration::from_micros(41_667))))
            .sum();
        assert!((99..=101).contains(&sum));
    }

    #[test]
    fn sampled_hash_changes_for_different_pixels() {
        let a = vec![0u8; 4096];
        let mut b = a.clone();
        b[64] = 1;
        assert_ne!(sample_hash(&a), sample_hash(&b));
    }
}
