# Clawy Bridge Codex 分发短提示词

本文档提供可直接复制给多个 Codex 线程的短提示词版本。

使用建议：

- 每个线程只拿自己对应的提示词
- 每个线程必须先创建并进入自己的独立 `git worktree`
- 不要在 `/Users/wykj/Projects/Clawy` 这份主 checkout 里切来切去
- 先启动 `G0`
- `G0` 有初版后再启动 `G2`
- `G2` 骨架稳定后再并行启动 `G3`、`G4`、`G5`、`G6`
- `G7` 可以提前准备验证，但最终验收依赖前面各组产出

## 所有组的共同前置动作

在开始任何编码或文档修改前，先为当前任务组创建独立 worktree。

总规则：

- `/Users/wykj/Projects/Clawy` 是主仓库，只用于统筹、拉最新代码、最终合并
- 每个任务组必须在自己的 worktree 目录里工作
- 不要在主仓库里切换到 `codex/...` 分支再开发
- 不要多个任务组共享同一个 worktree

创建规则：

1. 从主仓库目录 `/Users/wykj/Projects/Clawy` 执行 `git worktree add`
2. 如果目标分支还不存在，使用 `-b`
3. 如果目标分支已经存在，不要重复 `-b`，直接挂已有分支
4. 创建完成后，把后续所有操作都放在该 worktree 目录中

分支已不存在时的命令模板：

```bash
git worktree add <worktree-path> -b <branch-name> develop
```

分支已存在时的命令模板：

```bash
git worktree add <worktree-path> <branch-name>
```

如果你发现自己正在主仓库 `/Users/wykj/Projects/Clawy` 里改文件，请先停止，不要继续提交改动，先切换到自己的 worktree 再工作。

---

## G0

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g0 -b codex/bridge-g0-freeze develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g0 codex/bridge-g0-freeze

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g0

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g0/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g0/docs/clawy-bridge-tasklist.md

任务：
- 冻结 Clawy Bridge 的 P0 协议
- 输出 P0 HTTP request/response schema
- 输出统一错误结构和核心错误码
- 输出 WS 事件 envelope 和各 type 的 payload 定义
- 明确 session_id、run_id、node_id 规则
- 明确 Bearer token 和本地监听限制

边界：
- 只改 docs/
- 不写业务代码
- 不改 OpenClaw、本地 UI、安装链路
- 不把 register/heartbeat 混进 Clawy northbound API
- P0 不做附件协议

建议产物：
- /Users/wykj/Projects/Clawy-g0/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g0/docs/clawy-bridge-event-mapping.md

验收：
- 后续开发组不需要再猜 schema
- 文档里没有阻塞实现的关键待定项

最终输出：
1. 新增/更新了哪些文档
2. 已冻结的协议项
3. 留到 P1/P2 的内容
```

## G2

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g2 -b codex/bridge-g2-server develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g2 codex/bridge-g2-server

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g2

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g2/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g2/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g2/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g2/docs/clawy-bridge-event-mapping.md

任务：
- 在 Rust 侧建立 Clawy Bridge 的 HTTP/WS server 骨架
- 建立 /api/* 路由骨架
- 建立统一响应封装
- 建立 Bearer token 鉴权中间件
- 为 WS /api/events 提供连接入口
- 为后续组预留清晰 handler 扩展点

文件所有权：
- /Users/wykj/Projects/Clawy-g2/src-tauri/Cargo.toml
- /Users/wykj/Projects/Clawy-g2/src-tauri/src/bridge/server.rs
- /Users/wykj/Projects/Clawy-g2/src-tauri/src/bridge/auth.rs
- /Users/wykj/Projects/Clawy-g2/src-tauri/src/bridge/response.rs
- /Users/wykj/Projects/Clawy-g2/src-tauri/src/bridge/bootstrap.rs
- 允许最小改动 /Users/wykj/Projects/Clawy-g2/src-tauri/src/lib.rs 以接入启动

边界：
- 不实现 node/session/chat/runtime 业务逻辑
- 不改前端页面、store、desktop bridge
- 不改 OpenClaw、本地安装更新、Provider、Skill、Channel
- 不要在 JS 层起 ForgeAI API server

要求：
- 仅绑定 loopback
- 预留 request_id、caller_id 等上下文
- 保持结构便于后续组挂 handler

验收：
- server 能启动
- /api/* 骨架可访问
- 未授权请求能被统一拒绝

最终输出：
1. 建了哪些基础模块
2. 如何给后续组扩路由
3. 跑了哪些编译/验证
```

