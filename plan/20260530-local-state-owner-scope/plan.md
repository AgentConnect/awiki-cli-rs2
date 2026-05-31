# 计划：本地状态所有者作用域重构

状态：执行中
文档目录：`plan/20260530-local-state-owner-scope/`  
Harness：`/home/ecs-user/awiki-space/awiki-harness`  
创建日期：2026-05-30  
恢复执行位置：步骤 09 review；继续时先读取 [steps/09-docs-diagnostics-dart.md](steps/09-docs-diagnostics-dart.md) 和当前 `git status`

## 1. 目标

- 目标：在 AWiki 上线前，把本地 SQLite 的所有者隔离从可变的 `owner_did` 主键模型迁移到稳定的 `owner_identity_id` 主键模型。
- 期望行为：本地业务行按 `owner_identity_id` 分区；DID 恢复或替换只更新身份元数据和 DID 历史，不移动业务所有权；App、CLI、listener 共用 `im-core` 的本地状态契约。
- 非目标：服务端数据库迁移、ANP 协议变更、`user-service` 所有权变更、支持两个 live identity 共享一个 DID、向 CLI/App 暴露裸 SQLite。
- 完成标准：活跃本地状态表不再用 `owner_did` 建主键；活跃运行时查询不再使用 owner-DID 回退；迁移/重建路径会备份并执行不变量检查；重复 live DID fail closed；Secure 公开发现继续禁用；secure diagnostics/DTO 保持脱敏；仓库验证和 `../awiki-system-test` 下 remote 模式、`awiki.info` 域名的完整系统测试证据被记录。

## 2. 对原方案的审查结论

更新后的方案方向正确，应替代上一版短期加固思路。由于产品还未上线，直接落到 identity-owned schema 是更合理的目标。

实施前必须纳入以下修正：

- SQLite 不能原地修改主键。实现必须通过重建表或重建数据库完成，不能依赖 `ALTER TABLE` 修改主键。
- schema 切换必须对所有活跃本地状态表原子完成。`SCHEMA_VERSION` 一旦切到新版本，所有 `ON CONFLICT(owner_did, ...)`、全局 `event_id`、owner 回退查询都必须一起改掉。
- `conversation_id` 应作为稳定存储键加入。公开 `ThreadId`/Dart DTO 的兼容要单独处理；过渡期间 `thread_id` 可以作为等于 `conversation_id` 的别名。
- `owner_identity_id TEXT NOT NULL` 意味着迁移必须能解析所有权，否则要隔离、备份后重建，不能在运行时长期保留 credential/DID 回退。
- `identity_did_history` 需要事务化处理 current DID：先把旧 current 标成 `previous`，再插入新 current，并在 registry 验证阶段对重复 live DID fail closed。
- `e2ee_outbox` 改为 `(owner_identity_id, outbox_id)` 会影响 retry/drop/list/get 等 API，内部操作必须携带 owner scope。
- Group MLS 私有状态可能在业务表外的 provider/local 文件中，计划必须验证 SQLite 摘要和 provider/device 状态都按 `owner_identity_id + device_id` 隔离。
- 当前 workspace schema latest 是 3，本次应新增明确的 v3 到 v4 migration，不能把逻辑藏进旧 migration。
- Secure surface 不能因本次存储重构而扩大。Direct E2EE 和 Group E2EE public discovery 必须继续禁用；低层 group E2EE API 继续 hidden/internal；任何公开 DTO、CLI、doctor、日志、文档都不得暴露私钥、ratchet/MLS state、raw ciphertext、plaintext outbox payload、provider stdout/stderr、token 或备份内容。

这些不是目标阻塞项，但必须作为执行约束进入每一步。

