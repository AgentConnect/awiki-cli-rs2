


# Generic CLI Runtime Plugin 设计方案

版本：v0.4 implementation checkpoint
日期：2026-06-01
适用范围：awiki daemon / ANP Agent Runtime Host / Codex CLI / Claude Code / Gemini CLI
定位：以当前 `awiki-deamon` 实现为基线、以 Codex CLI 为首个落地基准的通用 CLI Runtime Plugin 方案

---

## 0. 当前结论

本方案以当前 `awiki-deamon` 的实现为基线，而不是以一个理想化目标架构为基线。

CLI 类 runtime 的第一版目标是：

```text
Controller DID
  -> ANP direct/direct-e2ee message
  -> awiki daemon
  -> generic-cli runtime plugin
  -> Codex driver
  -> Codex CLI headless run
  -> daemon CLI wrapper
  -> daemon local RPC
  -> daemon runtime / IM Core SDK
  -> message-service
```

核心决策：

1. **CLI 家族只保留一个 daemon-side 通用 runtime plugin type。** 消息入口路由始终先按 `target_agent_did` / `agent_did` 找到 Runtime Agent；`generic-cli` 不是消息路由键，也不是某个插件对象实例 ID，而是 Agent 命中后用于 daemon runtime plugin 分发的持久化 plugin type。Codex、Claude Code、Gemini 是同一个插件实现下的 `driver_id` / profile 配置，不生成新的持久化 plugin type。当前代码字段名/接口名仍可能叫 `runtime_plugin_id` / `plugin_id()`，但本方案按 type/discriminator 语义使用它们。运行时可以为不同 profile/run 创建不同的 `GenericCliRuntimePlugin<Driver>` 对象实例，但这些实例的 `plugin_id()` 仍应返回 `generic-cli` 这个 type。
2. **首个实现基准改为 Codex CLI。** MVP 以 `codex exec` headless non-interactive 模式跑通 controller 消息、workspace-bound 执行、状态回传、最终回复和受控外发消息。
3. **允许 driver 内部有各自特殊实现。** 通用插件只统一 daemon 边界、run/token/workspace/callback 语义；Codex、Claude Code、Gemini 的命令参数、context 文件、输出解析、sandbox/profile、native state、甚至 driver-specific 数据表可以各自不同。
4. **driver-specific 不等于新 plugin type。** 如果某个 CLI 需要自己的原生状态、原生命令或原生 SQL/schema，也应放在 `generic-cli` 插件内部的 driver namespace 下，例如 `cli_driver_codex_*`、`cli_driver_gemini_*` 表或 `driver_state_json`，而不是新增 `runtime.cli.codex` / `runtime.cli.gemini` 持久化 plugin type。
5. **不做 MCP。** 本方案不设计 Awiki MCP Server、MCP tool callback 或 MCP 配置安装。CLI agent 回调 daemon 的主链路是 Awiki CLI wrapper + local RPC。
6. **daemon 是唯一 IM Core SDK 调用者。** Codex/Claude/Gemini 不持有 DID 私钥，不直连 message-service，不自行判断 controller DID。
7. **MVP 使用消息/run 语义，不扩大产品层 task 概念。** 当前代码中的 `RuntimeTask`、`runtime_task`、`task.status`、`task.finish` 是历史兼容命名。文档可以标注这些名字，但产品语义应描述为“消息执行过程”和“run”。
8. **`msg.send` 必须是真正发送 ANP direct/direct-e2ee 消息。** 当前 local RPC 已支持 profile/run recipient policy、非 controller DID/handle、handle resolve、resolved DID 授权、security mode 校验、send success/failure audit；foreground runtime outbox 经 IM Core 发送 direct text，测试 outbox 只记录内存。剩余缺口是独立 send result 表、更多系统测试证据和更完整的 direct-e2ee 验收，而不是把 `msg.send` 设计成 status payload 或第二条消息链路。
9. **`task.finish` 当前只表达成功 final。** 重复 final 已做幂等保护；失败 final 仍未实现，MVP 失败先使用 `task.status(state=failed)`。
10. **session 首版不超前抽象。** Codex baseline 首版采用 task-scoped synthetic session / transcript summary；不强制新增通用 `runtime_session_mapping`。后续 Claude Code native session 或 Codex resume 能力成熟后再补。
11. **worktree-per-task 是变更隔离，不是安全边界。** 硬安全边界只能来自 container / OS sandbox / runtime sandbox。`shared-root` 只能用于低风险个人场景。

---

## 1. 设计目标

### 1.1 产品目标

用户可以创建一个 CLI Runtime Agent，并把它绑定到某个 workspace / repo / worktree。Controller DID 给该 Agent 发消息后：

1. daemon 校验消息来自 controller DID。
2. daemon 创建 run、签发 scoped local RPC token。
3. `generic-cli` 插件选择 profile 中配置的 driver。
4. 首版 driver 为 Codex，启动 `codex exec` 执行消息。
5. Codex 在指定 workspace/worktree/sandbox 中工作。
6. Codex 通过 Awiki CLI wrapper 调 daemon local RPC 上报状态、最终回复、外发消息和 artifact。
7. daemon 负责 IM Core SDK 调用、run 状态、audit、消息投递和 controller 回复。
8. 创建 CLI Runtime Agent 时，`runtime=codex` / `runtime=claude-code` / `runtime=gemini` 只是 create command 的便捷 alias；daemon 持久化 `runtime_plugin_id=generic-cli`，并在 CLI profile 中保存 `driver_id`。

### 1.2 工程目标

`generic-cli` 插件需要：

1. 统一 Agent DID / Runtime Profile / Workspace Binding / Run / Token / Callback 的 daemon 边界。
2. 首先实现 Codex driver，以 Codex 的 headless non-interactive 模式作为通用 plugin 的验证基线。
3. 为 Claude Code、Gemini CLI 预留 driver extension point，但不把它们做成新 plugin。
4. 支持 driver-specific 命令生成、context 注入、输出解析、sandbox 参数和 native state。
5. 支持 workspace-bound 执行，写任务优先使用 worktree-per-task。
6. 保持 local RPC token 最小授权，不把 token 写入 prompt 明文、日志或持久 transcript。
7. 复用 Hermes 已采用的 daemon local RPC / CLI wrapper 边界，避免为 Codex/Claude/Gemini 再造一套 callback 协议。
8. 明确当前实现缺口，避免文档把未实现能力写成已完成能力。

### 1.3 非目标

MVP 不做：

1. CLI 家族多持久化 plugin type 并列注册，例如 `runtime.cli.codex`、`runtime.cli.claude_code`、`runtime.cli.gemini`。
2. Awiki MCP Server 或 MCP tool callback。
3. CLI agent 直接连接 message-service。
4. CLI agent 直接持有或使用 Agent DID 私钥。
5. CLI agent 自行判断 controller DID。
6. 交互式 TUI 自动化作为主链路。
7. 完整 native session 生命周期。
8. approval UI / approval service。
9. 默认 `danger-full-access`、bypassPermissions、yolo 模式。
10. 把 `shared-root` 或 `worktree-per-task` 当作硬安全边界。
11. 完整 task workflow / `task.result` 对外协议。

