# 模块设计：directory

## 1. 职责

`directory` 为 profile、联系人、handle/DID 解析和关系状态提供统一入口。它是 App 和 CLI 都需要的基础能力。

## 2. 对外接口

```rust
pub enum IdentitySubject {
    Did(Did),
    Handle(Handle),
    Any(String),
}

impl DirectoryService<'_> {
    pub fn resolve_peer(&self, subject: IdentitySubject) -> ImResult<PeerProfile>;
    pub fn lookup_handle(&self, handle: Handle) -> ImResult<ResolvedIdentity>;
    pub fn save_contact(&self, request: SaveContactRequest) -> ImResult<Contact>;
    pub fn contacts(&self, query: ContactQuery) -> ImResult<Page<Contact>>;
    pub fn relation_status(&self, peer: PeerRef) -> ImResult<RelationStatus>;
}
```

## 3. 内部实现

内部可以继续使用：

- user-service RPC。
- handle lookup RPC。
- local contact cache。
- profile projection。

但不暴露 raw RPC params 或 contact store row。

## 4. 与 messages/groups 的关系

- `messages.send(Direct)` 内部调用 directory 解析 peer。
- `groups.add_member` 内部调用 directory 解析 member。
- 本地 message projection 可以更新 contact cache。
- App 不需要重复做 handle/DID resolve。

## 5. 第一阶段落地

先迁移/封装：

- `id resolve`。
- profile get/set。
- contact cache projection。
- relation status 如果现有 CLI 还不完整，可以先提供接口占位或返回 `UnsupportedCapability`。