## 3. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | 要求先做 Harness 路由、影响面识别、风险和验证记录。 |
| `awiki-harness/README.md` | 说明 Harness 是控制平面；子仓库代码和文档仍是实现权威。 |
| `context/00-context-map.md` | 将任务路由到 Client Architecture、Storage、Identity、Message Flow、E2EE。 |
| `context/02-repo-map.md` | 确认 `awiki-cli-rs2/crates/im-core` 是共享 SDK，`awiki-cli` 是 CLI 壳。 |
| `context/03-cross-repo-architecture.md` | 确认 CLI/App 不能手写 owner DID、SQLite、wire 或 E2EE internals。 |
| `context/20-rules-index.md` | 路由到架构、AI coding、文档和验证规则。 |
| `context/30-tools-env.md` | 提供仓库和系统测试验证命令线索。 |
| `context/40-verification.md` | 将本任务归类为 L3 identity/storage/security，需要安全审查和 E2E 证据。 |
| `context/50-task-workflow.md` | 要求非平凡任务记录上下文、分析、计划、决策和验证证据。 |
| `nodes/client-architecture.node.md` | 确认 App/CLI 必须消费 `im-core`，不能直接访问 SQLite。 |
| `nodes/storage.node.md` | 确认本地状态所有权、迁移文档和测试必须更新。 |
| `nodes/identity.node.md` | 确认 DID 身份不变量和 fail closed 行为。 |
| `nodes/message-flow.node.md` | 确认消息 projection 归 `im-core`。 |
| `nodes/e2ee.node.md` | 确认 E2EE 私有状态属于客户端本地，且需要安全审查。 |
| `features/direct-e2ee.md` | 确认 direct E2EE 公开发现禁用，status/log/output 必须脱敏。 |
| `features/group-e2ee.md` | 确认 group E2EE 公开发现禁用，已有安全姿态阻止 discovery，低层 MLS 操作 hidden/internal。 |
| `docs/sdk-refactor/modules/10-secure.md` | 确认 secure public API 只暴露领域状态，不暴露 raw crypto/session/provider artifacts。 |
| `docs/architecture/direct-e2ee-operations.md` | 确认 secure direct 本地状态按 identity 隔离，CLI 输出必须脱敏。 |
| `docs/architecture/group-e2ee-operations.md` | 确认 CLI 通过 `im-core` 编排 group secure，不能暴露 raw MLS artifacts。 |

## 4. 影响面分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| `awiki-cli-rs2/crates/im-core` 本地状态 | schema vNext、`OwnerScope`、actor commands、表重建、不变量检查。 | `crates/im-core/src/internal/local_state/*.rs` |
| 消息运行时 | 稳定 `conversation_id`、direct thread 生成、mark-read、conversations、本地 projection。 | `crates/im-core/src/internal/message_runtime/*`, `crates/im-core/src/messages/dto.rs` |
| 联系人/目录 | contacts、handle bindings、relationship events 按 owner identity 建键。 | `crates/im-core/src/internal/contact_store/records.rs`, `crates/im-core/src/directory/service.rs` |
| 群组 | groups、members 按 owner identity 建键；group E2EE 摘要保持 scoped。 | `crates/im-core/src/internal/local_state/groups.rs`, `crates/im-core/src/groups/*` |
| E2EE | `e2ee_outbox` 按 owner identity scoped；direct tables 保持 identity-owned；legacy `e2ee_sessions` 隔离或忽略。 | `crates/im-core/src/internal/store/e2ee_outbox.rs`, `secure_direct/sqlite_store.rs` |
| Secure 公开接口 | 保持脱敏的 direct/group secure status、hidden group low-level commands、disabled discovery/capability advertisement。 | `docs/sdk-refactor/modules/10-secure.md`, `docs/architecture/direct-e2ee-operations.md`, `docs/architecture/group-e2ee-operations.md` |
| Identity registry | 校验 identity id、alias、live DID、handle 唯一性；维护 DID history。 | `crates/im-core/src/identity/registry.rs`, `internal/identity_store.rs` |
| DID recover/replace | 停止业务行 owner rebind；更新 DID history/current snapshots；只在必要时处理 secure material。 | `internal/identity_recovery_runtime.rs`, `identity_replace_did_execution.rs` |
| Workspace upgrade | 新增 v3 到 v4 migration；备份/重建数据库；删除或隔离 legacy rebind。 | `crates/awiki-cli/src/workspace_upgrade/*` |
| Flutter/Dart SDK | 如果公开 conversation DTO 变化，需要更新 `conversation_id`/thread alias 映射和生成文件。 | `crates/im-core-dart/*`, `packages/awiki_im_core/*` |
| 文档和 diagnostics | 新增 owner model 文档、本地状态升级文档、doctor 检查。 | `docs/architecture/*`, `crates/awiki-cli/src/diagnostics/*` |

