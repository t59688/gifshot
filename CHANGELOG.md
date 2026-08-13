# Changelog

## 1.0.0

- Native Windows resident runtime with global hotkey and single-instance command routing.
- Win+Shift+S-style multi-monitor dimmer, crosshair selection, live selection cut-out and compact FPS chooser.
- Windows Graphics Capture / D3D11 region recording at 5/10/15/24 FPS.
- Non-activating red recording border and elapsed timer excluded from captured pixels.
- Streaming bounded-memory GIF encoder with duplicate-frame coalescing and cumulative centisecond timing.
- Atomic GIF finalization to Pictures\\GifShot.
- Automatic `CF_HDROP` file clipboard delivery with contention retry.
- Notification-area menu, success/warning/error notifications and local diagnostics.
- Persistent validated JSON configuration with corrupt-file recovery.
- Preferred/fallback hotkeys, configurable cursor capture, duration limit and output directory.
- npm CLI for capture/start/stop/quit/open/config/autostart/doctor.
- Windows CI, release packaging checks and documented manual release matrix.
