'use strict';
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const required = [
  'bin/gifshot.js',
  'vendor/win32-x64/gifshot.exe',
  'README.md',
  'LICENSE',
  'CHANGELOG.md',
];

for (const relative of required) {
  const file = path.join(root, relative);
  if (!fs.existsSync(file)) throw new Error(`Missing release file: ${relative}`);
}

const exe = fs.readFileSync(path.join(root, 'vendor', 'win32-x64', 'gifshot.exe'));
if (exe.length < 2 || exe[0] !== 0x4d || exe[1] !== 0x5a) {
  throw new Error('vendor/win32-x64/gifshot.exe is not a PE executable (missing MZ header).');
}

console.log('Release package verification passed.');