## 5. Secure 要求基线

以下 门禁 适用于每一个实现步骤，不只适用于步骤 07。

- 客户端私有 secure state 继续属于 `im-core`/client local storage。服务端继续只存储和路由 opaque E2EE artifacts；本计划不得把 plaintext、private keys、ratchet state 或 MLS private state 移到服务端。
- Direct E2EE 公开发现继续禁用。任何实现步骤都不得在默认 DID/service discovery 中 advertise `anp.direct.e2ee.v1` 或 `direct-e2ee`，除非另有单独安全评审通过的 enablement 计划。
- Group E2EE 公开发现继续禁用。任何实现步骤都不得在默认 discovery 中 advertise `anp.group.e2ee.v1` 或 `group-e2ee`；现有 `BLOCK_DISCOVERY` 姿态保持不变，除非另有新的安全评审明确覆盖它。
- CLI/App/Dart public API 只暴露 secure domain status 和 repair summary，不暴露 raw ciphertext、plaintext outbox payload、private key material、session counters、ratchet keys、KeyPackage/Welcome/Commit/Proposal payloads、raw MLS notices、provider stdout/stderr、provider binary paths、JWT 或 backup contents。
- `e2ee_outbox.plaintext` 可以暂时保留为内部 SQLite 实现细节，但 public `SecureOutboxEntry`、diagnostics、logs 和 docs 示例必须只展示脱敏摘要。
- Group MLS provider state 和 path selection 必须继续按 `owner_identity_id + device_id` scoped；低层 group E2EE 编排命令继续 hidden/internal 或 stable unsupported。
- Workspace upgrade backups 是敏感本地 artifacts。执行计划时必须保留现有 backup/lock/journal 行为，避免打印 secret paths 或内容，不得在 manifests、diagnostics 或正常日志中包含 private PEM、JWT 或 secure payload material。
- DID recover/replace 只有在 cryptographic key material 确实变化时才更新 secure material。单纯 owner DID 变化不得 reset、跨 identity merge 或泄露 secure sessions。

## 6. 假设和开放问题

### 假设

- 生产数据尚不存在；当确定性迁移无法分配所有权时，允许备份后重建本地库。
- `owner_identity_id` 对应 `IdentitySummary.id`，不是 `local_alias` 或 `credential_name`。
- `owner_did` 继续作为当前 DID snapshot，用于展示、调试或 wire 兼容，不作为分区键。
- 已提交的 Dart generated files 在 DTO 变化时必须重新生成。
- 当前 workspace schema latest 是 3；本次重构应成为 workspace schema 4，除非实现时发现已有更高版本。

### 开放问题

- Rust/Dart public DTO 是否立即暴露 `conversation_id`，还是先只保留 `thread_id` 并让 storage 内部先迁移。
- 旧 `e2ee_sessions` 在 v17 中是物理删除，还是迁移为 `legacy_e2ee_sessions` 仅供诊断。
- unresolved legacy rows 是隔离到 SQLite quarantine tables，还是备份后重建整个本地数据库。

这些不阻塞步骤 01；但必须在步骤 03 激活 v17 前做决定。

## 7. 任务拆分

