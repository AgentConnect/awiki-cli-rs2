# IM Core 渐进式迁移执行手册

**建议保存路径**：`docs/sdk-refactor/implementation-playbook.md`  
**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**目标**：在尽量不改底层模块、不破坏现有测试的前提下，让 `crates/im-core` 先跑起来，再逐步把核心能力迁移进去。  
**核心策略**：原仓库新分支 / worktree + 高层 SDK façade + CLI adapter + 垂直切片迁移。

---

## 1. 总体结论

不要新建仓库，也不要第一步就把 `identity/*`、`message/*`、`store/*`、`runtime/*` 这类底层模块整体搬到 `im-core`。

推荐路线是：

```text
Step 1：在原仓库新增 crates/im-core，只放 SDK 高层类型和最小 façade。
Step 2：在 awiki-cli 内新增 im_core_adapter，让 CLI handler 像调用 SDK 一样调用现有能力。
Step 3：保持底层模块和底层测试基本不动。
Step 4：先让 Phase 1 主链路跑通：身份 + auth + Handle 注册 + 私聊文本 + 群聊文本 + 必要 inbox/history。
Step 5：稳定后，再按“垂直业务切片”逐步把实现迁入 im-core。
```

这条路线的目标不是马上完成最终 SDK，而是先回答：

```text
1. ImCore / ImClient 这套高层 API 是否能嵌入 CLI？
2. CLI handler 是否能从业务实现中脱离出来？
3. 不大改底层模块的情况下，是否能跑通身份、私聊、群聊主链路？
4. 现有底层测试是否还能继续保护迁移？
```

---

## 2. 为什么不新建仓库

不建议新建仓库，原因如下：

```text
1. 现有底层模块测试都在 awiki-cli-rs2 中，新仓库会丢掉最重要的保护网。
2. 现有模块之间还有大量 crate 内部依赖，新仓库会立即引入复制代码和依赖搬运问题。
3. 新仓库容易形成“第二套实现”，后续与 CLI 主仓同步成本高。
4. 真正要验证的是 awiki-cli 是否能逐步切到 SDK 边界，这必须在同一个 workspace 中验证。
5. im-core 未来可以独立发布 crate，但不要求第一天就独立仓库开发。
```

推荐开发方式：

```bash
git checkout -b im-core-mvp
```

或者使用 worktree：

```bash
git worktree add ../awiki-cli-rs2-im-core im-core-mvp
cd ../awiki-cli-rs2-im-core
```

---

## 3. 迁移总架构

第一阶段采用三层过渡架构：

```text
CLI handler
   |
   v
awiki-cli::im_core_adapter        # 过渡层，可调用旧模块
   |
   v
现有底层模块                    # identity/authsdk/message/store/runtime 基本不动
```

同时新增真正的 SDK crate：

```text
crates/im-core
   |
   v
SDK public API / DTO / ImError / ImCore / ImClient
```

关键规则：

```text
1. im-core 不能依赖 awiki-cli。
2. awiki-cli 可以依赖 im-core。
3. awiki-cli::im_core_adapter 是过渡层，可以依赖旧模块。
4. im_core_adapter 后续随着真实 im-core 实现补齐而逐步删除。
5. 底层模块先保留现状，优先复用现有测试。
```

依赖方向：

```text
crates/awiki-cli
   ├── depends on crates/im-core
   └── has im_core_adapter that calls legacy modules

crates/im-core
   └── no dependency on awiki-cli
```

---

## 4. Phase 1 的两个层次

Phase 1 建议拆成两个层次，避免第一步就大改底层模块。

### 4.1 P1-alpha：接口和调用路径跑通

目标：

```text
im-core 的 public API 形态存在。
CLI handler 能通过 SDK DTO / ImCore / ImClient 形态调用能力。
adapter 内部暂时继续调用旧模块。
底层模块和测试基本不变。
```

P1-alpha 重点验证边界，不追求 `im-core` 已经完全独立实现所有业务。

### 4.2 P1-beta：把最小垂直切片迁入 im-core

目标：

```text
在 P1-alpha 证明 API 合理后，再把最小业务切片逐步迁入 im-core。
每次只迁移一个小切片，例如 auth refresh、direct text send、group text send。
旧模块通过 re-export 或 wrapper 保持兼容一段时间。
```

