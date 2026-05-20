# im-core / cli 整体架构

**状态**：Draft  
**日期**：2026-05-20  
**适用仓库**：`awiki-cli-rs2`

## 1. 设计结论

`awiki-cli-rs2` 后续应拆成两个 crate：

```text
crates/im-core      # 产品能力层：身份、登录、消息、群组、附件、IM runtime 等
crates/awiki-cli    # 命令行产品壳：命令解析、配置、路径解析、数据库初始化、输出
```

核心原则：

- `im-core` 是 **IM 产品能力层**，不是低层 wire helper 集合。当前 CLI 的大部分业务能力都应收敛到这里，包括身份、登录、消息、群组、附件、secure direct、group E2EE、实时事件和本地状态抽象。
- 第一阶段不采用 provider 方案。`cli` 负责解析配置和本机路径，然后把私钥路径、DID document 路径、auth 路径、SQLite 路径、E2EE/MLS 状态目录等显式传给 `im-core`。这样迁移面更小，也更接近当前 CLI 代码。
- `cli` 是 **命令行适配层**，负责把命令行输入、配置文件、工作区路径、系统服务、OpenClaw/Hermes 等 CLI 特有环境转换成 `im-core` 的配置、路径参数和业务请求。
- App 未来直接依赖 `im-core`。第一阶段 App 也可以传入自己的路径集合；第二阶段如果需要，再增加外部能力接管或适配层；内置 SQLite、HTTP、WebSocket 等底层实现仍保留。
- `im-core` 不能依赖 `cli`。依赖方向只能是 `cli -> im-core`。
- 旧的 sibling `awiki-im-core` 失败版本不作为设计输入，不作为目标 API，不作为迁移基线。目标是在本仓库内新增 `crates/im-core`，再让 `crates/awiki-cli` 依赖它。

阶段化口径：

- **Phase A：路径参数版**。先把业务流程搬进 `im-core`，由 `cli` 传入已经解析好的本地路径。允许 `im-core` 在这些显式路径上读取私钥、DID document、auth/session 文件和本地状态，但禁止 `im-core` 自己做 workspace/config 自动发现。
- **Phase B：可选外部能力版**。业务边界稳定后，再按需增加 `CredentialVault`、`Store`、`Transport`、`CryptoProvider` 等外部能力接口，让 App 可以选择接管存储、密钥和网络实现；这不要求移除 `im-core` 当前内置的 SQLite、HTTP、WebSocket 等底层依赖。

## 2. 分层模型

目标分层如下：

```text
Human / Agent / App
        |
        v
+-------------------+      +----------------------+
| awiki-cli crate   |      | App integration      |
| command shell     |      | app-side paths first |
+-------------------+      +----------------------+
        |                         |
        +-----------+-------------+
                    v
          +------------------+
          | im-core crate    |
          | product APIs     |
          +------------------+
                    |
                    v
       Explicit path bundle
       Phase B provider ports later
                    |
        +-----------+-------------+
        |                         |
 CLI paths/files/sqlite      app sandbox paths
```

`im-core` 不知道调用方是 CLI 还是 App。它只知道：

- 当前 actor / identity 是谁；
- 需要什么网络调用；
- 需要读取或写入哪些身份、会话、消息、群组、附件、E2EE、本地状态；
- 第一阶段需要从哪些显式路径读取 DID document、私钥、auth/session、本地状态；
- 业务流程成功或失败后返回什么领域结果。

`cli` 知道：

- 用户输入了什么命令；
- `--identity`、`--format`、`--dry-run`、`--jq` 等 CLI flag；
- `config.yaml` 在哪里；
- SQLite 文件、identity 目录、私钥文件、runtime socket、日志文件在哪里；
- 该如何初始化目录和数据库；
- 该把哪些路径传给 `im-core`；
- 该如何以 CLI 风格渲染输出和错误。

## 3. crate 依赖规则

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

具体 package 名称可以后续定，但路径和职责以 `crates/im-core` 为准。现有对仓库外失败版本的依赖必须移除，不能继续通过 `../../../awiki-im-core/...` 形成隐式设计约束。

`im-core` 禁止依赖：

- `crates/awiki-cli`
- CLI parser、`ParsedCommand`
- CLI `App`
- CLI `GlobalOptions`
- CLI `ExitError`
- CLI output renderer
- CLI config resolver / workspace resolver
- CLI identity manager 的文件布局实现
- CLI runtime service install/start/stop 实现
- OpenClaw/Hermes CLI setup UX

`im-core` 可以依赖：

