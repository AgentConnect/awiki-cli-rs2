# awiki-cli-rs2 / im-core 面向 awiki-me Flutter App 的可执行接入计划

本文档目标是让 Codex 在 `awiki-cli-rs2` 仓库中一次性完成 `im-core` 的 Flutter/Dart SDK 封装方案。当前阶段**不修改 `awiki-me` 仓库**，只在 `awiki-cli-rs2` 中补齐可被 `awiki-me` 将来依赖的 Flutter package、Rust-Dart bridge、构建脚本、测试和文档。

---

## 0. 结论

最佳方案是：

```text
awiki-me Flutter App
    |
    | 未来通过 pubspec path/git/pub dependency 引入
    v
packages/awiki_im_core              # Flutter/Dart SDK package
    |
    | flutter_rust_bridge / dart:ffi
    v
crates/im-core-dart                 # Rust -> Dart facade crate
    |
    v
crates/im-core                      # 纯 Rust IM 核心 SDK
```

核心原则：

1. `crates/im-core` 继续保持纯 Rust 业务核心，不引入 Flutter、Dart、FFI、C ABI、平台打包逻辑。
2. 新增 `crates/im-core-dart`，专门承接 Flutter/Dart FFI facade。
3. 新增 `packages/awiki_im_core`，作为 `awiki-me` 未来直接依赖的 Flutter package。
4. 首版只支持 Flutter native 平台：Android、iOS、macOS、Windows。Web 只提供 Dart stub，让依赖方可以分析通过，但运行时返回 `UnsupportedError`。
5. 因为 `awiki-me` 当前 Flutter 下限是 `>=3.24.0`，不要依赖 Flutter 3.38+ 才推荐的 `package_ffi` build hooks 作为唯一方案。首版采用 Flutter FFI plugin/package 兼容形态 + 显式构建脚本。后续如果 `awiki-me` 升级到 Flutter 3.38+，再迁移到 `package_ffi`/native assets build hooks。
6. 当前 goal 不切换 `awiki-me` 的 `AwikiGateway` / `AwikiAccountGateway` 实现，只准备可集成 SDK。

---

## 1. 仓库现状与约束

### 1.1 awiki-cli-rs2

`new-im-core` 分支已经具备下列基础：

```text
crates/im-core
crates/awiki-cli
xtask
```

`im-core` 已经是独立 Rust crate，并且默认 feature 是：

```toml
default = ["blocking", "sqlite", "http"]
```

同时预留了：

```text
attachments
realtime
secure-direct
group-e2ee
provider-traits
internal-test-helpers
```

这说明当前最适合做法不是把 `im-core` 改成 Flutter crate，而是在它旁边新增 Dart facade。

`im-core` 当前已暴露或准备暴露的业务入口包括：

```text
ImCore
ImClient
IdentityRegistry
AuthService
IdentityService
DirectoryService
MessageService
GroupService
RealtimeService
```

### 1.2 awiki-me

`awiki-me` 当前是 Dart-only Flutter App，README 说明账号创建、DID-WBA 认证、User Service、IM、message proof 都在 Dart 代码中完成。

`awiki-me` 的 `pubspec.yaml` 约束：

```yaml
environment:
  sdk: ">=3.8.0 <4.0.0"
  flutter: ">=3.24.0"
```

关键依赖包括：

```text
anp
http
web_socket_channel
flutter_secure_storage
sqflite
sqlite3_flutter_libs
path_provider
flutter_riverpod
flutter_local_notifications
```

`AppBootstrap.create()` 当前构造了：

```text
AwikiAccountService
AwikiAnpGateway
AwikiWsRealtimeGateway
AppNotificationFacade
NoopE2eeFacade
LocalePreferenceService
AppUpdateService
```

`AwikiGateway` 当前包含完整 App 业务入口：

```text
loadCapabilities
loadMyProfile
updateProfile
loadPublicProfile
listFollowers / listFollowing / follow / unfollow / getRelationshipStatus
listConversations
fetchDmHistory / fetchGroupHistory
sendTextMessage / retryMessage
createGroup / joinGroup / getGroup / listGroups / listGroupMembers
consumeRealtimeEvent
markRead
deleteLocalThread
```

`AwikiAccountGateway` 当前包含：

```text
restoreSession
currentSession
refreshSession
currentAnpSession
logout
listLocalCredentials
loginWithLocalCredential
deleteLocalCredential
exportCurrentCredentialAsZip
importCredentialFromZip
sendOtp
sendEmailVerification
checkEmailVerified
registerHandle
registerHandleWithEmail
recoverHandle
```

`AwikiAnpGateway` 当前承担 User Service、Message Service、profile、relationship、conversation、message、group 与 local cache 的聚合逻辑；`AwikiLocalCache` 使用 `sqflite` 存储 conversations、messages、groups。

因此 Flutter SDK 的第一版不能只暴露“send message”一个函数，至少要为未来 `awiki-me` adapter 预留完整 App-facing API 形状；但当前 goal 不要求修改 `awiki-me` 本身。

---

## 2. 整体方案

### 2.1 分层

```text
┌──────────────────────────────────────────────┐
│ awiki-me                                     │
│ Flutter App，未来只依赖 Dart package          │
└──────────────────────┬───────────────────────┘
                       │
                       v
┌──────────────────────────────────────────────┐
│ packages/awiki_im_core                       │
│ Flutter package                              │
│ - App-friendly Dart API                      │
│ - conditional import                         │
│ - native loader                              │
│ - generated FRB Dart binding                 │
│ - web stub                                   │
└──────────────────────┬───────────────────────┘
                       │
                       v
┌──────────────────────────────────────────────┐
│ crates/im-core-dart                          │
│ Rust facade for Dart                         │
│ - FFI-safe/app-friendly DTO                  │
│ - mapping to/from im-core DTO                │
│ - error mapping                              │
│ - async wrappers for blocking im-core calls  │
└──────────────────────┬───────────────────────┘
                       │
                       v
┌──────────────────────────────────────────────┐
│ crates/im-core                               │
│ Pure Rust SDK                                │
│ - identity/auth/message/group/directory      │
│ - sqlite/http/realtime internal orchestration│
│ - no Flutter/Dart/FFI concern                │
└──────────────────────────────────────────────┘
```

