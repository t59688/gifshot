'use strict';

/**
 * Interactive terminal settings and help for GifShot.
 * Kept in Node so `gifshot settings` / `gifshot help` feel native to the CLI,
 * while the resident Rust process only reloads config when asked.
 */

const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');
const { spawnSync } = require('node:child_process');

function configPath() {
  const appdata = process.env.APPDATA;
  if (!appdata) {
    throw new Error('APPDATA is not set');
  }
  return path.join(appdata, 'GifShot', 'config.json');
}

function defaultConfig() {
  return {
    schema_version: 1,
    hotkey: 'Win+Shift+G',
    fallback_hotkey: 'Ctrl+Shift+G',
    default_fps: 15,
    fps_options: [5, 10, 15, 24],
    default_quality: 'medium',
    capture_cursor: true,
    max_duration_secs: 120,
    dim_opacity: 128,
    gif_quantizer_speed: 10,
    copy_to_clipboard: true,
    show_notifications: true,
    output_dir: null,
  };
}

function loadConfig() {
  const file = configPath();
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (!fs.existsSync(file)) {
    const cfg = defaultConfig();
    saveConfig(cfg);
    return cfg;
  }
  try {
    return { ...defaultConfig(), ...JSON.parse(fs.readFileSync(file, 'utf8')) };
  } catch {
    const cfg = defaultConfig();
    saveConfig(cfg);
    return cfg;
  }
}

function saveConfig(cfg) {
  const file = configPath();
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, `${JSON.stringify(cfg, null, 2)}\n`, 'utf8');
  fs.renameSync(tmp, file);
}

/** Mirror native/src/hotkey.rs enough to reject bad input before saving. */
function validateHotkey(input) {
  const raw = String(input || '').trim();
  if (!raw) return { ok: false, error: '不能为空' };

  const tokens = raw.split('+').map((t) => t.trim()).filter(Boolean);
  if (tokens.length < 2) {
    return { ok: false, error: '至少需要一个修饰键 + 一个主键，例如 Ctrl+Shift+G' };
  }

  let modifiers = 0;
  let key = null;
  const pretty = [];

  for (const token of tokens) {
    const lower = token.toLowerCase();
    if (['win', 'windows', 'meta'].includes(lower)) {
      modifiers += 1;
      pretty.push('Win');
    } else if (['ctrl', 'control'].includes(lower)) {
      modifiers += 1;
      pretty.push('Ctrl');
    } else if (lower === 'shift') {
      modifiers += 1;
      pretty.push('Shift');
    } else if (lower === 'alt') {
      modifiers += 1;
      pretty.push('Alt');
    } else if (key) {
      return { ok: false, error: '只能有一个主键' };
    } else if (/^[a-z0-9]$/i.test(token)) {
      key = token.toUpperCase();
      pretty.push(key);
    } else if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(token)) {
      key = `F${token.slice(1)}`;
      pretty.push(key);
    } else {
      return { ok: false, error: `不支持的键: ${token}` };
    }
  }

  if (!modifiers) return { ok: false, error: '至少需要一个修饰键（Win / Ctrl / Shift / Alt）' };
  if (!key) return { ok: false, error: '缺少主键（字母、数字或 F1–F24）' };

  return { ok: true, value: pretty.join('+') };
}

function ask(rl, question) {
  return new Promise((resolve) => rl.question(question, resolve));
}

function openFolder(dir) {
  const target = path.resolve(dir);
  fs.mkdirSync(target, { recursive: true });

  // Never pass windowsHide to explorer.exe — CREATE_NO_WINDOW swallows the
  // folder window while still looking like success.
  const result = spawnSync('explorer.exe', [target], {
    windowsHide: false,
  });
  if (!result.error) {
    return;
  }

  // Fallback: empty title is required so `start` does not treat the path as a title.
  const fallback = spawnSync('cmd.exe', ['/c', 'start', '""', target], {
    windowsHide: true,
    encoding: 'utf8',
  });
  if (fallback.error) {
    throw fallback.error;
  }
  if (fallback.status !== 0 && fallback.status != null) {
    throw new Error((fallback.stderr || fallback.stdout || 'failed to open folder').trim());
  }
}

function requestReload(launchNative) {
  // Best-effort: tell the resident process to re-read config + rebind hotkeys.
  launchNative(['reload'], { detached: true });
}