---

## 2. 当前实现基线

当前 daemon 已有的关键能力：

| 能力 | 当前状态 | 说明 |
|---|---|---|
| Runtime Agent Profile | 已有 | `RuntimeAgentProfile` 包含 `agent_did`、`controller_did`、`runtime_profile_id`、`runtime_plugin_id`、workspace 字段 |
| RuntimePlugin v1 | 已有 | 同步接口：`plugin_id` / `check_install_status` / `launch_run`；方法名是历史命名，当前返回值按 plugin type 使用 |
| Generic CLI 插件 | 已有 MVP | `GenericCliRuntimePlugin<D>`，`plugin_id()` 固定返回 `generic-cli` 这个 plugin type |
| GenericCliDriver | 已有 MVP | 当前只有 `check_install_status` / `run` |
| CommandGenericCliDriver | 已有 MVP | 启动一个本地 program，设置 run/task/token 相关 env |
| controller 校验 | 已有 | 当前主要校验 `sender_did == controller_did` |
| Runtime run 状态 | 已有 | `pending` / `running` / `finished` / `failed` |
| workspace binding | 已有 MVP | `workspace_id`、`workspace_root`、`workspace_mode` 必须成组出现 |
| workspace mode | 已有 | `shared-root`、`worktree-per-task`、`container`、`sandbox` |
| local RPC token | 已有 | 绑定 `agent_did` / `runtime_profile_id` / `run_id` / allowed methods / recipients / TTL |
| local RPC 方法 | 已有 | `rpc.ping` / `task.status` / `task.finish` / `msg.send` / `artifact.created` |
| token debug redaction | 已有 | `RuntimeRpcToken` 和 `RuntimeRpcRequest` debug 不打印 secret |
| audit_log | 已有 | local RPC authorize 成功/失败都会写 audit |
| CLI wrapper 请求结构 | 已有 MVP | `task_status` / `task_finish` / `msg_send` |
| Hermes Runtime Plugin | 已有 MVP | 独立 native runtime，`runtime_plugin_id = runtime.hermes`，不属于 CLI 家族 |
| Hermes local RPC 边界 | 已有 MVP | Hermes prompt 不携带 token，能力调用通过 daemon wrapper + local RPC |
| driver profile 表 | 已实现 MVP | `cli_runtime_profile` 保存 `driver_id`、binary/config、default sandbox/workspace mode、recipient policy 和 driver config |
| CLI agent create alias | 已实现 | `runtime=codex` / `codex-cli` / `claude-code` / `gemini` / `gemini-cli` 新建时写入 `generic-cli + driver_id`；legacy `runtime.cli.*` 仅用于迁移/alias |
| Generic CLI driver registry | 已实现 MVP | `GenericCliDriverRegistry` 按 `cli_runtime_profile.driver_id` 选择 `codex` / `command`；Claude Code / Gemini 当前返回未实现 |
| Codex driver | 已实现 MVP | `CodexDriver` 构造 `codex exec`、stdin prompt、env、输出路径和 fallback final；真实 Codex 主链路不依赖 callback 模拟 |
| prompt envelope | 已实现 MVP | Codex driver 通过 stdin 传 Awiki runtime context、message/run、callback rules 和 safety 约束 |
| output parser | 部分实现 | 已记录 stdout/stderr/observation JSONL/final output path，并支持 `--output-last-message` fallback final；尚未完整解析 Codex JSONL event |
| worktree 创建器 | 已实现 MVP | `shared-root` canonicalize；`worktree-per-task` 在 daemon runtime temp 下创建 per-run detached git worktree；container/sandbox 尚未实现 |
| runtime session 表 | 部分已有 | Hermes 已有 `hermes_native_sessions` 和 `runtime_session_id`；CLI 首版不强制新增通用 `runtime_session_mapping`，但 run metadata 应保留 route/session 字段 |
| CLI driver run metadata | 已实现 MVP | `cli_driver_run` 记录 route/session/workspace/command/output/fallback final metadata |
| `task.finish` failed final | 部分实现 | 当前 `task.finish` 总是落到 `finished`；重复 final 幂等已实现；失败 final 尚未实现 |
| `msg.send` 真实 ANP direct send | 已实现 MVP | local RPC 支持 handle resolve、recipient allowlist、resolved DID 授权、security mode、send result audit；foreground outbox 经 IM Core 发送 direct text；缺少独立 send result 表和 remote E2E 证据 |
| MCP | 不做 | 本方案删除该能力 |

当前 v1 `RuntimePlugin` 接口：

```rust
trait RuntimePlugin {
    fn plugin_id(&self) -> &str;
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome>;
}
```

当前 `GenericCliDriver` 接口：

```rust
trait GenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit>;
}
```

后续如果继续演进，应尽量在这两个 v1 接口上兼容扩展，而不是一开始替换成 async runner/session/cancel 的目标态接口。

---

## 3. 术语与边界

| 概念 | 所属层 | 含义 | 是否对 App 暴露 |
|---|---|---|---:|
| `agent_did` | ANP / Awiki | Runtime Agent 的对外通信身份 | 是 |
| `agent_handle` | ANP / Awiki | 指向 `agent_did` 的 handle | 是 |
| `controller_did` | daemon / user-service | 允许控制该 Agent 的 DID | 可展示 |
| `runtime_plugin_id` | daemon | 当前代码字段名；语义上是 Runtime plugin type / discriminator，本方案首版为 `generic-cli` | 不建议直接暴露 |
| `driver_id` | generic-cli plugin | 插件内部 driver，例如 `codex` / `claude-code` / `gemini` | debug 可见 |
| plugin implementation | Rust 代码实现 | 例如 `GenericCliRuntimePlugin`；可被多个 driver/profile 复用 | 否 |
| plugin instance | 运行时对象实例 | 某个 profile/run 绑定的 `GenericCliRuntimePlugin<Driver>` 或等价对象；不是持久化 ID | 否 |
| `runtime_profile_id` | daemon / plugin | 指向一个 CLI runtime profile | debug 可见 |
| `workspace_id` | daemon | workspace binding ID | debug 可见 |
| `workspace_root` | daemon / plugin | 用户绑定的 repo/workspace 根目录 | 不建议直接暴露 |
| `workspace_instance_path` | plugin | 一次 run 使用的 shared root、worktree 或 container mount path | debug 可见 |
| `run_id` | daemon | 一次消息执行过程的内部运行 ID | debug 可见 |
| `message_id` | ANP / daemon | 触发执行的消息 ID | 可见 |
| `RuntimeTask` | 当前代码兼容名 | 由 controller 消息转换出的内部执行输入 | 不作为产品概念扩展 |
| `Awiki CLI wrapper` | daemon / runtime bridge | CLI agent 可调用的本地命令壳，负责请求 daemon local RPC | 不对 App 暴露 |
| `local RPC` | daemon | runtime 回调 daemon 的本地接口 | 不对 App 暴露 |
| `driver_native_state` | plugin | 某个 driver 自己的 session、transcript、config 或 native metadata | 不对 App 暴露 |

