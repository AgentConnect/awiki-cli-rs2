# groups 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P3；P1 只通过 `messages().send(MessageTarget::Group)` 支持面向已有群的文本消息。  
**职责**：群组生命周期、成员和群消息。

## 1. 目标

`groups` 负责群生命周期、成员管理、群 profile/policy、群消息读取以及群状态变更 notification 投影。

## 2. P1 边界

P1 不公开或不要求实现完整 `GroupService`。群聊 MVP 通过 messages 模块完成：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group_ref),
    body: MessageBody::Text { ... },
    security: MessageSecurityMode::DefaultPlain,
    ..
})?;
```

P1 可以支持 `client.messages().history(ThreadRef::Group(group_ref), query)` 的必要子集。

## 3. P3 职责

- `create(CreateGroupRequest) -> GroupSnapshot`。
- `get(group)`。
- `list(query)`。
- `join(group, request)`。
- `leave(group, request)`。
- `add_member(group, member, options)`。
- `remove_member(group, member, options)`。
- `update_profile(group, patch)`。
- `update_policy(group, patch)`。
- `members(group, query)`。
- `messages(group, query)`。
- 群状态变更 notification 投影。

## 4. 群组和消息边界

- 群生命周期和成员管理归 `groups`。
- 向群发送普通消息由 `messages.send(MessageTarget::Group)` 统一处理。
- 读取群消息可以在 `groups.messages` 暴露，也可以委托到 `messages.history(ThreadRef::Group)`；P3 可以同时保留 `groups.messages` 作为 convenience API。

## 5. 接口草案

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

    pub fn send_text(&self, group: GroupRef, text: String) -> ImResult<SendMessageResult>;
}
```

`send_text` 内部等价于 `client.messages().send(MessageTarget::Group)`，不要重复实现业务逻辑。

## 6. CLI 边界

CLI 负责参数解析、dry-run 呈现、输出格式和命令 UX。群状态、成员规则、远端调用和本地投影归 `groups`。

公开接口挂在 `ImClient` 上，自动使用绑定身份。内部实现可以继续把 actor 传入底层 transport/store helper，但 SDK 主接口不暴露 `ActorContext`。
