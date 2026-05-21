# SDK Refactor：迁移计划

## 1. 迁移原则

- 先建立高层 SDK façade，再移动底层代码。
- Phase 1 只迁移 core 框架、身份鉴权、Handle 注册、私聊文本、群聊文本和必要 inbox/history。
- 完整 directory/profile、完整 group lifecycle、本地 conversation projection、附件、realtime、secure、provider 抽象后移。
- 不改 CLI 命令行为，先改 handler 调用路径。
- 不让 `im-core` 依赖 CLI 类型。
- 不让 SDK public API 暴露 wire/store/crypto/path 细节。
- 每个阶段都要保持 CLI 可编译、主要命令可运行。
- Phase 1 采用 blocking-first，避免第一阶段同时引入 async runtime 复杂度。

## 2. Phase 0：最终方案确认

目标：确认本目录作为最终主方案，`docs/sdk-refactor-2` 只作为已吸收的实施草案参考。

任务：

- 更新 `docs/sdk-refactor/` 文档。
- 明确 Phase 1 MVP 范围。
- 增加 `public-api.md`、`cli-boundary.md`、`merge-decisions.md`。
- 明确 public/internal deny list。
- 明确 `IdentitySelector::LocalAlias`、`owner_identity_id`、blocking-first、feature flags。

完成判定：

- 团队认可 `ImCore` / `ImClient` 的高层入口。
- 团队认可 Phase 1 只做 SDK 主链路，不完整迁移 group lifecycle/directory/profile/secure/realtime。
- 团队认可 `sdk-refactor` 是唯一主实施方案。

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
  - P1 messages 基础 DTO
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

- 在 CLI 中新增集中 adapter：
  - `build_im_core_config(resolved)`
  - `build_im_core_paths(resolved, manager)`
  - `cli_identity_selector(identity_flag)`
  - `map_im_error(err, context)`
- `ImCore::client(selector)` 能解析默认身份和 local alias。
- `ImClient` 内部持有 identity summary 和旧模块调用所需 runtime context。
- 第一批 handler 改为调用 SDK façade：
  - `id list`
  - `id current`
  - `id status`
  - `id use`
  - `id refresh-token`

完成判定：

- CLI identity 命令行为不变。
- handler 中不再直接拼低层 identity path，统一通过 adapter。
- CLI handler 形态开始变成 parse -> sdk call -> render。

## 5. Phase 1C：identity/auth + Handle 注册

目标：让身份鉴权和 Handle 注册从 `im-core` 跑通。

任务：

- 实现或封装：
  - list/default/resolve local identity
  - load identity runtime
  - DID document 与 key1 private key 读取
  - DID auth login
  - ensure session
  - refresh session
  - Handle 注册：`core.identities().register_handle()`
- CLI 保留：
  - OTP 输入
  - identity alias 选择
  - 本地路径布局
  - 文件权限
  - 输出渲染
- `IdentityRegistry::load()` 保持 `pub(crate)`，不返回 runtime paths。

完成判定：

- `id register` 通过 `core.identities().register_handle()`。
- `id refresh-token` 通过 `client.auth().refresh_session()`。
- public API 不返回 private key、auth path、DID document path。
- P1 可用 tempdir/path fixture 测试身份加载和 session path 隔离。

## 6. Phase 1D：私聊/群聊文本 MVP

目标：普通 direct text 和 group text message 进入 SDK。

任务：

- 实现：
  - `client.messages().send(SendMessageRequest { target: Direct, body: Text, security: DefaultPlain | Plain })`
  - `client.messages().send(SendMessageRequest { target: Group, body: Text, security: DefaultPlain | Plain })`
  - `client.messages().inbox()` 的必要子集
  - `client.messages().history(ThreadRef::Direct | ThreadRef::Group)` 的必要子集
- 内部处理：
  - DID/handle 的最小 target resolve
  - auth ensure + 401 refresh retry
  - HTTP/RPC params 构造
  - 远端结果转领域 DTO
  - 必要本地状态写入或兼容旧存储
- CLI adapter 处理：
  - `--to`
  - `--group`
  - `--text`
  - `--text-file`
  - `--limit`
  - dry-run

完成判定：

- `msg send --to ... --text ...` 通过 `client.messages().send()`。
- `msg send --group ... --text ...` 通过 `client.messages().send()`。
- `msg inbox/history` 的 P1 子集通过 `client.messages()`。
- handler 不再直接调用 `message::send/inbox/history`。
- wire params builder 不作为 SDK public API 导出。
- `MessageSecurityMode::SecureDirect` / `GroupE2ee` 返回 `UnsupportedCapability`。
- `MessageBody::Attachment` 返回 `UnsupportedCapability`。

## 7. Phase 1E：P1 测试与 App sandbox path fixture