### 2.2 为什么用 flutter_rust_bridge

首版采用 `flutter_rust_bridge v2`：

```text
Dart/Flutter API 友好
支持 Rust struct/enum/Result
支持 async Dart 调用
减少手写 char* / handle / free / error-buffer 的 C ABI 维护成本
方便未来扩展 Stream/realtime
```

不采用 UniFFI 作为首选，因为当前 App 只有 Flutter/Dart，没有原生 Swift/Kotlin App。

不采用纯手写 `dart:ffi + C ABI + ffigen` 作为首选，因为 `im-core` 是复杂业务 SDK，DTO、错误、对象生命周期、异步调用会产生大量手工胶水代码。

### 2.3 平台支持边界

首版支持：

```text
Android: arm64-v8a, x86_64, optional armeabi-v7a
iphoneOS: aarch64-apple-ios
iOS Simulator: aarch64-apple-ios-sim, optional x86_64-apple-ios
macOS: aarch64-apple-darwin, x86_64-apple-darwin
Windows: x86_64-pc-windows-msvc, optional aarch64-pc-windows-msvc
```

Web：

```text
不支持 native im-core。
提供 Dart stub，保证 package 可被引用和 analyze。
运行时抛出 UnsupportedError。
awiki-me Web 后续继续走 Dart-only gateway，或另做 wasm 方案。
```

---

## 3. 目标目录结构

Codex 应在 `awiki-cli-rs2` 中新增/调整如下结构：

```text
awiki-cli-rs2/
  Cargo.toml

  crates/
    im-core/
      Cargo.toml
      src/
        ...existing...

    im-core-dart/
      Cargo.toml
      src/
        lib.rs
        api/
          mod.rs
          core.rs
          client.rs
          auth.rs
          identity.rs
          directory.rs
          messages.rs
          groups.rs
          profile.rs
          unsupported.rs
        dto/
          mod.rs
          config.rs
          identity.rs
          auth.rs
          directory.rs
          message.rs
          group.rs
          profile.rs
          error.rs
        mapping/
          mod.rs
          to_core.rs
          from_core.rs
        frb_generated.rs              # generated; do not hand-edit after generation
      tests/
        facade_contract.rs

    awiki-cli/
      ...existing...

    xtask/
      ...existing...

  packages/
    awiki_im_core/
      pubspec.yaml
      README.md
      CHANGELOG.md
      analysis_options.yaml
      lib/
        awiki_im_core.dart
        src/
          awiki_im_core_base.dart
          awiki_im_core_native.dart
          awiki_im_core_web_stub.dart
          native_library_loader.dart
          models/
            auth.dart
            config.dart
            identity.dart
            directory.dart
            message.dart
            group.dart
            profile.dart
            error.dart
          generated/
            bridge_generated.dart      # generated
            frb_generated.dart         # generated if required by FRB version
      android/
        build.gradle
        src/main/AndroidManifest.xml
        src/main/jniLibs/
          arm64-v8a/.gitkeep
          x86_64/.gitkeep
          armeabi-v7a/.gitkeep
      ios/
        awiki_im_core.podspec
        Classes/.gitkeep
        Frameworks/.gitkeep
      macos/
        awiki_im_core.podspec
        Classes/.gitkeep
        Frameworks/.gitkeep
      windows/
        CMakeLists.txt
        awiki_im_core.dll.placeholder
      test/
        awiki_im_core_stub_test.dart
      example/
        pubspec.yaml
        lib/main.dart

  scripts/
    flutter/
      codegen.sh
      build-host.sh
      build-android.sh
      build-apple.sh
      build-windows.ps1
      build-all.sh
      package.sh

  docs/
    flutter-sdk/
      awiki-im-core-flutter-sdk.md
      awiki-me-future-integration.md

  .github/
    workflows/
      flutter-im-core.yml              # optional but recommended
```

---

## 4. 详细执行步骤

### Step 1：确认工作分支和基础检查

执行位置：`awiki-cli-rs2` 仓库根目录。

```bash
git status
cargo +1.79.0 check -p im-core --locked
cargo +1.79.0 test -p im-core --locked
cargo +1.79.0 run -p xtask --locked -- check-structure
```

如果当前分支不是包含 `crates/im-core` 的分支，先切到对应分支：

```bash
git checkout new-im-core
```

验收：

```text
crates/im-core 存在
cargo check -p im-core 通过
```

---

### Step 2：更新 workspace

修改根目录 `Cargo.toml`：

```toml
[workspace]
members = [
    "crates/im-core",
    "crates/im-core-dart",
    "crates/awiki-cli",
    "xtask",
]
resolver = "2"
```

不要改 `im-core` 的职责，不要在 `crates/im-core` 中引入 Flutter/Dart 依赖。

验收：

```bash
cargo +1.79.0 metadata --no-deps >/dev/null
```

---

### Step 3：新增 Rust facade crate：`crates/im-core-dart`

创建 `crates/im-core-dart/Cargo.toml`：

