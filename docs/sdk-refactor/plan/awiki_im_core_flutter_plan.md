
# awiki-cli-rs2 / im-core Flutter SDK 可执行计划（修订版）

本文档用于 Codex 在 `awiki-cli-rs2` 仓库中一次性补齐 `im-core` 的 Flutter/Dart SDK 封装。当前阶段**不修改 `awiki-me` 仓库**，只在 `awiki-cli-rs2` 中新增可被 `awiki-me` 未来依赖的通用 Flutter SDK package、Rust-Dart facade、构建脚本、测试和文档。

> 定位修订：`packages/awiki_im_core` 是**通用 im-core Flutter SDK**，不是 `awiki-me` 专用 adapter。`awiki-me` 只是未来首个目标接入方。任何 `awiki-me` 的 UI/cache DTO mapping 都放在未来 `awiki-me` 集成阶段，不进入本 SDK 公共接口。

---

## 0. Review 采纳说明

除“计划范围过大”外，本版采纳 review 中的关键意见：

1. **SDK 定位**：明确 `packages/awiki_im_core` 是通用 Flutter SDK，不固化 `awiki-me` 当前 `AwikiGateway` / `ChatMessage` / `ConversationSummary` 形状。
2. **FFI 生命周期与线程模型**：增加 opaque object、dispose、Send/Sync、blocking 调用线程池要求。
3. **retry_message**：首版不得从展示用 message DTO 重建发送请求；先返回 `UnsupportedCapability("message-retry")`。
4. **group create**：`GroupCreateRequest.service_did` 必须显式解决，优先使用 request.service_did，其次使用 `ImCoreConfig.anp_service_did`，都没有则返回 `invalid_input`.
5. **realtime/WebSocket**：本次延后 realtime connect/session/events 与 WebSocket runtime；只暴露 capabilities/status 和明确 unsupported connect，未来再单独设计。
6. **DTO 漂移**：Dart/Rust facade DTO 贴近 `im-core` public DTO，Dart wrapper 可提供便利 getter，但不能改变语义。
7. **平台打包闭环**：补充 Android、iOS、macOS 的构建脚本、iOS/macOS podspec / vendored framework 或 library、loader 规则。Windows 不在 v0.1 范围内。
8. **MSRV**：保持 workspace `rust-version = "1.78"`，CI 可用 `1.79.0` 验证，但不得无说明提升 MSRV。
9. **generated 文件策略**：generated files 入库，并增加 codegen diff check；Rust generated 大文件需要 `docs/file-size-exceptions.md` 例外。

“计划范围过大”意见本版不处理，因为当前目标仍是一次性让 Codex 完成该 SDK scaffold、facade、脚本、测试和文档。

---

## 1. 总体方案

### 1.1 分层

```text
awiki-me Flutter App
    |
    | 未来通过 pubspec path/git/pub dependency 引入
    v
packages/awiki_im_core
    通用 Flutter/Dart SDK package
    - Dart public API
    - conditional import
    - native loader
    - generated flutter_rust_bridge Dart binding
    - web stub
    |
    | flutter_rust_bridge / dart:ffi
    v
crates/im-core-dart
    Rust -> Dart facade crate
    - im-core-facing DTO
    - mapping to/from im-core DTO
    - error mapping
    - opaque object lifecycle
    - blocking call boundary
    |
    v
crates/im-core
    纯 Rust IM 核心 SDK
    - identity / auth / directory / messages / groups / realtime
    - sqlite / http / local-state orchestration
    - no Flutter / Dart / FFI / platform packaging
```

### 1.2 核心原则

1. `crates/im-core` 继续保持纯 Rust SDK，不引入 Flutter、Dart、FFI、C ABI、平台打包逻辑。
2. `crates/im-core-dart` 只做 Dart facade，不承载业务逻辑，不重新实现 ANP RPC。
3. `packages/awiki_im_core` 是通用 SDK 包，不 import `awiki-me` 类型，不暴露 `awiki-me` 的 UI/cache DTO。
4. Dart public API 必须表达 `im-core` 业务意图：选择身份、auth、profile、directory、messages、groups、capabilities。
5. App-facing mapping，例如 `DartMessage -> awiki-me ChatMessage`、`DartConversation -> awiki-me ConversationSummary`，留到未来 `awiki-me` 仓库修改阶段。
6. 首版只支持 Flutter native 平台：Android、iOS、macOS。Windows 与 Web 均不在 v0.1 native 支持范围内；Web 只提供 stub，运行时抛 `UnsupportedError`。
7. 不切换 `awiki-me` 当前 `AwikiGateway` / `AwikiAccountGateway` 实现。

### 1.3 为什么选择 flutter_rust_bridge

首版采用 `flutter_rust_bridge v2`：

```text
Dart/Flutter API 友好
支持 Rust struct / enum / Result
支持 sync Rust -> async Dart
默认可把同步 Rust 函数放入 FRB worker thread pool
支持 opaque Rust object
减少手写 C ABI 的 char* / handle / free / error-buffer 维护成本
方便未来扩展 Stream / realtime
```

不采用 UniFFI 作为首选，因为当前 App 只有 Flutter/Dart，没有原生 Swift/Kotlin App。

不采用纯手写 `dart:ffi + C ABI + ffigen` 作为首选，因为 `im-core` 是复杂业务 SDK，DTO、错误、对象生命周期、异步调用会产生大量手工胶水代码。

### 1.4 平台支持边界

首版目标：

```text
Android:
  arm64-v8a
  x86_64
  optional armeabi-v7a

iOS:
  aarch64-apple-ios
  aarch64-apple-ios-sim
  optional x86_64-apple-ios

macOS:
  aarch64-apple-darwin
  x86_64-apple-darwin
```

Web：

```text
不支持 native im-core。
提供 Dart stub，保证 package 可被引用和 analyze。
运行时抛出 UnsupportedError。
未来如果需要 Web，另做 wasm 或纯 Dart API 方案。
```

Windows：

```text
v0.1 不支持。
不声明 Flutter Windows plugin platform。
不创建 Windows DLL 构建脚本。
未来如果需要 Windows，另做独立平台计划，补齐 DLL 构建、CMake、loader 和 CI。
```

---

## 2. 目标目录结构

Codex 应在 `awiki-cli-rs2` 中新增/调整如下结构：