| 步骤 | 标题 | 依赖 | 输出 | 步骤文档 | 提交 门禁 | 状态 |
|---|---|---|---|---|---|---|
| 01 | 所有者模型和不变量 | 无 | `OwnerScope`、校验 helper、registry invariant tests | [steps/01-owner-model.md](steps/01-owner-model.md) | 必须 | done |
| 02 | v17 schema 和重建脚手架 | 01 | identity-owned schema builder、rebuild/migration helpers、invariant SQL | [steps/02-schema-rebuild-scaffold.md](steps/02-schema-rebuild-scaffold.md) | 必须 | done |
| 03 | 原子 SQL key 切换 | 02 | 活跃表和运行时 SQL 使用 `owner_identity_id` keys | [steps/03-atomic-sql-cutover.md](steps/03-atomic-sql-cutover.md) | 必须 | done |
| 04 | 稳定消息会话 | 03 | 稳定 `conversation_id`，direct conversation key 不再包含本地 owner DID | [steps/04-message-conversations.md](steps/04-message-conversations.md) | 必须 | done |
| 05 | 联系人和关系事件 | 03 | identity-owned contact 和 relationship semantics | [steps/05-contacts-relationships.md](steps/05-contacts-relationships.md) | 必须 | done |
| 06 | 群组和成员 | 03 | identity-owned groups、members、snapshots、cached messages | [steps/06-groups-members.md](steps/06-groups-members.md) | 必须 | done |
| 07 | Secure 本地状态和 公开接口 门禁 | 03,04 | identity-scoped outbox、direct E2EE 验证、group E2EE device 检查、disabled discovery/redaction 门禁 | [steps/07-secure-local-state.md](steps/07-secure-local-state.md) | 必须 | done |
| 08 | Recover/replace 和 workspace upgrade | 01-07 | DID history、v3 到 v4 workspace migration、legacy rebind 退役 | [steps/08-recover-upgrade.md](steps/08-recover-upgrade.md) | 必须 | done |
| 09 | 回退移除、diagnostics、文档和 Dart | 01-08 | 无 runtime 回退、redacted doctor checks、文档和 codegen 更新 | [steps/09-docs-diagnostics-dart.md](steps/09-docs-diagnostics-dart.md) | 必须 | review |
| 10 | 集成门禁 | 01-09 | 完整验证报告、完整系统测试和最终清理 | [steps/10-integration-gate.md](steps/10-integration-gate.md) | 有变更时必须 | pending |

