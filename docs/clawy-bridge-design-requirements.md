# Clawy Bridge 设计需求文档

## 1. 文档目的

本文档用于定义 Clawy 在 ForgeAI 体系中的桥接职责、能力边界、通信分层和最小实现范围。

目标不是把 Clawy 扩展成新的 Agent Runtime，也不是让 Clawy 复制 OpenClaw 的底层执行能力。

目标是将 Clawy 定义为：

> 部署在每台开发机上的本地桥接层，用于将 ForgeAI 与本机 OpenClaw 安全、稳定、标准化地连接起来。

本文档作为后续 API 设计、实现拆解和联调的基础文档。

## 2. 系统定位

### 2.1 三层职责

整个体系分为三层：

#### OpenClaw

- 身份：底层执行层 / Agent Runtime
- 职责：
  - 会话运行
  - 模型调用
  - 工具调用
  - Skill 执行
  - 事件流输出
  - 最终响应生成

#### Clawy

- 身份：本机桥接层 / Local Control Proxy
- 职责：
  - 接收 ForgeAI 控制请求
  - 转发请求到本机 OpenClaw
  - 接收并标准化 OpenClaw 事件
  - 向 ForgeAI 暴露稳定统一的本地 API
  - 管理本机 OpenClaw 接入状态
  - 维持本机安全边界

#### ForgeAI

- 身份：总控层 / Orchestration UI
- 职责：
  - 统一管理多个 Clawy 节点
  - 汇总节点状态、会话、事件和结果
  - 作为总控台、调度台、状态台

### 2.2 设计原则

#### 单一职责

Clawy 只做桥接，不做执行。

#### 不复制 OpenClaw 能力

Clawy 不重新实现：

- session runtime
- model orchestration
- tool loop
- workflow execution
- skill execution
- event production

#### 标准接口优先

ForgeAI 必须通过 Clawy 暴露的标准接口访问 OpenClaw，不直接依赖 OpenClaw dashboard、内部 RPC 细节或桌面 UI 私有状态。

#### API 与桌面 UI 解耦

Clawy 需要明确分为两层：

- 桌面 UI
- 本地 Bridge/API 层

ForgeAI 只依赖 Bridge/API 层。

## 3. 目标与非目标

### 3.1 目标

Clawy Bridge 必须支持以下核心能力：

- 节点身份暴露
- 节点健康状态暴露
- OpenClaw 会话桥接
- OpenClaw 流式事件桥接
- 运行时状态和能力元信息暴露
- 基础鉴权和审计能力
- 稳定、可版本化的 API 契约

### 3.2 非目标

以下能力明确不属于 Clawy：

- 底层任务执行
- Skill 实际执行
- Tool 实际执行
- 多 Agent 编排
- 多节点调度策略
- 替代 OpenClaw Runtime

## 4. 当前能力基础

根据当前 Clawy 代码结构，已经存在以下可复用基础：

- 本机 OpenClaw runtime 管理
- 本机 Gateway 启停和状态监控
- 会话相关调用路径
- 聊天事件接收和前端状态机
- Provider、Skill、Channel、Gateway 配置同步
- OpenClaw CLI 调用能力

这意味着 Bridge 设计不需要重建底层接入链路，而是需要在现有本地能力之上，抽象出稳定的 ForgeAI 接口层。

## 5. 通信分层

Clawy 对 ForgeAI 采用两层通信模型：

- HTTP：控制面
- WebSocket：事件面

### 5.1 HTTP 作为控制面

用于：

- 查询
- 创建/发送
- 中止
- 健康检查
- 运行时状态读取
- 能力元信息读取

选择 HTTP 的原因：

- request / response 语义明确
- 易于鉴权
- 易于调试和文档化
- 适合 P0 快速落地

### 5.2 WebSocket 作为事件面

用于：

- 流式文本
- 工具调用过程
- 运行中状态变化
- 异步错误
- 会话状态变化

选择 WebSocket 的原因：

- 适合持续推送
- 适合总控台实时渲染
- 适合承载单节点或多节点事件流

## 6. API 边界定义

需要明确区分两套接口：

### 6.1 Clawy Northbound API

即 ForgeAI 调 Clawy 的接口。本文档的 API 设计范围只覆盖这一层。

### 6.2 ForgeAI Registry API

即 Clawy 主动调 ForgeAI 的注册与心跳接口。

这意味着：

- `GET /api/node/info` 等属于 Clawy 暴露给 ForgeAI 的接口
- `POST /api/node/register`
- `POST /api/node/heartbeat`

不应作为 Clawy northbound API 暴露给 ForgeAI，而应作为 ForgeAI 侧服务端接口另行定义。