P1-beta 才开始真正移动部分底层实现，但仍然避免按目录整体搬迁。

---

## 5. 全局保护规则

迁移期间必须遵守以下规则。

### 5.1 不整体搬目录

不要一开始做这些事情：

```text
不要整体搬 identity/*
不要整体搬 message/*
不要整体搬 store/*
不要整体搬 runtime/*
不要整体搬 app/*_handlers.rs
```

正确做法是按业务切片迁移：

```text
auth refresh
direct text send
group text send
inbox/history
Handle register
```

### 5.2 现有底层测试不能丢

每个阶段都应该跑：

```bash
cargo test -p awiki-cli
cargo test -p im-core
```

如果某个 helper 从 `awiki-cli` 搬到 `im-core`：

```text
1. 把原测试复制或迁移到 im-core。
2. awiki-cli 原位置先保留 re-export 或 wrapper。
3. 旧测试继续过一段时间。
4. 等新路径稳定后再删除旧 wrapper。
```

### 5.3 新 SDK 不暴露底层细节

`im-core` public API 不得暴露：

```text
ParsedCommand
ExitError
GlobalOptions
config::Resolved
identity::Manager
ActorContext
LocalStatePaths in business calls
AuthStatePaths in business calls
SecureStatePaths in business calls
RPC params
raw JSON payload
SQLite connection
WebSocket frame
secure session / prekey / MLS path
```

### 5.4 Phase 1 使用 blocking-first

第一阶段不强制 async：

```rust
pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
```

原因：

```text
1. 当前 CLI 迁移主要目标是边界收敛。
2. 不要同时引入 async runtime、async trait、tokio、spawn_blocking、lifetime 复杂度。
3. App 需要异步时，可在 Flutter plugin / mobile binding / 上层 runtime 放到 worker thread。
```

### 5.5 P1 不迁移高级能力

P1 不做：

```text
附件 upload/download
realtime runner / daemon
direct E2EE
group E2EE
MLS
secure outbox
provider traits
完整 group lifecycle
完整 directory/profile
conversation projection
mark-read 完整状态收口
```

这些能力进入后续阶段。

---

## 6. Phase 0：准备与基线固定

### 6.1 目标

在正式改代码前固定基线，避免迁移过程中不知道哪里被破坏。

### 6.2 具体步骤

#### Step 0.1 创建工作分支或 worktree

```bash
git checkout -b im-core-mvp
```

或者：

```bash
git worktree add ../awiki-cli-rs2-im-core im-core-mvp
cd ../awiki-cli-rs2-im-core
```

#### Step 0.2 跑当前基线测试

```bash
cargo test -p awiki-cli
cargo test --workspace
```

如果当前 workspace 全量测试已经有已知失败，记录下来：

```text
docs/sdk-refactor/baseline-test-notes.md
```

建议记录：

```text
命令
日期
通过/失败
失败测试名
是否历史已知
```

#### Step 0.3 定义 smoke commands

准备一组后续每阶段都要试的 CLI 命令：

```bash
awiki-cli id list
awiki-cli id current
awiki-cli id status
awiki-cli id refresh-token

awiki-cli msg send --to <peer> --text "hello"
awiki-cli msg send --group <group_did> --text "hello group"
awiki-cli msg inbox --limit 5
awiki-cli msg history --with <peer> --limit 5
```

如果当前环境没有真实服务，可以准备 dry-run 或 mock/fake service。

#### Step 0.4 确定 feature / env 开关

建议先加环境变量，而不是立刻替换默认路径：

```text
AWIKI_USE_IM_CORE_MVP=1
```

初期 handler 可以这样：

```rust
if use_im_core_mvp() {
    return self.run_msg_send_via_im_core(command);
}

self.run_msg_send_legacy(command)
```

好处：

```text
1. 默认行为不变。
2. 新路径可以渐进试跑。
3. 出问题可以快速回退。
4. 可以对比 legacy 和 SDK façade 输出。
```

### 6.3 产出

```text
- 新分支 / worktree
- baseline 测试记录
- smoke command 清单
- 是否启用 AWIKI_USE_IM_CORE_MVP 的决策
```

### 6.4 验收

```bash
cargo test -p awiki-cli
```