```text
awiki-cli-rs2/
  Cargo.toml

  crates/
    im-core/
      ...existing...

    im-core-dart/
      Cargo.toml
      src/
        lib.rs
        api/
          mod.rs
          core.rs
          client.rs
          identity.rs
          auth.rs
          directory.rs
          messages.rs
          groups.rs
          profile.rs
          realtime.rs
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
          realtime.rs
          error.rs
        mapping/
          mod.rs
          to_core.rs
          from_core.rs
        frb_generated.rs              # generated; committed; do not hand-edit
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
            realtime.dart
            error.dart
          generated/
            bridge_generated.dart      # generated; committed
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
        include/
          awiki_im_core.h
      macos/
        awiki_im_core.podspec
        Classes/.gitkeep
        Frameworks/.gitkeep
        include/
          awiki_im_core.h
      test/
        awiki_im_core_stub_test.dart
      example/
        pubspec.yaml
        lib/main.dart

  scripts/
    flutter/
      codegen.sh
      codegen-check.sh
      build-host.sh
      build-android.sh
      build-apple.sh
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

## 3. 详细执行步骤

### Step 1：确认工作分支和基础检查

执行位置：`awiki-cli-rs2` 仓库根目录。

```bash
git status
git checkout new-im-core
cargo +1.79.0 check -p im-core --locked
cargo +1.79.0 test -p im-core --locked
cargo +1.79.0 run -p xtask --locked -- check-structure
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

不要改 `crates/im-core` 的职责，不要在 `crates/im-core` 中引入 Flutter/Dart 依赖。

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

MSRV 规则：

```text
workspace rust-version 仍保持 1.78。
CI/脚本可使用 cargo +1.79.0 作为当前仓库已有验证工具链。
如果 flutter_rust_bridge = "2.12.0" 与 Rust 1.78/1.79 不兼容：
  1. 优先 pin 到能通过 cargo +1.78.0 check 和 cargo +1.79.0 check 的最新 2.x 版本；
  2. 只有在没有兼容 2.x 版本时，才允许提交 MSRV 升级；
  3. 若升级 MSRV，必须在 docs/flutter-sdk/awiki-im-core-flutter-sdk.md 说明原因。
```

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
pub mod realtime;
pub mod unsupported;
```

创建 `dto/mod.rs` 与 `mapping/mod.rs`，分别 re-export 子模块。

验收：

```bash
cargo +1.79.0 check -p im-core-dart
```

如果首次新增 crate 需要更新 lockfile，则保留更新后的 `Cargo.lock`，后续 CI 再恢复 `--locked`。

---

### Step 4：定义 Rust facade DTO

DTO 设计原则：

```text
贴近 im-core public DTO，不贴近 awiki-me UI/cache DTO。
全部使用 String / int / bool / List / Option / struct / enum。
不暴露 PathBuf。
不暴露泛型 Page<T>，改成专用 Page DTO。
不暴露 raw serde_json::Value，除非字段名为 diagnostic_raw_json。
不暴露 im-core internal 类型。
错误统一转换为 DartImError。
Dart package 可在 wrapper 层提供便利 getter，但 Rust facade DTO 字段语义不得漂移。
```

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

`dto/auth.rs` 必须贴近当前 `im_core::auth` DTO：

```rust
#[derive(Debug, Clone)]
pub enum DartAuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