如果后续需要保留本地注册调试能力，可在 Clawy 内部实现独立 client，不纳入对外 northbound API 契约。

## 7. Northbound HTTP API

本节定义 ForgeAI 调 Clawy 的 HTTP 接口。

### 7.1 节点接口

#### `GET /api/node/info`

用途：

- 获取节点静态身份信息

建议返回：

```json
{
  "node_id": "node-macstudio-01",
  "node_name": "macstudio-01",
  "machine_name": "Mac-Studio",
  "version": "0.3.11",
  "os": "macos",
  "arch": "arm64"
}
```

#### `GET /api/node/health`

用途：

- 获取节点当前健康状态

建议返回：

```json
{
  "online": true,
  "openclaw_reachable": true,
  "gateway_running": true,
  "last_error": null,
  "last_seen_at": "2026-03-20T10:22:00Z"
}
```

### 7.2 会话接口

#### `GET /api/sessions`

用途：

- 获取当前可见 session 列表

#### `GET /api/sessions/:id`

用途：

- 获取单个 session 摘要信息

摘要建议包含：

- session_id
- display_name
- current_state
- last_activity_at
- thinking_level
- model

#### `GET /api/sessions/:id/history`

用途：

- 获取历史消息

建议支持基础查询参数：

- `limit`
- `before`
- `after`

#### `POST /api/sessions/:id/send`

用途：

- 向指定 session 发送消息

建议请求体：

```json
{
  "message": "帮我检查这个项目状态",
  "attachments": []
}
```

建议返回语义为“已受理”，而非同步完成：

```json
{
  "ok": true,
  "accepted": true,
  "session_id": "sess_123",
  "run_id": "run_456"
}
```

说明：

- 最终输出走 WebSocket 事件流返回
- `run_id` 用于 ForgeAI 将一次请求和后续事件绑定

#### `POST /api/sessions/:id/abort`

用途：

- 中止当前运行

建议返回：

```json
{
  "ok": true,
  "session_id": "sess_123",
  "aborted": true
}
```

### 7.3 运行时接口

#### `GET /api/runtime/status`

用途：

- 获取当前 OpenClaw 运行时状态

建议返回字段：

- `gateway_running`
- `gateway_port`
- `gateway_reachable`
- `dashboard_reachable`
- `runtime_ready`
- `current_model`
- `current_provider`
- `config_dir`
- `openclaw_dir`
- `last_error`

#### `GET /api/runtime/capabilities`

用途：

- 获取节点能力元信息

建议返回字段：

- `agent_identity`
- `loaded_skills`
- `enabled_tools`
- `workspace_info`
- `channel_bindings`
- `runtime_source`
- `model_info`

## 8. WebSocket 事件接口

### 8.1 连接地址

事件面统一使用：

- `WS /api/events`

ForgeAI 建立连接后，持续接收来自 Clawy 的事件。

### 8.2 统一事件结构

所有事件统一规范为：

```json
{
  "event_id": "evt_001",
  "node_id": "node-macstudio-01",
  "session_id": "sess_123",
  "type": "message.delta",
  "ts": "2026-03-20T02:22:00Z",
  "payload": {}
}
```

字段定义：

- `event_id`
  全局唯一事件 ID
- `node_id`
  当前节点 ID
- `session_id`
  对应会话 ID
- `type`
  事件类型
- `ts`
  事件时间戳
- `payload`
  标准化后的事件内容

### 8.3 标准事件类型

建议至少支持以下事件类型：

#### `message.delta`

流式文本增量：

```json
{
  "type": "message.delta",
  "payload": {
    "text": "正在分析..."
  }
}
```

#### `message.final`

最终消息：

```json
{
  "type": "message.final",
  "payload": {
    "text": "分析完成",
    "message_id": "msg_001"
  }
}
```

#### `message.thinking`

处理中或思考态：

```json
{
  "type": "message.thinking",
  "payload": {
    "status": "thinking"
  }
}
```

#### `tool.call`

工具调用开始：

```json
{
  "type": "tool.call",
  "payload": {
    "tool": "read",
    "summary": "读取项目配置"
  }
}
```

#### `tool.result`

工具调用结果摘要：

```json
{
  "type": "tool.result",
  "payload": {
    "tool": "read",
    "ok": true,
    "summary": "已读取 3 个文件"
  }
}
```

#### `runtime.status`

运行时状态变化：

```json
{
  "type": "runtime.status",
  "payload": {
    "status": "busy"
  }
}
```

#### `runtime.error`

运行时错误：

```json
{
  "type": "runtime.error",
  "payload": {
    "code": "OPENCLAW_UNREACHABLE",
    "message": "gateway not reachable"
  }
}
```

#### `session.updated`