## 8. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| 步骤 | 状态 | 分支 | 开始 | 完成 | 提交 | 审查证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T16:18:42Z | 2026-05-30T16:30:27Z | 步骤 01 聚焦提交：`im-core: add local owner scope invariants` | 提交前审查：修复 registry snapshot 在应用 `default_identity` 标志前校验可能误判重复默认身份的问题；复查未修改 schema、SQL conflict target、secure discovery 或 public secure DTO。 | `cargo fmt --all --check` 通过；`cargo test -p im-core --locked owner_scope` 通过；`cargo test -p im-core --locked identity_registry` 通过；`cargo check -p im-core --locked` 通过；Secure 搜索仅命中既有 internal/profile/docs。 | 步骤 02 开始前读取步骤 02 文档和 `git status` |
| 02 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T16:34:45Z | 2026-05-30T16:45:37Z | 步骤 02 聚焦提交：`im-core: scaffold identity-owned local state schema` | 提交前审查：v17 schema helper 未被 `ensure_schema` 调用，活跃 `SCHEMA_VERSION` 保持 16；v17 主键使用 `owner_identity_id`；unresolved rebuild row 只返回 table/key/reason；未新增日志或 secure discovery/public DTO。 | `cargo fmt --all --check` 通过；`cargo test -p im-core --locked local_state_schema_v17` 通过；`cargo test -p im-core --locked owner_invariant` 通过；`cargo check -p im-core --locked` 通过；`rg "println!|eprintln!|tracing::|log::" crates/im-core/src/internal/local_state crates/im-core/src/internal/identity_recover_local_state` 无命中。 | 步骤 03 开始前读取步骤 03 文档和 `git status` |
| 03 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T16:52:11Z | 2026-05-30T18:06:59Z | 实现提交 `69d8d61`：`im-core: switch local state keys to owner identity` | 提交前审查：确认 active v17 schema、messages、contacts、groups、conversations、mail、recover、replace-did 计数和 `e2ee_outbox` 均按 `owner_identity_id` 分区；修复 recover merge 把 `final_credential_name` 当作 `owner_identity_id` 的问题，改为用保存后的 identity `unique_id` 写 owner key、`final_identity_name` 只作为 `credential_name` metadata；确认 DID-only wrapper fail closed；未新增 Secure public discovery 或 raw secure output。 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked local_state` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked contact_store` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked e2ee_outbox` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked identity_recovery` 通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；SQL 搜索仅命中 legacy DDL、未进入 active v17 的 legacy helper 和测试夹具。 | 步骤 04 开始前读取步骤 04 文档和 `git status` |
| 04 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T18:16:06Z | 2026-05-30T18:52:48Z | 实现提交 `332aec4`：`im-core: stabilize local conversation keys` | 提交前审查：确认内部 `MessageRecord`、active messages 写入、`threads` view、conversation list、direct/group/realtime projection、secure outbox flush、recover merge 和 replace-DID fixture 均使用稳定 `conversation_id`；新写入强制 `thread_id = conversation_id`；legacy `dm:<owner>:<peer>` alias 会归一到 `dm:<peer>`；未改变 public Rust/Dart DTO，未新增 Secure discovery 或 raw secure output。 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked conversation` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked local_projection` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked local_state_messages` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked secure_outbox` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked identity_recovery` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked replace_did` 通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；Step 04 搜索命中已分类为 secure direct session/outbox owner scope 和 Secure redacted 计数/internal operation 名称。 | 步骤 05 开始前读取步骤 05 文档和 `git status` |
| 05 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T18:56:15Z | 2026-05-30T19:11:24Z | 实现提交 `e756516`：`im-core: key contacts and relationship events by owner identity` | 提交前审查完成：联系人保存/目录解析/关系投影都从 `OwnerScope` 派生 owner 信息；contact update、handle binding upsert 和旧 handle 清理都有 affected-row checks；新增测试覆盖 DID snapshot 变化后一行更新、handle current uniqueness 按 owner identity scoped、相同 relationship `event_id` 可跨 owner 存储；未发现 Secure discovery 或 public secure DTO 变化；secret 搜索仅命中测试 fixture 假 token。提交后状态：分支 ahead 7，工作区干净。 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked contact` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked relationship` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked --test phase2_identity_directory` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked --test phase2_relationship_directory` 通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；legacy SQL 搜索无命中；secure secret 搜索仅命中测试 fixture 中的 `"jwt_token":"token"`。 | 步骤 06 开始前读取步骤 06 文档和 `git status` |
| 06 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T19:13:21Z | 2026-05-30T19:34:37Z | 实现提交 `f98fbec`：`im-core: key groups by owner identity` | 提交前审查完成：group snapshot、summary、members、messages 和 left projection 统一从 `OwnerScope::for_client` 派生 owner；`groups` upsert 使用 `(owner_identity_id, group_id)`；`group_members` replacement 和 left cleanup 只按 `owner_identity_id + group_id` 删除；无状态 stale projection 不会把 `left` group 重新激活；cached group mark-read fixture 使用 `owner_identity_id` 和稳定 group `conversation_id`；未新增 Secure discovery、public group E2EE command surface、raw MLS artifact、provider stdout/stderr/path 输出。提交后状态：分支 ahead 9，工作区干净。 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked group` 通过，33 个匹配测试通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --features group-e2ee --locked group_e2ee` 通过，60 个匹配测试通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；legacy group SQL 搜索无命中；group E2EE 搜索命中分类为既有 docs、internal service/test code 和 hidden CLI command catalog，没有本步骤新增 public output 或默认 discovery。 | 步骤 07 开始前读取步骤 07 文档和 `git status` |
| 07 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T19:35:53Z | 2026-05-30T20:20:56Z | 实现提交 `1eb0ce1`：`im-core: scope secure local state by owner identity` | 提交前审查完成：`e2ee_outbox` 的 get/list/retry/drop/mark-sent/failure update 都按 `owner_identity_id` strict predicate 执行，不再使用 credential/DID 回退；direct secure status/repair 的 pending/requeue 按 `owner_identity_id + peer_did` scoped；Group MLS provider 继续按 `owner_identity_id + device_id` scoped；Group E2EE dry-run plan 和 doctor `anp_mls` details 已移除 provider binary、MLS state path、state.db/state.lock 和 scoped state path 输出；审查发现 doctor I/O error 文本可能包含本地路径，已改为 `ErrorKind` 级别脱敏，并将 provider `binary_name` 兼容错误限制为文件名；低层 group E2EE command catalog 仍 hidden/internal；未发现默认 DID/service discovery 新增 direct/group E2EE advertisement。提交后状态：分支 ahead 11，工作区干净。 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked e2ee_outbox` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked direct_secure` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --features group-e2ee --locked group_e2ee` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked msg_secure` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked group_secure` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked group_e2ee_dry_run_plans_match_go_contracts` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked doctor_anp_mls_probe_and_state_details_match_go_contract` 脱敏修复后通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；redaction/discovery/legacy fallback 搜索命中已分类，无新增 public raw secure output 或默认 advertisement。过宽 `cargo test -p awiki-cli --locked e2ee` 失败在步骤 08 范围的 legacy workspace upgrade fixture 缺少 `messages.owner_identity_id`。 | 步骤 08 开始前读取步骤 08 文档和 `git status` |
| 08 | done | `feature/release-0526/db-refactor-in-async` | 2026-05-30T20:22:58Z | 2026-05-31T00:40:33Z | 实现提交 `20f21b6`：`awiki-cli: migrate local state to owner identity schema` | 提交前审查：recover/replace 已改为 DID history 和 owner_did snapshot refresh，不做业务 owner rebind；`LegacyOwnerLookup` 生产调用已改为使用 `IdentitySummary.unique_id`；审查发现并修复旧 schema v3->v4 未执行 clean rebuild 的问题，改为确认 workspace SQLite backup 后删除旧 DB 文件集并创建干净 v17；审查发现并修复 legacy import 显式未知 `owner_did`/`credential_name` 会落到 default owner 的问题，改为 fail closed 并补测试；未新增 Secure discovery/default advertisement 或 public raw secure output。提交后状态：分支 ahead 13，工作区干净。 | `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked workspace_upgrade` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked import_legacy_database` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked recover` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked replace_did` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked --test identity_replace_did_upgrade_contract` 通过；`CARGO_BUILD_JOBS=1 cargo check -p awiki-cli --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；redaction/discovery/legacy rebind 搜索命中已分类。 | 步骤 09 开始前读取步骤 09 文档和 `git status` |
| 09 | review | `feature/release-0526/db-refactor-in-async` | 2026-05-31T00:42:21Z | | | 提交前审查：`replace_did` 计划已停止按 `owner_did` 扫描业务表，只保留兼容零计数字段；`doctor` 只输出 owner invariant 的 table/invariant/row_count 和 legacy secure table 计数，不输出 raw SQLite rows、plaintext、JWT、private key 或 raw E2EE/MLS artifacts；新增文档保持 Direct/Group E2EE public discovery disabled；Dart generated diff 仅更新 loader stem，无 DTO 形状变化。 | `cargo fmt --all --check` 通过；`git diff --check` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked diagnostics` 通过但匹配 0 个用例，已补跑 `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked --test diagnostics_contract`，5 个用例通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked replace_did` 通过；`CARGO_BUILD_JOBS=1 cargo check -p awiki-cli --locked` 通过；`scripts/flutter/codegen-check.sh` 保留 generated loader stem 后第二次通过；fallback/redaction/discovery 搜索已分类。 | 创建步骤 09 聚焦提交并回填 commit hash |
| 10 | pending | `feature/release-0526/db-refactor-in-async` | | | | | | 等待步骤 09 |

