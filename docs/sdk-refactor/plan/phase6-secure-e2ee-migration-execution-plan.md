
# Phase 6：secure direct / group E2EE 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：`docs/sdk-refactor/implementation-playbook.md` 中的 `20. Phase 6：secure direct / group E2EE`  
**建议保存路径**：`docs/sdk-refactor/plan/phase6-secure-e2ee-migration-execution-plan.md`  
**目标**：把 secure direct、secure outbox、incoming decrypt、group E2EE status/repair/send/notice processing 按垂直切片迁入 `crates/im-core`，让普通调用方继续通过 `client.messages().send()` 和 `client.secure()` 的 scoped API 使用 secure 能力，同时保留 `awiki-cli` 对 CLI 参数、输出渲染、危险操作确认、legacy fallback 和系统集成的控制。

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

执行原则沿用前面 plan 的迁移策略：以 leaf-file / 小子模块 / 垂直业务切片为主，不整体搬迁 `message`、`store`、`runtime`、`app handlers`。P1-beta、Phase 4、Phase 5 都采用了这种“小切片、compat wrapper、旧测试保留、默认行为不变”的方式，Phase 6 也应继续保持这个风格。

默认不跑 live/system 测试。单个 PR 的默认目标是 unit / contract / boundary test；真实 secure send、真实 MLS、真实 service、真实 realtime secure decrypt 进入 Manual / live / system 验证层。

---

## 1. 总体结论

Phase 6 不应该一次性把 `crates/awiki-cli/src/message/secure_*`、`group_e2ee_*`、`store/e2ee_outbox.rs` 整体搬进 `im-core`。

推荐迁移粒度：

```text
主策略：secure direct / secure outbox / incoming decrypt / group E2EE 各自按 leaf-file 或小子模块迁移
辅策略：2-5 个强相关文件组成一个垂直业务切片
例外：函数级抽取只用于拆掉少量 CLI 依赖，不作为长期迁移单位
禁止：整体迁移 message 目录
禁止：整体迁移 group_e2ee 目录
禁止：整体迁移 runtime listener
禁止：把 ciphertext / prekey / KeyPackage / MLS provider path 暴露成普通 public API
```

Phase 6 的核心交付：

```text
1. `MessageSecurityPolicy::E2eeRequired` 真正接入 direct secure send 和 group E2EE send。
2. 迁移期兼容 `MessageSecurityMode::SecureDirect` / `GroupE2ee`，但不把这两个实现名作为长期 SDK 语义。
3. `client.secure()` 提供 direct/group/outbox scoped 状态、准备和修复能力。
4. secure outbox failed/retry/drop/flush 进入 im-core internal runtime。
5. inbox/history/realtime 能做 incoming decrypt projection。
6. group E2EE status/repair/MLS notice processing 进入 im-core internal runtime。
7. 附件 E2EE 当前不支持发送；`Attachment + E2eeRequired` 必须 fail-closed 返回 `UnsupportedCapability("secure-attachments")`，不能回退 plaintext attachment。
8. awiki-cli 通过 adapter/compat 渐进切换，legacy fallback 保留至少一个阶段。
```

普通发送仍然通过：

```rust
client.messages().send(SendMessageRequest {
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})
```

或者：

```rust
client.messages().send(SendMessageRequest {
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})
```

目标 SDK 语义是“这条 direct/group 文本消息必须 E2EE”，而不是“调用 SecureDirect 或 GroupE2ee 这套底层实现”。附件 E2EE 不属于 Phase 6 可发送能力，只保留明确的 unsupported 边界。advanced diagnostics 可以通过 feature-gated API 或 non-prelude API 暴露，但不把 low-level crypto 操作暴露给普通调用方。

---

## 2. 当前代码基线

### 2.1 im-core 当前状态

当前 `crates/im-core` 已有 `auth`、`directory`、`groups`、`messages`、`realtime`、`compat`、`internal` 等模块，但还没有 `secure` 模块。

`im-core` 的 feature 已预留：

```toml
attachments = []
realtime = []
secure-direct = []
group-e2ee = []
provider-traits = []
```

这意味着 Phase 6 可以自然按 `secure-direct` 和 `group-e2ee` 分开落地。

当前 `MessageSecurityMode` 已经有迁移期保留 variant：

```rust
pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    SecureDirect,
    GroupE2ee,
}
```

但 `MessageService::send()` 目前仍会对 `SecureDirect` 和 `GroupE2ee` 返回 `UnsupportedCapability`。

Phase 6 推荐新增或重命名为 target-independent 的长期 public 语义：

```rust
pub enum MessageSecurityPolicy {
    Default,
    Plaintext,
    E2eeRequired,
}
```

如果代码 churn 需要分阶段处理，可以先保留 `MessageSecurityMode`，但必须在文档和 adapter 中把 `SecureDirect` / `GroupE2ee` 视为 compatibility aliases，而不是长期推荐接口。

### 2.2 awiki-cli 当前 secure direct 代码

当前 direct secure 能力主要分散在：

```text
crates/awiki-cli/src/message/secure_client.rs
crates/awiki-cli/src/message/secure_control.rs
crates/awiki-cli/src/message/secure_commands.rs
crates/awiki-cli/src/message/secure_incoming.rs
crates/awiki-cli/src/message/secure_outbox_flush.rs
crates/awiki-cli/src/store/e2ee_outbox.rs
crates/awiki-cli/src/message/service.rs
```

`message/mod.rs` 已经 re-export direct secure、secure outbox、group E2EE 的大量函数，说明当前 secure 能力仍在 `awiki-cli` 内聚合。

direct secure send 当前由 `message/service.rs` 中的 `send_secure_direct` 执行：构造 auth session、RPC client、secure E2EE client、发布 prekeys，再调用 `send_text`；如果遇到 pending confirmation，则写入 secure outbox。

`secure_client.rs` 已经封装了 direct E2EE session、prekey bundle、legacy file-backed session/prekey store、`send_text`、`send_json`、`process_incoming` 等核心能力，是 Phase 6 direct secure 的主要迁移源。Phase 6 的长期目标不是把这些 file store 原样搬进 SDK，而是把 direct session、signed prekey、one-time prekey 状态迁入 `im-core` local SQLite store。

`secure_control.rs` 中有 direct secure control payload、ACK/init 判断、pending confirmation 判断、secure outbox queue、current session id 等逻辑。

`secure_outbox_flush.rs` 里已经有非常适合优先迁移的 pure planner：`flush_queued_secure_outbox_rows_plan()`。它可以先搬到 `im-core`，再让 `awiki-cli` 原函数 wrapper 回来。

`secure_incoming.rs` 已经实现 direct E2EE message 检测、按 server_seq 排序、构造 direct E2EE notification、调用 decrypt processor、应用 decrypt result、过滤 secure control message、ACK side effects 和 outbox flush。

### 2.3 awiki-cli 当前 group E2EE 代码

当前 group E2EE 主要分散在：

```text
crates/awiki-cli/src/message/group_e2ee_wire.rs
crates/awiki-cli/src/message/group_e2ee_provider.rs
crates/awiki-cli/src/message/group_e2ee_transport.rs
crates/awiki-cli/src/message/group_e2ee_status.rs
crates/awiki-cli/src/message/group_e2ee_repair.rs
crates/awiki-cli/src/message/group_e2ee_send.rs
crates/awiki-cli/src/message/group_e2ee_decrypt.rs
crates/awiki-cli/src/message/group_e2ee_publish.rs
crates/awiki-cli/src/message/group_e2ee_recover.rs
crates/awiki-cli/src/message/group_e2ee_update.rs
```

`group_e2ee_wire.rs` 是 group E2EE RPC params builder 的主要源文件，包含 `GROUP_E2EE_CIPHER_CONTENT_TYPE = "application/anp-group-cipher+json"` 以及 create/add/remove/leave/send/publish/get key package/recover/update 等 builder。

