


# Generic CLI Runtime Plugin 设计方案

版本：v0.2 draft
日期：2026-05-31
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

1. **只保留一个 daemon-side 通用插件。** 首版插件 ID 沿用当前实现的 `generic-cli`。Codex、Claude Code、Gemini 是同一个插件内部的 `driver_id` / profile 配置，不生成新的 plugin。
2. **首个实现基准改为 Codex CLI。** MVP 以 `codex exec` headless non-interactive 模式跑通 controller 消息、workspace-bound 执行、状态回传、最终回复和受控外发消息。
3. **允许 driver 内部有各自特殊实现。** 通用插件只统一 daemon 边界、run/token/workspace/callback 语义；Codex、Claude Code、Gemini 的命令参数、context 文件、输出解析、sandbox/profile、native state、甚至 driver-specific 数据表可以各自不同。
4. **driver-specific 不等于新 plugin。** 如果某个 CLI 需要自己的原生状态、原生命令或原生 SQL/schema，也应放在 `generic-cli` 插件内部的 driver namespace 下，例如 `cli_driver_codex_*`、`cli_driver_gemini_*` 表或 `driver_state_json`，而不是新增 `runtime.cli.codex` / `runtime.cli.gemini` plugin。
5. **不做 MCP。** 本方案不设计 Awiki MCP Server、MCP tool callback 或 MCP 配置安装。CLI agent 回调 daemon 的主链路是 Awiki CLI wrapper + local RPC。
6. **daemon 是唯一 IM Core SDK 调用者。** Codex/Claude/Gemini 不持有 DID 私钥，不直连 message-service，不自行判断 controller DID。
7. **MVP 使用消息/run 语义，不扩大产品层 task 概念。** 当前代码中的 `RuntimeTask`、`runtime_task`、`task.status`、`task.finish` 是历史兼容命名。文档可以标注这些名字，但
  品语义应描述为“消息执行过程”和“run”。
8. **`msg.send` 必须是真正发送 ANP direct/direct-e2ee 消息。** 如果当前实现仍只落到内存 outbox 或 status payload，这是实现缺口，后续必须补齐到 IM Core SDK direct/direct-e2ee send。
9. **`task.finish` 当前只表达成功 final。** 失败 final、幂等 final、重复 final 去重是后续缺口；MVP 失败先使用 `task.status(state=failed)`。
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

### 1.2 工程目标

`generic-cli` 插件需要：

1. 统一 Agent DID / Runtime Profile / Workspace Binding / Run / Token / Callback 的 daemon 边界。
2. 首先实现 Codex driver，以 Codex 的 headless non-interactive 模式作为通用 plugin 的验证基线。
3. 为 Claude Code、Gemini CLI 预留 driver extension point，但不把它们做成新 plugin。
4. 支持 driver-specific 命令生成、context 注入、输出解析、sandbox 参数和 native state。
5. 支持 workspace-bound 执行，写任务优先使用 worktree-per-task。
6. 保持 local RPC token 最小授权，不把 token 写入 prompt 明文、日志或持久 transcript。
7. 明确当前实现缺口，避免文档把未实现能力写成已完成能力。

### 1.3 非目标

MVP 不做：

1. 多 plugin 并列注册，例如 `runtime.cli.codex`、`runtime.cli.claude_code`、`runtime.cli.gemini`。
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
| RuntimePlugin v1 | 已有 | 同步接口：`plugin_id` / `check_install_status` / `launch_run` |
| Generic CLI 插件 | 已有 MVP | `GenericCliRuntimePlugin<D>`，`plugin_id()` 固定返回 `generic-cli` |
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
| driver profile 表 | 未实现 | 当前 `runtime_profile` 不包含 `driver_id` 和 driver-specific config |
| Codex driver | 未实现 | 当前只有 generic command driver 和 test driver |
| prompt envelope | 未实现 | 当前没有统一 prompt builder |
| output parser | 未实现 | 当前不解析 Codex JSONL/event/final output |
| worktree 创建器 | 未实现 | 当前 workspace mode 有枚举，但没有自动创建 per-run worktree 的实现 |
| runtime session 表 | 未实现 | 当前没有通用 `runtime_session_mapping` |
| `task.finish` failed final | 未实现 | 当前 `task.finish` 总是落到 `finished` |
| `msg.send` 真实 ANP direct send | 需要补齐 | 当前 runtime outbox 抽象存在，但真实 IM Core runtime outbox 链路需要核对/补齐 |
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

