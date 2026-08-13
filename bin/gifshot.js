#!/usr/bin/env node
'use strict';

/**
 * GifShot npm launcher.
 *
 * The JavaScript layer is deliberately tiny: npm handles installation/update,
 * while all latency-sensitive behavior lives in the native Rust executable.
 * Interactive settings/help also live here so the terminal UX stays simple.
 */

const fs = require('node:fs');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');
const {
  runSettings,
  printHelp,
  maybePause,
  captureDirFromConfig,
} = require('./interactive');

const APP_NAME = 'GifShot';
const { version: VERSION } = require('../package.json');
const RUN_KEY = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run';
const RUN_VALUE = 'GifShot';

function fail(message, code = 1) {
  console.error(`gifshot: ${message}`);
  process.exit(code);
}

function binaryPath() {
  const packaged = path.resolve(__dirname, '..', 'vendor', 'win32-x64', 'gifshot.exe');
  if (fs.existsSync(packaged)) return packaged;

  // Developer fallback after `npm run build:native`.
  const dev = path.resolve(__dirname, '..', 'native', 'target', 'release', 'gifshot.exe');
  if (fs.existsSync(dev)) return dev;

  fail('native binary is missing. Reinstall the package or run `npm run build:native` from source.');
}

function ensurePlatform() {
  if (process.platform !== 'win32') {
    fail('GifShot 1.0 supports Windows only.');
  }
  if (process.arch !== 'x64') {
    fail(`GifShot 1.0 supports Windows x64; detected ${process.arch}.`);
  }
}

function launchNative(args, { detached = true } = {}) {
  ensurePlatform();
  const exe = binaryPath();
  const child = spawn(exe, args, {
    detached,
    stdio: 'ignore',
    windowsHide: true,
  });
  child.on('error', (error) => fail(`could not launch native runtime: ${error.message}`));
  if (detached) child.unref();
  return child;
}

function startupCommand() {
  // The registry accepts a command line string. Quoting is mandatory because npm
  // global install paths commonly contain spaces.
  return `"${binaryPath()}" --background`;
}

function setAutostart(enabled) {
  ensurePlatform();

  // Make `autostart off` idempotent. Registry delete returns a non-zero status
  // when the value is already absent, which should still be considered success.
  if (!enabled) {
    const existing = spawnSync('reg.exe', ['query', RUN_KEY, '/v', RUN_VALUE], {
      encoding: 'utf8',
      windowsHide: true,
    });
    if (existing.status !== 0) {
      console.log('GifShot autostart already disabled.');
      return;
    }
  }

  const args = enabled
    ? ['add', RUN_KEY, '/v', RUN_VALUE, '/t', 'REG_SZ', '/d', startupCommand(), '/f']
    : ['delete', RUN_KEY, '/v', RUN_VALUE, '/f'];
  const result = spawnSync('reg.exe', args, { encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || '').trim();
    fail(`could not ${enabled ? 'enable' : 'disable'} autostart${detail ? `: ${detail}` : ''}`);
  }
  console.log(`GifShot autostart ${enabled ? 'enabled' : 'disabled'}.`);
}

function autostartStatus() {
  ensurePlatform();
  const result = spawnSync('reg.exe', ['query', RUN_KEY, '/v', RUN_VALUE], {
    encoding: 'utf8',
    windowsHide: true,
  });
  console.log(result.status === 0 ? 'enabled' : 'disabled');
}

function doctor() {
  ensurePlatform();
  const exe = binaryPath();
  const stat = fs.statSync(exe);
  const status = spawnSync('reg.exe', ['query', RUN_KEY, '/v', RUN_VALUE], {
    encoding: 'utf8',
    windowsHide: true,
  }).status === 0;

  console.log(`${APP_NAME} ${VERSION}`);
  console.log(`platform: ${process.platform}/${process.arch}`);
  console.log(`runtime:  ${exe}`);
  console.log(`binary:   ${(stat.size / 1024 / 1024).toFixed(2)} MiB`);
  console.log(`autostart:${status ? ' enabled' : ' disabled'}`);
}

function takeFlag(args, name) {
  const index = args.indexOf(name);
  if (index < 0) return false;
  args.splice(index, 1);
  return true;
}

async function main() {
  const args = process.argv.slice(2);
  const pauseAfter = takeFlag(args, '--pause-after');
  const command = (args[0] || 'capture').toLowerCase();
  if (pauseAfter) {
    if (command === 'settings' || command === 'setting' || command === 'prefs') {
      process.title = 'GifShot 设置';
    } else if (command === 'help' || command === '--help' || command === '-h') {
      process.title = 'GifShot 帮助';
    } else {
      process.title = 'GifShot';
    }
  }

  switch (command) {
    case 'capture':
      launchNative(['capture']);
      break;
    case 'start':
    case 'background':
      launchNative(['--background']);
      break;
    case 'stop':
    case 'quit':
    case 'open':
    case 'config':
    case 'reload':
      launchNative([command]);
      break;
    case 'settings':
    case 'setting':
    case 'prefs':
      ensurePlatform();
      await runSettings({
        launchNative,
        captureDir: captureDirFromConfig,
        binaryPath: binaryPath(),
      });
      break;
    case 'autostart': {
      const mode = (args[1] || 'status').toLowerCase();
      if (mode === 'on' || mode === 'enable') setAutostart(true);
      else if (mode === 'off' || mode === 'disable') setAutostart(false);
      else if (mode === 'status') autostartStatus();
      else fail('usage: gifshot autostart on|off|status');
      break;
    }
    case 'doctor':
      doctor();
      break;
    case 'help':
    case '--help':
    case '-h':
      printHelp(VERSION);
      break;
    case '--version':
    case '-v':
    case 'version':
      console.log(VERSION);
      break;
    default:
      fail(`unknown command: ${command}. Run \`gifshot help\`.`);
  }

  await maybePause(pauseAfter);
}

main().catch((error) => {
  fail(error && error.message ? error.message : String(error));
});
