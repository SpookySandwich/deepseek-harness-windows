# deepseek-harness-windows

[English](README.md) | 中文

DeepSeek Harness 的 Windows桌面打包

![DeepSeek Harness 桌面应用](assets/screenshot.jpg)

## 实现

- 通过CI/CD生成Windows 安装包，方便启动和统一化安装/卸载
- 关闭窗口最小化到托盘，可以设置开机自启动。
- 任务完成时弹 Windows 原生通知
- mica材质套壳，以及少量细节修改增加沉浸感

## 命令行与插件

安装程序提供一个可选组件 **「Add dsh command-line tool to PATH」**。它会在 `<安装目录>\bin\dsh.cmd` 写入一个启动器（按用户安装为 `%LOCALAPPDATA%\DeepSeek Harness`，全机安装为 `%ProgramFiles%\DeepSeek Harness`），并把该 `bin` 目录加入 PATH，于是在任意终端都能用 `dsh`：

```powershell
dsh --help
dsh plugin --profile web list
```

启动器用的是随应用打包的运行时，**无需另外安装 Node 或 npm**。

### 安装插件

`dsh plugin` 会调用 **pnpm**，因此运行时自带了一份 pnpm，`dsh.cmd` 只为该子进程把它加到 PATH 前面。这样在一台没有全局 Node / npm / pnpm 的干净机器上也能装插件：

```powershell
dsh plugin --profile web add dsh-plugin-smooth-stream
```

装完请重启 DeepSeek Harness——宿主端插件代码随服务器加载。也支持直接从 GitHub 仓库安装：

```powershell
dsh plugin --profile web add github:SpookySandwich/dsh-plugin-rollout-scout
```

安装程序还可以把 [dsh-plugin-smooth-stream](https://github.com/SpookySandwich/dsh-plugin-smooth-stream) 作为可选组件预装，首次启动时由壳合并进 web 配置。

### 关于 PATH 的说明

在安装程序里改 PATH 极易出灾难性问题，所以这里**不直接改**。

NSIS 的字符串长度上限是 `NSIS_MAX_STRLEN`（1024 个字符）。超过这个长度的 PATH 从 `ReadRegStr` 读回来会**被截断或变成空字符串**，而常见的「读出—追加—写回」写法会把这个截断值写回去，直接毁掉用户的 PATH。这是 NSIS 上一个众所周知的坑，也正是本安装程序早期版本会在勾选该选项后清空 PATH 的原因。

现在的做法：

- PATH 由 **PowerShell 辅助脚本**读写，不经过 NSIS，因此不受长度上限影响。
- 读取时使用 `DoNotExpandEnvironmentNames`，写回时用 `ExpandString`，`%SystemRoot%\system32` 之类的条目保持未展开，不会被固化成字面路径。
- 辅助脚本一旦失败，PATH **保持原样不动**——`dsh.cmd` 仍可用完整路径调用。
- 追加操作是幂等的，且只有成功时才写入 `DshPathAdded` 标记。
- 卸载时**只移除当初添加的那一条**，且仅当标记表明是本安装程序添加的。

按用户安装修改用户 PATH（`HKCU\Environment`），全机安装修改系统 PATH。

这一行为有回归测试覆盖。测试会**直接从 `installer.nsi` 中提取**辅助脚本（因此测的是真正随包发布的代码，而不是一份副本），把它指向一个临时注册表键，并验证一条 2000 字符以上的 PATH 在安装与卸载后都完好无损：

```powershell
pwsh -File scripts/test-dsh-path.ps1
```

若不勾选该组件，则不会创建启动器；应用本身照常使用，你也可以自行把 `bin` 目录加入 PATH。

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
npm run tauri build             # 产出 NSIS 安装包到 src-tauri/target/release/bundle/nsis/
```
