# Hermes Runtime Plugin MVP 执行契约

日期：2026-05-31  
状态：Step 01 冻结基线  
来源：[hermes_runtime_plugin_design.md](hermes_runtime_plugin_design.md)、[plan/plan.md](plan/plan.md)、[plan/steps/01-contract-baseline.md](plan/steps/01-contract-baseline.md)

## 1. 契约目标

本文件冻结 Hermes Runtime Plugin MVP 的执行边界，供后续 Step 02-08 引用。它只记录当前阶段已经确认的产品语义、安全边界、兼容命名和代码缺口，不提前实现 profile、TUI Gateway、session 或真实消息外发。

## 2. 产品语义

- Hermes MVP 是消息驱动：controller DID 向 Runtime Agent DID 发送可执行文本消息，daemon 校验 controller 后把消息投递给 Hermes。
- controller 执行结果回传由 daemon host 完成：daemon 读取 Hermes TUI Gateway final output 后，先写入 `runtime_final_outbox`，再自动以 Runtime Agent DID 给 controller DID 发送普通消息；发送成功后才把 run 标记为 `Finished`。该链路不经过 Skill + CLI。
- Runtime Agent 主动向其他用户或群外发消息时，才走 Hermes Skill + `awiki-deamon-runtime send` + daemon local RPC。
- Runtime Agent 收件箱查看是 App <-> Daemon 管理查询能力：App 向 Daemon Agent DID 发送 `runtime.inbox.query` / `runtime.inbox.thread.query` 控制 payload，daemon 校验 controller 和 Runtime Agent 归属后读取 Runtime Agent 本地 IM 投影并回传 status payload。Hermes、Skill 和 Runtime Agent 推理过程不参与这条链路。
- 现有代码中的 `RuntimeTask`、`runtime_task`、`task.status`、`task.finish` 是 Generic CLI MVP 遗留的内部或 local RPC 兼容命名。Hermes Skill、Prompt Wrapper 和用户可见描述必须使用 message/run 语义，不扩大为完整 product task workflow。
- MVP 不新增 `task.result`，也不新增 `application/vnd.awiki...` 这类专用 content type。结构化命令和状态继续使用 `application/json + body.payload`。
- 非 controller 消息默认只能进入 inbox/projection，不能自动进入 Hermes 执行链。

## 3. Runtime 与 RPC 契约

- `runtime: "hermes"` 的 daemon runtime plugin id 固定为 `runtime.hermes`。
- `task.status` 和 `task.finish` 作为 local RPC 兼容方法保留；Hermes controller final 当前由 daemon host 自动发送，不要求 Hermes 通过 `task.finish` 回传普通最终回复。后续如果新增 `message.status` / `message.finish`，必须通过计划变更日志和独立步骤引入。
- `msg.send` 的目标契约是真实 ANP direct/group 普通消息；当前也承载“文本 caption + 附件”的同一条消息发送。Hermes Skill 和 `awiki-deamon-runtime send` 只支持普通消息，不暴露加密发送路径。把 `msg.send` 做成 status payload 或只发回 controller 的行为不得被记录为完成语义。
- local RPC 授权只信任 runtime RPC token 反查到的 `agent_did`、`runtime_profile_id`、`run_id`、允许方法和 recipient scope；请求体或 debug 字段中的同名字段不能参与授权。
- `allowed_recipients = Some([...])` 时，`msg.send` 必须限制目标；`allowed_recipients = None` 目前表示不限制 recipient，但 Hermes 后续步骤必须在 profile/policy 或 run 构造处明确是否允许开放发送。

## 4. Hermes 边界

- MVP 不安装 Hermes Python plugin，不写 `plugin.yaml`，不创建 `<HERMES_PROFILE_HOME>/plugins/awiki-runtime/` 作为能力入口。
- Awiki Skills 只提供行为说明和 wrapper 调用约定，不是安全边界。当前 Hermes profile 只安装 `awiki-outbound-messaging` 一个 Skill；旧的 `awiki-runtime`、`awiki-messaging`、`awiki-collaboration` 目录在安装时清理。
- Hermes 不持有 DID 私钥，不直接连接 message-service，不直接调用 `im-core`。所有远端通信必须经 daemon，由 daemon 使用受控 Agent DID 和 SDK 完成。
- Hermes profile 初始化不得写入长期可用的 `msg.send`、`task.finish` 或 future `message.finish` token。run token 只能由 daemon 在每次消息执行前按 run 签发。
- MVP 不实现 approval、sandbox/container、handle.resolve、Hermes-side `inbox.list`、Hermes-side `conversation.read` 或完整 task workflow。Agent 收件箱查看已经作为 App <-> Daemon control payload 能力落地，不是 Hermes Skill/tool 能力。高风险 shell/file write 不因 Hermes 接入而自动开放。

