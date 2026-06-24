# Generic CLI Runtime Plugin 设计与首版就绪说明

版本：v0.7 implementation readiness
日期：2026-06-20
适用范围：`awiki-deamon` / ANP Agent Runtime Host / Codex CLI / Claude Code CLI
定位：记录当前 `generic-cli` 实现、消息 session 流转、目录关系、发布边界和首版未实现项。

> 本文是实现者、App 接入者和运维排查者的短入口，不是完整计划清单。更细的执行证据在
> `plan/20260619-daemon-codex-claude/plan.md` 和各 Step 文档中维护。

---

## 0. 核心速查

### 0.1 三层模型

Codex / Claude Code Runtime Agent 不能把身份、消息会话和原生 CLI 会话混成一个目录概念。当前实现固定使用三层：

| 层级 | 创建时机 | 事实源 | 用途 | 不能做什么 |
|---|---|---|---|---|
| Runtime Agent Identity | `runtime.agent.create` | `agent_definition`、runtime profile、`cli_runtime_profile` | 绑定 `runtime_agent_did + runtime_profile_id + driver_id`、controller scope、profile/config home、默认 sandbox/workspace 策略。 | 不能代表某个联系人或群组的长期消息 session；不能说明 CLI 已安装或已登录。 |
| Message Route Session | 收到真实、已授权消息后懒创建 | `cli_route_sessions` | 绑定 `runtime_agent_did + controller_scope_key + conversation_id`，保存 route workspace、session metadata、active native session id 和 route 状态。 | 不能在创建 Runtime Agent 时预创建所有联系人 session；不能通过扫描目录反向恢复 active route。 |
| Native CLI Session | 首轮 run 成功接受会话或后续 resume 成功后记录 | `cli_route_sessions.native_session_id` 与 `cli_driver_run.native_session_id` | 作为 Codex / Claude Code 的原生恢复指针。 | 不能作为 AWiki 授权源、消息路由源或顶层目录分区；不能用 synthetic id 冒充。 |

一句话：**创建 Runtime Agent 只创建身份层；收到某个 direct/group/thread 消息后才创建消息 route；Codex/Claude native session id 只是该 route 的可恢复指针。**

### 0.2 收到新消息时怎么创建 session

收到一个人的新消息时，daemon 必须按下面顺序处理：

```text
incoming message
  -> 明确的 target_runtime_agent_did / binding
  -> load Runtime Agent + Runtime Profile
  -> verify controller scope / recipient policy
  -> canonical conversation_id(direct/group/thread)
  -> route_key = cli:<runtime_agent_did>:<controller_scope_key>:<conversation_id>:message-run
  -> route_key_hash = daemon-local keyed hash(route_key)
  -> get_or_create cli_route_sessions
  -> prepare route workspace + session metadata dir
  -> acquire route/profile/host-home locks
  -> run Codex or Claude Code with cwd = route workspace
  -> route-bound final/status/native id writeback
```

不同 session 的区分规则：

- 私聊使用 direct conversation，例如 `direct:<peer>` 的 canonical 形式。实际实现必须从结构化消息上下文生成，不依赖字符串拼接反解析。
- 群聊使用 group conversation；群聊非 mention、未授权群消息或 binding 缺失时不创建 route session。
- thread 使用 thread conversation；不能规范化时 fail closed 或 one-shot，不写长期 route。
- 同一个 peer 发给不同 Runtime Agent 时，因 route key 包含 `runtime_agent_did`，并且目录按 `runtime_profile_id + route_key_hash` 分区，所以会落到不同 route。
- `route_key_hash` 只是路径和诊断标识，不是授权凭据；知道 hash 不能扩大 route list/status 权限。

### 0.3 Hermes / Codex / Claude Code 怎么分发

一条消息不是同时分配到 Hermes、Codex、Claude Code 三个候选 session。daemon 先定位唯一 Runtime Agent，再按 runtime plugin / driver 分流：

| 目标 runtime | 持久化分流字段 | Session 表 | 原生 session |
|---|---|---|---|
| Hermes | `runtime_plugin_id = runtime.hermes` | Hermes lifecycle / Hermes native session 表 | Hermes backend session |
| Codex | `runtime_plugin_id = generic-cli` + `driver_id = codex` | `cli_route_sessions` | Codex session/thread id |
| Claude Code | `runtime_plugin_id = generic-cli` + `driver_id = claude-code` | `cli_route_sessions` | Claude Code session id |

