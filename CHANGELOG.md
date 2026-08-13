# Changelog

## 1.0.0

- Native Windows resident runtime with global hotkey and single-instance command routing.
- Win+Shift+S-style multi-monitor dimmer, crosshair selection, live selection cut-out and compact FPS chooser.
- Windows Graphics Capture / D3D11 region recording at 5/10/15/24 FPS.
- Non-activating red recording border and elapsed timer excluded from captured pixels.
- Streaming bounded-memory GIF encoder with duplicate-frame coalescing and cumulative centisecond timing.
- Atomic GIF finalization to Pictures\\GifShot.
- Automatic `CF_HDROP` file clipboard delivery with contention retry.
- Embedded application icon for the executable and notification-area icon.
- Tray menu: capture/stop, settings, help, quit (Chinese labels; menu opens above the tray icon).
- Interactive `gifshot settings` (press a chord to change hotkeys; open capture folder) and `gifshot help`.
- Live config reload (`gifshot reload`) so a running resident process rebinds hotkeys after settings save.
- Persistent validated JSON configuration with corrupt-file recovery.
- Preferred/fallback hotkeys, GIF quality profile, cursor capture, duration limit and output directory.
- npm CLI for capture/start/stop/quit/settings/help/open/config/autostart/doctor.
- Windows CI; tagging `vX.Y.Z` builds the native binary, attaches the npm tarball to the GitHub Release, and publishes `gifshot-win` to npm.