## 5. 当前代码基线

| 能力 | 当前证据 | Step 01 结论 |
|---|---|---|
| Runtime Agent 注册 | `commands::handle_agent_payload_message` 支持 `runtime.agent.create`，`agent::runtime_plugin_id("hermes")` 映射到 `runtime.hermes` | 作为 Step 02 profile 初始化入口。 |
| controller DID 校验 | agent command 校验 `message.sender_did == daemon_agent.controller_did`；文本执行经 `route_controller_text_task` 校验 `sender_did == profile.controller_did` | 后续 Hermes 路由不能绕过。 |
| runtime token | `RuntimeTokenScope`、`authorize_runtime_rpc`、audit log 已存在 | local RPC 可信上下文来自 token，不能信任请求体自报身份。 |
| 兼容 RPC 名称 | `RpcMethod::parse` 支持 `rpc.ping`、`task.status`、`task.finish`、`msg.send`、`artifact.created` | Step 04/05 可继续用兼容名，但文档和 prompt 使用 message/run 语义。 |
| `task.finish` failed final | `apply_runtime_rpc_side_effects` 当前把 `task.finish` 固定落到 `finished` | 属于后续缺口，不在 Step 01 修复。 |
| `msg.send` 真实外发 | `RuntimeMessageSend` 支持 direct/group target、文本、附件；Hermes 默认 recipient policy 只允许 `default_plain`；`ImCoreAgentOutbox` 转成 im-core `SendMessageRequest` | 已作为主动外发统一出口；controller final 仍由 daemon host 自动发回 controller。 |
| 长驻 runtime 路由 | foreground 文本消息仍使用 `UdsTestRuntimePlugin` | Step 07 才切换 `runtime.hermes` 路由。 |
| Hermes profile/Skills/TUI Gateway/session | 当前无 `hermes_profiles`、`hermes_native_sessions`、TUI Gateway runner 或 profile installer | 分别由 Step 02、03、06 实现。 |
| controller final durable outbox | `runtime_final_outbox` 持久化 final，`flush_runtime_final_outbox` 负责启动/循环补发，final 消息带稳定 idempotency key | 已作为 Hermes controller final 回传基础可靠性机制。 |

## 6. 后续步骤引用要求

- Step 02 只能安装 Hermes profile 与 Awiki Skills，不得安装 Hermes Python plugin 或写长期可写 token；当前只安装 `awiki-outbound-messaging`。
- Step 03 只能通过 trait/adapter 隔离 TUI Gateway 协议差异，真实 Hermes 不存在时用 fake gateway 验证 daemon 行为。
- Step 04 负责把 controller text/plain 包装为 message/run prompt，继续使用兼容 local RPC 方法名，但不引入 product task protocol。
- Step 05 必须把 `msg.send` 收敛为真实 ANP direct/group 普通消息外发，并支持文本与“caption + 附件”同一条消息；Hermes Skill 不得描述或暴露加密发送路径；记录 recipient scope、安全 review 和系统测试证据。
- Step 06 负责 Hermes native session 持久化，至少保存可提升为通用 session mapping 的 `runtime_session_id`。
- Step 07 才把 foreground runtime 路由从测试 runtime 切到 `runtime.hermes`。
- Step 08 必须在 `../awiki-system-test` 使用 remote 模式和 `awiki.info` 域名执行完整系统测试并记录统计。

## 7. 可验证检查

Step 01 引入 `crates/awiki-deamon/tests/hermes_contracts.rs`，用 focused contract tests 锁定以下稳定点：

- `runtime_plugin_id("hermes") == "runtime.hermes"`。
- `task.status`、`task.finish`、`msg.send` 仍是当前 local RPC 兼容方法；`message.status`、`message.finish`、`task.result` 尚未承诺。
- `msg.send` recipient scope 继续由 token 控制。
- 非 controller 文本消息不能进入 runtime 执行链。
- 本契约文档明确禁止 Hermes Python plugin、`plugin.yaml` 和伪 `msg.send`。
