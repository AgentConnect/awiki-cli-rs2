# directory 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P2；P1 只允许 messages 内部做最小 target resolve。  
**职责**：联系人、关系、handle 缓存和公开资料读取。

## 1. 目标

`directory` 为消息、群组、身份解析和联系人展示提供统一的 DID/handle/profile 查询与本地缓存投影。

## 2. P1 边界

P1 不公开 `DirectoryService`。为了让私聊文本能跑通，`client.messages().send(MessageTarget::Direct)` 可以内部支持最小目标解析：

```text
如果 PeerRef 是 DID，直接使用。
如果 PeerRef 是 handle，解析成 DID。
解析失败返回 PeerNotFound。
```

P1 不要求：

- 联系人保存/查询。
- relation status。
- profile projection。
- contact cache merge。
- remote relationship mutation/list。

## 3. P2 职责

- handle 补全与规范化。
- DID/handle lookup。
- 联系人保存、查询、来源标注。
- profile 读取结果到联系人模型的投影。
- 本地 relation status 查询。
- 远端 DID relationship 关注、取消关注、关系状态、粉丝列表、关注列表。
- 关系事件记录和 best-effort contact projection。

## 4. 接口草案

```rust
pub struct DirectoryService<'a> {
    client: &'a ImClient,
}

pub enum IdentitySubject {
    Did(Did),
    Handle(Handle),
    Any(String),
}

impl DirectoryService<'_> {
    pub fn resolve_peer(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<PeerProfile>;

    pub fn lookup_handle(
        &self,
        handle: Handle,
    ) -> ImResult<ResolvedIdentity>;

    pub fn public_profile(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<PublicProfile>;

    pub fn save_contact(
        &self,
        request: SaveContactRequest,
    ) -> ImResult<Contact>;

    pub fn contacts(
        &self,
        query: ContactListQuery,
    ) -> ImResult<Page<Contact>>;

    pub fn relation_status(
        &self,
        peer: PeerRef,
    ) -> ImResult<RelationStatus>;

    pub fn follow(
        &self,
        request: FollowRequest,
    ) -> ImResult<FollowResult>;

    pub fn unfollow(
        &self,
        request: UnfollowRequest,
    ) -> ImResult<UnfollowResult>;

    pub fn relationship_status(
        &self,
        peer: PeerRef,
    ) -> ImResult<RelationshipStatus>;

    pub fn followers(
        &self,
        query: RelationshipListQuery,
    ) -> ImResult<Page<RelationshipListItem>>;

    pub fn following(
        &self,
        query: RelationshipListQuery,
    ) -> ImResult<Page<RelationshipListItem>>;
}
```

`relation_status(peer)` 保持本地通讯录投影语义。`relationship_status(peer)` 走 user-service DID relationship RPC，是远端 authoritative 状态，并只把 DID、handle/profile、关系布尔值、created_at、warnings 暴露到 SDK public DTO；`from_user_id` / `to_user_id` 等服务端内部字段不得进入 public DTO。

`ContactRecord`、`upsert_contact_record(owner, record, paths)` 这类 store 级对象和函数属于内部实现。

## 5. CLI 边界

CLI 负责：

- `id resolve` 等命令参数解析；
- 输出格式；
- 本地数据库路径传入；
- 是否展示缓存命中、刷新策略等 CLI UX。

`directory` 不负责读取 CLI config 或选择 SQLite 文件位置。
