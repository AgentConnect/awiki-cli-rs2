# im-core 对外接口梳理与收口建议

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用范围**：

```text
crates/im-core                 # Rust SDK public surface
crates/im-core-dart            # Flutter Rust Bridge API / DTO / mapping surface
packages/awiki_im_core         # Dart/Flutter package public facade
```

**目标**：整理当前 `im-core` 对外核心接口，并同步检查 Dart/Flutter 封装层是否把同一批 API 问题固化成跨语言 SDK public API。后续收口必须同时覆盖 Rust crate、FRB bridge crate、Dart package facade，避免 SDK public API 被 wire、DB、runtime、diagnostic、compat、CLI/service routing 细节污染。

**文档状态（2026-05-23）**：本文件已从“问题评审”更新为“问题评审 + 实施跟踪 + 合并前验收清单”。原始问题仍然是当前 public API 收口问题，但 realtime、prelude、compat、groups DTO、diagnostic raw、Phase 4A attachment skeleton、FRB/Dart facade 同步这几块已经在当前工作区推进实现；合并前仍需要补齐受环境阻塞的 im-core-dart、Dart/Flutter、FRB codegen 验收。

**当前方案边界**：

```text
1. 本文继续作为 public API 收口方案，但不再把已经完成的 realtime/prelude/groups/diagnostic_raw 改动描述成未开始任务。
2. Rust crate、FRB bridge crate、Dart package facade 必须同步收口；只改 Rust 而不改 Dart facade 不算完成。
3. 当前工作区已经完成第一阶段 API 收口和 Phase 4A attachment skeleton，但没有完成完整附件 runtime。
4. 当前 generated FRB 文件是手动同步状态；最终合并必须由 flutter_rust_bridge_codegen 重新生成确认。
5. im-core-dart、Dart/Flutter 验收受当前机器依赖源和工具链限制阻塞，不能视为已验证。
```

---

## 1. 总体结论

当前 `im-core` 的主链路已经基本形成高层 service API：

```text
ImCore
  -> identities()
  -> bootstrap()
  -> client(selector)

ImClient
  -> auth()
  -> identity()
  -> directory()
  -> messages()
  -> groups()
  -> realtime()
```

这条主链路符合既定边界：

```text
调用方选择身份
im-core 执行业务能力
CLI/App 负责 config/path/output/service host
```

原 review 的核心判断仍然成立：主 service 设计方向正确，需要收紧的是 public exports / prelude / compat / 少数 DTO，而不是重做 ImCore/ImClient 架构。现在需要把“当前问题”拆成两类：已在当前工作区处理的收口项，以及仍需继续挑战或验证的收口项。

已在当前工作区推进处理：

```text
1. realtime/mod.rs 默认导出已收紧，runner / heartbeat / reconnect / session_loop / transport helper 改走 compat::realtime 或内部路径。
2. compat 已补充 migration-only 定位说明，不进 prelude，不作为稳定 SDK facade 设计目标。
3. prelude 已移出 ReplaceDid*、GroupReadResult、ContactBindingResult、RecoverGeneratedIdentity、retry/send-state 等 advanced 或 diagnostic 倾向类型。
4. groups DTO 已移除普通 create request 中的 service_did，并新增 GroupDiscoverability / GroupAdmissionMode / GroupMessageSecurityProfile / GroupMemberRole / GroupMemberLimit 等领域类型。
5. crates/im-core-dart 与 packages/awiki_im_core 已同步移除 CreateGroupRequest.serviceDid，并把 Dart facade 的 group policy 字段改成 value object，而不是继续暴露裸 String。
6. GroupReadResult / ContactBindingResult / RecoverHandleResult 的 diagnostic_raw 默认 public method 已移出；awiki-cli 迁移期 raw response 访问改走 compat 访问器。
7. group create 对 e2ee=true 或 GroupMessageSecurityProfile::GroupE2ee 已明确返回 UnsupportedCapability("group-e2ee")，避免 Phase 6 前暴露半成品 group e2ee 创建能力。
8. Phase 4A attachment DTO / service skeleton 已落地：新增 attachments module、AttachmentService、canonical AttachmentInput / AttachmentDestination / send/download DTO，MessageBody::Attachment 已复用 attachments::AttachmentInput。
```

仍需继续处理或验证：

```text
1. AttachmentService::send/download 仍是 UnsupportedCapability skeleton；真实 manifest/digest/selection/upload/download 需要按 Phase 4B+ 继续迁移，不能把当前 Phase 4A 误读为附件能力已完成。
2. group e2ee: bool 与 message_security_profile 长期仍应合并到一个安全策略模型；当前只是先明确 unsupported，避免 Phase 6 前把半成品能力暴露成稳定 API。
3. im-core-dart 当前受 Cargo registry/mirror 依赖下载问题阻塞，尚未完成 cargo check/test。
4. 当前机器未安装 dart、flutter、flutter_rust_bridge_codegen，Dart facade 测试和 FRB 重新生成尚未完成。
```

结论：

```text
主 service 设计方向正确，本方案仍然是当前问题。
现在最大的挑战不是 Rust 单层 API，而是 Rust public DTO、FRB DTO/mapping、Dart facade 三层必须同步收口。
已经存在 Dart 封装层后，不能只靠 crates/im-core-dart 内部 mapping 兜底；packages/awiki_im_core 本身就是对 Flutter 调用方承诺的 public API。
```

---

## 2. 当前核心入口

### 2.1 Rust SDK crate

入口文件：

```text
crates/im-core/src/lib.rs
```

当前顶层 module：

```rust
pub mod auth;
pub mod config;
pub mod core;
pub mod directory;
pub mod error;
pub mod groups;
pub mod identity;
pub mod ids;
pub mod messages;
pub mod paths;
pub mod prelude;
pub mod realtime;

#[doc(hidden)]
pub mod compat;

mod internal;
```

当前顶层 re-export：

```rust
pub use self::config::{ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};
pub use crate::core::{CoreBootstrap, ImClient, ImCore};
pub use crate::directory::{DirectoryService, HandleLookupResult};
pub use crate::error::{ImError, ImResult};
pub use crate::groups::GroupService;
pub use crate::identity::{IdentitySelector, IdentitySummary};
pub use crate::paths::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};
pub use crate::realtime::RealtimeService;
```

