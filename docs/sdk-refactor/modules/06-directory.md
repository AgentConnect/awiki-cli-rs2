# directory 模块接口设计

**阅读顺序**：06 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：联系人、关系、handle 缓存和公开资料读取。

## 1. 目标

`directory` 为消息、群组、身份解析和联系人展示提供统一的 DID/handle/profile 查询与本地缓存投影。

## 2. 主要职责

- handle 补全与规范化。
- DID/handle lookup。
- 联系人 upsert、查询、来源标注。
- profile 读取结果到联系人模型的投影。
- 关系状态查询和关系事件记录。

## 3. Phase A 状态需求

- 可通过 `LocalStatePaths.database_file` 使用本地 SQLite contact cache。
- 远端 lookup 使用 `ImCoreConfig` 中的服务端点。
- CLI 负责传入 owner DID 和数据库路径，不让 `im-core` 扫描当前 workspace。

## 4. 接口草案

```rust
pub struct DirectoryService<'a> {
    core: &'a ImCore,
}

impl DirectoryService<'_> {
    pub async fn resolve_peer(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<PeerProfile>;

    pub async fn lookup_handle(
        &self,
        handle: Handle,
    ) -> ImResult<ResolvedIdentity>;

    pub fn upsert_contact(
        &self,
        owner: Did,
        contact: ContactRecord,
        local_state: &LocalStatePaths,
    ) -> ImResult<()>;

    pub fn list_contacts(
        &self,
        owner: Did,
        query: ContactQuery,
        local_state: &LocalStatePaths,
    ) -> ImResult<ContactPage>;

    pub async fn relation_status(
        &self,
        actor: ActorContext,
        peer: PeerRef,
    ) -> ImResult<RelationStatus>;
}
```

## 5. CLI 边界

CLI 负责：

- `id resolve` 等命令参数解析；
- 输出格式；
- 本地数据库路径传入；
- 是否展示缓存命中、刷新策略等 CLI UX。

`directory` 不负责读取 CLI config 或选择 SQLite 文件位置。
