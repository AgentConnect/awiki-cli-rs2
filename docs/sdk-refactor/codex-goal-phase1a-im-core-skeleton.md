# Codex Goal：Phase 1A - im-core skeleton

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**目标阶段**：`docs/sdk-refactor/implementation-playbook.md` 中的 Phase 1A  
**目标**：新增 `crates/im-core` 骨架，并接入 workspace；不迁移底层模块，不改 CLI handler。  
**建议提交信息**：`feat: add im-core skeleton`

---

## 1. 这个 goal 为什么要这么小

第一个 Codex goal 不应该尝试迁移 `identity/*`、`message/*`、`store/*` 或 CLI handler。第一步只做 SDK 骨架，原因是：

```text
1. 风险最小，可以验证 workspace 和 crate 边界。
2. 不破坏现有底层模块和已有测试。
3. 后续 CLI adapter、handler 改造、业务迁移都可以基于这个 crate 继续做。
4. 可以马上加 compile fence，防止 im-core 反向依赖 CLI 类型。
```

成功标准不是“SDK 能完整发消息”，而是：

```text
crates/im-core 可独立编译
awiki-cli 可以依赖 im-core
im-core 暴露 Phase 1 所需 public API / DTO / error
im-core 不引用 awiki-cli 或 CLI 类型
现有 awiki-cli 测试不被破坏
```

---

## 2. 复制给 Codex Goal 的 Prompt

下面这段可以直接复制给 Codex Goal。

```text
你在 AgentConnect/awiki-cli-rs2 仓库中工作。请执行 docs/sdk-refactor 的 Phase 1A：新增 crates/im-core SDK 骨架，并接入 workspace。这个 goal 只做骨架和边界测试，不迁移底层模块，不改 CLI handler，不改现有命令行为。

请先阅读这些文档，严格遵守里面的边界：
- docs/sdk-refactor/implementation-playbook.md
- docs/sdk-refactor/migration-plan.md
- docs/sdk-refactor/public-api.md
- docs/sdk-refactor/cli-boundary.md
- docs/sdk-refactor/modules/01-core.md
- docs/sdk-refactor/modules/07-messages.md

目标：
1. 新增 crates/im-core。
2. 将 crates/im-core 加入 workspace members。
3. 让 crates/awiki-cli 通过 path dependency 依赖 im-core。
4. 在 im-core 中定义 Phase 1A 所需的最小 public API / DTO / error 类型：
   - ImCore
   - ImClient
   - ImCoreConfig
   - ImCorePaths
   - IdentitySelector
   - IdentitySummary
   - ImError
   - ImResult<T>
   - IdentityRegistry
   - CoreBootstrap
   - AuthService
   - MessageService
   - SendMessageRequest
   - MessageTarget
   - MessageBody
   - MessageKind
   - MessageSecurityMode
   - MessageDeliveryOptions
   - InboxQuery
   - HistoryQuery
   - ThreadRef
   - SendMessageResult
   - Page<T> / Cursor / PageLimit / basic ID newtypes
5. 增加 compile fence / boundary test，禁止 im-core 引用：
   - ParsedCommand
   - ExitError
   - GlobalOptions
   - config::Resolved
   - identity::Manager
   - awiki_cli
   - crate::app
   - crate::cli
   - crate::config
6. 增加最小单元测试，验证：
   - IdentitySelector::LocalAlias 可构造。
   - MessageTarget::Direct / Group 可构造。
   - MessageSecurityMode::SecureDirect 和 GroupE2ee 在 Phase 1A 只能作为 reserved enum variant，不实现业务。
   - ImCore::new(config, paths) 可构造。
7. 确保 cargo test -p im-core 通过。
8. 确保 cargo test -p awiki-cli 通过，或至少 cargo check -p awiki-cli 通过；如果因为仓库已有已知测试失败，请说明失败原因和是否与本次改动相关。

强约束：
- 不要迁移 identity/*。
- 不要迁移 authsdk/*。
- 不要迁移 message/*。
- 不要迁移 store/*。
- 不要迁移 runtime/*。
- 不要修改 app/*_handlers.rs。
- 不要修改 CLI 命令行为。
- 不要引入 async runtime。
- 不要新增 provider traits。
- 不要实现 attachment / realtime / secure / group E2EE。
- 不要让 im-core 依赖 awiki-cli。
- 不要把 ActorContext、LocalStatePaths、AuthStatePaths、SecureStatePaths 暴露到业务 API 参数里。

实现建议：
- Phase 1A 可以使用 blocking-first API。
- 为减少依赖，ImCoreConfig 中的 endpoint 可以先用 String 或轻量 newtype；除非确有必要，不要引入新的外部依赖。
- 所有 DTO 尽量 derive Debug, Clone, PartialEq, Eq；需要 Default 的地方再加 Default。
- ImCore::client、IdentityRegistry、AuthService、MessageService 的业务方法在 Phase 1A 可以返回 ImError::UnsupportedCapability 或 IdentityRequired 等占位错误，但 public API 形态要稳定。
- lib.rs 应导出主要 public API，并提供 prelude.rs。
- internal runtime 类型应保持 pub(crate)，不要 public export。

并行策略：
请把工作拆成可并行的 track 来推进，加快速度：
Track A：workspace / Cargo.toml / crate skeleton。
Track B：core/error/path/identity/auth/messages DTO 与 public API。
Track C：boundary tests / compile fence / basic unit tests。
Track D：cargo check/test validation 与问题修复。
你可以先并行检查相关文档和 Cargo 配置，然后一次性写入多个独立文件。不要串行地每创建一个小文件就停下来等待。只有在测试失败时再聚焦修复。

交付内容：
- 代码改动 summary。
- 新增/修改文件清单。
- 运行过的命令与结果。
- 如果有测试失败，说明失败是否与本次改动相关。
```