```toml
[package]
name = "im-core-dart"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[lib]
name = "awiki_im_core"
crate-type = ["cdylib", "staticlib", "rlib"]

[features]
default = ["blocking", "sqlite", "http"]
blocking = ["im-core/blocking"]
sqlite = ["im-core/sqlite"]
http = ["im-core/http"]
android = []
ios = []
macos = []
windows = []
attachments = ["im-core/attachments"]
realtime = ["im-core/realtime"]
secure-direct = ["im-core/secure-direct"]
group-e2ee = ["im-core/group-e2ee"]

[dependencies]
anyhow.workspace = true
im-core = { path = "../im-core", default-features = false }
serde.workspace = true
serde_json.workspace = true
flutter_rust_bridge = "2.12.0"

[dev-dependencies]
tempfile = "=3.3.0"
```

如果 `flutter_rust_bridge = "2.12.0"` 与仓库 Rust toolchain/MSRV 冲突，Codex 应改为最新能在 `cargo +1.79.0 check -p im-core-dart` 下通过的 `2.x` 版本，并在 `docs/flutter-sdk/awiki-im-core-flutter-sdk.md` 记录实际版本。

创建 `crates/im-core-dart/src/lib.rs`：

```rust
pub mod api;
pub mod dto;
pub mod mapping;

#[allow(clippy::all)]
#[allow(unused)]
pub mod frb_generated;
```

创建 `api/mod.rs`：

```rust
pub mod auth;
pub mod client;
pub mod core;
pub mod directory;
pub mod groups;
pub mod identity;
pub mod messages;
pub mod profile;
pub mod unsupported;
```

创建 `dto/mod.rs` 与 `mapping/mod.rs`，分别 re-export 子模块。

验收：

```bash
cargo +1.79.0 check -p im-core-dart --locked
```

如果首次新增 crate 需要更新 lockfile，则执行：

```bash
cargo +1.79.0 check -p im-core-dart
```

然后保留更新后的 `Cargo.lock`。

---

### Step 4：定义 Dart facade DTO

DTO 设计原则：

```text
全部使用 String / int / bool / List / Option / struct / enum
不暴露 PathBuf
不暴露泛型 Page<T>
不暴露 raw serde_json::Value，除非字段名为 diagnosticRawJson
不暴露 im-core internal 类型
错误统一转换为 DartImError
```

必须实现以下 DTO。

#### 4.1 config DTO

`dto/config.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartImCoreConfig {
    pub service_base_url: String,
    pub did_domain: String,
    pub user_service_endpoint: Option<String>,
    pub message_service_endpoint: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub transport_policy: DartMessageTransportPolicy,
}

#[derive(Debug, Clone)]
pub enum DartMessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}

#[derive(Debug, Clone)]
pub struct DartImCorePaths {
    pub identity_root_dir: String,
    pub registry_path: String,
    pub default_identity_path: Option<String>,
    pub sqlite_path: String,
    pub cache_dir: String,
    pub temp_dir: String,
}
```

#### 4.2 identity DTO

`dto/identity.rs`：

```rust
#[derive(Debug, Clone)]
pub enum DartIdentitySelector {
    Default,
    Id { id: String },
    Did { did: String },
    Handle { handle: String },
    LocalAlias { alias: String },
}

#[derive(Debug, Clone)]
pub struct DartIdentitySummary {
    pub id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<String>,
}
```

#### 4.3 auth DTO

`dto/auth.rs`：

```rust
#[derive(Debug, Clone)]
pub enum DartAuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

#[derive(Debug, Clone)]
pub struct DartAuthStatus {
    pub authenticated: bool,
    pub expired: bool,
    pub did: String,
    pub handle: Option<String>,
    pub expires_at: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DartSessionBundle {
    pub did: String,
    pub handle: Option<String>,
    pub expires_at: Option<String>,
    pub warnings: Vec<String>,
}
```

字段从 `im-core::auth` 实际 DTO 映射。若字段名不同，以 `im-core` 的当前 public DTO 为准，但 Dart DTO 名称保持不变。

#### 4.4 message DTO

`dto/message.rs`：

```rust
#[derive(Debug, Clone)]
pub enum DartMessageTarget {
    Direct { peer: String },
    Group { group: String },
}

#[derive(Debug, Clone)]
pub enum DartThreadRef {
    Direct { peer: String },
    Group { group: String },
    Thread { thread_id: String },
}

#[derive(Debug, Clone)]
pub enum DartMessageSecurityMode {
    DefaultPlain,
    Plain,
    SecureDirect,
    GroupE2ee,
}

#[derive(Debug, Clone)]
pub struct DartSendTextRequest {
    pub target: DartMessageTarget,
    pub text: String,
    pub markdown: bool,
    pub security: DartMessageSecurityMode,
    pub client_message_id: Option<String>,
    pub wait_for_final_acceptance: bool,
}

#[derive(Debug, Clone)]
pub struct DartMessage {
    pub id: String,
    pub thread_id: String,
    pub sender_did: String,
    pub sender_name: Option<String>,
    pub receiver_did: Option<String>,
    pub group_id: Option<String>,
    pub text: Option<String>,
    pub original_type: String,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub is_mine: bool,
    pub server_sequence: Option<i64>,
    pub is_encrypted: bool,
    pub send_state: String,
}

#[derive(Debug, Clone)]
pub struct DartConversationSummary {
    pub thread_id: String,
    pub display_name: String,
    pub last_message_preview: String,
    pub last_message_at: Option<String>,
    pub unread_count: u32,
    pub is_group: bool,
    pub target_did: Option<String>,
    pub group_id: Option<String>,
    pub avatar_seed: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartMessagePage {
    pub items: Vec<DartMessage>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct DartConversationPage {
    pub items: Vec<DartConversationSummary>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct DartSendMessageResult {
    pub message: DartMessage,
    pub delivery_state: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DartMarkReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<String>,
    pub warnings: Vec<String>,
}
```

这些字段刻意接近 `awiki-me` 的 `ChatMessage` 和 `ConversationSummary`，但不要 import `awiki-me` 类型。

#### 4.5 group DTO

