# GifShot

**Win+Shift+S，但是给 GIF 用的。**

GifShot 是一个轻量的 Windows 工具：框选屏幕区域，直接录成动画 GIF。npm 包只负责安装/更新与命令行；常驻运行时是原生 Rust 可执行文件，没有 Electron、浏览器引擎、FFmpeg 或 GUI 框架。

## 使用流程

1. 按下 **Win+Shift+G**。
2. 虚拟桌面变暗，指针变为十字线。
3. 在任意显示器上拖选区域。
4. 在选区旁的紧凑弹层中选择 **5 / 10 / 15 / 24 FPS**。
5. 立即开始录制。红色边框在捕获像素之外；`REC mm:ss` 计时器尽量放在选区外，否则会排除捕获或隐藏，避免刻意烧进 GIF。
6. 再次按下 **Win+Shift+G** 结束。
7. GifShot 原子写入 GIF 到 **图片\\GifShot**，并把该文件放到 Windows 剪贴板。

若 `Win+Shift+G` 已被占用，会自动回退到 `Ctrl+Shift+G` 并通知你。

常驻进程没有主窗口。用全局热键或通知区域图标操作。右键托盘图标：**录制 GIF / 设置 / 帮助 / 退出**。设置与帮助走终端交互（没有 GUI 设置窗口）。

## 系统要求

- Windows 10 版本 2004 或更高，或 Windows 11
- GifShot 1.0 需要 x64 CPU
- 通过 npm 安装/使用 CLI 需要 Node.js 18+

## 安装

```powershell
npm install -g gifshot-win
gifshot start
```

登录时自动启动：

```powershell
gifshot autostart on
```

从源码检出并完成 `npm run build:native` 后：

```powershell
npm install -g .
gifshot start
```

## 命令

```text
gifshot                 触发捕获 / 必要时启动常驻进程
gifshot start           仅启动常驻进程
gifshot stop            停止当前录制
gifshot quit            退出 GifShot
gifshot settings        交互设置（按下组合键修改快捷键）
gifshot help            用法说明
gifshot open            打开捕获文件夹
gifshot config          打开 config.json
gifshot autostart on    启用登录启动
gifshot autostart off   关闭登录启动
gifshot autostart status
gifshot doctor          安装诊断
gifshot --version
```

`gifshot settings` 是数字菜单：按下组合键修改主/备用快捷键，或打开捕获文件夹。保存快捷键后会请求正在运行的常驻进程立即重载配置（`gifshot reload`）。若未生效，执行 `gifshot quit` 再 `gifshot start`。

## 配置

首次运行会创建 `%APPDATA%\\GifShot\\config.json`。高级项放在这里，是为了让捕获流程保持轻快。改快捷键优先用 `gifshot settings`；其他字段再用 `gifshot config`。

```json
{
  "schema_version": 1,
  "hotkey": "Win+Shift+G",
  "fallback_hotkey": "Ctrl+Shift+G",
  "default_fps": 15,
  "fps_options": [5, 10, 15, 24],
  "default_quality": "medium",
  "capture_cursor": true,
  "max_duration_secs": 120,
  "dim_opacity": 128,
  "gif_quantizer_speed": 10,
  "copy_to_clipboard": true,
  "show_notifications": true,
  "output_dir": null
}
```

启动时会校验并规范化配置。损坏的文件会先备份为 `config.corrupt-<timestamp>.json`，再恢复默认。手动改 JSON 后：若常驻进程在跑，可用 `gifshot reload`；否则下次启动生效（`gifshot quit`，再 `gifshot start`）。若 `output_dir` 是相对路径，相对于 `config.json` 所在目录解析，而不是进程工作目录。

## 从源码构建

前置：Rust 1.97.1 MSVC 工具链（由 `rust-toolchain.toml` 固定）、带 Windows SDK 的 Visual Studio Build Tools、以及 Node.js 18+。

```powershell
npm install
npm run build:native
npm run verify
```

`build:native` 会生成优化版原生可执行文件，并放到 `vendor/win32-x64/gifshot.exe` 供 npm 打包。源码归档本身不含预编译 exe；由 Windows CI/发版任务产出并校验。

应用图标为 `native/assets/gifshot.ico`（构建时嵌入）。只改 `native/assets/gifshot.svg` 不会更新 exe，需重新生成 ICO 后再跑 `build:native`。

仅开发原生部分时：

```powershell
cargo fmt --manifest-path native/Cargo.toml -- --check
cargo clippy --manifest-path native/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path native/Cargo.toml
cargo build --manifest-path native/Cargo.toml --release
```

## V1 行为与刻意边界

- 可在**任意**已连接显示器上开始选区，含混合 DPI。
- 单次捕获选区限制在开始拖拽的那块显示器上，保证物理坐标、GPU 捕获面、光标与 GIF 像素一致，并避免隐性的跨适配器合成成本。
- 受保护/DRM 内容与安全桌面可能因 Windows 设计而空白或不可用。
- 在无法使用无边框 WGC 捕获的系统/策略下，Windows 可能显示显示器级捕获指示；GifShot 不依赖隐藏该指示来保证正确性。
- 剪贴板投递的是真实文件（`CF_HDROP`）。接受文件粘贴的应用会得到动画 GIF；只认位图剪贴板格式的应用可能不接受。
- GifShot 只录像素。没有音频捕获、上传、分析、账号、云客户端或内置网络服务。自定义输出目录当然可以指向由 Windows 或其他应用同步的存储。

实现、验证与发版流程见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)、[docs/TEST_PLAN.md](docs/TEST_PLAN.md)、[docs/RELEASE.md](docs/RELEASE.md)、[docs/DELIVERY_STATUS.md](docs/DELIVERY_STATUS.md)。

## 卸载

若启用过登录启动，请先关闭，避免留下失效的 Run 项：

```powershell
gifshot autostart off
gifshot quit
npm uninstall -g gifshot-win
```

## 致谢

感谢 [Linux.do](https://linux.do) 社区的支持与反馈。
