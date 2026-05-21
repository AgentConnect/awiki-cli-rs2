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
   +-- auth()          # Phase 1
   +-- messages()      # Phase 1：direct/group text + necessary inbox/history
   +-- identity()      # Phase 2+
   +-- directory()     # Phase 2+
   +-- groups()        # Phase 3+
   +-- realtime()      # Phase 5+
   +-- secure()        # Phase 6+/diagnostic
```

SDK 对外暴露产品级意图，例如“使用某个身份刷新 session”“注册 handle”“发送一条私聊文本消息”“发送一条群聊文本消息”。SDK 不对普通调用方暴露 actor、RPC params、wire payload、SQLite connection、auth file path、secure session path、MLS provider binary path 等底层实现。

## 2. 与 `docs/sdk-refactor/` 的关系

`docs/sdk-refactor/` 是长期边界主方案，描述完整 IM 能力最终如何进入 `im-core`。`docs/sdk-refactor-2/` 是实施版补充，重点回答：

- Phase 1 怎样先让 SDK 跑起来；
- P1 public API 应该长什么样；
- CLI handler 如何改成 SDK façade 调用；
- 哪些能力必须后移，避免第一阶段变大。

如果两个目录出现范围冲突，应以当前文档中标注的 Phase 1 MVP 为准：**身份、鉴权、Handle 注册、私聊消息、群聊消息先跑通；其余后移。**

## 3. 非目标

第一阶段不做以下事情：

- 不完整迁移 directory/profile/recover/replace DID。
- 不迁移完整 group lifecycle、成员管理、群 profile/policy 更新。
- 不完整迁移 mark-read、conversation projection、本地 cache merge。
- 不迁移附件 upload/download。
- 不迁移 realtime runner、listener daemon 或 host notification。
- 不完整迁移 direct E2EE、group E2EE、secure outbox、MLS。
- 不引入 `CredentialVault`、`Store`、`Transport`、`CryptoProvider` 等 provider 抽象。
- 不重写 HTTP/WebSocket/SQLite 底层实现。
- 不把 CLI 的 `ParsedCommand`、`GlobalOptions`、`ExitError` 搬进 SDK。
- 不让 SDK 自己发现 CLI workspace 或读取 CLI config。

## 4. crate 拆分

```text
crates/im-core
  Rust IM SDK。
  Phase 1 负责 core、多身份、auth/session、Handle 注册、私聊文本、群聊文本和必要 inbox/history。
  后续阶段负责 directory/profile、群管理、附件、本地状态、realtime、secure 等能力。

crates/awiki-cli
  CLI 产品壳。
  负责命令解析、config/workspace/path 解析、文件权限、stdout/stderr、exit code、daemon/service、host notify、dry-run 展示。
```

依赖方向固定为：

```text
awiki-cli -> im-core
```

`im-core` 不能依赖 `awiki-cli`，不能引用 CLI 类型，不能读取 CLI config，不能假定路径来自 CLI。

## 5. 分层模型

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
| ImCore, ImClient, IdentityRegistry, MessageService  |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core business orchestration                      |
| identity loading, auth retry, target resolution,    |
| direct/group text flow, minimal local projection    |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core internal implementation                     |
| HTTP/RPC, wire params, DID proof, path IO, SQLite   |
+----------------------------------------------------+
```

public API 与 internal implementation 之间必须有明确防线。第一阶段最重要的代码约束是：`wire::*`、`store::*`、`transport::*`、`crypto::*`、`runtime::*` helper 不作为 SDK 主入口导出。

## 6. Phase 1 MVP 能力范围

```text
Foundation
  - ImCore / ImClient / ImCoreConfig / ImCorePaths / ImError
  - 多身份 registry
  - 显式路径参数
  - bootstrap path validation / minimal local state init

Identity/Auth
  - list/default/local alias resolve
  - register handle
  - login / ensure_session / refresh_session

Messages
  - direct text send
  - group text send to existing GroupRef
  - necessary inbox/history for direct/group verification
```

Phase 1 的群聊能力是 **群消息能力**，不是完整群管理能力。也就是说，P1 能面向已有 `GroupRef` 发送/读取群消息；`group create/join/add/remove/update/members` 后移到 Phase 3。

## 7. 多身份模型

`im-core` 必须从第一天按多身份设计。

```rust
let core = ImCore::new(config, paths)?;

let default_client = core.client(IdentitySelector::Default)?;
let alice_client = core.client(IdentitySelector::LocalAlias("alice".into()))?;

alice_client.messages().send(request)?;
```

规则：

- `ImCore` 是环境级入口，不绑定具体身份。
- `ImClient` 是绑定单个身份后的业务入口。
- 不把“当前身份”作为 SDK 全局可变状态。
- `Default` 只是 `IdentitySelector` 的一种解析方式。
- `auth/session`、local state、direct secure state、future MLS state 都必须按身份隔离。
- 第一阶段本地数据库可以共享，但所有业务查询必须由 `ImClient` 自动注入 owner，不允许 App/CLI 手动拼 owner 条件。
- 本地状态的稳定 owner key 建议优先使用 `owner_identity_id`，`owner_did` 作为兼容和展示字段。

`IdentitySelector::LocalAlias` 替代 `Name(String)`，避免和 display name、handle 混淆。CLI 的 `--identity alice` 转成 `IdentitySelector::LocalAlias("alice")`。

## 8. 显式路径参数

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

业务 API 不传路径。路径只出现在 `ImCore::new(config, paths)`、`core.bootstrap()` 和注册/恢复这类生命周期流程中。

## 9. public/internal 边界

| 类别 | public API | internal only |
| --- | --- | --- |
| core | `ImCore`、`ImClient`、`ImCoreConfig`、`ImCorePaths`、`ImError` | `ActorContext`、`ClientIdentityRuntime` |
| identity | `IdentitySelector`、`IdentitySummary`、P1 `register_handle` | `StoredIdentity`、private key material、runtime paths |
| auth | `login`、`ensure_session`、`refresh_session` | DID auth request builder、JWT file format helper |
| messages | P1 `send`、`inbox`、`history` | `build_*_rpc_params`、raw payload、store projection helper |
| directory | Phase 2 public API | raw user-service RPC、contact store row |
| groups | Phase 3 public API | `build_group_*_rpc_params`、group wire DTO |
| local_state | bootstrap/init/migrate only | SQLite connection、raw SQL、owner/path-level store helper |
| secure | Phase 6 diagnostics only | ciphertext payload, prekey, MLS provider, KeyPackage flow |
| realtime | Phase 5 runner/events | raw WebSocket frame, request id, pending dispatch queue |

## 10. 同步/异步策略

为了减少第一阶段改动，建议 `im-core` Phase 1 先沿用当前 CLI 的 blocking 实现方式：

```rust
pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
```

App 接入时可以在 Flutter plugin、mobile binding 或上层 runtime 中把 blocking 调用放到 worker thread。等 SDK 边界稳定后，再评估是否提供 async API 或 `im-core-async` feature。

这样可以避免第一阶段同时引入 async runtime、trait async、lifetime、tokio/spawn_blocking 等额外复杂度。

## 11. feature 规划

`im-core` 独立发布时，默认 feature 应只包含 Phase 1 稳定能力：

```text
default = ["blocking", "sqlite", "http"]
```

高级能力后续以 feature 打开：

```text
attachments       # Phase 4
realtime          # Phase 5
secure-direct     # Phase 6
group-e2ee        # Phase 6
provider-traits   # Phase 7
internal-test-helpers
```

不建议把 internal test helper 放入默认 feature。