`dto/group.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartGroupSummary {
    pub group_id: String,
    pub name: String,
    pub description: String,
    pub member_count: u32,
    pub last_message_at: Option<String>,
    pub my_role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartGroupMemberSummary {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartCreateGroupRequest {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: Option<String>,
    pub group_mode: Option<String>,
}
```

#### 4.6 profile / relationship DTO

`dto/profile.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartUserProfile {
    pub did: String,
    pub handle: Option<String>,
    pub nick_name: String,
    pub bio: String,
    pub tags: Vec<String>,
    pub profile_markdown: String,
    pub avatar_url: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartProfilePatch {
    pub nick_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub profile_markdown: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartRelationshipSummary {
    pub did: String,
    pub display_name: String,
    pub relationship: String,
}
```

#### 4.7 error DTO

`dto/error.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartImError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub status_code: Option<u16>,
    pub capability: Option<String>,
}

impl DartImError {
    pub fn unsupported(capability: impl Into<String>) -> Self {
        let capability = capability.into();
        Self {
            code: "unsupported_capability".to_string(),
            message: format!("unsupported capability: {capability}"),
            field: None,
            status_code: None,
            capability: Some(capability),
        }
    }
}
```

实现：

```rust
impl From<im_core::ImError> for DartImError { ... }
```

必须映射：

```text
InvalidInput -> invalid_input
IdentityRequired -> identity_required
IdentityNotFound -> identity_not_found
DefaultIdentityMissing -> default_identity_missing
AuthRequired -> auth_required
SessionExpired -> session_expired
PermissionDenied -> permission_denied
PeerNotFound -> peer_not_found
GroupNotFound -> group_not_found
MessageNotFound -> message_not_found
TransportUnavailable -> transport_unavailable
UnsupportedCapability -> unsupported_capability
LocalStateUnavailable -> local_state_unavailable
PathUnavailable -> path_unavailable
CredentialFileUnreadable -> credential_file_unreadable
Service -> service_error
Serialization -> serialization_error
Io -> io_error
Internal -> internal_error
```

验收：

```bash
cargo +1.79.0 check -p im-core-dart
```

---

### Step 5：实现 mapping 层

创建：

```text
crates/im-core-dart/src/mapping/to_core.rs
crates/im-core-dart/src/mapping/from_core.rs
```

`to_core.rs` 负责：

```text
DartImCoreConfig -> im_core::ImCoreConfig
DartImCorePaths -> im_core::ImCorePaths
DartIdentitySelector -> im_core::identity::IdentitySelector
DartAuthScope -> im_core::auth::AuthScope
DartMessageTarget -> im_core::messages::MessageTarget
DartThreadRef -> im_core::messages::ThreadRef
DartMessageSecurityMode -> im_core::messages::MessageSecurityMode
DartCreateGroupRequest -> im_core::groups::GroupCreateRequest
DartProfilePatch -> im_core::identity::ProfilePatch
```

`from_core.rs` 负责：

```text
IdentitySummary -> DartIdentitySummary
AuthStatus / SessionBundle / SessionUpdate -> DartAuthStatus / DartSessionBundle
Message -> DartMessage
SendMessageResult -> DartSendMessageResult
Page<Message> -> DartMessagePage
Conversation -> DartConversationSummary
Page<Conversation> -> DartConversationPage
Group read result -> DartGroupSummary / DartGroupMemberSummary
Profile / PublicProfile -> DartUserProfile
RelationStatus -> DartRelationshipSummary
```

映射规则：

1. 所有 ID newtype 通过 `.as_str().to_string()` 转出。
2. 时间字段保持 ISO-8601 string，不在 Rust-Dart 边界转换成 `DateTime`。
3. `MessageBodyView::Text` 映射为 `text = Some(text)`，否则 `text = None` 且 `original_type = "unsupported"`。
4. `MessageDirection::Outgoing` 映射 `is_mine = true`，其他为 `false`。
5. `ThreadRef::Direct(peer)` 生成 `thread_id = "dm:$peer"`；`ThreadRef::Group(group)` 生成 `thread_id = "group:$group"`；`ThreadRef::Thread(id)` 直接使用 id。
6. 对于 im-core 尚无字段，使用安全默认值，不 panic。

验收：

```bash
cargo +1.79.0 test -p im-core-dart
```

---

### Step 6：实现 Rust API facade

#### 6.1 core API

`api/core.rs`：

```rust
use std::sync::Arc;

pub struct DartImCore {
    inner: im_core::ImCore,
}

pub fn open_core(
    config: crate::dto::config::DartImCoreConfig,
    paths: crate::dto::config::DartImCorePaths,
) -> Result<Arc<DartImCore>, crate::dto::error::DartImError> {
    let inner = im_core::ImCore::new(config.try_into()?, paths.try_into()?)
        .map_err(crate::dto::error::DartImError::from)?;
    Ok(Arc::new(DartImCore { inner }))
}

pub fn validate_paths(
    core: Arc<DartImCore>,
) -> Result<Vec<String>, crate::dto::error::DartImError> {
    let report = core.inner.bootstrap().validate_paths()
        .map_err(crate::dto::error::DartImError::from)?;
    Ok(format_path_report(report))
}
```

`format_path_report` 用稳定字符串列表输出即可，避免先暴露复杂 path report DTO。

#### 6.2 client API

`api/client.rs`：

```rust
use std::sync::Arc;

pub struct DartImClient {
    inner: im_core::ImClient,
}

pub fn core_client(
    core: Arc<crate::api::core::DartImCore>,
    selector: crate::dto::identity::DartIdentitySelector,
) -> Result<Arc<DartImClient>, crate::dto::error::DartImError> {
    let inner = core.inner.client(selector.try_into()?)
        .map_err(crate::dto::error::DartImError::from)?;
    Ok(Arc::new(DartImClient { inner }))
}

pub fn current_identity(
    client: Arc<DartImClient>,
) -> crate::dto::identity::DartIdentitySummary {
    client.inner.current_identity().into()
}
```

