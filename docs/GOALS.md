# 项目目标

## 一句话

把 DeepSeek Harness（`dsh`）做成一个 Windows 桌面应用。核心思路：**不改上游源码**，用 Tauri 2 壳 + Node sidecar 包住 npm 上发布的 `@deepseek-ai/dsh`，补上它缺的几样能力。

## 要补的能力（就是最初的五个需求）

1. **Windows 安装包** —— 装完进开始菜单，不用再敲命令。
2. **Windows 11 Mica 材质** —— 窗口和侧边栏透 Mica，和标题栏无缝。
3. **系统托盘** —— 关闭窗口最小化到托盘，托盘可显示/退出/切换自启。
4. **任务完成原生通知** —— 任务结束时弹 Windows toast。
5. **开机自启动** —— 首次运行写入自启项。

## 已经做完的

- Tauri 2 壳 + Node sidecar（npm 版 `@deepseek-ai/dsh`）跑通，双击即用。
- Mica 材质调好（侧边栏透 Mica + 内容卡片层），见 [MICA.md](MICA.md)。
- 托盘 / 自启 / 通知逻辑已实现。
- 图标（1254px 源图重新生成）、README 截图、安全规范（[SAFETY.md](SAFETY.md)）。

## 还没做的（当前目标）

### 1. 版本跟 dsh 对齐

安装包版本 = 它打包进去的 `@deepseek-ai/dsh` 版本，**在 CI 里动态获取**，不写死。

- 展示 / 文件名用 dsh 原版本号（如 `0.1.0-rc.6`）。
- 安装包文件名带版本：`DeepSeek Harness_0.1.0-rc.6_x64-setup.exe`。
- Windows 注册表 / MSI 需要纯数字版本，做一次映射（`0.1.0-rc.6` → `0.1.0.6`）。

### 2. CI 打 tag 自动发 release

- 推 `v<version>` tag → CI 打包 NSIS + MSI → 创建 GitHub Release 并附上安装包。
- **Release 名称**：`DeepSeek Harness v<version>`。
- tag 名用 `v<version>`（version = 要打包的 dsh 版本）。

### 3. 打包装好

- 产出 NSIS（.exe）+ MSI 两个安装包。
- 装好后：开始菜单入口 + 自启指向安装路径（而不是 debug exe）。

## 版本获取方式（待实现）

- `npm view @deepseek-ai/dsh version` → 拿到 dsh 版本（如 `0.1.0-rc.6`）。
- 展示用：`0.1.0-rc.6`（release 名、文件名）。
- Windows 用：`0.1.0.6`（把 `-rc.` 转成 `.`）。