判断：

```text
1. 顶层 re-export 当前是偏克制的，基本都是高层入口。
2. `pub mod compat` 是最大迁移期暴露面，虽然 doc hidden，但仍可被外部依赖。
3. `pub mod realtime` 内部 re-export 过宽，会把底层 helper 暴露到默认 public module。
```

### 2.2 Dart / Flutter 封装层

当前已经新增跨语言封装：

```text
crates/im-core-dart
packages/awiki_im_core
```

当前 Dart package public export：

```dart
library awiki_im_core;

export 'src/awiki_im_core_base.dart';
export 'src/models/auth.dart';
export 'src/models/config.dart';
export 'src/models/directory.dart';
export 'src/models/error.dart';
export 'src/models/group.dart';
export 'src/models/identity.dart';
export 'src/models/message.dart';
export 'src/models/profile.dart';
export 'src/models/realtime.dart';
```

Dart facade 当前主链路：

```text
AwikiImCore.open(config, paths)
  -> client(selector)
  -> listIdentities/defaultIdentity/resolveIdentity/registerHandle/recoverHandle

AwikiImClient
  -> auth
  -> identity
  -> directory
  -> profile
  -> messages
  -> groups
  -> realtime
```

当前判断：

```text
1. Dart facade 的服务划分整体跟 Rust ImCore/ImClient 方向一致。
2. Dart facade 没有暴露 Rust compat/raw/diagnostic_raw，方向比 Rust public surface 更克制。
3. 原先 Dart facade 复制了 group DTO 问题，例如 serviceDid、discoverability/admissionMode/messageSecurityProfile/maxMembers stringly typed；当前工作区已同步移除 serviceDid，并改成 GroupDiscoverability / GroupAdmissionMode / GroupMessageSecurityProfile / GroupMemberLimit value object。
4. im-core-dart 不应维护 default_service_did 状态缓存；group service DID 应来自 ImCoreConfig.anpServiceDid / runtime config，并由 Rust runtime 注入请求 payload。
5. Dart realtime 当前只暴露 capability/status/connect stub，不暴露 runner；Rust 收紧 realtime helper 不应被 Dart 阻塞。
6. 当前生成文件是手动同步调整，因为本机缺少 flutter_rust_bridge_codegen；合并前需要在具备工具链的环境重新生成并确认无漂移。
```

后续原则：

```text
任何 Rust public DTO/API 收口都必须同步检查：

1. crates/im-core-dart/src/api/*
2. crates/im-core-dart/src/dto/*
3. crates/im-core-dart/src/mapping/*
4. packages/awiki_im_core/lib/src/models/*
5. packages/awiki_im_core/lib/src/awiki_im_core_native.dart
6. packages/awiki_im_core/lib/awiki_im_core.dart

如果 Rust 只改内部类型而 Dart facade 继续暴露旧字段，则本轮 public API 收口不算完成。
```

---

## 3. Core / Client 接口

当前核心 API：

```rust
impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;
    pub fn identities(&self) -> IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;
    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn did(&self) -> &Did;
    pub fn handle(&self) -> Option<&Handle>;
    pub fn auth(&self) -> AuthService<'_>;
    pub fn identity(&self) -> IdentityService<'_>;
    pub fn directory(&self) -> DirectoryService<'_>;
    pub fn messages(&self) -> MessageService<'_>;
    pub fn groups(&self) -> GroupService<'_>;
    pub fn realtime(&self) -> RealtimeService<'_>;
}
```

判断：

```text
高层。这里是当前 im-core 最重要、也最正确的 public shape。
```

保留策略：

```text
1. ImCore 继续作为环境级入口。
2. ImClient 继续作为绑定身份后的业务入口。
3. 不把 ActorContext、LoadedIdentity、auth path、SQLite path、RPC transport 暴露给调用方。
4. App/CLI 通过 ImCorePaths 显式注入路径，im-core 不自行发现 CLI workspace。
```

---

## 4. Bootstrap 接口

当前 API：

