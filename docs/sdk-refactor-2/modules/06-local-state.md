# 06-local-state：本地状态与 owner 隔离

## 1. 目标

Phase 1 只要求本地状态支持 SDK 启动和消息主链路的最小需要。完整 message/contact/conversation projection、cache merge、mark-read 状态和 group cache 后移到 Phase 3。

## 2. Phase 1 public API

只通过 `core.bootstrap()` 暴露生命周期入口：

```rust
impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

业务 API 不直接传 `LocalStatePaths`。

## 3. owner 隔离

本地状态应从设计上支持多身份隔离：

```rust
pub(crate) struct LocalOwnerContext {
    pub identity_id: IdentityId,
    pub current_did: Did,
}
```

建议长期优先使用 `owner_identity_id` 作为稳定主键，`owner_did` 作为兼容和展示字段。这样可以降低 replace DID、recover/rebind 之后的本地状态迁移成本。

## 4. internal only

不暴露：

```rust
open_sqlite_connection()
store_message(owner, paths, record)
query_inbox(owner, paths, query)
mark_read(owner, ids, paths)
execute_sql()
```

`debug.db.*` 留在 CLI。

## 5. Phase 3 补全

Phase 3 再实现：

```text
message projection
contact projection
conversation/thread projection
mark-read local state
cache merge
alice/bob owner isolation tests
group cache
```

## 6. 完成判定

P1 完成时：

- SDK 可以用显式 SQLite/local state path 启动。
- messages 主链路可以选择性写入最小本地状态。
- CLI 业务 handler 不直接拼 owner/path/store helper。

P3 完成时，App/CLI 可复用完整 conversation/thread projection。
