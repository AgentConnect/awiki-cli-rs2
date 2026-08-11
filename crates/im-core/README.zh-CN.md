# awiki-im-core 中文说明

> 英文版为默认 README： [README.md](./README.md)。

> 本文是 `awiki-im-core` README 的中文版本，方便 AWiki / ANP 协作者快速理解 crate 边界、配置方式和发布前检查项。

## awiki-im-core 是什么

`awiki-im-core` 是 Awiki 客户端栈复用的 Rust IM SDK。它承载 Awiki CLI、Flutter/Dart SDK 桥接层以及原生应用集成共同依赖的产品能力，包括身份、DID-WBA 认证、消息、群组、附件、实时通知、E2EE-ready 安全消息、本地状态、邮件以及内容/站点 API。

这个 crate 是 **产品 SDK**，不是 CLI helper。宿主应用负责构造 `ImCore`，再把某个身份绑定为 `ImClient`，然后调用 `messages()`、`groups()`、`directory()`、`realtime()` 等高层服务。

## 状态

- 当前版本：`0.1.0`。
- Rust 工具链：Rust `1.88.0` 或更高版本，与工作区 `rust-toolchain.toml` 保持一致。
- Public API 仍在演进中，目标是把 Awiki 客户端栈拆分成稳定 Rust SDK、FFI facade 和 Flutter/Dart bindings。
- 原生 Rust 宿主可以直接使用本 crate；Flutter/Dart 应用通常应使用 [`packages/awiki_im_core`](../../packages/awiki_im_core)。

## 与 ANP 的关系

Awiki IM 基于 Agent Network Protocol（ANP）的身份、DID-WBA 认证、proof、服务互操作和安全通信能力构建。`awiki-im-core` 依赖 Rust `anp` SDK 处理底层协议机制，但对业务调用方暴露 Awiki 产品语义 API，避免应用层直接拼装 ANP wire payload。

ANP 相关链接：

- ANP 官方站点：<https://agent-network-protocol.com/>
- ANP 协议 / 规范仓库：<https://github.com/agent-network-protocol/AgentNetworkProtocol>
- DID-WBA 方法规范：<https://github.com/agent-network-protocol/AgentNetworkProtocol/blob/main/03-did-wba-method-design-specification.md>
- AgentConnect 多语言 ANP SDK 仓库：<https://github.com/agent-network-protocol/AgentConnect>
- Rust `anp` crate：<https://crates.io/crates/anp>
- Rust `anp` 文档：<https://docs.rs/anp>

## 能力范围

| 领域 | Public entry point | 用途 |
| --- | --- | --- |
| Core lifecycle | `ImCore`, `CoreBootstrap` | 打开运行环境、校验路径、初始化和迁移本地状态。 |
| Identity | `IdentityRegistry`, `IdentityService` | 列出/选择本地身份，注册或恢复 handle，管理 profile 和 identity vault 状态。 |
| Authentication | `AuthService` | DID-WBA 会话状态、刷新和认证作用域操作。 |
| Directory | `DirectoryService` | 解析 handle/DID、读取公开 profile、联系人和关系状态。 |
| Messages | `MessageService` | 发送私聊/群聊消息，读取 inbox/history，本地优先会话，mark-read，同步和 runtime patch。 |
| Groups | `GroupService` | 群组生命周期、成员、策略/profile 更新、群组读取和群组 E2EE hook。 |
| Attachments | `AttachmentService` | 附件上传、加密 manifest、发送和下载。 |
| Secure messaging | `SecureService` | 私聊安全状态、群组安全状态、prepare/repair 和 secure outbox。 |
| Realtime | `RealtimeService` | WebSocket 会话状态、订阅、标准化事件和宿主通知事件。 |
| Email | `EmailService` | 邮件账号、收件箱、读取、发送、mark-read、附件和通知。 |
| Content/site | `ContentService`, `SiteService` | Awiki 页面/内容和站点操作。 |

## crate 边界

固定依赖方向：