async function editHotkey(rl, cfg, field, label, launchNative, binaryPath) {
  console.log();
  console.log(`${label}（当前 ${cfg[field]}）`);
  console.log('请按下新的快捷键组合（需含 Win/Ctrl/Alt/Shift）· Esc 取消');

  // Release stdin from readline so the native hook owns the keyboard session.
  rl.pause();
  let captured = '';
  let status = 0;
  try {
    const result = spawnSync(binaryPath, ['capture-hotkey'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      windowsHide: true,
    });
    status = result.status == null ? 2 : result.status;
    captured = String(result.stdout || '').trim();
  } finally {
    rl.resume();
  }

  if (status === 1) {
    console.log('已取消。');
    return false;
  }
  if (status !== 0 || !captured) {
    console.log('未能捕获快捷键，可改用手动输入。');
    const typed = (await ask(rl, '输入组合（例 Ctrl+Alt+G，回车取消）: ')).trim();
    if (!typed) {
      console.log('已取消。');
      return false;
    }
    captured = typed;
  }

  const checked = validateHotkey(captured);
  if (!checked.ok) {
    console.log(`无效：${checked.error}`);
    return false;
  }
  if (field === 'hotkey' && checked.value === cfg.fallback_hotkey) {
    console.log('主快捷键不能与备用快捷键相同。');
    return false;
  }
  if (field === 'fallback_hotkey' && checked.value === cfg.hotkey) {
    console.log('备用快捷键不能与主快捷键相同。');
    return false;
  }

  cfg[field] = checked.value;
  saveConfig(cfg);
  requestReload(launchNative);
  console.log(`已保存为 ${checked.value}，并尝试立即生效。`);
  return true;
}

async function runSettings({ launchNative, captureDir, binaryPath }) {
  if (!process.stdin.isTTY) {
    console.error('当前窗口无法交互输入。请在终端运行: gifshot settings');
    spawnSync('cmd.exe', ['/c', 'echo. & pause'], { stdio: 'inherit' });
    return;
  }
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  try {
    console.log();
    console.log('GifShot 设置');
    for (;;) {
      const cfg = loadConfig();
      console.log('────────────────────────');
      console.log(`  1. 主快捷键      ${cfg.hotkey}`);
      console.log(`  2. 备用快捷键    ${cfg.fallback_hotkey}`);
      console.log('  3. 打开捕获文件夹');
      console.log('  0. 退出');
      console.log('────────────────────────');
      const choice = (await ask(rl, '请选择 [0-3]: ')).trim();

      if (choice === '0' || choice === '') {
        console.log('再见。');
        return;
      }
      if (choice === '1') {
        await editHotkey(rl, cfg, 'hotkey', '主快捷键', launchNative, binaryPath);
        continue;
      }
      if (choice === '2') {
        await editHotkey(rl, cfg, 'fallback_hotkey', '备用快捷键', launchNative, binaryPath);
        continue;
      }
      if (choice === '3') {
        try {
          const dir = captureDir(cfg);
          fs.mkdirSync(dir, { recursive: true });
          openFolder(dir);
          console.log(`已打开：${dir}`);
        } catch (error) {
          console.error(`无法打开捕获文件夹：${error.message}`);
        }
        continue;
      }
      console.log('请输入 0–3。');
    }
  } finally {
    rl.close();
  }
}

function printHelp(version) {
  console.log(`GifShot ${version} — 区域录制成 GIF

用法（一条热键走完）
  1. 按主快捷键（默认 Win+Shift+G）
  2. 拖选屏幕区域
  3. 点选帧率：5 / 10 / 15 / 24 FPS
  4. 再按同一热键结束
  5. GIF 写入「图片\\GifShot」，并复制到剪贴板

常用命令
  gifshot              触发捕获（未运行则先启动）
  gifshot start        仅启动常驻
  gifshot stop         停止当前录制
  gifshot quit         退出常驻
  gifshot settings     交互设置（改快捷键等）
  gifshot help         显示本说明
  gifshot open         打开捕获文件夹
  gifshot config       打开 config.json
  gifshot autostart on|off|status
  gifshot doctor

快捷键
  主快捷键被占用时，自动改用备用快捷键并通知你。
  在设置里改完一般立即生效；若未生效：gifshot quit 后 gifshot start。

更多
  配置文件：%APPDATA%\\GifShot\\config.json
  托盘图标：右键可录制 / 设置 / 帮助 / 退出`);
}

async function maybePause(pauseAfter) {
  if (!pauseAfter) return;
  // When stdin is not a TTY (bad console attach), readline returns immediately.
  if (!process.stdin.isTTY) {
    spawnSync('cmd.exe', ['/c', 'echo. & pause'], { stdio: 'inherit' });
    return;
  }
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  try {
    await ask(rl, '\n按回车关闭…');
  } finally {
    rl.close();
  }
}

function captureDirFromConfig(cfg) {
  if (cfg.output_dir) {
    return path.isAbsolute(cfg.output_dir)
      ? cfg.output_dir
      : path.join(path.dirname(configPath()), cfg.output_dir);
  }
  const pictures = process.env.USERPROFILE
    ? path.join(process.env.USERPROFILE, 'Pictures', 'GifShot')
    : path.join('GifShot');
  return pictures;
}

module.exports = {
  runSettings,
  printHelp,
  maybePause,
  captureDirFromConfig,
  validateHotkey,
  loadConfig,
  configPath,
};
