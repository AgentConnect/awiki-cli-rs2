# Awiki daemon 本地开发

本目录实现 daemon 进程本身，daemon 与现有 `awiki-cli` 是平行入口，二者都复用 `im-core` SDK。daemon 代码固定放在 `crates/awiki-deamon`，不能依赖 `crates/awiki-cli` 内部模块。

当前提供的 daemon 管理命令：

- `awiki-deamon foreground --state-root <path>`
- `awiki-deamon init-state --state-root <path>`
- `awiki-deamon status --state-root <path>`
- `awiki-deamon setup-daemon-agent --state-root <path> --handle <handle> --controller-did <did> --registration-token <token>`
- `awiki-deamon agent-list --state-root <path>`
- `awiki-deamon agent-status --state-root <path> --agent-did <did>`
- `awiki-deamon runtime-list --state-root <path>`

这些命令都会加载 daemon 配置、初始化 daemon 状态库，并通过 `im-core` 公开 API 初始化 IM 本地状态。`agent-*` 和 `runtime-list` 是 daemon 自己的最小管理入口，和现有 `awiki-cli` 命令系统保持平行，不依赖 `crates/awiki-cli` 内部模块。

## 状态目录

产品安装默认使用：

```text
~/.awiki-daemon/deamon/state/
```

给定 `--state-root /path/to/state` 后，首版布局如下：

```text
/path/to/state/
  config.json
  daemon.db
  im-core/local-state.sqlite
  identity/registry.json
  identity/default
  runtime/cache/
  runtime/tmp/
  rpc/awiki-deamon.sock
  audit/audit.log
```

`config.json` 是 daemon 的持久配置文件。安装命令会写入后端根地址、daemon 下载根地址、DID domain、ANP endpoint 等解析后的配置；后续 foreground、service、upgrade 等命令会从同一状态目录读取。默认只需要配置一个后端根地址，其他字段会按规则派生：

```text
base_url = https://awiki.ai
user_service_base_url = base_url
message_service_base_url = base_url
mail_service_base_url = base_url
download_base_url = <base_url>/daemon
did_domain = base_url host
anp_service_endpoint = <base_url>/anp-im/rpc
anp_service_did = did:wba:<did_domain>
```

支持的环境变量覆盖：

- `AWIKI_DAEMON_BASE_URL`
- `AWIKI_DAEMON_SERVICE_BASE_URL`
- `AWIKI_DAEMON_USER_SERVICE_BASE_URL`
- `AWIKI_DAEMON_MESSAGE_SERVICE_BASE_URL`
- `AWIKI_DAEMON_MAIL_SERVICE_BASE_URL`
- `AWIKI_DAEMON_DOWNLOAD_BASE_URL`
- `AWIKI_DAEMON_DID_DOMAIN`
- `AWIKI_DAEMON_ANP_SERVICE_URL`
- `AWIKI_DAEMON_ANP_SERVICE_DID`
- `AWIKI_HERMES_GATEWAY_CMD`

## CLI Runtime 环境文件

产品安装模式下，daemon service 会引用一个通用的 CLI runtime 环境文件：

```text
~/.awiki-daemon/deamon/env/agent-cli.env
```

该文件用于把用户已经配置好的本机 CLI 环境显式注入到 `awiki-deamon` 进程中，供
Codex CLI、Claude Code CLI、Hermes gateway 或后续 generic-cli driver 复用。它不是
Claude Code 专用配置，也不是登录流程；daemon 不解析其中变量的业务含义，只在 service
启动时加载它。generic-cli driver 启动子进程时仍会先 `env_clear()`，再恢复最小运行环境和
driver 允许透传的变量，避免把 daemon 的完整环境无差别交给外部 CLI。

安全约束：

- 文件不存在时 service 仍能启动；安装只会创建 `env/` 目录，不会创建或覆盖 secret 文件。
- 该文件可能包含 token、API key、base URL 等敏感值，权限应保持为 `0600`，目录权限应为
  `0700`。
- 不要把该文件提交到仓库，不要在日志、E2E 报告或 UI 中打印变量值。
- 文件内容应使用 systemd 与 POSIX shell 都可读取的简单格式，例如：

```text
ANTHROPIC_BASE_URL=http://127.0.0.1:4000
ANTHROPIC_AUTH_TOKEN=...
CLAUDE_CODEX_MODEL=gpt-5.4-mini
AWIKI_DAEMON_CLI_ENV_PASSTHROUGH=ANTHROPIC_*,CLAUDE_CODEX_MODEL
```