`group_e2ee_provider.rs` 通过 `anp-mls` 外部 binary 执行 status、key-package、create/add/remove/recover/update、welcome/commit process、encrypt/decrypt 等 MLS 操作，并使用 `AWIKI_ANP_MLS_BINARY` 环境变量定位 binary。

`../anp/rust` 已经完成 MLS API 化后，新的 im-core 路径不应继续复用这个 subprocess/RPC provider。Phase 6 的 `NativeAnpMlsProvider` 应直接调用 `anp::group_e2ee::operations`，并通过 `anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore` 管理 owner/device scoped MLS state。历史 `MlsExecProvider` 只能作为待清理的旧 awiki-cli 路径存在，不能进入新的 im-core group E2EE runtime。

`group_e2ee_status.rs` 已经有 status、pending notice、local MLS status、service head、diagnosis、recovery artifact 等逻辑，是 group diagnostics 的主要迁移源。

`group_e2ee_transport.rs` 封装了 `group.e2ee.head`、`group.e2ee.notice`、`group.e2ee.send`、`group.e2ee.add/remove/recover/update` 等 authenticated RPC 调用。

`group_e2ee_send.rs` 已经实现 group E2EE send 主链路：检测 group 是否使用 E2EE、同步 group state、选择 MLS device、MLS encrypt、调用 `group.e2ee.send`、epoch mismatch 时 repair 后重试、最后持久化本地 message。

---

## 3. 阶段边界

### 3.1 Phase 6 做什么

Phase 6 做：

```text
client.messages().send(... E2eeRequired direct text ...)
client.messages().send(... E2eeRequired group text ...)
client.secure().direct(peer).status()
client.secure().direct(peer).prepare()
client.secure().direct(peer).repair()
client.secure().outbox().list_failed()
client.secure().outbox().retry(outbox_id)
client.secure().outbox().drop(outbox_id)
client.secure().group(group).status()
client.secure().group(group).prepare()
client.secure().group(group).repair()
direct incoming decrypt projection
group E2EE incoming decrypt projection
MLS notice processing
secure outbox failed/retry/drop/flush
```

### 3.2 Phase 6 不做什么

Phase 6 不做：

```text
不暴露 send_direct_cipher(...)
不暴露 process_ciphertext(...)
不暴露 publish_key_package(...) 作为默认 public API
不暴露 process_mls_notice(...) 作为默认 public API
不暴露 build_secure_init_payload(...)
不暴露 build_group_e2ee_*_rpc_params(...)
不暴露 raw ciphertext
不暴露 prekey bundle / KeyPackage / MLS provider binary path
不引入 Phase 7 provider traits 作为 public stable API
不迁移完整 group lifecycle
不迁移 service manager / daemon / systemd / launchd / Windows service
不实现 E2EE 附件发送；`Attachment + E2eeRequired` 只返回 `UnsupportedCapability("secure-attachments")`
不把 CLI output / ParsedCommand / ExitError 带进 im-core
```

KeyPackage、prekey、MLS notice、member recovery/update/rejoin 等能力可以作为 `internal`、`compat` 或 diagnostics-only feature 存在，但不进入默认 public prelude。

---

## 4. 进入条件

开始 Phase 6 前，建议满足：

```text
1. P1 direct/group text send 已稳定。
2. Phase 3 message/group/local_state 基础能力已稳定。
3. Phase 5 realtime runner 已稳定，或者 Phase 6 realtime secure integration 可后置到 PR 6L。
4. im-core auth/session provider 可用。
5. im-core local_state owner identity 隔离已经明确。
6. awiki-cli secure direct/group E2EE legacy 路径仍可回退。
7. im-core boundary 测试确认不引用 CLI 类型。
8. 当前 secure 相关 contract/live 测试基线已记录。
```

进入前建议检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test message_secure_outbox_flush_contract
cargo test -p awiki-cli --test store_e2ee_outbox_contract

rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" \
  crates/im-core/src crates/im-core/tests
```

如果某些 test target 当前不存在，不要在 Codex Goal 中把它们当成已有测试执行；先标注“待新增”。

---

## 5. 目标目录和 API 形态

### 5.1 建议新增目录

```text
crates/im-core/src/secure/
  mod.rs
  dto.rs
  diagnostics.rs
  service.rs

crates/im-core/src/internal/secure_direct/
  mod.rs
  client.rs
  control.rs
  incoming.rs
  outbox.rs
  prekey.rs
  repair.rs
  session_store.rs
  status.rs
  wire.rs

crates/im-core/src/internal/group_e2ee/
  mod.rs
  diagnosis.rs
  incoming.rs
  notices.rs
  provider.rs
  repair.rs
  send.rs
  status.rs
  transport.rs
  wire.rs

crates/im-core/src/compat/
  secure.rs

crates/awiki-cli/src/im_core_adapter/
  secure.rs
```

`crates/im-core/src/lib.rs` 后续增加：

```rust
pub mod secure;

pub use crate::secure::SecureService;
```

`crates/im-core/src/core/client.rs` 增加：

```rust
impl ImClient {
    pub fn secure(&self) -> crate::secure::SecureService<'_> {
        crate::secure::SecureService::new(self)
    }
}
```

### 5.2 Public secure API

建议与 `modules/10-secure.md` 保持一致，默认 public API 使用 scoped service：

```rust
pub struct SecureService<'a> {
    client: &'a ImClient,
}

impl SecureService<'_> {
    pub fn direct(&self, peer: PeerRef) -> DirectSecureConversation<'_>;
    pub fn group(&self, group: GroupRef) -> GroupSecureConversation<'_>;
    pub fn outbox(&self) -> SecureOutboxService<'_>;
}

pub struct DirectSecureConversation<'a> {
    client: &'a ImClient,
    peer: PeerRef,
}

impl DirectSecureConversation<'_> {
    pub fn status(&self) -> ImResult<DirectSecureStatus>;
    pub fn prepare(&self) -> ImResult<DirectSecurePrepareResult>;
    pub fn repair(&self) -> ImResult<DirectSecureRepairResult>;
}

pub struct GroupSecureConversation<'a> {
    client: &'a ImClient,
    group: GroupRef,
}

impl GroupSecureConversation<'_> {
    pub fn status(&self) -> ImResult<GroupSecureStatus>;
    pub fn prepare(&self) -> ImResult<GroupSecurePrepareResult>;
    pub fn repair(&self) -> ImResult<GroupSecureRepairResult>;
}

pub struct SecureOutboxService<'a> {
    client: &'a ImClient,
}

