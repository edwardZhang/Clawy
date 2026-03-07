# ClawX Tauri Lite Runtime 方案设计

## 1. 文档目的

本文档定义 ClawX 在 Tauri 架构下的 `Lite` 发行方案。

目标是：

- 发布一个体积更小的桌面安装包
- 首次启动时由 ClawX 自行下载并安装 `OpenClaw`、`Node`、`uv` 等运行时
- 保持 `OpenClaw` 运行配置目录继续使用 `~/.openclaw`
- 不对 `OpenClaw` 本体做任何定制修改
- 让后续 `OpenClaw` / `Node` / `uv` 更新从“整包更新”解耦出来

本文档同时明确该方案与当前“完整自包含包”方案的边界，以及后续研发任务拆分。

---

## 2. 核心结论

### 2.1 方案结论

ClawX 后续主交付建议切换为：

- 主线：`Lite` 小包 + 外部 runtime 下载与管理
- 备线：`Full` 完整包，作为离线分发或内部调试版本

### 2.2 不变约束

- `OpenClaw` 配置目录继续使用 `~/.openclaw`
- `OpenClaw` 不做源码修改、不做目录约定修改
- `ClawX` 只负责下载、校验、解压、切换、启动 runtime

### 2.3 关键取舍

- `Lite` 方案不再追求“安装包内自带全部运行时”
- `Lite` 方案追求“小包 + 首次自动安装 + 后续独立更新 runtime”
- 用户不需要手工安装 `Node`、`OpenClaw`、`uv`
- 用户首次启动时允许发生下载

---

## 3. 方案范围

### 3.1 范围内

- Tauri 桌面壳
- runtime 检测
- runtime 下载
- runtime 完整性校验
- runtime 解压与安装
- runtime 版本切换
- Gateway 重启与重新接管
- runtime 独立更新
- 失败回滚

### 3.2 范围外

- 修改 `OpenClaw` 默认配置目录
- 修改 `OpenClaw` 对 `~/.openclaw` 的读写行为
- 把下载后的 runtime 写回 `.app` / 安装目录
- 在本阶段重做前端 UI 结构

---

## 4. 目标用户体验

### 4.1 首次安装

用户下载并安装一个较小的 ClawX 安装包。

首次启动时：

1. ClawX 启动引导页
2. 引导页检测本机是否已有可用 runtime
3. 如果缺失，则自动下载运行时包
4. 下载完成后自动解压、校验、安装
5. Gateway 启动
6. 用户继续完成 Provider 配置并进入主界面

### 4.2 后续使用

- 之后再次启动时，不再重复下载
- 如果 runtime 已存在且可用，直接启动 Gateway
- 如果 runtime 可更新，ClawX 在后台检查并执行下载/切换

### 4.3 用户感知

用户感知应为：

- 安装后直接打开即可
- 如果缺依赖，ClawX 自己补齐
- 不需要手动安装任何开发环境

---

## 5. 运行时职责划分

### 5.1 ClawX 桌面壳负责

- 前端 UI
- 本地设置与引导状态
- runtime 检测
- runtime 下载与校验
- runtime 切换
- Gateway 进程编排
- 更新提示与失败恢复

### 5.2 OpenClaw 负责

- Gateway 服务
- Agent 运行
- Provider / Skill / Channel / Chat 等业务能力
- `~/.openclaw` 配置和数据读写

### 5.3 运行时资产定义

本文档中的 runtime 指：

- `OpenClaw` 主包
- `OpenClaw Plugins`
- `Node`
- `uv`
- `clawhub`

不包括：

- `~/.openclaw`
- ClawX 自身前端代码
- ClawX 自身设置

---

## 6. 目录设计

### 6.1 配置与数据目录

保持不变：

- `OpenClaw` 配置目录：`~/.openclaw`
- ClawX 本地状态目录：`~/.clawx-tauri`

### 6.2 新增 runtime 目录

建议新增：

