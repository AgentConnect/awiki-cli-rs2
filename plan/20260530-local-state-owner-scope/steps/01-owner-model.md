# 步骤 01：所有者模型和不变量

主计划：[../plan.md](../plan.md)
步骤编号：01
状态：完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-30T16:18:42Z |
| 完成时间 | 2026-05-30T16:30:27Z |
| 提交 | 步骤 01 聚焦提交：`im-core: add local owner scope invariants` |
| 审查证据 | 提交前审查：修复 registry snapshot 在应用 `default_identity` 标志前校验可能误判重复默认身份的问题；复查未修改 schema、SQL conflict target、secure discovery 或 public secure DTO；`credential_name` 仅作为 metadata 进入 outbox 适配，不作为新 owner 回退。 |
| 验证证据 | `cargo fmt --all --check` 通过；`cargo test -p im-core --locked owner_scope` 通过；`cargo test -p im-core --locked identity_registry` 通过；`cargo check -p im-core --locked` 通过；`rg "credential_name.*fallback|anp\\.direct\\.e2ee\\.v1|anp\\.group\\.e2ee\\.v1" crates/im-core/src crates/awiki-cli/src docs` 仅命中既有 internal/profile/docs，未见本步骤新增默认 discovery advertisement。 |
| 下一步 | 步骤 02 开始前读取 `steps/02-schema-rebuild-scaffold.md` 和当前 `git status`。 |

## 2. 目标

- 产出：集中本地 owner scope，包含 `owner_identity_id`、当前 DID snapshot 和可选 `device_id`。
- 用户/系统行为：暂不改变可见行为；后续步骤使用统一的内部 owner-scope 类型。
- 非目标：激活 schema v17、重建表、移除运行时 query 回退、改变 secure discovery 或公开 secure API。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/im-core/src/internal/local_state/` | 新增 `owner_scope.rs` 或等价模块。 | `OwnerScope { owner_identity_id, owner_did, device_id, credential_name? }`；从 `ImClient` 构造；`credential_name` 只能是 metadata，不能成为 owner 回退。 |
| `crates/im-core/src/internal/store/e2ee_outbox.rs` | 准备复用 `OwnerScope` 或转换现有 `E2eeOutboxOwnerScope`。 | 本步骤不改 DB key。 |
| `crates/im-core/src/identity/registry.rs` | 增加 identity id、alias、live DID、handle、default identity 的唯一性校验。 | 重复 live DID 必须 fail closed。 |
| `crates/im-core/src/internal/message_runtime/local_projection.rs` | 增加 stable conversation helper 签名。 | 暂不切换 storage。 |
| 测试 | 增加聚焦不变量测试。 | 本步骤必须通过。 |

## 4. 依赖

- 前置步骤：无。
- 外部文档或决策：更新后的 owner-scope 方案、SDK refactor 文档。
- 环境前提：Rust workspace 能在本地构建。

## 5. 核心设计

创建一个内部 owner-scope API，拒绝空的 `owner_identity_id`。`owner_did` 仍然是当前 DID snapshot 和 wire/display context，但不再被建模为分区键。`device_id` 只在 E2EE/MLS 明确需要设备作用域时使用。

Secure 边界：本步骤只是内部 plumbing。不得新增 public secure status fields、discovery flags、feature defaults 或 diagnostic output。`device_id` 可以为后续 E2EE/MLS scoping 建模，但不得暴露 provider paths、raw MLS identifiers、private key paths 或 session counters。

不要为了机械统一一次性删除所有既有函数参数。可先加入适配器，例如：

```rust
impl OwnerScope {
    pub(crate) fn for_client(client: &crate::core::ImClient) -> crate::ImResult<Self>;
    pub(crate) fn require_identity_id(value: impl Into<String>) -> crate::ImResult<String>;
}
```

Registry validation 应在解析 registry snapshots 之后、返回给调用方之前执行。错误可以使用 `ImError::InvalidInput` 或现有更具体错误，但必须足够定位 duplicate DID/alias。

## 6. 实施指南

1. 新增 `crates/im-core/src/internal/local_state/owner_scope.rs`，并从 `local_state/mod.rs` 导出。
2. 实现 trim、required-field validation 和 `for_client`。
3. 增加 `conversation_id` helper，但暂不改 call sites：
   - `direct_conversation_id(peer_did) -> "dm:<peer_did>"`
   - `group_conversation_id(group_id_or_did) -> "group:<value>"`
   - `mail_conversation_id(source) -> "mail:<value>"`
4. 在 `sdk_registry_snapshot` 和 `legacy_registry_snapshot` 中加入重复项校验。
5. 增加测试覆盖：空 identity id 拒绝、重复 live DID、重复 identity id、重复 alias、重复 handle、default alias 指向缺失 identity。
6. 确认任何 `credential_name` 处理都只是 metadata，不能被新 helper 用作 owner 回退。

## 7. 验收标准

- [x] 存在 `OwnerScope` 或等价中心类型，并拒绝空 `owner_identity_id`。
- [x] Registry parsing 拒绝重复 live DID 和重复 identity id。
- [x] `credential_name` 即使出现在 helper 类型中，也不能作为 owner partition 回退。
- [x] 本步骤不修改 secure discovery flags、public secure DTO 或 CLI secure output。
- [x] 本步骤不改变活跃表 schema 或 SQL conflict target。
- [x] 聚焦测试通过。
- [x] 审查发现 已处理或明确记录。
- [x] 步骤 02 开始前已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked owner_scope` | Owner-scope 测试通过。 |
| 单元 | `cargo test -p im-core --locked identity_registry` | Registry invariant 测试通过。 |
| 编译 | `cargo check -p im-core --locked` | 无新增编译错误。 |
| 搜索 | `rg "credential_name.*fallback|anp\\.direct\\.e2ee\\.v1|anp\\.group\\.e2ee\\.v1" crates/im-core/src crates/awiki-cli/src docs` | 本步骤没有引入新的 owner 回退或 discovery advertisement。 |

## 9. 审查流程

- 检查新代码没有把 `owner_did` 描述成分区键。
- 检查 duplicate DID 行为是 fail closed。
- 检查 owner scope 只是内部不变量 helper，没有扩大 Secure 公开接口。
- 检查没有引入无关 schema 修改。

## 10. 提交要求

- 提交时机：本步骤实现、验证和 审查 后。
- 提交范围：owner model、registry validation、测试。
- 建议提交信息：`im-core: add local owner scope invariants`

## 11. 风险、回滚和后续

- 风险：registry validation 可能暴露无效测试 fixture。
- 回滚/回退：调整 fixture 使 identity id/DID 有效；不要削弱 validation。
- 后续文档：步骤 09 在文档中说明 owner-scope 类型。