命名约束：

1. 消息入口路由按 `target_agent_did` / `agent_did` 完成，不按 `runtime_plugin_id` 完成。
2. `runtime_plugin_id` 是 Agent 命中后的 runtime plugin type/discriminator 字段，不等同于 Rust 类名、对象实例 ID 或对象实例数量。
3. `GenericCliRuntimePlugin` 可以按 profile/run 创建多个对象实例，但 CLI 家族持久化的 `runtime_plugin_id` 仍统一为 `generic-cli`。
4. `driver_id` 才是 Codex / Claude Code / Gemini 的区分字段。
5. `runtime.hermes` 是 native runtime plugin type，不属于 CLI 家族，不受 `generic-cli` 单插件类型约束影响。

---

## 4. 总体架构

```text
awiki daemon
├── AgentRouter
│   └── target_agent_did / agent_did -> agent_definition
├── ControllerRouter
│   └── sender_did == controller_did
├── WorkspaceManager
│   ├── shared-root
│   ├── worktree-per-task
│   ├── container
│   └── sandbox
├── RuntimePluginRegistry
│   ├── generic-cli          # CLI family
│   ├── runtime.hermes       # native runtime, outside this design
│   └── other native runtime plugins
├── LocalRpcServer
│   └── daemon CLI wrapper callback
└── IM Core SDK

generic-cli plugin
├── GenericCliRuntimePlugin
├── CliRuntimeProfile
│   └── driver_id = codex | claude-code | gemini
├── DriverRegistry
│   ├── CodexDriver            # MVP baseline
│   ├── ClaudeCodeDriver       # 后续
│   └── GeminiCliDriver        # 后续
├── PromptEnvelopeBuilder
├── WorkspaceInstancePreparer
├── DriverCommandBuilder
├── DriverOutputParser
├── DriverNativeStateStore
└── SandboxLauncher
```

关键约束：

1. 消息入口先按 `target_agent_did` / `agent_did` 路由到 `agent_definition`，再读取该 Agent 绑定的 `runtime_profile_id`、`runtime_plugin_id` 和 CLI `driver_id`。
2. 对 CLI 家族来说，`RuntimePluginRegistry` 只需要看到 `generic-cli` 这一个 plugin type；Hermes 等 native runtime 仍可以拥有自己的 plugin type，例如 `runtime.hermes`。
3. `DriverRegistry` 是 `generic-cli` 插件内部机制。
4. `agent_definition.runtime_plugin_id` 首版保存 `generic-cli`。
5. `cli_runtime_profile.driver_id` 决定使用 `codex`、`claude-code` 还是 `gemini`。
6. driver 可以有自己的 native state 和数据表，但不得绕过 daemon 的 local RPC、token、controller、outbox 和 audit。

消息执行路由链路：

```text
incoming message.target_agent_did
  -> agent_definition(agent_did = target_agent_did)
  -> verify sender_did == agent_definition.controller_did
  -> runtime_profile(runtime_profile_id)
  -> RuntimePluginRegistry.get(runtime_plugin_id)
  -> if runtime_plugin_id == generic-cli: load cli_runtime_profile.driver_id
  -> GenericCliRuntimePlugin instance with selected driver
  -> launch run
```

因此，`generic-cli` 只作为 runtime plugin type 参与 RuntimePluginRegistry 分发，不参与外部 ANP 消息寻址。外部消息寻址始终由 DID 完成。

Agent create alias 规则：

```text
runtime.agent.create args.runtime = generic-cli
  -> runtime_plugin_id = generic-cli
  -> driver_id 来自 args.driver_id；MVP 可默认 codex，但推荐显式传入

runtime.agent.create args.runtime = codex | codex-cli
  -> runtime_plugin_id = generic-cli
  -> driver_id = codex

runtime.agent.create args.runtime = claude-code
  -> runtime_plugin_id = generic-cli
  -> driver_id = claude-code

runtime.agent.create args.runtime = gemini | gemini-cli
  -> runtime_plugin_id = generic-cli
  -> driver_id = gemini

runtime.agent.create args.runtime = hermes
  -> runtime_plugin_id = runtime.hermes
  -> driver_id = NULL
```

兼容策略：

1. 旧实现中已写入的 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 记录应作为 legacy plugin type 处理。
2. MVP 可以选择一次性迁移到 `runtime_plugin_id=generic-cli` 并补写 `cli_runtime_profile.driver_id`。
3. 如果暂不迁移，daemon registry 可临时把 legacy plugin type alias 到 `generic-cli`，但新写入数据不得继续产生 `runtime.cli.*`。
4. alias 只用于兼容旧数据，不代表新增 daemon-side plugin。

---

## 5. 数据模型

### 5.1 当前已有表

当前 daemon 已有基础表：

```text
agent_definition
runtime_profile
workspace_binding
runtime_task
runtime_run
runtime_rpc_tokens
audit_log
agent_identity
agent_auth_state
```

其中：

```text
agent_definition.runtime_plugin_id = generic-cli
runtime_profile.runtime_plugin_id = generic-cli
runtime_run.runtime_plugin_id = generic-cli
```

这些字段名虽然叫 `runtime_plugin_id`，但在本方案中承载的是 runtime plugin type。它们不应因为 Codex / Claude Code / Gemini 切换而变成不同 type。

### 5.2 CLI runtime profile 扩展

已在当前 `runtime_profile` 之外新增插件内部 profile 表。`runtime_profile.runtime_plugin_id` 仍保存 `generic-cli`，CLI driver 选择信息保存在本表：