```rust
impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

判断：

```text
高层 lifecycle API。不是 IM 消息业务，但属于 SDK 初始化和迁移生命周期。
```

保留策略：

```text
1. 可以继续暴露。
2. 不暴露 raw rusqlite::Connection。
3. 不暴露 schema SQL、migration SQL、owner backfill helper。
4. CLI/App 可以用这些接口做启动前检查和迁移。
```

---

## 5. Identity / Auth 接口

### 5.1 IdentityRegistry

当前 API：

```rust
impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;
    pub fn register_handle(&self, request: RegisterHandleRequest) -> ImResult<HandleRegistrationResult>;
    pub fn recover_handle(&self, request: RecoverHandleRequest) -> ImResult<RecoverHandleResult>;
    pub fn plan_default_identity_change(&self, selector: IdentitySelector) -> ImResult<DefaultIdentityChange>;
}
```

判断：

```text
高层。
```

保留策略：

```text
1. list/default/resolve/register/recover 都是产品级身份能力。
2. plan_default_identity_change 保持 plan 语义，CLI/App 决定是否写 default identity 文件。
3. 不暴露 registry 文件格式、credential dir 名称、auth path、private key path。
```

### 5.2 IdentityService

当前 API：

```rust
impl IdentityService<'_> {
    pub fn profile(&self) -> ImResult<Profile>;
    pub fn update_profile(&self, patch: ProfilePatch) -> ImResult<Profile>;
    pub fn bind_contact(&self, request: ContactBindingRequest) -> ImResult<ContactBindingResult>;
    pub fn bind_email_status(&self, email: String) -> ImResult<ContactBindingResult>;
    pub fn replace_did_plan(&self, request: ReplaceDidPlanRequest) -> ImResult<ReplaceDidPlan>;
}
```

判断：

```text
profile/update/bind 是高层。
bind_email_status 稍偏流程细节，但仍可接受。
replace_did_plan 是高级危险能力，不应作为普通默认 API 使用。
```

建议：

```text
1. `replace_did_plan` 保留为 advanced API，CLI 默认 hidden。
2. `ReplaceDid*` DTO 不建议进入 prelude。
3. 若后续提供 execution API，也应放 advanced/diagnostic feature 或清晰命名。
4. `bind_email_status(email: String)` 可考虑改成 `ContactBindingStatusRequest`，避免裸 String 扩展性差。
```

### 5.3 AuthService

当前 API：

```rust
impl AuthService<'_> {
    pub fn login(&self) -> ImResult<SessionBundle>;
    pub fn ensure_session(&self, scope: AuthScope) -> ImResult<SessionBundle>;
    pub fn refresh_session(&self) -> ImResult<SessionUpdate>;
    pub fn status(&self) -> ImResult<AuthStatus>;
}
```

判断：

```text
高层。
```

保留策略：

```text
1. 不暴露 DID auth proof builder。
2. 不暴露 JWT file format。
3. 不要求调用方传 auth path。
4. AuthScope 是合理领域枚举。
```

---

## 6. Directory 接口

当前 API：

```rust
impl DirectoryService<'_> {
    pub fn resolve_peer(&self, peer: PeerRef) -> ImResult<DirectoryResolution>;
    pub fn lookup_handle(&self, handle: Handle) -> ImResult<HandleLookupResult>;
    pub fn public_profile(&self, subject: IdentitySubject) -> ImResult<PublicProfile>;
    pub fn save_contact(&self, request: SaveContactRequest) -> ImResult<Contact>;
    pub fn contacts(&self, query: ContactListQuery) -> ImResult<Page<Contact>>;
    pub fn relation_status(&self, peer: PeerRef) -> ImResult<RelationStatus>;
    pub fn owner_did(&self) -> &Did;
}
```

判断：

```text
整体高层。
```

注意点：

```text
1. `owner_did()` 与 `client.did()` 重复，可以保留但价值不高。
2. `SaveContactRequest` 同时有 peer/did/handle，有一定冗余，但还属于产品 DTO。
3. 不暴露 contact_store record / SQLite helper，方向正确。
```

建议：

```text
1. 后续可评估移除 `owner_did()`，统一使用 `client.did()`。
2. Contact relation/follow 能力如果继续增强，应仍放在 DirectoryService 或未来独立高层 People/Contacts service，不暴露 contact_store。
```

---

## 7. Messages 接口

当前 API：

```rust
impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
    pub fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
    pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
    pub fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
}
```

当前关键 DTO：

```rust
pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<MessageId>,
    pub delivery: MessageDeliveryOptions,
}

pub enum MessageTarget {
    Direct(PeerRef),
    Group(GroupRef),
}

pub enum MessageBody {
    Text { text: String, kind: MessageKind },
    Attachment { input: AttachmentInput, caption: Option<String>, mime_type: Option<String> },
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    SecureDirect,
    GroupE2ee,
}
```

判断：

```text
主接口高层。
```

需要注意：

```text
1. MessageBody::Attachment 当前仍返回 unsupported；完整 send/download 需要 Phase 4B+ 后续迁移。
2. MessageBody::Attachment 已复用 attachments::AttachmentInput canonical DTO，不再维护独立的 messages::AttachmentInput reserved 类型。
3. MessageMetadata.content_type / attributes 是一定程度的实现 metadata 透出，但作为只读结果可接受。
4. MessageRetryPlan / MessageRetryAction 比 raw outbox 高层，可以先保留。
```

建议：

```text
1. Phase 4A 已把附件 DTO 收敛到 `attachments::AttachmentInput` canonical 类型。
2. Phase 6 前 `SecureDirect` / `GroupE2ee` 继续返回 UnsupportedCapability。
3. 不暴露 direct/group wire builder、RPC params、secure outbox table。
4. 如果 retry 后续成为产品能力，新增高层 `messages().retry(...)`，不要让调用方理解 old outbox internals。
5. Dart 当前只暴露 sendText；未来新增附件发送时必须直接对齐 canonical AttachmentInput，不要先复制 messages::AttachmentInput reserved 形态。
```

---

## 8. Groups 接口

当前 API：

```rust
impl GroupService<'_> {
    pub fn create(&self, request: GroupCreateRequest) -> ImResult<GroupReadResult>;
    pub fn join(&self, request: GroupJoinRequest) -> ImResult<GroupReadResult>;
    pub fn leave(&self, request: GroupLeaveRequest) -> ImResult<GroupReadResult>;
    pub fn add_member(&self, request: GroupMemberMutationRequest) -> ImResult<GroupReadResult>;
    pub fn remove_member(&self, request: GroupMemberMutationRequest) -> ImResult<GroupReadResult>;
    pub fn update_profile(&self, request: GroupUpdateProfileRequest) -> ImResult<GroupReadResult>;
    pub fn update_policy(&self, request: GroupUpdatePolicyRequest) -> ImResult<GroupReadResult>;
    pub fn get(&self, group: GroupRef) -> ImResult<GroupReadResult>;
    pub fn list(&self, request: GroupListRequest) -> ImResult<GroupReadResult>;
    pub fn members(&self, request: GroupMembersRequest) -> ImResult<GroupReadResult>;
    pub fn messages(&self, request: GroupMessagesRequest) -> ImResult<GroupReadResult>;
}
```

判断：

```text
Service 方法是高层。
DTO 原先有部分偏底层或 stringly typed；当前工作区已完成第一轮领域化。
```

原主要问题：

```text
1. GroupCreateRequest.service_did 不应由普通调用方每次传入。
2. message_security_profile 使用 Option<String>，应领域 enum。
3. discoverability / admission_mode / role / max_members 等使用 String，后续应领域化。
4. e2ee: bool 与 message_security_profile 重叠，Phase 6 前应 unsupported；长期应合并到安全策略 enum。
5. GroupReadResult::diagnostic_raw() 暴露 raw response，不应鼓励默认调用方依赖。
```

当前实施状态：

```text
1. Rust GroupCreateRequest 已移除 service_did。
2. Rust group create payload 改由 GroupLifecycleRuntime 从 ImCoreConfig.anp_service_did 读取服务 DID；缺失时返回 invalid input，字段指向 anp_service_did。
3. Rust 已新增 GroupDiscoverability / GroupAdmissionMode / GroupMessageSecurityProfile / GroupMemberRole / GroupMemberLimit，并用于 create/profile/policy/member mutation DTO。
4. compat::wire 仍保留迁移期 string wire request，但会解析成领域类型后进入内部 builder，不把 string policy 继续推到稳定 SDK DTO。
5. awiki-cli 的命令输入仍保持字符串 flag 兼容，但 adapter 层负责解析成 im-core 领域类型。
6. crates/im-core-dart 已移除 DartCreateGroupRequest.service_did，并从 mapping 中移除 default service DID 注入逻辑。
7. packages/awiki_im_core 的 CreateGroupRequest 已移除 serviceDid，并提供 GroupDiscoverability / GroupAdmissionMode / GroupMessageSecurityProfile / GroupMemberLimit value object。
8. group create 已在 wire builder 层拒绝 e2ee=true 和 GroupMessageSecurityProfile::GroupE2ee，返回 UnsupportedCapability("group-e2ee")，不会再把 group-e2ee policy 写入 create payload。
9. GroupReadResult::diagnostic_raw() 已从默认 public DTO API 移除；awiki-cli 迁移期 raw response 访问改走 compat::groups::raw_response。
```

仍需挑战：

```text
1. e2ee: bool 仍与 GroupMessageSecurityProfile::GroupE2ee 有长期模型重叠；当前只是 Phase 6 前明确 unsupported，后续应移除 bool 或合并进安全策略 enum。
2. Dart value object 当前保留 custom(String) 扩展能力，这是为了兼容协议未来枚举值；如果 SDK 要严格限制输入，需要在 facade 层改成 sealed enum 并把 custom 放入 advanced API。
3. 当前 FRB generated 文件为手动同步更新；需要安装 flutter_rust_bridge_codegen 后重新生成，确认 field count/序列化顺序无漂移。
```

原 Dart/FRB 同步问题：

```text
1. packages/awiki_im_core/lib/src/models/group.dart 的 CreateGroupRequest 已暴露 serviceDid。
2. Dart CreateGroupRequest 也暴露 discoverability / admissionMode / messageSecurityProfile / maxMembers 等 String 字段。
3. crates/im-core-dart/src/mapping/to_core.rs 已支持从 ImCoreConfig.anp_service_did 注入默认 service_did，说明普通 Dart 调用方不需要每次传 serviceDid。
4. 如果只改 Rust GroupCreateRequest，不改 Dart facade，Flutter SDK 仍会固化旧的 service routing 和 stringly typed group policy。
```

建议：

```text
1. `service_did` 改由 config/discovery/client runtime 决定，或至少移到 advanced/internal create context。当前工作区已选择 config/runtime 注入。
2. 新增领域类型。当前工作区已实现：
   - GroupDiscoverability
   - GroupAdmissionMode
   - GroupMemberRole
   - GroupMessageSecurityProfile
   - GroupMemberLimit
