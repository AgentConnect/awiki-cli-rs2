# 模块设计：groups

## 1. 职责

`groups` 负责群生命周期、成员、群 profile/policy 和群消息读取。

第一阶段只做普通群聊，不做 group E2EE。

## 2. 对外接口

```rust
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

`send_text` 是 convenience，内部委托给 `messages().send(MessageTarget::Group)`。

## 3. DTO

```rust
pub struct CreateGroupRequest {
    pub profile: GroupProfileDraft,
    pub policy: GroupPolicyDraft,
}

pub struct GroupProfileDraft {
    pub display_name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
}

pub struct GroupPolicyDraft {
    pub discoverability: Option<GroupDiscoverability>,
    pub admission: Option<GroupAdmissionMode>,
    pub max_members: Option<u32>,
    pub attachments_allowed: Option<bool>,
}
```

不要把 CLI flag 名称直接变成 SDK 字段。

## 4. 不暴露内容

```rust
build_group_create_rpc_params
build_group_get_rpc_params
build_group_members_rpc_params
build_group_e2ee_*_rpc_params
raw group wire JSON
```

## 5. group E2EE 处理

第一阶段：

- `group.e2ee.*` CLI 命令不进入 SDK default API。
- 如果 CLI 仍需要这些命令，先继续调用旧模块或放在 CLI-only diagnostic 中。
- SDK 中 `GroupPolicyDraft` 可以有普通 message security profile 字段，但不实现 MLS 编排。

Phase 3 再处理：

- `MessageSecurityMode::GroupE2ee`。
- `secure().group_status()`。
- `secure().repair_group_state()`。

## 6. 第一阶段验收

- `group create/get/list/join/leave/add/remove/update/members/messages` 走 SDK。
- `msg send --group` 走 `messages().send()`。
- 群状态落库和 owner 隔离正确。