```text
awiki-cli      -> awiki-im-core
im-core-dart   -> awiki-im-core
awiki_im_core  -> im-core-dart native library
awiki-me       -> awiki_im_core
```

`awiki-im-core` 负责产品行为和本地状态，不负责：

- CLI 参数解析、终端输出、退出码或 workspace 自动发现。
- Flutter widget 状态、App 展示 DTO 或 UI cache model。
- Agent stdout/stderr envelope。
- 服务安装、daemon 进程管理或 OS 特定 UX。
- 通用 ANP 协议规范工作；这些属于 ANP / AgentConnect。

## 安装

发布到 crates.io 后，Rust 项目可这样依赖：

```toml
[dependencies]
awiki-im-core = "0.1"
```

本地工作区开发可使用 path dependency：

```toml
[dependencies]
awiki-im-core = { path = "../awiki-cli-rs2/crates/im-core" }
```

如果希望 Rust 代码中的 import 名保持 `im_core`，可以使用 Cargo dependency renaming：

```toml
[dependencies]
im-core = { package = "awiki-im-core", version = "0.1" }
```

启用额外能力：

```toml
[dependencies]
awiki-im-core = { version = "0.1", features = ["group-e2ee", "realtime", "attachments"] }
```

## Feature flags

| Feature | 默认 | 说明 |
| --- | --- | --- |
| `sqlite` | 是 | 通过 `rusqlite` 启用 SQLite 本地状态。 |
| `http` | 是 | 启用 HTTP/RPC transport 相关流程。 |
| `blocking` | 否 | 启用部分同步 helper，供阻塞式宿主使用。 |
| `attachments` | 否 | 附件上传/下载和消息附件 helper。 |
| `realtime` | 否 | Realtime/WebSocket 宿主集成能力。 |
| `secure-direct` | 否 | 私聊 E2EE 安全消息能力。 |
| `group-e2ee` | 否 | 群组 E2EE/MLS 能力；同时启用 `sqlite` 和 ANP `mls` feature。 |
| `email` | 否 | 邮件产品命名空间。 |
| `provider-traits` | 否 | 预留 provider 扩展点。 |
| `mcp-trusted-registration` | 否 | 内部 trusted-registration 集成面。 |
| `internal-test-helpers` | 否 | 测试专用 helper；下游生产构建不要启用。 |

默认 feature set 是 `sqlite` + `http`。

## 最小 Rust 用法

