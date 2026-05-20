# im-core 拆分迁移计划

**状态**：Draft  
**日期**：2026-05-20  
**适用仓库**：`awiki-cli-rs2`

## 1. 迁移原则

- 先做 Phase A 路径参数版，不先引入 provider 边界。
- `cli` 继续拥有配置解析、workspace 解析、identity 文件布局、权限设置和目录创建。
- `im-core` 接收显式路径，承载身份、登录、消息、群组、附件、secure、realtime 等业务流程。
- SQLite、HTTP、WebSocket 等当前底层实现依赖继续保留在 `im-core`，不要求替换。
- App 接入先通过 sandbox/tempdir 路径集合验证，不依赖 CLI。

## 2. 迁移顺序

### Phase 0：建立新 crate、路径 DTO 和边界测试

- 新增 `crates/im-core`。
- 从 `crates/awiki-cli` 移除对失败 sibling `awiki-im-core` 的依赖。
- 增加 compile fence：`im-core` 不能引用 CLI 类型。
- 定义 `ImCoreConfig`、`ImCorePaths`、`IdentityPaths`、`AuthStatePaths`、`LocalStatePaths`、`SecureStatePaths`。
- 增加最小 tempdir/path bundle 测试，证明 core 可以在显式路径上工作。

### Phase 1：移动纯领域 DTO 和错误类型

- `MessageTarget`、`MessageBody`、`InboxQuery`、`GroupRef` 等进入 `im-core`。
- CLI handler 做 adapter。
- 不移动本地 SQLite 实现。

### Phase 2：移动 identity/auth/login 业务流程

- 注册、绑定、恢复、刷新 token、profile、resolve 的流程进入 `im-core`。
- CLI 继续负责解析 identity 目录、私钥路径、DID document 路径和 auth/session 路径。
- `im-core` 使用显式路径读取/写入必要材料，并返回领域结果。

### Phase 3：移动 message/group/attachment 业务流程

- `msg send/inbox/history/mark-read` 下沉。
- group lifecycle 下沉。
- attachment send/download 下沉。
- CLI 保留 `--file`、`--text-file`、`--output` 等路径解析、覆盖策略和权限处理。

### Phase 4：移动 secure 和 realtime 业务编排

- direct E2EE 状态机和 outbox 编排下沉。
- group E2EE 编排下沉。
- WebSocket 分类、notification 投影、reconnect decision 和可嵌入 realtime runner 下沉。
- listener service 管理继续留在 CLI。
- 增加 CLI 后台进程和 App 线程/task 两个启动样例，证明两者使用同一套 `im-core::realtime` 运行循环。

### Phase 5：CLI 瘦身和 App 路径接入样例

- CLI handler 只剩 parse -> core call -> render。
- 增加一个 App/fake app 用例，通过 sandbox/tempdir 路径集合调用 `im-core`，证明 `im-core` 不依赖 CLI。
- 明确 public API 文档和 semver 规则。

### Phase 6：外部能力接口演进

- 在 Phase A API 稳定后，如果 App 接入确实需要，再逐步增加外部 credential/store/blob/crypto/transport 能力。
- 该阶段是可选扩展，不要求替换当前 SQLite、HTTP、WebSocket 等底层实现依赖。
- 该阶段必须保持前面沉淀下来的业务 DTO 和 handler 调用形态稳定，避免再次改动 CLI 命令层。

## 3. 当前命令到目标边界的映射

| 当前 CLI 命令 | 目标 im-core API | CLI 保留内容 |
| --- | --- | --- |
| `id status` | `identity.status()` | 渲染、默认 identity 选择 |
| `id register` | `identity.register_handle()` | 参数解析、OTP 输入、输出路径、文件权限 |
| `id bind` | `identity.bind_contact()` | 参数解析、等待策略展示 |
| `id refresh-token` | `auth.refresh_session()` | 读取当前 identity、保存 token |
| `id resolve` | `identity.resolve()` / `directory.resolve_peer()` | 输出格式 |
| `id recover` | `identity.recover_handle()` | 本地路径、密钥保存、旧状态合并 |
| `id replace-did` | `identity.replace_did()` | 危险命令 UX、备份路径、私钥保存 |
| `id profile get/set` | `identity.get_profile()` / `identity.update_profile()` | 参数解析、markdown-file 读取 |
| `msg send` | `messages.send()` | `--text-file`/`--file` 读取、dry-run 呈现 |
| `msg inbox` | `messages.inbox()` | table/pretty 输出 |
| `msg history` | `messages.history()` | 参数解析 |
| `msg mark-read` | `messages.mark_read()` | 参数解析 |
| `msg attachment download` | `attachments.download()` | 输出路径和文件权限 |
| `msg secure *` | `secure.direct_*()` | outbox id 参数解析、诊断输出 |
| `group create` | `groups.create()` | 参数解析 |
| `group get/list/members/messages` | `groups.get/list/members/messages()` | 输出格式 |
| `group join/add/remove/leave/update` | `groups.*()` | 参数解析、dry-run 呈现 |
| `group e2ee *` | `secure.group_*()` | MLS/session 路径初始化和命令 UX |
| `runtime mode/status` | `realtime.runtime_status()` 可提供领域状态 | config 文件读写仍在 CLI |
| `runtime listener *` | `realtime.run_until_shutdown()` / `realtime.connect()` | service install/start/stop、daemon socket、进程生命周期在 CLI |
| `runtime host-notify *` | `realtime.normalize_host_event()` | OpenClaw/Hermes 配置和投递在 CLI |

## 4. 完成判定

边界设计落地后，应满足：

- `crates/im-core` 可以在不编译 `crates/awiki-cli` 的情况下测试。
- App 可以只依赖 `im-core`，不依赖 CLI。
- CLI 所有 IM 业务命令最终都通过 `im-core`。
- `im-core` 中不存在 `ParsedCommand`、`ExitError`、`GlobalOptions`、CLI config resolver、CLI workspace paths。
- `cli` 中不再重复实现身份、登录、消息、群组、附件、secure、realtime 的业务规则，只做适配和渲染。
- 数据库初始化、私钥路径、文件权限、service manager、OpenClaw/Hermes UX 仍在 CLI。
- Phase A 中，core 流程可用 tempdir/path fixtures 测试，不需要 CLI 类型。
- Phase B 中，外部能力接口可再用 fake 实现覆盖。
