# P1-beta 垂直切片迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`

**适用阶段**：`docs/sdk-refactor/implementation-playbook.md` 中的 `15. P1-beta：开始垂直切片迁入 im-core`

**目标**：在不大面积重写、不整体搬迁顶层模块的前提下，让 `crates/im-core` 开始承载真实实现，并让 `awiki-cli` 通过 wrapper / adapter 平滑切换到 `im-core`。

---

## 0. 约束
  请先阅读并遵守这些文档：
  - docs/sdk-refactor/implementation-playbook.md 的 “Phase 1H：App sandbox path fixture”
  - docs/sdk-refactor/README.md
  - docs/sdk-refactor/architecture.md
  - docs/sdk-refactor/public-api.md
  - docs/sdk-refactor/im-core-cli-boundary.md
  - docs/sdk-refactor/Interface/README.md
  - docs/sdk-refactor/Interface/01-crate-layout.md
  - docs/sdk-refactor/Interface/02-core-interface.md
  - docs/sdk-refactor/Interface/03-identity-auth-interface.md
  - docs/sdk-refactor/Interface/05-cli-adapter-interface.md
  - docs/sdk-refactor/Interface/06-implementation-map.md
  - docs/sdk-refactor/Interface/07-phase1-acceptance.md
  - docs/sdk-refactor/modules/01-core.md
  - docs/sdk-refactor/modules/02-identity.md
  - docs/sdk-refactor/modules/03-auth.md
  - docs/sdk-refactor/modules/04-local-state.md
  - docs/sdk-refactor/modules/07-messages.md
  - docs/sdk-refactor/modules/08-groups.md

  不用进行系统测试，只用进行单元测试即可，系统测试统一做

## 1. 总体结论

P1-beta 不采用长期函数级迁移，也不采用顶层模块整体迁移。

推荐迁移粒度：

```text
主策略：文件级迁移
辅策略：小子模块级迁移，通常 2-5 个强相关文件
例外：函数级抽取只用于拆掉少量 CLI 依赖，不作为长期迁移单位
禁止：一次性整体迁移 message、identity、store、runtime、app handlers
```

原因：

```text
1. 函数级迁移成本太高，会制造大量 wrapper、visibility、错误映射和测试重复成本。
2. 顶层模块级迁移风险太大，会触发 auth、store、runtime、CLI output 的连锁重写。
3. 文件级 / 小子模块级迁移能保持迁移单元足够清晰，同时避免大面积重写。
```

P1-beta 的推荐 PR 顺序：

```text
PR 15A：建立 im-core internal/wire + compat 边界
PR 15B：迁移 direct/inbox/history/mark_read wire builder
PR 15C：迁移 origin proof 小子模块
PR 15D：迁移普通 group.send wire builder，不动 group lifecycle
PR 15E：auth ensure/refresh 真实 provider 边界
PR 15F：direct text send 真迁移
PR 15G：group text send 真迁移
PR 15H：inbox/history P1 子集真迁移
```

---

## 2. P1-beta 目标

P1-beta 的目标不是把 `message`、`identity`、`store`、`authsdk` 整体搬进 `im-core`，而是把已经被 P1-alpha 验证过的 SDK facade 后面，逐步替换成 `im-core` 自己的真实实现。

迁移完成后应达到：

```text
1. im-core 开始拥有真实 message/auth/wire/proof 实现，而不是只提供 DTO/stub。
2. awiki-cli 仍然保持现有命令行为和测试兼容。
3. awiki-cli 可以通过 compat wrapper 调用 im-core 已迁移能力。
4. 未迁移能力继续走 legacy awiki-cli 模块。
5. 每个切片都可以独立回滚。
```

当前仓库里的实际迁移入口主要是：