子进程透传策略：

- 为避免把 daemon 的完整环境无差别交给外部 provider CLI，Codex / Claude Code driver 默认只恢复
  PATH、locale、terminal、必要 HOME / profile home 与 AWiki callback 变量。
- 如果 provider 依赖环境变量认证、私有 base URL 或模型变量，必须在同一 env file 中设置
  `AWIKI_DAEMON_CLI_ENV_PASSTHROUGH`。
- `AWIKI_DAEMON_CLI_ENV_PASSTHROUGH` 的值是逗号、分号、冒号或空白分隔的变量名 / 前缀选择器，
  例如 `ANTHROPIC_*,CLAUDE_CODEX_MODEL`、`OPENAI_API_KEY,OPENAI_BASE_URL` 或
  `MY_PROVIDER_TOKEN,MY_PROVIDER_BASE_URL,ACME_*`。变量值不会被日志打印。
- Codex driver 仍会强制设置 profile-scoped `CODEX_HOME`；如果用户只使用 Codex profile home 的
  `auth.json`，不需要额外透传 provider secret。
- Claude Code driver 复用 daemon 进程的 host `HOME`，即用户已经配置好的 Claude Code CLI
  登录态；daemon 不要求也不解析 Claude 专用登录文件。因此 Claude Code setup diagnostics 使用
  `auth_status=not_applicable`，真实认证问题会在实际 `claude -p` run 中暴露。

Linux `systemd --user` service 使用 optional `EnvironmentFile=-.../agent-cli.env`，因此缺失
文件不会阻止 daemon 启动。macOS LaunchAgent 通过 `/bin/sh -c` wrapper 只 source 这个
AWiki env file 后再 `exec awiki-deamon foreground`；不会默认读取用户 shell rc。Windows 后续
service 化时应复用同一“daemon runtime env file / credential provider”设计，而不是在
Claude/Codex driver 中写死平台专用环境变量。

macOS 安装会在写入 LaunchAgent 后显式执行 `launchctl enable
gui/<uid>/ai.awiki.deamon`，用于覆盖旧 cleanup 或手工联调遗留的 disabled 状态；同时
`ensure_state_layout()` 会预创建 `state/logs/`，避免 launchd 因 stdout/stderr 目标目录缺失而在
首次 bootstrap 时返回 I/O error。

Hermes Runtime 使用 Hermes TUI Gateway 的 stdio JSON-RPC 入口，不是普通 messaging gateway。官方 TUI Gateway 入口形态是：

```text
python -m tui_gateway.entry
```

但这个命令只有在对应 `python` 可以 import Hermes 的 `tui_gateway` 模块时才可用。daemon 的优先级是：

1. `AWIKI_HERMES_GATEWAY_CMD` 显式覆盖。
2. `config.json` 中已持久化的 `hermes_gateway_cmd`。
3. 自动探测 `~/.hermes/hermes-agent/venv/bin/python -m tui_gateway.entry`、`~/.hermes/hermes-agent/.venv/bin/python -m tui_gateway.entry`，再尝试 PATH 中的 `python3` / `python`。

自动探测会用 daemon 的临时 Hermes home 启动候选命令，只有在短超时内收到 `gateway.ready` 才会认为可用并写入 `config.json`。探测失败不会阻止 daemon foreground、状态上报或非 Hermes runtime 运行；只有真实执行 Hermes runtime 消息时才会返回 gateway 配置缺失/不可用错误。`AWIKI_HERMES_BIN` 仅用于旧的安装存在性检查，不会被当成 TUI Gateway 启动命令。

Hermes TUI Gateway 约定在 stdout 上输出 line-delimited JSON-RPC response/event。真实 Hermes 首次启动时可能会安装 Node.js、agent-browser 或 Chromium 等可选依赖，并把普通进度日志误写到 stdout。daemon 的 stdio adapter 会跳过不以 `{` 或 `[` 开头的 stdout 噪声行，继续等待合法 JSON-RPC 行；如果输出看起来像 JSON 但无法反序列化，仍会按协议错误失败。这样既兼容首次启动 bootstrap 日志，也不会吞掉真正损坏的 JSON-RPC 帧。

安装包默认写入：

