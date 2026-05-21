# 05-groups：Phase 3 群生命周期与成员管理

## 1. 目标

Phase 1 的“群聊”只指群消息能力：面向已有 `GroupRef` 发送和读取普通群文本消息。完整群生命周期、成员管理和群 profile/policy 更新不进入 Phase 1。

这样可以先跑通 SDK 主链路：身份、auth、私聊、群聊文本。群管理等更复杂能力后移到 Phase 3。

## 2. Phase 1 群消息入口

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group_ref),
    body: MessageBody::Text { text, kind: MessageKind::Text },
    security: MessageSecurityMode::DefaultPlain,
    client_message_id: None,
    delivery: MessageDeliveryOptions::default(),
})?;

client.messages().history(ThreadRef::Group(group_ref), query)?;
```

P1 不要求 `client.groups()`。

## 3. Phase 3 GroupService

```rust
pub struct GroupService<'a> {
    client: &'a ImClient,
}

impl GroupService<'_> {
    pub fn create(&self, request: CreateGroupRequest) -> ImResult<GroupSnapshot>;
    pub fn get(&self, group: GroupRef) -> ImResult<GroupSnapshot>;
    pub fn list(&self, query: GroupQuery) -> ImResult<Page<GroupSummary>>;
    pub fn join(&self, group: GroupRef, request: JoinGroupRequest) -> ImResult<GroupMembershipChange>;
    pub fn leave(&self, group: GroupRef, request: LeaveGroupRequest) -> ImResult<GroupMembershipChange>;
    pub fn add_member(&self, group: GroupRef, member: PeerRef, options: AddMemberOptions) -> ImResult<GroupMembershipChange>;
    pub fn remove_member(&self, group: GroupRef, member: PeerRef, options: RemoveMemberOptions) -> ImResult<GroupMembershipChange>;
    pub fn update_profile(&self, group: GroupRef, patch: GroupProfilePatch) -> ImResult<GroupSnapshot>;
    pub fn update_policy(&self, group: GroupRef, patch: GroupPolicyPatch) -> ImResult<GroupSnapshot>;
    pub fn members(&self, group: GroupRef, query: MemberQuery) -> ImResult<Page<GroupMember>>;
    pub fn messages(&self, group: GroupRef, query: HistoryQuery) -> ImResult<Page<Message>>;
}
```

## 4. internal only

不暴露：

```text
build_group_*_rpc_params
raw group wire DTO
group E2EE add/remove params
MLS KeyPackage / notice / provider path
```

## 5. CLI 边界

Phase 1：

- `msg send --group` 适配到 `client.messages().send(MessageTarget::Group)`。
- `group messages` 适配到 `client.messages().history(ThreadRef::Group)`。

Phase 3：

- `group create/get/list/join/leave/add/remove/update/members` 再迁入 `client.groups()`。

## 6. 完成判定

P1 完成时不要求 group lifecycle 迁移。P3 完成时，普通群管理命令通过 `client.groups()`，group wire helper 不作为 SDK public API。
