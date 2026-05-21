# Phase 2 / Phase 3 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`

**适用阶段**：

```text
docs/sdk-refactor/implementation-playbook.md
  16. Phase 2：identity / directory / profile 补全
  17. Phase 3：message / group / local_state 补全
```

**参考文档**：`docs/sdk-refactor/plan/phase1-beta-migration-execution-plan.md`

**目标**：在 P1 direct/group text、inbox/history MVP 稳定后，继续以 leaf-file / 小子模块级迁移为主，把 identity/profile/directory/contact、message projection、group lifecycle、local_state 隔离能力逐步迁入 `crates/im-core`，同时保持 `awiki-cli` 行为兼容和 legacy fallback。

---

## 0. 约束
  请先阅读并遵守这些文档：
  - docs/sdk-refactor/implementation-playbook.md 的 “Phase 1H：App sandbox path fixture”
  - docs/sdk-refactor/README.md
  - docs/sdk-refactor/architecture.md
  - docs/sdk-refactor/public-api.md
  - docs/sdk-refactor/im-core-cli-boundary.md
  - docs/sdk-refactor/Interface/README.md
  - docs/sdk-refactor/Interface/01-crate-layout.md
  - docs/sdk-refactor/Interface/02-core-interface.md
  - docs/sdk-refactor/Interface/03-identity-auth-interface.md
  - docs/sdk-refactor/Interface/05-cli-adapter-interface.md
  - docs/sdk-refactor/Interface/06-implementation-map.md
  - docs/sdk-refactor/Interface/07-phase1-acceptance.md
  - docs/sdk-refactor/modules/01-core.md
  - docs/sdk-refactor/modules/02-identity.md
  - docs/sdk-refactor/modules/03-auth.md
  - docs/sdk-refactor/modules/04-local-state.md
  - docs/sdk-refactor/modules/07-messages.md
  - docs/sdk-refactor/modules/08-groups.md

  不用进行系统测试，只用进行单元测试即可，系统测试统一做
  
## 1. 总体结论

Phase 2 和 Phase 3 继续沿用 P1-beta 的迁移原则：

```text
主策略：leaf-file / 小子模块级迁移
辅策略：2-5 个强相关文件组成一个垂直业务切片
例外：函数级抽取只用于拆掉少量 CLI 依赖，不作为长期迁移单位
禁止：一次性整体迁移 identity、message、store、runtime、app handlers
```

“文件级迁移”仍然不是 `mv` 原文件。混合职责文件只能抽取本切片相关逻辑，原 `awiki-cli` 文件路径和原函数签名先保留，变成 wrapper 或继续承载未迁移能力。

两个阶段的边界：

```text
Phase 2：identity / directory / profile / contact 领域能力补全。
Phase 3：message / group / local_state 领域能力补全。
```

Phase 2 推荐 PR 顺序：

```text
PR 2A：Phase 2 SDK DTO / service skeleton
PR 2B：迁移 identity profile / directory wire builder
PR 2C：profile get 真迁移
PR 2D：profile update 真迁移
PR 2E：directory resolve / handle lookup 真迁移
PR 2F：directory contacts save/list + relation status + profile projection
PR 2G：bind phone/email 迁移到 identity service
PR 2H：recover handle 迁移
PR 2I：replace DID plan，危险能力后置
PR 2J：replace DID execution + local rebind，单独 PR
```

Phase 3 推荐 PR 顺序：

```text
PR 3A：local_state 边界和 schema adapter scaffold
PR 3B：mark_read 真迁移
PR 3C：conversation projection
PR 3D：owner_identity_id 兼容迁移，暂不迁 E2EE 表写路径
PR 3E：message send state / retry / cache merge
PR 3F：group get/list/members/messages 读路径迁移
PR 3G：group create/join/leave 迁移
PR 3H：group add/remove/update/members 写路径迁移
PR 3I：Phase 3 legacy wrapper 收敛和 compat 清理
```

---

## 2. 阶段边界

### 2.1 Phase 2 做什么

Phase 2 聚焦身份、目录、资料和联系人：

```text
client.identity().profile()
client.identity().update_profile()
client.identity().bind_contact()
core.identities().recover_handle()
client.identity().replace_did()   # late / split plan + execution
client.directory().resolve_peer()
client.directory().lookup_handle()
client.directory().save_contact()
client.directory().contacts()
client.directory().relation_status()
profile projection
```

注意：Phase 2 默认不新增 `client.contacts()` public service。联系人能力先放在 `DirectoryService` 中，与当前 `public-api.md` 保持一致。内部可以有 `contact_store`，但 public API 不单独暴露 `ContactService`。如果后续确实要独立 `ContactService`，必须先更新 `public-api.md`、`modules/06-directory.md` 和 Interface 文档。

Phase 2 可以迁移：

```text
identity/profile/directory 的纯 wire builder
profile get/update 的真实 RPC 调用
handle lookup / DID resolve / public profile projection
contacts 本地存取的 SDK 视图
bind phone/email 的 authenticated REST/RPC 调用
recover handle 的计划和执行
replace DID 的风险计划、远端调用和本地 rebind 调用边界
```

Phase 2 不迁移：

```text
CLI markdown-file 读取
CLI 输出渲染
CLI 危险命令确认
CLI 备份路径展示和文件权限提示
message conversation projection
group lifecycle
owner_identity_id schema 兼容迁移
attachment / realtime / secure direct / group E2EE
```

