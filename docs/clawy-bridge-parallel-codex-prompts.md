# Clawy Bridge 并行开发 Codex 提示词

本文档用于把 Clawy Bridge 开发拆成多个可并行推进的 Codex 任务组，并为每个任务组提供可直接复制使用的专业提示词。

目标：

- 让每个 Codex 工作线程完整理解需求和边界
- 降低多人并行开发时的冲突
- 明确禁止改动 OpenClaw 本体、运行时和无关 UI

## 1. 全局约束

以下约束适用于所有任务组，建议作为每个子任务提示词的固定前缀：

```text
你正在 Clawy 仓库中工作，路径为 /Users/wykj/Projects/Clawy。

在开始前先阅读以下文档：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md

这次工作的核心目标是：
- 在 Clawy 内实现面向 ForgeAI 的 northbound Bridge API
- 采用 HTTP 作为控制面、WebSocket 作为事件面
- OpenClaw 仍然是底层 runtime，Clawy 只做本地桥接层

严格边界：
- 不要修改 OpenClaw 仓库、OpenClaw runtime、OpenClaw 发布包、OpenClaw 安装逻辑
- 不要试图“改造 OpenClaw 协议”来配合这次需求
- 不要重写聊天 UI、设置页、安装向导、Provider 管理、技能管理，除非当前任务明确要求
- 不要把 ForgeAI 接口做在前端页面层；ForgeAI northbound API 必须落在 Rust 侧
- 不要把这次需求扩展成多节点调度系统、Agent runtime 或新的工作流引擎

实现原则：
- 优先复用 Clawy 现有的 Gateway/OpenClaw 接入能力
- 以最小闭环为目标，不做无关重构
- 先保证接口契约稳定，再补复杂功能
- 如果需要引用现有 Gateway 行为，可以读取以下文件作为参考，但不要随意重写它们：
  - /Users/wykj/Projects/Clawy/src/lib/desktop/bridge.ts
  - /Users/wykj/Projects/Clawy/src/stores/chat.ts
  - /Users/wykj/Projects/Clawy/src/stores/gateway.ts
  - /Users/wykj/Projects/Clawy/src-tauri/src/lib.rs

交付要求：
- 只修改当前任务授权的文件范围
- 如需越界改动，先在最终说明中明确指出原因
- 代码应保持平台兼容，尤其不要引入只适用于 macOS 的路径或进程假设
- 最终输出必须包含：
  1. 完成了什么
  2. 改了哪些文件
  3. 跑了哪些验证
  4. 还有哪些已知风险或待后续组处理
```

## 2. 任务组总览

| 组别 | 目标 | 建议分支 | 主要文件所有权 |
|------|------|----------|----------------|
| G0 | 架构与契约冻结 | `codex/bridge-g0-freeze` | `docs/` |
| G2 | Bridge 服务骨架 | `codex/bridge-g2-server` | `src-tauri/src/bridge/server.rs`, `auth.rs`, `response.rs`, `bootstrap.rs`, `Cargo.toml` |
| G3 | 节点与会话读接口 | `codex/bridge-g3-read-api` | `src-tauri/src/bridge/node.rs`, `sessions.rs` |
| G4 | 会话控制接口 | `codex/bridge-g4-chat-control` | `src-tauri/src/bridge/chat_control.rs` |
| G5 | 事件映射与 WS | `codex/bridge-g5-events-ws` | `src-tauri/src/bridge/events.rs`, `ws.rs`, `gateway_adapter.rs` |
| G6 | 运行时与能力元信息 | `codex/bridge-g6-runtime` | `src-tauri/src/bridge/runtime.rs`, `capabilities.rs`, `errors.rs` |
| G7 | 联调与验证 | `codex/bridge-g7-validation` | `src-tauri/tests/`, `docs/bridge-*` |

说明：

- G0 是并行开发前的前置冻结组，建议先完成。
- G2 是基础设施组，G3/G4/G5/G6 会依赖它提供的 server 骨架。
- G3/G4/G5/G6 可以在 G2 落完骨架后并行。
- G7 可以较早开始搭测试桩，但最终联调要等前面各组基本完成。

## 3. Prompt: G0 架构与契约冻结组