```text
~/.awiki-daemon/deamon/bin/
```

`daemon.db` 是 daemon 自己的状态库，首版包含 agent、runtime profile、workspace binding、runtime run、runtime RPC token 占位表和 audit 表。`im-core/local-state.sqlite` 由 `im-core` 自己初始化和维护。

当前 `daemon.db` 的 agent / runtime 相关状态包括：

- `agent_definition`：daemon agent 和 runtime agent 的本地定义，包含 `handle`、`agent_kind`、`controller_did`、runtime profile、workspace 和本地路径。
- `agent_identity`：daemon 生成并通过 user-service registration token 兑换后的 agent DID 文档和本地私钥材料。私钥 seal 到 daemon SecretVault，状态库只保存 sentinel 和 `SecretRef`，不进入 Debug 输出、日志或 audit。
- `agent_auth_state`：daemon/runtime agent 调 message-service 时使用的本地 bearer token 状态。token seal 到 daemon SecretVault，状态库只保存 sentinel 和 `SecretRef`，用于本地长驻 E2E 和后续登录态恢复。
- `runtime_profile`：runtime agent 绑定的 runtime plugin type、展示名和状态。CLI 家族新数据统一使用 `runtime_plugin_id=generic-cli`。
- `cli_runtime_profile`：`generic-cli` 插件内部 profile，保存 `driver_id`、binary/config、默认 sandbox、默认 workspace mode、`msg.send` recipient policy 和 driver-specific config。
- `cli_driver_run`：CLI run 的 route/session/workspace/output metadata，包括 workspace instance、command、stdout/stderr/JSONL/final output 路径和 fallback final 来源。
- `runtime_final_outbox`：Runtime controller final reply 持久 outbox。Hermes final text 以及 Codex / Claude Code 等 generic-cli fallback final text 都先写入该表，再由 daemon 以 Runtime Agent DID 给 controller / requester 发送普通消息；发送成功后标记 sent 并把 run 标记为 finished，foreground 启动和循环会补发 due pending 记录。
- `workspace_binding`：CLI 类 runtime 绑定的 workspace 和 workspace mode。

首个版本仍使用单个 `daemon.db`。不同 agent / runtime plugin 通过表字段隔离，后续如有迁移、备份或插件规模需求，再考虑拆成 per-agent DB 或 plugin DB。

## 私钥与 vault 边界

`im-core` 已把 DID-WBA auth、业务签名和 secure direct 静态 key material 的业务读取路径收敛到内部 `KeyMaterialProvider`，并提供显式 root key 的 SecretVault foundation。当前 daemon 的私钥持久化边界如下：

