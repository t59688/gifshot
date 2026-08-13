# GifShot 1.0 Delivery Status

This source tree is the V1.0 implementation of GifShot's defined product scope:

`hotkey -> region selection -> FPS choice -> recording HUD -> stop -> GIF -> file clipboard + capture folder`

## Implemented

- npm-installed Windows CLI / resident launcher (`gifshot-win` on npm)
- single-instance resident process with command rendezvous
- preferred + fallback global hotkey registration
- per-monitor-v2-DPI-aware multi-monitor region selector
- dimmed desktop, crosshair selection, custom FPS popup (5/10/15/24 by default)
- Windows.Graphics.Capture monitor capture with selected-region crop
- capability-based cursor and capture-border settings for Windows-version compatibility
- phase-locked 5/10/15/24 FPS sampling across common display refresh rates
- bounded producer/encoder pipeline with fresh-frame backpressure policy
- streaming GIF encoding with duplicate-frame coalescing and cumulative centisecond timing
- atomic `.gif.part` -> `.gif` publication
- recording border / timer HUD designed not to contaminate captured pixels
- maximum-duration stop
- `CF_HDROP` clipboard delivery with retry handling
- embedded application icon; tray menu (capture, settings, help, quit)
- interactive terminal settings (chord capture) and help; live hotkey reload
- capture folder, notifications, config, logs, autostart and CLI control
- corrupt-config recovery and atomic config writes
- named-mutex single-instance protection
- Explorer taskbar/tray restoration path
- Windows CI; tag `v*` builds, attaches the npm tarball to the GitHub Release, and publishes to npm

## Release gates

Automated on `windows-latest` (`.github/workflows/ci.yml` and `.github/workflows/release.yml`):

- `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings`, locked release compilation
- Node launcher syntax, source audit, staged PE (`MZ`) verification
- `npm pack` of a package that contains `vendor/win32-x64/gifshot.exe`

Manual desktop matrix remains in `docs/TEST_PLAN.md`. Capture, DPI, multi-monitor, clipboard, and tray behavior are not proven by compilation alone.

## Release prerequisite

Keep `native/Cargo.lock` generated on the pinned Rust 1.97.1 Windows toolchain, reviewed, and committed. The release workflow rejects an unlocked release build by design. `NPM_TOKEN` must exist as a repository secret before a tag can publish.