通过，或者已记录已知失败。

---

## 7. Phase 1A：新增 crates/im-core 骨架

### 7.1 目标

新增 `crates/im-core`，让它能独立编译，但不迁移底层业务逻辑。

### 7.2 改动文件

```text
Cargo.toml
crates/im-core/Cargo.toml
crates/im-core/src/lib.rs
crates/im-core/src/error.rs
crates/im-core/src/core.rs
crates/im-core/src/identity.rs
crates/im-core/src/auth.rs
crates/im-core/src/messages.rs
crates/im-core/src/paths.rs
crates/im-core/src/prelude.rs
crates/awiki-cli/Cargo.toml
```

### 7.3 workspace 增加 im-core

根 `Cargo.toml`：

```toml
[workspace]
members = [
    "crates/im-core",
    "crates/awiki-cli",
    "xtask",
]
```

`crates/awiki-cli/Cargo.toml`：

```toml
[dependencies]
im-core = { path = "../im-core" }
```

### 7.4 im-core 初始模块

`crates/im-core/src/lib.rs`：

```rust
pub mod auth;
pub mod core;
pub mod error;
pub mod identity;
pub mod messages;
pub mod paths;
pub mod prelude;

pub use crate::core::{ImClient, ImCore, ImCoreConfig};
pub use crate::error::{ImError, ImResult};
pub use crate::identity::{IdentitySelector, IdentitySummary};
```

### 7.5 定义核心类型

```rust
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

pub struct ImClient {
    core: Arc<ImCoreInner>,
    identity: IdentitySummary,
    runtime: Arc<ClientIdentityRuntime>, // pub(crate)
}

pub struct ImCoreConfig {
    pub service_base_url: String,
    pub did_domain: String,
    pub transport_policy: MessageTransportPolicy,
}

pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: LocalStatePaths,
    pub runtime: RuntimePaths,
}
```

第一阶段这些类型可以是最小字段，不要求一步完整。

### 7.6 定义 P1 message DTO

```rust
pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<MessageId>,
    pub delivery: MessageDeliveryOptions,
}

pub enum MessageTarget {
    Direct(PeerRef),
    Group(GroupRef),
}

pub enum MessageBody {
    Text {
        text: String,
        kind: MessageKind,
    },
    Attachment {
        // P4 reserved
    },
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    SecureDirect, // P6 reserved
    GroupE2ee,    // P6 reserved
}
```

P1 对 `Attachment`、`SecureDirect`、`GroupE2ee` 返回：

```rust
ImError::UnsupportedCapability
```

### 7.7 增加 compile fence

可以新增测试或脚本，检查 `im-core` 不引用 CLI 类型。

例如 `crates/im-core/tests/boundary.rs`：

```rust
#[test]
fn im_core_must_not_reference_cli_types() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "ParsedCommand",
        "ExitError",
        "GlobalOptions",
        "config::Resolved",
        "identity::Manager",
        "awiki_cli",
    ];

    // 可用 walkdir 实现；如果不想加依赖，也可以用 xtask 或 shell grep。
}
```

也可以先用脚本：

```bash
! grep -R "ParsedCommand\|ExitError\|GlobalOptions\|config::Resolved\|identity::Manager\|awiki_cli" crates/im-core/src
```

### 7.8 不要做的事

```text
不要迁移 identity/*
不要迁移 message/*
不要迁移 store/*
不要改 msg send handler
不要改 id register handler
```

### 7.9 验收

```bash
cargo test -p im-core
cargo test -p awiki-cli
```

---

## 8. Phase 1B：CLI adapter + SDK façade

### 8.1 目标

让 CLI 能构造 `ImCore` / `ImClient`，并通过 SDK 形态调用能力。  
adapter 内部可以继续调用旧模块。

### 8.2 新增目录

```text
crates/awiki-cli/src/im_core_adapter/
  mod.rs
  config.rs
  paths.rs
  identity.rs
  auth.rs
  messages.rs
  error.rs
  feature_flag.rs
```

`crates/awiki-cli/src/lib.rs` 增加：

```rust
pub mod im_core_adapter;
```

### 8.3 adapter 职责

adapter 是过渡层：