- 新写入 `agent_identity` 时，`auth_private_key_pem`、`e2ee_signing_private_key_pem` 和 `e2ee_agreement_private_key_pem` 列只保存 `<awiki-secret-vault-ref>` sentinel，真实私钥 seal 到 daemon SecretVault，并在 `*_private_key_ref_json` 列保存 `SecretRef` JSON。
- 新写入 `user_delegated_identity` 时，`private_key_material` 只保存 `<awiki-secret-vault-ref>` sentinel，真实 delegated private key seal 到 daemon SecretVault，并在 `private_key_ref_json` 保存 `SecretRef` JSON。
- daemon 优先使用 `AWIKI_DAEMON_VAULT_ROOT_KEY_B64` 作为 SecretVault root key；未显式提供时，会在 `secrets/vault/root-key.b64u` 懒初始化本机 no-prompt root key 文件。该文件在 Unix/macOS 上按 `0600` 写入，`secrets/vault/` 目录按 `0700` 收紧；不要提交、打印、放入 env file 或写入 E2E 报告。
- 旧明文行不再兼容读取；缺少 `SecretRef`、root key 文件损坏或 root key 不匹配时，secret 读取/持久化会 fail-closed，不回退明文。首次安装不再因为未配置 `AWIKI_DAEMON_VAULT_ROOT_KEY_B64` 回退明文，而是创建本机 root key 文件后继续以 SecretVault 密文保存。
- Direct E2EE session/prekey local state 通过 `im-core` SecretVault envelope 密文落盘；历史明文 blob / 文件只在读路径兼容。
- daemon delegated inbox 的 `inbox_auth_key_ref` 使用 `vault:`，私钥 seal 到 `im-core` file vault。新路径需要 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`，不再写 `runtime/cache/delegated-inbox/.../daemon-key-1.pem` 或 shadow identity `private.key`。
- `agent_auth_state` 保存 daemon/runtime agent bearer token 的 vault ref 状态，`jwt_token` 列只允许 `<awiki-secret-vault-ref>` sentinel，真实 token seal 到 daemon SecretVault。不要把 token 原文写入日志、audit 或 E2E 报告。
- `im_core_adapter` 的 Message/im-core SDK 主路径使用 hosted in-memory identity material，不写 `private.key`、`e2ee-agreement-private.pem` 或 `auth.json`。user-service inventory DID-auth 也使用内存态 `DidAuthMaterial` 签名，不再通过兼容 PEM/auth.json 文件落盘。
- 显式 delegated `key_ref` 仍兼容 `file:`、`local:` 和裸路径读取 caller-provided delegated private key；新 daemon-owned delegated key 应使用 `vault:`。
- App bridge bootstrap 的 `user_subkey_package.private_key_pem` 仍是临时兼容 DTO，传输可以暂时明文；daemon 接收后持久化必须按上面的 vault ref 存储。后续应改为端到端加密 bootstrap envelope。

因此，当前安全结论是：daemon 持久化的 agent identity 私钥、Message Agent delegated 私钥、agent auth token 以及 Direct E2EE session/prekey secret 已按 SecretVault 密文保存；daemon root key 由 env 或本机 `root-key.b64u` 解锁，不进入 daemon DB、日志、audit 或 UI。App -> daemon 的 bootstrap 传输加密、真实平台 no-prompt vault backend、root key rotation/backup、legacy file-backed identity provider 下线和 user-service DID-auth 兼容文件收敛仍是后续独立加固范围。不要在日志、audit、E2E 报告或 UI 中输出任何私钥、token、root key 或 E2EE 本地 secret。

## 本地验证

```bash
cargo run -p awiki-deamon -- init-state --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- status --state-root /tmp/awiki-deamon-state
cargo test -p awiki-deamon --locked
```

App Message Agent 的 ANP P9 群消息 mention 路由可以用 focused tests 验证：

```bash
cargo test -p awiki-deamon --locked mention
cargo test -p awiki-deamon --locked user_delegated -- --nocapture
```

该路径验证 daemon 会拉取 direct + group inbox；只有合法 P9 `text + mentions` 群 payload 中的 `target.kind = agent` 精确命中 runtime agent DID 时才创建 RuntimeTask。`target.kind = group_selector`（包括 `@agents` / `@all` / `@humans`）、`target.kind = human`、纯文本 `@AgentName`、invalid range 和 E2EE opaque 都不能触发 runtime。mention 只是注意力信号，不是授权；daemon 仍会通过 user-service invocation policy 做硬权限判断，并且 task 只携带群内回复 allowlist。

步骤 01 不启动真实 runtime，也不连接远端 message-service。

本地模拟安装脚本可使用 `file://` 下载根，不需要公网 CDN：

```bash
scripts/release/daemon/_stage-downloads.sh \
  --version <version> \
  --source-dir dist/daemon \
  --output-dir /tmp/awiki-daemon-downloads \
  --base-url https://awiki.ai \
  --download-base-url file:///tmp/awiki-daemon-downloads

sh /tmp/awiki-daemon-downloads/install.sh \
  --token <install-token> \
  --state-root /tmp/awiki-deamon-state \
  --foreground
```

## 本地 RPC 安全模型

runtime Skill 不直接调用 message-service，也不携带可信身份字段。Skill 调用 daemon CLI wrapper，wrapper 通过 Unix domain socket 调 daemon local RPC，请求体只使用 `runtime_rpc_token` 作为授权材料。

token scope 由 daemon 生成并持久化，绑定这些字段：

- `agent_did`
- `runtime_profile_id`
- `run_id`
- `allowed_methods`
- `allowed_recipients`
- `expires_at_ms`
- `single_use`

daemon 收到 RPC 后按顺序执行：

1. 校验 Unix domain socket 文件权限和同机 peer credential。
2. 解析 `runtime_rpc_token`，用 token hash 查本地 `runtime_rpc_tokens`。
3. 从 token 记录反查 `agent_did`、`runtime_profile_id` 和 `run_id`。
4. 校验过期、撤销、一次性使用、method scope 和 recipient scope。
5. 写 audit。audit 只记录 `token_id`、上下文、method、授权结果和原因，不记录 token 原文。

