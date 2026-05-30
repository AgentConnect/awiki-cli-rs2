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
