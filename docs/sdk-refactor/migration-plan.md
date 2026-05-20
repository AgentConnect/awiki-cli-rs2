# im-core 拆分迁移计划

**状态**：Draft
**日期**：2026-05-21
**适用仓库**：`awiki-cli-rs2`

## 1. 迁移原则

- 先做 Phase A 路径参数版，不先引入 provider 边界。
- `cli` 继续拥有配置解析、workspace 解析、identity 文件布局、权限设置和目录创建。
- `im-core` 接收显式路径，长期承载身份、登录、消息、群组、附件、secure、realtime 等业务流程；第一阶段只落地 core 框架、身份鉴权、Handle 注册、私聊/群聊消息。
- SQLite、HTTP、WebSocket 等当前底层实现依赖继续保留在 `im-core`，不要求替换。
- App 接入先通过 sandbox/tempdir 路径集合验证，不依赖 CLI。
- 当前仓库基线中 `crates/awiki-cli` 已经没有外部 `awiki-im-core` 依赖；迁移来源是 `awiki-cli` 内部模块，不是 sibling crate。

## 2. 迁移顺序

### Phase 1：core 框架 + 最小 IM 能力

- 新增 `crates/im-core`。
- 把 `crates/im-core` 加入 workspace，并让 `crates/awiki-cli` 后续通过 path dependency 依赖它。
- 确认 `crates/awiki-cli` 不重新引入 `../../../awiki-im-core/...` 这类仓库外失败版本依赖。
- 增加 compile fence：`im-core` 不能引用 CLI 类型。
- 定义 `ImCoreConfig`、`ImCorePaths`、`IdentityRegistryPaths`、`IdentitySummary` 和内部 `IdentityPaths` / `AuthStatePaths` / `LocalStatePaths` / `SecureStatePaths`。
- 建立高层入口形态：`core.client(selector)` 绑定身份，`client.auth()` / `client.messages()` 承载第一阶段业务调用，`core.identities()` 承载 Handle 注册，`core.bootstrap()` 承载初始化/迁移生命周期调用。
- 增加最小 tempdir/path bundle 测试，证明 core 可以在显式路径上工作。
- 搭起 `core` 基础类型：`ImCore`、`ImClient`、`IdentityRegistry`、`CoreBootstrap`、`OperationContext`、`ImError`、`ImResult<T>`、分页/cursor/id 基础 DTO。
- 实现身份鉴权的最小闭环：加载指定身份、读取 DID document 和签名私钥、DID auth/login、refresh/ensure session、按身份隔离 auth/session 路径。
- 实现 Handle 注册：`core.identities().register_handle()`，CLI 负责 OTP 输入、输出路径、文件权限和渲染，`im-core` 负责业务请求和领域结果。
- 实现消息私聊和群聊的最小闭环：`client.messages().send(SendMessageRequest { target: MessageTarget::Direct | MessageTarget::Group, ... })`，以及第一阶段必要的 `inbox/history` 读取能力。
- CLI 第一阶段只改造覆盖这些命令所需的 handler，让它们走 `im-core` 高层 API；其他命令继续留在 CLI 旧实现，等待后续迁移。

第一阶段不包含：

- 手机号/邮箱绑定、账号恢复、replace DID、profile 完整编辑。
- 完整联系人/directory 能力。
- 完整 group lifecycle 和成员管理；群聊消息只要求能面向已有 `GroupRef` 发送/读取。
- 附件上传/下载。
- secure direct、group E2EE、secure outbox。
- realtime listener、WebSocket runner、host notification。
- provider/trait 外部能力抽象。

### Phase 2：补全 identity / directory 业务

- 绑定手机号/邮箱、恢复 handle、默认身份切换、profile get/set、replace DID、resolve identity 等进入 `im-core`。
- `directory` 的 handle lookup、联系人保存/查询、关系状态和 profile 投影逐步下沉。
- CLI 继续负责 identity 目录、私钥路径、DID document 路径和 auth/session 路径解析，以及危险命令 UX。
- `im-core` 使用显式路径读取/写入必要材料，并返回领域结果；公开接口仍保持 `IdentitySummary` / `ImClient` 高层形态。

