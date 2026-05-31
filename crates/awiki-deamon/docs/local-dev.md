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

给定 `--state-root /path/to/state` 后，首版布局如下：

```text
/path/to/state/
  daemon.db
  im-core/local-state.sqlite
  identity/registry.json
  identity/default
  runtime/cache/
  runtime/tmp/
  rpc/awiki-deamon.sock
  audit/audit.log
```

`daemon.db` 是 daemon 自己的状态库，首版包含 agent、runtime profile、workspace binding、runtime run、runtime RPC token 占位表和 audit 表。`im-core/local-state.sqlite` 由 `im-core` 自己初始化和维护。

步骤 07 后，`daemon.db` 的 agent 相关状态包括：

- `agent_definition`：daemon agent 和 runtime agent 的本地定义，包含 `handle`、`agent_kind`、`controller_did`、runtime profile、workspace 和本地路径。
- `agent_identity`：daemon 生成并通过 user-service registration token 兑换后的 agent DID 文档和本地私钥材料。私钥只保存在本地 daemon 状态库中，不进入 Debug 输出、日志或 audit。
- `agent_auth_state`：daemon/runtime agent 调 message-service 时使用的本地 bearer token 状态。该表用于本地长驻 E2E 和后续登录态恢复；不要把 token 原文写入日志或 audit。
- `runtime_profile`：runtime agent 绑定的插件、展示名和状态。
- `workspace_binding`：CLI 类 runtime 绑定的 workspace 和 workspace mode。

首个版本仍使用单个 `daemon.db`。不同 agent / runtime plugin 通过表字段隔离，后续如有迁移、备份或插件规模需求，再考虑拆成 per-agent DB 或 plugin DB。

## 本地验证

```bash
cargo run -p awiki-deamon -- init-state --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- status --state-root /tmp/awiki-deamon-state
cargo test -p awiki-deamon --locked
```

步骤 01 不启动真实 runtime，也不连接远端 message-service。

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

daemon 当前提供最小 Generic CLI runtime 闭环：

1. 加载手工配置的 runtime agent profile。
2. 只接受 `sender_did == controller_did` 的 controller 文本消息。
3. 将文本消息标准化为 `RuntimeTask`。
4. 创建 `RuntimeRun`，并签发短期 `runtime_rpc_token`。
5. Generic CLI plugin 启动无界面 runtime，并把 token 注入 runtime launch context。
6. runtime 通过 CLI wrapper 形态的 `task.status` 和 `task.finish` callback 回到 daemon local RPC。
7. daemon 通过 token 反查可信上下文，更新 run 状态，并把 status/final 写入 testable outbox。

MVP 阶段的消息出口是 testable outbox，不直连 message-service。真实 `application/json + body.payload` 产品化闭环会在后续 SDK/service/agent 管理步骤完成。

workspace mode 只记录边界，不夸大安全性：

| mode | 定位 | 是否安全边界 |
|---|---|---|
| `shared-root` | 个人低风险、本机可信、读任务 | 否 |
| `worktree-per-task` | 代码变更隔离、避免任务互相污染 | 部分隔离，不防系统凭据读取 |
| `container` | 外部委托、高风险、自动写代码 | 是，依赖容器配置 |
| `sandbox` | 外部委托、高风险、自动写代码 | 是，依赖 sandbox profile |

RuntimeEvent 当前不作为任务状态和结果的第二条权威通道。权威回传链路是 Skill / daemon CLI wrapper / local RPC。

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

## 长驻 foreground E2E

Step 09 后，`foreground` 不再只是初始化状态后返回，它会作为长驻 daemon 进程运行：

1. 初始化 `daemon.db` 和 `im-core` 本地状态。
2. 将本地 daemon/runtime agent identity 同步到 `im-core` identity registry。
3. 启动 Unix domain socket local RPC worker。
4. 周期轮询 message-service inbox。
5. 消费 `application/json + body.payload` command。
6. 对 `runtime.agent.create` 复用 daemon agent 管理逻辑。
7. 对 `runtime.task.submit` 创建 runtime task/run，并启动 `test-runtime-uds`。
8. 测试 runtime 通过 UDS local RPC 回传 `task.status` 和 `task.finish`。
9. daemon 通过 `im-core` 发回 `awiki.agent.status.v1` payload。

系统测试使用这些控制参数让长驻进程稳定退出：

```bash
awiki-deamon foreground \
  --state-root /tmp/awiki-deamon-state \
  --ready-file /tmp/awiki-deamon-ready.json \
  --max-runtime-ms 30000 \
  --max-processed-messages 2 \
  --poll-interval-ms 100
```

本地同域 E2E 需要 message-service 能解析本地 user-service 刚注册的 agent DID。运行 `tests_v2/daemon/test_awiki_daemon_long_running_e2e.py` 前，确认本地 message-service 的运行配置等价于：

```toml
[did_resolution]
base_url = "http://127.0.0.1:9891"
verify_ssl = false
```

否则 runtime agent DID 会被解析到公网域名，本地刚注册的 DID 文档不可见，status/final 发送会失败。

系统测试入口位于：

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_SYSTEM_TEST_MODE=local \
E2E_USER_SERVICE_URL=http://127.0.0.1:9891 \
E2E_MESSAGE_SERVICE_URL=http://127.0.0.1:9900 \
E2E_MESSAGE_SERVICE_WS_URL=ws://127.0.0.1:9900/im/ws \
E2E_DID_DOMAIN=awiki.info \
AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/awiki-deamon-cli-rs2 \
CARGO_BUILD_JOBS=1 \
uv run --no-project --python .venv/bin/python -m pytest \
  tests_v2/daemon/test_awiki_daemon_long_running_e2e.py -q -rs
```

该测试覆盖：controller command、daemon listener、`controller_did` MVP 校验、runtime run 创建、UDS local RPC progress/final、`application/json + body.payload` status/final、controller history 结果和 audit 不记录 `runtime_rpc_token` 原文。
