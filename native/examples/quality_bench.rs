//! Controlled quality/size comparison for GifQuality profiles.
//!
//! Run: cargo run --example quality_bench --release --manifest-path native/Cargo.toml

use color_quant::NeuQuant;
use gif::{ColorOutput, DecodeOptions, Encoder, Frame, Repeat};
use std::{io::Cursor, time::Instant};

#[derive(Clone, Copy)]
struct Quality {
    label: &'static str,
    scale_percent: u32,
    max_colors: usize,
    speed: i32,
}

// Keep in sync with `GifQuality` in src/types.rs.
const QUALITIES: [Quality; 4] = [
    Quality {
        label: "低",
        scale_percent: 50,
        max_colors: 64,
        speed: 20,
    },
    Quality {
        label: "中",
        scale_percent: 70,
        max_colors: 128,
        speed: 10,
    },
    Quality {
        label: "高",
        scale_percent: 85,
        max_colors: 192,
        speed: 5,
    },
    Quality {
        label: "原始",
        scale_percent: 100,
        max_colors: 256,
        speed: 1,
    },
];

const WIDTH: u16 = 640;
const HEIGHT: u16 = 360;
const FRAMES: usize = 45;
const DELAY_CS: u16 = 7;

#[derive(Clone, Copy)]
enum Scene {
    Mixed,
    FlatUi,
}

fn main() {
    for scene in [Scene::Mixed, Scene::FlatUi] {
        run_scene(scene);
        println!();
    }
    println!("profiles: 低=50%/64c  中=70%/128c  高=85%/192c  原始=100%/256c");
}

fn run_scene(scene: Scene) {
    let source = synthesize_frames(WIDTH, HEIGHT, FRAMES, scene);
    let title = match scene {
        Scene::Mixed => "Mixed (gradient + UI card + fine detail + noise)",
        Scene::FlatUi => "Flat UI (solid panels / buttons / text bars)",
    };
    println!("GifShot quality bench — {title}");
    println!("canvas: {WIDTH}x{HEIGHT}, frames: {FRAMES}, delay: {DELAY_CS}cs (~15 FPS)");
    println!();

    let encoded: Vec<_> = QUALITIES
        .iter()
        .map(|quality| {
            let started = Instant::now();
            let gif = encode_gif(&source, WIDTH, HEIGHT, *quality);
            let encode_ms = started.elapsed().as_millis();
            let (psnr, rmse) = compare_psnr(&source, &gif);
            (quality, gif.len(), encode_ms, psnr, rmse)
        })
        .collect();

    let medium_size = encoded
        .iter()
        .find(|(q, ..)| q.label == "中")
        .map(|(_, size, ..)| *size)
        .unwrap_or(1);

    println!(
        "{:<8} {:>8} {:>8} {:>10} {:>10} {:>10} {:>10}",
        "quality", "out_wh", "size_kb", "vs_med", "encode_ms", "psnr_db", "rmse"
    );
    for (quality, size, encode_ms, psnr, rmse) in encoded {
        let (ow, oh) = scaled_size(WIDTH as u32, HEIGHT as u32, quality.scale_percent);
        let vs_med = (size as f64 / medium_size as f64 - 1.0) * 100.0;
        println!(
            "{:<8} {:>4}x{:<3} {:>10.1} {:>+9.1}% {:>10} {:>10.2} {:>10.2}",
            quality.label,
            ow,
            oh,
            size as f64 / 1024.0,
            vs_med,
            encode_ms,
            psnr,
            rmse
        );
    }
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

fn encode_gif(frames: &[Vec<u8>], width: u16, height: u16, quality: Quality) -> Vec<u8> {
    let (out_w, out_h) = scaled_size(u32::from(width), u32::from(height), quality.scale_percent);
    let mut out = Vec::new();
    {
        let mut encoder =
            Encoder::new(&mut out, out_w as u16, out_h as u16, &[]).expect("encoder");
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        for src in frames {
            let mut rgba = if out_w == u32::from(width) && out_h == u32::from(height) {
                src.clone()
            } else {
                downsample_rgba(src, u32::from(width), u32::from(height), out_w, out_h)
            };
            let mut frame = quantize(&mut rgba, out_w as u16, out_h as u16, quality);
            frame.delay = DELAY_CS;
            encoder.write_frame(&frame).expect("write frame");
        }
    }
    out
}

fn quantize(rgba: &mut [u8], width: u16, height: u16, quality: Quality) -> Frame<'static> {
    let colors = quality.max_colors.clamp(2, 256);
    let speed = quality.speed.clamp(1, 30);
    let nq = NeuQuant::new(speed, colors, rgba);
    let indices: Vec<u8> = rgba
        .chunks_exact(4)
        .map(|pix| nq.index_of(pix) as u8)
        .collect();
    Frame::from_palette_pixels(width, height, indices, nq.color_map_rgb(), None)
}