## G3

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g3 -b codex/bridge-g3-read-api develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g3 codex/bridge-g3-read-api

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g3

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g3/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g3/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g3/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g3/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy-g3/src/stores/chat.ts
- /Users/wykj/Projects/Clawy-g3/src-tauri/src/lib.rs

任务：
- 实现 GET /api/node/info
- 实现 GET /api/node/health
- 实现 GET /api/sessions
- 实现 GET /api/sessions/:id
- 实现 GET /api/sessions/:id/history
- 落地 node_id 持久化
- 落地 session_id 到 OpenClaw session key 的读映射

文件所有权：
- /Users/wykj/Projects/Clawy-g3/src-tauri/src/bridge/node.rs
- /Users/wykj/Projects/Clawy-g3/src-tauri/src/bridge/sessions.rs

边界：
- 不实现 send/abort
- 不实现 WS 事件
- 不改 server/auth 骨架
- 不改前端聊天行为
- 不改 OpenClaw session 数据格式

要求：
- 优先复用现有 Gateway/OpenClaw 读路径
- history 支持 limit/before/after
- 健康状态清楚区分 online、gateway_running、openclaw_reachable、last_error
- node_id 必须稳定持久化

验收：
- 节点与会话读接口返回稳定 schema
- session_id 映射一致
- 不可达场景返回结构化错误

最终输出：
1. 接口如何映射到底层能力
2. node_id/session_id 策略
3. 跑了哪些验证
```

## G4

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g4 -b codex/bridge-g4-chat-control develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g4 codex/bridge-g4-chat-control

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g4

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g4/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g4/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g4/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g4/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy-g4/src/stores/chat.ts
- /Users/wykj/Projects/Clawy-g4/src-tauri/src/lib.rs

任务：
- 实现 POST /api/sessions/:id/send
- 实现 POST /api/sessions/:id/abort
- 明确并返回 accepted/run_id
- 打通最小文本消息发送路径

文件所有权：
- /Users/wykj/Projects/Clawy-g4/src-tauri/src/bridge/chat_control.rs

边界：
- P0 只支持文本，不做附件协议
- 不实现 WS 推送
- 不动 sessions 列表/history 接口
- 不改前端聊天 UI
- 不改 OpenClaw runtime、模型、工具或技能执行

要求：
- send 是 accepted 语义，不返回最终回答
- 复用现有 Gateway/OpenClaw 控制路径
- 正确处理 session_id 映射
- 失败时返回结构化错误

验收：
- ForgeAI 能发文本消息
- ForgeAI 能 abort 当前运行
- 返回值包含 run_id 或清晰等价物

最终输出：
1. send/abort 如何映射到底层
2. run_id 来源或策略
3. 跑了哪些验证
```

## G5

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g5 -b codex/bridge-g5-events-ws develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g5 codex/bridge-g5-events-ws

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g5

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g5/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g5/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g5/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g5/docs/clawy-bridge-event-mapping.md
- /Users/wykj/Projects/Clawy-g5/src/lib/desktop/bridge.ts
- /Users/wykj/Projects/Clawy-g5/src/stores/gateway.ts
- /Users/wykj/Projects/Clawy-g5/src/stores/chat.ts

任务：
- 实现 WS /api/events
- 建立 OpenClaw 原始事件到标准事件的映射层
- 推送 message.delta
- 推送 message.final
- 推送 message.thinking
- 推送 tool.call
- 推送 tool.result
- 推送 runtime.status
- 推送 runtime.error
- 推送 session.updated

文件所有权：
- /Users/wykj/Projects/Clawy-g5/src-tauri/src/bridge/events.rs
- /Users/wykj/Projects/Clawy-g5/src-tauri/src/bridge/ws.rs
- /Users/wykj/Projects/Clawy-g5/src-tauri/src/bridge/gateway_adapter.rs