### Phase 3：补全 message / group 业务

- 扩展第一阶段消息能力：mark-read、本地投影、缓存合并、消息状态、失败重试、更多查询形态。
- group create/get/list/join/leave/add/remove/update/members/messages 等完整生命周期下沉。
- 群消息读取可以继续通过 `client.groups().messages()`，发送仍优先通过 `client.messages().send(MessageTarget::Group)`。
- CLI 保留参数解析、dry-run 呈现和输出格式。

### Phase 4：移动 attachment 业务流程

- attachment send/download、manifest、slot、download ticket、digest、临时文件和原子写入流程下沉。
- CLI 保留 `--file`、`--text-file`、`--output` 等路径解析、覆盖策略和权限处理。

### Phase 5：移动 secure 和 realtime 业务编排

- direct E2EE 状态机和 outbox 编排下沉。
- group E2EE 编排下沉。
- WebSocket 分类、notification 投影、reconnect decision 和可嵌入 realtime runner 下沉。
- listener service 管理继续留在 CLI。
- 增加 CLI 后台进程和 App 线程/task 两个启动样例，证明两者使用同一套 `im-core::realtime` 运行循环。

### Phase 6：CLI 瘦身和 App 路径接入样例

- CLI handler 只剩 parse -> core call -> render。
- 增加一个 App/fake app 用例，通过 sandbox/tempdir 路径集合调用 `im-core`，证明 `im-core` 不依赖 CLI。
- 明确 public API 文档和 semver 规则。

### Phase 7：外部能力接口演进

- 在 Phase A API 稳定后，如果 App 接入确实需要，再逐步增加外部 credential/store/blob/crypto/transport 能力。
- 该阶段是可选扩展，不要求替换当前 SQLite、HTTP、WebSocket 等底层实现依赖。
- 该阶段必须保持前面沉淀下来的业务 DTO 和 handler 调用形态稳定，避免再次改动 CLI 命令层。

## 3. 当前代码迁移来源

当前 `crates/awiki-cli` 已经包含 IM 相关业务模块。迁移应从这些模块拆分能力，而不是参考或依赖仓库外 `awiki-im-core`。

| 现有代码位置 | 目标模块 | 说明 |
| --- | --- | --- |
| `src/identity/*` | `im-core::identity` | identity layout 细节留在 CLI adapter；注册、恢复、DID/profile 业务下沉 |
| `src/authsdk/*` | `im-core::auth` | DID auth、session/JWT wire helper 和刷新流程下沉 |
| `src/message/service.rs`、`src/message/wire.rs`、`src/message/client.rs` | `im-core::messages` | direct send、inbox、history、mark-read、message DTO 下沉 |
| `src/message/group_*` | `im-core::groups`、`im-core::secure` | 群生命周期和 group E2EE 分别归 groups/secure |
| `src/message/attachment*` | `im-core::attachments` | 附件 manifest、slot、download ticket、send/download 下沉 |
| `src/message/secure_*` | `im-core::secure` | direct E2EE、secure outbox、incoming processing 下沉 |
| `src/message/service_discovery.rs` | `im-core::discovery` | DID document service selection 和 capability 选择下沉 |
| `src/runtime/listener_*` | `im-core::realtime` + CLI runtime shell | WebSocket 分类、notification 投影、runner 下沉；service install/daemon/socket 留在 CLI |
| `src/store/*` | `im-core::local_state` | SQLite schema、message/group/contact/outbox state 下沉；workspace 路径解析留在 CLI |
| `src/app/*_handlers.rs` | CLI adapter | handler 保留 parse -> core call -> render |

## 4. 当前命令到目标边界的映射

所有业务命令都先把 `--identity` 转换为 `IdentitySelector`，通过 `core.client(selector)` 获得绑定身份的 `ImClient`，再调用对应服务。注册、恢复、默认身份选择属于 `core.identities()`；数据库初始化/迁移属于 `core.bootstrap()`。CLI 不应直接调用 actor/path/RPC/store helper。

第一阶段只要求落地标记为 `P1` 的命令映射。后续阶段映射是目标边界，用于避免继续扩大第一阶段范围。

