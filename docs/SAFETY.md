# 安全操作规范（自托管环境）

本项目 deepseek-harness-windows 把 DeepSeek Harness（`dsh`）包成 Windows 桌面应用。
开发机 / 测试机本身可能就运行着一个 `dsh` 实例——包括正在构建本项目的 agent harness——监听 `http://127.0.0.1:3080`。

因此，本项目任何测试 / 打包出来的实例，都必须与这个"活实例"严格隔离。

## 两条铁律

**R1 端口隔离**：本项目的壳与任何测试实例**绝不绑定 3080**。

- 壳默认使用 `--port 0`（由操作系统分配空闲端口）。
- 固定端口只允许 `>= 3090`。
- 永远不要用默认端口（3080）跑测试实例。

**R2 进程隔离**：**禁止**任何批量或按端口的杀进程操作。

- 禁止 `taskkill /F /IM node.exe`、`Stop-Process -Name node`、`npx kill-port 3080` 之类命令。
- 只能 kill 自己 spawn 的那个 sidecar 的**精确 PID**，且该 PID 必须先落盘、kill 前核对。

## 为什么这样是安全的

- 端口冲突是 fail-safe：若测试实例误绑 3080，Node 抛 `EADDRINUSE`，结果是**新实例启动失败、活实例不受影响**。
- 唯一会伤到运行中 harness 的路径是"批量杀 node / 按端口杀进程"，已被 R2 禁止。
- 壳只管理自己 spawn 的 sidecar PID，不扫描端口、不反查进程。

## 测试核对清单（人开始测之前先读）

1. 确认活实例仍占着 3080：`Get-NetTCPConnection -LocalPort 3080 -State Listen`。
2. 启动壳 / 测试实例，确认它用的是 3090 以上，或 `--port 0`。
3. 结束测试实例时：只执行 `Stop-Process -Id <sidecar pid> -Force`，不要按名字或端口杀。
4. 收尾时再执行第 1 步，确认 3080 的活实例健在。