```text
你负责 Clawy Bridge 的前置冻结工作。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md

你的任务目标：
- 冻结 P0 northbound API schema
- 冻结统一错误结构
- 冻结 Bearer token 鉴权语义
- 冻结 session_id、run_id、node_id 的规则
- 冻结 OpenClaw 原始事件到标准 Bridge 事件的映射规则
- 为后续并行开发组提供不歧义的协议基础

你的边界：
- 只允许修改 docs/ 下的 Bridge 设计文档
- 不要实现代码
- 不要改 src/ 或 src-tauri/ 里的运行时代码
- 不要改 OpenClaw 相关逻辑

建议新增或更新的文档：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-event-mapping.md
- 必要时更新现有的 design/tasklist 文档

必须明确写清楚的内容：
- 每个 P0 HTTP 接口的 request/response schema
- 错误输出结构和核心错误码
- WS 事件统一 envelope
- 各事件 type 的 payload 定义
- session_id 是否直接暴露 OpenClaw key
- send 接口返回 accepted/run_id 的策略
- 本地监听限制和 token 鉴权规则

禁止事项：
- 不要把 register/heartbeat 混进 Clawy northbound API
- 不要引入附件上传协议到 P0，P0 只需文本路径
- 不要提出需要改 OpenClaw 协议才能完成的设计

验收标准：
- 后续组能直接按文档编码，不需要自行猜测 schema
- 文档中没有“待定”“以后再看”这种阻塞实现的关键项
- 术语统一，字段命名一致

最终输出：
- 列出新增/更新的文档
- 明确哪些协议已冻结
- 标出仍留到 P1/P2 的项目
```

## 4. Prompt: G2 Bridge 服务骨架组

```text
你负责在 Rust 侧建立 Clawy Bridge 的本地 HTTP/WS 服务骨架。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- 如果已经存在，请一并阅读：
  - /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
  - /Users/wykj/Projects/Clawy/docs/clawy-bridge-event-mapping.md

你的任务目标：
- 在 Rust 侧建立 northbound Bridge server 的最小骨架
- 提供 /api/* 路由入口
- 提供统一响应封装
- 提供 Bearer token 鉴权中间件
- 为 WS /api/events 提供连接入口和基础上下文
- 为后续业务组留出清晰的 handler 扩展点

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/Cargo.toml
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/server.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/auth.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/response.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/bootstrap.rs
- 允许最小化修改 /Users/wykj/Projects/Clawy/src-tauri/src/lib.rs 以接入 bridge 启停

你的边界：
- 不要实现具体的 node/session/chat/runtime 业务逻辑
- 不要抢占 G3/G4/G5/G6 的文件
- 不要改前端页面、store、desktop bridge
- 不要触碰 OpenClaw 安装、更新、Provider、Skill、Channel 逻辑

实现要求：
- Bridge server 必须在 Rust 层
- 默认仅绑定 loopback，本地可访问
- 鉴权采用 Bearer token，未授权返回统一结构
- 为 request_id、caller_id 这类后续字段预留上下文
- 结构要便于后续组直接新增 handler，而不是继续把逻辑堆进 lib.rs

建议输出的代码结构：
- bridge/mod.rs
- bridge/server.rs
- bridge/auth.rs
- bridge/response.rs
- bridge/bootstrap.rs

禁止事项：
- 不要在前端 JS 层起一个给 ForgeAI 用的 HTTP server
- 不要把 Gateway 客户端协议直接重写一遍作为本组任务
- 不要过度设计成微服务框架

验收标准：
- 可以启动本地 Bridge server
- /api/* 路由骨架可访问
- 未授权请求会被拒绝
- 后续组能在不重构骨架的情况下挂接具体 handler

最终输出：
- 说明新增了哪些基础模块
- 说明如何扩展路由
- 说明跑过的编译/测试验证
```

## 5. Prompt: G3 节点与会话读接口组

