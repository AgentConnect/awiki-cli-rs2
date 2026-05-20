# SDK Refactor 2：迁移计划

## 1. 迁移原则

- 先建立高层 SDK façade，再移动底层代码。
- 第一阶段只迁移基础能力、私聊文本、群聊文本和群管理。
- 加密、附件、realtime daemon、provider 抽象后移。
- 不改 CLI 命令行为，先改 handler 调用路径。
- 不让 `im-core` 依赖 CLI 类型。
- 不让 SDK public API 暴露 wire/store/crypto/path 细节。
- 每个阶段都要保持 CLI 可编译、主要命令可运行。

## 2. Phase 0：文档与边界确认

目标：确认新方案，避免继续按旧低层 helper 方向抽 SDK。

任务：

- 新增 `docs/sdk-refactor-2/` 文档。
- 明确第一阶段范围：foundation + identity/auth + directory + direct text + group text/lifecycle + local state。
- 明确排除范围：attachments、realtime runner、direct E2EE、group E2EE、provider traits。
- 明确 public/internal deny list。

完成判定：

- 团队认可 `ImCore` / `ImClient` 的高层入口。
- 团队认可 `IdentitySelector::LocalAlias` 替代 CLI `identity_name` 传入业务 request。
- 团队认可 group E2EE 和 secure 命令不决定第一阶段 SDK public API。

## 3. Phase 1A：新增 `crates/im-core` 骨架

目标：新 crate 可独立编译，但暂不大规模搬业务逻辑。

任务：

- 新增 `crates/im-core`。
- workspace 增加成员。
- `crates/awiki-cli/Cargo.toml` 增加 path dependency。
- 在 `im-core` 中定义：
  - `ImCore`
  - `ImClient`
  - `ImCoreConfig`
  - `ImCorePaths`
  - `IdentitySelector`
  - `IdentitySummary`
  - `ImError`
  - `ImResult<T>`
  - messages/groups 基础 DTO
- 增加 compile fence / grep test，禁止 `im-core` 引用：
  - `ParsedCommand`
  - `ExitError`
  - `GlobalOptions`
  - `config::Resolved`
  - `identity::Manager`
  - `awiki_cli::*`

完成判定：

- `cargo test -p im-core` 通过。
- `cargo test -p awiki-cli` 通过。
- `im-core` 没有 CLI 类型引用。

## 4. Phase 1B：CLI adapter + SDK façade

目标：先让 CLI handler 调 SDK façade，但 façade 内部可暂时调用旧模块。

任务：

- 在 CLI 中新增：
  - `build_im_core_config(resolved)`
  - `build_im_core_paths(resolved, manager)`
  - `cli_identity_selector(identity_flag)`
  - `map_im_error(err)`
- `ImCore::client(selector)` 能解析默认身份和 local alias。
- `ImClient` 内部持有 identity summary 和旧模块调用所需 runtime context。
- 第一批 handler 改为调用 SDK façade：
  - `id list/current/status/use`
  - `id refresh-token`

完成判定：

- CLI identity 命令行为不变。
- handler 中不再直接拼低层 identity path，统一通过 adapter。

## 5. Phase 1C：identity/auth 基础能力迁移

目标：把多身份、session、profile 的核心流程下沉到 `im-core`。

任务：

- 迁移或封装：
  - list/default/resolve identity
  - register handle
  - recover handle
  - auth login/ensure/refresh/logout
  - profile get/update
- CLI 保留：
  - OTP 输入
  - identity alias 选择
  - 本地路径布局
  - 文件权限
  - 输出渲染
- `IdentityRegistry::load()` 保持 `pub(crate)`，不返回 runtime paths。

完成判定：

- `id register/recover/refresh-token/profile get/profile set` 最终走 `im-core`。
- public API 不返回 private key、auth path、DID document path。

## 6. Phase 1D：私聊文本迁移

目标：普通 direct text message 进入 SDK。

任务：

- 实现：
  - `client.messages().send(SendMessageRequest { target: Direct, body: Text, security: Plain })`
  - `client.messages().inbox()`
  - `client.messages().history(ThreadRef::Direct)`
  - `client.messages().mark_read()`
  - `client.messages().conversations()`
- 内部处理：
  - handle/DID resolve
  - auth ensure + 401 refresh retry
  - HTTP/RPC params 构造
  - 远端结果转领域 DTO
  - 本地 message/contact/conversation projection
  - owner isolation
- CLI adapter 处理：
  - `--to`
  - `--text`
  - `--text-file`
  - `--limit`
  - `--unread`
  - dry-run

完成判定：

- `msg send --to ... --text ...` 通过 `client.messages().send()`。
- `msg inbox/history/mark-read` 通过 `client.messages()`。
- handler 不再直接调用 `message::send/inbox/history/mark_read`。
- wire params builder 不作为 SDK public API 导出。

## 7. Phase 1E：群聊基础能力迁移

目标：普通 group lifecycle、member、group text message 进入 SDK。

任务：