本方案的第一步应尽量在这两个 v1 接口上扩展，而不是一开始替换成 async runner/session/cancel 的目标态接口。

---

## 3. 术语与边界

| 概念 | 所属层 | 含义 | 是否对 App 暴露 |
|---|---|---|---:|
| `agent_did` | ANP / Awiki | Runtime Agent 的对外通信身份 | 是 |
| `agent_handle` | ANP / Awiki | 指向 `agent_did` 的 handle | 是 |
| `controller_did` | daemon / user-service | 允许控制该 Agent 的 DID | 可展示 |
| `runtime_plugin_id` | daemon | Runtime plugin 类型；本方案首版为 `generic-cli` | 不建议直接暴露 |
| `driver_id` | generic-cli plugin | 插件内部 driver，例如 `codex` / `claude-code` / `gemini` | debug 可见 |
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

---

## 4. 总体架构

```text
awiki daemon
├── AgentRouter
│   └── agent_did -> runtime_plugin_id = generic-cli
├── ControllerRouter
│   └── sender_did == controller_did
├── WorkspaceManager
│   ├── shared-root
│   ├── worktree-per-task
│   ├── container
│   └── sandbox
├── RuntimePluginRegistry
│   └── generic-cli
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

1. `RuntimePluginRegistry` 只看到 `generic-cli`。
2. `DriverRegistry` 是 `generic-cli` 插件内部机制。
3. `agent_definition.runtime_plugin_id` 首版保存 `generic-cli`。
4. `cli_runtime_profile.driver_id` 决定使用 `codex`、`claude-code` 还是 `gemini`。
5. driver 可以有自己的 native state 和数据表，但不得绕过 daemon 的 local RPC、token、controller、outbox 和 audit。

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

这些字段不应因为 Codex / Claude Code / Gemini 切换而变成不同 plugin id。

### 5.2 CLI runtime profile 扩展

建议在当前 `runtime_profile` 之外新增插件内部 profile 表：

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
  driver_config_json TEXT,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

说明：

1. `driver_id` 是插件内部路由字段，不是 plugin id。
2. `driver_config_json` 保存 driver-specific 配置，例如 Codex profile、Claude settings、Gemini sandbox profile。
3. 首版可以只支持 `driver_id = codex`，但表结构不应阻止后续加入 `claude-code` 和 `gemini`。

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

建议新增轻量 run metadata 表，避免一开始引入通用 session 表：

```sql
CREATE TABLE cli_driver_run (
  run_id TEXT PRIMARY KEY,
  runtime_profile_id TEXT NOT NULL,
  driver_id TEXT NOT NULL,
  workspace_instance_path TEXT,
  command_json TEXT,
  output_log_path TEXT,
  final_output_path TEXT,
  native_session_id TEXT,
  synthetic_session_id TEXT,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

Codex MVP 中：

```text
native_session_id = NULL 或 codex resume id
synthetic_session_id = task-scoped id
```

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
7. 根据 exit code 和 callback 结果返回 `GenericCliExit`。

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

---

## 7. Codex Driver Baseline

### 7.1 推荐运行模式

Codex MVP 使用 `codex exec` non-interactive mode。

只读分析：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox read-only \
  --json \
  --output-last-message <final-output.txt> \
  -
```

允许写入 worktree：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox workspace-write \
  --json \
  --output-last-message <final-output.txt> \
  -
```

结构化最终输出：

```bash
codex exec \
  --cd <workspace_instance_path> \
  --sandbox workspace-write \
  --output-schema <awiki-final.schema.json> \
  --output-last-message <final-output.json> \
  -
```

说明：

1. `-` 表示 prompt 从 stdin 读取，避免把完整 prompt 放到 shell argv。
2. `--json` 用于 JSONL event observation，不作为唯一事实源。
3. `--output-last-message` 用于 fallback final，不替代 `task.finish` callback。
4. `--output-schema` 只约束最终输出形状，不承担 daemon 安全边界。
5. `danger-full-access` 和 `--dangerously-bypass-approvals-and-sandbox` 只能在外部 container/VM 已经提供隔离时显式启用，不能作为默认。

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
3. `--sandbox read-only` / `--sandbox workspace-write` 控制工具执行权限。
4. `--output-schema` 约束最终输出。
5. `--json` 观察事件。
6. `--output-last-message` 保存最终消息。

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

当前 `CommandGenericCliDriver` 已注入：

```text
AWIKI_DAEMON_RUN_ID
AWIKI_DAEMON_TASK_ID
AWIKI_DAEMON_TASK_TEXT
AWIKI_DAEMON_RUNTIME_RPC_TOKEN
```

后续 daemon local RPC over UDS 落地时，还需要注入：

```text
AWIKI_DAEMON_SOCKET
AWIKI_DAEMON_AGENT_DID
AWIKI_DAEMON_RUNTIME_PROFILE_ID
```

约束：

1. token 可以通过 env 或 fd 注入，但不能放入 prompt 文本。
2. wrapper 调用时不要让 Codex 自己拼接 token 到日志。
3. run 结束后 token 过期或被 revoke。

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
2. 不新增 `runtime.cli.claude_code` plugin。
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
2. 不新增 `runtime.cli.gemini` plugin。

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
workspace_instance_path = ~/.awiki/worktrees/<repo>/<run_id>/
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
| `msg.send` | 发送 ANP direct/direct-e2ee 消息 | 入口已有，真实外发需补齐 | 必须是真正外发 |
| `artifact.created` | artifact 报告 | 入口已有 | 后续增强 |

不新增 `task.result` 作为 MVP 对外协议。

### 10.3 `msg.send` 必须语义

```text
CLI agent
  -> daemon CLI wrapper send-message --to <did-or-handle> --text "..."
  -> daemon local RPC msg.send
  -> daemon 校验 run token / method / recipient scope
  -> daemon resolve recipient
  -> IM Core SDK direct.send 或 direct-e2ee send
  -> message-service
  -> 目标 DID
```

这是必须语义。不能把 `msg.send` 实现成仅向 controller 发送 status payload，也不能只记录内存 outbox 后宣称发送成功。

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
4. 后续需要幂等 final，避免重复发送最终回复。

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

当前 Generic CLI MVP 行为：

1. driver exit code 为 0 时，生成 `running` status 和 `task.finish` callback。
2. driver exit code 非 0 时，生成 `failed` status callback。
3. `task.finish` side effect 会把 run 标为 `finished`。

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

不要求：

1. 通用 `runtime_session_mapping` 表。
2. 长期 workspace-scoped native session。
3. 自动 resume。
4. 跨 conversation 记忆。

### 12.2 后续 native session

后续 driver 可逐步增加 native session：

| driver | native session 方向 | 是否阻塞 MVP |
|---|---|---:|
| Codex | `codex exec resume` / session files / transcript | 否 |
| Claude Code | `--session-id` / `--resume` / `--continue` | 否 |
| Gemini | checkpoint / saved chat | 否 |

当多个 driver 都需要稳定 session routing 时，再设计通用 `runtime_session_mapping`。

---

## 13. Driver 能力矩阵

| 能力 | Codex CLI | Claude Code | Gemini CLI |
|---|---|---|---|
| MVP 顺序 | 第一优先级 | 第二阶段 | 第三阶段 |
| Plugin ID | `generic-cli` | `generic-cli` | `generic-cli` |
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

### Phase 0：对齐当前实现基线

1. 保留 `GenericCliRuntimePlugin` 作为唯一 CLI runtime plugin。
2. 明确 `runtime_plugin_id = generic-cli`。
3. 增加或规划 `driver_id` 到 runtime profile/plugin profile 的映射。
4. 记录当前 `RuntimePlugin v1`、`GenericCliDriver`、workspace binding、local RPC token、outbox 缺口。
5. 删除 MCP 相关设计和落地项。
6. 明确 `task.status` / `task.finish` 是兼容 RPC 名称，不是产品层 task workflow。
7. 明确 `msg.send` 必须补齐真实 ANP direct/direct-e2ee send。

### Phase 1：Codex Driver MVP

1. 新增 `CodexDriver`，仍注册在 `generic-cli` 插件内部。
2. 实现 installation check：查找 `codex` binary，记录版本和路径。
3. 实现 Codex command builder：`codex exec --cd ... --sandbox ... --json ... -`。
4. 实现 prompt envelope builder。
5. 通过 stdin 传 prompt，不把完整 prompt 放到 shell argv。
6. 注入 `AWIKI_DAEMON_RUNTIME_RPC_TOKEN` 和 run/task env。
7. 支持 shared-root 只读分析。
8. 支持 worktree-per-task 写任务。
9. 记录 stdout/stderr/JSONL/final output path。
10. 保持 process exit failed fallback。

### Phase 2：Codex 回调闭环与 outbox

1. 完善 daemon CLI wrapper 命令：status / finish / send-message / artifact。
2. 让 Codex prompt 明确调用 wrapper。
3. 确保 local RPC token 不进入 prompt 明文。
4. 补齐 `msg.send` 到 IM Core SDK direct/direct-e2ee 的真实发送链路。
5. 补齐 final 回复到 controller 的真实发送链路。
6. 增加 failed final 或记录当前失败限制。
7. 增加重复 final 防护。

### Phase 3：Codex 安全与可观测性

1. worktree-per-task 自动创建和清理。
2. `read-only` / `workspace-write` 策略映射。
3. external container/sandbox 接入点。
4. Codex JSONL event 解析和 audit 摘要。
5. driver run metadata 表。
6. token revoke/expiry 证据记录。

### Phase 4：Claude Code Driver

1. 在同一 `generic-cli` 插件内新增 `ClaudeCodeDriver`。
2. 复用 prompt envelope、workspace、token、wrapper、outbox。
3. 只新增 Claude-specific command builder、permission mapping、event parser、native session metadata。
4. 不新增 `runtime.cli.claude_code` plugin。

### Phase 5：Gemini CLI Driver

1. 在同一 `generic-cli` 插件内新增 `GeminiCliDriver`。
2. 复用 prompt envelope、workspace、token、wrapper、outbox。
3. 只新增 Gemini-specific command builder、sandbox mapping、event parser、checkpoint metadata。
4. 不新增 `runtime.cli.gemini` plugin。

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

1. `runtime_plugin_id` 仍为 `generic-cli`。
2. `driver_id = codex` 由 plugin profile 或等价配置决定。
3. controller DID 消息可以触发 Codex headless run。
4. 非 controller 消息不会进入执行链。
5. Codex 只在指定 workspace instance 中运行。
6. local RPC token 绑定 run、method、recipient scope 和 TTL。
7. token 不出现在 debug log、prompt 明文和持久 transcript 中。
8. Codex 可以通过 wrapper 上报 running/failed/finished。
9. Codex 可以提交最终回复。
10. `msg.send` 不再只是内存记录或 status payload，而是真实走 daemon / IM Core SDK 外发链路，或者在验收中明确标记为未完成 blocker。
11. `worktree-per-task` 被记录为变更隔离，不被宣传为安全边界。
12. 文档和测试都不依赖 MCP。
13. Codex 引入后，后续 Claude Code / Gemini 只需在同一 plugin 内新增 driver，不需要新 plugin。

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