3. Phase 6 前，group-e2ee 相关请求返回 UnsupportedCapability。
4. `diagnostic_raw()` 移到 diagnostic feature 或 compat-only。
5. Dart facade 同步移除或降级 `CreateGroupRequest.serviceDid`，普通 API 使用 `AwikiImCoreConfig.anpServiceDid` / runtime discovery。当前工作区已移除 serviceDid。
6. Dart facade 同步提供 GroupDiscoverability / GroupAdmissionMode / GroupMemberRole / GroupMessageSecurityProfile 等 enum/value type，而不是继续暴露裸 String。当前工作区已完成 create request 相关 value object，成员 role facade 如后续开放 mutation request 也应同步领域化。
```

---

## 9. Realtime 接口

### 9.1 高层 service

当前高层 API：

```rust
impl RealtimeService<'_> {
    pub fn status(&self) -> ImResult<RealtimeStatus>;
    pub fn connect(&self, options: RealtimeOptions) -> ImResult<RealtimeHandle>;
    pub fn run_until_shutdown(
        &self,
        options: RealtimeOptions,
        shutdown: ShutdownSignal,
    ) -> ImResult<RealtimeExit>;
}
```

相关高层 DTO：

```rust
RealtimeOptions
ReconnectPolicy
RealtimeSubscription
RealtimeStatus
RealtimeExit
RealtimeExitReason
RealtimeConnectionState
RealtimeHandle
RealtimeControl
ShutdownSignal
ImEvent
```

判断：

```text
RealtimeService 本身是高层的。
```

### 9.2 当前模块导出问题

`crates/im-core/src/realtime/mod.rs` 当前除了高层 service/DTO，还 public re-export 了大量 internal helper：

```text
run_realtime_transport_until_shutdown
run_realtime_transport_with_event_sink_until_shutdown
RealtimeRunnerTransport
RealtimeRunnerEventSink
RealtimeRunnerOutcome

consume_notifications_step
ConsumeNotificationsAction
ConsumeNotificationsDecision
NotificationPingOutcome
SESSION_PING_INTERVAL
SESSION_PING_TIMEOUT

SessionLoopBackoff
SessionLoopRetryDecision
SessionLoopRetryPhase
session_loop_start_decision
secure_prekey_retry_decision
RealtimeShutdownDecision