```text
1. 把 CLI Resolved 转成 ImCoreConfig。
2. 把 CLI identity manager / paths 转成 ImCorePaths。
3. 把 --identity 转成 IdentitySelector。
4. 把 ImError 转成 ExitError。
5. 把 SDK DTO 转成旧模块 request。
6. 把旧模块 result 转成 SDK 领域 result。
```

### 8.4 关键函数

```rust
pub fn build_im_core_config(resolved: &Resolved) -> Result<ImCoreConfig, ExitError>;

pub fn build_im_core_paths(
    resolved: &Resolved,
    manager: &Manager,
) -> Result<ImCorePaths, ExitError>;

pub fn cli_identity_selector(identity_flag: &str) -> IdentitySelector;

pub fn map_im_error(err: ImError, context: &'static str) -> ExitError;

pub fn use_im_core_mvp() -> bool;
```

`cli_identity_selector`：

```rust
pub fn cli_identity_selector(identity_flag: &str) -> IdentitySelector {
    let value = identity_flag.trim();
    if value.is_empty() {
        IdentitySelector::Default
    } else if value.starts_with("did:") {
        IdentitySelector::Did(Did::new(value))
    } else {
        IdentitySelector::LocalAlias(value.to_string())
    }
}
```

### 8.5 façade 形态

过渡期可以在 adapter 里定义：

```rust
pub struct CliImCore {
    resolved: Resolved,
    manager: Manager,
    sdk_core: im_core::ImCore,
}

pub struct CliImClient {
    resolved: Resolved,
    manager: Manager,
    identity_selector: IdentitySelector,
    sdk_client: im_core::ImClient,
}
```

也可以直接让 `App::build_im_core` 返回真实 `im_core::ImCore`，但如果 `ImCore` 还没有真实实现，adapter 包装会更灵活。

### 8.6 测试

新增 adapter 单元测试：

```text
空 identity flag -> IdentitySelector::Default
alice -> IdentitySelector::LocalAlias("alice")
did:xxx -> IdentitySelector::Did(...)
Resolved -> ImCoreConfig
ImError::IdentityRequired -> ExitError
ImError::UnsupportedCapability -> ExitError
```

### 8.7 不要做的事

```text
不要替换所有 handler
不要迁移底层模块
不要删除旧 request 类型
```

### 8.8 验收

```bash
cargo test -p awiki-cli im_core_adapter
cargo test -p im-core
cargo test -p awiki-cli
```

---

## 9. Phase 1C：切 identity/status/refresh-token 低风险命令

### 9.1 目标

先用低风险命令验证 `ImCore` / `ImClient` 能嵌进 CLI handler。

优先命令：

```text
id list
id current
id status
id use
id refresh-token
```

### 9.2 为什么先做这些

```text
1. 比 msg send 风险小。
2. 能验证 IdentitySelector / default identity / local alias。
3. 能验证 auth refresh 路径。
4. 能验证错误映射和输出不变。
```

### 9.3 实施方式

handler 从：

```rust
identity::list_identities(...)
identity::current_identity(...)
identity::refresh_token(...)
```

逐步改成：

```rust
let core = self.build_im_core(&resolved)?;
let result = core.identities().list()?;
```

或者：

```rust
let client = core.client(cli_identity_selector(&self.globals.identity))?;
let result = client.auth().refresh_session()?;
```

但 façade 内部可以继续调用旧模块：

```rust
identity::list_identities(&resolved, &manager)
identity::refresh_token(&resolved, &manager, ...)
```

### 9.4 `id use`

`id use` 建议先走：

```rust
core.identities().plan_default_identity_change(selector)
```

CLI 继续负责：

```text
写 default identity 文件
权限
输出提示
```

不要让 SDK 第一阶段直接写 CLI default 文件，除非路径明确传入并且写入边界已确认。

### 9.5 验收

```bash
cargo test -p awiki-cli
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id list
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id current
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id status
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id refresh-token
```

对比 legacy 输出：

```bash
awiki-cli id status
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id status
```

输出字段可以不完全一样，但业务语义必须一致。

---

## 10. Phase 1D：Handle 注册和 auth 主链路

### 10.1 目标

跑通身份鉴权和 Handle 注册：

```text
core.identities().register_handle()
client.auth().login()
client.auth().ensure_session()
client.auth().refresh_session()
```

### 10.2 具体步骤

