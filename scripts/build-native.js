'use strict';
const { spawnSync } = require('node:child_process');
const path = require('node:path');

if (process.platform !== 'win32') {
  console.error('build:native must run on Windows because GifShot links Win32/WGC APIs.');
  process.exit(1);
}

const root = path.resolve(__dirname, '..');
const result = spawnSync('cargo', [
  'build',
  '--manifest-path', path.join(root, 'native', 'Cargo.toml'),
  '--release',
], { cwd: root, stdio: 'inherit' });

if (result.error) {
  console.error(`Could not run cargo: ${result.error.message}`);
  process.exit(1);
}
if (result.status !== 0) process.exit(result.status || 1);

require('./stage-native.js');