当前实现的 RPC method：

- `rpc.ping`
- `task.status`
- `task.finish`
- `msg.send`
- `artifact.created`

Linux 使用 `SO_PEERCRED` 校验连接方 UID 必须等于 daemon UID。其他 Unix 平台已经 gated，后续需要补等价 peer credential 机制。Windows named pipe 不在当前步骤实现范围内。

## 通用 CLI runtime MVP

daemon 当前提供 Generic CLI runtime MVP 闭环：

1. `runtime.agent.create` 支持 `runtime=generic-cli` 以及 `codex`、`codex-cli`、`claude-code`、`gemini`、`gemini-cli` alias。
2. CLI family 新建 agent 时持久化 `runtime_plugin_id=generic-cli`，并在 `cli_runtime_profile.driver_id` 中保存具体 driver。`runtime=generic-cli` 未显式传 `driver_id` 时默认 `codex`。
3. 旧数据中的 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 只作为 legacy migration / alias 处理；新写入路径不再产生这些值。
4. 消息入口仍按 Runtime Agent DID 路由到 `agent_definition`，然后读取 `runtime_profile` 和 CLI profile 选择 driver；`generic-cli` 不是外部消息 routing key。
5. daemon 只接受 `sender_did == controller_did` 的 controller 消息执行 run，将文本消息标准化为内部 `RuntimeTask`。
6. daemon 创建 `RuntimeRun`，按 profile/run recipient policy 签发短期 `runtime_rpc_token`。
7. `GenericCliDriverRegistry` 按 `driver_id` 选择 `CodexDriver`、`command` driver 或后续 driver；Claude Code / Gemini 当前只保留未实现分支。
8. Codex driver 使用 `codex exec` headless 模式，通过 stdin prompt envelope 传入用户消息，使用 `--output-last-message` 记录 fallback final。
9. Codex / command driver 通过 daemon CLI wrapper + local RPC 回传 `task.status`、`task.finish`、`msg.send` 和 `artifact.created`；真实 Codex run 不使用 `RuntimeLaunchOutcome.callbacks` 作为 status/final 主链路。若 Codex / Claude Code 只产出 `--output-last-message` / stream final 而没有显式 `msg.send`，daemon 会把该 fallback final 写入 `runtime_final_outbox` 并以 Runtime Agent DID 发送成用户可见普通消息。
10. daemon 通过 token 反查可信上下文，更新 run 状态，写 audit，并通过 outbox 发送 status/final/message。单元测试使用 `MemoryRuntimeOutbox`；foreground 主链路通过 `im-core` SDK 发送 direct text。

Codex 真实 run 注入的环境变量包括：

```text
AWIKI_DAEMON_RUN_ID
AWIKI_DAEMON_TASK_ID
AWIKI_DAEMON_RUNTIME_RPC_TOKEN
AWIKI_DAEMON_SOCKET
AWIKI_DAEMON_AGENT_DID
AWIKI_DAEMON_RUNTIME_PROFILE_ID
AWIKI_DAEMON_CLI_WRAPPER
```

真实 Codex run 不注入 `AWIKI_DAEMON_TASK_TEXT`。用户消息只通过 stdin prompt envelope 进入 Codex，避免完整消息进入进程环境。

`msg.send` 支持 profile/run 授权的非 controller DID 或 handle。daemon 会先 resolve handle，再同时校验原始 handle 和 resolved DID 是否在 token scope / recipient policy 中，最后由 outbox 走测试内存记录或 foreground IM Core SDK 发送。未授权目标必须返回失败，不能伪造成发送成功。

workspace mode 只记录边界，不夸大安全性：

| mode | 定位 | 是否安全边界 |
|---|---|---|
| `shared-root` | 个人低风险、本机可信、读任务 | 否 |
| `worktree-per-task` | 代码变更隔离、避免任务互相污染 | 否，仅变更隔离 |
| `container` | 外部委托、高风险、自动写代码 | 是，依赖容器配置 |
| `sandbox` | 外部委托、高风险、自动写代码 | 是，依赖 sandbox profile |

RuntimeEvent 当前不作为任务状态和结果的第二条权威通道。权威回传链路是 runtime / daemon CLI wrapper / local RPC。

