# Step 07：message-service delegated key policy 与 fanout

主 Plan：[../plan.md](../plan.md)  
Step index：07  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T17:45:37Z |
| Completed | 2026-06-09T18:24:04Z |
| Commit | `message-service` `7afa621 message-service: support delegated inbox policy` |
| Review evidence | 2026-06-09 Review：检查 delegated send / inbox / history 的 DID proof、key owner、`keyid` 一致性、DID Document `authentication`、老本地 view 兼容、E2EE filtering、同 DID fanout 和文档契约。发现并修复：delegated local view 补 `validate_auth_scheme`；增加 key 不在 DID Document `authentication` 的拒绝测试；补 delegated send 测试；确认运行时授权输入只来自请求 proof、当前 DID Document `authentication`、key owner 一致性和普通非 E2EE scope。剩余风险：delegated `inbox.mark_read` MVP 明确不开放；撤销实时性依赖 DID Document `authentication` 更新和 message-service DID Document cache/refresh；workspace clippy 被无关 `im-group` 测试 lint 阻塞。 |
| Verification evidence | `cd message-service && cargo test -p im-direct -- --nocapture`：40 passed；`cd message-service && cargo test -p im-storage -- --nocapture`：6 passed，Postgres integration helper 因未设置 `MESSAGE_SERVICE_STORAGE_TEST_DATABASE_URL` 打印 skipped；`cd message-service && cargo test -p im-runtime notify_agent_delivers_to_all_matching_sessions -- --nocapture`：1 passed；`cd message-service && cargo test --workspace`：208 passed，doc tests 0；`cd message-service && cargo clippy -p im-direct --all-targets -- -D warnings`：通过；`cd message-service && cargo clippy -p im-storage --all-targets -- -D warnings`：通过；`cd message-service && cargo clippy --workspace --all-targets -- -D warnings`：失败，既有无关 `crates/im-group/src/handlers.rs:6746` `await_holding_lock`；`cd message-service && git diff --check`：通过。 |
| Next action | 启动 Step 08：APP action schema 与可见性 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：message-service 支持 `user_did#daemon-key-1` 对普通非 E2EE send/inbox/history 的 DID proof；支持同一个 user DID 的 APP 和 Daemon 多连接 fanout。
- 用户 / 系统可见行为：Daemon 可用 user delegated subkey 代用户发送普通消息和拉取普通 inbox/history；APP 与 Daemon 同时在线时都能收到该 DID 的普通消息与 E2EE opaque notification，Daemon 对 E2EE opaque 自行丢弃。
- 非目标：不让 message-service 解密 E2EE；不返回 E2EE 明文、metadata projection 或 private state；不实现 Agent DID delegation / ANP delegated proof。
- 完成标准：proof policy、scope policy、direct/history/inbox handler 和 WebSocket fanout 测试通过；API/architecture 文档更新。

## 3. 设计方法

- 设计边界：message-service 是普通消息路由/存储和 E2EE opaque 路由/存储服务，不持有 E2EE private state，不解密。
- 核心决策：`user_did#daemon-key-1` 可以作为用户 DID authentication key 通过 ANP/RFC9421 proof。MVP 第一版 message-service 运行时授权只校验 DID proof、DID Document `authentication`、key owner 与普通非 E2EE scope；跨服务 policy client 和撤销事件作为后续增强单独设计。
- 契约 / API / 数据流：send 验证 `meta.sender_did = user_did` 与 `keyid = user_did#daemon-key-1`；inbox/history 验证 `inbox_owner_did` 与 `inbox_auth_verification_method`；scope 至少包括 `message.send.plain`、`message.inbox.read.plain`、`message.history.read.plain`。
- 兼容性：老 APP 使用用户主 key 或现有 key 的发送/接收行为不变；新增 delegated key policy 不影响 E2EE API。
- 迁移策略：如果需要 token/cache 表或 connection registry 字段，使用 `message-service/crates/im-storage/migrations/` 迁移。
- 风险控制：delegated inbox/history pull 只返回普通非 E2EE 消息；撤销实时性依赖 DID Document `authentication` 更新和 message-service DID Document cache 刷新。后续版本再补 policy client、cache TTL、撤销事件和 deny-by-default。

## 4. 实现方法