```text
crates/awiki-cli/src/message/wire.rs
crates/awiki-cli/src/message/group_wire.rs
crates/awiki-cli/src/message/proof.rs
crates/awiki-cli/src/message/service.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
crates/awiki-cli/src/im_core_adapter/auth.rs
crates/im-core/src/messages/service.rs
crates/im-core/src/auth/service.rs
crates/im-core/src/internal/mod.rs
```

---

## 3. 进入条件

开始 P1-beta 前，必须满足：

```text
1. crates/im-core 能独立编译。
2. awiki-cli 已依赖 im-core，但 im-core 不依赖 awiki-cli。
3. P1-alpha adapter 路径已存在，CLI 能通过 SDK DTO 构造请求。
4. cargo test -p im-core 通过。
5. cargo test -p awiki-cli 通过，或至少当前主线约定的 selector 通过。
6. im-core boundary 测试确认不引用 CLI 类型。
7. 未迁移能力仍有 legacy 路径可回退。
```

建议进入前检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

如果 `cargo test -p awiki-cli` 在当前仓库成本过高，可以使用当前项目约定的 focused selector，但必须记录 selector 和未覆盖风险。

---

## 4. 目录和边界设计

P1-beta 建议先建立这些 `im-core` 内部目录：

```text
crates/im-core/src/internal/wire/
  mod.rs
  common.rs
  direct.rs
  group.rs
  inbox.rs
  history.rs

crates/im-core/src/internal/proof/
  mod.rs
  origin.rs

crates/im-core/src/internal/auth/
  mod.rs
  session.rs

crates/im-core/src/internal/message_runtime/
  mod.rs
  direct.rs
  group.rs
  inbox.rs
  history.rs

crates/im-core/src/compat/
  mod.rs
  wire.rs
  proof.rs
  auth.rs
```

边界规则：

```text
1. internal/* 是 im-core 真正实现，默认 pub(crate)。
2. compat/* 是 awiki-cli 迁移期专用公开口，使用 #[doc(hidden)]。
3. awiki-cli 不直接访问 im_core::internal。
4. P2 或 P3 稳定后集中删除 compat。
```

这样可以避免跨 crate 使用 `pub(crate)` 的 visibility 问题，也避免把 `internal` 直接变成隐形 public API。

---

## 5. 关键内部类型

`im-core` 内部不要直接使用 `awiki-cli` 的这些类型：

```text
StoredIdentity
MessageError
InboxRequest
HistoryRequest
MarkReadRequest
Resolved
Manager
ExitError
ParsedCommand
```

P1-beta 可以先新增最小内部类型：

```rust
pub(crate) struct WireIdentity {
    pub did: String,
    pub identity_label: String,
    pub did_document: Option<serde_json::Value>,
    pub key1_private_pem: String,
}

pub(crate) struct DirectTextWireRequest {
    pub target_did: String,
    pub text: String,
    pub message_kind: crate::messages::MessageKind,
}

pub(crate) struct InboxWireRequest {
    pub limit: u32,
    pub unread_only: bool,
}

pub(crate) struct HistoryWireRequest {
    pub peer_did: String,
    pub limit: u32,
    pub cursor: Option<String>,
    pub skip: u32,
}
```

`awiki-cli` wrapper 负责转换：

```text
StoredIdentity -> WireIdentity
message::InboxRequest -> im-core compat request
message::HistoryRequest -> im-core compat request
message::MarkReadRequest -> im-core compat request
ImError -> MessageError / ExitError
```

---

## 6. PR 15A：建立 compat 边界和 wire 内部结构

### 6.1 目标

建立 `im-core` 迁移承载结构，但不改变 `awiki-cli` 行为。

### 6.2 改动范围

```text
crates/im-core/src/lib.rs
crates/im-core/src/internal/mod.rs
crates/im-core/src/internal/wire/mod.rs
crates/im-core/src/internal/wire/common.rs
crates/im-core/src/compat/mod.rs
crates/im-core/src/compat/wire.rs
crates/im-core/tests/wire_contract.rs
```

### 6.3 执行步骤

