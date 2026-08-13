# GifShot 1.0 Architecture

## Product invariant

GifShot exists to make one action fast:

`hotkey -> select -> choose FPS -> record -> hotkey -> paste GIF`

Anything that does not improve that path is excluded from the native runtime. There is no editor, webview, project database, cloud service, media server, or GUI settings window. Hotkeys and a few recovery actions live in a terminal menu (`gifshot settings`) and the tray.

## Stack

| Layer | Technology | Responsibility |
|---|---|---|
| Distribution | npm + Node.js launcher | install/update, CLI commands, autostart, interactive settings/help |
| Native runtime | Rust 2024 | single resident process and lifecycle |
| Capture | Windows Graphics Capture + D3D11 through `windows-capture` | hardware-accelerated monitor frames and selected-region GPU readback |
| UI | raw Win32/GDI through `windows-sys` | dimmer, crosshair, selection outline, FPS popup, recording HUD, tray |
| GIF | `gif` crate | 256-color quantization, timing, streaming LZW output |
| Concurrency | `crossbeam-channel` + OS/capture threads | bounded frame handoff and background encoding |
| Persistence | JSON + atomic replace | user config |
| Clipboard | Win32 `CF_HDROP` | copy the generated GIF as a real file |
| Diagnostics | `tracing` | local rolling logs; no captured pixels |

## Process model

There is exactly one resident `gifshot.exe` per interactive user session. A session-local named Win32 mutex is acquired before runtime initialization, closing the startup race where two npm invocations arrive at the same time. The hidden main Win32 window is the command rendezvous point and UI message dispatcher.

Subsequent CLI invocations observe the mutex, wait briefly for the control window if the first process is still starting, then post private `WM_APP` messages instead of starting additional recorders.

```text
npm CLI
  |
  +---- settings / help ---- Node interactive TTY (no resident process required)
  |
  +---- gifshot.exe (existing?) ---- yes --> PostMessage(WM_APP + n) --> exit
  |                                  no
  +---------------------------------------> create resident process
                                                |
                                                +-- Win32 UI thread
                                                +-- WGC capture thread (only while recording)
                                                +-- GIF encoder thread (only while recording/finalizing)
```

`settings` and `help` stay in the Node launcher so they can own a real console. The tray opens those same commands via a temporary `.cmd` + `ShellExecute` (GUI-subsystem `gifshot.exe` cannot give Node a TTY). Changing a hotkey writes `config.json` and posts `WM_GIFSHOT_RELOAD_CONFIG` (`gifshot reload`) so the resident process rebinds the hotkey without a restart.

## State machine

```text
          hotkey
   +------------------+
   |                  v
 [Idle] ----------> [Selecting]
   ^                   |  FPS click
   | Escape/cancel     v
   +--------------- [Recording]
                         | hotkey / max duration / capture end
                         v
                     [Encoding]
                         |
                         v
                       [Idle]
```

Only the UI thread mutates the runtime state. Worker threads report completion back through posted Win32 messages, so no UI-owned HWND is touched from capture/encoding threads.

## Selection UI

`selection.rs` deliberately does not use a widget toolkit.

1. A topmost layered window covers the entire Windows virtual desktop with a black alpha dimmer.
2. It captures the mouse while dragging, so a region cut-out cannot leak pointer input to the underlying application.
3. Dragging is constrained to the monitor where selection starts.
4. The selected area is removed from the dimmer window region, exposing the live desktop.
5. A separate 1 px white topmost border marks the selection.
6. After mouse-up, a nearly transparent blocker covers the selected pixels so clicks cannot pass through.
7. The FPS picker is one custom-painted popup; its apparent buttons are rectangles plus hit-testing, not child controls.

This gives Snipping-Tool-like latency without loading XAML, WebView2, DirectComposition frameworks, or a browser engine.

## Recording HUD

The recording border is positioned *outside* the capture rectangle. The timer is also placed outside whenever space exists. Therefore the normal path excludes recording chrome geometrically, before any capture API exclusion mechanism is needed.

As a second defense, the HUD windows request `WDA_EXCLUDEFROMCAPTURE`. For full-height edge cases where the timer must overlap the selected rectangle, that affinity keeps the timer out of supported Windows capture pipelines.

The HUD uses `WS_EX_NOACTIVATE | WS_EX_TRANSPARENT`, so it does not steal focus or consume clicks intended for the recorded application.

## Capture pipeline

A capture session targets exactly one `HMONITOR`. WGC produces BGRA8 D3D11 frames. Only the requested crop is copied back to CPU memory. Cursor and WGC-border preferences are capability-gated at runtime so older supported Windows builds fall back to system defaults instead of failing capture initialization. Frame pacing follows an absolute target timeline rather than measuring from the previous accepted callback, so targets such as 24 FPS remain correct on 60 Hz sources instead of collapsing to every-third-refresh (~20 FPS).

```text
WGC/D3D11 frame
      |
      +-- FPS pacing
      |
      +-- GPU crop to selected rectangle
      |
      +-- tightly packed BGRA CPU buffer
      |
      v
bounded queue (4 frames)
      |
      v
GIF encoder thread
```

### Backpressure

