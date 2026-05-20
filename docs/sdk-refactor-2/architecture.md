# SDK Refactor 2：整体架构

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
   +-- identity()
   +-- directory()
   +-- messages()
   +-- groups()
   +-- local_state through high-level services
   +-- realtime()       # 后续阶段
   +-- secure()         # 后续阶段/诊断
```

SDK 对外暴露产品级意图，例如“发送一条私聊文本消息”“列出群成员”“刷新 session”“列出会话”。SDK 不对普通调用方暴露 actor、RPC params、wire payload、SQLite connection、auth file path、secure session path、MLS provider binary path 等底层实现。

## 2. 非目标

第一阶段不做以下事情：

- 不完整迁移 direct E2EE、group E2EE、secure outbox、MLS。
- 不引入 `CredentialVault`、`Store`、`Transport`、`CryptoProvider` 等 provider 抽象。
- 不重写 HTTP/WebSocket/SQLite 底层实现。
- 不要求 App 立即通过 provider 接管存储、密钥和网络。
- 不把 CLI 的 `ParsedCommand`、`GlobalOptions`、`ExitError` 搬进 SDK。
- 不让 SDK 自己发现 CLI workspace 或读取 CLI config。

## 3. crate 拆分

```text
crates/im-core
  Rust IM SDK。
  负责身份、session、profile/directory、私聊、群聊、本地状态和后续 realtime/secure 编排。

crates/awiki-cli
  CLI 产品壳。
  负责命令解析、config/workspace/path 解析、文件权限、stdout/stderr、exit code、daemon/service、host notify、dry-run 展示。
```

依赖方向固定为：

```text
awiki-cli -> im-core
```

`im-core` 不能依赖 `awiki-cli`，不能引用 CLI 类型，不能读取 CLI config，不能假定路径来自 CLI。

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
| auth retry, target resolution, message/group flow,  |
| local projection, cache merge, owner isolation      |
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

第一阶段只做基础 IM 能力：

```text
Foundation
  - ImCore / ImClient / ImCoreConfig / ImCorePaths / ImError
  - 多身份 registry
  - 显式路径参数
  - 本地状态初始化/迁移入口

Identity/Auth
  - list/default/resolve local identity
  - register/recover handle
  - profile get/update
  - login/ensure_session/refresh/logout

Directory
  - handle/DID resolve
  - contact save/list
  - relation/profile projection

Messages
  - direct text send
  - inbox/history/mark-read
  - conversation/thread projection
  - local cache merge

Groups
  - create/get/list/join/leave
  - add/remove member
  - update profile/policy
  - members/messages
  - group text send through messages or groups convenience API
```

明确排除：

```text
Attachments        -> Phase 2
Realtime runner    -> Phase 2
Direct E2EE        -> Phase 3
Group E2EE / MLS   -> Phase 3
Provider traits    -> Phase 4
```

## 6. 多身份模型

`im-core` 必须从第一天按多身份设计。

```rust
let core = ImCore::new(config, paths)?;

let default_client = core.client(IdentitySelector::Default)?;
let alice_client = core.client(IdentitySelector::LocalAlias("alice".into()))?;
let bob_client = core.client(IdentitySelector::Did(bob_did))?;

alice_client.messages().send(request)?;
bob_client.groups().list(GroupQuery::default())?;
```

规则：

- `ImCore` 是环境级入口，不绑定具体身份。
- `ImClient` 是绑定单个身份后的业务入口。
- 不把“当前身份”作为 SDK 全局可变状态。
- `Default` 只是 `IdentitySelector` 的一种解析方式。
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

这样能用最小改动承接现有 CLI 逻辑，同时给 App 保留自己的 sandbox path 接入方式。

## 8. public/internal 边界

| 类别 | public API | internal only |
| --- | --- | --- |
| core | `ImCore`、`ImClient`、`ImCoreConfig`、`ImCorePaths`、`ImError` | `ActorContext`、`ClientIdentityRuntime` |
| identity | `IdentitySelector`、`IdentitySummary`、register/recover/profile | `StoredIdentity`、private key material、runtime paths |
| auth | `login`、`ensure_session`、`refresh_session`、`logout` | DID auth request builder、JWT file format helper |
| directory | `resolve_peer`、`lookup_handle`、contacts、relation status | raw user-service RPC、contact store row |
| messages | `send`、`inbox`、`history`、`mark_read`、`conversations` | `build_*_rpc_params`、raw payload、store projection helper |
| groups | lifecycle/member/messages APIs | `build_group_*_rpc_params`、group wire DTO |
| local_state | bootstrap/init/migrate only | SQLite connection、raw SQL、owner/path-level store helper |
| secure | Phase 3 diagnostics only | ciphertext payload, prekey, MLS provider, KeyPackage flow |
| realtime | Phase 2 runner/events | raw WebSocket frame, request id, pending dispatch queue |

## 9. 同步/异步策略

为了减少第一阶段改动，建议 `im-core` Phase 1 先沿用当前 CLI 的 blocking 实现方式：

```rust
pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
```

App 接入时可以在 Flutter plugin、mobile binding 或上层 runtime 中把 blocking 调用放到 worker thread。等 SDK 边界稳定后，再评估是否提供 async API 或 `im-core-async` feature。

这样可以避免第一阶段同时引入 async runtime、trait async、lifetime、tokio/spawn_blocking 等额外复杂度。

## 10. App 参考能力映射

App 侧需要的高层能力通常是：

```text
loadMyProfile / updateProfile / loadPublicProfile
listFollowers / listFollowing / follow / unfollow / relation_status
listConversations
fetchDmHistory / fetchGroupHistory
sendTextMessage / retryMessage / markRead
createGroup / joinGroup / getGroup / listGroups / listGroupMembers
consumeRealtimeEvent  # 后续 realtime 阶段
```

第一阶段的 `im-core` 应覆盖这些能力中的基础 subset，尤其是 conversation/thread projection；否则 App 仍会重复实现 inbox 聚合、thread id 规则、本地 cache 合并。

## 11. 发布边界

`im-core` 独立发布时，默认 feature 应只包含第一阶段稳定能力：

```text
default = ["blocking", "sqlite", "http"]
```

高级能力可后续以 feature 打开：

```text
attachments
realtime
secure-direct
group-e2ee
provider-traits
internal-test-helpers
```

不建议把 internal test helper 放入默认 feature。
