'use strict';
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const files = [
  'native/Cargo.toml',
  'native/build.rs',
  'native/assets/gifshot.ico',
  'native/assets/gifshot.svg',
  'rust-toolchain.toml',
  'native/src/main.rs',
  'native/src/app.rs',
  'native/src/capture.rs',
  'native/src/encoder.rs',
  'native/src/selection.rs',
  'native/src/hud.rs',
  'native/src/clipboard.rs',
  'native/src/tray.rs',
  'native/src/config.rs',
  'native/src/hotkey_capture.rs',
  'bin/gifshot.js',
  'bin/interactive.js',
  'docs/ARCHITECTURE.md',
  'docs/TEST_PLAN.md',
  'docs/RELEASE.md',
];

for (const relative of files) {
  if (!fs.existsSync(path.join(root, relative))) {
    throw new Error(`Missing source artifact: ${relative}`);
  }
}

const checks = [
  ['native/src/app.rs', 'WM_GIFSHOT_TOGGLE'],
  ['native/src/app.rs', 'RuntimeState::Recording'],
  ['native/src/app.rs', 'CreateMutexW'],
  ['native/src/app.rs', 'TaskbarCreated'],
  ['native/src/app.rs', 'WM_GIFSHOT_SHUTDOWN'],
  ['native/src/app.rs', 'WM_GIFSHOT_RELOAD_CONFIG'],
  ['native/src/win32.rs', 'open_interactive_cli'],
  ['bin/interactive.js', 'runSettings'],
  ['bin/interactive.js', 'capture-hotkey'],
  ['native/src/hotkey_capture.rs', 'WH_KEYBOARD_LL'],
  ['bin/gifshot.js', 'settings'],
  ['native/src/capture.rs', 'start_free_threaded'],
  ['native/src/capture.rs', 'buffer_crop'],
  ['native/src/capture.rs', 'is_border_settings_supported'],
  ['native/src/capture.rs', 'FramePacer'],
  ['native/src/capture.rs', 'accepting_frames'],
  ['native/src/encoder.rs', 'bounded::<EncoderMessage>(4)'],
  ['native/src/selection.rs', 'FPS'],
  ['native/src/selection.rs', 'GetDpiForWindow'],
  ['native/src/selection.rs', 'destroy_aux_windows'],
  ['native/src/tray.rs', 'Shell_NotifyIconW'],
  ['native/src/tray.rs', 'IDI_GIFSHOT'],
  ['native/src/hud.rs', 'WDA_EXCLUDEFROMCAPTURE'],
  ['native/src/hud.rs', 'GetDpiForWindow'],
  ['native/src/hud.rs', 'timer_inside_capture'],
  ['native/src/hud.rs', 'SetTimer(timer_hwnd'],
  ['native/src/clipboard.rs', 'CF_HDROP'],
  ['native/src/config.rs', 'config.corrupt-'],
  ['docs/RELEASE.md', 'npm publish'],
  ['docs/RELEASE.md', 'NPM_TOKEN'],
];
for (const [file, needle] of checks) {
  const text = fs.readFileSync(path.join(root, file), 'utf8');
  if (!text.includes(needle)) throw new Error(`${file} is missing expected implementation: ${needle}`);
}

const cargoManifest = fs.readFileSync(path.join(root, 'native/Cargo.toml'), 'utf8');
if (!cargoManifest.includes('\"Win32_Security\"')) throw new Error('native/Cargo.toml must enable Win32_Security for CreateMutexW');
if (!cargoManifest.includes('windows-capture = \"2.0.1\"')) throw new Error('native/Cargo.toml must pin windows-capture 2.0.1');
const toolchain = fs.readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8');
if (!toolchain.includes('channel = \"1.97.1\"')) throw new Error('rust-toolchain.toml must pin Rust 1.97.1');

const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const cargoToml = cargoManifest;
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (cargoVersion !== packageJson.version) {
  throw new Error(`Version mismatch: package.json=${packageJson.version}, Cargo.toml=${cargoVersion || 'missing'}`);
}

console.log(`Source audit passed (${files.length} required artifacts, ${checks.length} implementation checks; version ${packageJson.version}).`);