```text
1. 新增 internal::wire 和 compat::wire 模块。
2. 把 now_rfc3339 / generate_operation_id / content_type_for_message_kind 这类基础 helper 放入 internal::wire::common。
3. compat::wire 只暴露迁移期需要的函数，不暴露整个 internal 模块。
4. 暂不改 awiki-cli 调用点。
5. 添加 im-core 测试覆盖 content type、operation_id 格式、created_at 格式。
```

### 6.4 验收

```bash
cargo test -p im-core wire
cargo test -p awiki-cli
rg "awiki_cli|ParsedCommand|ExitError|config::Resolved|identity::Manager" crates/im-core/src
```

### 6.5 完成标准

```text
1. im-core 有 wire 目录和 compat 入口。
2. awiki-cli 行为零变化。
3. im-core 没有引入 awiki-cli 类型。
```

---

## 7. PR 15B：迁移 direct/inbox/history/mark_read wire builder

### 7.1 目标

把 direct.send、inbox、history、mark_read 的 wire builder 迁到 `im-core`。

### 7.2 源和目标

源：

```text
crates/awiki-cli/src/message/wire.rs
```

目标：

```text
crates/im-core/src/internal/wire/direct.rs
crates/im-core/src/internal/wire/inbox.rs
crates/im-core/src/internal/wire/history.rs
crates/im-core/src/compat/wire.rs
```

### 7.3 执行方式

```text
1. 不直接搬 StoredIdentity 版本函数。
2. 在 im-core 内实现基于 WireIdentity / SDK DTO 的 builder。
3. awiki-cli 原 message/wire.rs 保留文件路径和函数签名。
4. awiki-cli 原函数内部改为调用 im_core::compat::wire。
5. MessageError 映射仍在 awiki-cli wrapper 中完成。
6. 复制 message_contract.rs 中 direct/inbox/history/mark_read 相关断言到 im-core tests。
7. awiki-cli 原 message_contract.rs 继续保留，验证 wrapper 兼容。
```

保留在 `awiki-cli` wrapper 的内容：

```text
StoredIdentity 读取
MessageError 构造
attachment_manifest_content_type 兼容
legacy request 类型转换
```

不要迁移：

```text
attachment
secure direct
group E2EE
runtime listener bridge
local store persist
```

### 7.4 验收