硬规则：

- 缺失或冲突的 `target_runtime_agent_did` / binding 必须 fail closed。
- daemon 不按 handle、display name、driver 名、最近活跃 route、目录名或 native session 猜测目标 runtime。
- Hermes 不写 `cli_route_sessions`；Codex/Claude 不写 Hermes native session 表。
- unknown / disabled / not implemented generic-cli driver 必须 fail closed。

### 0.4 目录是不是一一对应

目录是两层关系，不是“一个身份等于一个会话目录”。

```text
一个 Runtime Agent Identity
  -> 一个 profile/config home
  -> 多个 Message Route Session

一个 Message Route Session
  -> 一个 route workspace
  -> 一个 session metadata dir
  -> 最多一个 active native CLI session id

一个 CLI run
  -> 一个 cli_driver_run 记录
  -> 一个 run output/tmp 目录或文件集
```

建议布局以 daemon state root 为根：

```text
$AWIKI_DAEMON_STATE_ROOT/
  runtime/
    profiles/<runtime_profile_id>/
      profile.json                 # 诊断镜像，不是 SSOT
      codex-home/                  # Codex CODEX_HOME
      driver-config.json           # 诊断镜像

    workspaces/<runtime_profile_id>/
      identity/
      conversations/<route_key_hash>/

    sessions/<runtime_profile_id>/<route_key_hash>/
      session.json                 # 诊断镜像，不是 SSOT
      native-session.json          # 诊断镜像，不是 SSOT
      last-output.md
      runs/<run_id>/

    tmp/<run_id>/
```

DB 是 SSOT。`profile.json`、`session.json`、`native-session.json` 如果存在，只能作为本机诊断镜像；不能靠目录扫描反向创建 active route，也不能用镜像覆盖 DB route binding。

---

## 1. 当前实现概览

当前 `generic-cli` 已不是 Codex-only synthetic MVP。首版已经支持：

| 能力 | 状态 | 说明 |
|---|---|---|
| 单一 CLI runtime plugin type | 已实现 | CLI family 持久化 `runtime_plugin_id=generic-cli`；Codex/Claude/Gemini/command 是 `driver_id`。 |
| Runtime alias 解析 | 已实现 | `runtime=codex` / `codex-cli` -> `generic-cli + driver_id=codex`；`runtime=claude-code` -> `generic-cli + driver_id=claude-code`；legacy `runtime.cli.*` 仅兼容。 |
| `cli_runtime_profile` | 已实现 | 保存 driver、binary/config home、model、sandbox、workspace mode、recipient policy、driver config。 |
| `WorkspaceMode::RouteRoot` | 已实现 | 默认按消息 route 创建 cwd；它是上下文目录隔离，不是安全边界。 |
| `cli_route_sessions` | 已实现 | 长期 route session 表，保存 route hash、workspace/session path、active native session id、lock、last message/run、错误摘要。 |
| keyed route hash/salt | 已实现 | 新 route 使用 daemon-local keyed hash，降低路径可枚举风险；hash 不是授权凭据。 |
| Codex driver | 已实现 | `codex exec` fresh/resume/resume-last gated fallback，`CODEX_HOME` profile home，create 时种子复制 `config.toml` / `auth.json`，auth 缺失 fail fast，stdout/stderr/final output sanitizer，native id parser。 |
| Claude Code driver | 已实现 | `claude -p --verbose --output-format stream-json --session-id/--resume`，cwd 固定 route workspace，native id parser，settings/MCP 来源默认收紧；Claude Code 2.1.x 在 `stream-json` 输出下要求显式 `--verbose`。 |
| Gemini driver | 未实现 | create alias 可解析，但 registry 对 `gemini` fail closed。 |
| route list/status/reset | 已实现 | 只返回脱敏 route 摘要；Hermes 对 generic-cli list/status 返回 unsupported。 |
| profile/host-home lock | 已实现 | 获取顺序是 route lease -> profile lock -> host-home lock；Claude host default HOME 需要 host-home driver lock。 |
| install/status probe env allowlist | 已实现 | probe 使用 `env_clear()`，只恢复最小运行环境；Codex probe 可带 profile `CODEX_HOME`，并会补充常见用户 CLI bin 路径（如 `~/.nvm/versions/node/*/bin`）以覆盖 systemd user service 的最小 PATH；Claude probe 保留 `HOME` 仅表达 host-default setup 诊断。 |
| setup/status/version gate | foundation | create capability 与 setup readiness 分离；Codex `CODEX_HOME/auth.json` 缺失会显示 `auth_status=missing`，`auth_status=unknown` / `missing` 都不视为可运行。 |
| queue/deferred | foundation only | 有 `runtime_retry_queue` 和 `cli_route_message_queue` foundation、replay/drain/status summary；不承诺完整 durable FIFO、完整 rehydrate 或 manual replay。 |
| runtime card/App visual mapping | foundation | daemon 暴露低敏 `runtime_card`；App 消费 schema v1。它不是完整 remediation action UI。 |
| final provenance/output sanitizer | foundation | final source/hash、fallback source、sanitizer metadata 已有基础能力；不是完整 provider send ledger 或 support bundle。 |
| failed status/timeout/late callback guard | 已实现首版 | 失败不发送 final、不推进 `last_message_id`；timeout 释放锁；reset 后旧 callback/fallback/native id 写回被 route lease guard 拦截。 |

