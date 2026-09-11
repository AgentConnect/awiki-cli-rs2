# ACP Runtime Plugin

日期：2026-08-13  
状态：首个 catalog 条目 `deepseek-harness` 已实现

## 1. 边界

ACP 以 native runtime family 接入 awiki daemon，plugin id 固定为 `runtime.acp`。它不属于 `generic-cli`：ACP 子进程是常驻的 stdio NDJSON JSON-RPC 服务，拥有原生 session、流式 update 和 permission 反向请求。

实现分成两层：

- `runtime.acp`：协议编解码、进程池、connection epoch、session 路由、permission 回答、prompt 上下文和 final outcome。
- ACP Agent Catalog：具体 Agent 的显示名、运行程序与最低版本、npm/local entrypoint、local package links、启动 cwd/args、允许继承的环境变量、配置渲染、凭证变量和默认 permission policy。

当前 catalog 只包含 `deepseek-harness`。runtime alias 识别、`driver_id` 校验和 daemon 环境捕获会从 catalog registry 自动派生；增加其他兼容 Agent 时只注册新的 catalog 条目，不修改通用 install/runner 或新增 runtime plugin id。

## 2. DeepSeek Harness 契约

默认 npm 版本固定为 `@deepseek-ai/dsh-acp-demo@0.1.0-rc.6`，并以相同版本安装配置引用的 DeepSeek Harness 插件包。运行前要求 Node.js 22.19 或更高版本。

支持两种来源：

- `npm`（默认）：安装到 `<state_root>/runtime/acp/deepseek-harness`，安装过程有 10 分钟超时、进程组清理和文件锁。显式覆盖版本必须是精确 SemVer（允许预发布版本），不接受 `latest`、`next`、版本范围或带前导零的数值段。npm 子进程先清空继承环境，只恢复安装所需的 PATH、代理、locale 和临时目录变量，并使用 state root 下权限为 `0700` 的私有 cache。
- `local`：使用已经构建的 checkout；必须存在 `packages/examples/acp-demo/lib/bin.js`。缺失时先在 checkout 中执行 `pnpm install && pnpm run build`，daemon 不代跑构建。

每个 runtime profile 拥有私有目录：

```text
<state_root>/runtime/acp/profiles/<runtime_profile_id>/
├── .env
├── cordis.yml
├── sessions/
└── workspace/
```

目录权限为 `0700`，`.env` 与生成配置为 `0600`。生成的最小 `cordis.yml` 只启用 DeepSeek LLM adapter、local sandbox、sandbox policy、subprocess、bash、approval、fs policy、fs/bash tools 和 ACP app；不包含 hook、subagent 或 workflow。

## 3. 安装入口

先装后建可使用 daemon CLI：

```bash
awiki-deamon runtime-install \
  --state-root /absolute/path/to/state \
  --acp-agent-id deepseek-harness \
  --install-mode npm \
  --package-version 0.1.0-rc.6
```

本地 checkout 模式：

```bash
awiki-deamon runtime-install \
  --state-root /absolute/path/to/state \
  --acp-agent-id deepseek-harness \
  --install-mode local \
  --local-checkout /absolute/path/to/deepseek-harness
```

Daemon Agent JSON 命令也支持 `runtime.install`：

```json
{
  "schema": "awiki.agent.command.v1",
  "command_id": "install-deepseek-harness-1",
  "command": "runtime.install",
  "args": {
    "acp_agent_id": "deepseek-harness",
    "install_mode": "npm",
    "package_version": "0.1.0-rc.6"
  }
}
```

CLI 与 JSON 两个入口都会在 daemon state 中记录 `runtime.install` 成功或失败审计；审计只包含来源、Agent ID、安装模式、包名/版本或脱敏失败摘要，不包含凭证值。

安装状态与审计只记录 Agent id、来源、固定版本和 npm 包名/版本，不记录 token。

## 4. 创建 Runtime Agent

`runtime.agent.create` 可使用 `runtime: "acp"`，也可使用别名 `runtime: "deepseek-harness"`。`driver_id` 在 ACP family 内表示 `acp_agent_id`，省略时默认 `deepseek-harness`。

npm 示例：

```json
{
  "schema": "awiki.agent.command.v1",
  "command_id": "create-deepseek-agent-1",
  "command": "runtime.agent.create",
  "args": {
    "runtime": "acp",
    "driver_id": "deepseek-harness",
    "handle": "alice-deepseek",
    "controller_did": "did:example:alice",
    "registration_token": "<short-lived-registration-token>",
    "driver_config": {
      "install_mode": "npm",
      "package_version": "0.1.0-rc.6",
      "permission_policy": "allow-once"
    },
    "secrets": {
      "DEEPSEEK_API_KEY": "<api-key>",
      "DEEPSEEK_BASE_URL": "<optional-base-url>"
    }
  }
}
```