```text
你负责实现 Clawy Bridge 的只读接口：节点信息、健康检查、会话查询和会话历史。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy/src/stores/chat.ts
- /Users/wykj/Projects/Clawy/src-tauri/src/lib.rs

你的任务目标：
- 实现 GET /api/node/info
- 实现 GET /api/node/health
- 实现 GET /api/sessions
- 实现 GET /api/sessions/:id
- 实现 GET /api/sessions/:id/history
- 定义并落地 node_id 持久化
- 定义并落地外部 session_id 到 OpenClaw session key 的读映射

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/node.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/sessions.rs
- 如需少量共享类型，可在本组自有文件中定义，避免改动其他组文件

你的边界：
- 不要实现 send/abort
- 不要实现 WS 事件
- 不要修改鉴权和 server 骨架
- 不要改前端聊天 store 的现有行为
- 不要改 OpenClaw session 数据格式

实现要求：
- 优先复用现有 Gateway/OpenClaw 查询路径
- 历史查询支持设计文档里的基础 limit/before/after 能力
- 节点健康状态要能区分 online、gateway_running、openclaw_reachable、last_error
- node_id 必须有稳定持久化策略，不能每次启动随机变化

禁止事项：
- 不要因为 schema 不明确就自行发明一套不同字段名
- 不要直接暴露不稳定的内部结构给 ForgeAI
- 不要将前端 UI 私有字段原样透传为 API 输出

验收标准：
- 能返回稳定的节点与会话读模型
- session_id 映射可逆且行为一致
- OpenClaw/Gateway 不可达时返回结构化错误

最终输出：
- 说明读接口如何映射到底层现有能力
- 列出 node_id 和 session_id 的持久化/映射策略
- 说明跑过的验证
```

## 6. Prompt: G4 会话控制接口组

```text
你负责实现 Clawy Bridge 的会话控制接口。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy/src/stores/chat.ts
- /Users/wykj/Projects/Clawy/src-tauri/src/lib.rs

你的任务目标：
- 实现 POST /api/sessions/:id/send
- 实现 POST /api/sessions/:id/abort
- 明确并返回 accepted/run_id
- 打通最小文本消息发送路径

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/chat_control.rs

你的边界：
- P0 只支持文本发送，不要扩展附件上传协议
- 不要实现事件推送，这属于 G5
- 不要动会话列表/history 读接口，这属于 G3
- 不要改前端聊天 UI
- 不要改 OpenClaw runtime、模型、工具或技能执行逻辑

实现要求：
- 发送接口语义是 accepted，而不是同步返回最终回答
- 使用既有 Gateway/OpenClaw 能力发起 send/abort
- 正确处理 session_id 到底层 session key 的映射
- 底层失败时返回结构化错误，不要直接抛原始字符串

禁止事项：
- 不要把 send 设计成长轮询结果接口
- 不要为了“方便”而修改 OpenClaw 事件协议
- 不要在本组里顺便实现附件、多模态或外部文件上传

验收标准：
- ForgeAI 能通过 HTTP 发文本消息
- ForgeAI 能中止当前运行
- 返回值中包含可用于对齐事件流的 run_id 或清晰等价物

最终输出：
- 说明 send/abort 如何映射到底层调用
- 说明 run_id 的来源或生成策略
- 说明跑过的验证
```

## 7. Prompt: G5 事件映射与 WS 组

```text
你负责 Clawy Bridge 的事件标准化和 WebSocket 推送能力。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-event-mapping.md
- /Users/wykj/Projects/Clawy/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy/src/stores/gateway.ts
- /Users/wykj/Projects/Clawy/src/stores/chat.ts

你的任务目标：
- 实现 WS /api/events
- 建立 OpenClaw 原始事件到标准事件的映射层
- 推送至少以下事件：
  - message.delta
  - message.final
  - message.thinking
  - tool.call
  - tool.result
  - runtime.status
  - runtime.error
  - session.updated

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/events.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/ws.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/gateway_adapter.rs

你的边界：
- 不要实现 HTTP 读接口和 send/abort 接口
- 不要修改 OpenClaw 原始事件格式
- 不要重构前端聊天事件状态机
- 不要提前实现复杂的事件续传缓存，P0 只需最小实时推送

实现要求：
- 事件 envelope 要严格遵守已冻结 schema
- 需要能将底层 Gateway/OpenClaw 事件转换为标准事件
- 最小支持按连接推送；如果方便，可预留 filter/heartbeat 扩展点
- runtime.error 必须结构化，不要只是字符串广播

禁止事项：
- 不要把前端 localBus 事件名直接暴露给 ForgeAI
- 不要把未整理的原始 payload 全量透传为标准接口
- 不要让 WS 依赖前端页面是否打开

验收标准：
- WS 客户端可持续接收标准化事件
- send 触发后，事件流能按 session_id/run_id 正常关联
- Gateway 异常时能发出 runtime.error 或等价结构化状态

最终输出：
- 说明事件来源、映射规则和标准化策略
- 说明 WS 连接、订阅和断开行为
- 说明跑过的验证
```