---

## 2. 数据模型与 SSOT

### 2.1 核心表

当前实现涉及的关键表：

| 表 | 作用 |
|---|---|
| `agent_definition` | Runtime Agent DID、plugin type、profile/workspace binding 的身份层事实源。 |
| `runtime_profile` | runtime profile 基础字段。 |
| `cli_runtime_profile` | generic-cli driver profile，包含 `driver_id`、`config_home`、sandbox、workspace mode、recipient policy、driver config。 |
| `cli_route_sessions` | Message Route Session 的长期事实源。 |
| `cli_driver_run` | 每次 CLI run 的命令、workspace、output、native session、fallback final metadata。 |
| `cli_runtime_locks` | profile lock / host-home lock。 |
| `cli_route_message_queue` | route message queue foundation，保存最小 message/run reference。 |
| `runtime_retry_queue` | runtime retry/backoff foundation。 |
| `runtime_final_outbox` | controller final reply 持久 outbox，记录 final source/hash；Hermes final 与 Codex / Claude Code fallback final 都必须通过它发送成用户可见普通消息。 |
| `audit_log` | 脱敏操作审计。 |
| `daemon_state_metadata` | daemon-local metadata，例如 route hash salt。 |

### 2.2 route key 与 conversation

`cli_route_sessions.route_key` 使用 canonical 输入生成：

```text
cli:<runtime_agent_did>:<controller_scope_key>:<conversation_id>:message-run
```

要求：

- `conversation_id` 必须在进入 generic-cli 前规范化。direct/group/thread 都应有稳定 canonical form。
- direct message 不能因为缺失 conversation id 全部落到 `no-conversation`；不能规范化时应 fail closed 或 one-shot。
- controller 手工 one-shot task 可不写长期 route session；它不能污染长期 direct/group/thread route。
- `route_key_hash` 使用 daemon-local keyed hash，格式是 `route_<24 hex>`；它只用于路径和诊断。

### 2.3 last message 水位

`last_message_id` 是 final-only 水位：

- `final_sent` 或项目等价完成态才可推进。
- `accepted`、`queued`、`deferred`、`running`、`failed`、`dead_letter` 都不能推进。
- failed status 的首版契约是 `failed_message_recovery=unsupported`，用户需要重新发送消息或等待未来显式 retry/replay 能力。

---

## 3. Driver 行为

### 3.1 Codex

Codex 使用 profile 级 `CODEX_HOME`：

```text
CODEX_HOME=<profile>/codex-home
```

创建 Codex Runtime Agent 时，daemon 会为该 profile 创建独立 `codex-home`，并在宿主 `CODEX_HOME` 或 `~/.codex` 已存在时只种子复制 `config.toml` 与 `auth.json` 两个 setup 文件；已有 profile 文件不覆盖，history、sessions、logs、sqlite 状态和其他用户内容不复制。这样新建 agent 默认继承本机已登录的 Codex provider 配置，但仍保持每个 Runtime Agent 的 profile home 隔离。

daemon 在启动 Codex 子进程前会清空环境并恢复 allowlist；由于 Linux `systemd --user` service 默认 PATH 通常不包含 nvm/node 安装路径，Codex driver 会在子进程 PATH 前置常见用户 CLI 目录（`~/.local/bin`、`~/.npm-global/bin`、`~/.nvm/current/bin`、`~/.nvm/versions/node/*/bin`、`/opt/homebrew/bin`、`/usr/local/bin`）。这只影响 Codex 子进程查找 CLI / Node，不向 App 或远端状态暴露本机路径。

