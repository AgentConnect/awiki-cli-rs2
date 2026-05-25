# Phase 2R：people / follow / contact / profile 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`

**参考文档**：

```text
docs/sdk-refactor/implementation-playbook.md
docs/sdk-refactor/plan/phase2-phase3-migration-execution-plan.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/architecture.md
docs/sdk-refactor/modules/02-identity.md
docs/sdk-refactor/modules/06-directory.md
docs/sdk-refactor/plan/im-core-public-api-surface-review.md
```

**目标**：在保持 `im-core` 不依赖 `awiki-cli`、不整体搬迁 legacy 目录的前提下，补齐用户关系和通讯录能力，让 `ImCore / ImClient` 支持：

```text
1. 关注、取消关注、查询关系状态、查询关注列表、查询粉丝列表。
2. 添加和查询本地通讯录。
3. 读取和设置自己的 Profile，并继续支持公开 Profile 查询。
```

这是一组 Phase 2 后续切片，本文称为 **Phase 2R**。它复用已经迁入 `im-core` 的 identity/profile/directory/contact 基础，不重新设计 P1 message 或 P3 group/local_state。

---

## 1. 当前迁移状态

### 1.1 已经迁入 im-core 的能力

`ImClient` 已经暴露这些 service：

```rust
client.identity()
client.directory()
client.messages()
client.groups()
client.attachments()
client.realtime()
```

其中本计划相关的是：

```text
crates/im-core/src/core/client.rs
crates/im-core/src/identity/service.rs
crates/im-core/src/directory/service.rs
```

Profile 已经有真实 `im-core` 路径：

```text
IdentityService::profile()
IdentityService::update_profile(ProfilePatch)
internal::profile_runtime
internal::identity_wire::profile
compat::profile
```

CLI 侧 `id profile get/set` 已经通过 `crates/awiki-cli/src/im_core_adapter/identity.rs` 调用 `client.identity().profile()` 和 `client.identity().update_profile(...)`。CLI 的 `--markdown-file` 读取仍留在 adapter 层，符合 SDK 边界。

Directory / contact 已经有基础 public API：

```text
DirectoryService::resolve_peer(...)
DirectoryService::lookup_handle(...)
DirectoryService::public_profile(...)
DirectoryService::save_contact(...)
DirectoryService::contacts(...)
DirectoryService::relation_status(...)
```

本地 contact store 已经迁入：

```text
crates/im-core/src/internal/contact_store/records.rs
crates/im-core/src/internal/contact_store/projection.rs
crates/im-core/src/compat/directory.rs
```

SQLite schema 中已经有关系相关基础表和字段：

```text
contacts.followed
contacts.messaged
contacts.relationship
relationship_events
contact_handle_bindings
owner_identity_id fallback owner_did
```

已有测试覆盖了 profile、directory、contacts、relation status 的基础路径：

```text
crates/im-core/tests/phase2_identity_directory.rs
crates/im-core/tests/identity_wire_contract.rs
crates/awiki-cli/tests/store_contact_contract.rs
```

### 1.2 计划编写时仍未迁入或未完成的能力

本文开始执行前，远端用户关系能力还没有进入 `im-core` public API：

```text
follow
unfollow
get_status
get_followers
get_following
```

本文开始执行前，`awiki-cli` command metadata 里的以下命令仍是 planned placeholders：

```text
people.follow
people.unfollow
people.status
people.followers
people.following
people.contacts.list
people.contacts.save
```

当前 `im-core` 只有本地 contact 的 `followed` 投影字段，没有远端 authoritative relationship runtime，也没有 `relationship_events` 追加写入的 SDK 业务路径。

相邻 user-service 和旧脚本确认远端 DID relationship 的当前形态：

```text
POST /user-service/did/relationships/rpc

follow        params: { "target_did": "..." }
unfollow      params: { "target_did": "..." }
get_status    params: { "target_did": "..." }
get_followers params: { "limit": 50, "offset": 0 }
get_following params: { "limit": 50, "offset": 0 }
```

旧 Python 脚本 `manage_relationship.py` 的行为是：