local 模式把 `driver_config.install_mode` 改为 `local`，并提供绝对路径 `driver_config.local_checkout`。

创建流程会安装或校验 runtime、生成配置与 `.env`、写入 `acp_runtime_profile`，然后执行一次真实 ACP `initialize` 握手冒烟。任一步失败时 profile 状态变为 `failed`，错误摘要经脱敏后进入 status/audit。

运行前再次执行 `check_install_status`。profile 不再为 `ready`、配置/程序缺失或 catalog 必需凭证已无法从 profile `.env`/daemon 白名单环境取得时，host 返回 `runtime_not_installed`，并在状态 metadata 中给出 `next_action: setup_required`；不会创建 ACP session，也不会伪造 final。

## 5. 凭证边界

- `DEEPSEEK_API_KEY` 必需，`DEEPSEEK_BASE_URL` 可选。
- `secrets` 中只接受 catalog 声明的变量名。
- 数据库仅保存 `credential_env_names_json`，不保存值。
- 未在 `secrets` 提供时，可从 daemon 进程环境或 `cli-env-capture` 白名单回落。
- ACP 子进程使用清空后的环境，只注入 profile 允许的凭证和运行所需的 PATH、代理、locale、临时目录变量。
- `.env` 写入和读取都拒绝符号链接；Unix 写入/读取额外使用 `O_NOFOLLOW`，读取时拒绝 group/other 权限，避免 profile 凭证被重定向或从宽权限文件加载。
- process spec 的 Debug、stderr 摘要、用户错误和审计均不得输出凭证值。

## 6. 协议与进程

客户端固定协商 ACP `protocolVersion = 1`，实现：

- 请求：`initialize`、`session/new`、`session/prompt`。
- 通知：`session/cancel`。
- 入站：`session/update`、`session/request_permission` 和对应 response。

`session/new` 始终使用绝对 cwd，`mcpServers` 与 `additionalDirectories` 为空。prompt 当前只发送 text content；客户端 fs、terminal、auth terminal 等反向能力均声明为不可用。

每个 runtime profile 对应一个常驻 ACP 进程。stdout 专用于协议帧，stderr 保留最近 20 条已脱敏诊断。默认超时为：initialize 10 秒、session/new 120 秒、首个 prompt update 60 秒、prompt 总时长 30 分钟。子进程异常与超时会清理进程组。同一 runner 已有 in-flight prompt 时，新 prompt 会被明确拒绝；`session/cancel` 使用独立的并发写入路径，可在原 prompt 等待响应期间送达 ACP 子进程。

permission 请求按 profile 策略选择对方提供的 `allow-once` 或 `reject-once`；默认 `allow-once`。

## 7. Session 与结果回传

route key 与 Hermes 一样由 `agent_did + controller_scope_key + conversation_scope` 构成。`acp_native_sessions` 同时保存 ACP session id 与 `connection_epoch`。

DeepSeek Harness 当前不支持 resume/load。只在同一进程 epoch 内复用 session；进程或 daemon 重启后旧 session 标记为 stale，新 prompt 会创建新 session，在 launch metadata 中标记 `session_recreated`，并向 App/CLI 发送 `ACP subprocess restarted; created a new session` running status。

`session/update` 中的 `agent_message_chunk` 文本按顺序聚合为 `metadata.final_text`。`session/prompt` response 的 `stopReason`、permission 次数、epoch 和 session id 一并进入 metadata。host 通过 native runtime 通用契约写入并刷新 `runtime_final_outbox`，再以 Runtime Agent DID 把普通最终消息发回 controller/requester。

## 8. 验证

不依赖真实模型的门禁：

```bash
cargo test -p awiki-deamon --locked \
  --test acp_protocol \
  --test acp_connection \
  --test acp_profile \
  --test acp_message
```

跨仓库进程级系统/E2E 用例位于 `awiki-system-test/tests_v2/daemon/test_acp_runtime_e2e.py`。它使用 AWiki Me 的真实 daemon control payload probe、真实 daemon foreground 和确定性 local ACP server，覆盖 profile 初始化、permission、route/session、子进程重启、stale、final outbox 以及 controller 历史回传；不把凭证值或原始 prompt 写入测试日志。该用例需要 local user-service/message-service v2 测试栈，远端模式不会把缺少本地拓扑伪装为通过。

本机存在已构建 `/home/ecs-user/deepseek-harness` 时，`acp_local_live` 会运行真实 ACP initialize 握手；否则该测试明确跳过。完整真机 prompt 联调还需要有效的 `DEEPSEEK_API_KEY`，不得把密钥写入仓库、测试输出或文档。

当前明确限制：进程重启即新 session；不提供 ACP fs/terminal 反向能力；同一 session 同时只允许一个 in-flight prompt。