bearer_authorization_header
connect_realtime_with_transport
realtime_client_construction_plan
realtime_client_endpoints
simulate_realtime_connect
validate_refresh_bearer_preconditions
RealtimeAuthProvider
RealtimeTransport
RealtimeClientEndpoints
RealtimeConnectAction
RealtimeDialOutcome
RealtimeRefreshOutcome
```

判断：

```text
这些不是默认 public API 应该暴露的高层接口。
它们属于 internal / test helper / compat-only / diagnostic-only。
```

建议目标：

`realtime/mod.rs` 默认只保留：

```rust
pub use self::control::{RealtimeControl, ShutdownSignal};
pub use self::dto::{
    RealtimeConnectionState, RealtimeExit, RealtimeExitReason, RealtimeOptions,
    RealtimeStatus, RealtimeSubscription, ReconnectPolicy,
};
pub use self::events::{
    ConnectionStateChanged, GroupUpdateKind, GroupUpdatedEvent, HostNotificationEvent,
    HostNotificationKind, ImEvent, LocalNotificationEvent, MessageReceivedEvent,
    MessageUpdateKind, MessageUpdatedEvent, UnknownNotificationEvent,
};
pub use self::handle::{RealtimeEventReceiver, RealtimeHandle};
pub use self::service::RealtimeService;
```

其余移动到：

```text
crate::compat::realtime        # CLI 迁移期需要
crate::internal::realtime      # im-core 内部需要
#[cfg(test)] test support      # 测试需要
diagnostic feature             # 真正需要公开诊断时
```

Dart/FRB 影响：

```text
1. crates/im-core-dart 当前只暴露 realtime capability/status/connect stub。
2. Dart capability 明确 runner_exposed=false、connect_supported=false。
3. 因此 Rust 收紧 realtime internal helper re-export 不应破坏 Dart facade。
4. 如果未来 Dart 要暴露 realtime runner，应优先设计 high-level event stream / lifecycle API，不直接映射 RealtimeRunnerTransport、heartbeat、session loop、transport helper。
```

---

## 10. Compat / migration-only 接口

当前：

```rust
#[doc(hidden)]
pub mod compat;
```

包含：

```text
compat::directory
compat::groups
compat::identity
compat::local_state
compat::messages
compat::profile
compat::proof
compat::realtime
compat::wire
```

判断：

```text
这不是核心 public API。
它是迁移期对 awiki-cli 的 bridge surface。
```

问题：

```text
1. #[doc(hidden)] 只隐藏文档，不阻止外部依赖。
2. compat 中存在大量 raw/wire/bridge/local_state/proof helper。
3. 如果 CLI cutover 后仍依赖 compat，im-core 会长期保留低层 public surface。
```

收口策略：

```text
1. compat 不进 prelude。
2. compat 不作为 semver 稳定 API。
3. compat 文件顶部加 migration-only 注释。
4. CLI 默认 cutover path 不使用 compat。
5. 后续放入 non-default feature，例如 legacy-compat。
6. 对应能力迁入 high-level service 后，删除 compat wrapper。
```

---

## 11. Prelude 当前问题

当前 `prelude.rs` 导出大量类型：

```text
Auth*
Core*
Directory*
Group*
Identity*
Ids*
Messages*
Paths*
Realtime*
ImCoreConfig / MessageTransportPolicy / ServiceEndpoint
```

判断：

```text
prelude 当前过宽。
```

主要风险：

```text
1. ReplaceDid* 高危/advanced DTO 进入 prelude。
2. GroupReadResult diagnostic_raw 相关 DTO 进入默认导入面。
3. 未来如果 realtime internal helper 也进入 prelude，会进一步扩大低层 API。
4. prelude 应该帮助普通调用方，而不是暴露完整 crate surface。
```

建议 prelude 默认只保留：

```text
ImCore / ImClient
ImCoreConfig / ImCorePaths / ServiceEndpoint / MessageTransportPolicy
ImError / ImResult
IdentitySelector / IdentitySummary
AuthService / AuthScope / AuthStatus
DirectoryService / DirectoryResolution / PublicProfile / Contact
MessageService / SendMessageRequest / MessageTarget / MessageBody / MessageKind
MessageSecurityMode / InboxQuery / HistoryQuery / Conversation
GroupService / GroupCreateRequest / GroupJoinRequest / GroupSnapshot / GroupSummary
RealtimeService / RealtimeOptions / RealtimeHandle / ImEvent / ShutdownSignal
Did / Handle / PeerRef / GroupRef / MessageId / Page / PageLimit / Cursor
```

不建议默认 prelude：

```text
ReplaceDid*
diagnostic-only result helpers
raw/wire/compat types
runner transport traits
session loop / heartbeat / reconnect helper types
```

---

## 12. 当前接口高层程度评估

| 区域 | 当前高层程度 | 结论 |
| --- | --- | --- |
| `ImCore` / `ImClient` | 高 | 保持。 |
| `CoreBootstrap` | 高 | 保持 lifecycle API。 |
| `AuthService` | 高 | 保持。 |
| `IdentityRegistry` | 高 | 保持。 |
| `IdentityService` | 中高 | replace DID 改为 advanced，不进 prelude。 |
| `DirectoryService` | 高 | 保持。 |
| `MessageService` | 高 | 文本消息高层；附件 DTO 已完成 Phase 4A canonical skeleton，真实附件 runtime 仍需 Phase 4B+。 |
| `GroupService` | 中高 | service 高层；create/policy/member DTO 已完成第一轮领域化，diagnostic_raw 已移出默认 API；仍需长期合并 e2ee bool 与安全策略模型，并补齐 FRB/Dart 验证。 |
| `RealtimeService` | 高 | service 高层。 |
| `realtime/mod.rs` exports | 中高 | internal helper re-export 已移出默认 realtime module；迁移期 helper 仍在 compat::realtime。 |
| `compat` | 低 | 已标注 migration-only，不算核心 API；后续可继续收进 feature 或删除 wrapper。 |
| `prelude` | 中高 | 已移出 ReplaceDid*/diagnostic 倾向类型；仍需持续防止 compat/internal/helper 回流。 |

---

## 13. 建议 PR 执行顺序

### PR A：冻结三层 public API baseline

状态（2026-05-23）：

```text
已推进：本文件已把 crates/im-core、crates/im-core-dart、packages/awiki_im_core 纳入同一 public API baseline，并标出 Dart facade 同步要求。
已推进：group serviceDid/string policy、diagnostic_raw、realtime helper、prelude advanced 类型已完成第一轮文本审计。
仍缺：需要把 rg 检查固化到 CI 或测试脚本中；当前只完成文档和人工命令验收，FRB/Dart 工具链验收仍未完成。
```

目标：

```text
记录 Rust / FRB / Dart 三层 public surface，防止后续无意扩大或跨语言 facade 固化旧边界。
```

建议动作：

```text
1. 新增 public API surface 检查文档或测试，至少覆盖 crates/im-core、crates/im-core-dart、packages/awiki_im_core。
2. 明确 high-level public / compat / internal / diagnostic / advanced 分类。
3. CI 中至少用 rg 检查 compat 不进入 prelude、Dart package 不导出 generated/internal、Dart facade 不暴露 raw/diagnostic/compat。
4. 将已完成的 Dart group serviceDid/string policy 收口固化为回归检查，避免后续重新暴露同类字段。
```

验收：

```bash
rg "pub use crate::compat|pub use .*compat" crates/im-core/src/prelude.rs crates/im-core/src/lib.rs
rg "pub use crate::internal" crates/im-core/src/lib.rs crates/im-core/src/prelude.rs
rg "generated|frb_generated|Dart[A-Z].*Request|diagnostic|raw|compat" packages/awiki_im_core/lib/awiki_im_core.dart packages/awiki_im_core/lib/src/models packages/awiki_im_core/lib/src/awiki_im_core_native.dart
rg "serviceDid|service_did|default_service_did" packages/awiki_im_core/lib/src/models/group.dart crates/im-core-dart/src/dto/group.rs crates/im-core-dart/src/mapping/to_core.rs
rg "String\\? discoverability|String\\? admissionMode|String\\? messageSecurityProfile|String\\? maxMembers|discoverability: String\\?|admissionMode: String\\?|messageSecurityProfile: String\\?|maxMembers: String\\?" packages/awiki_im_core/lib/src/models/group.dart
```

### PR B：收紧 realtime 默认导出

状态（2026-05-23）：

```text
已推进：realtime/mod.rs 已移除 runner/session loop/heartbeat/reconnect/transport helper 默认 re-export；CLI runtime 和相关测试改走 compat::realtime。
仍缺：建议把“realtime 默认导出不含 helper”的 rg 检查放入 CI，避免后续回流。
```

目标：

```text
让 `crate::realtime` 只暴露高层 service / DTO / event / handle。
```

改动：

```text
1. 从 realtime/mod.rs 删除 internal helper re-export。
2. 需要迁移期调用的 helper 移到 compat::realtime。
3. 测试如果依赖 helper，改为 crate internal test 或 compat path。
4. 确认 crates/im-core-dart 只依赖 RealtimeService/status/capability，不依赖 runner helper。
5. Dart facade 继续保持 runner_exposed=false；未来如要开放 runner，另行设计高层 event stream API。
```

验收：

```bash
rg "pub use crate::internal::realtime" crates/im-core/src/realtime/mod.rs
rg "RealtimeRunnerTransport|SessionLoopBackoff|bearer_authorization_header" crates/im-core/src/realtime/mod.rs
rg "RealtimeRunner|SessionLoop|heartbeat|transport" crates/im-core-dart/src packages/awiki_im_core/lib/src/models/realtime.dart packages/awiki_im_core/lib/src/awiki_im_core_native.dart
cargo test -p im-core realtime
```

### PR C：收窄 prelude

状态（2026-05-23）：

```text
已推进：prelude 已移出 ReplaceDid*、GroupReadResult、ContactBindingResult、RecoverGeneratedIdentity、MessageRetryPlan/MessageSendState 等 advanced 或 diagnostic 倾向类型。
已推进：GroupReadResult / ContactBindingResult / RecoverHandleResult 的 diagnostic_raw 默认 public method 已在 PR G 收口。
仍缺：prelude 检查应固化到 CI 或脚本，避免 compat/internal/helper 后续回流。
```

目标：

```text
prelude 只导入普通 SDK 调用方最常用的高层类型。
```

改动：

```text
1. 移除 ReplaceDid*。
2. 移除 diagnostic-only 或 advanced-only 类型。
3. 确保 prelude 不导入 compat / internal / runner helper。
4. 确认 Dart facade 没有新增 replace DID 或 diagnostic 默认 API。
```

验收：

```bash
rg "ReplaceDid|compat|RealtimeRunner|SessionLoop|diagnostic" crates/im-core/src/prelude.rs
rg "ReplaceDid|diagnostic|raw" packages/awiki_im_core/lib crates/im-core-dart/src/api crates/im-core-dart/src/dto
cargo test -p im-core
```

### PR D：给 compat 加 migration-only 策略

状态（2026-05-23）：

```text
已推进：compat/mod.rs 已补充 migration-only 文档，明确不进入 prelude、不镜像到 Dart facade、不承诺稳定 SDK surface。
仍缺：compat 仍是 public module 且 CLI 迁移期仍有依赖；是否放入 non-default legacy-compat feature 需要单独 PR 评估。
```

目标：

```text
让 compat 的临时属性变成代码和文档层面的强约束。
```

改动：

```text
1. compat/mod.rs 顶部注明 migration-only。
2. 所有 compat module 文档注明不承诺 semver。
3. 可选：放入 non-default feature legacy-compat。
4. CLI cutover 完成后，逐步删除不再需要的 compat wrapper。
5. Dart/FRB 不依赖 compat module；如确需迁移期桥接，必须留在 crates/im-core-dart 内部 mapping，不进入 package facade。
```

验收：

```bash
rg "migration-only|legacy-compat" crates/im-core/src/compat
rg "im_core::compat" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg "im_core::compat|compat::" crates/im-core-dart/src packages/awiki_im_core/lib
```

### PR E：领域化 group DTO

状态（2026-05-23）：

```text
已推进：Rust GroupCreateRequest 已移除 service_did；service DID 由 ImCoreConfig.anp_service_did/runtime 注入；Rust/FRB/Dart create request 已同步移除 per-request serviceDid；Rust 与 Dart facade 已增加 group policy value object。
已推进：group create 已明确拒绝 e2ee=true 和 GroupMessageSecurityProfile::GroupE2ee，避免 Phase 6 前暴露半成品 group e2ee。
仍缺：im-core-dart cargo check/test 受依赖下载阻塞未完成；Dart/Flutter 测试与 FRB codegen 因工具链缺失未完成；e2ee bool 长期仍应合并进安全策略模型。
```

目标：

```text
减少 Group API 中的 stringly typed 和调用方不该传的服务端细节。
```

已落地的第一阶段改动：

```rust
pub enum GroupDiscoverability {
    Private,
    Public,
    Unlisted,
}