impl SecureOutboxService<'_> {
    pub fn list_failed(&self) -> ImResult<Vec<SecureOutboxEntry>>;
    pub fn retry(&self, outbox_id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
    pub fn drop(&self, outbox_id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
}
```

### 5.3 DTO 建议

```rust
pub struct DirectSecureStatus {
    pub peer: PeerRef,
    pub resolved_peer: Option<Did>,
    pub state: DirectSecureState,
    pub can_send_secure: bool,
    pub pending_outbox_count: u32,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub enum DirectSecureState {
    Ready,
    Preparing,
    WaitingForPeer,
    NeedsRepair,
    Unavailable,
    Unknown,
}

pub struct DirectSecurePrepareResult {
    pub peer: PeerRef,
    pub state: DirectSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

pub struct DirectSecureRepairResult {
    pub peer: PeerRef,
    pub state: DirectSecureState,
    pub repaired: bool,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub struct GroupSecureStatus {
    pub group: GroupRef,
    pub profile: GroupSecurityProfile,
    pub state: GroupSecureState,
    pub can_send_secure: bool,
    pub local_readiness: GroupSecureLocalReadiness,
    pub pending_work: GroupSecurePendingWork,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub enum GroupSecureState {
    Ready,
    Syncing,
    NeedsRepair,
    WaitingForMembershipUpdate,
    MissingLocalState,
    Unavailable,
    Unknown,
}

pub struct GroupSecurePrepareResult {
    pub group: GroupRef,
    pub state: GroupSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

pub struct GroupSecureRepairResult {
    pub group: GroupRef,
    pub state: GroupSecureState,
    pub repaired: bool,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub struct SecureOutboxEntry {
    pub id: SecureOutboxId,
    pub target: MessageTarget,
    pub message_kind: OutboxMessageKind,
    pub status: SecureOutboxStatus,
    pub attempt_count: u32,
    pub last_error: Option<SecureProblem>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub struct SecureOutboxResult {
    pub id: SecureOutboxId,
    pub status: SecureOutboxStatus,
    pub delivery: Option<SecureDelivery>,
    pub warnings: Vec<String>,
}

pub struct SecureProblem {
    pub code: SecureProblemCode,
    pub message: String,
    pub retryable: bool,
}

pub enum SecureProblemCode {
    IdentityNotReady,
    PeerNotFound,
    PeerKeysUnavailable,
    SessionNeedsRepair,
    GroupStateUnavailable,
    LocalStateUnavailable,
    TransportUnavailable,
    Unsupported,
    Unknown,
}

pub struct MessageSecurityReceipt {
    pub requested: MessageSecurityPolicy,
    pub applied: AppliedMessageSecurity,
    pub state: MessageSecurityState,
}

pub enum AppliedMessageSecurity {
    Plaintext,
    DirectE2ee,
    GroupE2ee,
}

pub enum MessageSecurityState {
    Secured,
    QueuedPendingPeerConfirmation,
    FailedClosed,
}
```

原则：

```text
1. DTO 可以表达 status / readiness / recovery hint / secure receipt。
2. DTO 不包含私钥、prekey material、KeyPackage binary、ciphertext body、raw attachment manifest。
3. DTO 不暴露 direct session id、ratchet counter、skipped-key count、MLS epoch、commit/proposal/welcome 等底层状态。
4. DTO 不以 serde_json::Value 作为主要 public 字段。
5. compat 可以临时返回 legacy JSON，但不进入 prelude。
6. Phase 6 不新增 AttachmentSecurityReceipt 或 secure attachment DTO；附件 E2EE 后续单独设计。
```

---

## 6. MessageSecurityPolicy 路由规则

`MessageService::send()` 的 Phase 6 目标行为：

```text
Direct + Default -> 普通 direct send，除非后续 peer policy 明确要求 E2EE
Direct + Plaintext -> 普通 direct send
Direct + E2eeRequired -> direct E2EE send

Group + Default -> 按 group security profile；E2EE group 必须走 E2EE 或 fail-closed
Group + Plaintext -> 显式普通 group send；如果 group policy 要求 E2EE，则 fail-closed
Group + E2eeRequired -> group E2EE send

Attachment + E2eeRequired -> 不支持，返回 UnsupportedCapability("secure-attachments")，不得回退 plaintext attachment
```

迁移期兼容映射：

```text
MessageSecurityMode::DefaultPlain -> MessageSecurityPolicy::Default
MessageSecurityMode::Plain -> MessageSecurityPolicy::Plaintext
MessageSecurityMode::SecureDirect + Direct target -> E2eeRequired
MessageSecurityMode::SecureDirect + Group target -> InvalidInput
MessageSecurityMode::GroupE2ee + Group target -> E2eeRequired
MessageSecurityMode::GroupE2ee + Direct target -> InvalidInput
```

group 的 `Default` 行为必须安全优先：

```text
1. 如果 group snapshot / local secure state 明确显示 group 是 E2EE group，Default 不发送 plaintext。
2. 不做静默 plaintext fallback。
3. Plaintext 是显式 plaintext 请求；如果 group policy 要求 E2EE，则返回 InvalidInput 或 PermissionDenied。
4. E2eeRequired 是 fail-closed：无法确认 secure state active 或修复失败时，不回退 plaintext。
```

---

## 7. 路径和 provider 边界

### 7.1 direct secure local state

当前 legacy direct secure store 使用：

```text
p5-e2ee-sessions
p5-signed-prekeys
p5-one-time-prekeys
```

Phase 6 的长期目标改为：direct E2EE session/prekey state 进入 `im-core` local SQLite store，不再把 `identity_dir` 下的 `p5-*` 文件目录作为新 runtime store。由于当前产品尚未上线，SDK 接口可以按长期形态设计；legacy `p5-*` 文件目录只作为可选导入源或本地开发数据迁移源，不作为新写入目标。

建议 internal contract：

```text
1. ImClient 通过 identity runtime 拿 owner_identity_id / owner_did。
2. direct secure runtime 使用 SqliteDirectSecureStateStore 或等价 internal store 读写 local_state SQLite。
3. session、signed prekey、one-time prekey 均按 owner_identity_id 隔离；owner_did 只作为可读身份字段或兼容索引。
4. awiki-cli 不再向 im-core direct secure runtime 传 identity_dir；adapter 只负责构造 ImCorePaths.local_state。
5. App 通过 ImCorePaths 显式提供 local state root/path。
6. im-core 不自行发现 workspace。
7. 如需兼容本地开发数据，可提供一次性 import/migrate helper，从 legacy p5-* 目录导入到 SQLite；导入完成后 runtime 只读写 SQLite。
```

建议 SQLite shape：

```sql
-- 可复用并升级现有 e2ee_sessions 表，也可以替换为更明确的 direct_e2ee_sessions。
CREATE TABLE IF NOT EXISTS direct_e2ee_sessions (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    peer_did          TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_did),
    UNIQUE (owner_identity_id, session_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_signed_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'active',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, key_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_one_time_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'available',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, key_id)
);
```

这些字段包含敏感本地状态。Phase 6 不强制把 SQLite payload 做静态加密，但实现层必须把它们当 secure local state 处理：不进入 public DTO、不进入日志、不进入 diagnostics 输出、不通过 SDK Interface 暴露原始 material。

### 7.2 group MLS path

群组 E2EE 存储策略需要对接 `../anp/rust` 已完成的 native MLS API 化结果。`anp-mls` 不再作为长期 binary / stdin JSON command surface；Phase 6 的 group E2EE 新路径应直接调用：

```rust
anp::group_e2ee::operations
anp::group_e2ee::storage::{GroupMlsStore, ImCoreSqliteGroupMlsStore}
```

这里的 `anp-mls` 对接指“承接前面从 `src/bin/anp-mls.rs` 抽出的 real OpenMLS library API”，不是在 im-core 中继续调用 `anp-mls` binary。如果前置 anp 改造只完成了 library extraction，但还缺 im-core 所需的 typed operation、store constructor、owner/device scope 参数、error mapping 或 redaction helper，这些缺口都在 Phase 6 内收口。

本计划中的落点：该对接放在 **PR 6H：group E2EE wire / transport / MLS provider internal 边界** 完成。原因是 6I 的 status、6J 的 repair / notice processing、6K 的 send flow 都依赖同一个 MLS provider 边界；如果 6H 没有完成 native anp MLS API 对接，后续阶段会被迫重新引入 binary/RPC 兼容层。

因此 6H 的交付不是“先留一个 provider stub”，而是形成一个后续阶段可以直接消费的 `NativeAnpMlsProvider`：

```text
1. im-core 能通过 internal GroupMlsProvider 调用 anp::group_e2ee::operations。
2. im-core 能通过 anp::group_e2ee::storage 构造 owner/device scoped MLS store。
3. 如果前面 anp-mls API 化仍缺 typed operation、store adapter 或 error mapping，统一在 6H 内补齐。
4. 6I/6J/6K 只能消费 6H 形成的 provider，不再新增 anp-mls binary、stdin JSON command、RPC provider 或 command envelope adapter。
```

6H handoff checklist：

```text
1. anp::group_e2ee::operations 暴露 im-core provider 所需的 typed API，而不是 JSON command envelope。
2. anp::group_e2ee::storage 暴露 im-core 可构造的 owner/device scoped store，不要求 im-core 知道 OpenMLS table schema。
3. anp side 继续负责 OpenMLS StorageProvider、schema migration、pending commit metadata 和 operation redaction。
4. im-core side 只封装 NativeAnpMlsProvider，把 owner_identity_id / owner_did / device_id / local_state path 传给 anp。
5. 6H 结束后，6I/6J/6K 只做 status、repair/notice、send 编排，不再补 anp 连接方式。
```

internal contract：

```text
1. im-core 不直接读写 OpenMLS state，也不直接读写 openmls_* tables。
2. im-core internal GroupMlsProvider 只暴露 status / prepare / finalize / abort / encrypt / decrypt / process welcome / process notice 等 SDK 编排所需能力。
3. NativeAnpMlsProvider 是默认实现，内部调用 anp::group_e2ee::operations，不 spawn anp-mls。
4. anp::group_e2ee::storage 负责 OpenMLS StorageProvider + group_mls_* metadata tables；im-core 只负责传入 owner_identity_id / owner_did / device_id / local_state path。
5. provider path / SQLite path / OpenMLS provider / data_dir 不进入 `client.secure().group()` public DTO。
6. KeyPackage、Welcome、Commit、MLS epoch、pending_commit_id 只能作为 internal/service artifact 或 diagnostics-redacted 信息，不进入普通 SDK public API。
7. 所有会推进 MLS epoch 的本地操作必须走 prepare -> message service RPC -> finalize/abort；prepare 成功但服务端提交失败时不能推进本地 binding epoch。
8. Phase 7 如需公开 provider trait，需要另开 public provider 设计；Phase 6 的 GroupMlsProvider 保持 `pub(crate)` internal。
9. 如果前置 `../anp/rust` MLS API 化仍有未完成项，统一在 PR 6H 补齐 `anp::group_e2ee::operations` / `storage` typed API；后续 6I/6J/6K 只能消费该 API，不能重新适配 `anp-mls` binary、stdin JSON command 或 RPC provider。
```

存储落地策略：

```text
首选：ImCoreSqliteGroupMlsStore 从 ImCorePaths.local_state.sqlite_path 推导 owner/device scoped sibling mls_state.sqlite。
约束：同库同事务只有在后续 spike 证明 OpenMLS SQLite storage 可安全共享 transaction 后再推进。
禁止：新 im-core group E2EE runtime 继续依赖 AWIKI_ANP_MLS_BINARY 或 anp-mls binary path。
允许：awiki-cli legacy group_e2ee_* 在切换完成前短期保留旧 binary 路径；该路径属于待清理历史路径，不作为 Phase 6 新能力的 fallback，也不能被新 im-core runtime 调用。
执行位置：该 native store 的首次对接放在 PR 6H；6I/6J/6K 只基于 6H 形成的 GroupMlsProvider 做 status / repair / send，不再改变 anp 连接方式。
```

### 7.3 compat 规则

如果 `awiki-cli` 需要调用 `im_core::compat::secure`：

```text
1. compat API 使用 #[doc(hidden)]。
2. compat API 不进入 prelude。
3. compat API 不承诺 semver。
4. compat API 只为迁移期 wrapper 服务。
5. 发布独立 crate 前清理或放入 non-default feature。
```

---

## 8. 测试分层规则

### 8.1 Required：单 PR 必跑

```bash
cargo test -p im-core secure
cargo test -p im-core secure_direct
cargo test -p im-core group_e2ee

rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" \
  crates/im-core/src crates/im-core/tests
```

涉及 awiki-cli wrapper 时补跑：

```bash
cargo test -p awiki-cli --test message_secure_outbox_flush_contract
cargo test -p awiki-cli --test runtime_listener_secure_outbox_flush_contract
cargo test -p awiki-cli --test store_e2ee_outbox_contract
```

涉及普通 message/group 路由时补跑：

```bash
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
```

### 8.2 Optional integration：合并前或本地补跑

```bash
cargo test -p awiki-cli --test message_contract
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test store_groups_contract
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

### 8.3 Manual / live / system：默认不跑

```bash
cargo test -p awiki-cli --test msg_secure_status_failed_live_contract
cargo test -p awiki-cli --test msg_secure_repair_live_contract

awiki-cli msg send --to <peer> --text "hello" --secure
awiki-cli msg secure status --with <peer>
awiki-cli msg secure repair --with <peer>
awiki-cli group e2ee status --group <group>
awiki-cli msg send --group <group> --text "hello group" --secure
awiki-cli runtime listener run
```

Manual / live / system 测试只在 PR 明确进入系统验证时运行。

---

## 9. PR 拆分

### PR 6A：Secure service / DTO skeleton

#### 目标

建立 `im-core::secure` public API 形态，不接真实 secure direct 或 group E2EE 实现。默认入口为 `client.secure()`，并提供 `direct(peer)`、`group(group)`、`outbox()` scoped service。

#### 改动范围

```text
crates/im-core/src/secure/mod.rs
crates/im-core/src/secure/dto.rs
crates/im-core/src/secure/diagnostics.rs
crates/im-core/src/secure/service.rs
crates/im-core/src/core/client.rs
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
crates/im-core/tests/secure_api.rs
```

#### 执行步骤

```text
1. 新增 secure module。
2. 新增 SecureService / DirectSecureConversation / GroupSecureConversation / SecureOutboxService。
3. ImClient 新增 secure()。
4. 新增 DirectSecureStatus / DirectSecurePrepareResult / DirectSecureRepairResult / SecureOutboxEntry / GroupSecureStatus 等 DTO。
5. secure scoped methods 先返回 UnsupportedCapability 或 empty status。
6. 新增 MessageSecurityPolicy 草案或 compatibility mapping 文档；MessageService::send 对旧 SecureDirect / GroupE2ee 仍可继续返回 unsupported，直到后续 PR 接线。
7. 增加 API shape 和 boundary tests。
```

#### Required 验收

```bash
cargo test -p im-core secure_api

rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" \
  crates/im-core/src crates/im-core/tests
```

#### 完成标准

```text
1. SecureService 和 scoped service 可编译。
2. 不连接真实服务。
3. 不读取 secure session 文件。
4. 不引入 CLI 类型。
5. CLI 行为零变化。
```

---

### PR 6B：direct secure control 纯逻辑迁移

#### 目标

迁移 direct secure control payload、ACK/init 判断、pending-confirmation 判断和 pending confirmation 识别逻辑。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/secure_control.rs
```

目标：

```text
crates/im-core/src/internal/secure_direct/control.rs
crates/im-core/src/compat/secure.rs
```

#### 迁移范围

可迁移：

```text
SECURE_ACK_SYSTEM_TYPE
SECURE_INIT_SYSTEM_TYPE
build_secure_ack_payload
build_secure_init_payload
is_secure_ack_plaintext
is_secure_init_plaintext
secure_ack_session_id
is_pending_confirmation_error
redacted session summary helper
```

暂不迁移：

```text
queue_secure_outbox_record 的真实 DB 写入
current_secure_session_id 的 legacy Manager 版本
send_secure_direct
secure_status / secure_repair command result
```

#### 执行方式

```text
1. im-core 内实现 pure helper。
2. awiki-cli 原 secure_control.rs 保留函数名。
3. awiki-cli 原函数改成 wrapper 或继续保留未迁移 DB/Manager 逻辑。
4. 复制 pure helper 测试到 im-core。
```

#### Required 验收

```bash
cargo test -p im-core secure_direct_control
cargo test -p awiki-cli --test message_secure_outbox_flush_contract
```

#### 完成标准

```text
1. direct secure control pure logic 由 im-core 覆盖测试。
2. awiki-cli 原调用点不变。
3. 不触发真实 crypto / RPC / SQLite。
```

---

### PR 6C：direct secure client / prekey / session runtime

#### 目标

把 direct E2EE client 的真实 crypto/session/prekey runtime 迁入 `im-core` internal，不接 `messages().send()`。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/secure_client.rs
```

目标：

```text
crates/im-core/src/internal/secure_direct/client.rs
crates/im-core/src/internal/secure_direct/prekey.rs
crates/im-core/src/internal/secure_direct/session_store.rs
crates/im-core/src/internal/secure_direct/sqlite_store.rs
crates/im-core/src/internal/secure_direct/wire.rs
crates/im-core/src/compat/secure.rs
```

#### 迁移范围

支持：

```text
prepare direct secure client from im-core identity runtime
SQLite direct session store
SQLite signed prekey store
SQLite one-time prekey store
ensure_fresh_prekey_bundle
publish_prekey_bundle
send_text
send_json
process_incoming
decrypt_history_page
DID resolver injection / fallback
fake RPC for tests
```

暂不支持：

```text
MessageService::send E2eeRequired direct route
secure status command
secure outbox retry/drop
incoming projection integration
CLI manager path access inside im-core
```

#### 关键改造

把 legacy：

```rust
prepare_secure_e2ee_client_for_record(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
)
```

改成 internal 输入：

```rust
pub(crate) struct DirectSecureClientInput {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub identity_name: String,
    pub signing_key_id: String,
    pub agreement_key_id: String,
    pub signing_private_pem: String,
    pub agreement_private_pem: String,
    pub local_state: LocalStateHandle,
}
```

`LocalStateHandle` 是示意名称；实现可以用现有 im-core local_state repository/connection abstraction。关键约束是 direct secure client 不接收 `identity_dir: PathBuf`，也不自行拼 `p5-e2ee-sessions` / `p5-signed-prekeys` / `p5-one-time-prekeys`。

#### Required 验收

```bash
cargo test -p im-core secure_direct_client
cargo test -p im-core secure_direct_prekey
cargo test -p im-core secure_direct_sqlite_store
```

#### 完成标准

```text
1. direct secure client 可用 temp SQLite + fake RPC 测试。
2. session/prekey 写入发生在 im-core local SQLite store，并按 owner_identity_id 隔离。
3. im-core 不依赖 awiki-cli::Manager / StoredIdentity。
4. awiki-cli secure_client 原路径可 wrapper 或暂时并存，但新 runtime 不再写 legacy p5-* 文件目录。
```

---

### PR 6D：direct status / prepare / repair

#### 目标

实现 `client.secure().direct(peer).status()`、`prepare()`、`repair()`。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/secure_commands.rs
```

目标：

```text
crates/im-core/src/secure/service.rs
crates/im-core/src/secure/dto.rs
crates/im-core/src/internal/secure_direct/status.rs
crates/im-core/src/internal/secure_direct/repair.rs
crates/awiki-cli/src/im_core_adapter/secure.rs
```

#### 支持范围

```text
secure().direct(peer).status()
secure().direct(peer).prepare()
secure().direct(peer).repair()
return DirectSecureStatus / DirectSecurePrepareResult / DirectSecureRepairResult
redact direct session state for advanced diagnostics only
resolve peer via directory runtime
reset peer session state
queue failed outbox records back to queued during repair
```

#### 不支持范围

```text
retry/drop outbox
flush outbox
incoming decrypt integration
group E2EE diagnostics
default public session_id / ratchet counter exposure
```

#### CLI 接入

新增 feature flag：

```text
AWIKI_USE_IM_CORE_SECURE=1
```

`awiki-cli` secure diagnostic command 初期：

```rust
if use_im_core_secure() {
    return run_secure_status_via_im_core(...);
}

run_secure_status_legacy(...)
```

#### Required 验收

```bash
cargo test -p im-core secure_direct_status
cargo test -p im-core secure_direct_repair
cargo test -p awiki-cli --test msg_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg secure status --with <peer>
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg secure init --with <peer>
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg secure repair --with <peer>
```

#### 完成标准

```text
1. status 输出不包含私钥、prekey material、plaintext、session_id、ratchet counter 或 skipped-key count。
2. init/repair 可用 fake RPC 测试。
3. CLI 可 fallback legacy。
4. direct diagnostics 不影响普通 msg send。
```

---

### PR 6E：secure outbox store / planner / failed-retry-drop

#### 目标

把 secure outbox 的 pure planner 和本地 store 操作迁入 `im-core`，实现 failed/list/retry/drop/flush 的 SDK diagnostics 能力。

本次决策：secure outbox 暂时保持现有 SQLite `e2ee_outbox.plaintext` 明文存储，不在 Phase 6 强制加密 outbox payload。后续若需要本地静态加密或 payload envelope，再单独设计迁移计划。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/secure_outbox_flush.rs
crates/awiki-cli/src/store/e2ee_outbox.rs
crates/awiki-cli/src/message/secure_commands.rs
```

目标：

```text
crates/im-core/src/internal/secure_direct/outbox.rs
crates/im-core/src/internal/store/e2ee_outbox.rs
crates/im-core/src/secure/service.rs
crates/im-core/src/compat/secure.rs
```

#### 迁移范围

支持：

```text
QueuedSecureOutboxRow
SecureOutboxSendOutcome
SecureOutboxFlushAction
SecureOutboxFlushPlan
flush_queued_secure_outbox_rows_plan
queue_e2ee_outbox
list_e2ee_outbox
get_e2ee_outbox
mark_e2ee_outbox_sent
set_e2ee_outbox_failure_by_id
update_e2ee_outbox_status
secure().outbox().list_failed()
secure().outbox().retry(outbox_id)
secure().outbox().drop(outbox_id)
flush_outbox(peer) internal runtime helper
```

暂不支持：

```text
runtime listener 自动 flush
incoming ACK 自动 flush
group E2EE outbox
secure attachment outbox
```

#### owner 隔离规则

Phase 6 store 必须兼容已有字段，并建议在未上线前一次性补齐 `owner_identity_id`：

```text
owner_did
credential_name
owner_identity_id
```

写入和查询规则：

```text
1. 新写入同时写 owner_identity_id + owner_did + credential_name。
2. 查询优先 owner_identity_id。
3. 如果已有开发数据缺失 owner_identity_id，可 fallback owner_did / credential_name。
4. 因为产品未上线，可以用干净 schema/migration 补齐 owner_identity_id；但不要删除 `plaintext` 字段。
5. public SecureOutboxEntry 不返回 plaintext，只返回 peer、状态、失败原因、创建/更新时间等摘要。
```

#### Required 验收

```bash
cargo test -p im-core secure_outbox
cargo test -p awiki-cli --test message_secure_outbox_flush_contract
cargo test -p awiki-cli --test store_e2ee_outbox_contract
```

#### 完成标准

```text
1. pure flush planner 已迁入 im-core。
2. awiki-cli 原 tests 仍通过。
3. failed/retry/drop 可通过 `secure().outbox()` 调用。
4. local state owner 隔离测试通过。
5. `e2ee_outbox.plaintext` 明文存储按本阶段决策保留，但不暴露到 SDK public DTO。
```

---

### PR 6F：direct E2EE send flow 接入 MessageService::send

#### 目标

让 `client.messages().send()` 支持 direct `MessageSecurityPolicy::E2eeRequired`。迁移期可兼容 `MessageSecurityMode::SecureDirect + Direct`。

#### 调用链

```text
MessageService::send
  -> validate target/body/security
  -> Direct + E2eeRequired
  -> resolve peer
  -> ensure auth session
  -> build DirectSecureClient
  -> best-effort publish prekey bundle
  -> send_text
  -> if pending confirmation: queue secure outbox
  -> persist local outgoing message
  -> return SendMessageResult
```

#### 改动范围

```text
crates/im-core/src/messages/service.rs
crates/im-core/src/internal/message_runtime/direct.rs
crates/im-core/src/internal/secure_direct/client.rs
crates/im-core/src/internal/secure_direct/outbox.rs
crates/im-core/src/internal/local_state/messages.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
crates/awiki-cli/src/im_core_adapter/secure.rs
```

#### 行为规则

```text
Direct + E2eeRequired + Text -> secure direct send
Direct + E2eeRequired + Attachment -> 不支持，UnsupportedCapability("secure-attachments")
Group + E2eeRequired -> 由 group E2EE route 处理，不进入 direct route
旧 SecureDirect + Group target -> InvalidInput
Secure send success -> DeliveryState::Accepted 或 Sent
Pending confirmation -> DeliveryState::StoredLocally 或 queued metadata
Crypto/RPC failure -> ImError::Service / Internal / TransportUnavailable
```

#### Required 验收

```bash
cargo test -p im-core secure_direct_send
cargo test -p im-core messages
cargo test -p awiki-cli --test msg_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg send --to <peer> --text "hello" --secure
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg secure failed
```

#### 完成标准

```text
1. Direct + E2eeRequired 不再返回 UnsupportedCapability。
2. success / pending-confirmation / failure 三条路径均有 unit test。
3. pending-confirmation 会写 secure outbox。
4. plaintext direct send 路径不变。
5. legacy fallback 可关闭新路径。
```

---

### PR 6G：direct incoming decrypt projection

#### 目标

把 direct E2EE incoming decrypt 接入 inbox/history/realtime projection，但不向 public API 暴露 ciphertext。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/secure_incoming.rs
```

目标：

```text
crates/im-core/src/internal/secure_direct/incoming.rs
crates/im-core/src/internal/message_runtime/read.rs
crates/im-core/src/internal/realtime/projection.rs
crates/im-core/src/compat/secure.rs
```

#### 支持范围

```text
detect direct E2EE wire content type
build direct E2EE notification from message view
sort decrypt order by server_seq / id
process incoming init/cipher
apply decrypt result to message projection
filter secure control messages
ACK side effect planning
outbox flush after ACK
read-only decrypt mode
with-side-effects decrypt mode
```

#### 关键模式

建议拆两个模式：

```rust
pub(crate) enum DirectDecryptMode {
    ReadOnly,
    WithSideEffects,
}
```

用途：

```text
inbox/history 默认 ReadOnly，避免 read API 隐式发送 ACK、flush outbox 或触发修复。
realtime runner 可 WithSideEffects。
CLI adapter 如需保持旧行为，可显式 opt-in WithSideEffects。
diagnostic preview 必须 ReadOnly。
```

#### Public projection 规则

```text
1. decrypted text -> MessageBodyView::Text
2. decrypted json attachment manifest -> Unsupported 或后续 Attachment body view
3. secure control message -> 不进入普通 message 列表，或 metadata 标记后过滤
4. undecryptable message -> Unsupported + warning，不返回 ciphertext
5. decrypt failure 不阻断整个 inbox/history page
```

#### Required 验收

```bash
cargo test -p im-core secure_direct_incoming
cargo test -p im-core messages_read
cargo test -p awiki-cli --test msg_contract
```

#### Optional integration

```bash
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

#### 完成标准

```text
1. direct E2EE inbox/history 可以返回 plaintext projection。
2. secure ACK/init control 不污染普通消息列表。
3. decrypt failure 不泄露 ciphertext。
4. ACK side effects 和 outbox flush 可测试。
```

---

### PR 6H：group E2EE wire / transport / MLS provider internal 边界

#### 目标

迁移 group E2EE wire builder、authenticated transport 和 MLS provider internal boundary，并在这一阶段完成 im-core 对 `../anp/rust` native MLS API 的直接对接；但不接 group `messages().send(... E2eeRequired ...)` route。

这个阶段是承接前置 `anp-mls` API 化工作的最合适位置：status、repair、notice processing 和 send 都依赖同一个 internal provider 边界。如果 6H 仍保留 binary/RPC provider，后续 6I/6J/6K 会继续把旧链路带进 im-core。

因此 6H 也是 native MLS 对接的收口阶段：如果 `../anp/rust` 只完成了部分 library extraction，或者 typed operation / store API 与 im-core 需要的 provider matrix 不完全匹配，应在本阶段同步补齐 anp 侧 API。6I/6J/6K 不再新增 `anp-mls` 兼容层，也不再通过 subprocess/RPC 间接调用 MLS。

本阶段新增明确决策：

```text
1. 6H 先完成 anp MLS native API acceptance，再迁移依赖该 provider 的 im-core group runtime 入口。
2. `../anp/rust` 的未完成对接项属于 6H 范围；实现时可以在同一 PR/同一任务链中先改 anp，再改 im-core adapter。
3. 6H 结束时，`NativeAnpMlsProvider` 必须是真实 anp library adapter，不是留给 6I/6J/6K 补的占位实现。
4. 后续 6I/6J/6K 不允许为了赶功能而回退到 `anp-mls` binary、stdin JSON command、stdout/stderr 解析或 RPC subprocess。
5. 如果 6H 发现前置 anp API 仍不能表达某个 MLS 操作，应优先补 `../anp/rust` 的 `group_e2ee::operations/storage`，而不是在 im-core 里访问 OpenMLS provider、复刻 command dispatcher 或保留 binary fallback。
```

#### 源和目标

源：

```text
crates/awiki-cli/src/message/group_e2ee_wire.rs
crates/awiki-cli/src/message/group_e2ee_transport.rs
crates/awiki-cli/src/message/group_e2ee_provider.rs
```

目标：

```text
crates/im-core/src/internal/group_e2ee/wire.rs
crates/im-core/src/internal/group_e2ee/transport.rs
crates/im-core/src/internal/group_e2ee/provider.rs
crates/im-core/src/internal/group_e2ee/native_provider.rs
crates/im-core/src/internal/group_e2ee/storage.rs
crates/im-core/src/compat/secure.rs
```

#### 支持范围

```text
build_group_e2ee_head_rpc_params
build_group_e2ee_notice_rpc_params
build_group_e2ee_send_rpc_params
build_group_e2ee_get_key_package_rpc_params
build_group_e2ee_recover_member_rpc_params
GROUP_E2EE_CIPHER_CONTENT_TYPE
GroupE2eeTransport internal RPC boundary
GroupMlsProvider internal trait
NativeAnpMlsProvider direct library adapter
ImCoreSqliteGroupMlsStore owner/device-scoped store construction
fake GroupMlsProvider for tests
legacy MlsExecProvider cleanup note only; not a Phase 6 im-core provider or fallback
```

Native provider 必须覆盖 `anp::group_e2ee::operations` 的 typed operation matrix：

```text
generate_key_package
create_group_prepare
add_member_prepare
remove_member_prepare
leave_prepare
update_member_prepare
recover_member_prepare
finalize_commit
abort_commit
status
process_welcome
process_notice
encrypt
decrypt
```

如果上面的 operation 在 `../anp/rust` 中尚不存在、仍然只存在于历史 binary command path，6H 必须先把它抽成 library API，再接入 NativeAnpMlsProvider。不要在 im-core 中复刻 OpenMLS 细节，也不要把历史 command envelope 搬进 SDK runtime。

#### 不支持范围

```text
group E2EE public KeyPackage API
group lifecycle public migration
group e2ee send route
notice repair execution
real MLS live/service test by default
OpenMLS table access from im-core
anp-mls binary path / stdin JSON command compatibility in new im-core runtime
```

#### MlsProvider 规则

```text
1. GroupMlsProvider 是 im-core internal trait，不是 Phase 7 public provider。
2. NativeAnpMlsProvider 是新 im-core runtime 的默认实现，直接调用 anp::group_e2ee::operations。
3. NativeAnpMlsProvider 通过 anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore 打开 owner/device-scoped MLS state。
4. im-core 只传 owner_identity_id / owner_did / device_id / local_state sqlite path，不传 binary path。
5. im-core 不读取 OpenMLS private tables，不把 OpenMLS StorageProvider 暴露到 SDK Interface。
6. MlsExecProvider 只能留在 awiki-cli legacy/compat 侧，不能作为新 im-core runtime 的 provider。
7. provider error 需要映射到 ImError/SecureProblem，不把 stdout/stderr、SQLite path 或 OpenMLS raw error 直接暴露给普通 SDK result。
8. anp side 的 OpenMLS provider、SQLite schema migration、pending commit 表和 metadata 表继续由 anp::group_e2ee::storage 管理；im-core 不为绕过 API 而直接操作这些表。
```

#### PR 6H 内部顺序

```text
6H-0. anp MLS native API acceptance：在 ../anp/rust 确认 anp::group_e2ee::operations/storage 是唯一 MLS library surface，且不需要 anp-mls binary。
6H-1. 对齐 anp native API：确认 operations/storage 覆盖 provider matrix；缺失的 typed operation、store adapter、owner/device scoped constructor、error type 或 redaction helper 先在 ../anp/rust 补齐。
6H-2. 引入 im-core internal GroupMlsProvider trait 和 fake provider，先用 typed DTO 固定 SDK 编排边界。
6H-3. 实现 NativeAnpMlsProvider，直接调用 anp::group_e2ee::operations，并使用 ImCoreSqliteGroupMlsStore 构造 owner/device-scoped store。
6H-4. 迁移 wire/transport builder，保持 message service RPC 只负责分发 commit/welcome/cipher/notice。
6H-5. 用 temp local_state 和 anp typed operations 做最小 native provider 验证；如果失败，修 anp API 或 storage adapter，不回退到 anp-mls binary。
6H-6. 移除新 im-core runtime 对 legacy MlsExecProvider / command envelope 的依赖；awiki-cli legacy 旧路径只作为后续 compat cleanup 的清理对象，不作为新 runtime fallback。
6H-7. 做 boundary grep：im-core group E2EE 新路径不能出现 AWIKI_ANP_MLS_BINARY、anp-mls command、stdin/stdout JSON command envelope、MlsExecProvider 或 anp::group_e2ee::commands。
```

#### Required 验收

```bash
cargo test -p im-core group_e2ee_wire
cargo test -p im-core group_e2ee_provider
cargo test -p im-core --features group-e2ee group_e2ee_native_provider
cargo test -p awiki-cli --test group_contract
```

#### 完成标准

```text
1. group E2EE wire shape 由 im-core 覆盖测试。
2. awiki-cli 原 group_e2ee_wire 可 wrapper。
3. provider 可 fake 测试。
4. NativeAnpMlsProvider 可用 temp local_state + anp typed operations 创建/finalize group 并读取 status。
5. create/add/remove/update/recover/leave 等 epoch-changing 本地操作只 prepare，不在 service RPC 前推进 local binding epoch。
6. KeyPackage / Welcome / Commit / MLS epoch / pending_commit_id 不进入普通 public DTO。
7. anp-mls binary path 不进入 public API，且新 im-core runtime 不 spawn anp-mls。
8. `crates/im-core/src/internal/group_e2ee` 不引用 `anp::group_e2ee::commands`、`AWIKI_ANP_MLS_BINARY`、`MlsExecProvider` 或 `anp-mls` command envelope。
9. 6I/6J/6K 可以只依赖 GroupMlsProvider 完成 status / repair / send，不需要再修改 anp 连接方式。
```

---

### PR 6I：group E2EE status

#### 目标

实现 `client.secure().group(group).status()`。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/group_e2ee_status.rs
```

目标：

```text
crates/im-core/src/internal/group_e2ee/status.rs
crates/im-core/src/internal/group_e2ee/diagnosis.rs
crates/im-core/src/secure/service.rs
crates/im-core/src/secure/dto.rs
crates/awiki-cli/src/im_core_adapter/secure.rs
```

#### 迁移范围

支持：

```text
local MLS status
candidate device id selection
service head
pending notices status
group_e2ee_recovery_diagnosis
group_e2ee_recovery_artifact
group_e2ee_local_epoch_from_status
status rank / diagnosis matrix
public GroupSecureStatus projection without MLS epoch / raw notice / KeyPackage
```

暂不支持：

```text
repair execution
MLS notice processing
group E2EE send
publish/update/recover member public API
```

#### Diagnosis 状态建议

```text
in_sync
pending_notices
pending_commit
missing_state
epoch_lag
local_ahead
inactive
local_only
unknown
```

#### Required 验收

```bash
cargo test -p im-core group_e2ee_status
cargo test -p im-core group_e2ee_diagnosis
cargo test -p awiki-cli --test group_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli group e2ee status --group <group>
```

#### 完成标准

```text
1. `secure().group(group).status()` 可聚合 local secure state + service head + pending work。
2. diagnosis matrix 有单元测试。
3. public recovery hint 不泄露 KeyPackage、MLS epoch、Welcome、Commit 或 raw notice。
4. CLI legacy status 可 fallback。
```

---

### PR 6J：group E2EE repair / MLS notice processing

#### 目标

实现 `client.secure().group(group).repair()`，迁入 MLS notice processing 的 internal runtime。

#### 源和目标

源：

```text
crates/awiki-cli/src/message/group_e2ee_repair.rs
crates/awiki-cli/src/message/group_e2ee_decrypt.rs
crates/awiki-cli/src/message/group_e2ee_recover.rs
crates/awiki-cli/src/message/group_e2ee_update.rs
```

目标：

```text
crates/im-core/src/internal/group_e2ee/repair.rs
crates/im-core/src/internal/group_e2ee/notices.rs
crates/im-core/src/internal/group_e2ee/incoming.rs
crates/im-core/src/secure/service.rs
```

#### 支持范围

```text
pull pending notices
process welcome notice
process commit notice
finalize local pending commit
abort failed pending commit where needed
repair stale local epoch
return GroupSecureRepairResult
refresh `secure().group(group).status()` projection after repair
```

#### 不支持范围

```text
完整 group lifecycle public API
owner-assisted recover member as default public API
update member key as default public API
publish key package as default public API
```

这些能力可以留在 `compat` 或 diagnostics-only advanced path。

#### Required 验收

```bash
cargo test -p im-core group_e2ee_repair
cargo test -p im-core group_e2ee_notices
cargo test -p awiki-cli --test group_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli group e2ee repair --group <group>
```

#### 完成标准

```text
1. repair 可用 fake transport + fake MLS provider 测试。
2. pending notice 处理失败时 fail-closed。
3. repair 不发送 plaintext fallback。
4. repair 后 status 可反映新的 local/service state。
```

---

### PR 6K：group E2EE send flow 接入 MessageService::send

#### 目标

让 `client.messages().send()` 支持 group `MessageSecurityPolicy::E2eeRequired`。迁移期可兼容 `MessageSecurityMode::GroupE2ee + Group`。

#### 调用链

```text
MessageService::send
  -> validate target/body/security
  -> Group + E2eeRequired
  -> ensure auth session
  -> inspect group E2EE local status
  -> optional sync group state
  -> MLS encrypt
  -> group.e2ee.send
  -> if epoch mismatch: repair notices once, then retry
  -> persist local outgoing group message with is_e2ee = true
  -> return SendMessageResult
```

#### 源和目标

源：

```text
crates/awiki-cli/src/message/group_e2ee_send.rs
```

目标：

```text
crates/im-core/src/internal/group_e2ee/send.rs
crates/im-core/src/internal/message_runtime/group.rs
crates/im-core/src/messages/service.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
```

#### 行为规则

```text
Group + E2eeRequired + Text -> group E2EE send
Group + E2eeRequired + Attachment -> 不支持，UnsupportedCapability("secure-attachments")
Direct + E2eeRequired -> direct E2EE route，不进入 group route
旧 GroupE2ee + Direct target -> InvalidInput
Missing local MLS state -> fail-closed
Epoch mismatch -> repair once, retry once
Repair failure -> fail-closed
Plaintext fallback -> 禁止
```

#### Required 验收

```bash
cargo test -p im-core group_e2ee_send
cargo test -p im-core messages
cargo test -p awiki-cli --test group_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli msg send --group <group> --text "hello group" --secure
```

#### 完成标准

```text
1. Group + E2eeRequired 不再返回 UnsupportedCapability。
2. active MLS group send success 有 unit test。
3. epoch mismatch repair + retry 有 unit test。
4. missing MLS state fail-closed。
5. group plaintext send 路径不受影响。
```

---

### PR 6L：realtime secure integration / compat cleanup

#### 目标

把 direct secure incoming decrypt、secure outbox flush、group E2EE MLS notice processing 接入 realtime runner 的 event projection，并开始清理稳定 compat。

#### 目标文件

```text
crates/im-core/src/internal/realtime/projection.rs
crates/im-core/src/internal/secure_direct/incoming.rs
crates/im-core/src/internal/secure_direct/outbox.rs
crates/im-core/src/internal/group_e2ee/incoming.rs
crates/im-core/src/internal/group_e2ee/notices.rs
crates/awiki-cli/src/im_core_adapter/realtime.rs
crates/awiki-cli/src/im_core_adapter/secure.rs
```

#### 支持范围

```text
direct E2EE notification -> decrypt -> ImEvent::MessageReceived plaintext projection
direct secure ACK -> flush secure outbox
direct secure init -> send ACK where policy allows
group E2EE notice notification -> process MLS notice
group E2EE cipher message -> decrypt projection where local MLS state exists
Unknown/undecryptable secure event -> warning / unsupported event
```

#### 不支持范围

```text
runtime listener service manager migration
OpenClaw / Hermes delivery migration
secure attachment realtime enrichment
public raw notification API
```

#### Required 验收

```bash
cargo test -p im-core realtime
cargo test -p im-core secure_direct_incoming
cargo test -p im-core group_e2ee_notices
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

#### Manual / live

```bash
AWIKI_USE_IM_CORE_SECURE=1 awiki-cli runtime listener run
```

#### 完成标准

```text
1. realtime secure direct event 可投影为 plaintext ImEvent。
2. secure control event 不作为普通消息展示。
3. group MLS notices 可触发 internal processing。
4. 不改变 service install/start/stop 行为。
5. 已稳定 compat wrapper 标注清理或删除。
```

---

## 10. 错误映射规则

`im-core` 使用领域错误：

```text
InvalidInput
IdentityRequired
IdentityNotReady
AuthRequired
SessionExpired
PermissionDenied
PeerNotFound
GroupNotFound
TransportUnavailable
UnsupportedCapability
LocalStateUnavailable
PathUnavailable
Service
Internal
```

Phase 6 推荐新增或复用这些语义：

```text
IdentityNotReady -> 缺少 DID signing key / X25519 E2EE key / MLS local state
UnsupportedCapability("secure-direct") -> feature 未启用或路径未实现
UnsupportedCapability("group-e2ee") -> feature 未启用或路径未实现
UnsupportedCapability("secure-attachments") -> secure attachment 未实现
InvalidInput -> target/security mode 不匹配
PermissionDenied -> group policy 不允许 plaintext 或 actor 非 member
LocalStateUnavailable -> secure outbox / MLS state 不可用
TransportUnavailable -> RPC / WebSocket / network 不可用
Service -> message service / group.e2ee service 返回错误
Internal -> crypto provider / MLS provider / serialization 内部错误
```

`awiki-cli` adapter 映射为 CLI hint：

```text
IdentityNotReady -> 提示补齐身份注册 / E2EE key
UnsupportedCapability -> 阶段未支持提示
PermissionDenied -> group membership / security policy 提示
LocalStateUnavailable -> local store / repair 提示
TransportUnavailable -> 网络或 listener 提示
Service -> 保持现有 service error envelope
```

规则：

```text
1. im-core 不返回 ExitError。
2. im-core 不知道 CLI command 名称。
3. im-core 不生成 systemd/launchd/Windows 文案。
4. awiki-cli 负责 pretty/json/table output。
```

---

## 11. 回滚策略

### 11.1 feature / env 开关

建议保留独立 secure 开关：

```text
AWIKI_USE_IM_CORE_SECURE=1
```

以及 crate feature：

```text
secure-direct
group-e2ee
```

迁移早期：

```text
1. im-core secure module 可以存在。
2. MessageSecurityPolicy::E2eeRequired 可逐步从 unsupported 变成可用；旧 MessageSecurityMode::SecureDirect / GroupE2ee 作为兼容入口映射到 E2eeRequired。
3. awiki-cli 默认仍可走 legacy secure path。
4. 出问题时关闭 AWIKI_USE_IM_CORE_SECURE 回到 legacy。
```

### 11.2 direct 和 group 独立回滚

```text
1. direct E2EE send 独立于 group E2EE send。
2. direct diagnostics 独立于 direct send。
3. group status/repair 独立于 group send。
4. realtime secure integration 最后接入，出问题只回滚 realtime adapter。
```

### 11.3 本地状态回滚

```text
1. direct E2EE session/prekey 的长期 runtime store 是 im-core local SQLite；不回滚到 p5-* 文件目录作为新写入目标。
2. 如果 SQLite direct store 出问题，可临时关闭 AWIKI_USE_IM_CORE_SECURE 回到 awiki-cli legacy direct secure path。
3. secure outbox 暂时保留 e2ee_outbox.plaintext 明文字段，不删除 legacy e2ee_outbox 字段。
4. group MLS 的新 im-core runtime 使用 anp native store；如需回滚，关闭新 group E2EE runtime 或回滚 adapter 接入，不让新 im-core runtime 重新依赖 anp-mls binary。
5. compat wrapper 至少保留一个阶段。
```

---

## 12. 明确不做事项

Phase 6 不做：

```text
1. 不迁移完整 group lifecycle。
2. 不把 group e2ee create/add/remove/update 作为默认 public SDK API。
3. 不把 KeyPackage / prekey bundle 暴露给普通调用方。
4. 不暴露 ciphertext send/process API。
5. 不迁移 systemd / launchd / Windows service manager。
6. 不迁移 OpenClaw / Hermes host notification delivery。
7. 不实现 E2EE 附件发送；后续设计完成前，`Attachment + E2eeRequired` 一律 fail-closed。
8. 不把 MLS provider trait 作为 Phase 6 public provider。
9. 不把 anp-mls binary path 放进 prelude 或普通 API。
10. 不让 im-core 依赖 awiki-cli。
11. 不把 CLI output envelope / ParsedCommand / ExitError 带入 im-core。
12. 不在 secure send 失败时回退 plaintext。
```

---

## 13. 最小验收标准

Phase 6 完成后应满足：

```text
1. im-core 有 secure module、SecureService 和 direct/group/outbox scoped service。
2. MessageSecurityPolicy::E2eeRequired 可用于 direct text send。
3. MessageSecurityPolicy::E2eeRequired 可用于 group text send。
4. direct secure status/prepare/repair 可用。
5. secure outbox failed/retry/drop 可通过 `secure().outbox()` 使用，flush 作为 internal runtime helper 可用。
6. direct inbox/history/realtime incoming decrypt projection 可用。
7. group E2EE status/repair/MLS notice processing 可用，public DTO 不暴露 MLS epoch / KeyPackage / raw notice。
8. group E2EE send fail-closed，不回退 plaintext。
9. im-core 不暴露 ciphertext/prekey/KeyPackage/MLS binary path，也不 spawn anp-mls。
10. awiki-cli legacy secure path 可 fallback。
11. awiki-cli 现有 secure/group/message/store contract tests 继续通过。
12. boundary grep 不出现 CLI 类型引用。
```

---

## 14. 推荐 PR 顺序汇总

```text
PR 6A：Secure service / DTO skeleton
PR 6B：direct secure control 纯逻辑迁移
PR 6C：direct secure client / prekey / session runtime
PR 6D：direct status / prepare / repair
PR 6E：secure outbox store / planner / failed-retry-drop
PR 6F：direct E2EE send flow 接入 MessageService::send
PR 6G：direct incoming decrypt projection
PR 6H：group E2EE wire / transport / MLS provider internal 边界（含 anp MLS native API 对接收口）
PR 6I：group E2EE status
PR 6J：group E2EE repair / MLS notice processing
PR 6K：group E2EE send flow 接入 MessageService::send
PR 6L：realtime secure integration / compat cleanup
```

---

## 15. 一句话执行原则

**先迁 secure 的 pure helper、diagnosis、outbox planner 和 internal runtime；再让 `MessageSecurityPolicy::E2eeRequired` 接入 direct/group `client.messages().send()`；最后把 incoming decrypt 和 MLS notices 接进 inbox/history/realtime，同时保持 CLI legacy fallback 和 fail-closed 安全策略。**