---

## 3. 建议 Codex 实际改动文件

### 3.1 根 workspace

修改：

```text
Cargo.toml
```

把 `crates/im-core` 加入 members：

```toml
[workspace]
members = [
    "crates/im-core",
    "crates/awiki-cli",
    "xtask",
]
```

### 3.2 awiki-cli 依赖 im-core

修改：

```text
crates/awiki-cli/Cargo.toml
```

增加：

```toml
im-core = { path = "../im-core" }
```

### 3.3 新增 im-core crate

新增：

```text
crates/im-core/Cargo.toml
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
crates/im-core/src/error.rs
crates/im-core/src/core.rs
crates/im-core/src/paths.rs
crates/im-core/src/identity.rs
crates/im-core/src/auth.rs
crates/im-core/src/messages.rs
crates/im-core/tests/boundary.rs
```

---

## 4. Phase 1A 最小代码形态建议

### 4.1 `lib.rs`

```rust
pub mod auth;
pub mod core;
pub mod error;
pub mod identity;
pub mod messages;
pub mod paths;
pub mod prelude;

pub use auth::{AuthScope, AuthService, AuthStatus, SessionBundle, SessionUpdate};
pub use core::{CoreBootstrap, ImClient, ImCore, ImCoreConfig, MessageTransportPolicy};
pub use error::{ImError, ImResult};
pub use identity::{
    DefaultIdentityChange, IdentityId, IdentityReadiness, IdentityRegistration,
    IdentityRegistry, IdentitySelector, IdentitySummary, RegisterHandleRequest,
};
pub use messages::{
    Cursor, GroupRef, HistoryQuery, InboxQuery, MessageBody, MessageDeliveryOptions,
    MessageId, MessageKind, MessageSecurityMode, MessageTarget, Page, PageLimit, PeerRef,
    SendMessageRequest, SendMessageResult, ThreadId, ThreadRef,
};
pub use paths::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};
```

### 4.2 `error.rs`

```rust
pub type ImResult<T> = Result<T, ImError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImError {
    InvalidInput { field: Option<String>, message: String },
    IdentityRequired,
    IdentityNotFound { selector: String },
    DefaultIdentityMissing,
    AuthRequired,
    SessionExpired,
    PermissionDenied,
    PeerNotFound,
    GroupNotFound,
    MessageNotFound,
    ContactNotFound,
    TransportUnavailable { detail: String },
    UnsupportedCapability { capability: String },
    LocalStateUnavailable { detail: String },
    PathUnavailable { path_kind: String, detail: String },
    CredentialFileUnreadable { path_kind: String, detail: String },
    Service { status_code: Option<u16>, code: Option<String>, message: String },
    Internal { message: String },
}
```

### 4.3 `core.rs`

```rust
use std::sync::Arc;

use crate::auth::AuthService;
use crate::error::ImResult;
use crate::identity::{IdentityRegistry, IdentitySelector, IdentitySummary};
use crate::messages::MessageService;
use crate::paths::ImCorePaths;

#[derive(Debug, Clone)]
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

#[derive(Debug)]
pub(crate) struct ImCoreInner {
    pub config: ImCoreConfig,
    pub paths: ImCorePaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImCoreConfig {
    pub service_base_url: String,
    pub did_domain: String,
    pub user_service_endpoint: Option<String>,
    pub message_service_endpoint: Option<String>,
    pub transport_policy: MessageTransportPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred,
}

#[derive(Debug, Clone)]
pub struct ImClient {
    pub(crate) core: Arc<ImCoreInner>,
    pub(crate) identity: IdentitySummary,
}

pub struct CoreBootstrap<'a> {
    pub(crate) core: &'a ImCore,
}

impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self> {
        Ok(Self {
            inner: Arc::new(ImCoreInner { config, paths }),
        })
    }

    pub fn identities(&self) -> IdentityRegistry<'_> {
        IdentityRegistry { core: self }
    }

    pub fn bootstrap(&self) -> CoreBootstrap<'_> {
        CoreBootstrap { core: self }
    }

    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient> {
        let identity = self.identities().resolve(selector)?;
        Ok(ImClient {
            core: Arc::clone(&self.inner),
            identity,
        })
    }
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary {
        &self.identity
    }

    pub fn auth(&self) -> AuthService<'_> {
        AuthService { client: self }
    }

    pub fn messages(&self) -> MessageService<'_> {
        MessageService { client: self }
    }
}
```