### 2.2 Phase 3 做什么

Phase 3 聚焦消息、群组和本地状态：

```text
client.messages().mark_read()
client.messages().conversations()
完整本地 message/contact/conversation projection
cache merge
message send state
失败重试
完整 group lifecycle
完整 group members/messages
owner_identity_id 本地隔离
```

Phase 3 可以迁移：

```text
mark_read 远端调用 + 本地已读写入
conversation projection
本地 messages/contacts/groups/group_members 查询和写入边界
owner_identity_id 兼容迁移
group get/list/members/messages 读路径
group create/join/leave 写路径
group add/remove/update/members 写路径
message send state / retry 基础状态机
```

Phase 3 不迁移：

```text
attachment send/download
realtime runner / listener service-run
secure direct E2EE
group E2EE / MLS
systemd / launchd / Windows service
OpenClaw / Hermes host notify
完整 attachment projection
E2EE table write-path owner_identity_id 迁移
```

---

## 3. 进入条件

### 3.1 Phase 2 进入条件

```text
1. P1 direct/group text 主链路稳定。
2. P1 inbox/history MVP 稳定。
3. CLI P1 命令可默认走 im-core，或者可通过 feature flag 走 im-core。
4. App sandbox fixture 通过。
5. im-core 不依赖 awiki-cli。
6. P1-beta compat wrapper 仍可回退 legacy。
```

建议进入前检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli --test msg_contract
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

### 3.2 Phase 3 进入条件

```text
1. P1 message MVP 稳定。
2. Phase 2 profile/directory/contact 稳定。
3. im-core 能解析当前 identity owner context。
4. directory/contact projection 已有 SDK 边界。
5. group.send 普通文本路径可走 im-core 或有稳定 adapter。
6. local_state 写入可 best-effort，不阻断主链路。
```

建议进入前检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test identity_live_contract
cargo test -p awiki-cli --test msg_live_contract
cargo test -p awiki-cli --test store_contact_contract
cargo test -p awiki-cli --test store_messages_contract
```

---

## 4. 目标目录和 API 形态

### 4.1 Phase 2 目标目录

```text
crates/im-core/src/identity/
  mod.rs
  dto.rs
  registry.rs
  service.rs
  profile.rs
  recovery.rs
  replace_did.rs

crates/im-core/src/directory/
  mod.rs
  dto.rs
  service.rs

crates/im-core/src/internal/identity_wire/
  mod.rs
  profile.rs
  directory.rs
  bind.rs
  recovery.rs
  replace_did.rs

crates/im-core/src/internal/directory_runtime/
  mod.rs
  resolve.rs

crates/im-core/src/internal/contact_store/
  mod.rs
  records.rs
  projection.rs

crates/im-core/src/compat/
  identity.rs
  directory.rs
```

建议新增 public service：

```rust
impl ImClient {
    pub fn identity(&self) -> crate::identity::IdentityService<'_>;
    pub fn directory(&self) -> crate::directory::DirectoryService<'_>;
}
```

不建议在 Phase 2 默认新增：

```rust
pub fn contacts(&self) -> crate::contacts::ContactService<'_>;
```

联系人能力先放在：

```rust
client.directory().save_contact(...)
client.directory().contacts(...)
client.directory().relation_status(...)
```

### 4.2 Phase 3 目标目录

```text
crates/im-core/src/messages/
  mod.rs
  dto.rs
  service.rs
  conversations.rs
  mark_read.rs
  send_state.rs

crates/im-core/src/groups/
  mod.rs
  dto.rs
  service.rs

crates/im-core/src/internal/local_state/
  mod.rs
  schema.rs
  migrations.rs
  messages.rs
  contacts.rs
  groups.rs
  conversations.rs
  projection.rs

crates/im-core/src/internal/group_runtime/
  mod.rs
  read.rs
  mutate.rs

crates/im-core/src/compat/
  local_state.rs
  groups.rs