```text
follow/unfollow 成功后 best-effort upsert contacts
follow/unfollow 成功后 best-effort append relationship_events
status/followers/following 只读取远端结果
```

Phase 2R 应迁移这个行为，但不要把 Python 脚本的 CLI 输出形态、stderr 文案或 argparse 设计带进 `im-core`。

---

## 2. 边界原则

### 2.1 public API 归属

本计划采用保守方案：**不新增 `client.people()` service**，先继续把 people/contact/relationship 能力放在 `DirectoryService` 下。

原因：

```text
1. public-api.md 当前已有 DirectoryService，并已包含 contacts / relation_status。
2. im-core-public-api-surface-review.md 建议 follow/relation 增强仍可放在 DirectoryService，或未来再独立 People/Contacts service。
3. 新增 PeopleService 会扩大 public API，需要同步更新 public-api.md、modules/06-directory.md、Interface 文档和 Flutter/Dart facade 文档。
```

CLI 仍然可以保留 `people.*` 命令面；CLI handler 只是把 `people.*` 转换成 `client.directory().*` 调用。

### 2.2 local relation 与 remote relationship 的关系

当前 `DirectoryService::relation_status(peer)` 是本地 contact projection 查询。Phase 2R 不直接改变它的语义，避免破坏已有 contact tests。

新增远端 authoritative API：

```rust
pub fn relationship_status(&self, peer: PeerRef) -> ImResult<RelationshipStatus>;
```

后续如果要把 `relation_status` 合并为 remote-first / local-fallback，需要单独 PR 更新 `public-api.md` 并标注兼容策略。

### 2.3 user_id 不进入新增 public DTO

user-service DID relationship 响应里可能包含：

```text
from_user_id
to_user_id
```

这些字段只允许在 `im-core` internal parser 中出现，不进入新增 SDK public DTO、CLI schema、Flutter facade DTO。公开层使用：

```text
Did
Handle
Profile
created_at
relationship booleans
warnings
```

### 2.4 best-effort 本地投影

关系远端写成功后，本地投影只做 best-effort：

```text
follow:
  contacts.followed = true
  contacts.relationship = "following"
  relationship_events event_type = "followed", status = "applied"

unfollow:
  contacts.followed = false
  contacts.relationship = "none"
  relationship_events event_type = "unfollowed", status = "applied"
```

本地 SQLite 写失败不应把已成功的远端 follow/unfollow 变成失败；结果里返回 warning。

---

## 3. 目标 public API

在 `crates/im-core/src/directory/dto.rs` 增加：

```rust
pub struct FollowRequest {
    pub peer: crate::ids::PeerRef,
}

pub struct UnfollowRequest {
    pub peer: crate::ids::PeerRef,
}

pub struct FollowResult {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub is_friend: bool,
    pub relation: RelationshipStatus,
    pub warnings: Vec<String>,
}

pub struct UnfollowResult {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub ok: bool,
    pub relation: RelationshipStatus,
    pub warnings: Vec<String>,
}

pub struct RelationshipStatus {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub is_following: bool,
    pub is_follower: bool,
    pub is_friend: bool,
    pub is_blocked: bool,
    pub is_blocked_by: bool,
    pub is_contact: bool,
    pub messaged: bool,
    pub relationship: Option<String>,
    pub warnings: Vec<String>,
}

pub struct RelationshipListQuery {
    pub limit: Option<crate::ids::PageLimit>,
    pub offset: Option<u32>,
    pub hydrate_profiles: bool,
}

pub struct RelationshipListItem {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub profile: Option<crate::identity::Profile>,
    pub created_at: Option<String>,
    pub warnings: Vec<String>,
}
```

在 `crates/im-core/src/directory/service.rs` 增加：

```rust
impl DirectoryService<'_> {
    pub fn follow(&self, request: FollowRequest) -> ImResult<FollowResult>;
    pub fn unfollow(&self, request: UnfollowRequest) -> ImResult<UnfollowResult>;
    pub fn relationship_status(&self, peer: PeerRef) -> ImResult<RelationshipStatus>;
    pub fn followers(&self, query: RelationshipListQuery) -> ImResult<Page<RelationshipListItem>>;
    pub fn following(&self, query: RelationshipListQuery) -> ImResult<Page<RelationshipListItem>>;
}
```