## 9. Codex Goal 执行协议

- 以本计划作为执行进度的唯一事实来源。
- 每次开始或恢复执行前，先阅读本计划、当前步骤文档、执行台账和当前 `git status`。
- 除非本计划明确标记某些步骤可并行，否则同一时间只执行一个步骤。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤：标记 `in_progress`，实现，验证，审查，修复审查发现，提交，记录证据，再标记 `done`。
- 不要带着上一步已完成但未提交的变更开始下一个依赖步骤。
- 改变 scope、顺序、验收标准、public contracts、data models、verification strategy 或 Secure discovery/redaction 边界前，必须先更新本计划。

## 10. 审查 策略

- 每步 审查：代码完成且提交前，检查行为、契约、测试、安全/隐私、文档漂移。
- 集成 审查：步骤 10 检查跨步骤 schema/runtime 兼容和 migration 行为。
- 契约/安全 审查：owner scope、identity registry、E2EE outbox、direct sessions、group MLS/device state 都必须 审查。
- Secure surface 审查：任何触碰 secure code、diagnostics、docs、Dart DTO、CLI output、DID/service discovery、feature flags、backups 或 workspace upgrade logs 的步骤，提交前都必须 审查。
- 文档 审查：优先更新子仓库文档；Harness 文档只有在路由或跨仓库摘要变化时才更新。