```bash
cargo test -p im-core wire
cargo test -p awiki-cli --test message_contract
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

### 7.5 完成标准

```text
1. direct/inbox/history/mark_read wire shape 与旧测试一致。
2. awiki-cli/src/message/wire.rs 变成薄 wrapper。
3. 所有旧调用点无需大面积改动。
```

---

## 8. PR 15C：迁移 origin proof 小子模块

### 8.1 目标

把 RFC9421 origin proof 生成迁到 `im-core`，`awiki-cli` 继续通过 wrapper 使用。

### 8.2 源和目标

源：

```text
crates/awiki-cli/src/message/proof.rs
```

目标：

```text
crates/im-core/src/internal/proof/origin.rs
crates/im-core/src/compat/proof.rs
```

### 8.3 前置条件

`im-core/Cargo.toml` 需要引入当前 `awiki-cli` 已使用的 `anp` workspace dependency。

### 8.4 执行方式

```text
1. 将 PrivateKeyMaterial 加载、verification_method_id_from_document、origin_auth_value 迁入 im-core。
2. build_origin_proof 改为接收 WireIdentity + DirectPayload。
3. awiki-cli/src/message/proof.rs 保留旧签名，转换 StoredIdentity 后调用 im_core::compat::proof。
4. 保留 ORIGIN_PROOF_SCHEME 值不变。
5. 复制 message_contract.rs 中 origin proof 相关测试到 im-core。
```

### 8.5 风险控制

```text
1. 不迁移 identity::Manager。
2. 不在 im-core 读取 identity 文件。
3. 不在 im-core public API 暴露 PrivateKeyMaterial。
```

### 8.6 验收

```bash
cargo test -p im-core proof
cargo test -p awiki-cli --test message_contract
cargo test -p awiki-cli --test message_group_e2ee_wire_contract
```

### 8.7 完成标准

```text
1. origin proof 可在 im-core 内生成。
2. awiki-cli direct/group/attachment/secure 旧路径仍通过原测试。
```

---

## 9. PR 15D：迁移普通 group.send wire builder

### 9.1 目标

只迁 group text send 所需 wire builder，不迁 group create/join/leave/add/remove/list/get。

`crates/awiki-cli/src/message/group_wire.rs` 混有 group lifecycle，所以不能整文件无脑搬。这个 PR 采用小子模块级迁移。

### 9.2 迁移范围

```text
build_group_send_rpc_params
signed_group_meta
group_base_meta
signed_params
group send 需要的常量
```

暂不迁移：

```text
build_group_create_rpc_params
build_group_join_rpc_params
build_group_add_rpc_params
build_group_remove_rpc_params
build_group_get_rpc_params
build_group_list_rpc_params
build_group_members_rpc_params
build_group_messages_rpc_params
group profile/policy patch builder
group lifecycle validation
```

### 9.3 目标文件

```text
crates/im-core/src/internal/wire/group.rs
crates/im-core/src/compat/wire.rs
```

### 9.4 执行方式

```text
1. im-core 内只实现 group.send wire。
2. awiki-cli build_group_send_rpc_params 改为 wrapper。
3. group lifecycle 函数仍留在 awiki-cli/src/message/group_wire.rs。
4. 复制 message_group_wire_contract.rs 中 group.send 相关测试到 im-core。
5. awiki-cli 原 group wire contract 继续全量跑。
```

### 9.5 验收

```bash
cargo test -p im-core group_wire
cargo test -p awiki-cli --test message_group_wire_contract
cargo test -p awiki-cli --test msg_ws_group_live_contract
```

### 9.6 完成标准

```text
1. 普通 group.send 的 wire builder 已由 im-core 提供。
2. group lifecycle 未被误迁。
3. group E2EE 未被误动。
```

---

## 10. PR 15E：auth ensure/refresh 真实 provider 边界

### 10.1 目标

让 `im-core` 拥有 session ensure/refresh 的真实执行口，但 CLI 仍可保留 legacy fallback。

当前 `im-core` 的 `AuthService` 仍接近 stub；legacy 的真实逻辑在 `message::auth_session` 和 `refresh_jwt_fallback`。

### 10.2 推荐做法

先做接口收敛，不一次性搬完整 `authsdk`：

```text
1. 在 im-core 定义 internal::auth::SessionProvider trait。
2. 默认实现先使用 im-core 自己的 identity runtime paths。
3. awiki-cli adapter 可以提供 legacy-backed provider。
4. AuthService::ensure_session/refresh_session 从 stub 改为调用 provider。
5. direct send 真迁移前，只验证 auth 状态和刷新路径，不改 message send。
```

### 10.3 目标文件

```text
crates/im-core/src/internal/auth/mod.rs
crates/im-core/src/internal/auth/session.rs
crates/im-core/src/auth/service.rs
crates/awiki-cli/src/im_core_adapter/auth.rs
```

### 10.4 验收

```bash
cargo test -p im-core auth
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli --test authsdk_contract
```

### 10.5 完成标准

```text
1. client.auth().ensure_session(AuthScope::Messaging) 不再只是无条件返回假 session。
2. awiki-cli id refresh-token 行为不变。
3. JWT persistence 仍由 CLI legacy manager 或显式 provider 完成。
```

---

## 11. PR 15F：direct text send 真迁移

### 11.1 目标

`msg send --to` 的普通文本路径可以由 `im-core` 真正执行。

### 11.2 前置条件

```text
1. direct wire 已在 im-core。
2. proof 已在 im-core。
3. auth ensure/refresh 有真实 provider。
4. transport 可先复用 legacy adapter，不强制一次性迁 HTTP client。
```

### 11.3 im-core 内部调用链

```text
MessageService::send
  -> validate Text + Plain
  -> resolve direct target
  -> ensure_session(AuthScope::Messaging)
  -> build direct.send params
  -> authenticated RPC
  -> map service result to SendMessageResult
  -> minimal local store write