```sql
CREATE TABLE cli_runtime_profile (
  runtime_profile_id TEXT PRIMARY KEY,
  driver_id TEXT NOT NULL,              -- codex / claude-code / gemini
  binary_path TEXT,
  config_home TEXT,
  auth_mode TEXT,                       -- user-local / api-key / managed
  default_model TEXT,
  default_sandbox TEXT,                 -- read-only / workspace-write / external-sandbox
  default_workspace_mode TEXT NOT NULL, -- shared-root / worktree-per-task / container / sandbox
  recipient_policy_json TEXT,           -- allowed DID/handle patterns, approval policy, defaults
  driver_config_json TEXT,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

说明：

1. `driver_id` 是插件内部路由字段，不是 runtime plugin type。
2. `driver_config_json` 保存 driver-specific 配置，例如 Codex profile、Claude settings、Gemini sandbox profile。
3. `recipient_policy_json` 保存 `msg.send` 的目标 DID / handle allowlist、默认安全模式、是否需要 approval 等策略。MVP 不做 approval UI 时，仍必须明确允许哪些 DID/handle。
4. 首版可以只支持 `driver_id = codex`，但表结构不应阻止后续加入 `claude-code` 和 `gemini`。

### 5.2.1 Runtime alias 解析结果

`runtime.agent.create` 不再只调用返回字符串的 `runtime_plugin_id(runtime)`。当前实现使用结构化解析结果：

```rust
struct RuntimeResolution {
    runtime_plugin_id: String,
    driver_id: Option<String>,
    legacy_runtime_plugin_id: Option<String>,
    defaulted_driver_id: bool,
}
```

解析规则：

```text
generic-cli                    -> runtime_plugin_id=generic-cli, driver_id=args.driver_id 或 codex
codex | codex-cli              -> runtime_plugin_id=generic-cli, driver_id=codex
claude-code                    -> runtime_plugin_id=generic-cli, driver_id=claude-code
gemini | gemini-cli            -> runtime_plugin_id=generic-cli, driver_id=gemini
hermes                         -> runtime_plugin_id=runtime.hermes, driver_id=NULL
其他显式 plugin type           -> 按 native/plugin-specific runtime 处理
```

当前写入要求：

1. 新创建的 Codex / Claude Code / Gemini agent 不得再写入 `runtime.cli.codex`、`runtime.cli.claude-code` 或 `runtime.cli.gemini-cli`。
2. 如果历史数据中存在这些 legacy plugin type，应在读取或迁移时转换为 `runtime_plugin_id=generic-cli` 和对应 `driver_id`。
3. `runtime=generic-cli` 且未提供 `driver_id` 时，MVP 可默认 `codex`，但对外命令帮助和 audit 需要记录该默认值。

### 5.3 Driver native state

如果某个 CLI 需要特殊 native state 或原生 SQL/schema，有两种允许方式：

方式一：统一 key/value 表：

```sql
CREATE TABLE cli_driver_native_state (
  runtime_profile_id TEXT NOT NULL,
  driver_id TEXT NOT NULL,
  state_key TEXT NOT NULL,
  state_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY(runtime_profile_id, driver_id, state_key)
);
```

方式二：同一插件内部的 driver-specific 表：

```text
cli_driver_codex_sessions
cli_driver_codex_runs
cli_driver_gemini_checkpoints
cli_driver_claude_sessions
```

约束：

1. 这些表仍属于 `generic-cli` 插件。
2. 表名必须带 `cli_driver_<driver_id>_` 前缀。
3. driver-specific 表只能保存 native metadata、transcript summary、command invocation、parser cache、event log path、session/checkpoint id 等。
4. Agent DID、controller、run 状态、token、audit、消息投递仍以 daemon core 表为事实源。

### 5.4 Run metadata

已新增轻量 run metadata 表，避免一开始引入通用 session 表：

```sql
CREATE TABLE cli_driver_run (
  run_id TEXT PRIMARY KEY,
  agent_did TEXT NOT NULL,
  runtime_profile_id TEXT NOT NULL,
  driver_id TEXT NOT NULL,
  controller_did TEXT NOT NULL,
  conversation_id TEXT,
  route_key TEXT NOT NULL,
  workspace_id TEXT,
  workspace_root TEXT,
  workspace_instance_path TEXT,
  workspace_mode TEXT,
  is_security_boundary INTEGER NOT NULL DEFAULT 0,
  command_json TEXT,
  output_json TEXT,
  final_output_path TEXT,
  native_session_id TEXT,
  synthetic_session_id TEXT,
  status TEXT NOT NULL,
  fallback_final_source TEXT,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

Codex MVP 中：

```text
native_session_id = NULL 或 codex resume id
synthetic_session_id = task-scoped id
route_key = cli:<agent_did>:<controller_did>:<conversation_id-or-no-conversation>:message-run
```

说明：

1. `agent_did`、`controller_did`、`conversation_id`、`route_key` 与 Hermes native session route 保持同一建模方向，避免后续 CLI native session 与 Hermes session 模型分裂。
2. Codex MVP 不要求复用长期 native session，但每个 run 必须能追溯触发 conversation 和 controller。

---

## 6. Driver 接口设计

### 6.1 当前 MVP 接口

当前代码已有：

```rust
pub trait GenericCliDriver {
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn run(&self, invocation: GenericCliInvocation) -> Result<GenericCliExit>;
}
```

Codex MVP 可以先沿用这个接口，实现一个 `CodexDriver`，内部完成：

1. 校验 `codex` binary。
2. 构造 prompt envelope。
3. 准备 workspace instance。
4. 通过 stdin 或受控临时文件传入 prompt。
5. 启动 `codex exec`。
6. 记录 JSONL/stdout/stderr/final output。
7. 真实运行时不通过 `RuntimeLaunchOutcome.callbacks` 模拟 status/final；Codex 进程内的 Awiki wrapper 通过 daemon local RPC 直接产生 side effect。
8. 根据 exit code、local RPC run 状态和 fallback final 文件返回 `GenericCliExit`。

### 6.2 目标接口

后续可以把 driver 拆成更清晰的阶段，但仍属于同一个 `generic-cli` plugin：

```rust
trait CliAgentDriver {
    fn driver_id(&self) -> &'static str;
    fn capabilities(&self) -> CliDriverCapabilities;

    fn check_install_status(&self, profile: &CliRuntimeProfile) -> Result<RuntimeInstallStatus>;
    fn check_auth_status(&self, profile: &CliRuntimeProfile) -> Result<CliAuthStatus>;
    fn prepare_workspace(&self, ctx: &CliWorkspaceContext) -> Result<WorkspaceInstance>;
    fn build_prompt(&self, ctx: &CliPromptContext) -> Result<PromptEnvelope>;
    fn build_command(&self, ctx: &CliCommandContext) -> Result<CommandSpec>;
    fn parse_event_line(&self, line: &str) -> Result<Option<CliRuntimeEvent>>;
    fn parse_final_output(&self, output: &ProcessOutput) -> Result<Option<CliFinalResult>>;
}
```

设计原则：

1. `CliAgentDriver` 只处理本地 CLI 的特殊性。
2. token 签发、controller 校验、run 状态、outbox、audit 不下放给 driver。
3. driver 不直接调用 IM Core SDK。
4. driver 不读取 daemon 私钥。
5. driver 不决定某个外部消息是否可执行。
6. driver 不应伪造 `RuntimeLaunchOutcome.callbacks` 作为主链路；该字段只保留给测试 driver 或兼容 fallback。

---

## 7. Codex Driver Baseline

### 7.1 推荐运行模式

Codex MVP 使用 `codex exec` non-interactive mode。

只读分析：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox read-only \
  --ephemeral \
  --json \
  --output-last-message <final-output.txt> \
  -
```

允许写入 worktree：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox workspace-write \
  --ephemeral \
  --json \
  --output-last-message <final-output.txt> \
  -
```

结构化最终输出：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox workspace-write \
  --ephemeral \
  --output-schema <awiki-final.schema.json> \
  --output-last-message <final-output.json> \
  -
```

说明：

1. `-` 表示 prompt 从 stdin 读取，避免把完整 prompt 放到 shell argv。
2. `--json` 用于 JSONL event observation，不作为唯一事实源。
3. `--output-last-message` 用于 fallback final，不替代 `task.finish` callback。
4. `--output-schema` 只约束最终输出形状，不承担 daemon 安全边界。
5. `--ephemeral` 用于避免 Codex 默认持久化 session 文件；如果 driver 明确需要 native resume，应显式关闭并记录 session metadata。
6. `--ignore-user-config`、`--ignore-rules` 可用于更强的 daemon-managed profile 场景；如果启用用户本地配置，必须在 `driver_config_json` 和 audit 中记录。
7. Codex CLI 原生 `--sandbox` 只接受 `read-only`、`workspace-write`、`danger-full-access`。文档中的 `external-sandbox` 是 Awiki 层抽象，不能直接作为 Codex 参数传入。
8. `danger-full-access` 和 `--dangerously-bypass-approvals-and-sandbox` 只能在外部 container/VM 已经提供隔离时显式启用，不能作为默认。

### 7.2 Codex context 注入

首版不要默认修改用户 repo 的 `AGENTS.md`。推荐优先级：

```text
1. prompt envelope via stdin
2. per-run env vars
3. isolated CODEX_HOME / profile config
4. worktree 内临时 context 文件
5. 用户明确允许后再写入项目规则文件
```

Codex driver 可使用：

1. `--cd <workspace_instance_path>` 固定工作根。
2. `--profile <name>` 或 `--config key=value` 注入 driver 配置。
3. `--ignore-user-config` / isolated `CODEX_HOME` 控制是否读取用户本地配置。
4. `--ignore-rules` 控制是否读取用户或项目 execpolicy rules；默认策略需要由 profile 明确声明。
5. `--sandbox read-only` / `--sandbox workspace-write` 控制工具执行权限。
6. `--output-schema` 约束最终输出。
7. `--json` 观察事件。
8. `--output-last-message` 保存最终消息。

### 7.3 Codex prompt envelope

daemon 为每个 run 生成标准 prompt envelope：

```text
[Awiki Runtime Context]
agent_did: did:agent:alice-coder
agent_handle: @alice-coder
runtime_plugin_id: generic-cli
driver_id: codex
workspace_id: workspace_awiki
workspace_mode: worktree-per-task
workspace_instance_path: <path>

[Controller]
sender_did: did:human:alice
controller_verified: true

[Message Run]
message_id: msg_123
run_id: run_task_msg_123
conversation_id: conv_123
user_message:
...

[Awiki Callback Rules]
- 不要直接连接 message-service。
- 不要尝试读取或使用 DID 私钥。
- 需要回传进度时，调用 daemon CLI wrapper 的 status 命令。
- 需要提交最终回复时，调用 daemon CLI wrapper 的 finish 命令。
- 需要给其他 DID/handle 发送消息时，调用 daemon CLI wrapper 的 send-message 命令。
- 如果 wrapper 调用失败，在最终输出中明确报告失败，不要伪造发送成功。

[Safety]
- 不读取 secrets、private keys、.env、credential stores。
- 不运行 destructive shell。
- 不使用未授权网络访问。
- 如果需要更高权限，先上报状态并等待 controller。
```

### 7.4 Codex callback env

Codex 应复用 Hermes 的 daemon CLI wrapper + local RPC 边界。prompt 只告诉 Codex 使用 wrapper 能力，token/socket 等能力凭据通过 env、fd 或受控临时 credential 文件注入，不进入 prompt 文本。

MVP 需要注入：

```text
AWIKI_DAEMON_RUN_ID
AWIKI_DAEMON_TASK_ID
AWIKI_DAEMON_RUNTIME_RPC_TOKEN
AWIKI_DAEMON_SOCKET
AWIKI_DAEMON_AGENT_DID
AWIKI_DAEMON_RUNTIME_PROFILE_ID
AWIKI_DAEMON_CLI_WRAPPER
```

约束：

1. 不再向真实 Codex run 注入 `AWIKI_DAEMON_TASK_TEXT`；用户消息只通过 stdin prompt envelope 进入 Codex，避免进程环境泄漏完整消息。
2. token 可以通过 env、fd 或权限受控的临时 credential 文件注入，但不能放入 prompt 文本、stdout/stderr、JSONL、final output 或持久 transcript。
3. wrapper 命令负责读取 token/socket 并构造 local RPC request；Codex 不应自己拼接 JSON-RPC token 到日志。
4. wrapper 与 Hermes 复用同一套 local RPC 协议和方法名：`task.status`、`task.finish`、`msg.send`、`artifact.created`、`rpc.ping`。
5. run 结束后 token 必须过期或 revoke，并在 audit / run metadata 中记录证据。

### 7.4.1 真实回调主链路

真实 Codex 回调只走一条链路：

```text
Codex
  -> daemon CLI wrapper
  -> daemon local RPC over UDS
  -> token / method / recipient 授权
  -> runtime run 状态 / audit / outbox
  -> IM Core SDK / message-service
```

约束：

1. 真实 Codex driver 不应把 `RuntimeLaunchOutcome.callbacks` 当作 status/final 主链路。
2. `RuntimeLaunchOutcome.callbacks` 只允许用于 test driver、内存测试或历史兼容 fallback。
3. daemon local RPC server 必须在 Codex 进程运行期间可用。
4. Codex 进程成功退出但没有 `task.finish` 时，daemon 可以读取 `--output-last-message` 作为 fallback final，但必须标记来源为 fallback，并先检查是否已有 final。
5. `task.finish` / fallback final 需要幂等保护，避免重复向 controller 发送最终回复。

### 7.5 Codex 输出事实源

事实源优先级：

```text
daemon local RPC task.finish
  > structured final output file
  > Codex final message file
  > process exit code
  > timeout fallback
```

说明：

1. `task.finish` 成功时，daemon run 状态为 `finished`。
2. `task.status(state=failed)` 可表示失败，但当前没有 failed final。
3. Codex JSONL event 可用于 observation/audit，但不能替代 local RPC 授权。
4. 如果 Codex 进程成功退出但没有 final callback，daemon 可以用 final output file 生成 fallback final，但必须标记来源。

---

## 8. 后续 Driver 微调

### 8.1 Claude Code Driver

Claude Code 后续作为 `generic-cli` 内部 driver 引入：

```text
runtime_plugin_id = generic-cli
driver_id = claude-code
```

可复用的通用部分：

1. Agent DID / controller DID / runtime profile。
2. workspace binding。
3. prompt envelope 的 Awiki 语义。
4. local RPC token。
5. daemon CLI wrapper。
6. run 状态、audit、outbox。

Claude Code 特有部分：

1. `claude -p` / `--print`。
2. `--output-format stream-json`。
3. `--session-id` / `--resume` / `--continue`。
4. `--settings`。
5. `--permission-mode`。
6. `--worktree`。
7. Claude-specific event parser。

注意：

1. Claude Code 的 native session 可以作为后续增强，不阻塞 Codex MVP。
2. 不新增 `runtime.cli.claude_code` runtime plugin type。
3. 不把 Claude Code memory/context 当作安全边界。

### 8.2 Gemini CLI Driver

Gemini CLI 后续作为 `generic-cli` 内部 driver 引入：

```text
runtime_plugin_id = generic-cli
driver_id = gemini
```

可复用的通用部分同上。

Gemini 特有部分：

1. `gemini -p` headless mode。
2. `--output-format stream-json` 或 final JSON。
3. `GEMINI.md` context 规则。
4. Gemini-specific sandbox profile。
5. Gemini checkpoint / saved chat metadata。
6. Gemini-specific event parser。

注意：

1. Gemini sandbox 能力可以接入 `WorkspaceMode::Sandbox`，但必须明确依赖具体 sandbox profile。
2. 不新增 `runtime.cli.gemini` runtime plugin type。

---

## 9. Workspace 模型与安全边界

### 9.1 shared-root

```text
workspace_instance_path = workspace_root
```

定位：个人低风险便利模式。

风险：

1. 不是硬隔离。
2. CLI agent 仍可能访问当前用户可访问的其他路径。
3. 容易污染用户当前工作区。

适用：

1. 只读分析。
2. controller 明确允许。
3. 本机可信。
4. 不执行外部委托。

### 9.2 worktree-per-task

```text
workspace_instance_path = <daemon_state_root>/runtime/tmp/worktrees/<workspace_id>/<run_id>/
```

定位：写代码任务默认建议模式。

优点：

1. 避免污染主 workspace。
2. 支持并发 run。
3. 方便 diff / rollback / audit。
4. 适合 Codex `--cd <worktree>`、Claude Code `--worktree`、Gemini git worktree 或 daemon 自建 worktree。

边界：

1. 只隔离代码变更。
2. 不防止读取用户级凭据。
3. 不限制网络。
4. 不限制系统命令能力。
5. 不是安全边界。

实现要求：

1. worktree 路径必须位于 daemon-managed `runtime_temp_dir` 或 `runtime_cache_dir` 下，不写死 `~/.awiki/worktrees`。
2. daemon 必须校验 worktree path 在 state root 管辖范围内，避免路径穿越或覆盖用户目录。
3. 如果 `workspace_root` 不是 git repo，MVP 应明确失败、回退到 `shared-root` 只读，或使用 copy workspace；不得静默污染原 workspace。
4. 每个 run 使用唯一 worktree path，支持并发 run。
5. 清理策略必须明确：成功后可保留用于 audit/diff，失败后默认保留，超过 TTL 后由 cleanup job 清理。
6. run metadata 需要记录 `workspace_instance_path`、base ref、created commit/branch、清理状态和 diff summary。

### 9.3 container / sandbox

定位：可作为硬安全边界，但依赖具体实现。

适用：

1. 外部 agent 委托任务。
2. 高风险 shell。
3. 需要运行不可信测试。
4. 需要限制环境变量、凭据、网络和文件系统。

要求：

```text
- 清理敏感 env
- 只挂载 workspace/worktree
- 禁止挂载 ~/.ssh、~/.aws、~/.config 等
- 按 run 注入最小 RPC token
- 禁止复用长寿命 credential
- 明确网络策略
- 明确出站消息策略
```

---

## 10. Local RPC 与消息语义

### 10.1 回调链路

```text
Codex / Claude / Gemini
  -> daemon CLI wrapper
  -> daemon local RPC
  -> token 校验
  -> run 状态 / audit / outbox
  -> IM Core SDK
```

CLI agent 不能直接调用 IM Core SDK。

### 10.2 local RPC 方法

当前兼容方法：

| 方法 | 产品语义 | 当前状态 | 备注 |
|---|---|---|---|
| `rpc.ping` | local RPC health | 已有 | read level |
| `task.status` | 当前消息 run 状态 | 已有 | 历史兼容名 |
| `task.finish` | 当前消息 run 成功 final | 已有 | 当前总是 `finished` |
| `msg.send` | 发送 ANP direct/direct-e2ee 消息 | 已实现 MVP | 支持 authorized DID/handle、handle resolve、send audit；foreground 经 IM Core 外发 |
| `artifact.created` | artifact 报告 | 入口已有 | 后续增强 |

不新增 `task.result` 作为 MVP 对外协议。

### 10.3 `msg.send` 必须语义

```text
CLI agent
  -> daemon CLI wrapper send-message --to <did-or-handle> --text "..."
  -> daemon local RPC msg.send
  -> daemon resolve recipient
  -> daemon 校验 run token / method / resolved recipient scope
  -> IM Core SDK direct.send 或 direct-e2ee send
  -> message-service
  -> 目标 DID
```

这是必须语义。不能把 `msg.send` 实现成仅向 controller 发送 status payload，也不能只记录内存 outbox 后宣称发送成功。

Recipient scope 要求：

1. `msg.send` 已支持发送给 controller 以外的 DID / handle，这是 CLI agent 协作能力的一部分。
2. local RPC token 不固定只允许 `controller_did`；`allowed_recipients` 从 CLI runtime profile 的 recipient policy 生成，默认仍是 `controller-only`。
3. 如果输入是 handle，daemon 会先通过 outbox resolver resolve 为 DID，再对 resolved DID 和原始 handle 做授权检查。
4. `allowed_recipients` 已支持精确 DID、精确 handle、profile allowlist 和 run token scope。MVP 不支持通配全网默认放开。
5. 如果目标不在 allowlist 中，wrapper 返回明确失败，Codex 不得声称消息已发送。
6. audit 已记录原始 recipient、resolved DID、security mode、授权结果、send result message id 或失败原因。当前还没有独立 send result 持久表。
7. `security` 默认为 `default_plain`，可显式指定 `direct_e2ee`；是否允许由 token/profile policy 控制。

### 10.4 `task.finish` 当前限制

```text
CLI agent
  -> daemon CLI wrapper finish --text "..."
  -> daemon local RPC task.finish
  -> daemon 标记 run finished
  -> daemon 向 controller 发送最终回复消息
```

当前限制：

1. `task.finish` 只表达 successful finish。
2. 失败时先调用 `task.status(state=failed)`。
3. 后续需要补 `finish(state=failed)` 或新的 message-level final 语义。
4. 重复 final 已具备幂等保护，已 finished 的 run 再收到 `task.finish` 不会重复发送最终回复。
5. final 回复仍通过 daemon local RPC -> outbox -> IM Core SDK 主链路发送，不新增 Codex 到 message-service 的直接链路。

---

## 11. Run 状态与输出解析

### 11.1 当前状态

当前代码已有：

```text
pending
  -> running
  -> finished
  -> failed
```

当前 Generic CLI test driver / command driver 行为：

1. driver exit code 为 0 时，生成 `running` status 和 `task.finish` callback。
2. driver exit code 非 0 时，生成 `failed` status callback。
3. `task.finish` side effect 会把 run 标为 `finished`。

真实 Codex driver 当前行为：

1. Codex 进程运行期间由 wrapper 调用 local RPC 上报 status/final/msg.send。
2. driver exit code 只作为 fallback 信号，不直接伪造成功 final。
3. `RuntimeLaunchOutcome.callbacks` 不作为真实 Codex 主链路，只作为测试 driver 或 legacy fallback。
4. 如果进程成功退出且 run 仍未 final，daemon 根据 `--output-last-message` final output file 生成 fallback final，并在 `cli_driver_run.fallback_final_source` 标记来源。

### 11.2 目标状态

后续可以扩展为：

```text
pending
  -> prepared
  -> submitted
  -> running
  -> status_reported*
  -> finishing
  -> finished
  -> failed / cancelled / timeout
```

但 Codex MVP 不要求一次性完成全部状态。

### 11.3 输出处理

| 输出来源 | 用途 | 是否事实源 |
|---|---|---:|
| daemon local RPC status | run 状态、controller 进度 | 是 |
| daemon local RPC finish | final reply | 是 |
| daemon local RPC msg.send | 外发消息请求 | 是 |
| Codex JSONL event | observation / audit / debug | 否 |
| final output file | fallback final | 条件事实源 |
| stdout/stderr | diagnostic | 否 |
| process exit code | failed fallback | 条件事实源 |

---

## 12. Session 策略

### 12.1 Codex MVP

Codex MVP 使用 task-scoped synthetic session：

```text
synthetic_session_id = synth_<run_id>
native_session_id = optional
```

保存内容：

1. trigger message summary。
2. prompt envelope hash。
3. final output summary。
4. workspace diff summary。
5. driver command metadata。
6. event log path。
7. route metadata：`agent_did`、`controller_did`、`conversation_id`、`route_key`。

不要求：

1. 通用 `runtime_session_mapping` 表。
2. 长期 workspace-scoped native session。
3. 自动 resume。
4. 跨 conversation 记忆。

与 Hermes 对齐：

1. Hermes 已有 native session route：按 `agent_did`、`runtime_profile_id`、`controller_did`、`conversation_id`、`session_kind` 生成 route key。
2. CLI Codex MVP 不需要立刻引入通用 `runtime_session_mapping`，但 `cli_driver_run` 必须保存同方向 route metadata。
3. 后续如果 Codex resume、Claude `--session-id`、Gemini checkpoint 都需要稳定路由，再抽象跨 runtime 的 `runtime_session_mapping`。

### 12.2 后续 native session

后续 driver 可逐步增加 native session：

| driver | native session 方向 | 是否阻塞 MVP |
|---|---|---:|
| Codex | `codex exec resume` / session files / transcript | 否 |
| Claude Code | `--session-id` / `--resume` / `--continue` | 否 |
| Gemini | checkpoint / saved chat | 否 |

当多个 driver 都需要稳定 session routing 时，再设计通用 `runtime_session_mapping`，并以 Hermes 已验证的 route key 模型为参考。

---

## 13. Driver 能力矩阵

| 能力 | Codex CLI | Claude Code | Gemini CLI |
|---|---|---|---|
| MVP 顺序 | 第一优先级 | 第二阶段 | 第三阶段 |
| Runtime Plugin Type | `generic-cli` | `generic-cli` | `generic-cli` |
| Driver ID | `codex` | `claude-code` | `gemini` |
| Headless | `codex exec` | `claude -p` / `--print` | `gemini -p` |
| JSON/event | `--json` JSONL | `--output-format stream-json` | `--output-format stream-json` |
| Final output | `--output-last-message` / `--output-schema` | `--output-format json` 或 stdout | JSON/final stdout |
| Workspace | `--cd` | cwd / `--worktree` | cwd / worktree |
| Context | prompt envelope / profile config / rules | prompt envelope / settings / CLAUDE.md | prompt envelope / GEMINI.md |
| Sandbox | `read-only` / `workspace-write` / external sandbox | permission mode / external sandbox | runtime sandbox / external sandbox |
| Session | task-scoped synthetic first | native session later | synthetic/checkpoint later |

---

## 14. 落地步骤

当前实现进度：

| 阶段 | 当前状态 | 说明 |
|---|---|---|
| Phase 0 | 已实现 MVP | runtime alias、`cli_runtime_profile`、legacy `runtime.cli.*` 迁移、`generic-cli` type 语义和 MCP 非目标已落地 |
| Phase 1 | 已实现 MVP | `CodexDriver`、`codex exec` command builder、stdin prompt envelope、env 注入、shared-root、worktree-per-task、stdout/stderr/JSONL/final output path 已落地 |
| Phase 2 | 已实现 MVP | wrapper/local RPC、token redaction、recipient policy、handle resolve、resolved DID 授权、send audit、final 幂等已落地；failed final 未落地 |
| Phase 3 | 部分实现 | workspace instance、sandbox 参数映射、`cli_driver_run`、route/session metadata 和 fallback final 已落地；container/sandbox 硬隔离、完整 Codex JSONL parser、cleanup job 和 token revoke 证据仍待后续 |
| Phase 4 | 未实现 | Claude Code driver 仍是 extension point |
| Phase 5 | 未实现 | Gemini CLI driver 仍是 extension point |
| Phase 6 | 未实现 | cancellation、artifact reporting、stable native session mapping、container sandbox 和更多 system-test 覆盖待后续 |

### Phase 0：对齐当前实现基线

1. 保留 `GenericCliRuntimePlugin` 作为唯一 CLI runtime plugin。
2. 明确 `runtime_plugin_id = generic-cli`。
3. 修改 runtime alias 解析：`runtime=codex|claude-code|gemini` 新写入 `runtime_plugin_id=generic-cli`，并保存对应 `driver_id`。
4. 增加 `cli_runtime_profile` 或等价 driver profile 存储，至少保存 `driver_id`、driver config 和 recipient policy。
5. 对旧的 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 设计迁移或 plugin type alias。
6. 记录当前 `RuntimePlugin v1`、`GenericCliDriver`、workspace binding、local RPC token、outbox 缺口。
7. 删除 MCP 相关设计和落地项。
8. 明确 `task.status` / `task.finish` 是兼容 RPC 名称，不是产品层 task workflow。
9. 明确 `msg.send` 已有 foreground IM Core 发送主链路，当前已补 recipient allowlist、handle resolve、resolved DID 授权、send audit 和 final 幂等；仍需补独立 send result 表和 remote 系统测试证据。

### Phase 1：Codex Driver MVP

1. 新增 `CodexDriver`，仍注册在 `generic-cli` 插件内部。
2. 实现 installation check：查找 `codex` binary，记录版本和路径。
3. 实现 Codex command builder：`codex exec --cd ... --sandbox ... --json ... -`。
4. 实现 prompt envelope builder。
5. 通过 stdin 传 prompt，不把完整 prompt 放到 shell argv。
6. 注入 local RPC 所需 env：`AWIKI_DAEMON_SOCKET`、`AWIKI_DAEMON_RUNTIME_RPC_TOKEN`、`AWIKI_DAEMON_RUN_ID`、`AWIKI_DAEMON_AGENT_DID`、`AWIKI_DAEMON_RUNTIME_PROFILE_ID`、`AWIKI_DAEMON_CLI_WRAPPER`；不要注入 `AWIKI_DAEMON_TASK_TEXT`。
7. 支持 shared-root 只读分析。
8. 支持 worktree-per-task 写任务。
9. 记录 stdout/stderr/JSONL/final output path。
10. 保持 process exit failed fallback，但不伪造 success final。

### Phase 2：Codex 回调闭环与 outbox

1. 复用 Hermes 的 daemon CLI wrapper + local RPC 协议，完善 status / finish / send-message / artifact 命令。
2. 让 Codex prompt 明确调用 wrapper，但不暴露 token/socket 细节。
3. 确保 local RPC token 不进入 prompt 明文、日志、JSONL、final output 或持久 transcript。
4. 对 `msg.send` 增加 profile/run 级 recipient allowlist、handle resolve、resolved DID 授权和 send result audit。
5. 确认 final 回复到 controller 走 daemon local RPC -> outbox -> IM Core SDK 主链路。
6. 增加 failed final 或记录当前失败限制。
7. 增加重复 final 防护。

### Phase 3：Codex 安全与可观测性

1. worktree-per-task 自动创建已实现；cleanup job 未实现，当前默认保留 worktree 作为证据。
2. Codex `read-only` / `workspace-write` 策略映射已实现。
3. external container/sandbox 接入点。
4. Codex JSONL event observation path 已记录；完整 event parser 和 audit 摘要未实现。
5. driver run metadata 表已实现。
6. route/session metadata 与 Hermes session route 模型对齐已实现 MVP。
7. token expiry 已由 runtime token scope 约束；显式 revoke 证据仍待后续。

### Phase 4：Claude Code Driver

1. 在同一 `generic-cli` 插件内新增 `ClaudeCodeDriver`。
2. 复用 prompt envelope、workspace、token、wrapper、outbox。
3. 只新增 Claude-specific command builder、permission mapping、event parser、native session metadata。
4. 不新增 `runtime.cli.claude_code` runtime plugin type。

### Phase 5：Gemini CLI Driver

1. 在同一 `generic-cli` 插件内新增 `GeminiCliDriver`。
2. 复用 prompt envelope、workspace、token、wrapper、outbox。
3. 只新增 Gemini-specific command builder、sandbox mapping、event parser、checkpoint metadata。
4. 不新增 `runtime.cli.gemini` runtime plugin type。

### Phase 6：统一增强

1. cancellation。
2. artifact reporting。
3. stable native session mapping。
4. container sandbox。
5. driver-specific native state migrations。
6. release/system-test 覆盖。

---

## 15. 验收标准

Codex MVP 完成时应满足：

| 标准 | 当前状态 | 说明 |
|---|---|---|
| `runtime_plugin_id` 仍为 `generic-cli` | 已满足 | CLI family 新写入统一使用 `generic-cli` runtime plugin type |
| `runtime.agent.create(runtime=codex|codex-cli)` 新写入 `generic-cli + driver_id=codex` | 已满足 | alias 解析和 create path 已覆盖 |
| `runtime.agent.create(runtime=generic-cli)` 可显式 `driver_id=codex` | 已满足 | 未显式时默认 `codex`，并记录 `defaulted_driver_id` |
| controller DID 消息可以触发 Codex headless run | 已满足 MVP | fake Codex binary 测试覆盖；真实 Codex smoke 仍按环境可用性记录 |
| 非 controller 消息不会进入执行链 | 已满足 | controller 校验保持在 daemon 层 |
| Codex 只在指定 workspace instance 中运行 | 已满足 MVP | `shared-root` / `worktree-per-task` 已实现；container/sandbox 未实现 |
| local RPC token 绑定 run、method、recipient scope 和 TTL | 已满足 | recipient scope 支持授权 controller 以外 DID/handle |
| token 不出现在 debug log、prompt、stdout/stderr、JSONL、final output 和持久 transcript 中 | 已满足 MVP | 通过 redaction、prompt token 检查和 grep gate 验证；没有持久 transcript 表 |
| Codex 通过 Hermes 同源 wrapper + local RPC 上报状态/最终回复/外发消息 | 已满足架构和 fake binary contract | 真实 Codex 是否实际调用 wrapper 取决于 prompt/runtime 行为，需 remote/smoke 持续验证 |
| 真实 Codex driver 不依赖 `RuntimeLaunchOutcome.callbacks` 模拟 status/final | 已满足 | Codex driver 返回空 callbacks，daemon 支持 fallback final |
| Codex 可以提交最终回复，且 final 具备重复防护 | 已满足 MVP | `task.finish` 幂等；无 finish 时成功退出可用 `--output-last-message` fallback |
| `msg.send` 真实走 daemon / IM Core SDK 外发链路，并记录 handle resolve、recipient 授权、security mode 和 send result 证据 | 已满足 MVP | local RPC + Memory/IM Core outbox 已实现；独立 send result 表和 remote E2E 证据仍待补 |
| `worktree-per-task` 被记录为变更隔离，不被宣传为安全边界 | 已满足 | worktree 位于 daemon-managed runtime temp，metadata 记录 `is_security_boundary=false` |
| Codex run metadata 保存 route/session 字段 | 已满足 MVP | `cli_driver_run` 保存 route key、synthetic session、workspace、command、output 和 fallback final source |
| 文档和测试都不依赖 MCP | 已满足 | MCP 明确非目标 |
| 后续 Claude Code / Gemini 只需在同一 plugin type 内新增 driver | 已满足架构 | 当前 driver registry 对两者返回未实现，不新增持久化 plugin type |

---

## 16. 参考链接

- Codex CLI: https://developers.openai.com/codex/cli
- Codex CLI command options: https://developers.openai.com/codex/cli/reference
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive
- Codex permissions: https://developers.openai.com/codex/permissions
- Claude Code CLI reference: https://docs.anthropic.com/en/docs/claude-code/cli-reference
- Claude Code memory: https://docs.anthropic.com/en/docs/claude-code/memory
- Claude Code permissions: https://docs.anthropic.com/en/docs/claude-code/iam
- Gemini CLI GitHub: https://github.com/google-gemini/gemini-cli
