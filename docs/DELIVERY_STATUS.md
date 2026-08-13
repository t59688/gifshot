# GifShot 1.0 Delivery Status

This source tree is the V1.0 implementation of GifShot's defined product scope:

`hotkey -> region selection -> FPS choice -> recording HUD -> stop -> GIF -> file clipboard + capture folder`

## Implemented

- npm-installed Windows CLI / resident launcher
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
- capture folder, tray menu, notifications, config, logs, autostart and CLI control
- corrupt-config recovery and atomic config writes
- named-mutex single-instance protection
- Explorer taskbar/tray restoration path
- Windows CI / release workflows, source audit, package verification and manual test plan

## Validation completed in this delivery environment

The delivery environment is Linux and does not provide the Windows SDK/MSVC toolchain or Rust toolchain, so it cannot execute Windows.Graphics.Capture or produce a trustworthy Windows PE binary.

Completed here:

- JavaScript syntax validation for the CLI and release/build scripts
- source-structure / required-implementation audit (`npm test`)
- review of critical lifecycle, capture, encoder, Win32 reentrancy, clipboard and failure paths
- source archive creation and checksum

Not falsely claimed as completed here:

- `cargo fmt`, `cargo test`, `cargo clippy` and Windows MSVC release compilation
- real Windows 10/11 capture, DPI, multi-monitor and clipboard smoke tests
- npm package dry-run containing a real `gifshot.exe`

Those checks are intentionally release gates in `.github/workflows/ci.yml`, `.github/workflows/release.yml` and `docs/TEST_PLAN.md`. A public npm release should not be published until they pass on Windows.

## Release prerequisite

Generate and review `native/Cargo.lock` on the pinned Rust 1.97.1 Windows build environment, commit it, then create the public release tag. The release workflow rejects an unlocked release build by design.