```

建议新增 public service：

```rust
impl ImClient {
    pub fn groups(&self) -> crate::groups::GroupService<'_>;
}
```

`client.messages()` 扩展：

```rust
impl MessageService<'_> {
    pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
    pub fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
}
```

保持与 `public-api.md` 中 P3+ 的签名一致。不要在 Phase 3 文档中另起 `mark_read(request: MarkReadRequest)`，除非先同步更新 `public-api.md`。

---

## 5. 通用边界规则

`im-core` 不能直接使用：

```text
ParsedCommand
ExitError
GlobalOptions
config::Resolved
identity::Manager
identity::types::StoredIdentity
message::types::MessageError
store::MessageRecord
store::GroupRecord
store::ContactRecord
runtime listener types
CLI output JSON envelope
```

允许的迁移方式：

```text
1. awiki-cli wrapper 把 legacy 类型转换成 im-core DTO。
2. im-core internal 使用自己的 record / DTO。
3. im-core compat 暂时为 awiki-cli 暴露迁移期函数。
4. legacy wrapper 稳定两个阶段后再删除。
```

错误规则：

```text
im-core 内部统一返回 ImError。
awiki-cli wrapper 负责映射到 IdentityError / MessageError / ExitError。
im-core 不生成 CLI help/hint 文案。
im-core 不知道 CLI flag 名。
```

---

## 6. Compat 与 internal trait 规则

Phase 2/3 可能继续使用 `im_core::compat` 和 internal trait。规则同 P1-beta：

```text
1. compat API 不进入 prelude。
2. compat API 使用 #[doc(hidden)]。
3. compat API 不承诺 semver。
4. 发布独立 crate 前应放到 non-default feature 或清理。
5. internal store/runtime trait 不是 Phase 7 provider trait。
```

例如 `LocalStateStore`：

```text
Phase 3A 可以定义 internal::local_state::LocalStateStore。
它是 internal store boundary，不是 public provider trait。
不进入 prelude。
不允许 App 在 Phase 3 接管它。
Phase 7 再决定是否演进成外部 provider。
```

---

## 7. 测试分层规则

每个 PR 的测试分三层。

### 7.1 Required：Codex Goal / 单 PR 必跑

```text
cargo test -p im-core <focused>
cargo test -p awiki-cli --test <relevant_contract>
rg import fence
```

### 7.2 Optional integration：合并前或本地补跑

```text
identity_contract
msg_contract
group_contract
store_messages_contract
store_groups_contract
store_contact_contract
```

### 7.3 Manual / live / system：不由默认 Codex Goal 执行

```text
identity_live_contract
msg_live_contract
group_live_contract
msg_ws_*_live_contract
runtime_listener_*_contract
真实 awiki-cli 命令
真实网络请求
真实 workspace 操作
```

只有当某个 PR 明确声明进入系统验证时，才运行 Manual / live / system 测试。

---

## 8. Phase 2 关键 DTO

Phase 2 建议先补齐这些 SDK DTO。

```rust
pub struct Profile {
    pub subject: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Vec<String>,
    pub markdown: Option<String>,
    pub avatar_url: Option<String>,
    pub updated_at: Option<String>,
    pub metadata: Vec<ProfileAttribute>,
}

pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub markdown: Option<String>,
}