pub enum GroupAdmissionMode {
    OpenJoin,
    InviteOnly,
    ApprovalRequired,
}

pub enum GroupMessageSecurityProfile {
    TransportProtected,
    GroupE2ee,
}

pub enum GroupMemberRole {
    Owner,
    Admin,
    Member,
}
```

仍需继续评估：

```text
1. e2ee bool 是否在 Phase 6 前后合并进 GroupMessageSecurityProfile，最终避免双字段表达同一安全策略。
2. Dart value object 是否继续允许 custom(String)；若 SDK 要严格限制输入，应改成 sealed enum，并把 custom 放入 advanced API。
3. crates/im-core-dart mapping 与 generated binding 需要由 codegen 重新生成验证，不能长期依赖手动同步。
4. 成员 mutation facade 如果后续在 Dart package 开放，也必须同步使用 GroupMemberRole value object，不能退回 String role。
```

验收：

```bash
rg "serviceDid|service_did|default_service_did" packages/awiki_im_core/lib/src/models/group.dart crates/im-core-dart/src/dto/group.rs crates/im-core-dart/src/mapping/to_core.rs crates/im-core-dart/src/api
rg "String\\? discoverability|String\\? admissionMode|String\\? messageSecurityProfile|String\\? maxMembers|discoverability: String\\?|admissionMode: String\\?|messageSecurityProfile: String\\?|maxMembers: String\\?" packages/awiki_im_core/lib/src/models/group.dart
rg "GroupDiscoverability|GroupAdmissionMode|GroupMessageSecurityProfile|GroupMemberLimit" crates/im-core/src/groups packages/awiki_im_core/lib/src/models/group.dart crates/im-core-dart/src/mapping/to_core.rs
cargo fmt --all -- --check
cargo check --offline -p im-core
cargo test --offline -p im-core group_domain_type_tests --lib
cargo test --offline -p im-core group_lifecycle --lib
cargo check --offline -p awiki-cli
cargo test -p im-core-dart
cargo test -p awiki-cli --test group_contract
cd packages/awiki_im_core && dart test
scripts/flutter/codegen.sh
```

### PR F：Phase 4A 收敛 attachments DTO skeleton

状态（2026-05-23）：

```text
已推进：完成 Phase 4A skeleton。新增 crates/im-core/src/attachments/{dto,service,mod}.rs；ImClient 新增 attachments()；lib.rs/prelude.rs 导出 AttachmentService 与 canonical DTO；MessageBody::Attachment 复用 attachments::AttachmentInput；AttachmentService::send/download 当前明确返回 UnsupportedCapability("attachments")。
仍缺：Phase 4B+ 的 manifest/digest/selection/upload/download runtime 尚未迁移；awiki-cli 仍保留 legacy attachment path；Dart package 仍未新增 attachment facade API。
```

目标：

```text
先建立 canonical AttachmentInput / AttachmentDestination / AttachmentService public shape，并替换 messages reserved attachment DTO。
```

改动：

```text
1. 新增或启用 attachments module public DTO。
2. MessageBody::Attachment 复用 attachments::AttachmentInput。
3. 不再维护 messages::AttachmentInput reserved 类型。
4. CLI 的 --file / --output 仍由 CLI 转换成 SDK DTO。
5. Dart package 新增附件发送 API 时直接暴露 canonical attachment model，不复制 reserved DTO。
6. FRB DTO/mapping 同步使用 canonical attachment model。
```

Phase 4B+ 后续开发入口：

```text
1. 将 crates/awiki-cli/src/message/attachment.rs 中可复用的 manifest/digest/selection 逻辑迁入 im-core internal/runtime。
2. 新增 attachments manifest/selection DTO 或 internal model，保持 CLI path/output 解析仍在 CLI。
3. AttachmentService::send 负责从 AttachmentInput 生成 manifest、digest、上传或写入 message payload。
4. AttachmentService::download 负责按 DownloadAttachmentRequest 写入 AttachmentDestination。
5. awiki-cli 从 legacy attachment helper 迁到 ImClient::attachments()/messages() 高层 API。
6. Dart package 在 Phase 4B+ 完成后再开放 attachment facade；开放时直接对齐 canonical model。
```

验收：

```bash
cargo test --offline -p im-core attachments
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
cargo test -p im-core-dart
cargo test -p awiki-cli --test msg_attachment_contract
cd packages/awiki_im_core && dart test
```

### PR G：diagnostic_raw 收口

状态（2026-05-23）：

```text
已推进：GroupReadResult / ContactBindingResult / RecoverHandleResult 已移除默认 public diagnostic_raw() method；内部字段改为 raw_response，普通 serde 输出仍跳过 raw；awiki-cli 迁移期通过 compat::groups::raw_response、compat::identity::{contact_binding_raw_response,recover_handle_raw_response} 读取 raw response。
仍缺：如果未来要重新开放调试能力，应设计明确 DiagnosticService 或 feature-gated diagnostic API，而不是把 raw response 放回默认 DTO。
```

目标：

```text
不让普通 SDK 调用方依赖 raw service response。
```

改动：

```text
1. GroupReadResult::diagnostic_raw() 移到 diagnostic feature 或 compat-only。
2. Identity/Recover/Bind result 中的 diagnostic_raw() 同样评估。
3. 如果需要调试，提供明确 DiagnosticService 或 feature-gated method。
4. Dart package 继续不暴露 diagnostic_raw/raw；如果要提供调试能力，新增明确命名的 diagnostic API 且默认不导出。
```

验收：

```bash
rg "diagnostic_raw" crates/im-core/src
rg "diagnostic|raw" packages/awiki_im_core/lib crates/im-core-dart/src/dto crates/im-core-dart/src/api
cargo test -p im-core
cargo test -p im-core-dart
cd packages/awiki_im_core && dart test
```

---

## 14. 当前验证记录

记录时间：2026-05-23。

已验证通过：

```bash
cargo fmt --all -- --check
cargo check --offline -p im-core
cargo test --offline -p im-core group_domain_type_tests --lib
cargo test --offline -p im-core group_lifecycle --lib
cargo test --offline -p im-core raw_response --lib
cargo test --offline -p im-core attachments
cargo check --offline -p awiki-cli
```

已完成的文本审计：

```bash
rg "serviceDid|service_did|default_service_did|discoverability: String\\?|admissionMode: String\\?|messageSecurityProfile: String\\?|maxMembers: String\\?" packages/awiki_im_core/lib/src crates/im-core-dart/src -g '!target'
rg "RealtimeRunnerTransport|SessionLoopBackoff|bearer_authorization_header|pub use .*runner|ReplaceDid|GroupReadResult|ContactBindingResult|RecoverGeneratedIdentity" crates/im-core/src/prelude.rs crates/im-core/src/realtime/mod.rs crates/im-core/src/compat/mod.rs
rg "diagnostic_raw|from_diagnostic_raw|with_diagnostic_raw" crates/im-core/src crates/im-core-dart/src packages/awiki_im_core/lib/src -g '!target'
rg "String\\? discoverability|String\\? admissionMode|String\\? messageSecurityProfile|String\\? maxMembers|discoverability: String\\?|admissionMode: String\\?|messageSecurityProfile: String\\?|maxMembers: String\\?" packages/awiki_im_core/lib/src/models/group.dart
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

