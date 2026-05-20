# 模块设计：local-state

## 1. 职责

`local_state` 是 SDK 内部持久化能力，不是 App/CLI 默认 public API。第一阶段继续使用内置 SQLite 实现。

负责：

- schema 初始化/迁移。
- message/group/contact/conversation/outbox 基础表。
- owner 隔离。
- 远端结果与本地 cache 合并。
- conversation/thread projection。

## 2. 对外入口

只通过 bootstrap 暴露生命周期：

```rust
impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

业务读写通过：

```rust
client.messages()
client.groups()
client.directory()
```

## 3. owner 隔离

建议引入：

```rust
pub(crate) struct LocalOwnerContext {
    pub owner_identity_id: IdentityId,
    pub current_did: Did,
    pub local_alias: Option<String>,
}
```

第一阶段兼容已有 `owner_did`，但新逻辑优先使用 `owner_identity_id`，避免 replace DID/recover 后数据需要全量 rebind。

## 4. 不暴露内容

```rust
SQLite connection
store_message(owner, record)
store_messages_batch
query_inbox(owner, query)
execute_sql
ContactRecord / MessageRecord / GroupRecord as DB rows
```

`debug.db.*` 是 CLI-only 能力。

## 5. 第一阶段验收

- local state 初始化/迁移可由 `core.bootstrap()` 完成。
- `messages().conversations()` 可用。
- alice/bob 两个身份共享 SQLite 时互不串数据。
- CLI 普通业务 handler 不直接调用 `store::*`。