```

为了避免大面积重写，第一版建议：

```text
1. target resolve：先由 awiki-cli adapter 注入已解析 DID，im-core 不直接依赖 identity service。
2. authenticated RPC：通过 internal transport trait 调用，awiki-cli 提供 legacy-backed transport。
3. local store write：先做 minimal write，可失败转 warning，不阻断发送。
```

### 11.4 需要新增的抽象

```text
crates/im-core/src/internal/transport.rs
  trait AuthenticatedRpcTransport

crates/im-core/src/internal/message_runtime/direct.rs
  DirectTextSender

crates/im-core/src/messages/service.rs
  send direct plain path
```

`awiki-cli` 侧主要改动：

```text
crates/awiki-cli/src/im_core_adapter/messages.rs
crates/awiki-cli/src/app/msg_handlers.rs
```

实际接入点以当前 P1-alpha adapter 调用路径为准。

### 11.5 验收

```bash
cargo test -p im-core messages
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test msg_live_contract
cargo test -p awiki-cli --test msg_jwt_fallback_trace_contract
```

### 11.6 完成标准

```text
1. 普通 direct text send 可走 im-core。
2. attachment/secure direct 仍走 legacy。
3. unauthorized fallback refresh 行为不回退。
4. local persist 失败仍只产生 warning。
5. AWIKI_USE_IM_CORE_MVP 或等价 feature flag 可回退 legacy。
```

---

## 12. PR 15G：group text send 真迁移

### 12.1 目标

`msg send --group` 的普通文本路径可以由 `im-core` 真正执行。

### 12.2 范围

支持：

```text
普通 group text send
```

不支持：

```text
group lifecycle
group E2EE
attachment
```

### 12.3 调用链

```text
MessageService::send
  -> validate Group target + Text + Plain
  -> ensure_session(AuthScope::GroupMessaging)
  -> build group.send params
  -> authenticated RPC
  -> map result to SendMessageResult
  -> minimal local group message store write
```

### 12.4 验收

```bash
cargo test -p im-core messages
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test group_live_contract
cargo test -p awiki-cli --test msg_ws_group_live_contract
```

### 12.5 完成标准

```text
1. 普通 group text send 可走 im-core。
2. group create/join/leave/add/remove/list/get 仍走 legacy。
3. group E2EE 仍走 legacy。
```

---

## 13. PR 15H：inbox/history P1 子集真迁移

### 13.1 目标

`msg inbox` 和 `msg history` 的 P1 子集可以由 `im-core` 执行。

### 13.2 范围

支持：

```text
direct inbox
direct history
group history 查询的 P1 子集
limit/cursor
基本 Page<Message> 映射
```

暂不迁移：

```text
mark_read 完整行为
复杂 unread count
复杂 cache merge
secure incoming decrypt
contact sync 深度逻辑
conversation projection
```

### 13.3 调用链

```text
MessageService::inbox/history
  -> ensure_session(AuthScope::Messaging)
  -> build inbox/history params
  -> authenticated RPC
  -> map messages to SDK Message DTO
  -> best-effort local persist
  -> return Page<Message>