`worktree-per-task` 会在 daemon 管理的 runtime 临时目录下创建 per-run git worktree，路径形如：

```text
<state-root>/runtime/tmp/worktrees/<workspace_id>/<run_id>/
```

该模式用于变更隔离和 audit/diff 保留，不是安全边界。`shared-root` 会使用用户绑定的 workspace root；container / sandbox 只保留接入点，尚不是当前 Codex MVP 的默认实现。

## Daemon Agent 与 Runtime Agent 管理

daemon agent 和 runtime agent 都通过 user-service 的 registration token API 注册 DID。daemon 侧只消费 token，不签发 token；registration token 原文只用于调用 user-service `exchange_token`，不写入 `daemon.db`、日志或 audit。

daemon setup 的最小流程：

1. App 或安装入口从 user-service 获取 daemon registration token。
2. daemon 生成 Daemon Agent DID 文档和本地密钥。
3. daemon 使用 token、handle、DID document 调 user-service `exchange_token`。
4. 兑换成功后，daemon 写入 `agent_identity` 和 `agent_definition`。
5. 后续再次 setup 同一个 handle 时，优先恢复本地已有 Daemon Agent 定义。

runtime agent 的最小创建流程：

1. App/controller 获取 runtime agent registration token。
2. controller 向 daemon agent 发送 `application/json + body.payload` 命令。
3. daemon 校验 `sender_did == daemon agent.controller_did`。
4. daemon 生成 Runtime Agent DID 文档，用 registration token 调 user-service `exchange_token`。
5. daemon 写入 `agent_identity`、`agent_definition`、`runtime_profile` 和可选 `workspace_binding`。
6. daemon 通过 `im-core` payload 消息出口回发 `awiki.agent.status.v1` ready/failed 状态；测试中使用 `MemoryRuntimeOutbox`。

结构化命令固定使用普通 JSON payload：

```json
{
  "schema": "awiki.agent.command.v1",
  "command_id": "cmd_create_agent_001",
  "command": "runtime.agent.create",
  "target_agent_kind": "runtime",
  "args": {
    "handle": "@alice-awiki-coder",
    "runtime": "claude-code",
    "workspace": "~/work/awiki-me",
    "controller_did": "did:human:alice",
    "registration_token": "tok_runtime_agent_123"
  },
  "reply_policy": {
    "progress": true,
    "final": true
  }
}
```

承载规则：

- `meta.content_type = "application/json"`。
- JSON 对象放在 `body.payload`。
- daemon 不使用历史结构化 JSON 同义字段。
- daemon 不定义 command/status/result/task 专用 JSON content type。
- `payload.schema`、`payload.command`、`payload.state`、`payload.result` 是 daemon 上层业务语义，不是 message-service 的传输语义。

## Runtime Agent 收件箱查询

Runtime Agent 收件箱查询是 App <-> Daemon 控制链路，不是 Hermes Skill / runtime 推理链路。App 只在 Runtime Agent 会话中展示入口，向 Daemon Agent DID 发送 `awiki.agent.command.v1` payload；daemon 校验 controller 和 Runtime Agent 归属后，以 Runtime Agent 身份读取它自己的 IM 本地投影，再用 `awiki.agent.status.v1` payload 回传结果。

当前实现的控制命令：

- `runtime.inbox.query`：查询收件箱会话摘要，`status_scope = "runtime_inbox"`。
- `runtime.inbox.thread.query`：查询某个 direct/group 线程正文和附件元信息，`status_scope = "runtime_inbox_thread"`。

两个命令都必须使用 `target_agent_kind = "daemon"`。daemon 会继续复用现有 command 校验，要求目标消息投递给 Daemon Agent，且 `sender_did == daemon_agent.controller_did`；然后再校验 `runtime_agent_did` 属于当前 daemon、同一 controller 且 agent kind 为 runtime。校验失败时只返回 failed status，不读取消息。

列表请求参数：

```json
{
  "schema": "awiki.agent.command.v1",
  "command_id": "cmd_runtime_inbox_001",
  "command": "runtime.inbox.query",
  "target_agent_kind": "daemon",
  "args": {
    "runtime_agent_did": "did:example:runtime",
    "scope": "all",
    "limit": 30,
    "cursor": null
  }
}
```

线程请求参数：