目标：证明 `im-core` 不依赖 CLI，可以被 App 以显式路径方式接入。

任务：

- 新增一个简单 example 或 test fixture：
  - tempdir identity registry
  - tempdir SQLite
  - explicit config
  - fake/stub service 或 contract test
- 覆盖：
  - `ImCore::new(config, paths)` 不需要 CLI `Resolved`
  - `core.client(selector)` 不需要 CLI `Manager`
  - 基础 DTO 不含 CLI flag 名称
  - alice/bob 多身份 auth path 隔离
  - direct/group text request 构造和 UnsupportedCapability 行为

完成判定：

- `cargo test -p im-core app_sandbox_paths` 通过。
- `cargo test -p awiki-cli` 通过。
- CLI P1 命令可运行。

## 8. Phase 2：identity / directory / profile 补全

进入条件：Phase 1 主链路稳定。

范围：

- `client.identity().profile()` / `update_profile()`。
- `client.identity().bind_contact()`。
- `core.identities().recover_handle()`。
- `client.identity().replace_did()`，危险能力，必须返回风险信息和 rebind plan。
- `client.directory().resolve_peer()` / `lookup_handle()`。
- contact save/list。
- relation status / profile projection。

CLI 继续保留：

- profile markdown file 读取。
- 危险命令确认。
- 本地路径选择、备份、权限。
- 输出渲染。

## 9. Phase 3：message / group / local_state 补全

进入条件：Phase 1 消息 MVP 和 Phase 2 identity/directory 稳定。

范围：

- `client.messages().mark_read()`。
- `client.messages().conversations()`。
- 本地 message/contact/conversation projection。
- 缓存合并、消息状态、失败重试。
- 完整 group lifecycle：
  - create/get/list/join/leave
  - add/remove member
  - update profile/policy
  - members/messages
- 本地状态完整 owner isolation：
  - 优先 `owner_identity_id`
  - 兼容 `owner_did`

完成判定：

- App 可以通过 SDK 获得 conversation list，而不是自己解析 inbox raw JSON。
- CLI 业务 handler 不直接调用 `store::*` 业务 helper，除 `debug.db.*` 外。
- `group create/get/list/join/leave/add/remove/update/members/messages` 通过 `client.groups()`。

## 10. Phase 4：附件

进入条件：普通消息和本地 projection 稳定。

范围：

- `client.attachments().send()` / `download()`。
- `AttachmentInput::LocalFile` + `Bytes`。
- manifest、slot、commit object、download ticket、digest、临时文件和原子写入流程下沉。
- CLI 保留 `--file`、`--text-file`、`--output` 路径解析、覆盖策略和权限处理。

## 11. Phase 5：realtime runner

进入条件：消息、群组、本地 projection 稳定。

范围：

- `client.realtime().connect()`。
- `client.realtime().run_until_shutdown()`。
- `ImEvent` 领域事件。
- WebSocket response / notification 分类。
- pending request 路由。
- notification 投影。
- reconnect / heartbeat。

不做：

- systemd/launchd/Windows service。
- pid/log/socket/service install/start/stop。
- OpenClaw/Hermes setup UX。

这些继续留在 CLI。

## 12. Phase 6：secure direct 与 group E2EE

进入条件：普通消息和 realtime 投影稳定。

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

## 13. Phase 7：provider 抽象

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

## 14. 风险与控制

| 风险 | 控制 |
| --- | --- |
| 一次性搬太多导致 CLI 回归 | Phase 1 只做 SDK 主链路；其余命令继续旧实现。 |
| SDK API 低层化 | public/internal deny list + re-export 收紧。 |
| 多身份隔离不彻底 | 所有 query 由 `ImClient` 注入 owner；测试 alice/bob 隔离。 |
| DID replace 后本地数据漂移 | owner key 优先 identity_id，did 作为当前状态字段。 |
| async 改造扩大范围 | Phase 1 blocking API，后续再评估 async feature。 |
| App 仍重复做 conversation projection | Phase 3 明确实现 `messages().conversations()`。 |
| group lifecycle 过早扩大 P1 | P1 群聊只做面向已有 `GroupRef` 的文本消息。 |

## 15. Phase 1 完成判定

Phase 1 完成后应满足：

- `crates/im-core` 可独立编译和测试。
- `crates/im-core` 不依赖 `crates/awiki-cli`。
- CLI 的 P1 identity/auth/私聊/群聊基础命令通过 `im-core`。
- CLI handler 基本变成 parse -> sdk call -> render。
- `im-core` public API 不暴露 actor、paths、wire params、SQLite connection、crypto state。
- `msg secure *`、`group e2ee *` 未进入默认 SDK API。
- App 可以通过显式路径构造 `ImCore` 并完成基础 identity/auth/message 调用。