Codex route workspace 可能是 daemon 为会话创建的空目录，不一定是 Git 仓库；driver 默认附加 `--skip-git-repo-check`，避免 Codex CLI 在会话目录缺少 `.git` 时直接退出。沙箱模式仍由 runtime profile 的 `read-only` / `workspace-write` 控制。

service install 还会写入通用 CLI runtime 环境注入入口：

- Linux `systemd --user` unit 引用 optional `EnvironmentFile=-<home>/.awiki-daemon/deamon/env/agent-cli.env`。
- macOS LaunchAgent 使用 `/bin/sh -c` wrapper 只 source 同一路径下的 `agent-cli.env`，再 `exec awiki-deamon foreground`。
- 该文件缺失不影响 daemon 启动；存在时用于把用户已配置好的 provider/base URL/model 等 CLI 环境显式注入 daemon 进程。

driver 子进程仍不会继承 daemon 完整环境，而是先 `env_clear()` 后恢复最小 PATH/locale/HOME、profile home 与 AWiki callback 变量。provider/API/base URL/model 等额外变量必须通过 `AWIKI_DAEMON_CLI_ENV_PASSTHROUGH` 显式列出变量名或前缀选择器，例如 `ANTHROPIC_*,CLAUDE_CODEX_MODEL` 或 `OPENAI_API_KEY,OPENAI_BASE_URL`。敏感值不能写入 service unit、日志、E2E 报告或仓库。

首轮新 route 或无可恢复 native id：

```bash
codex exec \
  --cd <route-workspace> \
  --sandbox <read-only|workspace-write> \
  --json \
  --output-last-message <session-dir>/last-output.md \
  -
```

已有可信 native session id：

```bash
codex exec \
  --cd <route-workspace> \
  --sandbox <read-only|workspace-write> \
  --json \
  --output-last-message <session-dir>/last-output.md \
  resume <native_session_id> \
  -
```

严格 fallback：只有 route 已有上一轮运行证据但没有捕获 native session id，且 workspace 是独立 route workspace 时，才允许：

```bash
codex exec \
  --cd <route-workspace> \
  --sandbox <read-only|workspace-write> \
  --json \
  --output-last-message <session-dir>/last-output.md \
  resume --last \
  -
```

禁止事项：

- 新 route 不使用 `resume --last`。
- 禁止 `resume --last --all`。
- fallback 成功但仍没有可信 native id 时，不能把 synthetic id 写入 `native_session_id`。
- Codex stdout/final 不能修改 reply target、route、recipient、workspace、policy 或 native id 写回逻辑。
- Codex fallback final 不能只作为 daemon status payload 上报；必须写入 `runtime_final_outbox`，由 daemon 以 Runtime Agent DID 发送普通消息，确保 App 聊天 UI 能看到回复。

启动前 readiness：`codex-home/auth.json` 缺失或为空时，Codex driver fail fast，返回 `generic_cli_auth_missing`、`setup_ready=false`，不 spawn `codex exec`、不创建 provider 会话、不发送 final。这避免空 profile home 触发 Codex CLI 交互式登录或长时间等待，造成首条消息 600s timeout。

当前默认 `ephemeral=false`，以便 native session 能恢复。`--ephemeral` 只适合 debug/临时模式，不能作为长期 route session 默认。

### 3.2 Claude Code

Claude Code 使用 `claude -p` print/headless 模式，cwd 必须固定为 route workspace。首轮：

```bash
claude -p \
  --verbose \
  --output-format stream-json \
  --permission-mode <plan|default> \
  --setting-sources user \
  --strict-mcp-config \
  --session-id <generated_uuid>
```

后续 route：

```bash
claude -p \
  --verbose \
  --output-format stream-json \
  --permission-mode <plan|default> \
  --setting-sources user \
  --strict-mcp-config \
  --resume <native_session_id>
```

当前策略：

- `read-only` 映射到 `permission_mode=plan`。
- `workspace-write` 映射到 `permission_mode=default`，仍不是硬安全边界。
- 默认只允许 `--setting-sources user`；project/local settings、MCP、hooks、browser/IDE/PR 集成默认不进入首版信任面。
- `--strict-mcp-config` 默认启用。
- route session 下禁止 `no_session_persistence=true`，否则 fail closed，不启动 Claude、不写 native id、不发 final。
- `--session-id` 是 proposed native id；只有 run 被接受，并通过 route lease、trusted parser/outcome、id/source 校验后，才能成为 active native id。
- 显式 `--resume` 不能替代 cwd/project directory scope；cwd 缺失、越界或无法设置时 fail closed，不能回落到 daemon cwd、profile home 或宿主 shell cwd。

