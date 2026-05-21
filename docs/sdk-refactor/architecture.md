# SDK Refactor：整体架构

## 1. 目标

本方案目标是把 `awiki-cli-rs2` 中可被 CLI 和 App 共同使用的 IM 能力抽成独立 Rust crate：`crates/im-core`。

目标不是把 CLI 代码按目录机械搬家，而是先建立稳定的 **高层 SDK 边界**：

```text
App / CLI
   |
   v
ImCore                 # 环境级 SDK 入口，多身份 registry、bootstrap
   |
   v
ImClient               # 绑定单个身份后的业务客户端
   |
   +-- auth()
   +-- messages()
   +-- identity()       # Phase 2+
   +-- directory()      # Phase 2+
   +-- groups()         # Phase 3+
   +-- attachments()    # Phase 4+
   +-- realtime()       # Phase 5+
   +-- secure()         # Phase 6+/diagnostic
```

SDK 对外暴露产品级意图，例如“注册 Handle”“刷新 session”“发送一条私聊文本消息”“向已有群发送文本消息”“读取 inbox/history”。SDK 不对普通调用方暴露 actor、RPC params、wire payload、SQLite connection、auth file path、secure session path、MLS provider binary path 等底层实现。

## 2. 非目标

第一阶段不做以下事情：

- 不完整迁移 direct E2EE、group E2EE、secure outbox、MLS。
- 不迁移完整 group lifecycle / member management。
- 不完整迁移 profile/directory/recover/replace DID。
- 不完整迁移附件 upload/download。
- 不迁移 realtime runner 或 CLI daemon/service。
- 不引入 `CredentialVault`、`Store`、`Transport`、`CryptoProvider` 等 provider 抽象。
- 不重写 HTTP/WebSocket/SQLite 底层实现。
- 不要求 App 立即通过 provider 接管存储、密钥和网络。
- 不把 CLI 的 `ParsedCommand`、`GlobalOptions`、`ExitError` 搬进 SDK。
- 不让 SDK 自己发现 CLI workspace 或读取 CLI config。

## 3. crate 拆分

```text
crates/im-core
  Rust IM SDK。
  负责身份、session、Handle 注册、私聊/群聊消息，以及后续 profile/directory、群管理、附件、realtime、secure、本地状态完整编排。

crates/awiki-cli
  CLI 产品壳。
  负责命令解析、config/workspace/path 解析、文件权限、stdout/stderr、exit code、daemon/service、host notify、dry-run 展示。
```

依赖方向固定为：

```text
awiki-cli -> im-core
```

目标 workspace：

```toml
[workspace]
members = [
    "crates/im-core",
    "crates/awiki-cli",
    "xtask",
]
```

`crates/awiki-cli/Cargo.toml` 目标依赖：

```toml
[dependencies]
im-core = { path = "../im-core" }
```

`im-core` 不能依赖 `awiki-cli`，不能引用 CLI 类型，不能读取 CLI config，不能假定路径来自 CLI。当前迁移来源是 `crates/awiki-cli` 内部模块，不重新引入仓库外 sibling `awiki-im-core`。

## 4. 分层模型

```text
+----------------------------------------------------+
| App / CLI                                          |
| - App UI / Flutter plugin / mobile lifecycle       |
| - CLI command parser / config resolver / renderer  |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core public API                                  |
| ImCore, ImClient, IdentityRegistry, services        |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core business orchestration                      |
| auth retry, target resolution, message flow,        |
| group message flow, local owner binding             |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core internal implementation                     |
| HTTP/RPC, wire params, DID proof, SQLite store,     |
| discovery, path IO, future realtime/secure helpers  |
+----------------------------------------------------+
```

public API 与 internal implementation 之间必须有明确防线。第一阶段最重要的代码约束是：`wire::*`、`store::*`、`transport::*`、`crypto::*`、`runtime::*` helper 不作为 SDK 主入口导出。

## 5. 第一阶段能力范围

第一阶段只做最小可运行 IM SDK：

```text
Foundation
  - ImCore / ImClient / ImCoreConfig / ImCorePaths / ImError
  - 多身份 registry
  - 显式路径参数
  - 本地状态路径校验/最小 bootstrap

Identity/Auth
  - list/default/resolve local identity
  - load identity runtime
  - register handle
  - login/ensure_session/refresh/status

Messages
  - direct text send
  - group text send, 面向已有 GroupRef
  - 必要 inbox/history
  - auth ensure + 401 refresh retry
  - 必要 target resolve
```

明确排除：

```text
Directory/Profile 完整能力 -> Phase 2
完整 Message/Group/Local State -> Phase 3
Attachments                 -> Phase 4
Realtime runner             -> Phase 5
Direct E2EE / Group E2EE    -> Phase 6
Provider traits             -> Phase 7
```