pub struct UpdateProfileResult {
    pub profile: Profile,
    pub changed_fields: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct DirectoryResolution {
    pub input: String,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub profile: Option<Profile>,
    pub warnings: Vec<String>,
}

pub struct HandleLookupResult {
    pub handle: crate::ids::Handle,
    pub did: crate::ids::Did,
    pub domain: Option<String>,
    pub status: Option<String>,
}

pub struct Contact {
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub relationship: Option<String>,
    pub followed: bool,
    pub messaged: bool,
    pub note: Option<String>,
    pub last_seen_at: Option<String>,
}

pub struct RelationStatus {
    pub peer: crate::ids::PeerRef,
    pub did: Option<crate::ids::Did>,
    pub is_contact: bool,
    pub followed: bool,
    pub messaged: bool,
    pub relationship: Option<String>,
}
```

CLI 的 `--markdown-file` 读取结果转换成 `UpdateProfileRequest.markdown`，文件路径不进入 `im-core` public API。

---

## 9. PR 2A：Phase 2 SDK DTO / service skeleton

### 9.1 目标

先建立 Phase 2 public API 形态，不迁真实远端执行。

### 9.2 改动范围

```text
crates/im-core/src/identity/dto.rs
crates/im-core/src/identity/service.rs
crates/im-core/src/identity/profile.rs
crates/im-core/src/directory/mod.rs
crates/im-core/src/directory/dto.rs
crates/im-core/src/directory/service.rs
crates/im-core/src/core/client.rs
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
```

### 9.3 执行步骤

```text
1. 新增 IdentityService、DirectoryService。
2. 在 ImClient 上增加 identity() / directory()。
3. 新增 DTO，但先只做输入校验和 unsupported/stub 返回。
4. 添加 boundary 测试，确认 public API 不泄漏 CLI 类型。
5. 不改 awiki-cli 默认行为。
```

### 9.4 Required 验收

```bash
cargo test -p im-core identity
cargo test -p im-core directory
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

### 9.5 完成标准

```text
1. Phase 2 API 形态存在。
2. im-core 仍可独立编译。
3. awiki-cli 行为零变化。
```

---

## 10. PR 2B：迁移 identity profile / directory wire builder

### 10.1 目标

把 identity/profile/directory 的纯 wire builder 迁入 `im-core`。

### 10.2 源和目标

源：

```text
crates/awiki-cli/src/identity/wire.rs
```

目标：

```text
crates/im-core/src/internal/identity_wire/profile.rs
crates/im-core/src/internal/identity_wire/directory.rs
crates/im-core/src/internal/identity_wire/bind.rs
crates/im-core/src/internal/identity_wire/recovery.rs
crates/im-core/src/internal/identity_wire/replace_did.rs
crates/im-core/src/compat/identity.rs
crates/im-core/src/compat/directory.rs
```

### 10.3 迁移范围

优先迁移：

```text
build_get_me_profile_rpc_call
build_update_me_profile_rpc_call
build_public_profile_rpc_call
build_profile_resolve_rpc_call
build_handle_lookup_by_did_rpc_call
build_handle_lookup_by_handle_rpc_call
build_phone_bind_send_rest_call
build_phone_bind_verify_rest_call
build_email_send_rest_call
build_email_status_rest_call
build_recover_handle_rpc_call
build_replace_did_rpc_call
normalize_email / normalize_phone / sanitize_otp / split_csv
```

暂不迁移：

```text
CommandResult 构造
CLI summary/warnings 文案
IdentityError 具体 CLI 映射
Manager 持久化
replace DID 备份文件操作
```

### 10.4 执行方式

```text
1. im-core 内定义自己的 RpcCall / RestCall / TransportProfile。
2. awiki-cli wrapper 把 im-core compat call 转换成 legacy client 需要的 call。
3. awiki-cli/src/identity/wire.rs 保留旧函数名和旧测试。
4. 复制 identity_wire_contract.rs 中纯 wire 断言到 im-core tests。
```

### 10.5 Required 验收

```bash
cargo test -p im-core identity_wire
cargo test -p awiki-cli --test identity_wire_contract
```

### 10.6 完成标准

```text
1. profile/directory/bind/recover/replace DID wire shape 由 im-core 覆盖测试。
2. awiki-cli 原测试继续通过。
3. im-core 不依赖 awiki-cli transportcfg::Profile。
```

---

## 11. PR 2C：profile get 真迁移

### 11.1 目标

让 `client.identity().profile()` 支持读取当前身份资料和公开资料。

### 11.2 范围

支持：

```text
self profile：authenticated did.profile.get_me
public profile by DID：did.profile.get_public_profile
public profile by handle：handle lookup -> public profile
```

暂不支持：

```text
profile markdown-file
profile update
本地 profile cache 强一致
```

### 11.3 调用链

```text
IdentityService::profile(query)
  -> if self: ensure_session(AuthScope::UserProfile)
  -> build get_me / public_profile call
  -> transport RPC
  -> map raw profile to Profile DTO
  -> return ProfileResult
```

### 11.4 awiki-cli 接入

```text
crates/awiki-cli/src/im_core_adapter/identity.rs
crates/awiki-cli/src/app/id_* 或 profile handler 当前接入点
```

CLI 仍保留：

```text
输出 JSON envelope
summary/warnings 文案
--self / --handle / --did 参数解析
legacy fallback
```

### 11.5 Required 验收

```bash
cargo test -p im-core profile
cargo test -p awiki-cli --test identity_contract
```

### 11.6 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_live_contract identity_profile_get
```

### 11.7 完成标准

```text
1. self/public profile 可走 im-core。
2. handle -> did -> public profile 顺序与 legacy 行为一致。
3. public profile lookup 的非致命 warning 行为不回退。
```

---

## 12. PR 2D：profile update 真迁移

### 12.1 目标

让 `client.identity().update_profile()` 支持更新当前身份资料。

### 12.2 范围

支持：

```text
display_name
bio
tags
markdown
changed_fields
远端 update_me
本地 display_name / profile projection best-effort 更新
```

CLI 保留：

```text
--markdown-file 读取
inline markdown 与 markdown-file 冲突检查
空字段错误文案
输出渲染
```

### 12.3 调用链

```text
IdentityService::update_profile(request)
  -> validate non-empty update
  -> ensure_session(AuthScope::UserProfile)
  -> build update_me payload
  -> authenticated RPC
  -> map changed_fields
  -> best-effort local identity/contact projection update
  -> return UpdateProfileResult
```

### 12.4 Required 验收

```bash
cargo test -p im-core profile
cargo test -p awiki-cli --test identity_contract
```

### 12.5 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_live_contract identity_profile_set
cargo test -p awiki-cli --test identity_profile_set_upgrade_contract
```

### 12.6 完成标准

```text
1. profile update 可走 im-core。
2. markdown-file 的原始内容保留语义不变。
3. changed_fields 与 legacy 输出一致。
4. 空更新不会发远端请求。
```

---

## 13. PR 2E：directory resolve / handle lookup 真迁移

### 13.1 目标

让 `client.directory().resolve_peer()` 和 `lookup_handle()` 支持 handle/DID 解析。

### 13.2 范围

支持：

```text
handle -> DID lookup
DID -> handle lookup
DID profile resolve
public profile lookup
非致命 public profile warning
```

暂不支持：

```text
全局目录搜索
推荐联系人
关系图谱
复杂缓存策略
```

### 13.3 调用链

```text
DirectoryService::resolve_peer(peer)
  -> normalize peer
  -> handle lookup or DID resolve
  -> optional public profile lookup
  -> map DirectoryResolution
  -> best-effort contact projection save
```

### 13.4 Required 验收

```bash
cargo test -p im-core directory
cargo test -p awiki-cli --test identity_contract
```

### 13.5 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_live_contract identity_resolve
```

### 13.6 完成标准

```text
1. resolve by handle 和 resolve by DID 可走 im-core。
2. lookup/profile/resolve 的请求顺序与 legacy 行为一致。
3. profile lookup 失败时保留 warning，不把整个 resolve 变成失败。
```

---

## 14. PR 2F：directory contacts save/list + relation status + profile projection

### 14.1 目标

把 contacts 本地存取和 profile projection 建立在 `im-core` 边界内，但 public API 仍在 `DirectoryService` 下。

### 14.2 源和目标

源：

```text
crates/awiki-cli/src/store/contacts.rs
```

目标：

```text
crates/im-core/src/directory/service.rs
crates/im-core/src/internal/contact_store/records.rs
crates/im-core/src/internal/contact_store/projection.rs
crates/im-core/src/compat/directory.rs
```

### 14.3 范围

支持：

```text
save contact by DID/handle/profile
list contacts
get contact by DID
get current contact by handle
contact handle binding history
relation status projection
profile -> contact projection
```

暂不迁移：

```text
relationship_events 完整业务流
推荐联系人算法
group member -> contact 自动同步深度逻辑
owner_identity_id schema 切换
```

### 14.4 执行方式

```text
1. im-core 定义 ContactRecord，不直接使用 store::ContactRecord。
2. awiki-cli store/contacts.rs 保留 wrapper 或 legacy 路径。
3. im-core internal contact_store 使用 rusqlite optional feature。
4. 查询条件仍先使用 owner_did，owner_identity_id 留到 Phase 3D。
5. directory/profile 成功后 best-effort 写 contact projection。
```

### 14.5 Required 验收

```bash
cargo test -p im-core contacts
cargo test -p awiki-cli --test store_contact_contract
```

### 14.6 Optional integration

```bash
cargo test -p awiki-cli --test msg_contract
```

### 14.7 Manual / live / system

```bash
cargo test -p awiki-cli --test msg_live_contract
cargo test -p awiki-cli --test msg_ws_inbox_live_contract
```

### 14.8 完成标准

```text
1. contacts save/list 能通过 im-core local store。
2. legacy contact tests 继续通过。
3. relation status 不依赖 CLI output 类型。
4. public API 没有新增 client.contacts()，除非同步更新 public-api.md。
```

---

## 15. PR 2G：bind phone/email 迁移到 identity service

### 15.1 目标

把当前身份的 phone/email bind 流程迁入 `client.identity()`。

### 15.2 范围

支持：

```text
phone bind send OTP
phone bind verify OTP
email bind send
email bind status
email bind wait loop 的核心状态判断
```

CLI 保留：

```text
--wait 参数解析
poll interval / timeout 的 CLI 默认值和提示
输出渲染
```

### 15.3 调用链

```text
IdentityService::bind_contact(request)
  -> ensure_session(AuthScope::UserProfile)
  -> choose phone/email flow
  -> REST call or status poll
  -> map BindResult
  -> best-effort identity/contact projection update
```

### 15.4 Required 验收

```bash
cargo test -p im-core identity_bind
cargo test -p awiki-cli --test identity_contract
```

### 15.5 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_live_contract identity_bind
```

### 15.6 完成标准

```text
1. bind phone/email 可走 im-core。
2. phone/email normalize 行为与 legacy 一致。
3. wait pending/completed/sent 三种状态映射不回退。
```

---

## 16. PR 2H：recover handle 迁移

### 16.1 目标

把 handle recovery 的计划、远端调用和本地 merge 边界迁入 `im-core`。

### 16.2 范围

支持：

```text
recover handle plan
send recover OTP
recover_handle RPC
recovered identity summary
local state recover merge 的 SDK 调用边界
```

CLI 保留：

```text
OTP 参数解析
备份路径展示
恢复前确认文案
输出渲染
```

### 16.3 执行方式

```text
1. 先迁 recovery wire builder 和 result DTO。
2. im-core 暴露 RecoverHandleRequest / RecoverHandleResult。
3. 本地 merge 初期可通过 compat 调 legacy recover_merge wrapper。
4. 后续将 recover_merge 的纯 record/sql helper 文件级迁入 im-core。
```

### 16.4 Required 验收

```bash
cargo test -p im-core identity_recovery
cargo test -p awiki-cli --test store_recover_merge_contract
cargo test -p awiki-cli --test identity_contract
```

### 16.5 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_recover_live_contract
```

### 16.6 完成标准

```text
1. recover handle 可走 im-core。
2. 旧 recover dry-run 和 live 输出不变。
3. local merge 失败时错误边界清晰，不产生半迁移状态。
```

---

## 17. PR 2I：replace DID plan

### 17.1 目标

只实现 `replace DID` 的计划能力，不执行远端 replace，不做本地 rebind。

这是 Phase 2-late / Phase 2.5 能力，不应与普通 profile/directory PR 混做。

### 17.2 必须返回的 SDK 信息

```text
risk summary
backup plan
local rebind plan
affected local state
remote replace DID call preview
rollback notes
```

### 17.3 范围

支持：

```text
replace DID plan
affected table counts
backup manifest preview DTO
remote replace DID call preview
local rebind dry-run
```

CLI 保留：

```text
危险确认
备份目录选择和权限提示
dry-run 输出渲染
用户可读 warning 文案
```

### 17.4 Required 验收

```bash
cargo test -p im-core replace_did_plan
cargo test -p awiki-cli --test store_rebind_contract
```

### 17.5 完成标准

```text
1. replace DID plan 信息完整。
2. 执行前必须有 backup plan。
3. 不触发远端 replace。
4. 不触发本地 destructive rebind。
```

---

## 18. PR 2J：replace DID execution + local rebind

### 18.1 目标

在 `ReplaceDidPlan` 稳定后，单独迁移执行能力。

### 18.2 范围

支持：

```text
remote did-auth.replace_did call
local identity record update
local store owner rebind 调用边界
backup manifest 写入
```

### 18.3 执行方式

```text
1. 执行前必须校验 backup plan。
2. 备份失败不得继续远端 replace。
3. 远端成功、本地失败时返回可人工恢复信息。
4. 本地 rebind 必须可 dry-run。
```

### 18.4 Required 验收

```bash
cargo test -p im-core replace_did_execution
cargo test -p awiki-cli --test store_rebind_contract
```

### 18.5 Manual / live / system

```bash
cargo test -p awiki-cli --test identity_replace_did_live_contract
cargo test -p awiki-cli --test identity_replace_did_upgrade_contract
```

### 18.6 完成标准

```text
1. 远端成功、本地失败、备份失败三类错误有明确边界。
2. CLI 危险确认仍留在 awiki-cli。
3. backup manifest 可用于人工恢复。
```

---

## 19. Phase 3 关键 DTO

Phase 3 建议补齐这些 SDK DTO：

```rust
pub struct MarkReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<crate::ids::MessageId>,
    pub warnings: Vec<String>,
}