### 3.3 `command` 与 `gemini`

`command` driver 是测试/内部 driver，不是 App runtime selector 的用户选项。即使 daemon capability 中包含它，App 也不能把它暴露为普通用户可创建 runtime，除非另有 UI、权限、审计和 system tests。

`gemini` 当前未实现。Registry 对 `gemini` launch fail closed。Google Cloud Code 不在本文范围内。

### 3.4 Cloud Code 命名

本文和相关计划中如果出现 Cloud Code，均指 **Claude Code CLI**。不表示支持 Google Cloud Code。Google Cloud Code 需要单独 driver、provider disclosure、setup/runbook 和系统测试。

---

## 4. Setup / Status / App 契约

### 4.1 create capability 与 setup readiness 分离

App 不应把 daemon 能创建 generic-cli Runtime Agent 理解成 CLI 已安装、已登录、可运行。

| 层级 | 含义 | App 行为 |
|---|---|---|
| create capability | daemon schema/driver/workspace/native resume 契约可支持创建。 | 决定 Codex/Claude 创建入口是否 enabled。 |
| setup/install status | binary、version、profile home、auth/setup 可诊断状态。 | 决定 runtime card 是否 `needs_setup` / `setup_ready`。 |
| dispatch readiness | 当前 runtime lifecycle、policy、locks、queue、setup 是否允许运行消息。 | 决定消息是否可运行、queued、failed 或 manual review。 |

旧 daemon 缺 `generic_cli` capability 时，Hermes 仍可用，Codex/Claude 创建入口 fail closed。schema v1 中新增未知字段可忽略；关键字段缺失、类型错误或未来 schema 版本必须让 Codex/Claude fail closed。

### 4.2 Runtime card

daemon 在 `diagnostics_summary.config_summary.runtime_card` 暴露低敏状态：

- `status_schema_version`
- `runtime_family`
- `driver_id`
- `lifecycle_state`
- `setup_ready`
- `setup_state`
- `queue_state`
- `active_run_state`
- `route_session_state`
- count/age bucket
- `next_action`
- `contains_user_content=false`
- `contains_provider_auth_material=false`
- `last_message_id_watermark_policy=final_only`

App 只能消费结构化 enum / schema version / typed fields，不能解析自然语言错误。`needs_setup` 不等于重新创建 Runtime Agent；missing binary、auth unknown、rate limited、provider unavailable 都不能引导重复 create。

### 4.3 route session list/status

`runtime.session.list` / `runtime.session.status` 是诊断面，不是联系人目录、授权源或自动 runtime selector。

要求：

- 请求必须定位当前 controller scope 下的具体 runtime。
- 过滤只能缩小范围，不能扩大权限。
- `route_key_hash` 不是授权凭据。
- 响应不返回完整 route key、完整 conversation id、peer/group DID、本机路径、native session id、source message id、prompt、附件或 provider auth material。
- Hermes 对 generic-cli list/status 返回 unsupported；generic-cli profile 缺失 fail closed。

---

## 5. 运行、锁、失败与队列

### 5.1 锁顺序

generic-cli run 的锁顺序固定为：

```text
route lease -> profile lock -> driver-family/host-home lock
```

释放顺序反向执行。任一后续锁失败必须释放已获锁。timeout、reset late callback、launch failure、profile busy、host-home busy 后不能永久 busy，也不能跨 route 写 active native session。

### 5.2 失败策略

首版失败策略：

- 返回脱敏 `failed + error_code + next_action`。
- 不发送 final。
- 不推进 `last_message_id`。
- 不自动重放旧 prompt。
- 不把 failed/deferred/queued 写成 final success。

已覆盖的 foundation：

- missing binary / setup failure 不创建 CLI route session，不启动 CLI，不发 final。
- Codex/Claude nonzero exit 记录稳定 `*_cli_failed`。
- run timeout 记录 `*_cli_timeout`，释放锁。
- reset 后旧 run 的 callback/fallback/native id 写回被 route lease guard 拦截，旧 run 记为 failed。

### 5.3 Queue foundation 边界

`cli_route_message_queue` 与 `runtime_retry_queue` 是首版 foundation/internal diagnostic：

