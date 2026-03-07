# ClawX Tauri 迁移任务清单

## 1. 文档目标

本文档用于将当前基于 Electron 的 ClawX 完整迁移到 Tauri。目标不是“引入第二套桌面壳并长期双维护”，而是：

- 保留现有 React + Vite + TypeScript 前端
- 完全替换 Electron 主进程、预加载、IPC、打包、更新链路
- 保留 OpenClaw / Gateway 的运行模式，但将其改为由 Tauri Rust Core 管理
- 在迁移过程中提升安全性，特别是 Provider 密钥与 OAuth Token 存储

本文档按模块、阶段、交付件、验收标准拆分研发任务，适合作为立项和执行清单。

---

## 2. 总体迁移原则

### 2.1 范围内

- React 前端保留
- Zustand / React Router / i18n 保留
- OpenClaw / Gateway 保留
- 业务模型保留：Provider、Skills、Channels、Cron、Chat、Settings
- 资源打包保留：OpenClaw、内建技能、平台二进制、CLI 资源

### 2.2 范围外

- 不重写 OpenClaw 为 Rust
- 不在本阶段重做 UI
- 不在本阶段新增新业务功能

### 2.3 强制技术约束

- 前端不得继续直接依赖 `window.electron`
- 所有桌面能力必须收敛到统一 `desktopApi`
- 敏感凭据不得继续明文保存在本地 JSON store
- 迁移期间必须保证 Electron 版可回退，直到 Tauri 版完成发布验证

---

## 3. 当前系统拆解

## 3.1 需要迁移的 Electron 能力域

- 应用生命周期：单实例、启动初始化、退出/重启
- 窗口系统：主窗口、标题栏控制、最小化/最大化/关闭
- 系统集成：托盘、菜单、外链打开、路径打开
- 桌面后端：IPC handlers、事件分发
- 子进程编排：OpenClaw Gateway 生命周期管理
- 本地文件能力：日志读取、文件选择、附件 staging、缩略图、图片保存、session 删除
- 配置存储：settings、window state、provider config
- 密钥与 OAuth：API Key、OAuth token、默认 provider 同步
- 网络/代理：桌面层代理设置、Gateway 代理注入
- 更新系统：检查、下载、安装、渠道切换
- 打包分发：安装包、资源拷贝、平台差异处理

## 3.2 不建议直接 1:1 保留的能力

- Electron `<webview>` 能力
- 依赖 `electron-updater` 的发布链
- 明文 `electron-store` 的密钥存储

---

## 4. 目标架构

## 4.1 迁移后架构

- 前端：React + Vite
- 桌面核心：Tauri 2 + Rust
- Gateway 管理：Rust sidecar manager
- OpenClaw runtime：Node 进程 / sidecar 进程
- 配置存储：
  - 非敏感配置 -> JSON store
  - 敏感配置 -> OS keyring/keychain
- 桌面通信：
  - request/response -> Tauri commands
  - push events -> Tauri event emitter

## 4.2 推荐目录结构

```text
src/
  lib/
    desktop/
      index.ts
      types.ts
      electron.ts
      tauri.ts
src-tauri/
  src/
    main.rs
    app/
    commands/
    events/
    gateway/
    storage/
    providers/
    files/
    updater/
    tray/
    menu/
    oauth/
    openclaw/
```

---

## 5. 里程碑规划

## 5.1 M1：接口收口与 Tauri 骨架

目标：

- 前端不再直接依赖 Electron
- Tauri 工程可启动基础窗口
- 桌面接口有统一抽象层

## 5.2 M2：本地后端替换

目标：

- Gateway、文件、存储、设置、Provider、日志、窗口、托盘迁移到 Rust
- 功能可用性达到 Electron 版主路径一致

## 5.3 M3：发布链与更新链切换

目标：

- 安装包可产出
- 自动更新可用
- 跨平台签名/发布流程打通

## 5.4 M4：安全与稳定性收尾

目标：

- 凭据迁移完成
- 回归测试完成
- Electron 构建链可下线

---

## 6. 详细任务清单

## A. 迁移准备

### A1. 建立迁移分支与工作约束