```

### 13.4 验收

```bash
cargo test -p im-core messages
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test msg_all_inbox_live_contract
cargo test -p awiki-cli --test msg_ws_inbox_live_contract
cargo test -p awiki-cli --test msg_ws_history_live_contract
```

### 13.5 完成标准

```text
1. P1 inbox/history 可走 im-core。
2. legacy fallback 保留。
3. secure decrypt/contact sync/cache merge 不被误重写。
```

---

## 14. 错误映射规则

`im-core` 内部统一返回 `ImError`：

```text
输入缺失 -> ImError::InvalidInput
身份缺失 -> ImError::IdentityRequired / IdentityNotReady
目标找不到 -> ImError::PeerNotFound / GroupNotFound
服务错误 -> ImError::Service
JSON/wire 构造失败 -> ImError::Serialization 或 Internal
不支持能力 -> ImError::UnsupportedCapability
```

`awiki-cli` wrapper 再映射到 legacy 错误：

```text
ImError::InvalidInput(field=text) -> MessageError::TextRequired
ImError::InvalidInput(field=target) -> MessageError::TargetRequired
ImError::InvalidInput(field=group) -> MessageError::GroupRequired
ImError::UnsupportedCapability(attachments) -> MessageError::AttachmentNotSupported
ImError::UnsupportedCapability(secure-direct) -> MessageError::SecureNotSupported
其他 -> MessageError::Internal / Json
```

规则：

```text
1. im-core 不返回 ExitError。
2. im-core 不知道 CLI flag 名。
3. im-core 不直接构造 CLI help/hint 文案。
```

---

## 15. 测试迁移规则

每个切片都执行双测试：

```text
1. im-core 新增对应 contract/unit test。
2. awiki-cli 原测试保留。
3. awiki-cli 原测试验证 wrapper 和 CLI 行为兼容。
4. im-core 测试验证新实现的稳定边界。
```

最小测试矩阵：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test message_contract
cargo test -p awiki-cli --test message_group_wire_contract
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test identity_im_core_mvp_contract
```

涉及实际发送路径时再加：

```bash
cargo test -p awiki-cli --test msg_live_contract
cargo test -p awiki-cli --test group_live_contract
cargo test -p awiki-cli --test msg_jwt_fallback_trace_contract
```

---

## 16. 每个 PR 的完成标准

每个 P1-beta PR 都必须满足：

```text
[ ] im-core 不依赖 awiki-cli。
[ ] awiki-cli public/CLI 行为不变，除非该 PR 明确声明切换路径。
[ ] legacy fallback 仍可用。
[ ] 没有整体搬迁 message/identity/store/runtime。
[ ] 没有把 ParsedCommand、ExitError、Resolved、Manager 暴露到 im-core。
[ ] 新 im-core 测试覆盖被迁移逻辑。
[ ] 旧 awiki-cli 测试仍覆盖 wrapper 兼容。
[ ] Cargo.toml 新依赖有明确必要性，优先复用 workspace 既有依赖。
```

---

## 17. 回滚策略

每个切片都按这个顺序设计，保证能快速回滚：

```text
1. im-core 新实现先落地。
2. awiki-cli wrapper 再切过去。
3. feature flag / adapter fallback 保留一个阶段。
4. 出问题时只回滚 wrapper 调用点，im-core 新代码可以暂时保留但不走生产路径。
5. compat API 稳定两个阶段后再清理 legacy wrapper。
```

---

## 18. 不做事项

P1-beta 明确不做：

```text
1. 不整体迁移 crates/awiki-cli/src/message。
2. 不整体迁移 identity/store/runtime/app handlers。
3. 不把 ParsedCommand / ExitError / Resolved / Manager 带入 im-core。
4. 不迁移 attachment。
5. 不迁移 secure direct。
6. 不迁移 group E2EE。
7. 不迁移完整 group lifecycle。
8. 不迁移复杂 local cache merge / conversation projection。
9. 不引入 async runtime 作为 P1-beta 的前置要求。
```

---

## 19. 方案核心

这套 P1-beta 方案的核心是：

```text
先把低耦合 wire/proof 文件级能力搬到 im-core，
再用 provider / transport / compat 把真实 vertical path 接起来。
```

这样可以避免函数级碎片迁移的高成本，也避免顶层模块整体迁移带来的大面积重写。
