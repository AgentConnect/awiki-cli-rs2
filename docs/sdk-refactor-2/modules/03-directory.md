# 03-directory：Phase 2 联系人、解析与资料能力

## 1. 目标

`directory` 是后续 App 体验很重要的模块，但不进入 Phase 1 验收。Phase 1 的私聊发送可以在 `messages` 内部完成最小 handle/DID 解析，不要求公开完整 `DirectoryService`。

## 2. Phase 2 public API

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
    pub fn resolve_peer(&self, subject: IdentitySubject) -> ImResult<PeerProfile>;
    pub fn lookup_handle(&self, handle: Handle) -> ImResult<ResolvedIdentity>;
    pub fn save_contact(&self, request: SaveContactRequest) -> ImResult<Contact>;
    pub fn contacts(&self, query: ContactQuery) -> ImResult<Page<Contact>>;
    pub fn relation_status(&self, peer: PeerRef) -> ImResult<RelationStatus>;
}
```

## 3. Phase 1 允许的内部解析

`client.messages().send(MessageTarget::Direct(PeerRef))` 可以内部使用最小 resolver：

- 如果是 DID，直接使用。
- 如果是 handle，调用 user-service 或已有 helper 解析成 DID。
- 解析结果可以作为内部 target resolution，不要求写入完整 contact cache。

这个 resolver 不作为 P1 public `directory` API 暴露。

## 4. internal only

不暴露：

```text
raw user-service RPC
contact store row
upsert_contact_record(owner, paths, record)
LocalStatePaths
owner DID 参数
```

## 5. 完成判定

Phase 2 完成后，App/CLI 不再自己重复实现 handle/DID/profile/relation 的业务解析；P1 不以此作为阻塞项。
