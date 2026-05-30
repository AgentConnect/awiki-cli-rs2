# 步骤 08：Recover/Replace 和 Workspace Upgrade

主计划：[../plan.md](../plan.md)  
步骤编号：08  
状态：草案

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | |
| 完成时间 | |
| 提交 | |
| 审查证据 | |
| 验证证据 | |
| 下一步 | 用 DID history 和 v3 到 v4 workspace migration 替换 DID rebind 语义。 |

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
   - v16 DB 且 ownership 可解析：rebuild 到 v17；
   - unresolved rows 且允许 destructive rebuild：backup 后创建 clean v17 并给出 warning；
   - 其他情况：返回明确 diagnostic。
4. 替换 `migration_v2_to_v3` 中 replace DID 后调用 `rebind_local_identity_state` 的成功路径，或确保 v3 到 v4 立即修复。
5. 更新 recover/replace finalizers，写入 DID history，避免 business owner rebind。
6. 增加 explicit secure-state decision points：
   - 同一 owner identity 且 key material 兼容：保留 scoped secure state；
   - key material 变化/不兼容：通过既有 secure workflows 标记相关 secure state 需要 repair 或 cleanup；
   - ownership unresolved：fail closed 或 backup 后 quarantine/rebuild。
7. 脱敏 backup manifests、upgrade warnings 和 diagnostics，只输出 counts/table names/status，不输出 private contents。
8. 增加测试，验证 old/new DID rows 按 natural key 合并为一个 identity-owned row。

## 7. 验收标准

- [ ] Workspace latest version 通过明确 migration bump。
- [ ] 旧 `UPDATE OR IGNORE` rebind 不再作为 production success path。
- [ ] Migration 在 rebuild/reset 前创建 SQLite backup。
- [ ] Backup manifests/logs/warnings 不包含 private PEMs、JWTs、message plaintext、secure outbox plaintext、raw E2EE artifacts、provider stdout/stderr 或 MLS private state。
- [ ] DID history 正确记录 current 和 previous DID。
- [ ] Recover/replace 不会重复 messages、contacts、groups、members、events 或 conversations。
- [ ] Recover/replace 不会按 DID、alias、credential name 或 path 跨 identity merge secure state。
- [ ] Secure state 只通过明确 key-material compatibility decision 被 preserve、repair、rotate 或 quarantine。
- [ ] Unresolved ownership 不会静默迁移。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p awiki-cli --locked workspace_upgrade` | Upgrade tests 通过。 |
| 单元 | `cargo test -p im-core --locked recover` | Recover local-state tests 通过。 |
| 单元 | `cargo test -p im-core --locked replace_did` | Replace DID tests 通过。 |
| Contract | `cargo test -p awiki-cli --locked identity_replace_did_upgrade_contract` | Contract 通过，或记录更新后的等价测试名。 |
| Redaction search | `rg "private_key|jwt_token|plaintext|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|backup_manifest" crates/awiki-cli/src/workspace_upgrade crates/im-core/src/internal/identity_recover_local_state crates/im-core/src/internal/identity_replace_did_execution.rs` | 所有 manifest/log/diagnostic output 已 审查 并脱敏。 |

## 9. 审查流程

- Data safety 审查：破坏性操作前必须 backup。
- Correctness 审查：migration 后 v17 active tables 中没有 old owner DID rows 作为所有权来源。
- Security 审查：secure material preserve/cleanup/rotation 只在 key-material compatibility 支持时发生；backup manifests/logs 已脱敏。

## 10. 提交要求

- 建议提交信息：`awiki-cli: migrate local state to owner identity schema`

## 11. 风险、回滚和后续

- 风险：legacy import 和 v3 K1 replacement 可能与 v4 migration 交互。
- 回滚/回退：从 `upgrade/backups/<id>/awiki-cli.db.bak` 恢复；如果发现 diagnostic 泄露，移除泄露输出，必要时轮换受影响本地测试凭据，再重新验证。
- 后续文档：更新 local-state upgrade docs，说明 backup sensitivity 和 redaction behavior。