#### Step 1：定义 RegisterHandleRequest

```rust
pub struct RegisterHandleRequest {
    pub local_alias: Option<String>,
    pub handle: Handle,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub otp: Option<String>,
    pub invite_code: Option<String>,
    pub display_name: Option<String>,
}
```

P1 可以只实现当前 CLI 已支持的注册路径。

#### Step 2：CLI 继续处理输入

CLI 保留：

```text
OTP 输入
--identity / local alias
输出路径
文件权限
dry-run
pretty/json/table 输出
```

#### Step 3：adapter 内部调用旧注册逻辑

过渡期：

```rust
core.identities().register_handle(request)
```

内部可以转成旧：

```rust
identity::register(...)
```

#### Step 4：auth path 隔离测试

新增 tempdir 测试：

```text
alice auth path != bob auth path
alice refresh 不写 bob auth
default selector 能解析到 default identity
LocalAlias("alice") 能解析到 alice
```

### 10.3 不要做的事

```text
不要把完整 recover_handle 放入 P1。
不要把 replace_did 放入 P1。
不要迁移 profile set/get。
不要迁移 contacts/relation status。
```

### 10.4 验收

```bash
cargo test -p im-core identity
cargo test -p awiki-cli identity
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id register ...
AWIKI_USE_IM_CORE_MVP=1 awiki-cli id refresh-token
```

---

## 11. Phase 1E：私聊文本 façade 跑通

### 11.1 目标

让 `msg send --to ... --text ...` 通过 SDK DTO 跑通，但底层发送实现暂时不动。

### 11.2 handler 目标形态

```rust
pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let core = self.build_im_core(&resolved)?;
    let client = core.client(cli_identity_selector(&self.globals.identity))?;

    let request = SendMessageRequest {
        target: cli_message_target(command)?,
        body: cli_message_text_body(command)?,
        security: MessageSecurityMode::DefaultPlain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    };

    if self.globals.dry_run {
        return self.render_msg_send_plan(&resolved, &request);
    }

    let result = client
        .messages()
        .send(request)
        .map_err(|err| map_im_error(err, "msg send"))?;

    self.render_im_result("awiki-cli msg send", &resolved, result)
}
```

### 11.3 CLI DTO 转换

```rust
fn cli_message_target(command: &ParsedCommand) -> Result<MessageTarget, ExitError> {
    let to = string_flag(command, "to");
    let group = string_flag(command, "group");

    match (to.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => Ok(MessageTarget::Direct(PeerRef::new(to))),
        (true, false) => Ok(MessageTarget::Group(GroupRef::new(group))),
        (true, true) => Err(...),
        (false, false) => Err(...),
    }
}
```

P1 direct send 只处理 `MessageTarget::Direct`。

### 11.4 adapter 内部转换成旧 request

```rust
fn to_legacy_send_request(
    identity_name: String,
    request: SendMessageRequest,
) -> message::SendRequest {
    match request.target {
        MessageTarget::Direct(peer) => message::SendRequest {
            identity_name,
            target: peer.to_string(),
            group: String::new(),
            text: request.body.expect_text(),
            message_type: "text".to_string(),
            secure_mode: String::new(),
            file_path: String::new(),
            mime_type: String::new(),
        },
        MessageTarget::Group(group) => {
            // Phase 1F
        }
    }
}
```

### 11.5 处理 UnsupportedCapability

P1 明确拒绝：

```text
MessageBody::Attachment
MessageSecurityMode::SecureDirect
MessageSecurityMode::GroupE2ee
```

返回：

```rust
ImError::UnsupportedCapability {
    capability: "attachment" | "secure-direct" | "group-e2ee",
}
```

### 11.6 验收

```bash
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg send --to <peer> --text "hello"
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg send --to <peer> --text-file ./hello.txt
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg send --to <peer> --file ./a.png
# 应返回 UnsupportedCapability 或继续走 legacy，取决于开关策略
```

---

## 12. Phase 1F：群聊文本 façade 跑通

### 12.1 目标

让 `msg send --group ... --text ...` 通过同一个 SDK API 跑通。

