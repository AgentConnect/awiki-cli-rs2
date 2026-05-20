# groups 模块接口设计

**阅读顺序**：08 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：群组生命周期、成员和群消息。

## 1. 目标

`groups` 负责群生命周期、成员管理、群 profile/policy、群消息读取以及群状态变更 notification 投影。

## 2. 主要职责

- `create(actor, CreateGroupRequest) -> GroupSnapshot`。
- `get(actor, group)`。
- `list(actor, query)`。
- `join(actor, group, reason)`。
- `leave(actor, group, reason, security_mode)`。
- `add_member(actor, group, member, role, options)`。
- `remove_member(actor, group, member, reason, options)`。
- `update_profile(actor, group, patch)`。
- `update_policy(actor, group, patch)`。
- `members(actor, group, query)`。
- `messages(actor, group, query)`。
- 群状态变更 notification 投影。

## 3. 群组和消息边界

- 群生命周期和成员管理归 `groups`。
- 向群发送一条普通消息可以由 `messages.send(MessageTarget::Group)` 统一处理。
- 读取群消息可以在 `groups.messages` 暴露，也可以委托到 `messages.history(ThreadRef::Group)`；第一版建议保留 `groups.messages`，因为当前 CLI 已经是 `group messages`。

## 4. 接口草案

```rust
pub struct GroupService<'a> {
    core: &'a ImCore,
}

impl GroupService<'_> {
    pub async fn create(
        &self,
        actor: ActorContext,
        request: CreateGroupRequest,
    ) -> ImResult<GroupSnapshot>;

    pub async fn get(
        &self,
        actor: ActorContext,
        group: GroupRef,
    ) -> ImResult<GroupSnapshot>;

    pub async fn list(
        &self,
        actor: ActorContext,
        query: GroupQuery,
    ) -> ImResult<GroupPage>;

    pub async fn join(
        &self,
        actor: ActorContext,
        group: GroupRef,
        reason: Option<String>,
    ) -> ImResult<GroupMembershipChange>;

    pub async fn leave(
        &self,
        actor: ActorContext,
        group: GroupRef,
        request: LeaveGroupRequest,
    ) -> ImResult<GroupMembershipChange>;

    pub async fn add_member(
        &self,
        actor: ActorContext,
        group: GroupRef,
        member: PeerRef,
        options: AddMemberOptions,
    ) -> ImResult<GroupMembershipChange>;

    pub async fn remove_member(
        &self,
        actor: ActorContext,
        group: GroupRef,
        member: PeerRef,
        options: RemoveMemberOptions,
    ) -> ImResult<GroupMembershipChange>;

    pub async fn update_profile(
        &self,
        actor: ActorContext,
        group: GroupRef,
        patch: GroupProfilePatch,
    ) -> ImResult<GroupSnapshot>;

    pub async fn update_policy(
        &self,
        actor: ActorContext,
        group: GroupRef,
        patch: GroupPolicyPatch,
    ) -> ImResult<GroupSnapshot>;

    pub async fn members(
        &self,
        actor: ActorContext,
        group: GroupRef,
        query: MemberQuery,
    ) -> ImResult<GroupMemberPage>;

    pub async fn messages(
        &self,
        actor: ActorContext,
        group: GroupRef,
        query: HistoryQuery,
    ) -> ImResult<MessagePage>;
}
```

## 5. CLI 边界

CLI 负责参数解析、dry-run 呈现、输出格式和命令 UX。群状态、成员规则、远端调用和本地投影归 `groups`。