文本审计结论：

```text
1. group create per-request serviceDid/service_did 已从 Rust DTO、FRB DTO、Dart facade create model 中移除。
2. 剩余 anp_service_did 属于 ImCoreConfig 配置字段，符合 service routing 下沉到 config/runtime 的设计。
3. packages/awiki_im_core 的 public facade model 不再用 String? 暴露 create group policy 字段。
4. realtime 默认 module 和 prelude 未发现 runner/session loop/heartbeat helper 或 advanced identity DTO 回流。
5. im-core、im-core-dart、Dart facade 中未发现 diagnostic_raw / from_diagnostic_raw / with_diagnostic_raw 默认 API。
6. FRB generated DTO 中仍有 String? discoverability/admissionMode/messageSecurityProfile/maxMembers，这是 bridge 传输层；packages/awiki_im_core public facade model 已用 value object 包装，不直接暴露这些 String? 字段。
7. im-core attachment skeleton 不引用 awiki-cli CLI 类型；AttachmentService 当前只暴露 send/download stub 和 canonical DTO，真实上传/下载仍未启用。
```

当前未验证：

```text
1. cargo check/test -p im-core-dart 尚未完成：当前机器 Cargo registry 使用 USTC mirror，下载依赖时出现 SSL connect error；offline cache 又缺少 addr2line v0.24.2。最新在线失败点为 threadpool 1.8.1 下载：SSL connect error in connection to crates-io.proxy.ustclug.org:443。临时 CARGO_HOME 重试仍进入 USTC 下载链路并复现同类 SSL 错误，已中止避免后台进程残留。
2. dart test / flutter test 尚未完成：当前机器未安装 dart、flutter。
3. FRB codegen 尚未重新运行：当前机器未安装 flutter_rust_bridge_codegen。
4. 当前 generated Rust/Dart binding 文件为手动同步修改，合并前必须在具备工具链的环境重新生成并确认无 diff。
```

