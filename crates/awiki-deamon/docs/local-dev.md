# Awiki daemon 本地开发

本目录实现 daemon 进程本身，daemon 与现有 `awiki-cli` 是平行入口，二者都复用 `im-core` SDK。daemon 代码固定放在 `crates/awiki-deamon`，不能依赖 `crates/awiki-cli` 内部模块。

步骤 01 只提供最小进程骨架：

- `awiki-deamon foreground --state-root <path>`
- `awiki-deamon init-state --state-root <path>`
- `awiki-deamon status --state-root <path>`

三个命令都会加载 daemon 配置、初始化 daemon 状态库，并通过 `im-core` 公开 API 初始化 IM 本地状态。后续步骤再补本地 RPC、runtime plugin、daemon agent 和注册能力。

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
