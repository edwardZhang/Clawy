<p align="center">
  <img src="src/assets/logo.svg" width="128" height="128" alt="Clawy Logo" />
</p>

<h1 align="center">Clawy</h1>

<p align="center">
  <strong>面向 OpenClaw 的轻量级 Tauri 桌面工作台</strong>
</p>

<p align="center">
  <a href="#功能特性">功能特性</a> •
  <a href="#为什么选择-clawy">为什么选择 Clawy</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#架构">架构</a> •
  <a href="#开发">开发</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platform" />
  <img src="https://img.shields.io/badge/tauri-2+-24C8DB?logo=tauri" alt="Tauri" />
  <img src="https://img.shields.io/badge/react-19-61DAFB?logo=react" alt="React" />
  <img src="https://img.shields.io/github/downloads/edwardZhang/Clawy/total?color=%23027DEB" alt="Downloads" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
</p>

<p align="center">
  <a href="README.md">English</a> | 简体中文 | <a href="README.ja-JP.md">日本語</a>
</p>

---

## 概述

**Clawy** 是一个基于 **Tauri** 重构的 OpenClaw 桌面应用。它将 OpenClaw 的运行时、技能、模型提供商、频道和自动化能力整合为一个更轻、更快、更适合日常使用的桌面工作台。

相较于传统的大而全桌面壳，Clawy 更强调“轻量桌面层 + 受管运行时”：

- 安装包更轻，桌面壳开销更小
- 启动更快，日常交互更流畅
- 运行时依赖可以由应用自行检测并准备
- OpenClaw 可以与桌面壳解耦，独立演进和更新

## 为什么选择 Clawy

Clawy 保留了 OpenClaw 工作流的核心能力，同时把桌面层做得更精简。

| 问题 | Clawy 的方案 |
|---|---|
| 桌面包体过大 | 使用轻量级 Tauri 壳层，降低桌面框架额外体积 |
| 冷启动偏慢 | 采用原生窗口 + WebView 架构，提高启动响应速度 |
| 用户不应手工准备运行环境 | 启动后自动检查 Node.js；如果缺失，可由应用自动下载和准备 |
| OpenClaw 不应绑定每次桌面发版 | OpenClaw 运行时可由应用独立准备并支持独立更新 |
| OpenClaw 工作流偏 CLI | 通过桌面 UI 管理对话、技能、频道、计划任务和运行状态 |

## 功能特性

### 轻量包体，启动更快

Clawy 基于 **Tauri** 构建，桌面框架本身更加轻量，安装包额外负担更小，应用启动体验也更干净利落。当前 Lite 包体控制在较小规模：

- Windows 安装包约 **5.5 MB**
- macOS 安装包约 **9.9 MB**

### 自动检测并准备 Node.js

Clawy 启动后可以检查系统是否已经具备所需的 Node.js 运行环境；如果没有，应用可自动下载并准备运行时，而不是要求用户先手工安装。

### OpenClaw 服务自动准备

Clawy 的设计目标是由应用自身完成 OpenClaw 服务的准备和接入，让用户专注于使用，而不是处理运行时部署细节。

### OpenClaw 可独立更新

OpenClaw 作为受管运行时，可与桌面壳解耦更新。这意味着桌面体验迭代和 OpenClaw 服务升级可以分开推进，整体维护成本更低。

### 内置 OpenClaw 版本检测与一键更新

Clawy 内置 OpenClaw 版本检测能力，可检查当前运行版本、可用更新和推荐版本，并支持在桌面端一键更新到最新版本，降低手工维护成本。

### 多频道接入能力

Clawy 已集成多种频道接入能力，适合把 OpenClaw 工作流扩展到更多协作和消息场景，目前已覆盖：

- 飞书
- QQ
- 企业微信
- 个人微信（测试）
- 钉钉
- Matrix
- Discord
- Telegram
- WhatsApp

### 模型接入与认证能力

Clawy 支持主流模型 API 接入，也兼顾官方认证流和第三方兼容协议：

- 支持 `chat-completions` 与 `openai-responses` 两类第三方 API 协议
- 支持 Codex、Claude Code、MiniMax、Qwen 等模型服务的 OAuth 认证
- 支持自定义模型提供商配置，便于接入代理网关、兼容平台和私有部署接口

### Token 消耗实时查看

Clawy 提供 Token 消耗实时查看能力，可直观了解输入、输出和总消耗情况，便于快速评估模型成本和会话使用强度。

### 提炼 OpenClaw 核心配置，降低使用门槛

Clawy 对 OpenClaw 常用配置做了桌面化提炼，保留高频操作入口，减少直接编辑底层配置文件的负担。普通用户可以通过图形界面完成常见配置，开发者则可在开启开发者模式后进入 OpenClaw 控制台，进一步查看运行状态、日志与高级配置。

### 完整的 OpenClaw 桌面工作流

Clawy 提供以下桌面能力：

- 对话与会话历史
- AI 提供商配置
- 技能浏览、安装与管理
- 频道配置与状态管理
- 定时任务管理
- 运行时状态、日志与诊断

## 快速开始

### 系统要求

- macOS 11+、Windows 10+ 或 Linux
- 至少 4 GB 内存
- 需要联网以完成运行时准备与更新

### 安装

从 [Releases](https://github.com/edwardZhang/Clawy/releases) 页面下载最新版本。

### 首次启动

首次启动时，Clawy 会引导用户完成初始化和运行时检查。典型流程如下：

1. 选择语言与基础偏好
2. 检查所需运行环境是否存在
3. 若缺失则自动准备 Node.js
4. 自动准备或下载 OpenClaw 服务运行时
5. 配置 AI 提供商
6. 进入主工作台

### 代理支持

Clawy 内置代理设置，适用于桌面运行时、OpenClaw Gateway 以及外部频道需要通过本地代理客户端访问网络的场景。

支持配置：

- 代理服务器
- 绕过规则
- HTTP 代理
- HTTPS 代理
- ALL_PROXY / SOCKS

保存代理设置后，桌面运行时会立即重新应用代理，并自动重启 Gateway。

## 架构

Clawy 采用“桌面壳 + 受管运行时”的结构：

```text
Clawy Desktop Shell (Tauri)
  -> 窗口生命周期、托盘、更新、系统集成
  -> 运行时检查与引导
  -> 本地设置、日志、诊断

Managed Runtime Layer
  -> Node.js 运行时检测 / 下载
  -> OpenClaw 运行时准备
  -> 运行时版本管理

OpenClaw Runtime
  -> gateway
  -> skills / plugins
  -> channels
  -> provider / agent orchestration
```

这种分层让 UI 保持轻快，也让 OpenClaw 运行时可以独立迭代。

## 开发

### 前置要求

- Node.js 22+
- pnpm 10+
- 用于构建 Tauri 的 Rust 工具链

### 常用命令

```bash
pnpm run init
pnpm dev
pnpm run lint
pnpm run typecheck
pnpm test
pnpm run build:tauri
```

### 从源码运行

```bash
git clone https://github.com/edwardZhang/Clawy.git
cd Clawy
pnpm run init
pnpm dev
```

## 致谢

Clawy 构建于这些优秀的开源项目之上：

- [OpenClaw](https://github.com/OpenClaw)
- [Tauri](https://tauri.app/)
- [React](https://react.dev/)
- [Zustand](https://github.com/pmndrs/zustand)

Clawy 也是一个参考 ClawX 并使用 Tauri 重构的项目。感谢 ClawX 项目开发者为产品方向与前期基础工作所做的贡献。

## 许可证

Clawy 基于 [MIT License](LICENSE) 发布。
