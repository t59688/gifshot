# GifShot 1.0 Release Test Plan

A V1 release is not accepted from compilation alone. Run this matrix on real Windows desktops before cutting a `v*` tag (which auto-publishes npm).

## Automated gates

On `windows-latest` CI:

- Rust formatting check
- Rust Clippy
- Rust unit tests
- optimized native build
- Node launcher syntax check (`bin/gifshot.js` and `scripts/*.js`)
- source audit
- staged PE (`MZ`) package verification
- `npm pack --dry-run`

Unit tests cover at minimum hotkey parsing, configuration normalization, GIF cumulative timing, and duplicate-frame hash discrimination.

## Manual OS matrix

| Environment | Required |
|---|---|
| Windows 10 22H2 x64 | yes |
| Windows 11 current stable x64 | yes |
| 100% DPI single monitor | yes |
| 125% / 150% DPI | yes |
| 200% DPI | yes |
| two monitors, same DPI | yes |
| two monitors, mixed DPI | yes |
| monitor left/up of primary (negative virtual coordinates) | yes |
| landscape + portrait monitor | yes |

## Capture interaction

For every required desktop geometry:

1. hotkey makes all monitors dim immediately;
2. crosshair is visible and correct;
3. drag in every direction normalizes correctly;
4. drag cannot escape the monitor where it started;
5. selection stays bright while surrounding desktop stays dim;
6. FPS popup appears fully onscreen near the selection;
7. 5/10/15/24 FPS each start recording;
8. FPS preference persists to next capture;
9. Escape cancels with no output file;
10. right-click cancels;
11. pressing the global hotkey while selecting cancels;
12. clicking selected content while FPS popup is open does not click the underlying app.

## Recording HUD

- red border is visible and exactly outside capture pixels;
- timer increments while recording;
- HUD never steals active-window focus;
- underlying controls remain clickable through the HUD;
- generated GIF contains neither red border nor timer;
- full-monitor selection never leaks the timer into the GIF: show it only when capture exclusion succeeds; otherwise hide it;

## Output correctness

Exercise captures at 5/10/15/24 FPS for 2 s, 10 s, 60 s and configured maximum duration.

Validate:

- GIF decodes successfully;
- animation loops;
- pixel dimensions exactly match selection;
- playback duration stays within expected GIF timing tolerance;
- 15 and 24 FPS recordings do not accumulate visible long-run timing drift;
- 24 FPS pacing averages correctly when the source monitor runs at 60/120/144 Hz;
- static-screen periods coalesce instead of producing needless identical frames;
- rapidly changing content remains close to current screen state under encoder pressure;
- no `.gif.part` remains after success;
- interrupted/failing encode does not leave a final file masquerading as valid.

## Clipboard/application matrix

Paste immediately after recording into applications representative of file-paste behavior:

- File Explorer
- Slack desktop
- Discord desktop
- Teams desktop
- browser upload field where paste is supported
- a bitmap-only editor (expected: may reject file paste)

The contractual behavior is that `CF_HDROP` contains the saved `.gif` path. An application that does not consume file clipboard formats is not a GifShot data-loss condition.

## Reliability / recovery

- start GifShot twice -> one resident process;
- `gifshot stop` while recording -> valid finalize;
- `gifshot quit` while recording -> finalize then exit;
- `gifshot quit` idle -> immediate exit;
- preferred hotkey already registered -> fallback hotkey active + notification;
- both hotkeys unavailable -> visible startup error;
- clipboard held open by another process -> retries, valid saved GIF, warning;
- malformed config -> corrupt copy preserved and defaults restored;
- capture directory absent -> created;
- capture directory unwritable -> visible error, no phantom success notification;
- WGC session terminates unexpectedly -> finalize available frames when possible and warn;
- max duration -> automatic finalize;
- Explorer restart -> tray icon is automatically restored through the registered `TaskbarCreated` message;
- tray right-click menu appears above the icon (not below the taskbar) with 录制 GIF / 设置 / 帮助 / 退出;
- tray **设置** and **帮助** open a usable console (does not flash-close; stdin is a TTY);
- `gifshot settings` item 1/2 captures a chord (Esc cancels); item 3 opens Pictures\\GifShot in Explorer (window is visible);
- after saving a hotkey in settings, the running resident process rebinds without quit/start;
- `gifshot help` prints the usage guide;
- repeat capture -> Escape cancel -> capture at least 100 times -> no crash, stuck overlay, or leaked topmost window;
- repeatedly issue `gifshot quit` while selector/FPS chooser is visible -> clean exit with no access violation;
- hammer capture/stop/quit commands from separate terminals -> exactly one resident instance and no double finalization;

## Content/OS constraints

Confirm safe behavior (not bypass) for:

- DRM/protected video;
- UAC secure desktop;
- lock screen;
- Remote Desktop session;
- HDR monitor;
- hardware acceleration on/off where available.

Blank/blocked protected content is acceptable and should never trigger privilege escalation or capture-bypass techniques.

## Performance acceptance

Measure on representative Windows 11 hardware at 1920x1080 and 4K:

- idle resident memory and CPU;
- hotkey-to-selector latency;
- 800x600 and 1920x1080 region at 15/24 FPS;
- encoder frame-drop count;
- stop-to-GIF-ready latency;
- resulting GIF size for a typical UI demo.

No fixed universal hardware number is encoded into the product; regression baselines should be captured per release and must not materially regress without an explicit decision.