Profile API 继续使用既有形态：

```rust
client.identity().profile()
client.identity().update_profile(ProfilePatch { ... })
client.directory().public_profile(IdentitySubject::...)
```

Contact API 继续使用既有形态：

```rust
client.directory().save_contact(SaveContactRequest { ... })
client.directory().contacts(ContactListQuery { ... })
client.directory().relation_status(peer)
```

Phase 2R 可以增强 `save_contact`：当 `SaveContactRequest.peer` 是 handle 且 `did` 为空时，先通过 directory resolve 得到 DID，再写入本地 contact。这个增强不改变 public API。

---

## 4. 目标目录

新增或扩展：

```text
crates/im-core/src/directory/dto.rs
crates/im-core/src/directory/service.rs
crates/im-core/src/internal/identity_wire/relationships.rs
crates/im-core/src/internal/relationship_runtime.rs
crates/im-core/src/internal/contact_store/relationships.rs
crates/im-core/src/internal/contact_store/records.rs
crates/im-core/src/compat/directory.rs
crates/im-core/src/prelude.rs
```

CLI cutover 涉及：

```text
crates/awiki-cli/src/im_core_adapter/people.rs
crates/awiki-cli/src/app/people_handlers.rs
crates/awiki-cli/src/app.rs
crates/awiki-cli/src/cmdmeta/mod.rs
```

如果不想立刻改 CLI 行为，可以先只交付 `im-core` API 和测试，CLI cutover 单独排到最后。

---

## 5. PR 切片

## PR 2R-A：基线确认和 public API 文档

### 目标

冻结当前状态，明确本轮新增 API 仍放在 `DirectoryService`。

### 改动范围

```text
docs/sdk-refactor/public-api.md
docs/sdk-refactor/modules/06-directory.md
docs/sdk-refactor/plan/im-core-public-api-surface-review.md
crates/im-core/src/directory/dto.rs
crates/im-core/src/prelude.rs
```

### 执行步骤

```text
1. 在 public-api.md 中补充 relationship API 草案。
2. 在 modules/06-directory.md 中说明 remote relationship 和 local relation_status 的区别。
3. 新增 FollowRequest、UnfollowRequest、RelationshipStatus、RelationshipListQuery、RelationshipListItem DTO。
4. DTO 不包含 from_user_id / to_user_id。
5. 添加 compile-level / serde roundtrip 测试。
```

### 验收