Phase 1A 可以让 `IdentityRegistry::resolve()` 对空 registry 返回 `IdentityNotFound`，后续 Phase 1B/1C 再接入真实路径加载。

---

## 5. Boundary test 建议

`crates/im-core/tests/boundary.rs`：

```rust
use std::fs;
use std::path::Path;

#[test]
fn im_core_does_not_reference_cli_types() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let forbidden = [
        "ParsedCommand",
        "ExitError",
        "GlobalOptions",
        "config::Resolved",
        "identity::Manager",
        "awiki_cli",
        "crate::app",
        "crate::cli",
        "crate::config",
    ];

    let mut offenders = Vec::new();
    visit_rs_files(&src_dir, &mut |path, content| {
        for token in forbidden {
            if content.contains(token) {
                offenders.push(format!("{} contains {}", path.display(), token));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "im-core must not reference CLI-only types:\n{}",
        offenders.join("\n")
    );
}

fn visit_rs_files(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    let entries = fs::read_dir(dir).expect("read dir");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            visit_rs_files(&path, f);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let content = fs::read_to_string(&path).expect("read rust file");
            f(&path, &content);
        }
    }
}
```

这个测试不需要引入 `walkdir` 依赖。

---

## 6. 并行策略怎么告诉 Codex

建议在 goal prompt 中明确使用“track”语言：

```text
请把任务拆成 4 个并行 track：
Track A：workspace/Cargo/crate skeleton。
Track B：public API DTO/error/path/core 模块。
Track C：boundary tests 和最小 unit tests。
Track D：cargo check/test validation。

你可以并行阅读文档、Cargo 配置和现有 crate 结构；然后一次性创建多个独立文件。不要在每个小文件后停下来等待。遇到编译错误后，按错误所属 track 并行定位：Cargo 配置问题、类型定义问题、test/fence 问题分别处理。
```

Codex 不一定真的开多个 worker，但这个提示会让它按并行轨道组织工作，减少“先改一点、停一下、再看一点”的串行行为。

---

## 7. 不应该让第一个 goal 做什么

第一个 goal 绝对不要写成：

```text
实现 Phase 1
迁移身份和消息
让私聊群聊跑通
把底层模块迁到 im-core
重构 CLI
实现 SDK
```

这些都太大，Codex 很容易：

```text
1. 大范围修改底层模块。
2. 改坏现有测试。
3. 把 CLI 类型带进 im-core。
4. 把 Phase 2/3/4 能力也顺手做进去。
5. 生成一堆低层 API，偏离 SDK 边界。
```

第一个 goal 应该只做：

```text
新增 crates/im-core skeleton
workspace 接入
public API / DTO / error 类型
compile fence
最小测试
```

---

## 8. 第一个 goal 的验收命令

让 Codex 最后必须执行或至少尝试执行：

```bash
cargo test -p im-core
cargo check -p awiki-cli
cargo test -p awiki-cli
```

如果 `cargo test -p awiki-cli` 太慢或仓库已有历史失败，则至少：

```bash
cargo check -p awiki-cli
```

并要求它说明：

```text
哪些命令成功
哪些命令失败
失败是否和本次改动有关
是否存在已知历史失败
```

---

## 9. 第一个 goal 的成功标准

这个 goal 成功后应该满足：

```text
1. crates/im-core 存在。
2. Cargo workspace 包含 crates/im-core。
3. awiki-cli 能依赖 im-core。
4. im-core 可独立编译和测试。
5. im-core 中有 P1 所需 public API 类型。
6. im-core 不依赖 awiki-cli。
7. im-core 不引用 CLI-only 类型。
8. 没有修改底层 identity/message/store/runtime 模块。
9. 没有修改 CLI handler 行为。
10. 后续可以开始 Phase 1B：CLI adapter + SDK façade。
```

---

## 10. 给 Codex 的短版提示词

如果你想给 Codex 一个更短的 prompt，可以用下面这个：

```text
执行 docs/sdk-refactor 的 Phase 1A。只新增 crates/im-core skeleton 并接入 workspace，不迁移底层模块，不改 CLI handler，不改命令行为。

请阅读 docs/sdk-refactor/implementation-playbook.md、migration-plan.md、public-api.md、cli-boundary.md。新增 im-core crate，定义 ImCore/ImClient/ImCoreConfig/ImCorePaths/IdentitySelector/IdentitySummary/ImError/ImResult 以及 P1 message DTO。awiki-cli 增加 im-core path dependency。增加 boundary test，确保 im-core 不引用 ParsedCommand、ExitError、GlobalOptions、config::Resolved、identity::Manager、awiki_cli、crate::app、crate::cli、crate::config。

不要迁移 identity/*、authsdk/*、message/*、store/*、runtime/*。不要改 app handlers。不要引入 async runtime、provider traits、attachment/realtime/secure/group-e2ee 实现。

请采用并行策略推进：Track A workspace/Cargo，Track B public API DTO，Track C tests/fence，Track D cargo check/test。一次性处理独立文件，最后运行 cargo test -p im-core、cargo check -p awiki-cli、cargo test -p awiki-cli，并汇报结果。
```