### 12.2 SDK 调用

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(GroupRef::new(group_did)),
    body: MessageBody::Text {
        text,
        kind: MessageKind::Text,
    },
    security: MessageSecurityMode::DefaultPlain,
    client_message_id: None,
    delivery: MessageDeliveryOptions::default(),
})
```

### 12.3 adapter 转旧 request

```rust
message::SendRequest {
    identity_name,
    target: String::new(),
    group: group_ref.to_string(),
    text,
    message_type: "text".to_string(),
    secure_mode: String::new(),
    file_path: String::new(),
    mime_type: String::new(),
}
```

### 12.4 不做完整群管理

P1 不迁移：

```text
group create
group join
group add
group remove
group update
group members
```

这些命令继续走旧实现或后续 Phase 3 再迁。

### 12.5 验收

```bash
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg send --group <group_did> --text "hello group"
cargo test -p awiki-cli
```

---

## 13. Phase 1G：inbox/history 必要子集

### 13.1 目标

让 SDK 主链路不只是能发，也能读必要 inbox/history。

P1 子集：

```text
client.messages().inbox(query)
client.messages().history(ThreadRef::Direct(peer), query)
client.messages().history(ThreadRef::Group(group), query)
```

### 13.2 不做

P1 不做：

```text
mark_read 完整实现
conversation projection
复杂本地 cache merge
完整 unread count 统计
```

### 13.3 adapter 实现

过渡期调用旧：

```rust
message::inbox(...)
message::history(...)
message::group_messages(...)
```

SDK DTO：

```rust
pub struct InboxQuery {
    pub limit: PageLimit,
    pub unread_only: bool,
}

pub struct HistoryQuery {
    pub limit: PageLimit,
    pub cursor: Option<Cursor>,
}
```

### 13.4 验收

```bash
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg inbox --limit 5
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg history --with <peer> --limit 5
AWIKI_USE_IM_CORE_MVP=1 awiki-cli msg history --group <group_did> --limit 5
```

---

## 14. Phase 1H：App sandbox path fixture

### 14.1 目标

证明 `im-core` 可以脱离 CLI config 和 CLI Manager 进行最小构造。

### 14.2 测试结构

```text
crates/im-core/tests/app_sandbox_paths.rs
```

测试内容：

```rust
#[test]
fn app_can_construct_core_with_explicit_paths() {
    let temp = tempfile::tempdir().unwrap();

    let config = ImCoreConfig {
        service_base_url: "...".parse().unwrap(),
        did_domain: "example.test".to_string(),
        transport_policy: MessageTransportPolicy::HttpOnly,
    };

    let paths = test_paths(temp.path());

    let core = ImCore::new(config, paths).unwrap();

    assert!(core.identities().list().is_ok());
}
```

### 14.3 多身份隔离 fixture

```text
temp/
  identities/
    alice/
      did_document.json
      key-1-private.pem
      auth.json
    bob/
      did_document.json
      key-1-private.pem
      auth.json
  state/
    im.sqlite
```

断言：

```text
Alice client 只读 alice auth path。
Bob client 只读 bob auth path。
Default selector 能解析默认身份。
LocalAlias 能解析指定身份。
```

### 14.4 验收

```bash
cargo test -p im-core app_sandbox_paths
```

---

## 15. P1-beta：开始垂直切片迁入 im-core

P1-alpha 完成后，CLI 已经能通过 SDK façade 调旧模块。  
接下来开始把最小实现切片迁入 `im-core`。

### 15.1 迁移顺序

建议顺序：

```text
1. 纯 DTO / 错误类型 / 基础 helper
2. wire params builder 纯函数
3. DID proof builder 纯函数
4. auth refresh / ensure session
5. direct text send
6. group text send
7. inbox/history
8. Handle register
```

### 15.2 先迁纯函数

适合先搬：

```text
content_type_for_message_type
build_direct_text_payload
build_direct_send_rpc_params
build_group_send_rpc_params
build_inbox_rpc_params
build_history_rpc_params
operation_id / message_id helper
不依赖 Resolved / Manager / ExitError 的 proof helper
```

搬到：

```text
crates/im-core/src/internal/wire/
crates/im-core/src/internal/proof/
```

默认不 public export。

### 15.3 awiki-cli 旧位置保留 re-export

例如：

```rust
// crates/awiki-cli/src/message/wire.rs
pub use im_core::internal_test_helpers::wire::*;
```

更推荐：

```rust
pub(crate) use im_core::internal::wire::*;
```

如果 Rust visibility 不允许跨 crate `pub(crate)`，可以先保留 wrapper：

```rust
pub fn build_direct_send_rpc_params(...) -> Result<Value, MessageError> {
    im_core::wire_compat::build_direct_send_rpc_params(...).map_err(...)
}
```

### 15.4 迁移测试

每迁一个 helper：

```text
1. 复制旧测试到 im-core。
2. 保留旧测试。
3. 两边同时跑。
4. 稳定后再删除旧测试或改旧测试为兼容测试。
```

### 15.5 direct text send 真迁移

当 wire/proof/auth 都准备好后，把 direct send 从 adapter 内部旧调用：

```rust
message::send(...)
```

改成真实 SDK 内部实现：

```text
ImClient runtime
  -> ensure_session
  -> resolve target
  -> build direct.send params
  -> authenticated RPC
  -> map result to SendMessageResult
  -> minimal local store write