## 6. 多身份模型

`im-core` 必须从第一天按多身份设计。

```rust
let core = ImCore::new(config, paths)?;

let default_client = core.client(IdentitySelector::Default)?;
let alice_client = core.client(IdentitySelector::LocalAlias("alice".into()))?;
let bob_client = core.client(IdentitySelector::Did(bob_did))?;

alice_client.messages().send(request)?;
```

规则：

- `ImCore` 是环境级入口，不绑定具体身份。
- `ImClient` 是绑定单个身份后的业务入口。
- 不把“当前身份”作为 SDK 全局可变状态。
- `Default` 只是 `IdentitySelector` 的一种解析方式。
- `LocalAlias` 表达 CLI credential name / 本地身份别名，比 `Name` 更准确。
- `auth/session`、local state、direct secure state、future MLS state 都必须按身份隔离。
- 第一阶段本地数据库可以共享，但所有业务查询必须由 `ImClient` 自动注入 owner，不允许 App/CLI 手动拼 owner 条件。
- 本地状态的稳定 owner key 建议优先使用 `owner_identity_id`，`owner_did` 作为兼容和展示字段。

## 7. 显式路径参数

第一阶段采用路径参数版：

```text
CLI/App 负责：
  - workspace/config 解析
  - identity root/default/registry path
  - DID document path
  - private key path
  - auth/session path
  - SQLite path
  - runtime temp/cache path
  - 文件权限、目录创建、备份策略

im-core 负责：
  - 在显式传入的路径上读写业务所需状态
  - 按身份绑定路径
  - 不自行发现 workspace
  - 不读取 CLI config
```

这样能用最小改动承接现有 CLI 逻辑，同时给 App 保留自己的 sandbox path 接入方式。路径 DTO 只在 `ImCore::new`、`core.bootstrap()`、注册/恢复等构造或生命周期阶段出现，不作为业务 API 参数。

## 8. public/internal 边界

| 类别 | public API | internal only |
| --- | --- | --- |
| core | `ImCore`、`ImClient`、`ImCoreConfig`、`ImCorePaths`、`ImError` | `ActorContext`、`ClientIdentityRuntime` |
| identity | `IdentitySelector`、`IdentitySummary`、P1 register handle | `StoredIdentity`、private key material、runtime paths |
| auth | `login`、`ensure_session`、`refresh_session`、`status` | DID auth request builder、JWT file format helper |
| messages | P1 `send`、`inbox`、`history` | `build_*_rpc_params`、raw payload、store projection helper |
| local_state | P1 bootstrap/path validation | SQLite connection、raw SQL、owner/path-level store helper |
| directory | Phase 2 `resolve_peer`、contacts、relation status | raw user-service RPC、contact store row |
| groups | Phase 3 lifecycle/member/messages APIs | `build_group_*_rpc_params`、group wire DTO |
| attachments | Phase 4 send/download | upload slot/commit/ticket internals |
| realtime | Phase 5 runner/events | raw WebSocket frame, request id, pending dispatch queue |
| secure | Phase 6 diagnostics + secure send integration | ciphertext payload, prekey, MLS provider, KeyPackage flow |

## 9. 同步/异步策略

为了减少第一阶段改动，`im-core` Phase 1 采用 blocking-first：

```rust
pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
```

App 接入时可以在 Flutter plugin、mobile binding 或上层 runtime 中把 blocking 调用放到 worker thread。等 SDK 边界稳定后，再评估是否提供 async API 或 `im-core-async` feature。

这样可以避免第一阶段同时引入 async runtime、trait async、lifetime、tokio/spawn_blocking 等额外复杂度。

## 10. App 参考能力映射

App 侧长期需要的高层能力通常是：

```text
loadMyProfile / updateProfile / loadPublicProfile
listFollowers / listFollowing / follow / unfollow / relation_status
listConversations
fetchDmHistory / fetchGroupHistory
sendTextMessage / retryMessage / markRead
createGroup / joinGroup / getGroup / listGroups / listGroupMembers
consumeRealtimeEvent
```

这些能力不全部进入 P1。P1 只覆盖 SDK 主链路：身份、auth、Handle 注册、私聊文本、群聊文本、必要 inbox/history。完整 App 能力按 Phase 2 ~ Phase 6 逐步补齐。

## 11. 发布边界

`im-core` 独立发布时，默认 feature 应只包含基础稳定能力：

```toml
default = ["blocking", "sqlite", "http"]
```

高级能力后续以 feature 打开：

```text
attachments
realtime
secure-direct
group-e2ee
provider-traits
internal-test-helpers
```

不建议把 internal test helper 放入默认 feature。
