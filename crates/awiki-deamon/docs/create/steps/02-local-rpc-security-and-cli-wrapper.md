# 步骤 02：本地 RPC 安全与 CLI 封装器

主计划：[../plan.md](../plan.md)
步骤编号：02
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 2026-05-31 00:38:20 CST |
| 完成时间 | 2026-05-31 01:08:54 CST |
| 提交 | `395c815` |
| 审查证据 | Review 已完成：token scope、hash 存储、过期/撤销/一次性使用、method/recipient 授权、UDS 权限、Linux `SO_PEERCRED`、macOS `getpeereid`、请求体身份字段不参与授权、audit 不记录 token 原文、CLI wrapper 边界和测试覆盖已审查；发现 macOS peer credential 分支缺失，已补齐。 |
| 验证证据 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；`git diff --check -- Cargo.toml Cargo.lock crates/awiki-deamon` 通过；源码边界 `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无结果；secret 搜索确认生产代码无 token 原文日志，audit 测试确认只记录 `token_id`。 |
| 下一步 | 开始步骤 03：通用 CLI 运行时插件 MVP。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：daemon 暴露给 runtime Skill / CLI 封装器的本地 RPC 端点，并使用短期 `runtime_rpc_token` 与 OS-local 检查保护。
- 可见行为：runtime 可通过封装器调用 `task.status`、`task.finish`、`msg.send`；daemon 根据 token 反查可信上下文，再通过 `im-core` 发消息。
- 非目标：不实现完整通用 CLI 运行时插件；不信任请求体身份字段做授权。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `crates/awiki-deamon/src/local_rpc/` | UDS server、request/response DTO、method routing。 | macOS/Linux 先用 Unix domain socket。 |
| `crates/awiki-deamon/src/security/runtime_token.rs` | token 生成、hash/storage、校验、撤销。 | token 原文只返回一次。 |
| `crates/awiki-deamon/src/state/` | `runtime_rpc_tokens` 和 audit 持久化。 | audit 只记录 `token_id`。 |
| `crates/awiki-deamon/src/cli_wrapper/` | 封装器 command client。 | runtime Skill 调用这个轻量壳。 |
| `crates/awiki-deamon/src/im_core_adapter.rs` | 通过 `im-core` 发送 status/result/message。 | 使用 SDK DTO。 |
| `crates/awiki-deamon/docs/` | 本地 RPC 安全与封装器 command 文档。 | 必须写明 token 和 UDS 规则。 |
| `crates/awiki-deamon/tests/` | token、UDS、method authorization 测试。 | 包含负向测试。 |

## 4. 依赖

- 前置步骤：步骤 01。
- 外部文档：架构文档中的本地 RPC 安全模型。
- 环境前提：macOS/Linux 环境可测试 UDS；Windows 可先文档化延期。

## 5. 核心设计

必须实现的 token 模型：

1. daemon 生成短期 `runtime_rpc_token`。
2. token scope 绑定：
   - `agent_did`
   - `runtime_profile_id`
   - `run_id`
   - `allowed_methods`
   - 可选 `allowed_recipients`
   - `expires_at`
3. CLI 封装器只携带 token，不携带可信身份字段。
4. daemon 根据 token 反查可信上下文。
5. 请求体中的 `agent_did` 如出现，只能用于 display/debug，不参与授权。

必须实现的安全控制：

- Unix domain socket 文件权限。
- macOS/Linux peer credential 校验，例如 `SO_PEERCRED` 或等价机制。
- token 原文不写日志。
- audit 只记录 `token_id`。
- token 可撤销。
- token 可一次性或短期有效。
- RPC method 分级，例如 read/status/send/finalize/admin。

## 6. 实施指引

1. 定义 RPC method set：
   - `rpc.ping`
   - `task.status`
   - `task.finish`
   - `msg.send`
   - 可选 `artifact.created`
2. 定义 method level 和 scope 校验。
3. 实现 token table，存 hashed token secret 和生命周期字段。
4. 实现 daemon 内部 runtime launch 时的 token issue。
5. 实现 UDS server，严格限制 socket path 和 parent directory 权限。
6. 在支持平台实现 peer credential 校验。
7. 实现 CLI 封装器 client command parsing。
8. 确保日志脱敏，audit 只记录 `token_id`。
9. 增加测试：
   - valid token 可调用 allowed method。
   - expired/revoked/used token 被拒绝。
   - scope 外 method 被拒绝。
   - scope 外 recipient 被拒绝。
   - 请求体 `agent_did` mismatch 不能授权。
   - 日志/audit 不包含 token 原文。

## 7. 验收标准

- [x] 本地 RPC 使用 UDS 且文件权限受限。
- [x] peer credential check 已实现，或平台 gated 原因已明确记录。
- [x] `runtime_rpc_token` scope 包含必需字段。
- [x] CLI 封装器只把 token 作为授权材料。
- [x] daemon 从 token storage 反查可信上下文。
- [x] token 原文不进入日志或 audit。
- [x] audit 只记录 `token_id`。
- [x] method level authorization 被强制执行。
- [x] 测试覆盖成功和失败路径。
- [x] 审查发现已修复或明确记录。
- [x] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 格式通过。 |
| daemon 测试 | `cargo test -p awiki-deamon --locked` 或 workspace 等价命令 | 本地 RPC/token 测试通过。 |
| workspace 测试 | `cargo test --workspace --locked` | 无 workspace 回归。 |
| secret 搜索 | 检查日志、测试和 token print/debug pattern | token 原文不落日志和 audit。 |
| socket 权限 | temp 状态根目录集成测试或手工冒烟验证 | socket mode 和父目录权限受限。 |
| 安全审查 | 手工审查 token、peer credential、method scope | 发现已记录。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查本地权限假设、token 生命周期、日志、audit、method scope、recipient scope、重放风险和测试。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 1 项 | macOS peer credential 分支缺失，与 macOS/Linux peer credential 要求不完全一致。 |
| 已修复 | 已修复 | 补充 macOS `getpeereid` 分支；Linux 继续使用 `SO_PEERCRED`；其他 Unix 平台保留明确 gated 错误。 |
| 残余风险 | 已记录 | 当前 UDS server 只提供测试可用的单请求处理；真实长驻 listener、Windows named pipe、runtime 调 im-core 发送消息在后续步骤实现。 |
| 测试缺口 | 可接受 | Linux 环境已覆盖 UDS 权限、同 UID peer credential、token 生命周期、scope 和 audit；macOS 分支依赖目标平台后续验证。 |
| 文档缺口 | 已补充 | `docs/local-dev.md` 已记录 token scope、UDS 权限、peer credential、audit 规则和平台边界。 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：本地 RPC server、token security、CLI 封装器、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`daemon: secure runtime local rpc`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：不同平台 peer credential 检查差异大；token 原文误打日志；封装器 UX 膨胀成完整 CLI。
- 回滚：禁用本地 RPC listener 和 runtime callback path，直到 token 校验修复。
- 后续：步骤 03 runtime 插件必须使用该封装器，不能直连 message-service。