- 领域 DTO；
- 路径参数 DTO；
- Phase B provider trait，但第一阶段不把它作为主边界；
- serde 类型；
- 纯协议/纯算法 helper；
- 与 CLI 无关的错误类型；
- 必要的 ANP 协议类型，前提是不会把 CLI 文件布局带入 core；
- 当前底层实现依赖，例如 SQLite、HTTP client、WebSocket client、TLS、serde/json、tokio/async runtime、加密/签名/MLS 相关库。

第一阶段允许 `im-core` 使用标准库文件系统读取调用方传入的明确路径，例如私钥文件、DID document、auth/session 文件、SQLite 数据库文件、E2EE session 目录。限制是：`im-core` 不能自己发现 workspace、不能自己解析 `config.yaml`、不能假定路径来自 CLI，也不能把 CLI 的目录命名作为公共语义。

是否允许 `im-core` 直接依赖系统 keychain、平台 service、CLI config、OpenClaw/Hermes、process manager，应默认回答为否。SQLite、HTTP、WebSocket 这类底层实现依赖按当前设计保留在 `im-core` 中；边界变化只是不让它们携带 CLI workspace/config 语义。所有数据库路径都必须放在 `LocalStatePaths` 之类的显式 DTO 中，所有网络 endpoint 都必须来自 `ImCoreConfig` 或领域请求，而不是从 CLI 配置文件自动解析。

## 4. 总体职责边界

| 能力 | im-core | cli |
| --- | --- | --- |
| 命令解析 | 不负责 | 负责 |
| `schema` / `docs` / completion | 不负责 | 负责 |
| `--dry-run` 呈现 | 只可返回计划对象，不负责文案 | 负责渲染 |
| JSON/pretty/table 输出 | 不负责 | 负责 |
| 配置文件读取/写入 | 不负责 | 负责 |
| 工作区路径解析 | 不负责 | 负责 |
| 数据库文件创建 | 不负责 | 负责 |
| 数据库 schema 初始化/迁移触发 | Phase A 可提供按路径初始化/迁移函数；不负责发现路径 | 负责决定何时初始化和传入路径 |
| 私钥文件布局和权限 | 不负责 | 负责 |
| 密钥生成/签名/加密流程决策 | 负责业务流程；Phase A 可读取显式私钥路径 | 负责生成/选择/保护路径和文件权限 |
| 身份注册/恢复/资料/解析 | 负责业务 API 和流程 | 负责命令输入、配置和路径参数 |
| 登录/JWT/DID auth 流程 | 负责业务 API 和状态转换 | 负责 session 持久化、凭证读取、命令输出 |
| 消息发送/收件箱/历史/已读 | 负责 | 只适配命令 |
| 群组生命周期/成员/群消息 | 负责 | 只适配命令 |
| 附件上传/manifest/下载流程 | 负责；Phase A 可读写显式附件路径 | 负责解析 CLI 输入路径和输出路径 |
| Secure direct / E2EE outbox | 负责业务流程；Phase A 使用显式 session/outbox 路径 | 负责路径选择、目录创建和权限 |
| Group E2EE / MLS 编排 | 负责业务流程；Phase A 使用显式 MLS 状态路径 | 负责路径选择和命令入口 |
| WebSocket 消息分类/通知投影 | 负责 | 负责 listener 进程和本地 daemon |
| systemd/launchd/Windows service | 不负责 | 负责 |
| OpenClaw/Hermes 配置 UX | 不负责 | 负责 |
| Host notification 标准事件 | 负责生成领域事件 | 负责投递到具体 host sink |

## 5. cli 应保留的模块

`cli` 是命令行产品壳，不是业务核心。

### 5.1 命令与输出

保留：

- `cli` parser。
- `cmdmeta`。
- `App` command dispatch。
- help/schema/docs/completion。
- `--format json|pretty|table`。
- `--jq`。
- `--dry-run` 展示。
- `ExitError` 和 exit code 映射。

规则：

- handler 只做输入解析、调用 core、渲染结果。
- 不在 handler 里重新实现业务规则。
- CLI 参数名不泄漏进 core DTO。

### 5.2 配置与工作区

保留：

- `config.yaml` 读取/写入。
- workspace home 解析。
- runtime socket path。
- service base URL / did domain / runtime mode 的 CLI 默认值。
- config upgrade。

CLI 把配置转换成 `ImCoreConfig` 和 `ImCorePaths` / `IdentityPaths` / `LocalStatePaths` 等路径参数。

### 5.3 本机状态初始化

