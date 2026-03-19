# Clawy Bridge Tasklist

## 1. 目标

本文档用于把 Clawy Bridge 设计需求拆解成可执行任务，并明确优先级、前置依赖和交付物。

目标是按阶段推进：

- 先打通最小 P0 桥接链路
- 再补齐运行时元信息、错误模型和稳定性能力
- 最后再做注册、心跳和增强能力

## 2. 总体原则

- 优先复用现有 Clawy 本地能力，不重写 OpenClaw runtime
- 优先做 ForgeAI 可直接接入的 northbound API
- 先定义契约，再落实现
- 先打通单节点闭环，再做多节点协作能力

## 3. 任务分期

### P0: 最小可用桥接能力

目标：

- ForgeAI 能通过 Clawy 控制本机 OpenClaw 会话
- ForgeAI 能通过 Clawy 接收标准化事件流
- ForgeAI 能识别节点基础状态

#### P0-1 API 契约冻结

- [ ] 冻结 P0 HTTP 接口定义
- [ ] 冻结 `WS /api/events` 事件结构
- [ ] 冻结统一错误结构
- [ ] 冻结基础鉴权方案

交付物：

- northbound API 契约文档
- 事件 schema 文档
- 错误码表

前置依赖：

- [clawy-bridge-design-requirements.md](/Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md)

#### P0-2 Bridge 服务骨架

- [ ] 确定 Clawy Bridge 服务运行形态
- [ ] 选定本地 HTTP/WS 服务实现方式
- [ ] 建立 `/api/*` 路由骨架
- [ ] 建立统一请求上下文和鉴权中间件
- [ ] 建立统一响应封装

交付物：

- 本地 Bridge server 基础框架
- HTTP 路由骨架
- WS 连接入口

前置依赖：

- P0-1

#### P0-3 节点接口

- [ ] 实现 `GET /api/node/info`
- [ ] 实现 `GET /api/node/health`
- [ ] 定义 `node_id` 生成或持久化策略
- [ ] 定义 `last_seen_at` 更新机制

交付物：

- 可访问的节点信息接口
- 可访问的节点健康接口

前置依赖：

- P0-2

#### P0-4 会话查询接口

- [ ] 实现 `GET /api/sessions`
- [ ] 实现 `GET /api/sessions/:id`
- [ ] 实现 `GET /api/sessions/:id/history`
- [ ] 建立 Clawy session id 到 OpenClaw session key 的映射策略

交付物：

- 会话列表接口
- 会话摘要接口
- 会话历史接口

前置依赖：

- P0-2

#### P0-5 会话控制接口

- [ ] 实现 `POST /api/sessions/:id/send`
- [ ] 实现 `POST /api/sessions/:id/abort`
- [ ] 确认 `run_id` 返回策略
- [ ] 确认文本消息最小发送路径

交付物：

- 消息发送接口
- 中止运行接口

前置依赖：

- P0-4

#### P0-6 事件标准化与 WS 推送

- [ ] 建立 OpenClaw 原始事件到标准事件的映射层
- [ ] 实现 `WS /api/events`
- [ ] 推送 `message.delta`
- [ ] 推送 `message.final`
- [ ] 推送 `message.thinking`
- [ ] 推送 `tool.call`
- [ ] 推送 `tool.result`
- [ ] 推送 `runtime.status`
- [ ] 推送 `runtime.error`
- [ ] 推送 `session.updated`

交付物：

- 可连接的事件 WS
- 最小标准化事件流

前置依赖：

- P0-2
- P0-4
- P0-5

#### P0-7 基础鉴权

- [ ] 实现 Bearer token 校验
- [ ] 支持节点级 token 配置
- [ ] 建立 allowlist 入口
- [ ] 未授权响应统一化

交付物：

- 基础鉴权能力
- 统一未授权错误响应

前置依赖：

- P0-2

#### P0-8 P0 联调与验收

- [ ] 单节点完整链路联调
- [ ] 文本消息发送与事件回流验证
- [ ] Gateway 不可达时错误验证
- [ ] OpenClaw 未就绪时错误验证
- [ ] 鉴权失败验证

交付物：

- P0 联调记录
- P0 验收清单

前置依赖：

- P0-3
- P0-4
- P0-5
- P0-6
- P0-7

### P1: 能力补全与稳定化

目标：

- 补齐运行时和能力元信息
- 固化 schema 与错误模型
- 增强可维护性和可审计性

#### P1-1 运行时状态接口

- [ ] 实现 `GET /api/runtime/status`
- [ ] 汇总 Gateway 状态
- [ ] 汇总 OpenClaw runtime 状态
- [ ] 暴露当前 provider / model / config_dir

交付物：

- 运行时状态接口

前置依赖：

- P0-8

#### P1-2 能力元信息接口

