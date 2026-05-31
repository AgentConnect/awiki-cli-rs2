# local_state 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P1 只做路径校验/最小 bootstrap；P3 补全本地 projection、cache merge、owner isolation；当前重构已落到 identity-owned schema。
**职责**：本地 SQLite 状态、领域模型、缓存合并和迁移。

## 1. 目标

`local_state` 是本地状态的领域 contract。`im-core` 在调用方传入的 SQLite 文件路径上读写消息、群组、联系人、relationship events、secure outbox 和 direct secure state。SQLite 作为当前底层实现依赖保留在 `im-core` 中，但 App / CLI 不能直接依赖裸表结构作为产品 API。

## 2. P1 职责

- `LocalStatePaths` 路径 DTO。
- `core.bootstrap().validate_paths()`。
- `core.bootstrap().initialize_local_state()` 的最小实现。
- `core.bootstrap().migrate_local_state()` 的最小实现。
- P1 消息发送/读取可复用旧存储实现或最小写入，不强制完整 projection。

## 3. P3 / 当前职责

- message record、thread record、group record、member record、contact record、outbox record 的领域模型。
- schema 初始化/迁移。
- inbox/history 远端结果与本地 cache 合并。
- conversation projection。
- mark-read 本地/远端同步。
- owner isolation 自动注入，运行时以 `owner_identity_id` 为唯一 owner partition key。
- `owner_did` 只作为当前 DID snapshot，不能作为活跃主键或 owner fallback。
- `conversation_id` 是稳定会话键；`thread_id` 在过渡期保持等同 alias。
- 迁移前后数据一致性规则和 owner invariant 检查。
- DID recover/replace 只写 `identity_did_history` 并刷新同一 owner identity 的 snapshot，不做业务行 owner rebind。

## 4. 生命周期入口

```rust
pub struct CoreBootstrap<'a> {
    core: &'a ImCore,
}

impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

owner/path 级接口属于内部持久化实现或测试辅助：

```rust
pub(crate) struct LocalOwnerContext {
    pub identity_id: IdentityId,
    pub current_did: Did,
}

pub(crate) struct LocalStateService<'a> {
    core: &'a ImCore,
}

impl LocalStateService<'_> {
    pub(crate) fn open(&self, paths: &LocalStatePaths) -> ImResult<LocalStateConnection>;

    pub(crate) fn store_messages(
        &self,
        owner: LocalOwnerContext,
        messages: Vec<MessageRecord>,
        paths: &LocalStatePaths,
    ) -> ImResult<()>;
}
```

## 5. 不负责

- 发现哪个 SQLite 文件。
- 创建工作区目录。
- chmod。
- CLI debug SQL。
- legacy 路径自动发现。

CLI 负责决定何时调用 path-based 初始化/迁移函数，并把 SQLite 文件路径传入 `im-core`。

## 6. Public API 约束

`local_state` 不作为 App 面向的主 SDK 模块暴露。App 和 CLI 应通过 `client.messages()`、`client.groups()`、`client.directory()`、`client.secure()` 等高层接口读写本地状态。

Debug SQL 属于 CLI `debug.db.*`，不属于 SDK default API。

默认 public DTO 不暴露 raw SQLite rows、backup contents、message plaintext、secure outbox plaintext、private keys、JWT、raw ciphertext、direct ratchet counters、MLS private state、provider stdout/stderr/path 或低层 group E2EE operation payload。

## 7. Owner Scope 当前契约

权威设计见 [本地状态 owner scope 设计](../../architecture/local-state-owner-scope.md)。

当前 SQLite schema version 为 `17`。活跃表主键按 `owner_identity_id` 分区：

- `messages(owner_identity_id, msg_id)`
- `contacts(owner_identity_id, did)`
- `contact_handle_bindings(owner_identity_id, handle, did)`
- `groups(owner_identity_id, group_id)`
- `group_members(owner_identity_id, group_id, user_id)`
- `relationship_events(owner_identity_id, event_id)`
- `e2ee_outbox(owner_identity_id, outbox_id)`
- direct E2EE tables 使用 `owner_identity_id` scoped keys。

Legacy owner-DID / credential-name 解析只允许在 migration/import 兼容路径中存在，并且必须 fail closed，不能在运行时静默 fallback 到默认 owner。
