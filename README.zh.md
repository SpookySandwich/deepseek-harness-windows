# deepseek-harness-windows

[English](README.md) | 中文

DeepSeek Harness 的 Windows桌面打包

![DeepSeek Harness 桌面应用](assets/screenshot.jpg)

## 实现

- 通过CI/CD生成Windows 安装包，方便启动和统一化安装/卸载
- 关闭窗口最小化到托盘，可以设置开机自启动。
- 任务完成时弹 Windows 原生通知
- mica材质套壳，以及少量细节修改增加沉浸感

## 架构

```text
Tauri 2 壳 (Rust)                           sidecar (node.exe + node_modules)
  窗口: Mica 材质 + 注入 CSS                    node .../dsh/lib/bin.js web
  托盘 / 通知 / 自启                             -> http://127.0.0.1:<空闲端口>
  spawn sidecar ----------------------------->  WebView2 加载该本地端口
```

## 目录

- `shell/` — Tauri 2 壳（Rust 主逻辑 + 闪屏页）。
- `scripts/prepare-runtime.ps1` — 生成 sidecar 运行时（安装 dsh + 复制 node.exe / node_modules）。
- `.github/workflows/release.yml` — CI：拉取 → 准备运行时 → 打包 → 发布。
- `docs/SAFETY.md` — 自托管环境安全操作规范（端口/进程隔离铁律）。

## 本地开发

前置：Rust（MSVC toolchain）、Node 22+。

```powershell
# 1. 准备开发用 sidecar 运行时
npm install @deepseek-ai/dsh@latest --prefix sidecar --omit=dev --no-audit --no-fund

# 2. 装壳依赖并启动
cd shell
npm install
$env:DSH_RUNTIME_DIR = "D:/deepseek-harness-windows/sidecar"
npm run tauri dev
```

## 打包

```powershell
./scripts/prepare-runtime.ps1   # 生成 shell/src-tauri/resources/runtime
cd shell
npm install
npm run tauri build             # 产出 NSIS + MSI 到 src-tauri/target/release/bundle/
```