- [ ] 实现 `GET /api/runtime/capabilities`
- [ ] 暴露 agent identity
- [ ] 暴露 loaded skills
- [ ] 暴露 enabled tools
- [ ] 暴露 workspace info
- [ ] 暴露 channel bindings 概况

交付物：

- 能力元信息接口

前置依赖：

- P0-8

#### P1-3 错误模型统一

- [ ] 建立统一错误码枚举
- [ ] 统一 HTTP 错误输出结构
- [ ] 统一 WS `runtime.error` payload
- [ ] 建立 OpenClaw 错误到 Bridge 错误的映射表

交付物：

- 错误模型文档
- 统一错误输出实现

前置依赖：

- P0-8

#### P1-4 API 版本化

- [ ] 定义 API version 策略
- [ ] 增加 schema version 字段或路径版本
- [ ] 版本兼容约束写入文档

交付物：

- 版本化约定
- 版本字段实现

前置依赖：

- P0-8

#### P1-5 审计与日志

- [ ] 为每个 HTTP 请求生成 request_id
- [ ] 记录 caller_id / session_id / node_id
- [ ] 记录关键操作审计日志
- [ ] 定义敏感字段脱敏策略

交付物：

- 审计日志基础能力

前置依赖：

- P0-7

#### P1-6 稳定性增强

- [ ] WS heartbeat / ping
- [ ] 订阅过滤能力
- [ ] 断线恢复预留
- [ ] Gateway 异常恢复策略梳理

交付物：

- WS 稳定性增强

前置依赖：

- P0-6

### P2: 节点管理增强

目标：

- 打通 Clawy 到 ForgeAI 的主动注册和保活
- 增强权限、恢复和发现能力

#### P2-1 ForgeAI 注册 client

- [ ] 定义 ForgeAI registry 接口契约
- [ ] 实现节点启动注册
- [ ] 实现定时 heartbeat
- [ ] 实现异常重试

交付物：

- 注册和心跳 client

前置依赖：

- P1-1

#### P2-2 更细粒度权限

- [ ] 读写权限分层
- [ ] session 级权限控制
- [ ] 能力级权限预留

交付物：

- 权限模型增强方案

前置依赖：

- P1-3

#### P2-3 事件续传与离线恢复

- [ ] `last_event_id` 机制
- [ ] 事件缓存策略
- [ ] 重连补发策略

交付物：

- 事件恢复机制

前置依赖：

- P1-6

#### P2-4 节点发现增强

- [ ] 动态节点发现方案
- [ ] 离线节点标记策略
- [ ] 失联超时策略

交付物：

- 节点发现与状态同步增强方案

前置依赖：

- P2-1

## 4. 横向专题任务

这些任务跨多个阶段，需要尽早明确边界。

### X-1 Session ID 映射

- [ ] 定义 OpenClaw session key 与外部 session_id 的映射规则
- [ ] 确定是否对外暴露原始 key
- [ ] 确定 canonical key 归一化策略

### X-2 附件模型

- [ ] 确定附件方案
  - 已上传文件引用
  - multipart
  - 临时 URL
- [ ] 确定 P0 是否仅支持文本
- [ ] 确定媒体事件是否纳入 P0

### X-3 事件映射策略

- [ ] 明确 OpenClaw 原始事件到标准事件的转换规则
- [ ] 明确哪些事件保留原始 payload
- [ ] 明确 summary 字段如何生成

### X-4 Node ID 持久化

- [ ] 定义 node_id 生成规则
- [ ] 定义持久化位置
- [ ] 定义重装或清理后的行为

### X-5 安全基线

- [ ] token 存储位置
- [ ] token 轮换策略
- [ ] allowlist 配置来源
- [ ] 本地监听地址限制

## 5. 推荐实现顺序

建议按以下顺序推进：

1. P0-1 API 契约冻结
2. X-1 Session ID 映射
3. X-3 事件映射策略
4. P0-2 Bridge 服务骨架
5. P0-3 节点接口
6. P0-4 会话查询接口
7. P0-5 会话控制接口
8. P0-6 事件 WS
9. P0-7 基础鉴权
10. P0-8 联调验收
11. P1-1 运行时状态
12. P1-2 能力元信息
13. P1-3 错误模型统一
14. P1-4 API 版本化
15. P1-5 审计日志
16. P1-6 稳定性增强

## 6. P0 验收标准

P0 完成时，必须满足：

- ForgeAI 能通过 HTTP 获取节点信息
- ForgeAI 能获取本机 session 列表
- ForgeAI 能查看指定 session 历史
- ForgeAI 能向指定 session 发送文本消息
- ForgeAI 能中止当前运行
- ForgeAI 能通过 WS 实时收到标准化事件
- 未授权请求被正确拒绝
- OpenClaw 不可达时返回结构化错误

## 7. 下一步建议

在正式实现前，建议立刻完成两件事：

- 先把 P0 northbound API 文档冻结为 request / response schema
- 再把事件映射规则单独整理成一个对照表

这样可以避免一边写代码一边改协议。