- queue item 默认只保存 message/run reference、route/profile、attempt/backoff、脱敏错误码。
- 不长期保存完整消息、完整 prompt、附件内容、完整 route key、本机路径、native session id 或 secret。
- status summary 只暴露聚合 count、due count、due route count、oldest queued age、`next_action` 和 `contains_user_content=false`。
- 当前 drain foundation 不能写成完整 message rehydrate、完整撤回/retention/附件 manifest/group authorization/provider disclosure 重校验。
- dead-letter manual replay 首版 unsupported。未来如实现，必须 local-only/admin-only、默认 dry-run、重新校验 message reference、附件、binding、disclosure、setup、capability，创建新 run id 并写 audit。

---

## 6. 安全、隐私与外部 Provider

### 6.1 环境与 prompt

Codex/Claude run 使用结构化 `Command::new`，不走 shell 拼接。真实 prompt 通过 stdin 进入 CLI，不放入 argv/env。Runtime RPC token 不进入 prompt、debug、stdout/stderr、JSONL/final output 或持久 transcript。

Codex run env：

- `env_clear()` 后恢复最小 `PATH` / locale / terminal 变量。
- 从 daemon runtime env file / service 环境中按 `AWIKI_DAEMON_CLI_ENV_PASSTHROUGH` 显式选择的变量名或前缀透传 provider 环境；未显式选择的 provider/API/cloud token 不透传。
- 设置 `CODEX_HOME` 到 profile-scoped config home。
- 注入本次 run 需要的 AWiki wrapper/socket/token/run/profile env。

Claude run env：

- `env_clear()` 后恢复最小 `PATH` / locale / terminal 变量和 `HOME`。
- `HOME` 保留只表达 host-default Claude setup/auth 诊断，不等于 profile auth 隔离。
- 从 daemon runtime env file / service 环境中按 `AWIKI_DAEMON_CLI_ENV_PASSTHROUGH` 显式选择的变量名或前缀透传 provider 环境；不继承未被显式选择的 provider/API/cloud token、OAuth/JWT/private key 或 daemon secret。

Probe env 与 run env 分离；`--version` / install probe 不注入 AWiki runtime token/socket/run env。

### 6.2 输出与 parser

CLI stdout、stderr、final output 和 fallback final 都进入 sanitizer：

- 移除 ANSI/control 字符。
- 替换非 UTF-8。
- 截断到 bounded size。
- redacts runtime RPC token。
- metadata 只记录长度/布尔/version，不保存 raw output。

Codex JSONL / Claude stream-json parser fail closed：

- 只有可信 event 和通过 driver-specific id/source 校验的值能更新 active native id。
- unknown/malformed/truncated/schema mismatch 只进入 parser diagnostic 或不写 native id。
- CLI 输出和 final 文本不能修改 route、recipient、thread、workspace、policy、reset、cleanup、manual replay 或 support bundle 等控制面。

### 6.3 Provider disclosure

Codex CLI 和 Claude Code CLI 是外部 provider 路径，不是本地模型。根据 CLI 配置和任务内容，用户消息、上下文摘要、附件 manifest 和受控 workspace 文件内容可能被发送给对应 provider。

首版要求：

- App/docs 不能把 Codex/Claude 描述成本地模型。
- CLI provider auth/account 与 AWiki Runtime Agent DID 分离。
- `home_isolation=host_default`、`profile_home`、`unknown` 只表达本机 CLI auth 隔离程度，不改变 AWiki DID 和 controller scope。
- 支持 bundle、remote/App status 默认不包含真实 prompt/transcript、provider auth material 或 native transcript。
- Provider allowlist/denylist、组织策略、数据驻留、企业 DPA、provider send ledger、provider account fingerprint/scope provenance 仍是 future/residual，不能把 runtime card 当合规证明。

---

## 7. Reset、Cleanup、Backup

`runtime.session.reset` 首版只清 AWiki active route/native pointer 和 route 状态。它不删除：

- Codex `CODEX_HOME`
- Claude Code 自身保存的 native transcript/session/history
- route workspace
- session metadata dir
- run history
- provider-side retained content

cleanup/delete/support bundle/backup/restore 首版 unsupported/future。未来实现必须至少具备：

- local-only/admin-only 命令面。
- dry-run。
- 二次确认。
- no-follow symlink/junction 与 hardlink/path escape 防护。
- SQLite WAL/SHM/schema integrity 说明。
- route hash salt/key metadata 备份与恢复策略。
- 敏感等级标记：`contains_user_content`、`contains_provider_auth_material`。
- 不通过目录扫描反向创建 active route。