- 实现：
  - `client.groups().create()`
  - `client.groups().get()`
  - `client.groups().list()`
  - `client.groups().join()`
  - `client.groups().leave()`
  - `client.groups().add_member()`
  - `client.groups().remove_member()`
  - `client.groups().update_profile()` / `update_policy()`
  - `client.groups().members()`
  - `client.groups().messages()`
  - `client.messages().send(MessageTarget::Group)`
- CLI adapter 处理：
  - group profile flags
  - group policy flags
  - member/role/reason flags
  - dry-run

完成判定：

- `group create/get/list/join/leave/add/remove/update/members/messages` 通过 `client.groups()`。
- `msg send --group ...` 通过 `client.messages().send(MessageTarget::Group)`。
- group wire helper 不作为 SDK public API。

## 8. Phase 1F：本地状态与 conversation projection 收口

目标：让 App/CLI 都能复用 conversation/thread projection，不重复聚合 inbox。

任务：

- `core.bootstrap().initialize_local_state()` 和 `migrate_local_state()` 可用。
- `client.messages().conversations()` 返回 conversation page。
- direct/group message projection 共用本地规则。
- 本地状态查询内部自动注入 owner。
- 优先引入 `owner_identity_id`，兼容已有 `owner_did`。

完成判定：

- App 可以通过 SDK 获得 conversation list，而不是自己解析 inbox raw JSON。
- CLI 不直接调用 `store::*` 业务 helper，除 `debug.db.*` 外。

## 9. Phase 1G：App sandbox 示例

目标：证明 `im-core` 不依赖 CLI，可以被 App 以显式路径方式接入。

任务：

- 新增一个简单 example 或 test fixture：
  - tempdir identity registry
  - tempdir SQLite
  - explicit config
  - fake/stub service 或 contract test
- 验证：
  - `ImCore::new(config, paths)` 不需要 CLI `Resolved`
  - `core.client(selector)` 不需要 CLI `Manager`
  - 基础 DTO 不含 CLI flag 名称

完成判定：

- `cargo test -p im-core app_sandbox_paths` 通过。

## 10. Phase 2：附件与 realtime runner

进入条件：Phase 1 普通私聊/群聊稳定。

范围：

- `client.attachments().send()` / `download()`。
- `AttachmentInput::LocalFile` + `Bytes`。
- `client.realtime().connect()` / `run_until_shutdown()`。
- `ImEvent` 领域事件。
- CLI listener service 仍留 CLI，只调用 SDK runner。

不做：

- secure direct。
- group E2EE。
- provider traits。

## 11. Phase 3：secure direct 与 group E2EE

进入条件：Phase 1/2 API 稳定，普通消息和 realtime 投影稳定。

范围：

- `MessageSecurityMode::SecureDirect`。
- direct session status/repair。
- secure outbox failed/retry/drop。
- group E2EE status/repair。
- group message 解密和 incoming secure processing 作为 messages/realtime 内部步骤。

原则：

- 普通发送仍走 `client.messages().send()`。
- 不暴露 ciphertext API。
- 不把 KeyPackage、MLS provider binary、prekey path 暴露给普通调用方。
- 诊断 API 可以 feature-gated。

## 12. Phase 4：provider 抽象

进入条件：App 接入确实需要接管存储、网络、密钥或 crypto。

可选 trait：

```rust
CredentialVault
SessionStore
MessageStore
GroupStore
ContactStore
BlobStore
Transport
CryptoProvider
MlsProvider
Clock
IdGenerator
```

原则：

- 不破坏 Phase 1 沉淀下来的 `ImCore` / `ImClient` / service DTO。
- provider 是替换底层实现，不是替换业务 API。
- 内置 SQLite/HTTP 实现继续保留。

## 13. 风险与控制

| 风险 | 控制 |
| --- | --- |
| 一次性搬太多导致 CLI 回归 | 先 façade，再逐步迁移；每阶段 CLI 命令可运行。 |
| SDK API 低层化 | public/internal deny list + re-export 收紧。 |
| 多身份隔离不彻底 | 所有 query 由 `ImClient` 注入 owner；测试 alice/bob 隔离。 |
| DID replace 后本地数据漂移 | owner key 优先 identity_id，did 作为当前状态字段。 |
| async 改造扩大范围 | Phase 1 blocking API，后续再评估 async feature。 |
| App 仍重复做 conversation projection | Phase 1F 明确实现 `messages().conversations()`。 |

## 14. 第一阶段完成判定

第一阶段完成后应满足：

- `crates/im-core` 可独立编译和测试。
- `crates/im-core` 不依赖 `crates/awiki-cli`。
- CLI 的 identity、auth、私聊、群聊基础命令最终都通过 `im-core`。
- CLI handler 基本变成 parse -> sdk call -> render。
- `im-core` public API 不暴露 actor、paths、wire params、SQLite connection、crypto state。
- `msg secure *`、`group e2ee *` 未进入默认 SDK API。
- App 可以通过显式路径构造 `ImCore` 并完成基础 profile/message/group 调用。