#### 6.3 identity API

`api/identity.rs`：

```text
list_identities(core)
default_identity(core)
resolve_identity(core, selector)
register_handle_with_phone(core, ...)
register_handle_with_email(core, ...)
recover_handle(core, ...)
```

这些方法直接调用 `core.inner.identities()`。

如果某个 im-core registration API 当前字段与 Dart DTO 不完全一致，Codex 应根据 `crates/im-core/src/identity/dto.rs` 的实际 public DTO 调整 mapping，保持 Dart API 名称不变。

#### 6.4 auth API

`api/auth.rs`：

```text
auth_status(client)
auth_login(client)
auth_ensure_session(client, scope)
auth_refresh_session(client)
```

直接调用：

```rust
client.inner.auth().status()
client.inner.auth().login()
client.inner.auth().ensure_session(scope)
client.inner.auth().refresh_session()
```

#### 6.5 profile / directory API

`api/profile.rs`：

```text
load_my_profile(client)
update_profile(client, patch)
load_public_profile(client, did_or_handle)
```

实现：

```text
load_my_profile -> client.inner.identity().profile()
update_profile -> client.inner.identity().update_profile(...)
load_public_profile -> client.inner.directory().public_profile(...)
```

`api/directory.rs`：

```text
resolve_peer(client, peer)
lookup_handle(client, handle)
relation_status(client, peer)
```

`awiki-me` 目前的 follow/unfollow/listFollowers/listFollowing 逻辑需要完整 remote relationship RPC。若 `im-core` 尚未有对应 public API，先在 `im-core-dart` 中保留函数但返回：

```rust
Err(DartImError::unsupported("relationship-remote-mutation"))
```

必须保留函数名，便于未来 `awiki-me` adapter 一次性切换接口。

#### 6.6 message API

`api/messages.rs`：

```text
send_text(client, request)
inbox(client, limit, cursor, unread_only)
history(client, thread, limit, cursor)
mark_read(client, message_ids)
conversations(client, limit, include_groups, include_direct, unread_only)
retry_message(client, message)
```

实现：

```text
send_text -> client.inner.messages().send(...)
inbox -> client.inner.messages().inbox(...)
history -> client.inner.messages().history(...)
mark_read -> client.inner.messages().mark_read(...)
conversations -> client.inner.messages().conversations(...)
retry_message -> rebuild SendTextRequest from DartMessage if enough fields exist; otherwise return invalid_input
```

`fetchDmHistory(peerDid)` 的未来 Dart wrapper 应使用：

```text
history(ThreadRef::Direct(peerDid), limit, cursor)
```

`fetchGroupHistory(groupId)` 的未来 Dart wrapper 应使用：

```text
history(ThreadRef::Group(groupId), limit, cursor)
```

#### 6.7 group API

`api/groups.rs`：

```text
create_group(client, request)
join_group(client, group_did)
get_group(client, group_did)
list_groups(client, limit, cursor)
list_group_members(client, group_did, limit, cursor)
leave_group(client, group_did)
```

直接调用 `client.inner.groups()` 中已有 public API。

`getGroupJoinCode` / `refreshGroupJoinCode` 当前 `awiki-me` 也是 `null`，在 SDK 中可以实现为：

```rust
pub fn get_group_join_code(...) -> Result<Option<String>, DartImError> { Ok(None) }
pub fn refresh_group_join_code(...) -> Result<Option<String>, DartImError> { Ok(None) }
```

#### 6.8 realtime API

首版只做 stub：

```text
realtime_status(client) -> unsupported or disconnected status
realtime_connect(...) -> UnsupportedCapability("realtime-runner")
```

原因：`awiki-me` 当前已有 Dart WebSocket gateway；当前 goal 不切换它。

---

### Step 7：新增 Flutter package：`packages/awiki_im_core`

创建 `packages/awiki_im_core/pubspec.yaml`：

```yaml
name: awiki_im_core
description: Awiki IM core Flutter SDK backed by Rust im-core.
version: 0.1.0
publish_to: "none"

environment:
  sdk: ">=3.8.0 <4.0.0"
  flutter: ">=3.24.0"

dependencies:
  flutter:
    sdk: flutter
  ffi: ^2.1.0
  flutter_rust_bridge: ^2.12.0
  meta: ^1.12.0
  path: ^1.9.0

dev_dependencies:
  flutter_test:
    sdk: flutter
  flutter_lints: ^4.0.0

flutter:
  plugin:
    platforms:
      android:
        ffiPlugin: true
      ios:
        ffiPlugin: true
      macos:
        ffiPlugin: true
      windows:
        ffiPlugin: true
```

如果 Flutter 3.24 对 `ffiPlugin: true` 的平台配置有差异，Codex 应以 `flutter create --template=plugin_ffi` 在临时目录生成的 pubspec 结构为准，然后迁移到 `packages/awiki_im_core`。

创建 `lib/awiki_im_core.dart`：

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
```

创建 `lib/src/awiki_im_core_base.dart`：

```dart
import 'awiki_im_core_native.dart'
    if (dart.library.html) 'awiki_im_core_web_stub.dart';

export 'awiki_im_core_native.dart'
    if (dart.library.html) 'awiki_im_core_web_stub.dart';
```

如果当前 Dart 条件导入对 web 使用 `dart.library.js_interop`，Codex 应按项目 Dart 版本采用可通过 analyze 的写法。

创建 `lib/src/awiki_im_core_web_stub.dart`：

```dart
import 'models/config.dart';

