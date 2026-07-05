# 06. Implementation Map

本文件最早说明 P1 接口如何和当时 CLI 模块对应。当前它保留历史迁移背景，同时记录当前实现中仍然有效的 public/internal 边界。若本文件和 `04-message-interface.md`、`im-core-public-api.md` 或源码不一致，以当前 interface/public API 文档和源码为准。

当前固定依赖方向仍然是：

```text
awiki-cli -> im-core
```

## 1. Public API 与 Legacy Adapter

历史 P1 分两层；当前实现已经把消息、conversation projection、sync、read-state、send local echo 等主能力迁入 `crates/im-core`，不应再按 P1-alpha 把 App/CLI 主路径路由到旧模块。

### 1.1 Historical P1-alpha：CLI 过渡 adapter 调旧模块

历史 P1-alpha 曾允许这样做：

```text
CLI handler
  -> awiki-cli::im_core_adapter
     -> current low-level implementation in awiki-cli
```

此时：

```text
im-core 只提供 public API / DTO / error / façade shape
awiki-cli::im_core_adapter 负责把 SDK DTO 转成旧 request
旧模块测试继续保留
```

### 1.2 Historical P1-beta：im-core internal legacy module 调已迁入代码

P1-beta 允许这样做：

```text
im-core public API
  -> im-core internal legacy module
     -> implementation copied/moved into crates/im-core
```

这里的 implementation 必须已经在 `crates/im-core` 内部，不能是对 `crates/awiki-cli` 的反向依赖。

### 1.3 禁止

不允许：

```text
im-core -> awiki-cli
im-core -> current awiki-cli message/auth/identity modules
im-core public API exposes current SendRequest / InboxRequest / RPC params
```

## 2. Identity/Auth 映射

| P1 SDK API | 现有能力来源 | 迁移方式 |
| --- | --- | --- |
| `core.identities().list()` | CLI identity manager / identity records | P1-alpha 由 CLI adapter 读旧模块；P1-beta 再迁到 `im-core::identity`。 |
| `core.identities().default_identity()` | default identity file | CLI 决定路径，SDK 读显式路径。 |
| `core.client(selector)` | identity runtime loading | SDK 内部 `load_runtime`，不返回 paths。 |
| `client.auth().refresh_session()` | DID auth/session code | 先封装现有逻辑，返回 `SessionUpdate`。 |
| `core.identities().register_handle()` | id register flow | CLI 输入 OTP，SDK 编排注册业务。 |

## 3. Message 映射

| P1 SDK API | 现有能力来源 | 注意点 |
| --- | --- | --- |
| `messages().send(Direct + Text)` | current direct send | 当前 `SendRequest { identity_name, target, ... }` 不能作为 SDK public DTO。 |
| `messages().send(Group + Text)` | current group send | P1 只面向已有 `GroupRef`，不做 group lifecycle。 |
| `messages().send_conversation_text()` / `messages().send_conversation_payload()` | conversation resolver + local projection + network send | 当前 AWiki Me conversation UI send 主路径；pending/accepted/sent/failed 由 `im-core` durable projection 表达。 |
| `messages().inbox()` | current inbox | P1 返回 `Page<Message>`，不返回默认 raw JSON payload。 |
| `messages().history()` | current history | direct/group history 统一成 `ThreadRef`。 |
| `messages().sync_delta()` | message-service `sync.delta` + local SQLite apply | `since_event_seq` 和 checkpoint 只在 `im-core` 内部；不得由 CLI/App 传入。 |
| `messages().sync_thread_after()` | message-service `sync.thread_after` | thread-local 补新；不得直接返回本地合并的 `history_async` page。 |
| `messages().sync_conversation_after()` | `sync_thread_after` + `ConversationReadRef` resolver | AWiki Me / Dart 主路径；用 canonical `conversation_id` 解析 storage thread，不让 App 自己拼 alias。 |
| `messages().local_conversation_timeline()` | SQLite `messages` projection | conversationId-first local-first timeline；远端 history/backfill 只有持久化后才能通过这里成为 UI 事实。 |
| `messages().mark_conversation_read()` | `mark_thread_read` + `ConversationReadRef` resolver | conversationId-first read watermark；UI 不从 renderable cache 计算默认水位。 |
| `messages().watch_conversation_timeline_patches()` | message runtime store | conversationId-first timeline patch stream；旧 `watch_thread_patches(ThreadRef)` 仅保留兼容。 |

## 4. 现有低层 re-export 收紧目标

当前 message 模块公开了大量低层 helper。P1 迁移后，以下能力不应作为 SDK public API：

```text
build_direct_send_rpc_params
build_group_send_rpc_params
build_inbox_rpc_params
build_history_rpc_params
build_sync_delta_rpc_params
build_sync_thread_after_rpc_params
build_conversation_alias_or_thread_key_in_app
build_group_*_rpc_params
build_secure_*_payload
secure outbox flush internals
attachment slot/commit/ticket helpers
SQLite store helpers
sync checkpoint load/store helpers
raw wire payload
```

收紧节奏：

```text
1. P1 先保证 im-core 不 re-export 这些 helper。
2. CLI 中仍可临时 pub/use 旧 helper。
3. 当对应业务迁入 im-core 后，把旧 helper 改为 pub(crate) 或 internal module。
```

## 5. Unsupported Capability Contract

以下是历史 P1 placeholder 规则。当前 public API 已经暴露 identity、directory、groups、attachments、realtime、secure 等后续能力；这些能力以各自当前 docs/source 为准，不再按本段限制判断是否存在。

历史 P1 默认 public API 只暴露：

```text
client.auth()
client.messages()
```

P1 默认 public API 不暴露：

```text
client.identity()
client.directory()
client.groups()
client.attachments()
client.realtime()
client.secure()
```

如果为了前向兼容提前暴露这些 placeholder service，必须满足：

```text
1. 放在 non-default feature、experimental API 或明确 P2+ 文档中。
2. 调用时返回 UnsupportedCapability。
3. 不进入 Phase 1 默认 public API / prelude。
```

历史 P1 中这些 message 调用必须返回 `UnsupportedCapability`，不能静默降级。当前实现已支持 attachment、secure direct 和 group E2EE 的部分/完整路径；当前行为以 `04-message-interface.md`、attachment/E2EE docs 和源码为准：

```text
MessageBody::Attachment
MessageSecurityMode::SecureDirect
MessageSecurityMode::GroupE2ee
```

CLI 可以提前拦截并提示，但 SDK 层也必须有保护。

## 6. Owner Injection

所有 P1 message/auth 调用必须通过 `ImClient` 注入：

```text
identity_id
current_did
auth state path
owner context
```

不允许 CLI/App 传：

```text
owner_did
identity_name
auth path
sqlite path
ActorContext
```

## 7. Current Local State

历史 P1 local state 只要求：

```text
bootstrap validate/init/migrate
optional message write-through for compatibility
owner context available
```

当前实现已经要求并由 `im-core` 拥有：

```text
conversation projection
unread aggregation
mark-read sync
conversation read model
conversation timeline read model
read-state projection
send/outbox local echo
reliable sync checkpoint
contact cache merge
group/member projection
```

这些不再是 future Phase 3 事项。AWiki Me 必须通过 high-level SDK/API 消费这些 read model，不应重新实现 alias、read watermark、send/outbox、sync checkpoint 或 conversation ownership。