```json
{
  "schema": "awiki.agent.command.v1",
  "command_id": "cmd_runtime_inbox_thread_001",
  "command": "runtime.inbox.thread.query",
  "target_agent_kind": "daemon",
  "args": {
    "runtime_agent_did": "did:example:runtime",
    "thread_id": "group:did:example:team",
    "kind": "group",
    "group_did": "did:example:team",
    "limit": 50,
    "cursor": null
  }
}
```

限制和安全边界：

- `scope` 只支持 `all`、`direct`、`group`。
- `kind` 只支持 `direct`、`group`。
- 列表默认 limit 30，线程默认 limit 50，最大 100。
- 列表只返回最近消息 preview，线程详情才返回正文。
- 单条正文最多返回 4000 字符，超出时设置 `truncated = true`。
- 附件只返回 `attachment_id`、`filename`、`mime_type`、`size_bytes`、`download_state` 元信息。
- 响应不能包含本机路径、JWT、runtime RPC token、私钥、API key 或 Hermes profile 路径。

当前 daemon 会先通过 `im-core` inbox/history 做 best-effort 远端刷新，再读取 `im-core/local-state.sqlite` 中 Runtime Agent owner identity 对应的本地 projection。这样控制查询不会因为一次远端 history refresh 不可用而直接失败。这个实现依赖当前 im-core 本地 projection schema，后续如果 im-core 暴露稳定的 local-read API，应优先收敛到 im-core 公共接口。

## 长驻 foreground E2E

当前 `foreground` 不再只是初始化状态后返回，它会作为长驻 daemon 进程运行：

1. 初始化 `daemon.db` 和 `im-core` 本地状态。
2. 将本地 daemon/runtime agent identity 同步到 `im-core` identity registry。
3. 启动 Unix domain socket local RPC worker。
4. 为 active runtime agent 启动 per-agent `im-core` realtime session。
5. 将多条 WebSocket session 的事件 fan-in 到 daemon 内部统一队列。
6. 消费 `application/json + body.payload` command。
7. 对 `runtime.agent.create` 复用 daemon agent 管理逻辑。
8. 对 `runtime.task.submit` 创建 runtime task/run，并启动 `test-runtime-uds`。
9. 测试 runtime 通过 UDS local RPC 回传 `task.status` 和 `task.finish`。
10. daemon 通过 `im-core` 发回 `awiki.agent.status.v1` payload。

foreground 的调度模型是事件驱动，而不是固定 250ms 扫描所有 agent 和所有队列：

- 远端消息通过 per-agent WebSocket realtime session 触发，daemon 层包装 `agent_did`、session generation 和 endpoint source 后统一 fan-in。
- WebSocket `sync` hint、gap、disconnect、reconnect、unknown notification、session ended 和 channel pressure 只会标记 dirty work，不会被当成 reliable checkpoint。
- reliable checkpoint 仍由 `im-core` 的 `sync_delta_async` 事务推进；daemon 只调度 `sync_delta_async`、`sync_thread_after_async` 和 targeted group fetch。
- direct message event 可以直接进入 runtime dispatcher；group message event 默认先进入 targeted group fetch，避免缺少 recent group context 时提前写入 processed-message dedupe。
- message sync outbox、runtime final outbox、CLI route queue 和 runtime retry queue 使用 `Notify + due timer + low-frequency reconciliation`，不会依赖 250ms 固定循环。
- WebSocket 不可用或发生未知事件时进入 degraded fallback。fallback 间隔明显大于 250ms，按 reason 记录 audit，并用 backoff/jitter 防止 event storm。
- `snapshot_required` 表示本地 checkpoint 不可信，daemon fail-closed 并记录 audit，不自行写 checkpoint 或盲目继续处理旧 projection。
- Runtime backend 仍只能通过 Skill / daemon CLI wrapper / local RPC 回传；runtime 不持有 DID 私钥，也不直接连接 message-service。

系统测试使用这些控制参数让长驻进程稳定退出。`--max-processed-messages` 只统计真实处理的 inbox command 和 retry queue 工作项，心跳/status latest 同步不计入这个退出条件：

```bash
awiki-deamon foreground \
  --state-root /tmp/awiki-deamon-state \
  --ready-file /tmp/awiki-deamon-ready.json \
  --max-runtime-ms 30000 \
  --max-processed-messages 2 \
  --poll-interval-ms 100
```