- 创建 `codex/tauri-migration` 或等价迁移分支
- 冻结 Electron 主干上的高风险结构调整
- 明确迁移期允许并行开发的模块边界

交付件：

- 迁移分支
- 研发约束说明

验收标准：

- 团队确认迁移期间禁止直接新增 `window.electron` 使用点

### A2. 建立迁移资产清单

- 列出现有 Electron IPC 清单
- 列出所有前端调用点
- 列出所有主进程事件通道
- 列出所有打包资源和平台二进制

交付件：

- IPC 映射表
- 资源清单

验收标准：

- 每个 Electron API 都能映射到调用文件和目标替代方案

---

## B. 前端适配层改造

### B1. 新建 `desktopApi` 抽象层

- 创建 `src/lib/desktop/`
- 设计统一接口：
  - app
  - window
  - shell
  - dialog
  - gateway
  - provider
  - settings
  - files
  - media
  - updates
  - events

交付件：

- `src/lib/desktop/types.ts`
- `src/lib/desktop/index.ts`

验收标准：

- 页面层不直接依赖 `window.electron`

### B2. 接入 Electron 适配实现

- 用现有 Electron API 实现 `desktopApi`
- 保证前端行为不变

交付件：

- `src/lib/desktop/electron.ts`

验收标准：

- 现有 Electron 版功能可正常运行
- 全量 `window.electron` 直接调用被替换

### B3. 重构前端调用点

- 替换 `src/pages/`
- 替换 `src/stores/`
- 替换 `src/components/`
- 替换 `src/lib/`

重点模块：

- Chat
- Settings
- Dashboard
- Skills
- Channels
- Setup
- Sidebar
- TitleBar

交付件：

- 前端调用改造 PR

验收标准：

- `rg "window\\.electron" src` 仅允许出现在 Electron 适配层

---

## C. Tauri 工程骨架

### C1. 初始化 Tauri 2 工程

- 增加 `src-tauri/`
- 配置 Tauri 与现有 Vite 集成
- 打通 `pnpm tauri dev`
- 打通 `pnpm tauri build`

交付件：

- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- 基础 `main.rs`

验收标准：

- 能启动空壳应用并加载现有前端

### C2. 引入基础插件

建议插件范围：

- shell
- dialog
- fs
- process
- updater
- single-instance
- os
- clipboard-manager
- store 或等价配置方案

交付件：

- Tauri 插件依赖配置

验收标准：

- 基础平台能力可通过 command 或 plugin 正常调用

---

## D. 桌面 command / event 契约

### D1. 定义 Rust command 契约

- 为现有 Electron IPC 设计等价 command
- 避免直接暴露 Electron 风格 channel 字符串
- 按模块定义 payload / result 类型

交付件：

- command 清单
- TypeScript 类型定义
- Rust DTO 定义

验收标准：

- `desktopApi` 与 Rust commands 一一对应

### D2. 定义事件模型

- Gateway status changed
- Gateway notification
- Gateway error
- Gateway chat message
- OAuth events
- Update status events
- Navigation events

交付件：

- event name 规范
- event payload schema

验收标准：

- 事件订阅和取消订阅语义清晰

---

## E. 应用与窗口能力迁移

### E1. 应用信息与生命周期

- 版本号
- 应用名
- 平台识别
- quit
- relaunch
- 单实例锁

交付件：

- `commands/app.rs`

验收标准：

- 相关设置页和更新页可正常展示

### E2. 窗口控制

- 最小化
- 最大化 / 还原
- 关闭
- 查询最大化状态
- 自定义标题栏拖拽区域适配

交付件：

- `commands/window.rs`

验收标准：

- Windows/Linux 标题栏行为与现有版一致

### E3. 窗口状态持久化

- 窗口大小
- 窗口位置
- 最大化状态
- 不可见屏幕位置纠正

交付件：

- `storage/window_state.rs`

验收标准：

- 重启应用后窗口状态恢复正常

---

## F. 菜单与托盘迁移

### F1. 原生菜单

- App 菜单
- File
- Edit
- View
- Navigate
- Window
- Help

交付件：

- `menu/mod.rs`

验收标准：