---

## 8. Readiness / Go-No-Go

| 能力 | 状态 | 发布口径 |
|---|---|---|
| Hermes create/run | 已有 | 必须保持兼容。 |
| Codex Runtime Agent create | 已实现 | 可按 daemon capability 向 App 开放；setup 仍单独诊断。 |
| Claude Code Runtime Agent create | 已实现 | 可按 daemon capability 向 App 开放；setup 仍单独诊断。 |
| Codex RouteRoot native resume | 已实现 | route workspace + active native id 优先；`resume --last` 是严格 fallback。 |
| Claude Code RouteRoot native resume | 已实现 | `--session-id` / `--resume` + route cwd；host HOME 隔离风险需如实展示。 |
| Fake CLI command-template tests | 已实现 | 证明 argv/env/parser/route isolation 的核心契约。 |
| Real CLI canary | optional local-only / future | 不是首版发布必需门禁；执行时必须 synthetic profile/route/workspace，记录成本和污染风险。 |
| Route list/status/reset | 已实现 | 只作为脱敏诊断面。 |
| Runtime card / App visual mapping | foundation | 可展示 created/needs_setup/queued/running/failed 等低敏状态；不是完整 remediation UI。 |
| Queue/deferred/drain | foundation only | 不能承诺完整 durable FIFO、完整授权重校验、失败消息自动恢复或 manual replay。 |
| Failed status | 已实现首版 | 脱敏 error code + next_action；不发 final，不推进水位。 |
| Timeout/process group | foundation | Unix process group cleanup + timeout 已有；完整 cancel/watchdog/daemon crash orphan recovery 是 residual/future。 |
| Output sanitizer/final provenance | foundation | 有 sanitizer、fallback source、final hash；不是完整 provider send ledger/support bundle。 |
| WorktreePerRoute | unsupported/future | 首版只承诺 `RouteRoot`；worktree/真实仓库策略需另起计划。 |
| Container/Sandbox hard isolation | unsupported/future | `RouteRoot`、`SharedRoot`、`WorktreePerTask` 都不是硬安全边界。 |
| Cleanup/delete/support bundle/backup | unsupported/future | 只记录数据分级和未来 gate。 |
| Google Cloud Code | unsupported | 本文 Cloud Code 指 Claude Code CLI。 |

---

## 9. Threat Model / Abuse Case

| 场景 | 当前防护 | 剩余风险 |
|---|---|---|
| 用户消息试图覆盖 route、recipient、sandbox、model 或 cleanup 控制面 | 控制面来自 DB/profile/lease/trusted parser；prompt 只是任务内容。 | prompt injection 仍可能影响模型自然语言输出，需要后续更强 policy/review。 |
| CLI stdout/final 伪造控制面 | stdout/final 只作为 output；native id 需 parser + id/source 校验；reply target 由 daemon route binding 决定。 | provider send ledger 和完整 parser schema evolution 仍是 future。 |
| reset 后旧 run callback 污染新 route | local RPC side effect、fallback final、native id writeback、lease release 都检查当前 route lock。 | 完整 generation/tombstone 和 daemon crash orphan recovery 仍是 future。 |
| stale queue item 绕过撤权 | queue foundation 保存最小 reference；docs 标明完整 rehydrate/authorization replay 未实现。 | 首版 drain 不能宣称完整撤回/retention/附件/group revision 重校验。 |
| provider account 变化后误 resume | `home_isolation` 和 setup 状态能表达部分风险。 | provider account fingerprint/scope provenance 尚未完整实现。 |
| 同一 state root 多活 split-brain | state-root owner guard foundation。 | stale owner 接管、clone/restore split-brain 仍需运维 runbook 和后续 hardening。 |
| support bundle 泄露 route workspace 或 native transcript | 首版不实现 support bundle；文档要求默认不导出敏感内容。 | 未来实现必须做数据分级、dry-run、二次确认和遍历安全。 |

---

## 10. 本机 Runbook

### 10.1 Setup 诊断

App/remote status 只能展示低敏 `setup_state`、`next_action`、binary/auth/home-isolation 摘要和检查时间。完整 setup command、profile home 路径、provider login 过程只允许 local-only admin/debug 场景。

典型状态解释：