`--poll-interval-ms` 只保留为测试兼容和低频 floor 计算输入。正常 foreground 主循环主要等待 WebSocket 事件、local RPC/outbox notify、queue due timer、heartbeat timer、shutdown 信号和低频 reconciliation。验证事件驱动相关行为时优先使用：

```bash
cargo test -p awiki-deamon --locked realtime -j1
cargo test -p awiki-deamon --locked queue_scheduler -j1
cargo test -p awiki-deamon --locked runtime_inbox_reconciliation_interval -j1
cargo test -p awiki-deamon --locked -j1
```

本地同域 E2E 需要 message-service 能通过公开 DID 地址解析刚注册的 agent DID。运行 `tests_v2/daemon/test_awiki_daemon_long_running_e2e.py` 前，确认 `E2E_DID_DOMAIN` 及 message-service 节点域名能按当前环境解析到 user-service 公开 DID 文档；本地不再依赖 message-service 内部旁路配置。

```toml
[did_resolution]
verify_ssl = false
timeout_seconds = 10.0
```

否则 runtime agent DID 会被解析到不可访问的公网域名，本地刚注册的 DID 文档不可见，status/final 发送会失败。

控制状态 payload 使用非加密 direct 通道，范围仅限 App <-> daemon 的管理 JSON payload，例如 `agent.status.query` 和 daemon 回传的 `awiki.agent.status.v1`。Runtime Agent 代表用户向外发送的普通消息、附件、群聊消息仍按对应 CLI 参数和消息类型选择安全模式，不受这个控制面策略影响。Daemon 本地状态仍固定使用 `daemon.db` 和 `im-core/local-state.sqlite` 两个 SQLite 文件，MySQL 只用于本地 `user-service`。

本地 daemon E2E 系统测试入口位于：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=local \
E2E_USER_SERVICE_URL=http://127.0.0.1:9891 \
E2E_DID_DOMAIN=awiki.test \
E2E_MESSAGE_SERVICE_URL=http://127.0.0.1:18080 \
E2E_MESSAGE_SERVICE_WS_URL=ws://127.0.0.1:18080/im/ws \
E2E_MESSAGE_V2_USER_SERVICE_URL=http://127.0.0.1:9891 \
E2E_MESSAGE_V2_NODE_A_DOMAIN=msg-a.awiki.test \
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=http://127.0.0.1:18080 \
E2E_MESSAGE_V2_NODE_A_RPC_URL=http://127.0.0.1:18080/im/rpc \
E2E_MESSAGE_V2_NODE_A_WS_URL=ws://127.0.0.1:18080/im/ws \
E2E_MESSAGE_V2_DATABASE_A_URL=postgresql://message_service:message_service@127.0.0.1:5432/message_service_a \
E2E_MESSAGE_V2_NODE_B_DOMAIN=msg-b.awiki.test \
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=http://127.0.0.1:18081 \
E2E_MESSAGE_V2_NODE_B_RPC_URL=http://127.0.0.1:18081/im/rpc \
E2E_MESSAGE_V2_NODE_B_WS_URL=ws://127.0.0.1:18081/im/ws \
E2E_MESSAGE_V2_DATABASE_B_URL=postgresql://message_service:message_service@127.0.0.1:5432/message_service_b \
DB_HOST=127.0.0.1 DB_PORT=3306 DB_USER=awiki DB_PASSWORD=123456 DB_NAME=awikidb-dev DB_CHARSET=utf8mb4 \
USER_SERVICE_DATABASE_URL='mysql+aiomysql://awiki:123456@127.0.0.1:3306/awikidb-dev?charset=utf8mb4&connect_timeout=5' \
AWIKI_DAEMON_RUST_REPO=../awiki-cli-rs2 \
CARGO_BUILD_JOBS=1 \
uv run --no-sync pytest \
  tests_v2/daemon/test_awiki_daemon_long_running_e2e.py -q -rs
```

该测试覆盖：controller command、daemon listener、`controller_did` MVP 校验、runtime run 创建、UDS local RPC progress/final、`application/json + body.payload` status/final、controller history 结果和 audit 不记录 `runtime_rpc_token` 原文。

Generic CLI / Codex 的 focused daemon contract 验证可通过系统测试仓库的 Rust wrapper 执行：

```bash
cd ../awiki-system-test
AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 \
CARGO_BUILD_JOBS=1 \
uv run --no-sync python -m pytest \
  tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q -rs
```