## 11. 验证策略

| 层级 | 命令 / 检查 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked local_state` 和每步 focused filters | 新增和既有 storage tests 通过。 |
| 工作区 | `cargo test --workspace --locked` | Rust workspace 全量通过。 |
| Cutover | `bash scripts/sdk-refactor/final-cutover-check.sh` | CLI/SDK boundary checks 通过。 |
| Dart/Flutter | `scripts/flutter/codegen-check.sh` | Rust/Dart generated bindings 是最新的。 |
| 文档 | 检查路径、链接和中文计划文档要求；如果改 Harness，则执行 Harness docs validation | 文档和链接存在，计划类文档使用中文。 |
| 局部系统测试 | 中间步骤可按需要运行 focused system tests | 可作为辅助证据，但不替代最终完整系统测试。 |
| 最终完整系统测试 | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws uv run awiki-system-test` | 最后一步必须完整执行并通过；使用 remote 模式和 `awiki.info` 域名；报告通过、失败、跳过、耗时和配置上下文。 |
| Security | 步骤文档中的 focused secure direct/group E2EE checks | 私有本地状态没有 owner-scope regression。 |
| Secure discovery | 检查 DID/service discovery 生成和 feature defaults；分类 `rg "anp.direct.e2ee.v1|direct-e2ee|anp.group.e2ee.v1|group-e2ee" crates docs config.template.yaml` 命中 | 没有新的默认 public advertisement；docs/internal/test 命中是有意的。 |
| Secure redaction | 搜索 public DTO/CLI diagnostics 的 raw secure fields，并运行步骤文档里的 secure status/outbox tests | public output 中无 private keys、JWTs、plaintext outbox payloads、ratchet/MLS internals、provider stdout/stderr 或 raw secure rows。 |

## 12. 文档更新

- 子仓库文档：
  - `docs/architecture/local-state-owner-scope.md`
  - `docs/architecture/local-state-upgrade.md`
  - `docs/architecture/direct-e2ee-operations.md`：仅当 owner-scope 变化影响 secure direct local state wording 时更新。
  - `docs/architecture/group-e2ee-operations.md`：仅当 owner/device scope 或 secure command posture 变化时更新。
  - `docs/sdk-refactor/modules/10-secure.md`：仅当 secure DTO 或 outbox status wording 变化时更新。
  - `docs/sdk-refactor/modules/04-local-state.md`：如果存在则更新，否则新增 local-state module note。
  - `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`：仅当 public Dart DTO 变化时更新。
- Harness 文档：
  - 只有在路由/摘要变化时，更新 `awiki-harness/context/nodes/storage.node.md` 或 repo profile。
- 生成的任务文档：
  - 执行期间保持本计划和步骤文档更新。