```text
~/.clawx-tauri/runtime/
  manifests/
    current.json
    installed/
      runtime-2026.3.1.json
  downloads/
    runtime-2026.3.1-darwin-arm64.tar.zst
  staging/
    runtime-2026.3.1/
  versions/
    runtime-2026.3.1/
      build/openclaw/...
      build/openclaw-plugins/...
      build/clawhub/...
      resources/bin/darwin-arm64/node
      resources/bin/darwin-arm64/uv
  current -> versions/runtime-2026.3.1
```

说明：

- `versions/` 保存已安装的运行时版本
- `current` 指向当前生效版本
- `staging/` 用于解压与校验
- `downloads/` 存放下载缓存
- `manifests/` 记录已安装版本和当前版本元数据

### 6.3 目录设计原则

- `current` 目录结构与当前包内 resources 结构尽量同构
- 这样可以最小化现有路径解析代码改造成本
- 不把 runtime 写进 `.app`、`Program Files` 或安装目录

---

## 7. 运行时查找顺序

### 7.1 Lite 模式建议顺序

#### OpenClaw

1. `~/.clawx-tauri/runtime/current/build/openclaw`
2. 如果不存在，视为未安装

#### Node

1. `~/.clawx-tauri/runtime/current/resources/bin/<platform-arch>/node`
2. 如果不存在，视为未安装

#### uv

1. `~/.clawx-tauri/runtime/current/resources/bin/<platform-arch>/uv`
2. 如果不存在，视为未安装

### 7.2 Full 模式建议顺序

#### OpenClaw

1. `.app` / 安装包内 resources
2. 如果不存在，报错

#### Node / uv

1. `.app` / 安装包内 resources
2. 如果不存在，报错

### 7.3 结论

`Lite` 与 `Full` 需要共用一套 resolver，但使用不同优先级策略。

---

## 8. Runtime 包形态

### 8.1 推荐策略

建议不要拆成多个独立散件下载，而是把 runtime 作为一个版本化总包下载。

例如：

```text
runtime-2026.3.1-darwin-arm64.tar.zst
runtime-2026.3.1-win32-x64.zip
runtime-2026.3.1-linux-x64.tar.zst
```

### 8.2 总包内容

```text
build/openclaw/
build/openclaw-plugins/
build/clawhub/
resources/bin/<platform-arch>/node
resources/bin/<platform-arch>/uv
manifest.json
```

### 8.3 为什么用总包

- 避免 `Node` / `OpenClaw` / `uv` 版本矩阵爆炸
- 降低引导逻辑复杂度
- 降低下载和切换失败点
- 保持与当前 `Full` 包的资源布局接近

---

## 9. Runtime Manifest 设计

### 9.1 远端发布 manifest

建议使用：

```json
{
  "version": "2026.3.1",
  "channel": "stable",
  "platform": "darwin",
  "arch": "arm64",
  "artifacts": {
    "runtime": {
      "url": "https://example.com/runtime-2026.3.1-darwin-arm64.tar.zst",
      "sha256": "..."
    }
  },
  "components": {
    "openclaw": "2026.3.1",
    "node": "22.19.0",
    "uv": "0.10.0",
    "clawhub": "x.y.z"
  }
}
```

### 9.2 本地 current manifest

建议使用：

```json
{
  "version": "2026.3.1",
  "channel": "stable",
  "installedAt": "2026-03-07T12:00:00Z",
  "path": "/Users/mi/.clawx-tauri/runtime/versions/runtime-2026.3.1",
  "components": {
    "openclaw": "2026.3.1",
    "node": "22.19.0",
    "uv": "0.10.0",
    "clawhub": "x.y.z"
  }
}
```

### 9.3 校验要求

- 下载后必须校验 SHA256
- 校验失败立即删除 staging 目录和下载缓存
- 不允许未校验完成的 runtime 被切换为 current

---

## 10. 引导流程设计

### 10.1 环境检查阶段