保留：

- 目录创建。
- SQLite 文件打开。
- schema 初始化。
- migration 触发。
- legacy import/recover/rebind 的本机路径处理。
- debug db query。

CLI 负责决定何时调用 `im-core` 暴露的 path-based 初始化/迁移函数，并把 SQLite 文件路径传入 `im-core`。SQLite 连接、schema 和读写实现可以继续在 `im-core` 内部；Phase B 只有在 App 需要接管持久化时，才考虑额外提供 `MessageStore`、`GroupStore`、`ContactStore`、`SecureStore` 等接口。

### 5.4 私钥和凭证管理

保留：

- identity 目录布局。
- DID document 文件路径。
- key1 private key 文件路径。
- E2EE private key 文件路径。
- session/prekey/OPK/MLS 本地文件布局。
- 文件权限。
- 备份和恢复目录。
- JWT / auth.json 持久化。

CLI 在 Phase A 负责构造路径 bundle，例如 DID document path、key1 private path、E2EE private path、auth/session path、prekey dir、MLS state dir。Phase B 才考虑由 CLI 实现 `CredentialVault`、`SignerProvider`、`CryptoProvider`。

### 5.5 Runtime service 与主机集成

保留：

- listener install/start/stop/restart/uninstall。
- foreground/service-run 进程入口。
- local daemon socket。
- systemd/launchd/Windows service。
- OpenClaw/Hermes setup UX。
- host notification sink 配置和 route 注册。

但 WebSocket message 分类、notification 标准化、IM event 投影和 realtime runner 应下沉到 `im-core::realtime`。CLI 的后台程序只是这个 runner 的一个宿主：负责进程化、安装、启动、停止、日志和本机 socket；不重新实现 IM realtime 状态机。

## 6. Phase A 路径参数边界

第一阶段的目标是减少改动面：CLI 继续拥有配置解析、workspace 解析、identity 文件布局、权限设置和目录创建；`im-core` 只接收已经解析好的路径。这样可以先把身份、登录、消息、群组、附件和 secure 业务流程下沉，而不用同时重写本地存储和密钥抽象。

建议先定义一个总路径 DTO，并按模块拆成子 DTO：

```rust
pub struct ImCorePaths {
    pub identity: IdentityPaths,
    pub auth: AuthStatePaths,
    pub local_state: LocalStatePaths,
    pub secure: SecureStatePaths,
    pub runtime: RuntimePaths,
}

pub struct IdentityPaths {
    pub identity_dir: PathBuf,
    pub did_document_path: PathBuf,
    pub key1_private_path: PathBuf,
    pub e2ee_private_path: PathBuf,
    pub metadata_path: Option<PathBuf>,
}

pub struct AuthStatePaths {
    pub auth_path: PathBuf,
    pub session_path: Option<PathBuf>,
    pub token_cache_path: Option<PathBuf>,
}

pub struct LocalStatePaths {
    pub database_file: PathBuf,
    pub migration_dir: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
}

pub struct SecureStatePaths {
    pub direct_session_dir: PathBuf,
    pub signed_prekey_dir: PathBuf,
    pub one_time_prekey_dir: PathBuf,
    pub secure_outbox_dir: PathBuf,
    pub mls_state_dir: PathBuf,
    pub mls_provider_binary: Option<PathBuf>,
}

pub struct RuntimePaths {
    pub runtime_dir: PathBuf,
    pub websocket_state_path: Option<PathBuf>,
    pub notification_queue_path: Option<PathBuf>,
}

pub enum AttachmentSourceRef {
    Path(PathBuf),
}

pub enum AttachmentSinkRef {
    Path(PathBuf),
}
```

路径参数规则：

- DTO 字段表达 core 需要的语义，不表达 CLI 目录布局规则。
- `im-core` 可以读取/写入这些显式路径，但不能自己拼出 workspace 默认路径。
- `im-core` 可以校验“路径缺失、文件不可读、格式错误”，但不负责 chmod、备份、目录创建策略。
- CLI 在调用前创建目录、检查覆盖策略、设置权限，并在需要时把 legacy 路径转换成新路径。
- App 第一阶段也可以通过自己的 sandbox 路径构造这些 DTO。

初始化示例：