class AwikiImCore {
  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    throw UnsupportedError('awiki_im_core native Rust backend is not supported on Flutter Web.');
  }
}
```

创建 native facade `lib/src/awiki_im_core_native.dart`，包装 generated FRB API，并对 App 暴露稳定 Dart API：

```dart
class AwikiImCore {
  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    // initialize native library
    // call generated openCore
  }

  Future<AwikiImClient> client(IdentitySelector selector);
  Future<List<IdentitySummary>> listIdentities();
  Future<IdentitySummary?> defaultIdentity();
}

class AwikiImClient {
  AuthApi get auth;
  IdentityApi get identity;
  DirectoryApi get directory;
  MessageApi get messages;
  GroupApi get groups;
}
```

Dart model 文件应与 Rust DTO 同名同义，但可以更 Dart 化，例如 camelCase 字段。

验收：

```bash
cd packages/awiki_im_core
flutter pub get
dart analyze
flutter test
```

---

### Step 8：生成 flutter_rust_bridge 绑定

创建 `scripts/flutter/codegen.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if ! command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
  echo "flutter_rust_bridge_codegen is required. Install with: cargo install flutter_rust_bridge_codegen --version 2.12.0" >&2
  exit 1
fi

flutter_rust_bridge_codegen generate \
  --rust-root "crates/im-core-dart" \
  --rust-input "crate::api" \
  --dart-output "packages/awiki_im_core/lib/src/generated/bridge_generated.dart" \
  --rust-output "crates/im-core-dart/src/frb_generated.rs"
```

如果 `flutter_rust_bridge_codegen generate` 的 CLI 参数与当前 2.x 版本不同，Codex 应执行：

```bash
flutter_rust_bridge_codegen generate --help
```

然后调整脚本，但保持输入/输出路径不变。

执行：

```bash
cargo install flutter_rust_bridge_codegen --version 2.12.0
scripts/flutter/codegen.sh
```

验收：

```text
packages/awiki_im_core/lib/src/generated/bridge_generated.dart 存在
crates/im-core-dart/src/frb_generated.rs 存在
cargo +1.79.0 check -p im-core-dart 通过
cd packages/awiki_im_core && dart analyze 通过
```

---

### Step 9：native library loader

创建 `packages/awiki_im_core/lib/src/native_library_loader.dart`。

职责：

```text
Android -> DynamicLibrary.open('libawiki_im_core.so')
Windows -> DynamicLibrary.open('awiki_im_core.dll')
macOS -> DynamicLibrary.open('libawiki_im_core.dylib') 或 DynamicLibrary.process()
iOS -> DynamicLibrary.process() / DynamicLibrary.executable()
```

示例：

```dart
import 'dart:ffi';
import 'dart:io';

DynamicLibrary loadAwikiImCoreLibrary() {
  if (Platform.isAndroid) {
    return DynamicLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isWindows) {
    return DynamicLibrary.open('awiki_im_core.dll');
  }
  if (Platform.isMacOS) {
    return DynamicLibrary.open('libawiki_im_core.dylib');
  }
  if (Platform.isIOS) {
    return DynamicLibrary.process();
  }
  throw UnsupportedError('Unsupported platform for awiki_im_core native library.');
}
```

Codex 应按 FRB 2.x 生成代码要求，将 loader 接入 generated API 初始化位置。

---

### Step 10：Android 构建脚本

创建 `scripts/flutter/build-android.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

OUT_DIR="${ROOT_DIR}/packages/awiki_im_core/android/src/main/jniLibs"
TARGETS=(aarch64-linux-android x86_64-linux-android armv7-linux-androideabi)

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would cargo ndk build arm64-v8a x86_64 armeabi-v7a into ${OUT_DIR}"
  exit 0
fi

rustup target add "${TARGETS[@]}"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is required. Install a version compatible with the workspace toolchain." >&2
  exit 1
fi

cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  -t armeabi-v7a \
  -o "${OUT_DIR}" \
  build \
  -p im-core-dart \
  --release \
  --no-default-features \
  --features blocking,sqlite,http,android
```

验收：

```bash
scripts/flutter/build-android.sh --dry-run
```

完整构建验收：

```bash
scripts/flutter/build-android.sh
ls packages/awiki_im_core/android/src/main/jniLibs/arm64-v8a/libawiki_im_core.so
ls packages/awiki_im_core/android/src/main/jniLibs/x86_64/libawiki_im_core.so
```

---

### Step 11：Apple 构建脚本

创建 `scripts/flutter/build-apple.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

LIB_NAME="awiki_im_core"
IOS_FRAMEWORK_DIR="${ROOT_DIR}/packages/awiki_im_core/ios/Frameworks"
MACOS_FRAMEWORK_DIR="${ROOT_DIR}/packages/awiki_im_core/macos/Frameworks"
DIST_DIR="${ROOT_DIR}/dist/flutter/awiki_im_core/apple"

TARGETS=(
  aarch64-apple-ios
  aarch64-apple-ios-sim
  x86_64-apple-ios
  aarch64-apple-darwin
  x86_64-apple-darwin
)

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would build staticlibs and lipo universal simulator/macos libs"
  exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Apple build must run on macOS." >&2
  exit 1
fi

rustup target add "${TARGETS[@]}"

for target in "${TARGETS[@]}"; do
  cargo build \
    -p im-core-dart \
    --release \
    --target "${target}" \
    --no-default-features \
    --features blocking,sqlite,http,ios,macos
done

mkdir -p "${IOS_FRAMEWORK_DIR}" "${MACOS_FRAMEWORK_DIR}" "${DIST_DIR}"

cp "target/aarch64-apple-ios/release/lib${LIB_NAME}.a" \
  "${IOS_FRAMEWORK_DIR}/lib${LIB_NAME}_ios_device.a"

lipo -create \
  "target/aarch64-apple-ios-sim/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-ios/release/lib${LIB_NAME}.a" \
  -output "${IOS_FRAMEWORK_DIR}/lib${LIB_NAME}_ios_simulator.a"