```

但仍然不动 attachment/secure。

### 15.6 group text send 真迁移

类似 direct：

```text
ImClient runtime
  -> ensure_session
  -> build group.send params
  -> authenticated RPC
  -> map result to SendMessageResult
  -> minimal local store write
```

P1 不实现 group lifecycle。

---

## 16. Phase 2：identity / directory / profile 补全

### 16.1 进入条件

```text
P1 direct/group text 主链路稳定。
CLI P1 命令可默认走 im-core。
App sandbox fixture 通过。
```

### 16.2 范围

```text
client.identity().profile()
client.identity().update_profile()
client.identity().bind_contact()
core.identities().recover_handle()
client.identity().replace_did()
client.directory().resolve_peer()
client.directory().lookup_handle()
contacts save/list
relation status
profile projection
```

### 16.3 做法

继续采用垂直切片：

```text
1. profile get
2. profile update
3. directory resolve peer
4. handle lookup
5. contact save/list
6. bind contact
7. recover handle
8. replace DID
```

`replace_did` 是危险能力，必须后置，并返回：

```text
risk summary
backup plan
local rebind plan
affected local state
```

### 16.4 CLI 保留

```text
profile markdown-file 读取
危险命令确认
备份路径
文件权限
输出渲染
```

---

## 17. Phase 3：message / group / local_state 补全

### 17.1 进入条件

```text
P1 message MVP 稳定。
Phase 2 directory/profile 稳定。
```

### 17.2 范围

```text
client.messages().mark_read()
client.messages().conversations()
完整本地 message/contact/conversation projection
cache merge
message send state
失败重试
完整 group lifecycle
完整 group members/messages
owner_identity_id 本地隔离
```

### 17.3 做法

建议拆成：

```text
Phase 3A：mark_read
Phase 3B：conversation projection
Phase 3C：local_state owner_identity_id 迁移
Phase 3D：group get/list
Phase 3E：group create/join/leave
Phase 3F：group add/remove/update/members
```

### 17.4 owner_identity_id 迁移

当前如果已有 `owner_did`，不要立刻破坏 schema。做兼容迁移：

```text
1. 新增 owner_identity_id nullable column。
2. 写入新数据时同时写 owner_identity_id + owner_did。
3. 查询优先 owner_identity_id，缺失时 fallback owner_did。
4. 后续 schema version 再考虑强约束。
```

---

## 18. Phase 4：附件

### 18.1 范围

```text
client.attachments().send()
client.attachments().download()
AttachmentInput::LocalFile
AttachmentInput::Bytes
AttachmentDestination::LocalFile
AttachmentDestination::Memory
manifest
slot
commit object
download ticket
digest
temp file
atomic rename
```

### 18.2 CLI 保留

```text
--file
--text-file
--output
overwrite policy
file permission
path validation
```

### 18.3 做法

先 façade 调旧 attachment，再迁移：

```text
1. DTO 和 CLI adapter
2. manifest builder
3. upload slot / commit
4. send attachment message
5. download ticket
6. download write
```

---

## 19. Phase 5：realtime runner

### 19.1 范围

SDK 做：

```text
client.realtime().connect()
client.realtime().run_until_shutdown()
ImEvent stream
WebSocket connect
heartbeat/reconnect
notification classify
message/group event projection
```

CLI 保留：

```text
systemd
launchd
Windows service
daemon socket
pid/log
listener install/start/stop
OpenClaw/Hermes setup
host notify sink
```

### 19.2 做法

```text
1. 抽 raw WebSocket frame 分类纯函数。
2. 抽 notification -> ImEvent 投影。
3. 抽 reconnect/heartbeat decision。
4. 实现 RealtimeRunner。
5. CLI listener service-run 调 SDK runner。
```

---

## 20. Phase 6：secure direct / group E2EE

### 20.1 范围

```text
MessageSecurityMode::SecureDirect
direct session status/repair
secure outbox failed/retry/drop
group E2EE status/repair
group E2EE incoming processing
MLS state
```

### 20.2 原则

```text
普通发送仍走 client.messages().send()
不暴露 ciphertext API
不把 KeyPackage / prekey / MLS provider binary path 暴露给普通调用方
诊断 API 可以 feature-gated
```

### 20.3 做法

```text
1. direct status/repair diagnostic
2. secure send internal flow
3. secure outbox
4. incoming decrypt projection
5. group E2EE status
6. group E2EE repair
7. MLS notice processing
```

---

## 21. Phase 7：provider 抽象

### 21.1 进入条件

```text
SDK public API 稳定。
App 接入确实需要接管存储、网络、密钥或 crypto。
```

### 21.2 可选 provider

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

### 21.3 原则

```text
provider 替换底层实现，不替换业务 API。
ImCore / ImClient / service DTO 保持稳定。
内置 SQLite/HTTP 实现继续保留。
```

---

## 22. 回滚策略

每个阶段都要能快速回滚。

### 22.1 使用环境变量开关

```text
AWIKI_USE_IM_CORE_MVP=1
```

默认先不打开。出问题时关掉即可回到 legacy。

### 22.2 保留 legacy handler

初期可以保留：

```rust
run_msg_send_legacy(command)
run_msg_send_via_im_core(command)
```

稳定后再删除 legacy wrapper。

### 22.3 每个 PR 控制范围

单个 PR 不要同时做：

```text
新增 im-core
改 handler
搬底层模块
改测试
改 schema
```

推荐每个 PR 只做一件主事。

---

## 23. 推荐 PR 拆分

```text
PR 1：新增 crates/im-core skeleton + compile fence
PR 2：新增 awiki-cli::im_core_adapter + adapter tests
PR 3：id list/current/status/use/refresh-token 走 adapter
PR 4：msg send --to 走 adapter
PR 5：msg send --group 走 adapter
PR 6：msg inbox/history P1 子集走 adapter
PR 7：App sandbox path fixture
PR 8：迁移纯 wire/proof helper 到 im-core，并保留 wrapper
PR 9：auth ensure/refresh 真迁移
PR 10：direct text send 真迁移
PR 11：group text send 真迁移
PR 12：inbox/history 真迁移
```

---

## 24. 每个 PR 的验收清单

每个 PR 都必须检查：

```text
[ ] cargo test -p im-core
[ ] cargo test -p awiki-cli
[ ] im-core 不引用 CLI 类型
[ ] CLI 默认行为不变
[ ] 如果改 handler，legacy 路径仍可回退
[ ] 如果新增 DTO，没有 CLI flag 名称泄漏
[ ] 如果迁移 helper，旧测试仍通过或已迁移
[ ] 如果涉及本地状态，alice/bob owner 隔离测试通过
```

---

## 25. 最小成功标准

P1 成功标准：

```text
1. crates/im-core 可独立编译。
2. awiki-cli 依赖 im-core，但 im-core 不依赖 awiki-cli。
3. CLI 可通过 ImCore / ImClient façade 跑通：
   - id status
   - id refresh-token
   - id register
   - msg send --to
   - msg send --group
   - msg inbox/history 必要子集
4. 底层模块测试仍保留并通过。
5. 底层模块没有大规模重写。
6. App sandbox path fixture 能构造 ImCore。
```

---

## 26. 一句话执行原则

**先让 CLI 像调用 SDK 一样调用现有能力；再把这些能力按垂直业务切片逐步迁进真正的 SDK。**

这样可以最大化保留现有底层模块和测试，避免一开始就付出大规模重构成本。
