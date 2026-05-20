# directory 模块接口设计

**阅读顺序**：06 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：联系人、关系、handle 缓存和公开资料读取。

## 1. 目标

`directory` 为消息、群组、身份解析和联系人展示提供统一的 DID/handle/profile 查询与本地缓存投影。

## 2. 主要职责

- handle 补全与规范化。
- DID/handle lookup。
- 联系人保存、查询、来源标注。
- profile 读取结果到联系人模型的投影。
- 关系状态查询和关系事件记录。

## 3. Phase A 状态需求

- 可通过 `ImClient` 内部绑定的本地 SQLite contact cache 读写联系人。
- 远端 lookup 使用 `ImCoreConfig` 中的服务端点。
- CLI 负责在构造 `ImCorePaths` 时传入数据库路径，不让 `im-core` 扫描当前 workspace；公开 directory 调用不传 owner DID 或数据库路径。

## 4. 接口草案

```rust
pub struct DirectoryService<'a> {
    client: &'a ImClient,
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

    pub fn save_contact(
        &self,
        request: SaveContactRequest,
    ) -> ImResult<Contact>;

    pub fn contacts(
        &self,
        query: ContactQuery,
    ) -> ImResult<ContactPage>;

    pub async fn relation_status(
        &self,
        peer: PeerRef,
    ) -> ImResult<RelationStatus>;
}
```

`ContactRecord`、`upsert_contact_record(owner, record, paths)` 这类 store 级对象和函数属于内部实现。公开接口使用 `SaveContactRequest`、`Contact`、`ContactPage` 等领域 DTO，避免调用方绕过 owner 注入、缓存合并和关系状态规则。

## 5. CLI 边界

CLI 负责：

- `id resolve` 等命令参数解析；
- 输出格式；
- 本地数据库路径传入；
- 是否展示缓存命中、刷新策略等 CLI UX。

`directory` 不负责读取 CLI config 或选择 SQLite 文件位置。

公开接口挂在 `ImClient` 上，自动注入 owner DID 和 local state。`owner`、`LocalStatePaths` 只允许出现在内部 store helper 中。
