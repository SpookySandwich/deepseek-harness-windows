# Windows 11 Mica 材质（壳 UI）

这个壳（`shell/`）的窗口材质是按 Win11 的分层思路做的：**侧边栏直接坐在纯 Mica 上，跟标题栏无缝连成一片**；内容区则是 WinUI 风格的"卡片层"——比底色略亮的半透明填充，左上 8px 圆角，外加一条 1px 细描边。

## 改哪里

- **原生层**：`shell/src-tauri/src/lib.rs` 里建窗口的地方，`.transparent(true)` + `.effects(WindowEffect::Mica)`。标题栏是 DWM 画的，等于零叠加的纯 Mica。
- **网页层**：同一个文件里的 `MICA_SCRIPT` 常量，通过 `.initialization_script(MICA_SCRIPT)` 注入到每一次页面加载（启动页和 dsh web 应用都会注入）。**以后所有视觉调参都改这个常量里的 CSS，别去动 dsh 源码。**

## 踩过的几个坑

1. **覆盖 token 要挂在 `body` 上，而不是 `:root`。** `design-platform.css` 把 `--dsw-*` 别名定义在 `body` / `body[data-ds-dark-theme]` 上；CSS 自定义属性按"最近的定义祖先"来解析，写在 `:root` 上的覆盖会被 `body` 自己顶掉，对后代全部失效。最早的版本就是死在这。

2. **透明度是由层级决定的。** 自下而上是 Mica → body → `.frame`（全窗垫底）→ 三列。想让侧边栏透出 Mica，就得把它下面垫着的 body 和 `.frame` 都清成透明，光清侧边栏自己没用。内容区（ConversationRoot、DetailsPanel）会自己再刷一层 `bg-base`，所以清掉 `.frame` 不会影响内容区的观感。

3. **类名被 CSS Module 哈希了，没法直接写。** 稳定锚点是：`.frame` = `div:has(> [data-shell-overlay])`，三列用它的 `nth-of-type(1/2/3)`（1=侧边栏，2=内容，3=详情，顺序见 `deepseek-harness/packages/client/ui-layout/src/client/AppFrame.tsx`）。所有规则都带 `!important`（注入的样式可能先于应用样式加载）。

4. **token 各管一摊：** `--dsw-alias-bg-base` = 内容面填充；`--dsw-specific-sidebar-fill` = 侧边栏列；`--dsw-alias-bg-layer-1/2/3` = 弹窗 / 设置 / 浮层——这一组**保持不透明，别碰**。

## 调参 & 验证

- 内容区保持原生不透明（试过透 Mica，观感不好又回退了）；材质感只来自侧边栏和卡片圆角边缘露出来的那点 Mica。以后要是想再试内容层渗透，就在 `body[data-ds-dark-theme]` 上覆盖 `--dsw-alias-bg-base: rgba(21, 21, 23, <alpha>)`，alpha 越小越透。Win11 的原则是：阅读区别强透（要保证可读性），材质感靠卡片边缘露出的 Mica 来体现。
- 暗色 Mica 很吃壁纸：纯黑壁纸下几乎看不见，**调试时换张亮色壁纸对照**。
- 验证流程：改完 `MICA_SCRIPT` → `cargo build --manifest-path shell/src-tauri/Cargo.toml` → 按 `docs/SAFETY.md` 的进程隔离规矩重启（只 kill 精确 PID）→ `DSH_RUNTIME_DIR=<sidecar 目录>` 跑 `target/debug/deepseek-harness-windows.exe`。