## 8. Prompt: G6 运行时与能力元信息组

```text
你负责 Clawy Bridge 的运行时状态和能力元信息接口。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/src-tauri/src/lib.rs

你的任务目标：
- 实现 GET /api/runtime/status
- 实现 GET /api/runtime/capabilities
- 整理 Bridge 错误到 OpenClaw/Gateway 错误的映射基础

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/runtime.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/capabilities.rs
- /Users/wykj/Projects/Clawy/src-tauri/src/bridge/errors.rs

你的边界：
- 不要实现 node/session/chat/WS 的主要逻辑
- 不要改 Provider、Skill、Channel 的配置写入逻辑
- 不要去“增强” OpenClaw 的能力探测方式，只在 Clawy 现有能力之上抽象接口

实现要求：
- runtime/status 至少覆盖 gateway 状态、OpenClaw runtime 状态、当前 provider/model、config_dir、连接状态
- runtime/capabilities 至少覆盖 agent identity、loaded skills、enabled tools、workspace info、channel bindings 概况
- 错误结构要兼容后续组复用

禁止事项：
- 不要把 UI 展示字段和后端 API 字段混在一起
- 不要引入需要改动 OpenClaw 配置格式的新约束

验收标准：
- ForgeAI 能以稳定 schema 拉到运行时状态和能力概览
- 常见错误有统一输出结构

最终输出：
- 说明各字段来源
- 说明哪些能力是直接读取、哪些是归纳计算
- 说明跑过的验证
```

## 9. Prompt: G7 联调与验证组

```text
你负责 Clawy Bridge 的联调、验证和验收收口。

先阅读：
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy/docs/clawy-bridge-event-mapping.md

你的任务目标：
- 为 P0 搭建最小验证方案
- 验证 node info/health、sessions、history、send、abort、WS events
- 验证未授权、Gateway 不可达、OpenClaw 未就绪等失败路径
- 产出联调记录和验收清单

文件所有权：
- /Users/wykj/Projects/Clawy/src-tauri/tests/
- /Users/wykj/Projects/Clawy/docs/bridge-validation-*.md

你的边界：
- 不要替其他组重写生产代码
- 只在确有必要时提交最小测试支撑改动
- 不要把“为了让测试通过”变成协议回退或随意放宽鉴权

实现要求：
- 验证要覆盖成功路径和关键失败路径
- 优先写能长期复用的 smoke/integration 测试
- 若当前仓库测试基础设施不足，可以补最小可维护方案，但不要重型化

禁止事项：
- 不要把测试逻辑塞进生产 handler
- 不要为了模拟环境而改 OpenClaw 行为
- 不要跳过错误路径验证

验收标准：
- 能证明 P0 最小链路闭环成立
- 能明确指出剩余风险和后续 P1/P2 工作

最终输出：
- 列出验证覆盖面
- 列出实际跑过的命令
- 列出失败案例是否符合预期
```

## 10. 推荐投放顺序

推荐按以下顺序把提示词投给不同 Codex 线程：

1. 先启动 G0
2. G0 有初版冻结结果后，启动 G2
3. G2 落完 server 骨架后，同时启动 G3、G4、G5、G6
4. G7 在 G2 后即可开始准备验证桩，在 G3/G4/G5 基本完成后做集中联调

## 11. 额外提醒

给每个 Codex 线程下发任务时，建议再额外补一句：

```text
如果你发现当前需求需要改 OpenClaw 本体、OpenClaw 协议或桌面 UI 才能推进，不要自行扩 scope。请先在结果中清楚说明阻塞点和替代方案。
```

这句话很有用，可以显著减少线程擅自扩范围的概率。