引导页当前已有环境检查步骤，后续改为真实检测：

- `node`
- `uv`
- `openclaw`
- `gateway`
- `python`（可选，放到安装阶段也可）

检测结果分为：

- `ready`
- `missing`
- `installing`
- `failed`

### 10.2 安装阶段

当 runtime 缺失时：

1. 获取远端 runtime manifest
2. 选择平台匹配的 runtime 包
3. 下载到 `downloads/`
4. 校验 hash
5. 解压到 `staging/`
6. 校验目录结构是否完整
7. 原子移动到 `versions/`
8. 更新 `current`
9. 重新执行 runtime check
10. 启动 Gateway

### 10.3 安装完成后的接管

下载完成后无需重启整个 ClawX 进程。

只需：

1. 让 resolver 指向新 `current`
2. 启动或重启 Gateway
3. 前端重新检查 Gateway 健康状态

---

## 11. Gateway 接管流程

### 11.1 启动前准备

每次启动 Gateway 前：

- 校验 current runtime 是否完整
- 同步 `gateway token` 到 `~/.openclaw/openclaw.json`
- 校验 `node`、`openclaw.mjs`、`uv` 路径可用

### 11.2 启动流程

1. 解析 current runtime
2. 解析 `node`
3. 解析 `openclaw/openclaw.mjs`
4. 启动 `openclaw gateway`
5. 日志采集
6. 桥接层握手
7. 自动配对
8. 标记 Gateway ready

### 11.3 切换 runtime 时的行为

- 停止旧 Gateway
- 切换 `current`
- 启动新 Gateway
- 恢复前端连接

---

## 12. 更新策略

### 12.1 App 更新与 Runtime 更新解耦

建议拆成两个更新源：

- `ClawX App Update`
- `Runtime Update`

### 12.2 更新优先级

#### App 更新

用于：

- 前端功能变更
- Tauri / Rust 逻辑修复
- 桌面壳能力更新

#### Runtime 更新

用于：

- `OpenClaw` 升级
- `Node` 升级
- `uv` 升级
- `clawhub` 升级

### 12.3 更新执行方式

#### App 更新

- 继续用当前桌面端更新链路

#### Runtime 更新

- 下载新 runtime 包
- 解压到新版本目录
- 原子切换 `current`
- 重启 Gateway

### 12.4 回滚策略

如果新 runtime 启动失败：

1. 保留旧 `current`
2. 将新版本标记为 failed
3. 自动回退到上一个已知可用版本
4. 向前端报告恢复成功或失败

---

## 13. 平台差异

### 13.1 macOS

- 不向 `.app` 写入 runtime
- runtime 必须放在用户可写目录

### 13.2 Windows

- 技术上可更新安装目录文件
- 但为统一实现，仍建议使用外部 runtime 目录

### 13.3 Linux

- 仍建议使用外部 runtime 目录
- 避免不同安装方式带来的写权限差异

### 13.4 跨平台统一原则

统一走外部 runtime 目录，避免平台分叉逻辑。

---

## 14. 安全与完整性

### 14.1 完整性保护

- runtime 包必须带 hash
- 下载完成必须做 SHA256 校验
- staging 校验失败立即清理

### 14.2 原子切换

- 不直接覆盖正在使用的 current
- 使用 `staging -> versions/<ver> -> current` 三段式切换

### 14.3 敏感信息边界

- `~/.openclaw` 继续保留业务配置和会话数据
- ClawX 自己的更新状态和 runtime 索引保存在 `~/.clawx-tauri`

### 14.4 不做的事

- 不修改 `OpenClaw` 配置约定
- 不给 `OpenClaw` 打补丁
- 不改写安装包本体

---

## 15. 对现有代码的改造点

### 15.1 Rust Runtime Resolver

需要改造：

- `get_openclaw_dir`
- `get_openclaw_entry_path`
- `bundled_binary_path`
- `resolve_node_binary`
- `resolve_uv_binary`

目标：