- 主要快捷键与当前版本一致

### F2. 托盘菜单

- 显示主窗口
- 快捷跳转 Dashboard / Chat / Settings
- 检查更新
- 退出应用

交付件：

- `tray/mod.rs`

验收标准：

- macOS / Windows / Linux 托盘基本可用

### F3. 导航事件桥接

- 托盘与菜单点击后前端路由跳转

交付件：

- `events/navigation.rs`

验收标准：

- 不刷新页面即可跳转目标路由

---

## G. 设置与本地存储迁移

### G1. 普通设置存储

- theme
- language
- gatewayAutoStart
- proxy settings
- update settings
- sidebarCollapsed
- devModeUnlocked
- selectedBundles
- enabledSkills / disabledSkills

交付件：

- `storage/settings.rs`

验收标准：

- 设置读写行为与现有 UI 对齐

### G2. Provider 配置存储

- provider metadata
- default provider
- fallback models
- fallback provider ids

交付件：

- `storage/providers.rs`

验收标准：

- Provider 列表页行为一致

### G3. 敏感信息改造为系统密钥存储

- API key
- OAuth token
- 必要时的 device identity 私钥

交付件：

- `storage/secrets.rs`

验收标准：

- 不再将明文 API key 写入 JSON store

### G4. 旧数据迁移工具

- 读取旧 `electron-store`
- 导入 settings / providers / keys
- 将 keys 转写到 keyring
- 输出迁移结果与失败日志

交付件：

- 一次性迁移逻辑

验收标准：

- 旧用户升级后 Provider 不丢失

---

## H. Gateway 管理器迁移

### H1. 设计 Rust SidecarManager

- 启动 OpenClaw Gateway
- 维护进程句柄
- 停止/强制停止
- debounce restart
- reconnect backoff
- 生命周期状态机

交付件：

- `gateway/manager.rs`

验收标准：

- 能替代现有 `GatewayManager`

### H2. 子进程环境构造

- Gateway token
- Provider env
- Proxy env
- uv 相关 env
- OpenRouter header 注入策略
- Windows `windowsHide` 等效处理

交付件：

- `gateway/env.rs`

验收标准：

- Gateway 启动参数和环境变量完整可控

### H3. WebSocket JSON-RPC 客户端

- connect
- reconnect
- ping / health
- request timeout
- notification 分发
- pending request map

交付件：

- `gateway/rpc.rs`

验收标准：

- Chat、Cron、Status、Notifications 全部可跑通

### H4. Gateway 日志采集与分类

- stdout/stderr 捕获
- 错误级别分类
- ring buffer
- log file 持久化

交付件：

- `gateway/logging.rs`

验收标准：

- 设置页日志查看功能可用

---

## I. OpenClaw 资源与配置迁移

### I1. OpenClaw 路径与状态接口

- package status
- openclaw dir
- config dir
- skills dir
- cli command

交付件：

- `openclaw/paths.rs`

验收标准：

- Setup / Settings 页面所需信息完整可读

### I2. OpenClaw 配置同步

- 默认模型同步
- provider config 同步
- auth profiles 同步
- browser/gateway token 同步
- proxy config 同步

交付件：

- `openclaw/config_sync.rs`

验收标准：

- Provider 变更后 Gateway 能正确使用新配置

### I3. 内建技能与上下文部署

- built-in skills 安装
- bootstrap repair
- workspace context merge

交付件：

- `openclaw/bootstrap.rs`

验收标准：

- 首次启动和 Gateway 重启后上下文仍正确

---

## J. Provider / OAuth 迁移

### J1. Provider CRUD commands

- list
- get
- save
- updateWithKey
- delete
- setDefault
- validateKey

交付件：

- `providers/commands.rs`

验收标准：

- Setup 和 Settings 中 provider 管理完整可用

### J2. API Key 校验

- OpenAI compatible
- Google query key
- Anthropic header
- OpenRouter auth endpoint
- 可配置 baseUrl

交付件：

- `providers/validation.rs`

验收标准：

- 与当前验证逻辑结果保持一致

### J3. Device OAuth

方案优先级：