GIF color quantization can briefly be slower than frame delivery. The queue is intentionally bounded to four frames. On saturation, the capture producer evicts an older queued frame and keeps the freshest state when possible. This has three production properties:

- recording memory use remains bounded by region size instead of recording duration;
- the GIF does not accumulate seconds of visual latency;
- drop count is retained for diagnostics.

No captured pixels are ever written to logs.

## GIF timing and static-frame coalescing

GIF stores frame delays in 10 ms units. Rates such as 15 FPS and 24 FPS do not map to an integer number of centiseconds. `DelayClock` quantizes **cumulative elapsed time** instead of independently rounding every frame, preventing long-recording drift.

The encoder retains one pending frame. If a newly sampled frame is byte-identical, it is not encoded again; its eventual delay simply extends. This dramatically reduces output for static UI without changing playback semantics.

Frames are encoded while recording; there is no full-recording frame vector in RAM. Output is first written to `*.gif.part`, flushed and synchronized, and then renamed to the final `.gif`. Failed encodes remove the partial file.

## Clipboard and save semantics

The GIF is saved before clipboard mutation. Clipboard copy is therefore non-fatal: if another application keeps the clipboard busy, the user still owns a valid file and receives a warning.

`clipboard.rs` creates a standard wide-character `DROPFILES` payload and publishes it under `CF_HDROP`. Ownership of the global allocation is transferred to Windows only after `SetClipboardData` succeeds.

## DPI and multi-monitor coordinates

The process requests Per-Monitor DPI Awareness V2 before creating windows. Selection and monitor geometry therefore operate in physical desktop coordinates, matching captured pixels. The FPS picker, selection affordances, recording border, and timer derive their visual metrics from the owning monitor DPI, so 125%/150%/200% displays remain usable without introducing coordinate virtualization.

A selection cannot span two monitors in V1. This is an explicit invariant, not an unfinished edge case: a monitor is the capture surface and the selected rectangle is its crop. Cross-monitor recording could require cross-adapter composition, differing scale transforms, and synchronized surfaces; all of that is unnecessary for the product's fast-region-capture job.

## Win32 lifetime and re-entrancy safety

Win32 window destruction is synchronous: `DestroyWindow` can re-enter a window procedure with `WM_DESTROY` and `WM_NCDESTROY` before the caller returns. Rust state owned by an HWND therefore follows an explicit ownership rule: the pointer in `GWLP_USERDATA` is cleared before destruction whenever a callback could otherwise re-enter the same object, and final `Box` ownership is recovered exactly once. The resident main window routes shutdown through a private message so normal handlers finish their mutable borrow before destruction starts; selection child windows clear their controller pointer before teardown.

This is a correctness invariant, not an optimization. It prevents use-after-free, double-free, and re-entrant mutable aliasing during rapid cancel/quit/error paths.

## Failure handling

- preferred hotkey unavailable -> register configured fallback and notify;
- both hotkeys unavailable -> fail startup visibly and log;
- malformed config -> preserve corrupt copy, normalize defaults, atomically rewrite;
- selector/HUD creation error -> return to Idle and notify;
- encoder queue saturation -> bounded latest-state policy; log dropped-frame count at completion;
- capture pipeline ends -> finalize available frames into a partial-but-valid GIF when possible and surface capture issue as warning;
- encoder failure -> remove `.part`, report fatal error;
- clipboard contention -> retry, preserve GIF, warn;
- quit while recording -> stop/finalize first, then quit;
- repeated CLI invocations -> route to the single resident instance;
- `gifshot reload` with no resident process -> no-op;
- settings hotkey capture cancelled (Esc) -> leave config unchanged.

## Privacy and security

GifShot has no built-in network client. The runtime makes no upload, telemetry, account, analytics, or update-service requests. A user can still deliberately configure `output_dir` to a path managed by Windows or third-party sync software, including a network-backed path. Logs contain state transitions, durations, frame/drop counts, paths and errors, but never frame buffers. Rolling local logs are pruned after 14 days.

Protected video, secure desktop/UAC screens, and some privileged surfaces are OS-governed and are intentionally not bypassed.

## Module ownership

- `app.rs` — lifecycle, single instance, state machine, message routing
- `selection.rs` — dimmer/crosshair/region/FPS interaction
- `hud.rs` — red capture border and timer
- `capture.rs` — WGC session, frame pacing, crop/readback, backpressure
- `encoder.rs` — streaming GIF encoding, timing, atomic finalization
- `clipboard.rs` — `CF_HDROP`
- `tray.rs` — notification-area icon, Chinese recovery menu, notifications
- `hotkey.rs` — user-readable hotkey parsing
- `hotkey_capture.rs` — low-level keyboard hook for `gifshot settings` chord capture
- `config.rs` — schema/normalization/recovery/atomic persistence
- `paths.rs` — standard per-user Windows directories
- `win32.rs` — concentrated unsafe Win32 helpers, including tray CLI launch
- `types.rs` — shared value types
- `logging.rs` — local diagnostics
- `bin/gifshot.js` — npm CLI dispatcher
- `bin/interactive.js` — settings menu and help text
- `native/assets/gifshot.ico` — embedded exe / tray icon (from `gifshot.svg`)