完整代码示例见英文部分的 [Minimal Rust example](#minimal-rust-example)。核心流程如下：

1. 构造 `ImCoreConfig`，显式传入 `service_base_url` 和 `did_domain`。
2. 构造 `ImCorePaths`，显式传入 identity、本地 SQLite、cache 和 temp 路径。
3. 调用 `ImCore::open(...)` 打开 SDK。
4. 通过 `core.bootstrap()` 校验路径并初始化/迁移本地状态。
5. 使用 `core.client_async(IdentitySelector::Default)` 绑定默认身份。
6. 调用 `client.messages().send_async(...)`、`client.groups()`、`client.directory()` 等业务服务。

示例假定 `IdentitySelector::Default` 已能在配置的 identity 路径下解析到本地身份。新宿主可以通过 `core.identities().register_handle_async(...)` 创建身份，或通过 recovery API 恢复已有 handle。

## 配置模型

宿主通过 `ImCoreConfig` 显式传入环境配置：

- `service_base_url`：Awiki 服务基础地址，例如 `https://awiki.info`。
- `did_domain`：默认 DID / handle 域名，例如 `awiki.info`。
- 可选服务覆盖：`user_service_endpoint`、`message_service_endpoint`、`mail_service_endpoint`、`anp_service_endpoint`。
- 可选 `anp_service_did`，用于需要明确 ANP service DID 的流程。
- 可选 `ca_bundle`，用于自定义 TLS trust root。
- `transport_policy`：`Auto`、`HttpOnly` 或 `RealtimePreferred`。

SDK 不会自行发现 CLI workspace，也不会直接读取 App 配置文件。CLI、Flutter、daemon 和测试宿主必须自己解析配置并把规范化结果传入 `awiki-im-core`。

## 存储和路径

宿主通过 `ImCorePaths` 显式传入所有存储路径：

- `IdentityRegistryPaths.identity_root_dir`：本地身份目录。
- `IdentityRegistryPaths.registry_path`：identity registry JSON 文件。
- `IdentityRegistryPaths.default_identity_path`：可选默认身份 marker。
- `LocalStatePaths.sqlite_path`：SQLite 本地投影和同步状态。
- `RuntimePaths.cache_dir`：cache 和 snapshot 数据。
- `RuntimePaths.temp_dir`：临时文件和传输 staging。

`CoreBootstrap` 可以校验路径并初始化/迁移 SQLite 本地状态。目录创建策略、文件权限、备份、清理和迁移时机仍由宿主负责。

## 身份密钥存储

默认情况下，`ImCore::new` / `ImCore::open` 使用 `IdentitySecretStoragePolicy::FileCompat`，兼容现有 identity 目录。生产 App 宿主应显式传入 SecretVault options，并优先使用 `VaultRequired`。

安全规则：

- 不要记录 root key、private key、JWT、bearer token、raw `SecretRef`、ciphertext internals 或 MLS private state。
- `VaultRequired` 是 fail-closed：宿主不能提供有效 vault context 时，SDK open 失败，而不是静默回退到明文。
- status、migration 和 verification API 只暴露脱敏 metadata 与 warning。

## Realtime 和本地优先读取

`awiki-im-core` 把已提交的本地投影视为快速 UI 读取的持久真相源。conversation snapshot 和 patch stream 是 SQLite 状态上的加速层，不是第二套真相源。

典型 App 流程：

1. 用显式 config 和 paths 打开 `ImCore`。
2. 用 `core.client_async(IdentitySelector::Default)` 绑定身份。
3. 首屏使用 `client.messages().conversations_async(...)` 或 local timeline API。
4. transport 可用时启动 `client.realtime()`。
5. 只在 SDK 发出 committed projection 事件之后应用 conversation/message patch。
6. 远端 history/sync API 作为后台 freshness / reconciliation 路径。

## 错误处理

大多数 public API 返回：

```rust
pub type ImResult<T> = Result<T, ImError>;
```

`ImError` 表示 SDK / 领域错误，例如非法输入、缺少身份、认证过期、权限失败、peer/group/message 不存在、本地状态失败、能力不支持、transport 失败、服务端错误或内部错误。CLI 和 App 宿主应把 `ImError` 映射为自己的退出码、UI 文案、telemetry 和重试策略，同时避免暴露任何 secret。

## 开发与发布前检查

在工作区根目录执行：

```bash
cargo test -p awiki-im-core --locked
cargo test --workspace --locked
cargo package -p awiki-im-core --allow-dirty
cargo publish -p awiki-im-core --dry-run
```

发布到 crates.io 时，每个 non-dev dependency 都必须带 crates.io `version`。本地 path dependency 可以保留，但需要同时写上已发布版本，例如：

```toml
anp = { version = "0.9.3", path = "../anp/anp/rust", default-features = false }
```

## 相关文档

- Workspace README: [../../README.md](../../README.md)
- SDK 架构: [../../docs/architecture/im-core-sdk-architecture.md](../../docs/architecture/im-core-sdk-architecture.md)
- Public API 总览: [../../docs/api/im-core-public-api.md](../../docs/api/im-core-public-api.md)
- Interface API 文档: [../../docs/api/im-core-interface/README.md](../../docs/api/im-core-interface/README.md)
- Flutter/Dart SDK 文档: [../../docs/flutter-sdk/awiki-im-core-flutter-sdk.md](../../docs/flutter-sdk/awiki-im-core-flutter-sdk.md)
- Rust-Dart facade crate: [../im-core-dart](../im-core-dart)
- Flutter package: [../../packages/awiki_im_core](../../packages/awiki_im_core)

## License

本 crate 遵循工作区 license 配置，使用 MIT license。