- 优先复用现有 OpenClaw Node 逻辑
- 通过 sidecar / node command 方式调用
- 不建议首阶段重写 OAuth 协议到 Rust

交付件：

- `oauth/device.rs`

验收标准：

- MiniMax / Qwen OAuth 流程可完成登录并写回配置

### J4. OAuth 事件桥接

- oauth:code
- oauth:success
- oauth:error

交付件：

- `events/oauth.rs`

验收标准：

- Setup 页面设备码交互行为一致

---

## K. Channel / Skills / Cron 迁移

### K1. Channel 配置命令

- save config
- get config
- get form values
- delete config
- list configured
- enable / disable
- validate
- validate credentials

交付件：

- `commands/channels.rs`

验收标准：

- Channels 页面功能与当前对齐

### K2. WhatsApp 登录桥接

- request QR
- cancel QR
- qr/success/error event

交付件：

- `commands/whatsapp.rs`

验收标准：

- 当前 WhatsApp 配网流程可用

### K3. ClawHub 交互

- search
- install
- uninstall
- list installed
- open readme

交付件：

- `commands/clawhub.rs`

验收标准：

- Skills 页面与市场功能可用

### K4. Cron 代理

- list
- create
- update
- delete
- toggle
- trigger

交付件：

- `commands/cron.rs`

验收标准：

- Cron 页面功能完整

---

## L. 文件与媒体能力迁移

### L1. 文件选择对话框

- 单文件
- 多文件
- 保存对话框
- message dialog

交付件：

- `commands/dialog.rs`

验收标准：

- Chat / Settings / Media 相关弹窗可用

### L2. 附件 staging

- 从磁盘路径复制到 outbound 目录
- 从 base64 buffer 写入 staged 文件
- 生成返回结构

交付件：

- `files/staging.rs`

验收标准：

- Chat 上传与粘贴文件可用

### L3. 缩略图与图片保存

- 图片预览生成
- 图片另存为
- 历史附件缩略图恢复

交付件：

- `files/media.rs`

验收标准：

- Chat 图片预览和保存功能可用

### L4. Session 删除

- 解析 sessionKey
- 修改 sessions.json
- JSONL 软删除

交付件：

- `files/session_cleanup.rs`

验收标准：

- 删除会话后 UI 与磁盘状态一致

---

## M. 日志与诊断能力

### M1. 日志读取接口

- recent logs
- read file tail
- log file path
- log dir
- list log files

交付件：

- `commands/logs.rs`

验收标准：

- 设置页日志查看可用

### M2. 诊断信息补齐

- gateway status
- gateway health
- openclaw status
- version/platform/path

交付件：

- `commands/diagnostics.rs`

验收标准：

- Dashboard / Setup / Settings 所需诊断信息齐全

---

## N. 更新系统与发布系统迁移

### N1. 设计 Tauri Updater 策略

- 稳定版 / 测试版渠道
- 清晰的 feed URL 约定
- 公钥签名校验
- 自动下载 / 手动安装策略

交付件：

- 更新设计说明

验收标准：

- 覆盖当前 `stable/beta/dev` 的主要诉求

### N2. 实现更新 commands 和 events

- status
- version
- check
- download
- install
- set channel
- set auto download
- auto install countdown

交付件：

- `updater/mod.rs`

验收标准：

- `src/stores/update.ts` 可切换到 Tauri 实现

### N3. 迁移打包资源布局

- OpenClaw runtime
- openclaw plugins
- platform bins
- CLI resources
- icons

交付件：

- Tauri bundle resource 配置

验收标准：

- 打包产物启动后资源路径正确

### N4. CI/CD 改造

- macOS build
- Windows build
- Linux build
- 签名
- 发布
- updater metadata 发布

交付件：

- GitHub Actions / 发布脚本

验收标准：

- 三平台构建与发布链跑通

---

## O. Control UI 策略

### O1. 第一阶段能力约束

- 仅支持外部浏览器打开 OpenClaw Control UI
- 保留 token 注入 URL 的逻辑

交付件：

- 需求确认记录

验收标准：

- 设置页、Dashboard、Sidebar 可正常打开 Control UI

### O2. 可选第二阶段增强