- 支持 `runtime/current` 目录优先
- 支持 `Lite` / `Full` 两种 flavor

### 15.2 Setup 页面

需要改造：

- 当前环境检查里的“Node.js 永远成功”逻辑
- 当前安装阶段只执行 `uv python install`

目标：

- 改成真实 runtime 检测
- 支持 runtime 下载与安装进度

### 15.3 Updater

需要新增：

- runtime manifest 拉取
- runtime 下载器
- runtime 校验器
- runtime 安装器
- runtime 回滚器

### 15.4 Gateway 生命周期

需要保证：

- 切换 runtime 后可无缝重新启动
- 错误时能回滚并上报 UI

---

## 16. 推荐实施顺序

### M1. Runtime Resolver 抽象

- 抽取统一 runtime resolver
- 支持 `Full` 继续正常运行
- 引入 `Lite` 目录结构但先不下载

### M2. 引导页真实检测

- 新增 `runtime:check`
- 让 Setup 页显示真实缺失状态

### M3. Runtime 下载器

- manifest 拉取
- 下载
- 校验
- 解压
- 切换 current

### M4. Gateway 接管

- 安装完成后自动启 Gateway
- 失败回滚

### M5. Runtime 更新

- 启动时检查更新
- 设置页手动检查更新
- 后台下载与切换

---

## 17. 研发任务拆分

### A. 架构改造

- 新增 `RuntimeFlavor` 概念：`full` / `lite`
- 新增 runtime resolver 模块
- 新增 runtime metadata 模块

### B. 本地目录管理

- 创建 runtime 根目录
- 创建 current / versions / staging / downloads / manifests
- 实现 current 指针切换

### C. 下载与完整性

- 定义远端 manifest 结构
- 实现下载器
- 实现 SHA256 校验
- 实现解压器
- 实现 staging 校验

### D. 前端引导

- 增加 runtime 检测状态
- 增加 runtime 下载进度展示
- 增加失败重试与查看日志

### E. Gateway 接管

- 切换到新 runtime 后重启 Gateway
- 保持配对与 token 同步逻辑可复用

### F. 更新系统

- runtime 版本检查
- runtime 更新下载
- runtime 原子切换
- runtime 回滚

### G. 发布链

- 新增 `Lite` 构建 flavor
- 生成 runtime 包与 manifest
- 发布 Lite runtime CDN 产物

---

## 18. 风险清单

### 风险 1：首次下载失败

应对：

- 重试
- 镜像源
- 清晰错误提示

### 风险 2：runtime 包损坏

应对：

- SHA256 校验
- staging 校验
- 禁止未校验通过的 runtime 切换为 current

### 风险 3：新 runtime 无法启动 Gateway

应对：

- 启动探针
- 回滚到旧 runtime

### 风险 4：平台打包路径差异

应对：

- resolver 统一抽象
- 对外只暴露 runtime root，不在业务层散落平台路径判断

### 风险 5：Full / Lite 双模式长期分叉

应对：

- 共用同一套 resolver
- 仅 runtime 来源不同
- 不分叉业务逻辑

---

## 19. 最终建议

建议将 `Lite` 方案作为主发行方向，原因是：

- 安装包显著变小
- OpenClaw / Node / uv 更新可独立进行
- 构建与发布成本更低
- 后续迭代更灵活

同时保留 `Full` 方案作为次级发行形态，用于：

- 离线安装
- 内部调试
- 特殊环境交付

最终产品策略建议为：

- 默认对外发布：`ClawX Lite`
- 备用对外发布：`ClawX Full`

---

## 20. 落地结论

基于当前项目状态，推荐下一步直接进入：

1. 抽 runtime resolver
2. 建立 `Lite` runtime 目录结构
3. 将 Setup 环境检查改为真实 runtime 检测
4. 实现 runtime manifest + 下载 + 切换链路

这条路线不修改 OpenClaw，且与当前 Tauri 迁移结果兼容。