fn downsample_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
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
                    sum[0] += u32::from(src[i]);
                    sum[1] += u32::from(src[i + 1]);
                    sum[2] += u32::from(src[i + 2]);
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

fn compare_psnr(source: &[Vec<u8>], gif_bytes: &[u8]) -> (f64, f64) {
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    let mut decoder = options.read_info(Cursor::new(gif_bytes)).expect("decode gif");
    let out_w = u32::from(decoder.width());
    let out_h = u32::from(decoder.height());

    let mut sse = 0f64;
    let mut pixels = 0f64;
    let mut index = 0usize;

    while let Some(frame) = decoder.read_next_frame().expect("next frame") {
        let scaled = if out_w == u32::from(WIDTH) && out_h == u32::from(HEIGHT) {
            source[index].clone()
        } else {
            downsample_rgba(
                &source[index],
                u32::from(WIDTH),
                u32::from(HEIGHT),
                out_w,
                out_h,
            )
        };
        let decoded = frame.buffer.as_ref();
        assert_eq!(decoded.len(), scaled.len());
        for (s, d) in scaled.chunks_exact(4).zip(decoded.chunks_exact(4)) {
            for c in 0..3 {
                let diff = s[c] as f64 - d[c] as f64;
                sse += diff * diff;
            }
            pixels += 3.0;
        }
        index += 1;
    }
    assert_eq!(index, source.len());

    let mse = sse / pixels;
    let rmse = mse.sqrt();
    let psnr = if mse <= 1e-12 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    };
    (psnr, rmse)
}

fn synthesize_frames(width: u16, height: u16, count: usize, scene: Scene) -> Vec<Vec<u8>> {
    let w = width as i32;
    let h = height as i32;
    (0..count)
        .map(|frame| {
            let t = frame as i32;
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    let (rr, gg, bb) = match scene {
                        Scene::Mixed => mixed_pixel(x, y, w, h, t),
                        Scene::FlatUi => flat_ui_pixel(x, y, w, h, t),
                    };
                    rgba[i] = rr;
                    rgba[i + 1] = gg;
                    rgba[i + 2] = bb;
                    rgba[i + 3] = 255;
                }
            }
            rgba
        })
        .collect()
}

fn mixed_pixel(x: i32, y: i32, w: i32, h: i32, t: i32) -> (u8, u8, u8) {
    let r = (40 + x * 140 / w + (y / 8) % 17) as u8;
    let g = (55 + y * 120 / h) as u8;
    let b = (90 + ((x + y + t * 3) % 64)) as u8;

    let card_x = 80 + (t * 5) % 180;
    let card_y = 60 + ((t * 3) % 90);
    let in_card = x >= card_x && x < card_x + 160 && y >= card_y && y < card_y + 96;

    let fine = ((x / 2) ^ (y / 2) ^ (t / 2)) & 1 == 0 && x > w / 2 && y > h / 3 && y < h * 2 / 3;

    let noise_zone = x < 120 && y > h - 120;
    let noise = if noise_zone {
        ((x * 37 + y * 91 + t * 13) % 51) - 25
    } else {
        0
    };

    let (mut rr, mut gg, mut bb) = if in_card {
        (236, 72, 72)
    } else if fine {
        (245, 245, 247)
    } else {
        (r, g, b)
    };
    if noise_zone {
        rr = (rr as i32 + noise).clamp(0, 255) as u8;
        gg = (gg as i32 + noise / 2).clamp(0, 255) as u8;
        bb = (bb as i32 - noise / 3).clamp(0, 255) as u8;
    }
    (rr, gg, bb)
}

fn flat_ui_pixel(x: i32, y: i32, w: i32, h: i32, t: i32) -> (u8, u8, u8) {
    if y < 40 {
        return (37, 37, 38);
    }
    if x < 56 {
        return (45, 45, 48);
    }
    if y > h - 28 {
        return (30, 30, 30);
    }

    let panel_x = 80 + (t % 40);
    if x >= panel_x && x < panel_x + 220 && (70..220).contains(&y) {
        if y < 102 {
            return (0, 122, 204);
        }
        if ((y - 110) / 18) % 2 == 0 && x < panel_x + 180 {
            return (212, 212, 212);
        }
        return (60, 60, 64);
    }

    let caret = 300 + ((t * 7) % (w - 360)).max(0);
    if x >= caret && x < caret + 2 && (90..250).contains(&y) {
        return (255, 255, 255);
    }

    (30, 30, 30)
}