| 阶段 | 当前 CLI 命令 | 目标 im-core API | CLI 保留内容 |
| --- | --- | --- | --- |
| P1 | `id status` | `core.identities().default_identity()` / `core.identities().list()` / `client.current_identity()` | 渲染、默认 identity 选择 |
| P1 | `id register` | `core.identities().register_handle()` | 参数解析、OTP 输入、输出路径、文件权限 |
| P1 | `id refresh-token` / login 所需路径 | `client.auth().refresh_session()` / `client.auth().ensure_session()` | 读取当前 identity、保存 token、输出 |
| P1 | `msg send` direct/group | `client.messages().send()` | `--text-file` 读取、dry-run 呈现、输出 |
| P1 | `msg inbox` | `client.messages().inbox()` | table/pretty 输出 |
| P1 | `msg history` direct/group | `client.messages().history()` | 参数解析 |
| P2 | `id bind` | `client.identity().bind_contact()` | 参数解析、等待策略展示 |
| P2 | `id resolve` | `core.identities().resolve()` / `client.directory().resolve_peer()` | 输出格式 |
| P2 | `id recover` | `core.identities().recover_handle()` | 本地路径、密钥保存、旧状态合并 |
| P2 | `id replace-did` | `client.identity().replace_did()` | 危险命令 UX、备份路径、私钥保存 |
| P2 | `id profile get/set` | `client.identity().profile()` / `client.identity().update_profile()` | 参数解析、markdown-file 读取 |
| P3 | `msg mark-read` | `client.messages().mark_read()` | 参数解析 |
| P3 | `group create` | `client.groups().create()` | 参数解析 |
| P3 | `group get/list/members/messages` | `client.groups().get/list/members/messages()` | 输出格式 |
| P3 | `group join/add/remove/leave/update` | `client.groups().*()` | 参数解析、dry-run 呈现 |
| P4 | `msg attachment download` | `client.attachments().download()` | 输出路径和文件权限 |
| P5 | `msg secure *` | `client.secure().direct_*()` | outbox id 参数解析、诊断输出 |
| P5 | `group e2ee *` | `client.secure().group_*()` | MLS/session 路径初始化和命令 UX |
| P5 | `runtime mode/status` | `client.realtime().status()` 可提供领域状态 | config 文件读写仍在 CLI |
| P5 | `runtime listener *` | `client.realtime().run_until_shutdown()` / `client.realtime().connect()` | service install/start/stop、daemon socket、进程生命周期在 CLI |
| P5 | `runtime host-notify *` | `client.realtime().normalize_host_event()` | OpenClaw/Hermes 配置和投递在 CLI |

## 5. 完成判定

第一阶段完成后，应满足：

- `crates/im-core` 存在并可独立编译/测试，不依赖 `crates/awiki-cli`。
- `core` 框架入口稳定：`ImCore`、`ImClient`、`IdentityRegistry`、`CoreBootstrap`、`ImError`、`ImResult<T>`。
- 身份鉴权、Handle 注册、消息私聊和群聊的 P1 命令已经通过 `im-core` 高层 API 执行。
- P1 CLI handler 不再直接调用 actor/path/RPC/store helper。
- P1 能用 tempdir/path fixtures 验证，不需要 CLI 类型。
- 附件、secure、realtime、完整 group lifecycle、完整 identity/directory 可继续留在 CLI 旧实现，等待后续阶段。

全部边界设计落地后，应满足：

- `crates/im-core` 可以在不编译 `crates/awiki-cli` 的情况下测试。
- App 可以只依赖 `im-core`，不依赖 CLI。
- CLI 所有 IM 业务命令最终都通过 `im-core`。
- `im-core` 中不存在 `ParsedCommand`、`ExitError`、`GlobalOptions`、CLI config resolver、CLI workspace paths。
- `cli` 中不再重复实现身份、登录、消息、群组、附件、secure、realtime 的业务规则，只做适配和渲染。
- 数据库初始化、私钥路径、文件权限、service manager、OpenClaw/Hermes UX 仍在 CLI。
- Phase A 中，core 流程可用 tempdir/path fixtures 测试，不需要 CLI 类型。
- Phase B 中，外部能力接口可再用 fake 实现覆盖。
