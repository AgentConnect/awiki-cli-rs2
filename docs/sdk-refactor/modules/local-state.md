# local_state 模块接口设计

**所属 crate**：`crates/im-core`  
**模块职责**：本地 SQLite 状态、领域模型、缓存合并和迁移。

## 1. 目标

`local_state` 是本地状态的领域 contract。Phase A 允许 `im-core` 在调用方传入的 SQLite 文件路径上读写消息、群组、联系人和 outbox 状态。SQLite 作为当前底层实现依赖保留在 `im-core` 中，不要求第一阶段或第二阶段替换掉。

## 2. 主要职责

- message record、thread record、group record、member record、contact record、outbox record 的领域模型。
- `LocalStatePaths`：数据库文件、迁移目录、临时目录等路径 DTO。
- 按路径打开 SQLite 连接的最小封装。
- schema 初始化/迁移函数，必须由 CLI 明确调用并传入路径。
- 缓存合并规则。
- 已读状态更新规则。
- owner DID 隔离规则。
- 迁移前后数据一致性规则。

## 3. 接口草案

```rust
pub struct LocalStateService<'a> {
    core: &'a ImCore,
}

impl LocalStateService<'_> {
    pub fn initialize(
        &self,
        paths: &LocalStatePaths,
    ) -> ImResult<LocalStateStatus>;

    pub fn migrate(
        &self,
        paths: &LocalStatePaths,
    ) -> ImResult<MigrationReport>;

    pub fn open(
        &self,
        paths: &LocalStatePaths,
    ) -> ImResult<LocalStateConnection>;

    pub fn store_messages(
        &self,
        owner: Did,
        messages: Vec<MessageRecord>,
        paths: &LocalStatePaths,
    ) -> ImResult<()>;

    pub fn query_inbox(
        &self,
        owner: Did,
        query: InboxQuery,
        paths: &LocalStatePaths,
    ) -> ImResult<MessagePage>;

    pub fn mark_read(
        &self,
        owner: Did,
        ids: Vec<MessageId>,
        paths: &LocalStatePaths,
    ) -> ImResult<()>;
}
```

## 4. 不负责

- 打开哪个 SQLite 文件。
- 创建工作区目录。
- chmod。
- CLI debug SQL。
- legacy 路径自动发现。

CLI 负责决定何时调用 path-based 初始化/迁移函数，并把 SQLite 文件路径传入 `im-core`。

## 5. Phase B 可选演进

如 App 需要完全接管持久化，再提取 `MessageStore` / `GroupStore` / `ContactStore` / `SecureStore` trait。这不是 Phase A 的前置条件，也不是迁移 `im-core` 时必须完成的替换项。