| 状态 | 含义 | 用户动作 |
|---|---|---|
| `created` | AWiki identity/profile 已创建。 | 等待 setup 或发送消息前检查 setup readiness。 |
| `needs_setup` | binary/auth/profile readiness 不足。 | 本机安装/登录对应 CLI；不要重复创建 Runtime Agent。 |
| `queued` / `running` | 消息处理中或排队。 | 不代表已回复。 |
| `failed` | 单条消息失败。 | 查看脱敏 `error_code` / `next_action`；首版不自动恢复旧消息。 |
| `manual_review_required` | 需要本机人工确认。 | 不能由普通远端消息触发 cleanup/replay/support bundle。 |

### 10.2 reset

reset 适合清掉当前 route 的 AWiki active native pointer，让下一条消息重新创建或重新捕获 native session。reset 不删除 provider/native 历史，不等于 cleanup、disable、archive 或 DID/key revocation。

### 10.3 system-test gate

最终集成 gate 使用 sibling system-test repo：

```bash
cd ../awiki-system-test
AWIKI_ENABLE_DAEMON_REMOTE_SMOKE=1 \
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
AWIKI_DAEMON_RUST_REPO=../awiki-cli-rs2-daemon-codex-claude \
AWIKI_ME_REPO=../awiki-me-daemon-codex-claude \
uv run --no-sync pytest tests_v2/daemon -q -rs -k "remote"
```

真实 CLI canary 不是首版必需 gate。若本机执行，应使用 synthetic runtime/profile/route/workspace 和无敏感 prompt，并记录可能 provider 成本、写入 profile/native session 的污染风险和清理策略。

---

## 11. 实现证据索引

已提交的关键切片：

| 领域 | Commit |
|---|---|
| profile home / Codex baseline | `3347720` |
| App runtime create generalization | `def5541` |
| RouteRoot / `cli_route_sessions` | `cbaf8cb` |
| Codex native resume | `b575871` |
| Claude Code driver | `4904d11` |
| create capability / App gate | `f20c0d9`、`319c2e2` |
| reset/list/status | `5b9b6d4`、`35b55e1` |
| profile/host-home lock | `dbedf3f` |
| keyed route hash/salt | `193dd56` |
| probe env allowlist / setup gates | `843ddae`、`125efcc` |
| process/timeout/busy/queue foundations | `8650d4b`、`1d3beff`、`4ee0383`、`54eac55`、`cefd345`、`3de5176`、`30e49e1` |
| route queue status / runtime card / App card | `a842fcc`、`af3d629`、`87a9a5b` |
| final provenance / sanitizer / failed status / timeout / late callback guard | `b211228`、`2fe8b82`、`6e8393e`、`873fa5e`、`fa31423` |

常用本地验证：

```bash
cd awiki-cli-rs2-daemon-codex-claude
cargo fmt --all --check
CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked generic_cli -- --nocapture
CARGO_BUILD_JOBS=1 cargo check -p awiki-deamon --tests --locked
```

App 验证：

```bash
cd awiki-me-daemon-codex-claude
dart analyze
dart run tests/unit/runner.dart
dart run tests/e2e/runner.dart --case smoke
```

---

## 12. 外部资料与漂移边界

官方文档只作为 CLI 语义参考；发布证据仍以本仓库 fake CLI command-template tests、本机 `--help` / `--version` 记录、driver args schema 和 system-test 结果为准。真实 binary 未安装或未登录时，必须记录跳过原因，不能把未验证参数写成真实 canary 通过。

当前文档检查口径：

| 项 | 记录 |
|---|---|
| `checked_at` | 2026-06-20 |
| Codex command builder schema | fresh/resume/resume-last 由 fake CLI / Rust command builder tests 覆盖；禁止 `resume --last --all`。 |
| Claude Code command builder schema | first/resume 由 fake CLI / Rust command builder tests 覆盖；默认 `--setting-sources user` 与 `--strict-mcp-config`。 |
| Real CLI canary | 首版 optional local-only / future；未作为必需发布 gate。 |
| 本机 binary evidence | Step 04/05 已记录 Codex / Claude Code `--help` 摘要；最终 Review 如重跑失败或未安装，需要记录跳过原因。 |

参考链接：

- Codex CLI reference: https://developers.openai.com/codex/cli/reference/
- Codex environment variables: https://developers.openai.com/codex/environment-variables/
- Claude Code CLI reference: https://docs.anthropic.com/en/docs/claude-code/cli-reference
- Claude Code settings: https://code.claude.com/docs/en/settings