```bash
cargo test -p im-core directory
rg "from_user_id|to_user_id|ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

完成标准：

```text
1. 新 DTO 可编译、可序列化。
2. im-core public API 不暴露 CLI 类型。
3. 文档明确不新增 client.people()。
```

---

## PR 2R-B：relationship wire builder

### 目标

把 DID relationship 的纯 wire builder 迁入 `im-core`。

### 改动范围

```text
crates/im-core/src/internal/identity_wire/relationships.rs
crates/im-core/src/internal/identity_wire/mod.rs
crates/im-core/tests/identity_wire_contract.rs
```

### Wire 形态

```text
endpoint: /user-service/did/relationships/rpc
profile: RpcDefault for follow/unfollow/status
profile: RpcReadHeavy for followers/following
```

Builders：

```rust
build_follow_rpc_call(target_did)
build_unfollow_rpc_call(target_did)
build_relationship_status_rpc_call(target_did)
build_followers_rpc_call(limit, offset)
build_following_rpc_call(limit, offset)
```

### 关键规则

```text
1. target 使用 target_did，不使用 target_id。
2. get_status 使用 target_did。
3. list 返回中的 from_user_id / to_user_id 只在 internal parser 中允许出现。
4. 不迁移 block/unblock；这是后续关系安全能力。
```

### 验收

```bash
cargo test -p im-core identity_wire_contract relationship
```

完成标准：

```text
1. RPC method、endpoint、params 与 user-service DID relationship router 一致。
2. 参数校验覆盖空 DID、limit=0、offset。
```

---

## PR 2R-C：relationship runtime

### 目标

实现 `DirectoryService` 的远端 relationship 真实调用。

### 改动范围

```text
crates/im-core/src/internal/relationship_runtime.rs
crates/im-core/src/directory/service.rs
crates/im-core/src/internal/transport.rs
crates/im-core/tests/phase2_relationship_directory.rs
```

### 执行流程

`follow(peer)`：

```text
1. validate peer 非空。
2. 如果 peer 是 DID，直接使用。
3. 如果 peer 是 handle，通过 DirectoryRuntime::resolve_peer 解析 DID。
4. 拒绝自己关注自己，返回 InvalidInput。
5. ensure UserProfile session / 使用 CoreHttpTransport authenticated_rpc。
6. 调 follow RPC。
7. 解析 is_friend。
8. 调 relationship_status 获取 authoritative 状态，失败时不回滚 follow，只写 warning。
9. 返回 FollowResult。
```

`unfollow(peer)`：

```text
1. 与 follow 相同的 peer -> DID 解析。
2. 调 unfollow RPC。
3. 调 relationship_status 获取最新状态，失败时 warning。
4. 返回 UnfollowResult。
```

`relationship_status(peer)`：

```text
1. peer -> DID。
2. 调 get_status RPC。
3. 合并本地 contact projection，补 is_contact / messaged / relationship。
4. 返回 RelationshipStatus。
```

`followers/following(query)`：

```text
1. validate limit > 0，默认 limit=50，offset=0。
2. 调 get_followers / get_following。
3. 从 from_did/to_did 选择 peer DID。
4. hydrate_profiles=true 时 best-effort public_profile。
5. 转成 Page<RelationshipListItem>。
6. 如果服务不返回 total，用 items.len() == limit 推断 has_more。
```

### 验收

```bash
cargo test -p im-core phase2_relationship_directory
rg "from_user_id|to_user_id" crates/im-core/src/directory crates/im-core/src/prelude.rs crates/im-core/tests
```

完成标准：

```text
1. follow/unfollow/status/list 都可用 fake transport 单测验证。
2. public DTO 不泄漏 user_id。
3. handle target 会通过现有 directory resolve 先转 DID。
```

---

## PR 2R-D：relationship local projection

### 目标

把旧脚本中的本地沉淀行为迁入 `im-core` internal contact store。

### 改动范围

```text
crates/im-core/src/internal/contact_store/relationships.rs
crates/im-core/src/internal/contact_store/records.rs
crates/im-core/src/internal/local_state/schema.rs
crates/im-core/src/compat/directory.rs
crates/im-core/tests/phase2_relationship_directory.rs
crates/awiki-cli/tests/store_recover_merge_contract.rs
crates/awiki-cli/tests/store_rebind_contract.rs
```

### 新增 internal 能力

```rust
record_follow_applied(client, target_did, target_handle, status)
record_unfollow_applied(client, target_did, target_handle, status)
append_relationship_event(connection, RelationshipEventRecord)
list_relationship_events(...)
```

### 关系事件约定

```text
follow:
  event_type = "followed"
  status = "applied"

unfollow:
  event_type = "unfollowed"
  status = "applied"

source_type:
  "directory.relationship"
