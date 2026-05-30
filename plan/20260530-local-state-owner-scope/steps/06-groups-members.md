# 步骤 06：群组和成员

主计划：[../plan.md](../plan.md)  
步骤编号：06  
状态：完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-30T19:13:21Z |
| 完成时间 | 2026-05-30T19:34:37Z |
| 提交 | 实现提交 `f98fbec`：`im-core: key groups by owner identity` |
| 审查证据 | 提交前审查完成：group snapshot、summary、members、messages 和 left projection 统一从 `OwnerScope::for_client` 派生 owner；`groups` upsert 使用 `(owner_identity_id, group_id)`；`group_members` replacement 和 left cleanup 只按 `owner_identity_id + group_id` 删除；无状态 stale projection 不会把 `left` group 重新激活；cached group mark-read fixture 使用 `owner_identity_id` 和稳定 group `conversation_id`；未新增 Secure discovery、public group E2EE command surface、raw MLS artifact、provider stdout/stderr/path 输出。提交后状态：分支 ahead 9，工作区干净。 |
| 验证证据 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked group` 通过，33 个匹配测试通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --features group-e2ee --locked group_e2ee` 通过，60 个匹配测试通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；`rg "DELETE FROM group_members WHERE owner_did|ON CONFLICT\\(owner_did, group_id" crates/im-core/src/internal/local_state/groups.rs` 无命中；group E2EE 搜索命中分类为既有 docs、internal service/test code 和 hidden CLI command catalog，没有本步骤新增 public output 或默认 discovery。 |
| 下一步 | 步骤 07 开始前读取步骤 07 文档和 `git status`。 |

## 2. 目标

- 产出：groups 和 members 按 `owner_identity_id` 建键和修改。
- 用户/系统行为：同一 identity 在 old/new DID 下只看到一个 group snapshot 和一套 members。
- 非目标：改变 message-service group APIs、group DID semantics、group secure protocol semantics、public discovery 或 low-level group E2EE command surface。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `local_state/groups.rs` | 更新 upsert、get、list、replace members、mark left。 | delete/replace 按 owner identity。 |
| `group_runtime/projection.rs` | 从 client 传递 scope。 | async path 通过 actor。 |
| `groups/service.rs` | 确保 local projection 使用 owner identity，需要时使用 device id。 | 不暴露 raw SQL。 |
| 测试 | duplicate group/member under old/new DID、delete isolation、cached messages。 | 包含 group E2EE summary metadata。 |

## 4. 依赖

- 前置步骤：步骤 03。

## 5. 核心设计

Group snapshots 按 `(owner_identity_id, group_id)` 建键。Group members 按 `(owner_identity_id, group_id, user_id)` 建键。`owner_did` 只保留为当前本地 identity DID snapshot。Group DID 和 group owner DID 保持各自领域语义，不是本地 owner 分区键。

`replace_group_members` 只能删除：

```sql
DELETE FROM group_members
WHERE owner_identity_id = ? AND group_id = ?
```

Group E2EE metadata 在业务表中必须保持高层、脱敏。不得通过 group snapshots、member records、CLI output 或 Dart DTO 存储或暴露 raw KeyPackages、Welcome/Commit/Proposal payloads、raw MLS notices、provider stdout/stderr、MLS private state 或 provider paths。低层 `group e2ee *` 操作继续 hidden/internal 或 stable unsupported。

## 6. 实施指南

1. 更新 `GroupRecord` 和 `GroupMemberRecord` runtime validation，要求 owner identity。
2. 将 group upsert conflict target 替换为 `(owner_identity_id, group_id)`。
3. 将 member delete/insert 逻辑替换为 owner-identity conditions。
4. 将 get/list active group refs 和 cached members queries 替换为 owner-identity predicates。
5. 更新 actor command signatures，让 `ReplaceGroupMembers` 和 `MarkGroupLeft` 接收 `OwnerScope` 或明确 owner identity。
6. 增加测试，验证迁移后 old owner DID rows 不会生成重复 groups。
7. 审查 group secure summary fields 和 CLI/Dart mappings，确保脱敏和 hidden-command posture。

## 7. 验收标准

- [ ] Group upsert 使用 `(owner_identity_id, group_id)`。
- [ ] Group member replacement 不会影响另一个 identity 下相同 `owner_did` 或 `group_id` 的数据。
- [ ] 同一 identity 的 old/new DID group data 合并为一个 group。
- [ ] Group E2EE summary metadata 在 upsert 后保留。
- [ ] Group rows/DTOs 不暴露 raw MLS artifacts、provider stdout/stderr 或 provider paths。
- [ ] 本步骤不会将 hidden/internal group E2EE commands 公开。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked group` | Group local-state/runtime tests 通过。 |
| 单元 | `cargo test -p im-core --features group-e2ee --locked group_e2ee` | 如果受影响，focused group E2EE summary tests 通过。 |
| 搜索 | `rg "DELETE FROM group_members WHERE owner_did|ON CONFLICT\\(owner_did, group_id" crates/im-core/src/internal/local_state/groups.rs` | 无 active legacy patterns。 |
| 搜索 | `rg "KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|provider.*path|group e2ee" crates/im-core/src crates/awiki-cli/src docs/architecture/group-e2ee-operations.md` | 命中已 审查 为 internal/docs-only；没有新的 public output 或 default command surface。 |

## 9. 审查流程

- 检查 group membership status 不会被 stale data 降级。
- 检查 owner DID 没有作为本地 owner 分区参与 group queries。
- 检查 group secure metadata 保持脱敏，不改变 discovery/command exposure。

## 10. 提交要求

- 建议提交信息：`im-core: key groups by owner identity`

## 11. 风险、回滚和后续

- 风险：group tests 可能混用 group_id 和 group_did。
- 回滚/回退：规范测试 fixtures，storage key 保持为 group_id。