```rust
let paths = ImCorePaths {
    identity: IdentityPaths {
        identity_dir,
        did_document_path,
        key1_private_path,
        e2ee_private_path,
        metadata_path,
    },
    auth: AuthStatePaths {
        auth_path,
        session_path,
        token_cache_path,
    },
    local_state: LocalStatePaths {
        database_file,
        migration_dir,
        temp_dir,
    },
    secure: SecureStatePaths {
        direct_session_dir,
        signed_prekey_dir,
        one_time_prekey_dir,
        secure_outbox_dir,
        mls_state_dir,
        mls_provider_binary,
    },
    runtime: RuntimePaths {
        runtime_dir,
        websocket_state_path,
        notification_queue_path,
    },
};

let core = ImCore::new(config, paths)?;
```

## 7. Phase B 可选外部能力方向

业务边界稳定后，可以在路径 DTO 和内置底层实现之外，增加可选的 provider/trait 接管能力。这个阶段的方向是：

- credential 能力：加载/保存 DID document、JWT、签名私钥、E2EE 私钥。
- identity repository：列出、保存、切换 identity record。
- store 能力：message/group/contact/secure store。
- blob 能力：附件 source/sink 支持内存、app storage、云端缓存等。
- crypto 能力：签名、direct E2EE、group E2EE/MLS。
- transport 能力：user-service、did-auth、message service、attachment service、WebSocket。

Phase B 的目标不是改变业务 API，也不是删除当前 SQLite、HTTP、WebSocket 实现，而是在需要时把 `ImCorePaths` 包裹成外部能力接口，让 App 可以选择完全接管存储、密钥和网络实现。

## 8. API 风格和 DTO 规则

CLI handler 目标形态：

```rust
pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let cli_context = self.resolve_cli_context()?;
    let core = self.build_im_core(&cli_context)?;

    let request = SendMessageRequest {
        target: parse_message_target(command)?,
        body: parse_message_body(command)?,
        security: parse_security_mode(command)?,
        client_message_id: None,
    };

    let result = core.messages().send(cli_context.actor(), request)
        .map_err(map_im_error)?;

    self.render_im_result("awiki-cli msg send", result)
}
```

App 目标形态：

```rust
let core = ImCore::new(config, app_paths)?;
let actor = core.identity().current_identity()?;
let page = core.messages().inbox(&actor, InboxQuery::default())?;
```

关键差异：

- CLI 和 App 都调用同一个 `im-core`。
- CLI 的 `ParsedCommand` 不进入 `im-core`。
- App 不需要知道 CLI config 和 runtime service 文件；第一阶段只需要提供自己的 sandbox 路径集合。

`im-core` DTO 必须使用领域语言，例如 `ActorContext`、`PeerRef`、`GroupRef`、`ThreadRef`、`MessageTarget`、`MessageBody`、`MessageSecurityMode`、`InboxQuery`、`HistoryQuery`、`GroupPolicyPatch`、`AttachmentSourceRef`、`AttachmentSinkRef`、`SessionBundle`。

禁止使用 CLI 语言：

- `to` / `with` 作为核心字段名；
- `text-file`；
- `mime-type`；
- `dry_run`；
- `identity_name` 作为唯一 actor 表达；
- `output_path`；
- `ParsedCommand`；
- `ExitError`。

例外：为了兼容现有 CLI，可在 `cli` crate 内有 adapter DTO，但不得导出为 core API。

## 9. 错误边界

`im-core` 错误应表达领域失败：

- `IdentityRequired`
- `AuthRequired`
- `PermissionDenied`
- `PeerNotFound`
- `GroupNotFound`
- `MessageNotFound`
- `AttachmentNotFound`
- `TransportUnavailable`
- `UnsupportedCapability`
- `SecureSessionMissing`
- `SecureOutboxFailed`
- `PathUnavailable`
- `CredentialFileUnreadable`
- `LocalStateUnavailable`
- `InvalidInput`

`cli` 负责映射为：

- exit code；
- `error.code`；
- human hint；
- pretty/table/json 输出。

`im-core` 不应该写出“Use `awiki-cli ...`”这类 hint。它可以返回机器可读 repair action，例如：

```rust
RepairHint::RefreshSession
RepairHint::StartRealtimeListener
RepairHint::PublishPrekey
RepairHint::RetryOutbox { outbox_id }
```

CLI 再把它渲染成命令行提示。

## 10. 最重要的边界口径

**im-core 负责“AWiki IM 能做什么以及业务流程怎么走”；cli 负责“这个能力在命令行、本机文件、本机数据库、本机服务和本机密钥环境里怎么运行”。**

身份、登录、消息、群组、附件、secure、realtime 都是 `im-core` 能力。命令、配置、路径解析、数据库初始化触发、私钥文件布局和权限、系统服务和输出都是 `cli` 能力。