#[derive(Debug, Clone)]
pub struct DartAuthStatus {
    pub subject: String,
    pub has_session: bool,
    pub expires_at: Option<String>,
    pub needs_refresh: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DartSessionBundle {
    pub subject: String,
    pub scope: DartAuthScope,
    pub expires_at: Option<String>,
    pub refreshed: bool,
}

#[derive(Debug, Clone)]
pub struct DartSessionUpdate {
    pub subject: String,
    pub previous_expires_at: Option<String>,
    pub new_expires_at: Option<String>,
    pub refreshed: bool,
}
```

Dart wrapper 可以提供便利 getter，例如：

```dart
extension AuthStatusConvenience on AuthStatus {
  bool get authenticated => hasSession;
}
```

不要在 Rust facade DTO 中把 `has_session` 改名成 `authenticated`，避免语义漂移。

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
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

#[derive(Debug, Clone)]
pub enum DartMessageDirection {
    Outgoing,
    Incoming,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DartMessageBodyView {
    pub text: Option<String>,
    pub kind: Option<String>,
    pub unsupported_content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartMessageMetadata {
    pub operation_id: Option<String>,
    pub delivery_state: Option<String>,
    pub send_state: Option<String>,
    pub retryable: Option<bool>,
    pub retry_action: Option<String>,
    pub server_sequence: Option<i64>,
    pub content_type: Option<String>,
    pub attributes: Vec<DartMessageMetadataAttribute>,
}

#[derive(Debug, Clone)]
pub struct DartMessageMetadataAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct DartMessage {
    pub id: String,
    pub thread_kind: String,
    pub thread_id: String,
    pub direction: DartMessageDirection,
    pub sender: String,
    pub receiver: Option<String>,
    pub group: Option<String>,
    pub body: DartMessageBodyView,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub metadata: DartMessageMetadata,
}

#[derive(Debug, Clone)]
pub struct DartMessagePage {
    pub items: Vec<DartMessage>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct DartConversation {
    pub thread_kind: String,
    pub thread_id: String,
    pub title: Option<String>,
    pub participants: Vec<String>,
    pub last_message: Option<DartMessage>,
    pub unread_count: u32,
    pub message_count: u32,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartConversationPage {
    pub items: Vec<DartConversation>,
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

不要在 Rust facade DTO 中加入 `last_message_preview`、`avatar_seed` 等 `awiki-me` UI/cache 字段。未来 `awiki-me` adapter 可从 `DartConversation.last_message` 派生。

#### 4.5 group DTO

`dto/group.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartGroupSummary {
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartGroupSnapshot {
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartGroupMember {
    pub did: Option<String>,
    pub handle: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartCreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    pub discoverability: Option<String>,
    pub admission_mode: Option<String>,
    pub message_security_profile: Option<String>,
    pub e2ee: bool,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<String>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,

    // Mapping requirement:
    // 1. if present, map to im_core::groups::GroupCreateRequest.service_did
    // 2. otherwise use DartImClient.default_service_did captured from DartImCoreConfig.anp_service_did
    // 3. if both missing, return DartImError invalid_input(field = "service_did")
    pub service_did: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartGroupReadResult {
    pub group: Option<DartGroupSnapshot>,
    pub groups: Vec<DartGroupSummary>,
    pub members: Vec<DartGroupMember>,
    pub messages: DartMessagePage,
    pub total: Option<u32>,
    pub source: Option<String>,
    pub warnings: Vec<String>,
}
```

#### 4.6 profile / directory DTO

`dto/profile.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartUserProfile {
    pub subject: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Vec<String>,
    pub markdown: Option<String>,
    pub avatar_url: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartProfilePatch {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub markdown: Option<String>,
}
```

`dto/directory.rs`：

```rust
#[derive(Debug, Clone)]
pub enum DartIdentitySubject {
    Did { did: String },
    Handle { handle: String },
    Any { value: String },
}

#[derive(Debug, Clone)]
pub struct DartDirectoryResolution {
    pub input: String,
    pub did: String,
    pub handle: Option<String>,
    pub profile: Option<DartUserProfile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DartRelationStatus {
    pub peer: String,
    pub relationship: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartRelationshipStatus {
    pub peer: String,
    pub did: String,
    pub is_following: bool,
    pub is_follower: bool,
    pub is_friend: bool,
    pub is_contact: bool,
    pub messaged: bool,
    pub relationship: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DartRelationshipListItem {
    pub did: Option<String>,
    pub handle: Option<String>,
    pub profile: Option<DartUserProfile>,
    pub created_at: Option<String>,
    pub warnings: Vec<String>,
}
```

Remote relationship APIs 现在可绑定到 Rust `im-core` `DirectoryService`：

```text
follow -> client.inner.directory().follow(...)
unfollow -> client.inner.directory().unfollow(...)
relationship_status -> client.inner.directory().relationship_status(...)
list_followers -> client.inner.directory().followers(...)
list_following -> client.inner.directory().following(...)
```

Flutter/Dart facade 仍不得重新实现 HTTP RPC，也不得暴露 user-service 内部 `from_user_id` / `to_user_id` 字段。

#### 4.7 realtime DTO

`dto/realtime.rs`：

```rust
#[derive(Debug, Clone)]
pub struct DartRealtimeCapability {
    pub status_supported: bool,
    pub connect_supported: bool,
    pub runner_exposed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DartRealtimeStatus {
    pub connected: bool,
    pub state: String,
    pub subscriptions: Vec<String>,
    pub last_error: Option<String>,
    pub warnings: Vec<String>,
}
```

Realtime DTO 规则：

```text
本次不暴露 RealtimeSession / Stream<RealtimeEvent> / runner / connect。
realtime_connect 必须返回 UnsupportedCapability("realtime-runner")。
WebSocket / ws / wss / ping / pong / raw frame / request id / pending dispatch queue 不得进入 Dart public DTO。
未来若启用 realtime bridge，WebSocket 仍只能是 im-core internal transport。
```

#### 4.8 error DTO

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
    pub fn invalid_input(
        field: impl Into<Option<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: "invalid_input".to_string(),
            message: message.into(),
            field: field.into(),
            status_code: None,
            capability: None,
        }
    }

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

    pub fn object_closed(object: impl Into<String>) -> Self {
        let object = object.into();
        Self {
            code: "object_closed".to_string(),
            message: format!("{object} has been disposed"),
            field: None,
            status_code: None,
            capability: None,
        }
    }
}
```

必须实现：

```rust
impl From<im_core::ImError> for DartImError { ... }
```

错误码映射：

```text
InvalidInput -> invalid_input
IdentityRequired -> identity_required
IdentityNotFound -> identity_not_found
DefaultIdentityMissing -> default_identity_missing
IdentityNotReady -> identity_not_ready
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
DartCreateGroupRequest + service_did resolution -> im_core::groups::GroupCreateRequest
DartProfilePatch -> im_core::identity::ProfilePatch
DartIdentitySubject -> im_core::directory::IdentitySubject
```

`from_core.rs` 负责：

```text
IdentitySummary -> DartIdentitySummary
AuthStatus -> DartAuthStatus
SessionBundle -> DartSessionBundle
SessionUpdate -> DartSessionUpdate
Message -> DartMessage
SendMessageResult -> DartSendMessageResult
Page<Message> -> DartMessagePage
Conversation -> DartConversation
Page<Conversation> -> DartConversationPage
GroupReadResult -> DartGroupReadResult
Profile / PublicProfile -> DartUserProfile
DirectoryResolution -> DartDirectoryResolution
RelationStatus -> DartRelationStatus
RealtimeStatus -> DartRealtimeStatus
```

关键映射规则：

1. 所有 ID newtype 通过 `.as_str().to_string()` 转出。
2. 时间字段保持 ISO-8601 string，不在 Rust-Dart 边界转换成 `DateTime`。
3. `MessageBodyView::Text` 映射为 `DartMessageBodyView { text: Some(text), kind: Some(kind), unsupported_content_type: None }`。
4. `MessageBodyView::Unsupported` 映射为 `text: None` 且 `unsupported_content_type` 保留。
5. `ThreadRef::Direct(peer)` 映射为 `thread_kind = "direct"`、`thread_id = peer`。
6. `ThreadRef::Group(group)` 映射为 `thread_kind = "group"`、`thread_id = group`。
7. `ThreadRef::Thread(id)` 映射为 `thread_kind = "thread"`、`thread_id = id`。
8. 对于 im-core 尚无字段，使用安全默认值，不 panic。
9. `DartCreateGroupRequest.service_did` 解析优先级：
   ```text
   request.service_did
   -> DartImClient.default_service_did
   -> invalid_input(field = "service_did")
   ```
10. realtime bridge 延后；本次不得把 `im_core::realtime::ImEvent`、raw notification JSON、WebSocket frame 或 transport error object 透传到 Dart public API。

验收：

```bash
cargo +1.79.0 test -p im-core-dart
```

---

### Step 6：实现 FFI 对象生命周期与线程模型

#### 6.1 opaque object 原则

`DartImCore` 与 `DartImClient` 是 FRB opaque Rust objects，不导出裸指针，不手写 C handle/free。

要求：

```text
DartImCore 和 DartImClient 必须是 Send + Sync。
内部使用 Arc 和 Mutex/RwLock 管理状态。
对象必须有 close/dispose 方法。
close 后再次调用任何方法必须返回 DartImError { code: "object_closed" }。
不得依赖 Dart GC 作为唯一释放方式；Dart wrapper 必须暴露 dispose()。
```

建议结构：

```rust
use std::sync::{Arc, RwLock};

pub struct DartImCore {
    state: Arc<RwLock<DartImCoreState>>,
}

struct DartImCoreState {
    inner: Option<im_core::ImCore>,
    default_service_did: Option<String>,
}

pub struct DartImClient {
    state: Arc<RwLock<DartImClientState>>,
}

struct DartImClientState {
    inner: Option<im_core::ImClient>,
    default_service_did: Option<String>,
}

```

辅助函数：

```rust
impl DartImCore {
    fn with_inner<T>(
        &self,
        f: impl FnOnce(&im_core::ImCore) -> Result<T, crate::dto::error::DartImError>,
    ) -> Result<T, crate::dto::error::DartImError> {
        let guard = self.state.read().map_err(|_| {
            crate::dto::error::DartImError::internal("core lock poisoned")
        })?;
        let inner = guard.inner.as_ref().ok_or_else(|| {
            crate::dto::error::DartImError::object_closed("DartImCore")
        })?;
        f(inner)
    }
}
```

`DartImClient` 同理。Realtime session / runner lifecycle 不在本次实现范围内。

#### 6.2 blocking 调用线程模型

`im-core` 当前是 blocking-first。所有可能触发 IO / SQLite / HTTP 的 facade 函数不得在 Dart synchronous mode 下调用。

要求：

```text
Rust facade API 使用普通 Rust fn 即可，但 generated Dart API 使用 async Future。
不要给这些函数加 FRB sync 标记。
Dart wrapper 的 public API 一律返回 Future<T>。
禁止在 Widget build 等同步路径调用 native blocking 方法。
```

如果 Codex 发现 FRB 生成结果把某个 blocking 方法生成为 sync Dart API，应调整 FRB 注解或 wrapper，使其对 App 仍是 `Future<T>`。

#### 6.3 错误跨 isolate / worker 传递

所有跨边界错误统一为 `DartImError`。

要求：

```text
不要跨 FFI 传递 anyhow::Error。
不要让 panic 跨 FFI 边界。
panic 应通过 std::panic::catch_unwind 或 FRB 默认错误机制转成 internal_error。
Dart wrapper 接收到 generated exception 后，统一包装为 AwikiImCoreException。
```

---

### Step 7：实现 Rust API facade

#### 7.1 core API

`api/core.rs`：

```rust
use std::sync::{Arc, RwLock};

pub fn open_core(
    config: crate::dto::config::DartImCoreConfig,
    paths: crate::dto::config::DartImCorePaths,
) -> Result<Arc<DartImCore>, crate::dto::error::DartImError> {
    let default_service_did = config.anp_service_did.clone();
    let inner = im_core::ImCore::new(config.try_into()?, paths.try_into()?)
        .map_err(crate::dto::error::DartImError::from)?;

    Ok(Arc::new(DartImCore {
        state: Arc::new(RwLock::new(DartImCoreState {
            inner: Some(inner),
            default_service_did,
        })),
    }))
}

pub fn close_core(core: Arc<DartImCore>) -> Result<(), crate::dto::error::DartImError> {
    let mut guard = core.state.write().map_err(|_| {
        crate::dto::error::DartImError::internal("core lock poisoned")
    })?;
    guard.inner = None;
    Ok(())
}

pub fn validate_paths(
    core: Arc<DartImCore>,
) -> Result<Vec<String>, crate::dto::error::DartImError> {
    core.with_inner(|inner| {
        let report = inner.bootstrap().validate_paths()
            .map_err(crate::dto::error::DartImError::from)?;
        Ok(format_path_report(report))
    })
}
```

`format_path_report` 用稳定字符串列表输出即可，避免先暴露复杂 path report DTO。

#### 7.2 client API

`api/client.rs`：

```rust
pub fn core_client(
    core: Arc<crate::api::core::DartImCore>,
    selector: crate::dto::identity::DartIdentitySelector,
) -> Result<Arc<DartImClient>, crate::dto::error::DartImError> {
    let default_service_did = core.default_service_did()?;
    core.with_inner(|inner| {
        let client = inner.client(selector.try_into()?)
            .map_err(crate::dto::error::DartImError::from)?;
        Ok(Arc::new(DartImClient::new(client, default_service_did)))
    })
}

pub fn close_client(client: Arc<DartImClient>) -> Result<(), crate::dto::error::DartImError> {
    client.close()
}

pub fn current_identity(
    client: Arc<DartImClient>,
) -> Result<crate::dto::identity::DartIdentitySummary, crate::dto::error::DartImError> {
    client.with_inner(|inner| Ok(inner.current_identity().into()))
}
```

#### 7.3 identity API

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

如果某个 im-core registration API 当前字段与 Dart DTO 不完全一致，Codex 应根据 `crates/im-core/src/identity/dto.rs` 的实际 public DTO 调整 mapping，不得 invent 字段。

#### 7.4 auth API

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

映射到：

```text
AuthStatus -> DartAuthStatus
SessionBundle -> DartSessionBundle
SessionUpdate -> DartSessionUpdate
```

#### 7.5 profile / directory API

`api/profile.rs`：

```text
load_my_profile(client)
update_profile(client, patch)
load_public_profile(client, subject)
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
follow(client, peer)
unfollow(client, peer)
relationship_status(client, peer)
list_followers(client, query)
list_following(client, query)
```

```text
follow -> client.inner.directory().follow(...)
unfollow -> client.inner.directory().unfollow(...)
relationship_status -> client.inner.directory().relationship_status(...)
list_followers -> client.inner.directory().followers(...)
list_following -> client.inner.directory().following(...)
```

如果某个具体 Dart facade 切片尚未绑定上述 Rust API，临时 unsupported 必须只标注 facade 缺口，例如：

```rust
Err(DartImError::unsupported("relationship-facade-binding"))
```

不要在 `im-core-dart` 中重新写 HTTP RPC。

#### 7.6 message API

`api/messages.rs`：

```text
send_text(client, request)
inbox(client, limit, cursor, unread_only)
history(client, thread, limit, cursor)
mark_read(client, message_ids)
conversations(client, limit, include_groups, include_direct, unread_only)
retry_message(client, message_id)
```

实现：

```text
send_text -> client.inner.messages().send(...)
inbox -> client.inner.messages().inbox(...)
history -> client.inner.messages().history(...)
mark_read -> client.inner.messages().mark_read(...)
conversations -> client.inner.messages().conversations(...)
retry_message -> UnsupportedCapability("message-retry")
```

`retry_message` 规则：

```rust
pub fn retry_message(
    _client: Arc<DartImClient>,
    _message_id: String,
) -> Result<crate::dto::message::DartSendMessageResult, crate::dto::error::DartImError> {
    Err(crate::dto::error::DartImError::unsupported("message-retry"))
}
```

禁止从 `DartMessage` 重建 `SendTextRequest`。展示用 message DTO 会丢失 target、body type、security、client idempotency、metadata/retry plan，存在重复发送或发错对象风险。只有当 `im-core` public API 增加正式 `messages().retry(...)` 后，才能改为真正实现。

#### 7.7 group API

`api/groups.rs`：

```text
create_group(client, request)
join_group(client, group_did)
get_group(client, group_did)
list_groups(client, limit)
list_group_members(client, group_did, limit)
list_group_messages(client, group_did, limit, cursor)
leave_group(client, group_did)
get_group_join_code(client, group_did)
refresh_group_join_code(client, group_did)
```

直接调用 `client.inner.groups()` 中已有 public API。

`create_group` 的 `service_did` 必须这样解析：

```rust
let service_did = request.service_did
    .clone()
    .or_else(|| client.default_service_did())
    .ok_or_else(|| DartImError::invalid_input(
        Some("service_did".to_string()),
        "group create requires service_did or ImCoreConfig.anp_service_did",
    ))?;

let core_request = im_core::groups::GroupCreateRequest {
    name: request.name,
    description: request.description,
    discoverability: request.discoverability,
    admission_mode: request.admission_mode,
    message_security_profile: request.message_security_profile,
    e2ee: request.e2ee,
    slug: request.slug,
    goal: request.goal,
    rules: request.rules,
    message_prompt: request.message_prompt,
    doc_url: request.doc_url,
    attachments_allowed: request.attachments_allowed,
    max_members: request.max_members,
    member_max_messages: request.member_max_messages,
    member_max_total_chars: request.member_max_total_chars,
    service_did: im_core::ids::Did::parse(service_did)?,
};
```

`get_group_join_code` / `refresh_group_join_code` 首版：

```rust
pub fn get_group_join_code(...) -> Result<Option<String>, DartImError> {
    Ok(None)
}

pub fn refresh_group_join_code(...) -> Result<Option<String>, DartImError> {
    Ok(None)
}
```

#### 7.8 realtime API

本次延后 realtime/WebSocket runtime 接入。`im-core` 可以继续保留已有 `ImClient::realtime()`、runner/event 类型，但 Flutter SDK v0.1 不暴露可工作的 runner/connect。

`api/realtime.rs` 首版只提供：

```text
realtime_capability(client) -> DartRealtimeCapability {
  status_supported: true/false depending on im-core status availability
  connect_supported: false
  runner_exposed: false
  reason: "Dart SDK v0.1 does not expose realtime runner yet"
}

realtime_status(client) -> call client.inner.realtime().status() if public API compiles
                         -> otherwise stable not_exposed status

realtime_connect(...) -> UnsupportedCapability("realtime-runner")
```

Rust facade 示例：

```rust
pub fn realtime_connect(
    _client: Arc<DartImClient>,
) -> Result<(), crate::dto::error::DartImError> {
    Err(crate::dto::error::DartImError::unsupported("realtime-runner"))
}
```

未来真正启用 realtime bridge 时，WebSocket 仍只能作为 `im-core` internal transport。Dart SDK / `awiki-me` 只应消费高层 `RealtimeSession` / `Stream<RealtimeEvent>`，不直接依赖 WebSocket、raw frame、ping/pong、request id 或 pending queue。

验收必须覆盖：

```text
realtime_capability.connect_supported == false
realtime_capability.runner_exposed == false
realtime_status 可以调用
realtime_connect 返回 UnsupportedCapability("realtime-runner")
WebSocket 字符串、raw frame、ping/pong、request id 不出现在 Dart public API
```

`awiki-me` 当前 Dart WebSocket gateway 未来可以继续作为 fallback。真正把 realtime runner 接入 Flutter 需要单独处理 stream、shutdown、lifecycle、reconnect、App background/foreground，不在本次计划中完成。

---

### Step 8：新增 Flutter package：`packages/awiki_im_core`

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
```

如果 Flutter 3.24 对 `ffiPlugin: true` 的平台配置有差异，Codex 应以：

```bash
flutter create --template=plugin_ffi temp_awiki_im_core_plugin
```

生成的结构为准，然后迁移到 `packages/awiki_im_core`。

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
export 'src/models/realtime.dart';
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
    throw UnsupportedError(
      'awiki_im_core native Rust backend is not supported on Flutter Web.',
    );
  }

  Future<void> dispose() async {}
}
```

创建 native facade `lib/src/awiki_im_core_native.dart`，包装 generated FRB API，并对 App 暴露稳定 Dart API：

```dart
class AwikiImCore {
  AwikiImCore._(this._inner);

  final Object _inner;
  bool _disposed = false;

  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    // initialize native library
    // call generated openCore
    // return AwikiImCore._(inner)
    throw UnimplementedError();
  }

  Future<AwikiImClient> client(IdentitySelector selector) async {
    _ensureNotDisposed();
    // call generated coreClient
    throw UnimplementedError();
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    // call generated closeCore
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw AwikiImCoreException(code: 'object_closed', message: 'core disposed');
    }
  }
}

class AwikiImClient {
  AwikiImClient._(this._inner);

  final Object _inner;
  bool _disposed = false;

  AuthApi get auth => AuthApi._(this);
  IdentityApi get identity => IdentityApi._(this);
  DirectoryApi get directory => DirectoryApi._(this);
  MessageApi get messages => MessageApi._(this);
  GroupApi get groups => GroupApi._(this);
  RealtimeApi get realtime => RealtimeApi._(this);

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    // call generated closeClient
  }
}

class RealtimeApi {
  RealtimeApi._(this._client);

  final AwikiImClient _client;

  Future<RealtimeStatus> status() async {
    _client._ensureNotDisposed();
    // call generated realtimeStatus
    throw UnimplementedError();
  }

  Future<void> connect() async {
    _client._ensureNotDisposed();
    // call generated realtimeConnect, which returns unsupported_capability("realtime-runner")
    throw AwikiImCoreException(
      code: 'unsupported_capability',
      message: 'unsupported capability: realtime-runner',
      capability: 'realtime-runner',
    );
  }
}
```

Dart model 文件应与 Rust DTO 同名同义，但可以使用 camelCase 字段。Realtime Dart public API 不得出现 `WebSocket`、`ws://`、`wss://`、raw frame、ping/pong、request id 等 transport 概念。`RealtimeSession` / `Stream<RealtimeEvent>` 不在本次实现范围内。

验收：

```bash
cd packages/awiki_im_core
flutter pub get
dart analyze
flutter test
```

---

### Step 9：生成 flutter_rust_bridge 绑定

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

### Step 10：generated 文件策略

本计划要求 generated files 入库：

```text
crates/im-core-dart/src/frb_generated.rs
packages/awiki_im_core/lib/src/generated/bridge_generated.dart
```

原因：

```text
让 package checkout 后可 analyze / build。
避免用户必须先安装 codegen 才能使用 SDK。
让 CI 能检查 generated 文件是否过期。
```

创建 `scripts/flutter/codegen-check.sh`：

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

scripts/flutter/codegen.sh

git diff --exit-code -- \
  crates/im-core-dart/src/frb_generated.rs \
  packages/awiki_im_core/lib/src/generated/bridge_generated.dart
```

`xtask check-structure` 风险处理：

```text
如果 crates/im-core-dart/src/frb_generated.rs 超过 1200 行：
  在 docs/file-size-exceptions.md 增加一行，说明该文件为 generated FRB glue，不手工维护。
  不修改 xtask 规则本身。
```

示例 exception：

```markdown
| Rust path | Reason | Owner |
| --- | --- | --- |
| `crates/im-core-dart/src/frb_generated.rs` | Generated flutter_rust_bridge glue; stale checked by scripts/flutter/codegen-check.sh. | Flutter SDK |
```

验收：

```bash
scripts/flutter/codegen-check.sh
cargo +1.79.0 run -p xtask --locked -- check-structure
```

---

### Step 11：native library loader

创建 `packages/awiki_im_core/lib/src/native_library_loader.dart`。

职责：

```text
Android -> DynamicLibrary.open('libawiki_im_core.so')
macOS -> DynamicLibrary.open('libawiki_im_core.dylib') or process/executable depending on plugin template
iOS -> DynamicLibrary.process() when statically linked through podspec/xcframework
Windows -> unsupported in v0.1
```

示例：

```dart
import 'dart:ffi';
import 'dart:io';

DynamicLibrary loadAwikiImCoreLibrary() {
  if (Platform.isAndroid) {
    return DynamicLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isMacOS) {
    return DynamicLibrary.open('libawiki_im_core.dylib');
  }
  if (Platform.isIOS) {
    return DynamicLibrary.process();
  }
  if (Platform.isWindows) {
    throw UnsupportedError('Windows is not supported by awiki_im_core v0.1.');
  }
  throw UnsupportedError('Unsupported platform for awiki_im_core native library.');
}
```

Codex 应按 FRB 2.x 生成代码要求，将 loader 接入 generated API 初始化位置。若 FRB starter / generated API 对 loader 有不同约定，以 generated API 为准，但必须保留 native loader 文件和平台分支。

---

### Step 12：Android 构建脚本

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

### Step 13：Apple 构建脚本与 podspec 闭环

首版 Apple 推荐使用 XCFramework，而不是散放多个 `.a` 文件。这样 iOS device / simulator / macOS universal 的选择由 Xcode 处理，Flutter plugin 的 podspec 也更清晰。

创建 `packages/awiki_im_core/ios/include/awiki_im_core.h`：

```c
#pragma once
// The FFI symbols are generated by flutter_rust_bridge.
// This header exists so xcodebuild can create a static-library XCFramework.
```

macOS 同理：

```text
packages/awiki_im_core/macos/include/awiki_im_core.h
```

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
IOS_INCLUDE_DIR="${ROOT_DIR}/packages/awiki_im_core/ios/include"
MACOS_INCLUDE_DIR="${ROOT_DIR}/packages/awiki_im_core/macos/include"

IOS_XCFRAMEWORK="${IOS_FRAMEWORK_DIR}/AwikiImCore.xcframework"
MACOS_XCFRAMEWORK="${MACOS_FRAMEWORK_DIR}/AwikiImCore.xcframework"

TARGETS=(
  aarch64-apple-ios
  aarch64-apple-ios-sim
  x86_64-apple-ios
  aarch64-apple-darwin
  x86_64-apple-darwin
)

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would build staticlibs and create iOS/macOS XCFrameworks"
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

mkdir -p "${IOS_FRAMEWORK_DIR}" "${MACOS_FRAMEWORK_DIR}" "${IOS_INCLUDE_DIR}" "${MACOS_INCLUDE_DIR}"

cat > "${IOS_INCLUDE_DIR}/awiki_im_core.h" <<'HEADER'
#pragma once
HEADER

cat > "${MACOS_INCLUDE_DIR}/awiki_im_core.h" <<'HEADER'
#pragma once
HEADER

SIM_DIR="$(mktemp -d "${TMPDIR:-/tmp}/awiki-ios-sim.XXXXXX")"
MACOS_DIR="$(mktemp -d "${TMPDIR:-/tmp}/awiki-macos.XXXXXX")"

lipo -create \
  "target/aarch64-apple-ios-sim/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-ios/release/lib${LIB_NAME}.a" \
  -output "${SIM_DIR}/lib${LIB_NAME}.a"

lipo -create \
  "target/aarch64-apple-darwin/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-darwin/release/lib${LIB_NAME}.a" \
  -output "${MACOS_DIR}/lib${LIB_NAME}.a"

rm -rf "${IOS_XCFRAMEWORK}" "${MACOS_XCFRAMEWORK}"

xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/release/lib${LIB_NAME}.a" \
  -headers "${IOS_INCLUDE_DIR}" \
  -library "${SIM_DIR}/lib${LIB_NAME}.a" \
  -headers "${IOS_INCLUDE_DIR}" \
  -output "${IOS_XCFRAMEWORK}"

xcodebuild -create-xcframework \
  -library "${MACOS_DIR}/lib${LIB_NAME}.a" \
  -headers "${MACOS_INCLUDE_DIR}" \
  -output "${MACOS_XCFRAMEWORK}"
```

创建 `packages/awiki_im_core/ios/awiki_im_core.podspec`：

```ruby
Pod::Spec.new do |s|
  s.name             = 'awiki_im_core'
  s.version          = '0.1.0'
  s.summary          = 'Awiki IM Core Flutter SDK'
  s.description      = 'Flutter FFI bindings for Rust im-core.'
  s.homepage         = 'https://github.com/AgentConnect/awiki-cli-rs2'
  s.license          = { :type => 'MIT' }
  s.author           = { 'AgentConnect' => 'dev@awiki.ai' }
  s.source           = { :path => '.' }
  s.platform         = :ios, '12.0'
  s.source_files     = 'Classes/**/*'
  s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'
  s.pod_target_xcconfig = {
    'OTHER_LDFLAGS' => '$(inherited) -force_load $(PODS_TARGET_SRCROOT)/Frameworks/AwikiImCore.xcframework/ios-arm64/libawiki_im_core.a'
  }
end
```

创建 `packages/awiki_im_core/macos/awiki_im_core.podspec`：

```ruby
Pod::Spec.new do |s|
  s.name             = 'awiki_im_core'
  s.version          = '0.1.0'
  s.summary          = 'Awiki IM Core Flutter SDK'
  s.description      = 'Flutter FFI bindings for Rust im-core.'
  s.homepage         = 'https://github.com/AgentConnect/awiki-cli-rs2'
  s.license          = { :type => 'MIT' }
  s.author           = { 'AgentConnect' => 'dev@awiki.ai' }
  s.source           = { :path => '.' }
  s.platform         = :osx, '10.14'
  s.source_files     = 'Classes/**/*'
  s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'
end
```

如果 `-force_load` 路径因 XCFramework slice 名称不同而失败，Codex 应按 `xcodebuild -create-xcframework` 生成的 `Info.plist` slice 路径修正 podspec。目标是保证 iOS static symbols 能被链接进 app executable，使 `DynamicLibrary.process()` 能解析 FRB symbols。

验收：

```bash
scripts/flutter/build-apple.sh --dry-run
```

完整构建验收必须在 macOS runner 上执行。

---

### Step 14：host 构建脚本

创建 `scripts/flutter/build-host.sh`：

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

### Step 15：总构建脚本

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

注意：`build-all.sh` 默认只做 Android/Apple dry-run，避免非 Android/Apple 环境失败。完整平台构建交给 CI matrix 或人工平台验证。

---

### Step 16：Flutter package tests

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

  test('web/native API exposes disposable core type', () {
    expect(AwikiImCore, isNotNull);
  });
}
```

创建 Rust facade tests：`crates/im-core-dart/tests/facade_contract.rs`：

```rust
#[test]
fn dart_error_unsupported_has_stable_code() {
    let err = im_core_dart::dto::error::DartImError::unsupported("relationship-facade-binding");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("relationship-facade-binding"));
}

#[test]
fn retry_message_is_explicitly_unsupported_until_im_core_has_retry_api() {
    let err = im_core_dart::dto::error::DartImError::unsupported("message-retry");
    assert_eq!(err.code, "unsupported_capability");
    assert_eq!(err.capability.as_deref(), Some("message-retry"));
}

#[test]
fn realtime_connect_is_explicitly_unsupported_until_bridge_plan_is_ready() {
    let capability = im_core_dart::dto::realtime::DartRealtimeCapability {
        status_supported: true,
        connect_supported: false,
        runner_exposed: false,
        reason: Some("Dart SDK v0.1 does not expose realtime runner yet".to_string()),
    };
    assert!(!capability.connect_supported);
    assert!(!capability.runner_exposed);
}
```

验收：

```bash
cargo +1.79.0 test -p im-core-dart
cd packages/awiki_im_core && flutter test
```

---

### Step 17：文档

创建 `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`，内容包括：

```text
SDK 分层
通用 im-core Flutter SDK 定位
为什么不是 awiki-me adapter
支持平台
不支持 Web native 的原因
opaque object / dispose / blocking thread pool 说明
Realtime/WebSocket ownership：本次延后 runtime bridge；未来 WebSocket 仍应是 im-core internal transport
如何 codegen
generated files 入库和 codegen-check
如何构建 Android/iOS/macOS
group create service_did 规则
retry_message 为什么 unsupported
realtime connect/session/events 为什么延后
Windows 不在 v0.1 native 支持范围内
常见错误：找不到 anp sibling checkout、cargo-ndk 未安装、iOS staticlib 未链接
```

创建 `docs/flutter-sdk/awiki-me-future-integration.md`，只描述未来 `awiki-me` 需要怎么接，不实际修改仓库。

未来 `awiki-me` 集成建议：

```yaml
dependencies:
  awiki_im_core:
    path: ../awiki-cli-rs2/packages/awiki_im_core
```

未来 `AppBootstrap` 切换建议：

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

### Step 18：可选 CI workflow

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
          rustup toolchain install 1.78.0 --profile minimal
          rustup toolchain install 1.79.0 --profile minimal
          cargo +1.78.0 check -p im-core --locked
          cargo +1.79.0 check -p im-core-dart --locked
          cargo +1.79.0 test -p im-core-dart --locked
          scripts/flutter/codegen-check.sh
          cargo +1.79.0 run -p xtask --locked -- check-structure

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

如果 `Cargo.lock` 更新后 `--locked` 不适用，Codex 应先更新 lockfile 并提交，然后恢复 `--locked`。

---

## 4. Codex 一次性执行 Goal

把下面内容作为 Codex goal 使用：

```text
Goal: In the awiki-cli-rs2 repository, on the branch that contains crates/im-core, add a general-purpose Flutter/Dart SDK package for im-core without modifying the awiki-me repository.

Requirements:
1. Keep crates/im-core as a pure Rust SDK. Do not add Flutter, Dart, FFI, or platform packaging logic to crates/im-core.
2. Add crates/im-core-dart as a Rust facade crate depending on im-core. Its lib name must be awiki_im_core and crate-type must include cdylib, staticlib, and rlib.
3. packages/awiki_im_core must be a general im-core Flutter SDK, not an awiki-me adapter. Do not import awiki-me types or encode awiki-me UI/cache DTOs such as ChatMessage or ConversationSummary into the SDK public DTO.
4. Add DTO, mapping, and API modules under crates/im-core-dart. DTOs must follow im-core public DTO semantics:
   - AuthStatus: subject, has_session, expires_at, needs_refresh, warnings.
   - SessionBundle: subject, scope, expires_at, refreshed.
   - GroupCreateRequest must resolve service_did from request.service_did or ImCoreConfig.anp_service_did; if missing, return invalid_input(field = service_did).
5. Add explicit opaque object lifecycle:
   - DartImCore and DartImClient are FRB opaque Rust objects.
   - Expose close/dispose.
   - Calls after close return DartImError code object_closed.
   - Blocking im-core calls are exposed as Future-returning Dart APIs, not sync Dart calls.
6. Message API must include send_text, inbox, history, mark_read, conversations. retry_message must return unsupported_capability("message-retry") until im-core exposes a real messages().retry API. Do not rebuild SendTextRequest from DartMessage.
7. Realtime/WebSocket runtime bridge is deferred:
   - Dart SDK v0.1 may expose capabilities/status only.
   - realtime connect must return unsupported_capability("realtime-runner").
   - Do not implement WebSocket transport in packages/awiki_im_core or im-core-dart.
   - Do not expose WebSocket, ws/wss URLs, raw frames, ping/pong, request id, pending queue, RealtimeSession, or Stream<RealtimeEvent> in the Dart public API.
   - Future realtime bridge work should keep WebSocket as im-core internal transport and expose only high-level session/events.
8. Add packages/awiki_im_core as a Flutter package compatible with Dart >=3.8.0 and Flutter >=3.24.0. Use flutter_rust_bridge 2.x and dart:ffi. Do not rely exclusively on Flutter 3.38+ package_ffi build hooks.
9. Add a web stub so the package can be analyzed when imported by a Flutter project that also has web support. The stub must throw UnsupportedError at runtime.
10. Add scripts/flutter/codegen.sh, codegen-check.sh, build-host.sh, build-android.sh, build-apple.sh, and build-all.sh. Android and Apple platform build scripts must support --dry-run. Do not add Windows build support in v0.1.
11. Generated files must be committed:
    - crates/im-core-dart/src/frb_generated.rs
    - packages/awiki_im_core/lib/src/generated/bridge_generated.dart
    Add scripts/flutter/codegen-check.sh and docs/file-size-exceptions.md entry if frb_generated.rs exceeds the xtask line limit.
12. Add minimal Rust and Flutter tests.
13. Add docs/flutter-sdk/awiki-im-core-flutter-sdk.md and docs/flutter-sdk/awiki-me-future-integration.md.
14. Do not modify awiki-me. Do not change the current awiki-cli release workflow unless required for workspace correctness.
15. Keep workspace rust-version = 1.78 unless flutter_rust_bridge has no compatible 2.x version. CI may use 1.79.0 as validation toolchain, but MSRV changes must be explicitly documented.

Acceptance:
- cargo metadata succeeds.
- cargo +1.79.0 check -p im-core-dart succeeds.
- cargo +1.79.0 test -p im-core-dart succeeds.
- packages/awiki_im_core/pubspec.yaml exists and flutter pub get succeeds when Flutter is available.
- dart analyze succeeds for packages/awiki_im_core when Flutter is available.
- Realtime capability reports connect_supported = false and runner_exposed = false.
- realtime connect returns unsupported_capability("realtime-runner").
- Realtime Dart public API does not expose RealtimeSession / Stream<RealtimeEvent> / WebSocket transport details.
- scripts/flutter/codegen-check.sh succeeds when codegen tool is available.
- scripts/flutter/build-android.sh --dry-run succeeds.
- cargo +1.79.0 run -p xtask --locked -- check-structure succeeds.
- No file in awiki-me is modified.
```

---

## 5. 后续 awiki-me 集成方向，不在本次执行范围

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

未来 `awiki-me` adapter 可做这些 mapping：

```text
DartMessage -> awiki-me ChatMessage
DartConversation -> awiki-me ConversationSummary
DartGroupSummary / DartGroupSnapshot -> awiki-me GroupSummary
DartUserProfile -> awiki-me UserProfile
DartImError -> awiki-me UI error state
```

这些 mapping 不应反向污染 `packages/awiki_im_core` 的公共 DTO。

---

## 6. 风险与处理

### 6.1 Flutter Web

`dart:ffi` 不支持 Web native library。当前 package 必须提供 web stub，不能承诺 Web 使用 Rust backend。

### 6.2 SQLite 双缓存

`awiki-me` 当前用 `sqflite` 做 local cache，`im-core` 默认也启用 `sqlite`。未来切换时应选择一个 source of truth。建议未来 Rust backend 模式下以 `im-core` local state 为准，`awiki-me` 的 `AwikiLocalCache` 逐步退化为 UI cache 或删除。

### 6.3 anp sibling dependency

`awiki-cli-rs2` 当前 workspace 依赖 sibling path `../anp/anp/rust`。CI 和本地构建必须 checkout sibling `anp`。后续发布 Flutter SDK 时，应评估把 `anp` 改成 pinned git dependency 或 vendored submodule。

### 6.4 flutter_rust_bridge 版本

首选使用 `2.12.0`。如果与 Rust 1.78/1.79 不兼容，Codex 应 pin 到能通过 workspace 检查的最新 2.x 版本，并在文档中记录。不要静默提升 workspace MSRV。

### 6.5 iOS static linking

首版 iOS 用 staticlib XCFramework。Dart 端通过 `DynamicLibrary.process()` 解析符号。podspec 必须声明 `vendored_frameworks`，必要时使用 `OTHER_LDFLAGS` / `-force_load` 保证 FRB symbols 被链接进 executable。

### 6.6 Opaque object 泄漏

Dart wrapper 必须暴露 `dispose()`。Rust side close 后应清空内部 `Option<im_core::ImCore>` / `Option<im_core::ImClient>`。GC 只能作为兜底，不是生命周期策略。

### 6.7 blocking 调用卡 UI

所有 IO/SQLite/HTTP 相关函数必须作为 Dart `Future` 暴露。不要生成或包装成 synchronous Dart API。若 FRB sync mode 被误用，必须移除。

---

## 7. 最终验收命令清单

在 `awiki-cli-rs2` 根目录：

```bash
cargo +1.79.0 metadata --no-deps >/dev/null
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p im-core --locked
cargo +1.79.0 check -p im-core-dart --locked
cargo +1.79.0 test -p im-core-dart --locked
cargo +1.79.0 run -p xtask --locked -- check-structure
bash -n scripts/flutter/codegen.sh
bash -n scripts/flutter/codegen-check.sh
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

完成后应满足：

```text
awiki-cli-rs2 中新增通用 Flutter SDK package
awiki-cli-rs2 中新增 Rust-Dart facade crate
SDK public DTO 不绑定 awiki-me UI/cache DTO
retry_message 明确 unsupported，不伪造重发
group create service_did 有确定来源
realtime connect/session/events 延后，connect 明确 unsupported
WebSocket runtime 不在本次范围内，也不暴露给 Dart SDK / awiki-me
generated files 入库且有 codegen-check
平台构建脚本存在且 dry-run 通过
不修改 awiki-me
im-core 仍为纯 Rust SDK
```