边界：
- 不实现 HTTP 读接口和 send/abort
- 不改 OpenClaw 原始事件格式
- 不重构前端聊天状态机
- P0 不做复杂事件续传缓存

要求：
- 严格遵守已冻结事件 schema
- 不把前端 localBus 事件名直接暴露给 ForgeAI
- runtime.error 要结构化
- WS 行为不能依赖前端页面是否打开

验收：
- WS 客户端能持续收到标准化事件
- 发送后事件能按 session_id/run_id 关联
- Gateway 异常能发出 runtime.error 或等价结构化状态

最终输出：
1. 事件来源和映射策略
2. WS 连接与断开行为
3. 跑了哪些验证
```

## G6

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g6 -b codex/bridge-g6-runtime develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g6 codex/bridge-g6-runtime

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g6

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g6/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g6/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g6/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g6/src-tauri/src/lib.rs

任务：
- 实现 GET /api/runtime/status
- 实现 GET /api/runtime/capabilities
- 打基础版 Bridge 错误映射

文件所有权：
- /Users/wykj/Projects/Clawy-g6/src-tauri/src/bridge/runtime.rs
- /Users/wykj/Projects/Clawy-g6/src-tauri/src/bridge/capabilities.rs
- /Users/wykj/Projects/Clawy-g6/src-tauri/src/bridge/errors.rs

边界：
- 不实现 node/session/chat/WS 主逻辑
- 不改 Provider、Skill、Channel 配置写入逻辑
- 不去改造 OpenClaw 的能力探测方式

要求：
- runtime/status 覆盖 gateway 状态、OpenClaw runtime 状态、当前 provider/model、config_dir、连接状态
- runtime/capabilities 覆盖 agent identity、loaded skills、enabled tools、workspace info、channel bindings 概况
- 错误结构可供其他组复用

验收：
- ForgeAI 能稳定拉到运行时状态和能力概览
- 常见错误有统一输出结构

最终输出：
1. 字段来源
2. 哪些是直接读取，哪些是归纳计算
3. 跑了哪些验证
```

## G7

```text
主仓库是 /Users/wykj/Projects/Clawy，但你不能直接在这份共享 checkout 里开发。

先创建并进入独立 worktree：
- 若分支不存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g7 -b codex/bridge-g7-validation develop
- 若分支已存在：
  git -C /Users/wykj/Projects/Clawy worktree add /Users/wykj/Projects/Clawy-g7 codex/bridge-g7-validation

从现在开始，你的实际工作目录是：
- /Users/wykj/Projects/Clawy-g7

不要在 /Users/wykj/Projects/Clawy 主仓库里切换分支或修改文件。

先阅读：
- /Users/wykj/Projects/Clawy-g7/docs/clawy-bridge-design-requirements.md
- /Users/wykj/Projects/Clawy-g7/docs/clawy-bridge-tasklist.md
- /Users/wykj/Projects/Clawy-g7/docs/clawy-bridge-api-p0.md
- /Users/wykj/Projects/Clawy-g7/docs/clawy-bridge-event-mapping.md

任务：
- 为 P0 搭建最小验证方案
- 验证 node info/health、sessions、history、send、abort、WS events
- 验证未授权、Gateway 不可达、OpenClaw 未就绪等失败路径
- 产出联调记录和验收清单

文件所有权：
- /Users/wykj/Projects/Clawy-g7/src-tauri/tests/
- /Users/wykj/Projects/Clawy-g7/docs/bridge-validation-*.md

边界：
- 不替其他组重写生产代码
- 只在必要时补最小测试支撑
- 不为了测试通过去放宽协议或鉴权

要求：
- 覆盖成功路径和关键失败路径
- 优先写可复用 smoke/integration 测试
- 如果测试基础设施不够，只补最小可维护方案

验收：
- 能证明 P0 闭环成立
- 能明确指出剩余风险和后续 P1/P2 工作

最终输出：
1. 验证覆盖面
2. 实际跑过的命令
3. 失败案例是否符合预期
```

---

## 补充句

建议每次分发时都额外补一句：

```text
如果你发现当前任务需要改 OpenClaw 本体、OpenClaw 协议或桌面 UI 才能推进，不要自行扩 scope。请明确说明阻塞点、影响面和替代方案。
```
