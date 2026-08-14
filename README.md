# deepseek-harness-windows

English | [中文](README.zh.md)

Windows desktop packaging for DeepSeek Harness

![DeepSeek Harness desktop app](assets/screenshot.jpg)

## Features

- Windows installers built via CI/CD, for easy startup and consistent install/uninstall
- Closing the window minimizes to the tray; launch at login can be enabled.
- Pops a native Windows notification when a task completes
- Mica shell, plus a few detail tweaks for a more immersive feel

## Architecture

```text
Tauri 2 shell (Rust)                        sidecar (node.exe + node_modules)
  window: Mica + injected CSS                 node .../dsh/lib/bin.js web
  tray / notifications / autostart            -> http://127.0.0.1:<free port>
  spawn sidecar -------------------------->   WebView2 loads that local port
```

## Layout

- `shell/` — Tauri 2 shell (Rust logic + splash page).
- `scripts/prepare-runtime.ps1` — builds the sidecar runtime (installs dsh + copies node.exe / node_modules).
- `.github/workflows/release.yml` — CI: fetch → prepare runtime → build → release.
- `docs/SAFETY.md` — safety rules for self-hosted environments (port/process isolation).

## Development

Prerequisites: Rust (MSVC toolchain), Node 22+.

```powershell
# 1. Prepare the dev sidecar runtime
npm install @deepseek-ai/dsh@latest --prefix sidecar --omit=dev --no-audit --no-fund

# 2. Install shell deps and run
cd shell
npm install
$env:DSH_RUNTIME_DIR = "D:/deepseek-harness-windows/sidecar"
npm run tauri dev
```

## Packaging

```powershell
./scripts/prepare-runtime.ps1   # produces shell/src-tauri/resources/runtime
cd shell
npm install
npm run tauri build             # NSIS + MSI in src-tauri/target/release/bundle/
```