1. 阅读 `message-service/docs/api/ANP-client-server-api-direct.md`、`message-service/docs/architecture/identity-auth-proof-architecture.md`、`message-service/docs/architecture/direct-e2ee-service-boundary.md` 和 `message-service/docs/architecture/group-e2ee-service-boundary.md`。
2. 定位 proof 验证和身份模块，优先检查 `message-service/crates/im-identity/src`、`im-binding/src`、`im-runtime/src`、`im-direct/src`、`im-storage/src` 和 `bins/message-service/src`。
3. 扩展 proof policy：识别 `keyid` 所属 DID 与 sender/owner DID 一致；`#daemon-key-1` 必须存在于用户 DID Document 的 `authentication`；scope 必须限制为普通非 E2EE send/inbox/history。
4. 实现普通 send 权限：只允许 default plain / non-E2EE 普通消息；拒绝 E2EE private state 或 secure send 伪装。
5. 实现 delegated inbox/history pull：接受 DID proof 或预留 `ScopedInboxToken`；返回普通非 E2EE 消息；不返回 E2EE 明文、metadata projection、private state。
6. 实现同 DID 多连接 fanout：connection registry 支持 user DID 下多个 APP/Daemon 连接；下行通知 fanout 给所有连接。普通消息和 E2EE opaque notification 都可下发；不要求服务端识别哪个连接是 Daemon 后过滤 E2EE opaque。
7. 增加 tests：delegated send success/failure、key 不在 authentication 中失败、delegated inbox only plain、E2EE boundary、multi-connection fanout。跨服务 policy client 的 unavailable/deny 测试作为后续增强预留。
8. 更新 API 和 architecture 文档，明确 MVP 不支持 Agent DID delegation / ANP delegated proof。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `message-service/crates/im-identity/src` | DID proof / identity policy 扩展 | 具体文件由执行者定位 |
| `message-service/crates/im-binding/src` | proof / identity policy 接入点 | MVP 运行时授权只校验 DID proof、DID Document `authentication`、owner 和 scope |
| `message-service/crates/im-direct/src` | 普通 direct send/inbox/history handler | 保持 E2EE opaque boundary |
| `message-service/crates/im-runtime/src` | WebSocket/session/fanout 路由 | 支持同 DID 多连接 |
| `message-service/crates/im-storage/src` | 如需 connection/token/policy 存储 | 迁移同步 |
| `message-service/crates/im-storage/migrations/` | 数据库迁移 | 仅在 schema 变更时 |
| `message-service/bins/message-service/src` | 服务入口配置或 route wiring | 视当前结构 |
| `message-service/docs/api/*` | API 文档更新 | delegated proof 和 inbox 命名 |
| `message-service/docs/architecture/*` | 架构边界更新 | E2EE boundary 不变 |

## 6. 依赖

- 前置步骤：Step 01 registry 契约；Step 02 `InboxHistoryOptions` 和 delegated proof optional 参数契约。
- 外部文档或决策：`agent_im_core_design.md` 第 3.2、5.4；`agent_delegated_identity_message_proof_plan.md` 第 3.3、3.4、5.8、5.9。
- 环境前提：Rust 1.79 兼容；如修改 SQL，需要 PostgreSQL 或测试 fixture 支持迁移。

## 7. 验收标准