会话状态变化：

```json
{
  "type": "session.updated",
  "payload": {
    "session_id": "sess_123",
    "state": "completed"
  }
}
```

### 8.4 WebSocket 附加要求

为保证稳定性，建议在第一版接口中预留以下能力：

- `last_event_id`
  用于断线重连和事件续传
- `subscription filter`
  支持按 `session_id` 或 `run_id` 过滤订阅
- `heartbeat / ping`
  用于连接保活和快速发现断链

如果 P0 时间有限，可先保留字段与接口扩展位，具体重放能力在 P1 落地。

## 9. 鉴权要求

Clawy 对 ForgeAI 暴露的 northbound API 必须独立鉴权，不直接复用 OpenClaw 的内部认证。

### 9.1 P0 最小要求

- Bearer token
- 节点级 token
- 基础 allowlist

### 9.2 请求上下文字段

建议记录以下审计字段：

- request_id
- caller_id
- node_id
- session_id
- remote_addr
- ts

### 9.3 目标

- 防止 ForgeAI 接口被旁路滥用
- 保持 OpenClaw 原生保护边界
- 为后续权限系统预留空间

## 10. 错误模型

Clawy 需要向 ForgeAI 返回结构化错误，而不是暴露未经整理的底层异常。

建议统一错误结构：

```json
{
  "ok": false,
  "error": {
    "code": "OPENCLAW_UNREACHABLE",
    "message": "Gateway not reachable",
    "detail": "127.0.0.1:18789 connection refused"
  }
}
```

P0 至少需要覆盖：

- `UNAUTHORIZED`
- `INVALID_REQUEST`
- `SESSION_NOT_FOUND`
- `RUN_NOT_FOUND`
- `OPENCLAW_UNREACHABLE`
- `GATEWAY_NOT_RUNNING`
- `INTERNAL_ERROR`

## 11. 数据映射原则

Clawy 对 OpenClaw 数据只做最小标准化，不应破坏原始语义。

### 11.1 必须保留的原则

- 不篡改原始业务含义
- 不重写 OpenClaw runtime 逻辑
- 不引入第二套对话状态机语义

### 11.2 允许做的适配

- 事件类型标准化
- 字段命名统一
- 错误摘要格式化
- 对 ForgeAI 友好的 payload 裁剪

## 12. 附件模型约束

`POST /api/sessions/:id/send` 中的 `attachments` 字段必须提前明确语义。

推荐方案需要在实现前二选一：

### 方案 A：文件已上传到 Clawy

请求中传文件元信息和已存在的文件引用。

### 方案 B：HTTP multipart 上传

由 ForgeAI 直接通过 multipart 把附件传给 Clawy。

### 方案 C：远程 URL

ForgeAI 提供临时下载 URL，Clawy 负责拉取。

在未明确方案前，不建议进入具体实现。

P0 如只做文本消息，可先将 `attachments` 设计为可选保留字段，不在第一阶段落地媒体传输。

## 13. 节点生命周期

### 13.1 启动时

Clawy 需要：

1. 检查本机 OpenClaw 状态
2. 准备本地 Bridge 服务
3. 生成节点元信息
4. 准备与 ForgeAI 通信所需鉴权信息

### 13.2 运行时

Clawy 需要：

1. 接收 ForgeAI 请求
2. 转发给 OpenClaw
3. 接收 OpenClaw 事件
4. 推送标准化事件
5. 维护本地健康状态

### 13.3 异常时

Clawy 需要：

1. 标记 OpenClaw 不可达
2. 返回结构化错误
3. 保留最后错误摘要
4. 允许后续恢复连接

## 14. 实施优先级

### P0

优先实现以下最小集合：

- `GET /api/node/info`
- `GET /api/node/health`
- `GET /api/sessions`
- `GET /api/sessions/:id/history`
- `POST /api/sessions/:id/send`
- `POST /api/sessions/:id/abort`
- `WS /api/events`
- 基础鉴权

### P1

随后实现：

- `GET /api/runtime/status`
- `GET /api/runtime/capabilities`
- 错误模型统一
- 事件 schema 稳定化
- 审计日志

### P2

后续增强：

- 更细粒度权限控制
- 事件重放与断线恢复
- 节点注册和心跳 client
- 离线缓存与恢复机制

## 15. 最终定义

### 一句话定义

Clawy 是部署在每台开发机上的本地桥接层，用于将 ForgeAI 与本机 OpenClaw 安全、标准化地连接起来。

### 更严格的定义

Clawy：

- 不负责执行
- 不复制 runtime
- 不扩张为第二个 OpenClaw
- 只负责桥接、映射、暴露、转发

真正做事情的，始终是底层 OpenClaw。