lipo -create \
  "target/aarch64-apple-darwin/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-darwin/release/lib${LIB_NAME}.a" \
  -output "${MACOS_FRAMEWORK_DIR}/lib${LIB_NAME}.a"
```

创建 `packages/awiki_im_core/ios/awiki_im_core.podspec` 和 `macos/awiki_im_core.podspec`，链接对应 static library。

验收：

```bash
scripts/flutter/build-apple.sh --dry-run
```

完整构建验收必须在 macOS runner 上执行。

---

### Step 12：Windows 构建脚本

创建 `scripts/flutter/build-windows.ps1`：

```powershell
param(
  [string]$Target = "x86_64-pc-windows-msvc",
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "../..")
Set-Location $Root

if ($DryRun) {
  Write-Host "Would rustup target add $Target"
  Write-Host "Would cargo build -p im-core-dart --release --target $Target"
  exit 0
}

rustup target add $Target

cargo build `
  -p im-core-dart `
  --release `
  --target $Target `
  --no-default-features `
  --features blocking,sqlite,http,windows

New-Item -ItemType Directory -Force -Path "packages/awiki_im_core/windows" | Out-Null
Copy-Item "target/$Target/release/awiki_im_core.dll" "packages/awiki_im_core/windows/awiki_im_core.dll" -Force
```

验收：

```powershell
scripts/flutter/build-windows.ps1 -DryRun
```

---

### Step 13：host 构建脚本

创建 `scripts/flutter/build-host.sh`，方便本地 macOS/Linux/Windows bash 环境做 smoke build：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

cargo build \
  -p im-core-dart \
  --release \
  --no-default-features \
  --features blocking,sqlite,http
```

---

### Step 14：总构建脚本

创建 `scripts/flutter/build-all.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

scripts/flutter/codegen.sh
scripts/flutter/build-host.sh
scripts/flutter/build-android.sh --dry-run

if [[ "$(uname -s)" == "Darwin" ]]; then
  scripts/flutter/build-apple.sh --dry-run
fi

(
  cd packages/awiki_im_core
  flutter pub get
  dart analyze
  flutter test
)
```

注意：`build-all.sh` 默认只做 Android/Apple dry-run，避免非 Android/Apple 环境失败。完整平台构建交给 CI matrix。

---

### Step 15：Flutter package tests

创建 `packages/awiki_im_core/test/awiki_im_core_stub_test.dart`：

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:awiki_im_core/awiki_im_core.dart';

void main() {
  test('config model can be constructed', () {
    final config = AwikiImCoreConfig(
      serviceBaseUrl: 'https://awiki.ai',
      didDomain: 'awiki.ai',
    );
    expect(config.serviceBaseUrl, 'https://awiki.ai');
  });
}
```

创建 Rust facade tests：`crates/im-core-dart/tests/facade_contract.rs`：

```rust
#[test]
fn dart_error_unsupported_has_stable_code() {
    let err = im_core_dart::dto::error::DartImError::unsupported("realtime-runner");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("realtime-runner"));
}
```

验收：

```bash
cargo +1.79.0 test -p im-core-dart
cd packages/awiki_im_core && flutter test
```

---

### Step 16：文档

创建 `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`，内容包括：

```text
SDK 分层
支持平台
不支持 Web native 的原因
如何 codegen
如何构建 Android/iOS/macOS/Windows
如何在 awiki-me 未来通过 path dependency 引入
常见错误：找不到 anp sibling checkout、cargo-ndk 未安装、iOS staticlib 未链接、Windows dll 未复制
```

创建 `docs/flutter-sdk/awiki-me-future-integration.md`，只描述未来 awiki-me 需要怎么接，不实际修改仓库。

未来 awiki-me 集成建议：

```yaml
dependencies:
  awiki_im_core:
    path: ../awiki-cli-rs2/packages/awiki_im_core
```

未来 AppBootstrap 切换建议：

```dart
const backend = String.fromEnvironment(
  'AWIKI_IM_BACKEND',
  defaultValue: 'dart',
);

if (backend == 'rust') {
  // use AwikiRustAccountGateway / AwikiRustGateway
} else {
  // keep existing Dart gateway
}
```

但这些修改不在当前 goal 中执行。

---

### Step 17：可选 CI workflow

创建 `.github/workflows/flutter-im-core.yml`：

```yaml
name: Flutter IM Core SDK

on:
  pull_request:
  workflow_dispatch:

jobs:
  rust-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          path: awiki-cli-rs2
      - uses: actions/checkout@v4
        with:
          repository: agent-network-protocol/anp
          path: anp
      - name: Rust
        working-directory: awiki-cli-rs2
        run: |
          rustup toolchain install 1.79.0 --profile minimal
          cargo +1.79.0 check -p im-core --locked
          cargo +1.79.0 check -p im-core-dart --locked
          cargo +1.79.0 test -p im-core-dart --locked

  flutter-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          path: awiki-cli-rs2
      - uses: actions/checkout@v4
        with:
          repository: agent-network-protocol/anp
          path: anp
      - uses: subosito/flutter-action@v2
        with:
          flutter-version: '3.24.0'
      - name: Check package
        working-directory: awiki-cli-rs2/packages/awiki_im_core
        run: |
          flutter pub get
          dart analyze
          flutter test
```

如果 `Cargo.lock` 更新后 `--locked` 不适用，Codex 应先更新 lockfile并提交，然后恢复 `--locked`。

---

## 5. Codex 一次性执行 Goal

把下面内容作为 Codex goal 使用：