合并前建议补跑：

```bash
cargo check -p im-core-dart
cargo test -p im-core-dart
cargo test -p awiki-cli --test group_contract
scripts/flutter/codegen.sh
cd packages/awiki_im_core && dart test
```

---

## 15. 最终目标接口形态

最终默认 Rust public API 应长这样：

```rust
let core = ImCore::new(config, paths)?;
core.bootstrap().validate_paths()?;

let identities = core.identities().list()?;
let client = core.client(IdentitySelector::Default)?;

client.auth().ensure_session(AuthScope::Messaging)?;

client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Text {
        text,
        kind: MessageKind::Text,
    },
    security: MessageSecurityMode::DefaultPlain,
    client_message_id: None,
    delivery: MessageDeliveryOptions::default(),
})?;

client.groups().list(GroupListRequest { limit })?;

client.realtime().run_until_shutdown(
    RealtimeOptions::default(),
    ShutdownSignal::pending(),
)?;
```

最终默认 Dart public API 应长这样：

```dart
final core = await AwikiImCore.open(config: config, paths: paths);
final identities = await core.listIdentities();
final client = await core.client(const IdentitySelector.defaultIdentity());

await client.auth.ensureSession(AuthScope.messaging);

await client.messages.sendText(
  SendTextRequest(
    target: MessageTarget.direct(peer),
    text: text,
    security: MessageSecurityMode.defaultPlain,
  ),
);

await client.groups.listGroups(limit: 50);
await client.realtime.status();
```

普通 Dart 调用方也不应该看到：

```text
FRB generated DTO
Rust Arc handle
serviceDid per-request routing override
raw diagnostic response
compat/bridge helper
wire/RPC method name
session loop / heartbeat / reconnect helper
CLI workspace path convention
```

普通调用方不应该看到：

```text
RPC method name
wire params
DID proof builder
JWT file path
SQLite connection
raw SQL
WebSocket frame classifier
pending dispatch queue
heartbeat/reconnect decision helper
MLS KeyPackage / secure prekey helper
OpenClaw / Hermes / service manager
CLI ParsedCommand / ExitError
```

---

## 16. 完成定义

本轮 public API 收口完成后，应满足：

```text
1. `crate::realtime` 只暴露 high-level service / DTO / event / handle。
2. `crate::compat` 仍可短期存在，但明确 migration-only，且不进 prelude。
3. `crate::prelude` 只包含普通调用方常用高层类型。
4. Group DTO 不再要求普通调用方传 service_did 或 raw string policy。
5. Message attachment DTO 在 Phase 4 后使用 canonical attachments DTO。
6. diagnostic_raw 不再是默认 API 鼓励路径。
7. im-core public API 不暴露 CLI 类型、wire helper、raw RPC、SQLite helper、WebSocket frame helper。
8. crates/im-core-dart 不把 compat/internal/raw helper 映射成跨语言 API。
9. packages/awiki_im_core 不导出 generated/FRB/internal 类型，只导出稳定 facade 和高层 model。
10. Dart Group DTO 不再要求普通调用方传 serviceDid 或 raw string policy。
11. Dart attachments API 新增时直接使用 canonical attachment model。
12. Rust public API、FRB DTO/mapping、Dart facade 三层 API baseline 同步更新并有验收检查。
```

一句话目标：

```text
im-core 与 awiki_im_core public API 只表达“选择身份后做什么 IM 业务”；
实现细节留在 internal / compat / diagnostic feature / bridge mapping；
CLI/App/Flutter 不再因为迁移期便利而绑定底层 helper。
```