```

### 写入规则

```text
1. 同时写 owner_identity_id 和 owner_did。
2. 查询优先 owner_identity_id，fallback owner_did。
3. relationship_events 使用全局 event_id，保持 store_recover_merge_contract 当前约束。
4. 本地投影失败只进入 warnings，不覆盖远端成功。
```

### 验收

```bash
cargo test -p im-core relationship
cargo test -p awiki-cli --test store_recover_merge_contract
cargo test -p awiki-cli --test store_rebind_contract
```

完成标准：

```text
1. follow/unfollow 成功后 contacts.followed 和 relationship_events 都被 best-effort 写入。
2. local projection 写失败不会让远端结果失败。
3. owner_identity_id 兼容策略不破坏现有 store tests。
```

---

## PR 2R-E：contact save/list 补强和 CLI people.contacts cutover

### 目标

完成“添加、查询通讯录”的 SDK 和 CLI 迁移闭环。

### im-core 范围

```text
1. DirectoryService::save_contact 支持 peer 为 handle 且 did 为空时自动 resolve。
2. ContactListQuery 保持 limit-only；如需 search/cursor，另起 API PR。
3. SaveContactRequest.reason 不进入 SDK；CLI 的 --reason 映射到 note 或 relationship。
```

### CLI 范围

```text
people.contacts.save --did <did> [--handle <handle>] [--reason <text>]
people.contacts.list
people.status <TARGET>   # 如果只查询本地，则走 relation_status；如果查询远端，则走 relationship_status。
```

建议 `people.status` 默认走远端 `relationship_status`，并在输出中包含本地 `is_contact / messaged` 投影。

### 改动范围

```text
crates/awiki-cli/src/im_core_adapter/people.rs
crates/awiki-cli/src/app/people_handlers.rs
crates/awiki-cli/src/app.rs
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/tests/cli_shell_core_contract.rs
crates/awiki-cli/tests/cli_cutover_command_surface_contract.rs
```

### 验收

```bash
cargo test -p im-core contacts
cargo test -p awiki-cli --test store_contact_contract
cargo test -p awiki-cli --test cli_shell_core_contract people
cargo test -p awiki-cli --test cli_cutover_command_surface_contract people
```

完成标准：

```text
1. IMCore 已可 save/list contacts。
2. CLI people.contacts 不再是 stub。
3. CLI 仍负责输出格式、ExitError 和确认文案。
```

---

## PR 2R-F：people follow/following/followers CLI cutover

### 目标

让 CLI reserved people relationship 命令调用 `im-core`，同时保持 SDK 业务逻辑独立。

### CLI 命令

```text
people.follow <TARGET>
people.unfollow <TARGET>
people.status <TARGET>
people.followers [--limit N] [--offset N]
people.following [--limit N] [--offset N]
```

`TARGET` 支持 DID 或 handle，由 CLI adapter 转成 `PeerRef`，最终由 `im-core` 解析。

### 确认策略

`people.follow`、`people.unfollow` 是远端关系写操作。CLI 应沿用现有危险/有副作用命令的确认或 dry-run 约定；SDK 不实现交互确认。

### 输出策略

CLI 输出可以保留 command envelope：

```json
{
  "action": "follow",
  "target": "...",
  "relationship": {
    "is_following": true,
    "is_follower": false,
    "is_friend": false
  },
  "warnings": []
}
```

但 `im-core` 只返回 DTO，不返回 CLI envelope。

### 验收

```bash
cargo test -p awiki-cli --test cli_shell_core_contract people
cargo test -p awiki-cli --test cli_cutover_command_surface_contract people
```

Optional / live：

```bash
cargo test -p awiki-cli --test identity_live_contract people_relationship
```

完成标准：

```text
1. people.follow/unfollow/status/followers/following 不再是 stub。
2. CLI handlers 只做参数、确认、输出和 ImError -> ExitError 映射。
3. 远端 RPC 和本地投影逻辑只在 im-core。
```

---

## PR 2R-G：Profile 完成度收敛

### 目标

Profile 主路径已经迁入，本 PR 只做收敛，确保“设置自己的 Profile”作为本计划验收项稳定。

### 检查项

```text
1. IdentityService::profile 使用 authenticated profile RPC。
2. IdentityService::update_profile 使用 ProfilePatch。
3. update_profile 空 patch 返回 InvalidInput，不发远端请求。
4. markdown 文件读取仍在 CLI adapter，不进入 im-core。
5. display_name 更新本地 identity summary 的逻辑仍在 CLI/identity store wrapper，不进入 SDK profile RPC。
6. public_profile 成功后 best-effort 写 contact projection。
```

### 测试

```bash
cargo test -p im-core phase2_identity_directory
cargo test -p im-core identity_wire_contract
cargo test -p awiki-cli --test identity_im_core_mvp_contract profile
cargo test -p awiki-cli --test identity_profile_set_upgrade_contract
```

完成标准：

```text
1. client.identity().update_profile(...) 是设置自己 Profile 的唯一 SDK 主入口。
2. CLI id.profile.set 继续走 im-core。
3. Profile 相关 wire builder 不再需要 legacy 真实实现。
```

---

## PR 2R-H：compat 收敛和文档验收

### 目标

迁移完成后清理临时 compat、更新命令状态和文档。

### 改动范围

```text
docs/sdk-refactor/public-api.md
docs/sdk-refactor/modules/06-directory.md
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
docs/sdk-refactor/plan/awiki_im_core_flutter_plan.md
skills/references/09-people-planned.md
crates/awiki-cli/src/cmdmeta/mod.rs
crates/im-core/src/compat/directory.rs
```

### 执行步骤

```text
1. 把已迁移的 people relationship/contact 命令从 planned placeholder 改为 implemented。
2. 更新 People Planned Reference，不再说 contacts/follow 全部不可用。
3. 标注 block/unblock/search/recommendation 仍未实现。
4. 对 compat directory 中不再被 awiki-cli 使用的迁移接口加 #[doc(hidden)] 或删除。
5. 更新 Flutter plan：relationship APIs 可以开始绑定 im-core，而不是返回 unsupported("relationship-remote-mutation")。
```

### 验收

```bash
cargo test -p im-core
cargo test -p awiki-cli --test cli_shell_core_contract
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
rg "people.*stub|relationship-remote-mutation|reserved but currently unimplemented" docs/sdk-refactor/public-api.md docs/sdk-refactor/modules/06-directory.md docs/sdk-refactor/plan/cli-im-core-cutover-plan.md docs/sdk-refactor/plan/awiki_im_core_flutter_plan.md skills/references/09-people-planned.md crates/awiki-cli/src crates/awiki-cli/tests
```

完成标准：

```text
1. 文档、cmdmeta、测试对 people/contact/profile 状态一致。
2. 未实现能力明确留在后续阶段。
3. im-core 仍无 awiki-cli 反向依赖。
```

---

## 6. 不在本计划范围内

```text
1. block / unblock。
2. people search。
3. 推荐联系人算法。
4. group discovery 自动推荐、自动 follow、自动 DM。
5. ContactService / PeopleService 独立 public service。
6. 通讯录全文搜索、分页 cursor、排序策略。
7. 真实系统测试和生产环境联调。
8. Flutter/Dart facade 真实实现；只更新计划和解除 unsupported 的后续入口。
```

---

## 7. 最终验收矩阵

### im-core SDK 验收

```rust
let client = core.client(IdentitySelector::Default)?;