- 语言要求：
  - 所有规划文档、计划文档、执行计划、步骤计划和计划类记录必须用中文撰写；技术标识、代码符号、路径和命令可保留原文。

## 13. 提交计划

- 每个完成、审查、验证后的步骤创建一个聚焦提交。
- 提交前：记录 `git status` 和包含文件。
- 提交后：记录 commit hash 和提交后的工作区状态。
- 步骤 10 只有在集成修复或证据文档变更时提交。
- 不要把所有步骤积累成最后一个大提交。

## 14. 阻塞处理

| 阻塞 | 步骤 | 证据 | 已尝试方法 | 影响范围 | 下一决策 |
|---|---|---|---|---|---|
| 暂无 | | | | | |

- 只有当依赖允许且风险已记录时，才能继续另一个 pending 步骤。
- 只有没有安全假设、回退或独立下一步时，才询问用户。

## 15. 计划变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 审查 |
|---|---|---|---|---|
| 2026-05-30 | 从更新后的 owner-scope 方案生成初版可执行计划。 | 用户要求 审查 并生成详细步骤计划。 | 全部 | 是 |
| 2026-05-30 | 增加 Secure 要求基线并加强每步 secure 门禁。 | 用户要求按 Secure 要求复核计划。 | 全部，尤其 07-10 | 是 |
| 2026-05-30 | 将计划文档改为中文，并要求最终步骤在 `../awiki-system-test` 使用 remote 模式和 `awiki.info` 域名执行完整系统测试。 | 用户要求所有计划文档使用中文，并要求最后一步完整系统测试；随后补充系统测试使用 `awiki.info` 域名和 remote 模式。 | 全部，尤其 10 | 是 |
| 2026-05-31 | 步骤 08 的旧 schema v3->v4 路径收敛为“确认 workspace SQLite backup 后 clean rebuild”，并要求 legacy import 显式未知 owner fail closed。 | 实现审查发现：直接按旧 owner-DID rows 推断 ownership 风险高；旧方案要求不能按 DID、credential、alias 或 path 静默迁移。系统未上线，备份后重建更符合数据安全目标。 | 08 | 是 |

## 16. 风险和回滚

| 风险 | 缓解 | 回滚 / 回退 |
|---|---|---|
| 原子 schema cutover 影响模块多。 | 先构建 inactive v17 scaffold；只有所有活跃 SQL 就绪后再激活。 | 恢复 pre-migration backup；回滚聚焦提交。 |
| legacy rows 无法映射到 identity。 | 隔离或备份后重建；绝不静默分配。 | 备份数据库并创建干净 v17 DB。 |
| public DTO 变化破坏 Dart/App 用户。 | storage 迁移期间保留 `thread_id` alias；在专门步骤 regenerate bindings。 | 回滚 DTO 暴露，同时保留内部 `conversation_id`。 |
| E2EE 私有状态 scope regression。 | 增加 owner identity/device tests 和 security 审查门禁。 | 保持 direct_e2ee 既有 identity-owned tables；隔离 legacy outbox changes。 |
| Secure 公开接口被意外扩大。 | 在步骤 07、09、10 增加 discovery/redaction 门禁；检查 feature defaults 和 public DTO/CLI output。 | 回滚公开接口变化；只保留 storage/internal refactor。 |
| Migration、diagnostics 或 backup logs 泄露敏感本地材料。 | 脱敏 logs/manifests，保留 backup permissions，并测试 diagnostics 输出。 | 回滚泄露输出，必要时轮换本地测试凭据，从受保护 backup 恢复。 |
| Workspace migration 与既有 v3 流程冲突。 | 新增明确 v3 到 v4 migration 和测试；除非共享 helper 必需，不改旧 migration。 | 从 `upgrade/backups` 恢复 workspace。 |
| 最终完整系统测试不可用或失败。 | 在步骤 10 必须记录环境、命令、失败/跳过细节并修复后重跑。 | 不得标记计划完成；保留失败证据并继续修复。 |
