# 步骤 05：联系人和关系事件

主计划：[../plan.md](../plan.md)  
步骤编号：05  
状态：审查中

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | review |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-30T18:56:15Z |
| 完成时间 | |
| 提交 | |
| 审查证据 | 提交前审查完成：联系人保存/目录解析/关系投影都从 `OwnerScope` 派生 owner 信息；contact update、handle binding upsert 和旧 handle 清理都有 affected-row checks；新增测试覆盖 DID snapshot 变化后一行更新、handle current uniqueness 按 owner identity scoped、相同 relationship `event_id` 可跨 owner 存储；未发现 Secure discovery 或 public secure DTO 变化；secret 搜索仅命中测试 fixture 假 token。 |
| 验证证据 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked contact` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked relationship` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked --test phase2_identity_directory` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked --test phase2_relationship_directory` 通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；legacy SQL 搜索无命中；secure secret 搜索仅命中测试 fixture 中的 `"jwt_token":"token"`。 |
| 下一步 | 创建步骤 05 聚焦提交。 |

## 2. 目标

- 产出：contacts、handle bindings、relationship events 使用 identity-owned keys，并具备确定性 merge 和 affected-row checks。
- 用户/系统行为：DID 变化后不会产生重复联系人或跨 owner event overwrite。
- 非目标：改变 `user-service` relationship truth、DID auth 行为、secure discovery 或 secure public DTOs。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `contact_store/records.rs` | 将 active upsert/read/update 改为 strict `owner_identity_id`。 | 移除 owner 回退。 |
| `contact_store/projection.rs` | 使用 `OwnerScope`。 | async path 应通过 actor。 |
| `directory/service.rs` | 确保所有 local store calls 从 `ImClient` 派生 scope。 | CLI/App 不传散落 owner strings。 |
| 测试 | contact duplicate、handle current、event id per owner。 | 包含 old/new DID migration fixture。 |

## 4. 依赖

- 前置步骤：步骤 03。

## 5. 核心设计

Contacts 按 `(owner_identity_id, did)` 建键。Handle bindings 按 `(owner_identity_id, handle, did)` 建键，并保证每个 `(owner_identity_id, handle)` 只有一个 current binding。Relationship events 按 `(owner_identity_id, event_id)` 建键，因此不同 identities 下同一个 external event id 是合法的。

所有期望命中 row 的手写 `UPDATE` 都必须检查 affected rows，或者明确回退到 insert/upsert。旧的 zero-row update 路径必须消失。

Relationship/contact diagnostics 可以在必要时包含 owner identity id、peer DID/handle、status 和 timestamps，但不得包含 JWT、private key material、secure outbox plaintext、raw E2EE payloads 或 MLS/provider internals。

## 6. 实施指南

1. 将 contact owner predicates 替换为 `owner_identity_id = ?`。
2. 将 contact upsert conflict target 替换为 `(owner_identity_id, did)`。
3. 将 handle binding conflict target 替换为 `(owner_identity_id, handle, did)`。
4. 将 relationship event conflict target 替换为 `(owner_identity_id, event_id)`。
5. 在 `ContactRecord`、`ContactHandleBindingRecord`、`RelationshipEventRecord` runtime writes 中要求 `owner_identity_id`。
6. 对期望命中的 updates 增加 affected-row checks。
7. 更新 directory 和 relationship runtime call sites，传入 `OwnerScope`。
8. 审查 新增 diagnostics/errors，确保 secret 和 secure-payload redaction。

## 7. 验收标准

- [ ] 同一 identity 下同一 contact DID 在 DID 变化前后 upsert 为一行。
- [ ] 不同 identities 下相同 relationship `event_id` 可以存两行。
- [ ] active SQL 不可能跨 owner overwrite relationship event。
- [ ] Handle current uniqueness 按 owner identity 约束。
- [ ] 预期命中的 0-row updates 会失败或明确 insert。
- [ ] Contact/relationship errors 和 diagnostics 不打印 private 或 secure payload material。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked contact` | Contact tests 通过。 |
| 单元 | `cargo test -p im-core --locked relationship` | Relationship tests 通过。 |
| 搜索 | `rg "relationship_events.*event_id TEXT PRIMARY KEY|ON CONFLICT\\(event_id\\)|WHERE owner_did" crates/im-core/src/internal/contact_store` | 无 active legacy patterns。 |
| 搜索 | `rg "jwt|private_key|plaintext|ciphertext|KeyPackage|Welcome|Commit|Proposal" crates/im-core/src/internal/contact_store crates/im-core/src/directory` | 命中必须是非输出常量/测试，或被移除。 |

## 9. 审查流程

- 检查 merge rules 保留 user note/relationship，且不会用空 projection 覆盖非空字段。
- 检查 relationship event status/timestamps 遵守确定性 merge rules。
- 检查 relationship/contact 路径不会成为 secure payload 或 secret 泄露通道。

## 10. 提交要求

- 建议提交信息：`im-core: key contacts and relationship events by owner identity`

## 11. 风险、回滚和后续

- 风险：旧测试可能依赖全局 event id uniqueness。
- 回滚/回退：更新 fixtures 以包含 owner identity；不要恢复全局 event id key。