- [x] send 接受 `meta.sender_did = user_did`、`keyid = user_did#daemon-key-1` 的普通非 E2EE proof。
- [x] inbox/history 接受 `inbox_owner_did`、`inbox_auth_verification_method` 的 delegated proof。
- [x] message-service MVP 只校验 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope；key 不在 DID Document `authentication` 中时拒绝。
- [x] delegated inbox/history pull 不返回 E2EE 明文、metadata projection 或 private state。
- [x] 同一 user DID 的 APP 和 Daemon 多连接均可收到下行 fanout。
- [x] E2EE opaque notification 可以下发给 Daemon 连接，但服务端不解密、不转明文。
- [x] 老客户端 send/inbox/history 和 E2EE 行为不回归。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit/workspace | `cd message-service && cargo test --workspace` | delegated proof、DID Document authentication、fanout、E2EE boundary 测试通过。 |
| Lint | `cd message-service && cargo clippy --workspace --all-targets -- -D warnings` | 无新增 clippy 错误；如耗时或环境问题需记录。 |
| Migration | `cd message-service && sqlx migrate run --source crates/im-storage/migrations` | 如新增迁移，在 clean DB 可运行。 |
| Docs | 检查 `message-service/docs/api/*` 和 `docs/architecture/*` | delegated key scope 和 E2EE boundary 已记录。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：DID proof 验证、DID Document authentication、scope 限制、E2EE boundary、多连接 fanout、老客户端兼容、后续 registry policy client 预留点。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 有发现 | delegated local view 需要显式校验 auth scheme；需要覆盖 key 不在 DID Document `authentication` 的拒绝路径；需要明确 delegated `inbox.mark_read` MVP 不开放。 |
| 已修复问题 | 已修复 | delegated local view 补 `validate_auth_scheme`；新增 keyid mismatch、wrong auth key owner、key not in authentication、delegated send、delegated inbox/history、E2EE filtering 测试；API/architecture 文档明确授权输入只来自 DID proof 与 DID Document `authentication`。 |
| 剩余风险 | 已记录 | delegated `inbox.mark_read` MVP 明确不开放；撤销实时性依赖 DID Document `authentication` 更新和 message-service DID Document cache/refresh；`cargo clippy --workspace --all-targets -- -D warnings` 被无关 `im-group` 测试 lint 阻塞。 |
| 新增或缺失测试 | 已补测试 | `direct_send_accepts_daemon_key_origin_proof`、`delegated_inbox_get_accepts_daemon_key_proof`、`delegated_history_accepts_daemon_key_proof`、wrong owner/keyid/not-in-authentication、E2EE filtering、fanout 定向测试均通过。缺失：Postgres integration 需要 `MESSAGE_SERVICE_STORAGE_TEST_DATABASE_URL`，本轮未提供该环境。 |
| 已更新或缺失文档 | 已更新 | 更新 `message-service/docs/api/ANP-client-server-api-direct.md` 和 `message-service/docs/architecture/identity-auth-proof-architecture.md`；未改 E2EE boundary 文档，因为服务端 opaque boundary 未改变。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 message-service delegated key policy、fanout、测试和直接文档/迁移。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`message-service: support delegated inbox policy`

执行记录：

| 项 | 记录 |
|---|---|
| Commit 前状态 | `message-service`：`## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong`，纳入 Step 07 变更后提交。 |
| 纳入文件 | `crates/im-direct/src/service.rs`、`crates/im-storage/src/lib.rs`、`crates/im-storage/src/postgres.rs`、`docs/api/ANP-client-server-api-direct.md`、`docs/architecture/identity-auth-proof-architecture.md`。 |
| Commit 后证据 | `7afa621 message-service: support delegated inbox policy`；`message-service` post-commit status：`## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 1]`。 |
| 遗留未提交变更 | `message-service` 无遗留未提交变更；本轮 `awiki-cli-rs2` 只回填 Plan / Step 07 台账和两篇设计文档收敛改动。 |

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| DID Document cache 无法及时反映 authentication 撤销 | Step 07 只以当前解析到的 DID Document `authentication` 判定 key 有效性，未引入跨服务撤销事件。 | 记录为 MVP 剩余风险；跨服务 policy client / cache TTL / 撤销事件作为后续增强。 | 当前步骤 / Step 05 / Step 09 | 不阻塞 Step 07 完成；Step 09 全局 Review 继续检查撤销风险记录。 |
| workspace clippy 被无关 `im-group` 测试 lint 阻塞 | `cargo clippy --workspace --all-targets -- -D warnings` 失败于 `crates/im-group/src/handlers.rs:6746` `await_holding_lock`，不在 Step 07 touched files。 | 已运行 touched crate clippy：`im-direct`、`im-storage` 均通过；不修改无关模块。 | 全 workspace lint 证据 | 记录残余风险；Step 09 全局验证时再统一处理或记录。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 07 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | Step 07 收口为 DID Document authentication 授权源 | 实现和 Review 确认 message-service MVP 运行时授权输入只来自请求 proof、当前 DID Document `authentication`、key owner 一致性和普通非 E2EE scope；delegated `inbox.mark_read` 不进入 MVP。 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：把 daemon key 当完整用户 key 放行所有 API。
- 回滚 / 回退：关闭 delegated key policy feature flag，拒绝 `#daemon-key-1`，保留老用户 key 路径。
- 后续文档：更新 message-service API、architecture 和 changelog，说明 delegated inbox/history 仅普通非 E2EE。