```text
Goal: In the awiki-cli-rs2 repository, on the branch that contains crates/im-core, add a Flutter/Dart SDK package for im-core without modifying the awiki-me repository.

Requirements:
1. Keep crates/im-core as a pure Rust SDK. Do not add Flutter, Dart, FFI, or platform packaging logic to crates/im-core.
2. Add crates/im-core-dart as a Rust facade crate depending on im-core. Its lib name must be awiki_im_core and crate-type must include cdylib, staticlib, and rlib.
3. Add DTO, mapping, and API modules under crates/im-core-dart. The exposed API must cover core open/client, identity list/default/resolve/register/recover, auth status/login/ensure/refresh, profile read/update/public profile, directory resolve/lookup/relation status, message send/inbox/history/mark-read/conversations/retry, group create/join/get/list/members, and realtime stubs. For im-core capabilities that are not yet implemented, return a stable DartImError with code unsupported_capability instead of omitting the function.
4. Add packages/awiki_im_core as a Flutter package compatible with Dart >=3.8.0 and Flutter >=3.24.0. Use flutter_rust_bridge 2.x and dart:ffi. Do not rely exclusively on Flutter 3.38+ package_ffi build hooks.
5. Add a web stub so the package can be analyzed when imported by a Flutter project that also has web support. The stub must throw UnsupportedError at runtime.
6. Add scripts/flutter/codegen.sh, build-host.sh, build-android.sh, build-apple.sh, build-windows.ps1, and build-all.sh. All platform build scripts must support --dry-run or -DryRun where appropriate.
7. Add minimal Rust and Flutter tests.
8. Add docs/flutter-sdk/awiki-im-core-flutter-sdk.md and docs/flutter-sdk/awiki-me-future-integration.md.
9. Do not modify awiki-me. Do not change the current awiki-cli release workflow unless required for workspace correctness.
10. Run and fix: cargo +1.79.0 check -p im-core-dart, cargo +1.79.0 test -p im-core-dart, scripts/flutter/build-android.sh --dry-run, scripts/flutter/build-all.sh if Flutter is available, and cargo +1.79.0 run -p xtask --locked -- check-structure.

Acceptance:
- cargo metadata succeeds.
- cargo check -p im-core-dart succeeds.
- cargo test -p im-core-dart succeeds.
- packages/awiki_im_core/pubspec.yaml exists and flutter pub get succeeds when Flutter is available.
- dart analyze succeeds for packages/awiki_im_core when Flutter is available.
- scripts/flutter/build-android.sh --dry-run succeeds.
- scripts/flutter/build-windows.ps1 -DryRun succeeds on Windows or is syntactically valid.
- No file in awiki-me is modified.
```

---

## 6. 后续 awiki-me 集成方向，不在本次执行范围

未来真正修改 `awiki-me` 时，建议按以下方式切换，而不是直接删除 Dart-only 实现：

```text
lib/src/data/services/awiki_rust_account_gateway.dart
lib/src/data/gateways/awiki_rust_gateway.dart
lib/src/app/bootstrap.dart 增加 backend env switch
```

环境开关：

```bash
flutter run --dart-define=AWIKI_IM_BACKEND=rust
flutter run --dart-define=AWIKI_IM_BACKEND=dart
```

迁移顺序：

```text
1. identity restore/list/default
2. auth status/refresh
3. profile read/update/public profile
4. send direct/group text
5. inbox/history/conversations/markRead
6. group create/join/get/list/members
7. relationship APIs
8. realtime
9. remove Dart-only duplicate code
```

在 `awiki-me` 未切换前，现有 Dart-only gateway 继续作为 production fallback。

---

## 7. 风险与处理

### 7.1 Flutter Web

`dart:ffi` 不支持 Web native library。当前 package 必须提供 web stub，不能承诺 Web 使用 Rust backend。

### 7.2 SQLite 双缓存

`awiki-me` 当前用 `sqflite` 做 local cache，`im-core` 默认也启用 `sqlite`。未来切换时应选择一个 source of truth。建议未来 Rust backend 模式下以 `im-core` local state 为准，`awiki-me` 的 `AwikiLocalCache` 逐步退化为 UI cache 或删除。

### 7.3 anp sibling dependency

`awiki-cli-rs2` 当前 workspace 依赖 sibling path `../anp/rust`。CI 和本地构建必须 checkout sibling `anp`。后续发布 Flutter SDK 时，应评估把 `anp` 改成 pinned git dependency 或 vendored submodule。

### 7.4 flutter_rust_bridge 版本

首选使用 `2.12.0`。如果与 Rust 1.79/MSRV 不兼容，Codex 应 pin 到能通过 workspace 检查的最新 2.x 版本，并在文档中记录。

### 7.5 iOS static linking

首版 iOS 用 staticlib。Dart 端通过 `DynamicLibrary.process()` 解析符号。若后续改 dynamic framework，需要额外处理 framework embedding、signing、App Store 审核风险。

---

## 8. 最终验收命令清单

在 `awiki-cli-rs2` 根目录：

```bash
cargo +1.79.0 metadata --no-deps >/dev/null
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p im-core --locked
cargo +1.79.0 check -p im-core-dart --locked
cargo +1.79.0 test -p im-core-dart --locked
cargo +1.79.0 run -p xtask --locked -- check-structure
bash -n scripts/flutter/codegen.sh
bash -n scripts/flutter/build-host.sh
bash -n scripts/flutter/build-android.sh
bash -n scripts/flutter/build-apple.sh
bash -n scripts/flutter/build-all.sh
scripts/flutter/build-android.sh --dry-run
```

在 `packages/awiki_im_core`：

```bash
flutter pub get
dart analyze
flutter test
```

Windows PowerShell：

```powershell
scripts/flutter/build-windows.ps1 -DryRun
```

完成后应满足：

```text
awiki-cli-rs2 中新增 Flutter SDK package
awiki-cli-rs2 中新增 Rust-Dart facade crate
平台构建脚本存在且 dry-run 通过
不修改 awiki-me
im-core 仍为纯 Rust SDK
```
