'use strict';
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const source = path.join(root, 'native', 'target', 'release', 'gifshot.exe');
const destination = path.join(root, 'vendor', 'win32-x64', 'gifshot.exe');

if (!fs.existsSync(source)) {
  console.error(`Native binary not found: ${source}`);
  process.exit(1);
}
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.copyFileSync(source, destination);
console.log(`Staged ${destination}`);