- 如必须内嵌，则设计本地反向代理
- 对 CSP / X-Frame-Options 做安全可控改写

交付件：

- 可选增强设计稿

验收标准：

- 不作为首发阻塞项

---

## P. 测试与验证

### P1. 自动化测试补强

- `desktopApi` 单测
- 前端 store 行为测试
- Rust command 单测
- Gateway manager 集成测试

交付件：

- 新增测试用例

验收标准：

- 核心路径有自动化覆盖

### P2. 端到端回归

覆盖场景：

- 首次启动
- Setup provider 配置
- OAuth 登录
- Gateway 启停与自动重连
- Chat 发送文本与附件
- Skills 安装/卸载
- Cron 创建/执行
- Channels 配置
- 日志查看
- 更新检查
- 托盘和菜单

交付件：

- 回归测试清单

验收标准：

- 三平台主路径全部通过

### P3. 升级迁移验证

- 从 Electron 旧版本升级到 Tauri 版本
- settings 保留
- providers 保留
- keys 成功转移到 keyring
- Gateway 可继续工作

交付件：

- 升级验证报告

验收标准：

- 真实升级路径可用

---

## Q. 下线 Electron

### Q1. 删除 Electron 构建链

- 删除 `electron/`
- 删除 `vite-plugin-electron`
- 删除 `electron-builder`
- 删除 `electron-updater`
- 删除 Electron 相关脚本

交付件：

- 清理 PR

验收标准：

- 项目依赖树中不再包含 Electron 主链路

### Q2. 清理遗留兼容代码

- 删除 Electron adapter
- 删除旧 preload 类型定义
- 清理旧配置与文档

交付件：

- 最终清理 PR

验收标准：

- 仓库只保留 Tauri 方案

---

## 7. 任务优先级建议

## P0

- A1
- A2
- B1
- B2
- B3
- C1
- D1
- D2

## P1

- E1
- E2
- G1
- G2
- H1
- H2
- H3
- I1
- J1
- K4
- L1
- L2
- M1

## P2

- F1
- F2
- G3
- G4
- I2
- I3
- J2
- J3
- J4
- K1
- K2
- K3
- L3
- L4
- M2
- N1
- N2
- N3

## P3

- O2
- P1
- P2
- P3
- Q1
- Q2

---

## 8. 风险清单

## R1. Gateway sidecar 跨平台行为差异

风险：

- Windows/macOS/Linux 对子进程、路径、环境变量、隐藏窗口处理不同

缓解：

- 优先做跨平台最小样例
- 尽早验证 OpenClaw sidecar 启动

## R2. OAuth 迁移复杂度高

风险：

- 现有逻辑直接复用了 OpenClaw Node 扩展

缓解：

- 首阶段继续复用 Node 逻辑，不重写协议

## R3. 自动更新链切换风险

风险：

- Electron updater 与 Tauri updater 的产物结构、签名模型不同

缓解：

- 单独建里程碑
- 先完成手动安装包，再接 updater

## R4. 旧用户数据兼容

风险：

- Provider、密钥、默认模型可能丢失

缓解：

- 必须做迁移工具
- 增加升级路径测试

## R5. Control UI 内嵌兼容性

风险：

- Tauri WebView 对 frame/csp 的控制方式不同于 Electron

缓解：

- 首版不以内嵌为目标

---

## 9. 建议执行顺序

1. 先做前端接口收口，不直接碰业务逻辑。
2. 再起 Tauri 壳，验证窗口、命令、事件、托盘。
3. 最先迁移 Gateway 管理与 settings，因为这是主链路。
4. 之后迁移 provider、files、logs、cron。
5. 最后处理 updater、数据迁移、Electron 下线。

---

## 10. 完成定义

满足以下条件后，视为 Tauri 迁移完成：

- Electron 运行时已从主链路移除
- React 前端全部通过 `desktopApi` 调用桌面能力
- 三平台均可打包、安装、启动、更新
- Gateway、Chat、Provider、Skills、Channels、Cron、Settings 主路径可用
- 旧用户升级后关键数据保留
- 密钥不再明文存储
- 核心回归测试通过