pub struct Conversation {
    pub thread: crate::messages::ThreadRef,
    pub title: Option<String>,
    pub participants: Vec<crate::ids::PeerRef>,
    pub last_message: Option<crate::messages::Message>,
    pub unread_count: u32,
    pub message_count: u32,
    pub last_message_at: Option<String>,
}

pub struct ConversationQuery {
    pub limit: crate::ids::PageLimit,
    pub include_groups: bool,
    pub include_direct: bool,
    pub unread_only: bool,
}

pub struct Group {
    pub id: Option<String>,
    pub did: crate::ids::GroupRef,
    pub name: Option<String>,
    pub description: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

pub struct GroupMember {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
}

pub struct GroupMutationResult {
    pub group: Group,
    pub delivery_state: Option<String>,
    pub warnings: Vec<String>,
}
```

`mark_read` public API 与 `public-api.md` 保持一致：

```rust
pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
```

如果后续要引入 `MarkReadRequest`，必须先同步更新 `public-api.md`。

---

## 20. PR 3A：local_state 边界和 schema adapter scaffold

### 20.1 目标

在 `im-core` 建立 local_state 访问边界，但不立即重写全部 store SQL。

### 20.2 改动范围

```text
crates/im-core/src/internal/local_state/mod.rs
crates/im-core/src/internal/local_state/schema.rs
crates/im-core/src/internal/local_state/messages.rs
crates/im-core/src/internal/local_state/contacts.rs
crates/im-core/src/internal/local_state/groups.rs
crates/im-core/src/internal/local_state/conversations.rs
crates/im-core/src/compat/local_state.rs
```

### 20.3 执行步骤

```text
1. 定义 internal LocalStateStore trait。
2. 定义 MessageRecord / ContactRecord / GroupRecord / GroupMemberRecord 的 im-core 版本。
3. 先接 rusqlite optional feature。
4. awiki-cli store 模块保留 wrapper。
5. 不改变现有 schema。
6. 添加 schema boundary tests。
```

`LocalStateStore` 是 internal store boundary，不是 public provider trait，不进入 prelude，不允许 App 在 Phase 3 接管它。Phase 7 再决定是否演进成外部 provider。

### 20.4 Required 验收

```bash
cargo test -p im-core local_state
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test store_groups_contract
cargo test -p awiki-cli --test store_contact_contract
```

### 20.5 完成标准

```text
1. im-core 有 local_state 内部边界。
2. 不改变旧 schema 行为。
3. store 旧测试继续通过。
```

---

## 21. PR 3B：mark_read 真迁移

### 21.1 目标

把 `client.messages().mark_read()` 接到真实远端调用和本地状态更新。

### 21.2 源和目标

源：

```text
crates/awiki-cli/src/message/mark_read.rs
crates/awiki-cli/src/message/wire.rs
crates/awiki-cli/src/store/messages.rs
```

目标：

```text
crates/im-core/src/messages/mark_read.rs
crates/im-core/src/internal/wire/inbox.rs
crates/im-core/src/internal/local_state/messages.rs
```

### 21.3 范围

支持：

```text
direct message mark_read remote call
group/local-only message local mark_read
local mail notification local-only mark_read
websocket -> http fallback 语义，若 P1 已有 transport trait
unauthorized fallback refresh
```

暂不迁移：

```text
secure incoming decrypt
复杂 notification consume
runtime listener bridge service internals
```

### 21.4 Required 验收

```bash
cargo test -p im-core mark_read
cargo test -p awiki-cli --test msg_contract
```

### 21.5 Optional integration

```bash
cargo test -p awiki-cli --test msg_ws_mark_read_live_contract
cargo test -p awiki-cli --test runtime_listener_bridge_connection_contract
```

### 21.6 完成标准

```text
1. mark_read 可走 im-core。
2. direct remote + local update 行为与 legacy 一致。
3. local-only 消息不误发远端 mark_read。
```

---

## 22. PR 3C：conversation projection

### 22.1 目标

实现 `client.messages().conversations()`。

### 22.2 范围

支持：

```text
direct conversation projection
group conversation projection
unread count
last message
last_message_at
message_count
limit
```

暂不支持：

```text
全文搜索
复杂置顶/归档
多设备 reconcile
attachment preview
secure message preview 解密
```

### 22.3 执行方式

```text
1. 复用 messages 表和 threads view 的语义。
2. im-core 内定义 Conversation DTO。
3. local_state/conversations.rs 做 projection。
4. awiki-cli 如已有 debug 或 inbox 相关展示，保留原输出。
```

### 22.4 Required 验收

```bash
cargo test -p im-core conversations
cargo test -p awiki-cli --test store_messages_contract
```

### 22.5 Optional integration

```bash
cargo test -p awiki-cli --test msg_contract
```

### 22.6 完成标准

```text
1. conversation projection 不暴露 SQLite row。
2. direct/group thread 区分稳定。
3. unread count 与旧 local view 语义一致。
```

---

## 23. PR 3D：owner_identity_id 兼容迁移

### 23.1 目标

在不破坏现有 `owner_did` schema 的前提下，增加 `owner_identity_id` 作为本地隔离键。

### 23.2 迁移原则

```text
1. 新增 owner_identity_id nullable column。
2. 写入新数据时同时写 owner_identity_id + owner_did。
3. 查询优先 owner_identity_id，缺失时 fallback owner_did。
4. 后续 schema version 再考虑强约束和主键重建。
```

### 23.3 涉及表

Phase 3D 覆盖：

```text
contacts
contact_handle_bindings
messages
groups
group_members
relationship_events
```

Phase 3D 不覆盖写路径迁移：

```text
e2ee_outbox
e2ee_sessions
secure direct state
MLS/group E2EE state
```

E2EE 相关表可只做兼容读取评估，真正 owner_identity_id 迁移放 Phase 6 secure。

### 23.4 查询规则

兼容期查询：

```text
WHERE owner_identity_id = :identity_id
   OR (owner_identity_id IS NULL AND owner_did = :owner_did)
```

写入规则：

```text
owner_identity_id = current identity id
owner_did = current DID
credential_name = legacy identity alias, if available
```

回填规则：

```text
1. 通过 identity registry 建立 did -> identity_id 映射。
2. 优先 credential_name 匹配。
3. 其次 owner_did 匹配。
4. 无法匹配的行保持 owner_identity_id NULL。
```

### 23.5 Required 验收

```bash
cargo test -p im-core local_state_owner
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test store_groups_contract
cargo test -p awiki-cli --test store_contact_contract
```

### 23.6 Optional integration

```bash
cargo test -p awiki-cli --test identity_replace_did_live_contract
```

### 23.7 完成标准

```text
1. 新旧数据都可读。
2. 新写入数据双写 owner_identity_id + owner_did。
3. replace DID 后本地隔离不再只依赖旧 DID。
4. 不重建主键，不做破坏性 schema 改造。
5. E2EE 表未被提前迁移。
```

---

## 24. PR 3E：message send state / retry / cache merge

### 24.1 目标

补齐普通 message 的发送状态、失败状态和基础重试边界。

### 24.2 范围

支持：

```text
outgoing message send state
accepted/sent/stored_locally/failed
operation_id/message_id 关联
失败原因保存
基础 retry plan
remote result 与 local cache merge
```

暂不支持：

```text
secure direct retry
attachment retry
group E2EE retry
后台 realtime 自动重试
```

### 24.3 执行方式

```text
1. 扩展 MessageMetadata / MessageSendState DTO。
2. local_state/messages.rs 支持 send_state 字段投影，先写入 metadata 或兼容字段。
3. send direct/group 成功路径写 accepted/sent。
4. 失败路径可写 failed，是否写入由 request policy 控制。
5. retry 只生成 plan，不启动 background runner。
```

### 24.4 Required 验收

```bash
cargo test -p im-core message_state
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test msg_contract
```

### 24.5 Manual / live / system

```bash
cargo test -p awiki-cli --test msg_live_contract
cargo test -p awiki-cli --test group_live_contract
```

### 24.6 完成标准

```text
1. send state 可从 SDK Message metadata 中读取。
2. remote/local merge 不重复消息。
3. failed 状态不会破坏 legacy inbox/history 展示。
```

---

## 25. PR 3F：group get/list/members/messages 读路径迁移

### 25.1 目标

把 group 读路径迁入 `client.groups()`。

### 25.2 范围

支持：

```text
group get
group list
group members
group messages
cached group snapshot
cached group members
cached group messages
remote result best-effort persist
```

暂不支持：

```text
group create/join/leave
group add/remove/update
group E2EE
MLS state
```

### 25.3 源和目标

源：

```text
crates/awiki-cli/src/message/group_service.rs
crates/awiki-cli/src/message/group_wire.rs
crates/awiki-cli/src/store/groups.rs
```

目标：

```text
crates/im-core/src/groups/service.rs
crates/im-core/src/internal/group_runtime/read.rs
crates/im-core/src/internal/wire/group.rs
crates/im-core/src/internal/local_state/groups.rs
```

### 25.4 Required 验收

```bash
cargo test -p im-core groups
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test store_groups_contract
```

### 25.5 Optional integration

```bash
cargo test -p awiki-cli --test msg_ws_group_live_contract
```

### 25.6 Manual / live / system

```bash
cargo test -p awiki-cli --test group_live_contract
```

### 25.7 完成标准

```text
1. group read APIs 可走 im-core。
2. local cache fallback 行为与 legacy 一致。
3. group E2EE 路径未被误动。
```

---

## 26. PR 3G：group create/join/leave 迁移

### 26.1 目标

把 group create/join/leave 普通路径迁入 `client.groups()`。

### 26.2 范围

支持：

```text
group create
group join
group leave
group policy/profile payload
remote result persist snapshot
leave 后 mark cached group left
```

暂不支持：

```text
group add/remove/update
group E2EE create/join/leave
MLS welcome/commit
```

### 26.3 执行方式

```text
1. 迁移 group lifecycle wire builder 的普通路径。
2. im-core 定义 GroupCreateRequest / GroupJoinRequest / GroupLeaveRequest。
3. CLI 仍保留 flag parsing 和输出。
4. E2EE 检测仍由 legacy 路径处理，或者 im-core 返回 UnsupportedCapability。
5. 普通路径成功后 best-effort persist group snapshot。
```

### 26.4 Required 验收

```bash
cargo test -p im-core group_lifecycle
cargo test -p awiki-cli --test group_contract
```

### 26.5 Optional integration

```bash
cargo test -p awiki-cli --test group_e2ee_create_contract
cargo test -p awiki-cli --test group_e2ee_remove_leave_contract
```

### 26.6 Manual / live / system

```bash
cargo test -p awiki-cli --test group_live_contract
```

### 26.7 完成标准

```text
1. 普通 group create/join/leave 可走 im-core。
2. E2EE group lifecycle 仍走 legacy 或明确 unsupported。
3. group owner cannot leave 等本地保护不回退。
```

---

## 27. PR 3H：group add/remove/update/members 写路径迁移

### 27.1 目标

迁移 group 成员和 profile/policy mutation 普通路径。

### 27.2 范围

支持：

```text
group add member
group remove member
group update profile
group update policy
members refresh
mutation 后 sync group state
```

暂不支持：

```text
group E2EE member add/remove
MLS key update
hidden group.e2ee.* RPC
```

### 27.3 执行方式

```text
1. 普通 group mutation wire builder 迁到 im-core。
2. member handle 解析走 DirectoryService。
3. mutation 成功后调用 group get/members/messages 读路径做 best-effort refresh。
4. 如果 group snapshot 显示 E2EE，返回 UnsupportedCapability 或交给 legacy fallback。
```

### 27.4 Required 验收

```bash
cargo test -p im-core group_mutation
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test message_group_wire_contract
```

### 27.5 Optional integration

```bash
cargo test -p awiki-cli --test group_e2ee_add_contract
cargo test -p awiki-cli --test group_e2ee_remove_leave_contract
```

### 27.6 Manual / live / system

```bash
cargo test -p awiki-cli --test group_live_contract
```

### 27.7 完成标准

```text
1. 普通 group add/remove/update/members 可走 im-core。
2. E2EE mutation 未被普通路径误处理。
3. mutation 后本地 group/member cache 可被刷新。
```

---

## 28. PR 3I：Phase 3 legacy wrapper 收敛和 compat 清理

### 28.1 目标

在 Phase 3 能力稳定后，收敛 P1/P2/P3 中已经不再需要的 wrapper。

### 28.2 清理范围

可清理：

```text
只被 awiki-cli compat 调用、且已有 im-core public API 替代的 wrapper
重复的 wire builder 旧测试
临时 feature flag 分支
临时 legacy bridge request
```

不可清理：

```text
attachment legacy path
realtime listener path
secure direct path
group E2EE path
CLI 输出和参数解析
```

### 28.3 Required 验收

```bash
cargo test -p im-core
cargo test -p awiki-cli
rg "im_core::compat" crates/awiki-cli/src
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

### 28.4 完成标准

```text
1. 已稳定能力优先通过 im-core public API 调用。
2. compat 只保留 Phase 4/5/6 前仍需要的边界。
3. legacy fallback 清理有明确记录。
```

---

## 29. 回滚策略

每个切片都按这个顺序设计：

```text
1. im-core 新实现先落地。
2. awiki-cli wrapper 再切过去。
3. feature flag / adapter fallback 保留一个阶段。
4. 出问题时只回滚 wrapper 调用点，im-core 新代码可以暂时保留但不走生产路径。
5. compat API 稳定两个阶段后再清理 legacy wrapper。
```

涉及 schema 的回滚策略：

```text
1. 只做 additive schema change。
2. 新 column 允许 NULL。
3. 不立刻重建 primary key。
4. 不删除 owner_did。
5. 查询保留 owner_did fallback。
6. 回滚时旧代码仍可忽略新增 column。
```

涉及 replace DID / recover handle 的回滚策略：

```text
1. 执行前必须有 backup manifest。
2. 远端成功、本地失败时必须返回可人工恢复信息。
3. 本地 rebind 必须可 dry-run。
4. 备份失败时不得继续远端 replace。
```

---

## 30. 明确不做事项

Phase 2 不做：

```text
1. 不迁 CLI markdown-file 读取。
2. 不迁 CLI 输出渲染。
3. 不迁 CLI 危险确认。
4. 不迁 message conversation projection。
5. 不迁 group lifecycle。
6. 不迁 owner_identity_id schema 迁移。
7. 不新增默认 public client.contacts()，除非同步更新 public-api。
```

Phase 3 不做：

```text
1. 不迁 attachment send/download。
2. 不迁 realtime runner / listener service-run。
3. 不迁 secure direct E2EE。
4. 不迁 group E2EE / MLS。
5. 不迁 platform service 管理。
6. 不迁 OpenClaw / Hermes host notify。
7. 不做破坏性 SQLite schema 重建。
8. 不迁 e2ee_outbox / e2ee_sessions 写路径。
```

---

## 31. 方案核心

Phase 2 的核心：

```text
先把 identity/profile/directory 的 wire 和 DTO 边界迁到 im-core，
再接 profile、directory、contacts、bind、recover、replace DID 的真实执行。
```

Phase 3 的核心：

```text
先建立 local_state 边界和兼容 schema，
再迁 mark_read、conversation projection、owner_identity_id、group read/write 和 message state。
```

这套方案避免函数级碎片迁移，也避免把 `identity`、`message`、`store`、`group` 一次性整体搬入 `im-core` 导致大面积重写。