client.identity().update_profile(ProfilePatch {
    display_name: Some("Alice".to_string()),
    bio: Some("Rust port".to_string()),
    tags: Some(vec!["sdk".to_string()]),
    markdown: Some("## Alice".to_string()),
})?;

client.directory().save_contact(SaveContactRequest { ... })?;
client.directory().contacts(ContactListQuery { ... })?;

client.directory().follow(FollowRequest { peer })?;
client.directory().relationship_status(peer)?;
client.directory().following(RelationshipListQuery { ... })?;
client.directory().followers(RelationshipListQuery { ... })?;
client.directory().unfollow(UnfollowRequest { peer })?;
```

### Required tests

```bash
cargo test -p im-core phase2_identity_directory
cargo test -p im-core phase2_relationship_directory
cargo test -p im-core identity_wire_contract
cargo test -p awiki-cli --test store_contact_contract
cargo test -p awiki-cli --test cli_shell_core_contract people
cargo test -p awiki-cli --test cli_cutover_command_surface_contract people
```

### Import fence

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
rg "from_user_id|to_user_id" crates/im-core/src/directory crates/im-core/src/prelude.rs
```

### 完成定义

```text
1. im-core 支持 Profile self get/update，且 CLI id.profile.get/set 继续走 im-core。
2. im-core 支持 contacts save/list/relation local projection。
3. im-core 支持 follow/unfollow/relationship_status/followers/following 远端 DID relationship。
4. follow/unfollow 成功后 best-effort 写 contacts 和 relationship_events。
5. people relationship/contact CLI 可选切到 im-core；如果本轮包含 CLI PR，则不再是 placeholder。
6. public API 不暴露 CLI 类型、raw JSON-RPC payload、SQLite record、from_user_id/to_user_id。
```
