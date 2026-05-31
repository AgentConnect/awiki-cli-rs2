# 步骤 08：Recover/Replace 和 Workspace Upgrade

主计划：[../plan.md](../plan.md)  
步骤编号：08  
状态：完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-30T20:22:58Z |
| 完成时间 | 2026-05-31T00:40:33Z |
| 提交 | `20f21b6`：`awiki-cli: migrate local state to owner identity schema` |
| 审查证据 | 提交前审查：recover/replace 已改为 DID history 和 owner_did snapshot refresh，不做业务 owner rebind；`LegacyOwnerLookup` 生产调用已改为使用 `IdentitySummary.unique_id`；审查发现并修复旧 schema v3->v4 未执行 clean rebuild 的问题，改为确认 workspace SQLite backup 后删除旧 DB 文件集并创建干净 v17；审查发现并修复 legacy import 显式未知 `owner_did`/`credential_name` 会落到 default owner 的问题，改为 fail closed 并补测试；未新增 Secure discovery/default advertisement 或 public raw secure output；提交后状态为分支 ahead 13，工作区干净。 |
| 验证证据 | `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked workspace_upgrade` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked import_legacy_database` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked recover` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked replace_did` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked --test identity_replace_did_upgrade_contract` 通过；`CARGO_BUILD_JOBS=1 cargo check -p awiki-cli --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；redaction/discovery/legacy rebind 搜索命中已分类。 |
| 下一步 | 步骤 09 开始前读取 `../plan.md`、[09-docs-diagnostics-dart.md](09-docs-diagnostics-dart.md) 和当前 `git status`。 |

## 2. 目标

- 产出：DID recover/replace 和 workspace upgrade 不再通过 owner DID 移动业务所有权。
- 用户/系统行为：DID 变化只更新 registry/history/snapshots；业务行继续留在同一 owner identity 下。
- 非目标：改变远端 DID recovery protocol、改变 secure cryptographic protocol semantics、暴露 backup/secure internals。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `identity_recover_local_state.rs` | 将 merge/rebind 语义转换为 identity-owned migration，或退役旧 owner-DID movement。 | 既有 merge code 可变为 migration-only。 |
| `identity_recovery_runtime.rs` | finalize 时写入 `identity_did_history`。 | sync 和 async paths。 |
| `identity_replace_did_execution.rs` | 用 DID history/current snapshot update 替换 business-row rebind。 | 只有 key material 变化时才处理 secure state。 |
| `workspace_upgrade/*` | 新增 `migration_v3_to_v4`；bump latest workspace schema version。 | 使用现有 lock/backup/journal。 |
| `legacy_sqlite/*` | 替换 `UPDATE OR IGNORE` rebind，或标记为 migration-only 且 production runtime 不使用。 | 既有测试需要反向调整。 |
| Backup/manifest/logging paths | 保留权限并脱敏敏感字段。 | backups 可能包含 private material；manifests/logs 不得包含。 |
| 测试 | v16 fixture migration、old/new DID mixed rows、unresolved owner rows、backup redaction。 | 包含 backup/rebuild 证据。 |

## 4. 依赖

- 前置步骤：步骤 01-07。

## 5. 核心设计

identity-owned schema 之后，DID replacement 不是 owner transfer。它应执行：

1. 更新 identity registry/store；
2. 在 `identity_did_history` 中将旧 DID 标记为 `previous`；
3. 插入/更新新 DID 为 `current`；
4. 对同一 `owner_identity_id` 下可用的冗余 `owner_did` snapshot 字段做更新；
5. 只有 cryptographic key material 变化时才 preserve 或 rotate secure material。

Workspace upgrade v3 到 v4 负责旧数据库 migration 或 clean rebuild。它必须先创建 backup，再执行 invariant checks。

Secure data-safety 规则：

- DID change 本身不是跨 identities merge secure state。Direct sessions、OPKs、signed prekeys、secure outbox rows 和 group MLS state 可以留在同一 `owner_identity_id` 下，但不得复制到另一个 identity，也不得只按 DID 匹配。
- 如果 cryptographic key material 变化，secure state rotation/cleanup 必须遵循既有 direct/group E2EE 权威文档，并明确 审查；不得静默复用不兼容 sessions。
- Workspace backups 是敏感的。Backup files 可能包含 private PEMs、JWTs、plaintext local message views、secure outbox plaintext 和 MLS/provider state。正常 logs、warnings、doctor output、backup manifests 不得打印这些内容。
- Backup directories/files 在平台支持时应保留或加强 local-only permissions。
- Unresolved legacy secure rows 必须 fail closed，或在 backup 后 quarantine/rebuild；不得通过 DID、alias、credential name 或 path heuristics 分配。

## 6. 实施指南

1. 新增 `migration_v3_to_v4.rs` 并接入 `new_default_upgrader`；bump `LATEST_WORKSPACE_SCHEMA_VERSION`。
2. 复用现有 backup lock/journal infrastructure。
3. 实现本地 DB 路径：
   - 无 DB：创建 clean v17 schema；
   - v17 DB：记录 identity DID history，刷新同一 `owner_identity_id` 下的 `owner_did` snapshots，并校验 identity-owned invariants；
   - 旧 schema：必须先确认 workspace SQLite backup 已存在，再删除旧 DB 文件集并创建 clean v17 schema；旧业务行不按 DID、credential name、alias 或 path 静默迁移，meta warning 只记录 schema/version/status，不输出旧行内容；
   - 新于当前支持版本的 schema：fail closed，返回明确 diagnostic，不删除数据库。
4. 替换 `migration_v2_to_v3` 中 replace DID 后调用 `rebind_local_identity_state` 的成功路径，或确保 v3 到 v4 立即修复。
5. 更新 recover/replace finalizers，写入 DID history，避免 business owner rebind。
6. 增加 explicit secure-state decision points：
   - 同一 owner identity 且 key material 兼容：保留 scoped secure state；
   - key material 变化/不兼容：通过既有 secure workflows 标记相关 secure state 需要 repair 或 cleanup；
   - ownership unresolved：fail closed 或 backup 后 quarantine/rebuild。
7. 脱敏 backup manifests、upgrade warnings 和 diagnostics，只输出 counts/table names/status，不输出 private contents。
8. 增加测试，验证 DID history/snapshot refresh、旧 schema backup 后 clean rebuild、未知 legacy owner fail closed，以及 recover/replace 不再移动业务所有权。

## 7. 验收标准

- [x] Workspace latest version 通过明确 migration bump。
- [x] 旧 `UPDATE OR IGNORE` rebind 不再作为 production success path。
- [x] Migration 在 rebuild/reset 前创建 SQLite backup。
- [x] Backup manifests/logs/warnings 不包含 private PEMs、JWTs、message plaintext、secure outbox plaintext、raw E2EE artifacts、provider stdout/stderr 或 MLS private state。
- [x] DID history 正确记录 current 和 previous DID。
- [x] Recover/replace 不会重复 messages、contacts、groups、members、events 或 conversations。
- [x] Recover/replace 不会按 DID、alias、credential name 或 path 跨 identity merge secure state。
- [x] Secure state 只通过明确 key-material compatibility decision 被 preserve、repair、rotate 或 quarantine。
- [x] Unresolved ownership 不会静默迁移。
- [x] 审查发现 已处理或明确记录。
- [x] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p awiki-cli --locked workspace_upgrade` | Upgrade tests 通过。 |
| 单元 | `cargo test -p im-core --locked recover` | Recover local-state tests 通过。 |
| 单元 | `cargo test -p im-core --locked replace_did` | Replace DID tests 通过。 |
| Contract | `cargo test -p awiki-cli --locked identity_replace_did_upgrade_contract` | Contract 通过，或记录更新后的等价测试名。 |
| Redaction search | `rg "private_key|jwt_token|plaintext|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|backup_manifest" crates/awiki-cli/src/workspace_upgrade crates/im-core/src/internal/identity_recover_local_state crates/im-core/src/internal/identity_replace_did_execution.rs` | 所有 manifest/log/diagnostic output 已 审查 并脱敏。 |

已执行验证：

- `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked workspace_upgrade` 通过：lib workspace upgrade 40 个匹配测试通过，`workspace_upgrade_contract.rs` 19 个匹配测试通过，`workspace_upgrade_if_needed_contract.rs` 15 个匹配测试通过。
- `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked import_legacy_database` 通过：5 个匹配测试通过，包含 `import_legacy_database_uses_identity_unique_id_as_owner_identity_id` 和 `import_legacy_database_rejects_unknown_explicit_owner_did`。
- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked recover` 通过：16 个匹配测试通过。
- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked replace_did` 通过：5 个匹配测试通过。
- `CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked --test identity_replace_did_upgrade_contract` 通过：1 个测试通过。
- `CARGO_BUILD_JOBS=1 cargo check -p awiki-cli --locked` 通过。
- `cargo fmt --all --check` 通过；`git diff --check` 通过。
- Redaction 搜索命中分类：workspace legacy identity 内部迁移读取/写入 `private_key`/`jwt_token`；legacy SQLite 内部 import/outbox storage 字段和测试 fixture `plaintext`；replace-did 内部 `backup_manifest` result 字段引用；未发现 provider stdout/stderr、KeyPackage、Welcome、Commit、Proposal 的 public output 命中。
- Secure discovery 搜索命中分类：既有 docs、feature flags、deprecated alias、internal/test secure direct/group E2EE code；本步骤未新增默认 DID/service public advertisement。
- Legacy rebind 搜索命中分类：production `migration_v2_to_v3` 不再调用旧 `rebind_local_identity_state`；`legacy_sqlite::rebind` 仅保留兼容 no-op，生产 re-export 已移除；剩余调用在测试或 compat trait 命名中。

## 9. 审查流程

- Data safety 审查：破坏性操作前必须 backup。
- Correctness 审查：migration 后 v17 active tables 中没有 old owner DID rows 作为所有权来源。
- Security 审查：secure material preserve/cleanup/rotation 只在 key-material compatibility 支持时发生；backup manifests/logs 已脱敏。

提交前审查结论：

- 已确认 `identity_recover_local_state` 不再 merge/delete 业务行，也不清理 secure rows；只记录旧 DID 为 `previous`、当前 DID 为 `current`，并刷新同一 `owner_identity_id` 的 snapshot。
- 已确认 replace DID execution bridge 携带 `owner_identity_id`，local failure 阶段文案改为 `local DID history update`，dry-run local writes 从 owner rebind/cleanup 改为 `sqlite.identity_did_history` 和 `sqlite.owner_did_snapshot_refresh`。
- 已确认 workspace schema latest 从 3 bump 到 4，并新增明确 `workspace_3_to_4_owner_identity_local_state` migration；v17 DB 记录 DID history 和刷新 snapshot；旧 schema 在已有 backup 后 clean rebuild；新于支持版本 fail closed。
- 已确认 legacy import 不导入旧 `e2ee_sessions`，活跃表使用 identity-owned conflict keys；生产 owner lookup 使用 identity `unique_id`，未知显式 owner DID/credential name fail closed，不落到 default owner。
- 已确认旧 `UPDATE OR IGNORE` owner rebind 不再是生产 success path，兼容 rebind helper 已退役为 zero-count no-op。
- 已确认 Secure 公开发现和 public secure DTO/diagnostics 没有因为本步骤扩大；Group MLS provider scoped state/path 仍由步骤 07 的 owner identity + device gate 约束，本步骤未改动该路径。
- 剩余风险：旧 schema clean rebuild 会丢弃本地业务缓存，但会在 workspace backup 后执行，并符合系统未上线阶段“不能可靠分配所有权时备份后重建”的数据安全策略；真实业务数据迁移不是本步骤目标。

## 10. 提交要求

- 建议提交信息：`awiki-cli: migrate local state to owner identity schema`

## 11. 风险、回滚和后续

- 风险：legacy import 和 v3 K1 replacement 可能与 v4 migration 交互；旧 schema clean rebuild 会丢弃本地业务缓存。
- 回滚/回退：从 `upgrade/backups/<id>/awiki-cli.db.bak` 恢复；如果发现 diagnostic 泄露，移除泄露输出，必要时轮换受影响本地测试凭据，再重新验证。
- 后续文档：更新 local-state upgrade docs，说明 backup sensitivity 和 redaction behavior。
