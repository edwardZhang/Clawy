# Tauri 迁移进度

## 已完成

- 建立 Tauri 迁移任务清单
- 安装 Rust 工具链
- 引入 Tauri 前端依赖与 CLI
- 新增 `src-tauri/` 工程骨架
- 新增 Tauri capability / config / icon 资源
- 新增前端启动桥接 `src/lib/desktop/bridge.ts`
- 在 Tauri 环境下注入桌面桥对象
- 新增正式前端入口 `desktopApi`
- 将页面层 / store 层对桌面全局桥的直接依赖收口到 `desktopApi`
- 在前端桥中实现本地 Gateway 客户端：
  - Gateway 启动/停止/重启
  - WebSocket `connect.challenge` 握手
  - RPC 请求/响应
  - `gateway:status-changed` / `gateway:notification` / `gateway:chat-message` / `gateway:error` / `gateway:exit` 本地事件总线
  - 自动重连
  - `chat:sendWithMedia` 本地拦截
- 将 `main.tsx` 改为先安装桌面桥再渲染 React
- 在 Rust 侧建立 `invoke_ipc` 总线
- 在 Rust 侧迁移 Gateway 运行时能力：
  - OpenClaw Gateway 进程启动、停止、重启、健康检查
  - 子进程日志采集
  - 进程退出监控
  - 设备身份生成与 challenge 签名
  - Gateway 连接状态同步
- 迁移一批基础命令：
  - `app:*`
  - `settings:*`
  - `openclaw:*`
  - `log:*`
  - `provider:*`（本地存储 + OpenClaw `auth-profiles.json` / `openclaw.json` 基础同步）
  - `file:*`（基础 staging 已可用）
  - `media:getThumbnails`
  - `media:saveImage`
  - `chat:prepareWithMedia`
  - `session:delete`
- 在前端桥中将 `cron:*` 迁移为真实 Gateway RPC：
  - `cron:list`
  - `cron:create`
  - `cron:update`
  - `cron:delete`
  - `cron:toggle`
  - `cron:trigger`
- 在 Rust 侧迁移 Skills / ClawHub / Channels 的真实本地能力：
  - `skill:getConfig`
  - `skill:getAllConfigs`
  - `skill:updateConfig`
  - `clawhub:list`
  - `clawhub:search`
  - `clawhub:install`
  - `clawhub:uninstall`
  - `clawhub:openSkillReadme`
  - `channel:saveConfig`
  - `channel:getConfig`
  - `channel:getFormValues`
  - `channel:listConfigured`
  - `channel:deleteConfig`
  - `channel:setEnabled`
  - `channel:validate`
  - `channel:validateCredentials`
- 在 Rust 侧迁移剩余 Electron 重依赖能力：
  - `update:*`
    - 自定义 `release-info.json` 检查
    - 平台产物选择
    - 下载进度事件
    - 自动下载与自动安装倒计时事件
    - 打开已下载安装包
  - `provider:requestOAuth`
  - `provider:cancelOAuth`
  - `channel:requestWhatsAppQr`
  - `channel:cancelWhatsAppQr`
  - `uv:check`
  - `uv:install-all`
  - `usage:recentTokenHistory`
  - `app:relaunch`
- 新增本地 Node runner：
  - `scripts/tauri/oauth-runner.mjs`
  - `scripts/tauri/whatsapp-runner.mjs`
- 在 Rust 侧补齐 OAuth / WhatsApp 子进程桥：
  - stdout JSON 事件解析
  - OAuth 成功后写入 provider store
  - OAuth token 写入 OpenClaw `auth-profiles.json`
  - OAuth provider runtime override 写入 `openclaw.json`
  - WhatsApp QR / success / error 事件转发
- 新增 Tauri 原生菜单与托盘：
  - 页面导航菜单
  - Help / Docs / Issue 链接
  - 托盘显示/隐藏主窗口
  - 托盘更新检查入口
  - 托盘 tooltip 跟随 Gateway 状态变化
- 调整 Tauri build：
  - `beforeBuildCommand` 增加 OpenClaw / 插件 bundle
  - 打包脚本 runner 资源
- 切换为 Tauri-only 前端构建链：
  - 删除 `vite-plugin-electron`
  - 删除 `electron-builder` / `electron-updater` / `electron-store` 依赖
  - 将 `package.json` 默认开发 / 构建 / 打包命令切到 Tauri
  - 简化 `vite.config.ts` 与 `tsconfig.node.json`
- 删除旧 Electron 代码与测试：
  - 删除 `electron/`
  - 删除 Electron-only 单测
  - 删除 `electron-builder.yml` 与旧安装脚本
- 删除旧 preload 类型定义与 Electron 命名残留：
  - `src/types/electron.d.ts` -> `src/types/desktop.d.ts`
  - 前端不再出现 `window.electron`
- `pnpm typecheck` 通过
- `pnpm run lint` 通过
- `pnpm test` 通过（4 个测试文件 / 29 个测试）
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过
- `pnpm tauri build --debug --no-bundle` 通过
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `pnpm tauri build --debug --bundles app` 通过

## 进行中

- 收口 Tauri bundle 资源布局，进一步逼近最终发布形态
- 收敛 Channels 与 OpenClaw 最新配置模型之间的差异（尤其是 WhatsApp / DingTalk）

## 下一步

1. 收敛 Skills / Channels / ClawHub 到最终 `desktopApi`，移除兼容桥语义
2. 继续缩减桌面桥兼容面，逐步去掉仅为迁移保留的 IPC 命名
3. 收口 Tauri-only 发布物与发布流程
