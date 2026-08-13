# GifShot

**Win+Shift+S, but for GIFs.**

GifShot is a small Windows utility for recording a selected screen region directly to an animated GIF. The npm package is only the install/update and command-line layer; the resident runtime is a native Rust executable with no Electron, browser engine, FFmpeg, or GUI framework.

## User experience

1. Press **Win+Shift+G**.
2. The virtual desktop dims and the pointer becomes a crosshair.
3. Drag a region on any monitor.
4. Choose **5 / 10 / 15 / 24 FPS** from the compact popup beside the selection.
5. Recording starts immediately. The red border stays outside the captured pixels; the `REC mm:ss` timer is placed outside the selection when possible, otherwise capture-excluded or hidden so it is never intentionally burned into the GIF.
6. Press **Win+Shift+G** again to stop.
7. GifShot atomically writes the GIF to **Pictures\\GifShot** and places the GIF file on the Windows clipboard.

If `Win+Shift+G` is already registered, GifShot automatically falls back to `Ctrl+Shift+G` and notifies you.

## Requirements

- Windows 10 version 2004 or later, or Windows 11
- x64 CPU for GifShot 1.0
- Node.js 18+ for npm installation/CLI

## Install

Once published under this package name:

```powershell
npm install -g gifshot-win
gifshot start
```

Run at sign-in:

```powershell
gifshot autostart on
```

The resident process has no main window. Use the global hotkey or the notification-area icon.

## Commands

```text
gifshot                 Trigger capture / start resident runtime
gifshot start           Start resident runtime only
gifshot stop            Stop current recording
gifshot quit            Quit GifShot
gifshot settings        Interactive settings (hotkeys, folders)
gifshot help            Usage guide
gifshot open            Open capture folder
gifshot config          Open config.json
gifshot autostart on    Enable sign-in startup
gifshot autostart off   Disable sign-in startup
gifshot autostart status
gifshot doctor          Installation diagnostics
gifshot --version
```

## Configuration

The first run creates `config.json` in the standard Windows per-user config directory. Advanced settings intentionally live there so the capture flow stays fast.

```json
{
  "schema_version": 1,
  "hotkey": "Win+Shift+G",
  "fallback_hotkey": "Ctrl+Shift+G",
  "default_fps": 15,
  "fps_options": [5, 10, 15, 24],
  "capture_cursor": true,
  "max_duration_secs": 120,
  "dim_opacity": 128,
  "gif_quantizer_speed": 10,
  "copy_to_clipboard": true,
  "show_notifications": true,
  "output_dir": null
}
```

GifShot validates and normalizes configuration on startup. A malformed file is preserved as `config.corrupt-<timestamp>.json` before defaults are restored. Manual edits take effect the next time the resident process starts (`gifshot quit`, then `gifshot start`).
If `output_dir` is relative, it is resolved relative to the directory containing `config.json`, not the process working directory.

## Build from source

Prerequisites: Rust 1.97.1 MSVC toolchain (pinned by `rust-toolchain.toml`), Visual Studio Build Tools with the Windows SDK, and Node.js 18+.

```powershell
npm install
npm run build:native
npm run verify
```

`build:native` creates the optimized native executable and stages it under `vendor/win32-x64/gifshot.exe` for npm packaging. The source archive itself intentionally does not contain a prebuilt executable; the Windows CI/release job produces and verifies that artifact.

For native-only development:

```powershell
cargo fmt --manifest-path native/Cargo.toml -- --check
cargo clippy --manifest-path native/Cargo.toml --all-targets
cargo test --manifest-path native/Cargo.toml
cargo build --manifest-path native/Cargo.toml --release
```

## V1 behavior and deliberate boundaries

- Selection can start on **any** attached monitor, including mixed-DPI layouts.
- A single capture selection is constrained to the monitor where the drag starts. This makes physical coordinates, GPU capture surfaces, cursor position, and GIF pixels deterministic and avoids hidden cross-adapter composition costs.
- Protected/DRM content and secure desktops may be blank or unavailable by Windows design.
- Windows may show its own monitor-level capture indicator on builds/policies where borderless WGC capture is unavailable; GifShot never depends on suppressing that indicator for correctness.
- Clipboard delivery uses a real file (`CF_HDROP`). Applications that accept file paste receive the animated GIF; applications that only request bitmap clipboard formats may not accept it.
- GifShot records pixels only. It has no audio capture, upload, analytics, account, cloud client, or built-in network service. A custom output folder may of course point at storage that Windows or another app synchronizes.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/TEST_PLAN.md](docs/TEST_PLAN.md), and [docs/DELIVERY_STATUS.md](docs/DELIVERY_STATUS.md) for implementation, validation status, and release criteria.

## Uninstall

If you enabled sign-in startup, disable it before uninstalling so Windows does not retain a stale Run entry:

```powershell
gifshot autostart off
gifshot quit
npm uninstall -g gifshot-win
```
