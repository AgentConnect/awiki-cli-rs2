# CLI 切到 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：Phase 5 realtime runner 完成后，CLI 默认实现切到 `crates/im-core`  
**目标**：让 `awiki-cli` 的 IM 主链路默认通过 `im-core` public API 执行；不再把旧 `awiki-cli` 业务模块作为默认 fallback；同时收窄 CLI 命令面，使 CLI 保持高层产品接口，而不是暴露 wire、DB、E2EE、runtime 细节。

---

## 0. 前提和阅读约束

执行本计划前，必须先阅读并遵守：

```text
docs/sdk-refactor/README.md
docs/sdk-refactor/architecture.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/cli-boundary.md
docs/sdk-refactor/im-core-cli-boundary.md
docs/sdk-refactor/Interface/05-cli-adapter-interface.md
docs/sdk-refactor/Interface/06-implementation-map.md
docs/sdk-refactor/plan/phase1-beta-migration-execution-plan.md
docs/sdk-refactor/plan/phase2-phase3-migration-execution-plan.md
docs/sdk-refactor/plan/phase4-attachments-migration-execution-plan.md
docs/sdk-refactor/plan/phase5-realtime-runner-migration-execution-plan.md
docs/sdk-refactor/plan/phase5-attachment-enrichment-follow-up-plan.md
```

本计划按以下前提设计：

```text
1. Phase 5 core realtime runner 已完成。
2. client.realtime().run_until_shutdown(...) 已可被 CLI foreground/service-run 调用。
3. im-core 仍不依赖 awiki-cli。
4. runtime listener install/start/stop/restart/uninstall 仍归 CLI 管。
5. Phase 4 attachments 不一定已经完成。
6. Phase 5' attachment enrichment 不一定已经完成。
7. Phase 6 secure / group E2EE 不在本次 cutover 范围内。
```

如果实际执行顺序是：

```text
Phase 5 core -> CLI cutover -> Phase 4 -> Phase 5'
```

那么 cutover 后附件命令先返回明确 unsupported。Phase 4 完成后再单独把附件命令打开；Phase 5' 完成后再打开附件通知 enrichment。

---

## 1. 总体结论

Phase 5 完成后，CLI 的 realtime runner 已经具备直接调用 `im-core` 的基础，但这不等于 CLI 已经整体切到 `im-core`。

CLI cutover 还需要一次明确的默认路径切换：

```text
awiki-cli command
  -> parse flags / resolve workspace / choose identity
  -> build ImCore / ImClient
  -> call im-core public service
  -> render CLI output
```

cutover 后默认行为：

```text
1. 支持的 IM 命令默认走 im-core。
2. im-core 不支持的命令返回 UnsupportedCapability 或从默认命令面隐藏。
3. 不再通过 AWIKI_USE_IM_CORE_MVP 选择旧业务路径。
4. 不再把旧 awiki-cli message/group/identity/runtime 业务实现作为默认 fallback。
5. CLI 仍保留命令解析、workspace/config/path、输出、exit code、service manager、host notify UX。
```

这次 cutover 的核心不是“把所有旧命令都接上”，而是建立一个更薄、更高层的 CLI：

```text
保留：用户要完成的产品任务。
去掉或隐藏：实现细节、调试口、raw wire、SQL、E2EE 内部流程、未实现 stub。
```

---

## 2. 明确目标

### 2.1 本次 cutover 要达到什么

cutover 完成后应满足：

```text
1. CLI 的默认 IM 主链路通过 im-core public API。
2. `runtime listener run` 和 `runtime listener service-run` 通过 Phase 5 runner 运行。
3. `runtime listener install/start/stop/restart/uninstall` 继续只管理 service-run 进程。
4. im_core_adapter 只保留 CLI 边界转换职责，不再承载 legacy business bridge。
5. 命令 schema / docs / completion 只展示当前默认 CLI 要支持的高层能力。
6. 不支持的命令有稳定、可测试、可解释的 unsupported 行为。
7. old awiki-cli legacy implementation 可以暂留在源码中，但默认 dispatch 不再进入它。
```

### 2.2 本次 cutover 不做什么

本次不做：

```text
1. 不为了兼容旧 CLI 暴露 raw RPC / SQL / wire payload。
2. 不把 awiki-cli 的 ParsedCommand / ExitError / config::Resolved 搬进 im-core。
3. 不把 OpenClaw / Hermes / systemd / launchd / Windows service 搬进 im-core。
4. 不迁 Phase 4 attachment send/download，除非 Phase 4 已经完成。
5. 不迁 Phase 5' attachment notification enrichment。
6. 不迁 Phase 6 secure direct / group E2EE。
7. 不迁 mail/page/site 等不属于当前 im-core public API 的产品域。
8. 不保留运行时默认 legacy fallback。
```

---

## 3. CLI 高层命令原则

CLI 应保持高层产品接口。判断一个命令是否应该保留在默认 CLI 中，使用下面规则。

### 3.1 应该保留的命令形态

保留这类命令：

```text
id list
id status
id register
id profile get
msg send
msg inbox
msg history
msg mark-read
group create
group join
group members
runtime listener enable
runtime listener status
runtime host-notify setup
doctor
config show
```

这些命令表达的是用户要完成的任务，不要求用户理解内部 wire、DB、outbox、MLS、WebSocket frame 或 service manager 细节。

### 3.2 不应该出现在默认 CLI 的命令形态

默认 CLI 不应暴露这类命令：

```text
debug raw rpc
debug db query
group e2ee publish-key-package
group e2ee process-leave-request
msg secure failed/retry/drop
runtime host-notify openclaw set-token
runtime host-notify openclaw route add/remove
runtime host-notify hermes set-secret
runtime heartbeat placeholder commands
people.search placeholder command
group code placeholder commands
```

这些命令要么是实现细节，要么是调试入口，要么是尚未完成的占位符。它们可以被：

```text
1. 删除；
2. hidden；
3. 放进 advanced / diagnostic feature；
4. 暂时返回 unsupported；
5. 移到单独的 operator/debug 文档，不进入默认 help/schema。
```

### 3.3 命令不等于 SDK API

CLI 可以有高层命令：

```text
msg send --secure on
group create --message-security-profile group-e2ee
runtime listener service-run
```

但 SDK public API 不能因此暴露：

```rust
build_secure_init_payload(...)
publish_key_package(...)
process_mls_notice(...)
execute_sql(...)
process_websocket_frame(...)
```

如果 im-core 尚未支持对应高层能力，CLI 返回 unsupported，而不是绕回旧模块。

---

## 4. 切换后的职责边界

### 4.1 CLI 继续负责

```text
命令解析、alias、completion、schema
全局参数：--identity / --format / --dry-run / --verbose
config/workspace 解析
显式路径组装：identity root、auth path、SQLite path、runtime path
文件权限、目录创建、备份、atomic write policy
stdout/stderr、pretty/table/json/ndjson 渲染
ImError -> ExitError / exit code / hint 映射
dry-run 展示
service manager：systemd / launchd / Windows service
listener process install/start/stop/restart/uninstall
pid/log/socket 等本机 runtime 管理
OpenClaw / Hermes host notify 配置和投递
```

### 4.2 im-core 负责

```text
ImCore / ImClient 构造后的 IM 业务能力
identity registry / auth / session
directory / profile / contact
direct/group messages
group lifecycle
local_state owner isolation / projection
attachments send/download，Phase 4 后
realtime runner，Phase 5 后
secure diagnostics / secure send integration，Phase 6 后
transport / RPC / wire / DID proof / local store internal implementation
```

### 4.3 Phase 5 完成后的 runtime 分工

Phase 5 完成后，runtime 分工固定为：

```text
runtime listener run
  CLI foreground process
  当前进程/线程构造 ImCore + ImClient
  调用 client.realtime().run_until_shutdown(...)

runtime listener service-run
  service manager 启动的 service-run process
  service-run 主线程构造 ImCore + ImClient
  调用 client.realtime().run_until_shutdown(...)

runtime listener install/start/stop/restart/uninstall
  CLI 只管理 service-run 进程
  不运行 realtime runner
  不消费 ImEvent
```

`im-core` 不创建 OS daemon，不 fork，不安装 service，不决定在哪个进程运行。调用方在哪个线程调用 `run_until_shutdown`，runner 主循环就在哪个线程阻塞运行；如果未来 App 需要 worker thread 或 async task，那是 App 的宿主决策。

---

## 5. im_core_adapter 的 cutover 策略

### 5.1 短期保留

cutover 期间可以继续保留：

```text
crates/awiki-cli/src/im_core_adapter/
  mod.rs
  config.rs
  paths.rs
  identity.rs
  messages.rs
  groups.rs
  attachments.rs       # Phase 4 后
  realtime.rs          # Phase 5 后
  error.rs
  render.rs
  unsupported.rs
```

但它的定位必须从“legacy adapter”变成“CLI boundary adapter”。

### 5.2 允许的职责

`im_core_adapter` 可以做：

```text
1. CLI config -> ImCoreConfig。
2. CLI workspace/identity manager -> ImCorePaths。
3. --identity -> IdentitySelector。
4. ParsedCommand flags -> im-core DTO。
5. CLI dry-run DTO 展示。
6. ImError -> ExitError。
7. Realtime signal / service-run config -> RealtimeOptions / ShutdownSignal。
8. ImEvent -> CLI-owned host notification event。
9. UnsupportedCapability 的统一输出和 exit code。
```

### 5.3 禁止的职责

`im_core_adapter` cutover 后不能做：

```text
1. 把 im-core DTO 转回旧 awiki-cli request。
2. 调用 crate::message::* 旧业务实现。
3. 调用 crate::identity::* 旧业务流程作为业务 fallback。
4. 调用 crate::runtime::listener_* 旧 realtime loop。
5. 暴露 im_core::compat 作为默认路径。
6. 拼 raw RPC method / wire params。
7. 维护 auth retry / target resolve / local projection 等 IM 业务逻辑。
```

### 5.4 最终状态

cutover 完成后：

```text
1. legacy bridge 型 adapter 删除。
2. thin boundary 型 adapter 可以保留。
3. 如果命名容易误导，后续可把 im_core_adapter 重命名为 im_core_boundary。
4. im_core::compat 只允许短期 migration-only；默认 CLI handler 不直接依赖 compat。
```

---

## 6. 命令面 Review 和 cutover 策略

本节按当前 `crates/awiki-cli/src/cmdmeta/mod.rs` 的命令面分类。

### 6.1 CLI-owned，默认保留

这些命令不属于 im-core 业务，但仍是 CLI 产品壳职责，应默认保留：

| 命令 | cutover 策略 | 原因 |
| --- | --- | --- |
| `status` | 保留 | 汇总 CLI workspace / identity / runtime 状态。 |
| `docs` | 保留 | CLI 内置文档入口。 |
| `schema` | 保留 | 命令契约输出，但必须反映 cutover 后命令面。 |
| `doctor` | 保留 | 本机环境和存储诊断。 |
| `version` | 保留 | CLI 版本。 |
| `upgrade` | 保留 | CLI 安装升级提示。 |
| `init` | 保留 | workspace/config 初始化。 |
| `completion.*` | 保留 | shell completion。 |
| `config.show` / `config.set` | 保留 | CLI config 管理。 |

实现要求：

```text
1. 这些命令不得调用旧 IM business path。
2. 如果需要读取 identity/runtime 状态，优先通过 im-core 或 CLI path/status helper。
3. schema/completion 不展示被 hidden 或 removed 的默认外命令。
```

### 6.2 Identity / auth，默认走 im-core

| 命令 | cutover 策略 | im-core API |
| --- | --- | --- |
| `id.status` | 默认走 im-core | `core.identities().list()` + readiness / `client.auth().status()` |
| `id.list` | 默认走 im-core | `core.identities().list()` |
| `id.current` | 默认走 im-core | `core.identities().default_identity()` |
| `id.use` | 默认走 im-core plan，CLI 写 default | `core.identities().plan_default_identity_change(...)` |
| `id.register` | 默认走 im-core | `core.identities().register_handle(...)` |
| `id.refresh-token` | 默认走 im-core | `client.auth().refresh_session()` |
| `id.resolve` | 默认走 im-core | `client.directory().resolve_peer(...)` 或 registry resolve |
| `id.bind` | Phase 2 完成后默认走 im-core | `client.identity().bind_contact(...)` |
| `id.recover` | Phase 2 完成后默认走 im-core | `core.identities().recover_handle(...)` |
| `id.profile.get` / `id.profile.set` | Phase 2 完成后默认走 im-core | `client.identity().profile()` / `update_profile(...)` |
| `id.replace-did` | 默认 hidden 或 advanced | `client.identity().replace_did(...)`，危险能力，不能作为普通默认入口 |
| `id.create` | 保持 hidden | 本地 bootstrap/migration helper，不作为普通产品命令 |
| `id.import-v1` | 保留或 hidden | migration tool；如果保留，仍是 CLI-owned，不进入 im-core public API |

cutover 规则：

```text
1. 如果 Phase 2 对应能力未完成，命令返回 unsupported。
2. 不回到旧 identity handler。
3. `id.replace-did` 即使 im-core 支持，也建议默认 hidden，保留 dry-run 和强提示。
```

### 6.3 Message，默认走 im-core，但收窄到高层能力

| 命令 | cutover 策略 | 说明 |
| --- | --- | --- |
| `msg.send --to ... --text ...` | 默认走 im-core | direct text。 |
| `msg.send --group ... --text ...` | 默认走 im-core | group text。 |
| `msg.send --file ...` | Phase 4 前 unsupported | Phase 4 后走 `client.attachments().send(...)`。 |
| `msg.send --secure on` | Phase 6 前 unsupported | 不回到旧 secure path。 |
| `msg.inbox` | 默认走 im-core | filters 只开放 im-core 支持的高层字段。 |
| `msg.history` | 默认走 im-core | direct/group history 按 im-core DTO。 |
| `msg.mark-read` | Phase 3 完成后默认走 im-core | 未完成则 unsupported。 |
| `msg.attachment.download` | Phase 4 前 unsupported | Phase 4 后走 `client.attachments().download(...)`。 |

需要收窄的点：

```text
1. `--type` 只允许高层 message kind，不暴露 raw content_type。
2. `--secure on` 不触发旧 secure fallback。
3. attachment caption 可继续由 `--text` / `--text-file` 表达，但附件业务由 Phase 4 打开。
4. message id / cursor 可以保留，因为这是用户级分页/定位概念。
```

### 6.4 Mail，由独立 Email 阶段打开默认命令面

Email / Mail 不是 Phase 1 IM MVP，但独立 Email 阶段完成后通过 `im-core::email` 进入默认命令面：

| 命令 | cutover 策略 |
| --- | --- |
| `mail.inbox` / `mail.notify` / `mail.read` / `mail.mark-read` | `client.email()` |
| `mail.account` / `mail.send` | `client.email()` |
| `mail.attachment.download` | `client.email().download_attachment(...)`，CLI 写输出文件 |

理由：

```text
1. mail 是独立产品域，接口与执行顺序由 `docs/sdk-refactor/Interface/08-email-interface.md` 和 `docs/sdk-refactor/plan/email-migration-execution-plan.md` 约束。
2. CLI 只负责 flag parse / dry-run / render / 附件文件写入。
3. cutover 不应为了兼容旧 CLI 而保留一条旧业务 fallback。
4. 系统测试使用 awiki.ai 和 mail-service 配置。
```

### 6.5 Secure direct，Phase 6 前不进入默认命令面

| 命令 | cutover 策略 |
| --- | --- |
| `msg.secure.status` | Phase 6 前 hidden 或 unsupported |
| `msg.secure.init` | Phase 6 前 hidden 或 unsupported |
| `msg.secure.repair` | Phase 6 前 hidden 或 unsupported |
| `msg.secure.failed` / `retry` / `drop` | 默认 hidden；即使 Phase 6 后也建议 diagnostic-only |

Phase 6 后推荐默认只开放高层 secure 能力：

```text
msg send --secure on
msg secure status
msg secure repair
```

`failed/retry/drop` 暴露的是 outbox 内部模型，默认 CLI 不应展示；必要时放到 diagnostic feature。

### 6.6 Group，默认走 im-core，但隐藏 E2EE 和未实现 code stub

Phase 3 完成后，这些命令默认走 im-core：

```text
group.create
group.get
group.join
group.add
group.remove
group.leave
group.update
group.list
group.members
group.messages
```

cutover 收窄规则：

```text
1. `--e2ee` Phase 6 前 unsupported。
2. `--message-security-profile group-e2ee` Phase 6 前 unsupported。
3. 普通 group policy 字段可以保留，但必须映射到 GroupService DTO，不能拼 raw group wire patch。
4. 如果某个 policy 字段 im-core 暂不支持，返回 field-level unsupported。
```

这些命令不进入默认命令面：

| 命令 | cutover 策略 | 原因 |
| --- | --- | --- |
| `group.e2ee.*` | Phase 6 前 hidden / unsupported；Phase 6 后 diagnostic-only | MLS / KeyPackage / pending notices 是实现细节。 |
| `group.code.*` | hidden 或 removed | 当前是未实现 stub，不应展示。 |

### 6.7 Runtime，保留高层宿主命令，runner 走 im-core

| 命令 | cutover 策略 | 说明 |
| --- | --- | --- |
| `runtime.status` | 保留，CLI-owned | 可汇总 service/process/config 和 im-core realtime status。 |
| `runtime.apply` | 保留或收窄 | CLI-owned，本机状态应用。 |
| `runtime.setup` | 保留或收窄 | CLI-owned，本机 runtime bootstrap。 |
| `runtime.mode.get` / `runtime.mode.set` | 保留 | CLI config -> transport policy。 |
| `runtime.listener.status` | 保留 | CLI-owned process/service status。 |
| `runtime.listener.install/start/stop/restart/uninstall` | 保留 | CLI-owned service manager。 |
| `runtime.listener.run` | hidden，走 im-core runner | foreground runner 宿主入口。 |
| `runtime.listener.service-run` | hidden，走 im-core runner | service-run 宿主入口。 |
| `runtime.listener.config.show/set` | 保留或收窄 | CLI-owned listener config。 |
| `runtime.listener.enable/disable` | 保留 | 高层 enable/disable。 |

必须禁止：

```text
1. run/service-run 回到旧 listener session loop。
2. install/start/stop 在 im-core 里实现。
3. im-core 读取 CLI service config 或 host notify config。
```

### 6.8 Host notify，保留高层 UX，隐藏 provider 细节

host notify 属于 CLI runtime UX，不属于 im-core。cutover 后建议默认保留：

```text
runtime.host-notify.config.show
runtime.host-notify.config.set
runtime.host-notify.enable
runtime.host-notify.disable
runtime.host-notify.hermes.guide
runtime.host-notify.hermes.status
runtime.host-notify.hermes.setup
```

建议 hidden 或 advanced：

```text
runtime.host-notify.openclaw.set
runtime.host-notify.openclaw.set-token
runtime.host-notify.openclaw.clear-token
runtime.host-notify.openclaw.route.add
runtime.host-notify.openclaw.route.list
runtime.host-notify.openclaw.route.remove
runtime.host-notify.hermes.set
runtime.host-notify.hermes.set-secret
runtime.host-notify.hermes.clear-secret
runtime.host-notify.hermes.bridge.service-run
```

理由：

```text
1. 用户默认需要的是“打开通知 / 配好通知 / 查看状态”。
2. token、route、secret 是 provider 细节。
3. bridge.service-run 是 service manager 内部入口，应保持 hidden。
```

如果 OpenClaw route 确实是产品级用户需求，应重新设计成一个高层命令，例如：

```text
runtime host-notify setup --provider openclaw
runtime host-notify target add ...
```

而不是把 provider 内部字段作为默认命令树。

### 6.9 Heartbeat / people / page / site / debug

| 命令族 | cutover 策略 | 原因 |
| --- | --- | --- |
| `runtime.heartbeat.*` | removed 或 hidden | 当前是 stub，不应展示。 |
| `people.follow/unfollow/status/followers/following` | im-core | 已由 `DirectoryService` relationship API 支撑；`people.search` 仍 unsupported。 |
| `people.contacts.*` | im-core | 已由 `DirectoryService::save_contact/contacts` 支撑。 |
| `page.*` | hidden 或 unsupported | 不属于当前 im-core IM public API。 |
| `site.*` | hidden 或 unsupported | 不属于当前 im-core IM public API。 |
| `debug.db.handle-history` | advanced/diagnostic-only | 可保留给迁移排错，但不进默认 help。 |
| `debug.db.query` | hidden 或 feature-gated | raw SQL 不应是默认 CLI 产品接口。 |
| `debug.db.import-v1` | migration-only | 可保留但 hidden。 |
| `debug.raw.rpc` | removed 或 hidden | raw RPC 不能作为 SDK/CLI cutover 依据。 |
| `debug.schema-cache` / `debug.logs` | hidden until implemented | stub 不展示。 |

---

## 7. Unsupported contract

cutover 后，unsupported 不是临时 panic，也不是静默 fallback。它必须是稳定 CLI contract。

### 7.1 Pretty 输出

示例：

```text
Command is not supported by the im-core CLI cutover path.

command: msg send --file
capability: attachments
required phase: Phase 4
hint: run text-only msg send now, or enable this command after Phase 4 attachments lands.
```

### 7.2 JSON 输出

建议统一结构：

```json
{
  "ok": false,
  "error": {
    "code": "unsupported_capability",
    "command": "msg.send",
    "capability": "attachments",
    "required_phase": "Phase 4",
    "message": "attachments are not supported by the im-core CLI cutover path",
    "hint": "Use text-only msg send until Phase 4 is enabled."
  }
}
```

### 7.3 Exit code

建议：

```text
unsupported capability -> exit 2
invalid input          -> exit 2
auth/session required  -> exit 3
not found              -> exit 4
transport unavailable  -> exit 5
internal error         -> exit 1
```

exit code 仍是 CLI 策略，不进入 `ImError`。

---

## 8. 执行 PR 计划

### PR C0：cutover command classifier

目标：

```text
为所有当前命令建立 cutover 分类，不改默认业务行为。
```

建议改动：

```text
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/src/app/unsupported.rs
crates/awiki-cli/tests/cli_cutover_command_surface_contract.rs
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
```

分类建议：

```rust
enum CutoverStatus {
    CliOwned,
    ImCore,
    Unsupported { capability: &'static str, phase: &'static str },
    Hidden,
    Removed,
    DiagnosticOnly,
}
```

如果不想改 `CommandSpec`，也可以先维护一个独立 classifier：

```rust
fn cutover_status(command_name: &str) -> CutoverStatus
```

验收：

```text
1. 每个 command spec 都有 cutover 分类。
2. schema/help/completion 能排除 Hidden/Removed 命令，或至少测试能识别它们。
3. unsupported 输出 contract 有单元测试。
4. 不改变当前默认业务路径。
```

### PR C1：thin im_core_adapter policy

目标：

```text
把 im_core_adapter 明确收口成 CLI boundary，不再新增 legacy bridge。
```

建议改动：

```text
crates/awiki-cli/src/im_core_adapter/mod.rs
crates/awiki-cli/src/im_core_adapter/config.rs
crates/awiki-cli/src/im_core_adapter/paths.rs
crates/awiki-cli/src/im_core_adapter/identity.rs
crates/awiki-cli/src/im_core_adapter/error.rs
crates/awiki-cli/src/im_core_adapter/unsupported.rs
```

验收：

```bash
rg "crate::message::|crate::runtime::listener_session|crate::identity::.*register|im_core::compat" crates/awiki-cli/src/im_core_adapter
```

允许的例外必须逐条注释为 temporary migration-only，并在后续 PR 删除。

### PR C2：默认路径切到 im-core，关闭 runtime legacy fallback

目标：

```text
移除 AWIKI_USE_IM_CORE_MVP 作为默认路径开关。
支持命令走 im-core；不支持命令走 unsupported；不回旧业务路径。
```

建议改动：

```text
crates/awiki-cli/src/app/mod.rs
crates/awiki-cli/src/app/*_handlers.rs
crates/awiki-cli/src/im_core_adapter/*
crates/awiki-cli/tests/*im_core*_contract.rs
```

规则：

```text
1. `AWIKI_USE_IM_CORE_MVP=1` 不再是进入 im-core 的前置条件。
2. 如果保留 emergency legacy flag，必须默认关闭、文档标注删除日期、测试不依赖它。
3. cutover command classifier 决定 dispatch。
4. unsupported command 不能进入旧 handler。
```

静态检查：

```bash
rg "AWIKI_USE_IM_CORE_MVP|use_im_core_mvp" crates/awiki-cli/src crates/awiki-cli/tests
rg "run_.*_legacy|legacy path|fallback legacy" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

验收：

```text
1. 不设置任何 env 时，IM 支持命令走 im-core。
2. unsupported 命令返回稳定 unsupported。
3. legacy path 不再是默认 dispatch 分支。
```

### PR C3：identity / auth cutover

目标：

```text
身份和 auth 命令默认通过 im-core。
```

建议命令：

```text
id list
id current
id status
id use
id register
id refresh-token
id resolve
id bind
id recover
id profile get
id profile set
```

危险或迁移命令：

```text
id replace-did -> hidden/advanced，im-core 未完成则 unsupported
id create      -> hidden
id import-v1   -> migration-only，CLI-owned 或 hidden
```

验收：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli id_
rg "run_id_.*legacy|identity::register|identity::recover" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

如果 test 名称已改，应使用当前同等 focused selector，并记录替代命令。

### PR C4：message / group text cutover

目标：

```text
消息和普通群命令默认通过 im-core。
```

建议命令：

```text
msg send --to ... --text ...
msg send --group ... --text ...
msg inbox
msg history
msg mark-read

group create
group get
group join
group add
group remove
group leave
group update
group list
group members
group messages
```

必须 unsupported：

```text
msg send --file
msg attachment download
msg send --secure on
group create --message-security-profile group-e2ee
group add/remove/leave --e2ee
```

直到对应 Phase 完成。

验收：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
rg "crate::message::send|message::SendRequest|build_group_.*rpc|build_direct_.*rpc" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

### PR C5：Phase 5 runtime runner cutover

目标：

```text
runtime listener run/service-run 只作为 im-core realtime runner 宿主。
```

建议命令：

```text
runtime listener run
runtime listener service-run
runtime listener status
runtime listener install/start/stop/restart/uninstall
runtime listener enable/disable
```

实现规则：

```text
1. run/service-run 构造 ImCore / ImClient。
2. run/service-run 调用 client.realtime().run_until_shutdown(...).
3. Ctrl-C / SIGTERM / service stop 转成 ShutdownSignal。
4. install/start/stop/restart/uninstall 不构造 realtime runner。
5. host notification delivery 仍由 CLI 消费 ImEvent 后投递。
```

验收：

```bash
cargo test -p im-core realtime
cargo test -p awiki-cli --test runtime_listener_foreground_contract
cargo test -p awiki-cli --test runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
rg "listener_session_loop|run_listener_session|legacy.*listener" crates/awiki-cli/src/runtime crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

### PR C6：默认命令面裁剪

目标：

```text
让 help/schema/completion 展示一个高层、可支持的 CLI。
```

建议处理：

```text
mail.*                       -> im-core Email service
msg.secure.failed/retry/drop -> hidden / diagnostic-only
group.e2ee.*                 -> hidden / diagnostic-only
group.code.*                 -> removed / hidden
runtime.heartbeat.*          -> removed / hidden
people.search                -> unsupported until search API exists
people.follow/status/followers/following/contacts.* -> im-core directory relationship/contact API
page.* / site.*              -> hidden / unsupported
debug.raw.*                  -> removed / hidden
debug.db.query               -> hidden / feature-gated
provider token/secret/route  -> hidden / advanced
```

验收：

```bash
cargo test -p awiki-cli --test cli_schema_contract
cargo test -p awiki-cli --test cli_parser_unknown_global_flags
awiki-cli schema
awiki-cli docs
```

检查：

```text
1. 默认 help 不展示 unsupported stub。
2. schema 不把 hidden/removed 命令当作普通 supported 命令。
3. completion 不补全 removed 命令。
4. 直接调用 hidden 内部入口时，要么允许 service manager 使用，要么返回稳定 unsupported。
```

### PR C7：legacy bridge cleanup

目标：

```text
删除默认路径上的旧业务桥接。
```

清理对象：

```text
1. im_core_adapter 中 DTO -> old request 的转换。
2. app handlers 中对 crate::message / crate::group / crate::identity legacy flow 的默认调用。
3. im_core::compat 在 CLI 默认 handler 中的使用。
4. AWIKI_USE_IM_CORE_MVP 和旧 fallback 测试。
```

验收检查：

```bash
rg "AWIKI_USE_IM_CORE_MVP|use_im_core_mvp" crates/awiki-cli/src crates/awiki-cli/tests docs
rg "im_core::compat" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg "crate::message::|crate::runtime::listener_|message::SendRequest|InboxRequest|HistoryRequest" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

允许保留的旧模块必须满足：

```text
1. 不在默认 dispatch 可达路径。
2. 有 issue/TODO 指向删除计划。
3. 只为迁移工具、diagnostic-only 或历史测试暂留。
```

### PR C8：Phase 4 后打开 attachments

只有 Phase 4 完成后执行。

目标：

```text
把附件 send/download 从 unsupported 改为 im-core attachments public API。
```

命令：

```text
msg send --file ...
msg attachment download ...
```

规则：

```text
1. CLI 解析 path、overwrite、mime type、caption。
2. im-core 执行 digest / upload / commit / manifest / send / download。
3. CLI 不直接调用旧 attachment helper。
4. Phase 5' 前，附件 realtime notification enrichment 仍不要求完成。
```

验收：

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_attachment_contract
rg "attachment_slot|commit_object|download_ticket|message::attachment" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

### PR C9：Phase 5' 后打开 attachment notification enrichment

只有 Phase 5' 完成后执行。

目标：

```text
让 runtime listener 产生的附件类 notification 有附件摘要和 download action metadata。
```

规则：

```text
1. CLI host notification 只负责展示和投递。
2. im-core realtime projection 负责附件 metadata enrichment。
3. CLI 不在 host notify 层重新解析 raw message payload。
4. 无法 enrichment 时保留 generic notification fallback。
```

验收：

```bash
cargo test -p im-core realtime attachments
cargo test -p awiki-cli --test runtime_listener_foreground_contract
cargo test -p awiki-cli --test runtime_host_notify_local_contracts
```

---

## 9. Cutover 后推荐默认命令面

cutover 后，默认 CLI 建议展示为：

```text
awiki-cli status
awiki-cli doctor
awiki-cli init
awiki-cli config show
awiki-cli config set
awiki-cli id list
awiki-cli id current
awiki-cli id use
awiki-cli id status
awiki-cli id register
awiki-cli id refresh-token
awiki-cli id resolve
awiki-cli id profile get
awiki-cli id profile set
awiki-cli msg send
awiki-cli msg inbox
awiki-cli msg history
awiki-cli msg mark-read
awiki-cli group create
awiki-cli group get
awiki-cli group join
awiki-cli group leave
awiki-cli group list
awiki-cli group members
awiki-cli group messages
awiki-cli runtime status
awiki-cli runtime mode get
awiki-cli runtime mode set
awiki-cli runtime listener status
awiki-cli runtime listener enable
awiki-cli runtime listener disable
awiki-cli runtime listener install
awiki-cli runtime listener start
awiki-cli runtime listener stop
awiki-cli runtime listener restart
awiki-cli runtime listener uninstall
awiki-cli runtime host-notify config show
awiki-cli runtime host-notify config set
awiki-cli runtime host-notify enable
awiki-cli runtime host-notify disable
awiki-cli runtime host-notify hermes guide
awiki-cli runtime host-notify hermes status
awiki-cli runtime host-notify hermes setup
awiki-cli docs
awiki-cli schema
awiki-cli completion ...
awiki-cli version
awiki-cli upgrade
```

Phase 4 后再展示：

```text
awiki-cli msg send --file ...
awiki-cli msg attachment download ...
```

Phase 6 后再评估展示：

```text
awiki-cli msg send --secure on
awiki-cli msg secure status
awiki-cli msg secure repair
```

不建议默认展示：

```text
debug raw rpc
debug db query
group e2ee publish-key-package
group e2ee pending
group e2ee process-leave-request
msg secure failed/retry/drop
runtime heartbeat *
people.search
group code stubs
page/site/mail
provider token/secret/route internals
```

---

## 10. 测试和验收

### 10.1 基础测试

建议每个 cutover PR 至少跑：

```bash
cargo test -p im-core
cargo test -p awiki-cli
```

如果 `cargo test -p awiki-cli` 成本过高或已有历史失败，必须记录 focused selector：

```bash
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test runtime_listener_foreground_contract
cargo test -p awiki-cli --test runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

### 10.2 Unsupported contract 测试

必须覆盖：

```text
msg send --file                         # Phase 4 前 unsupported
msg attachment download                 # Phase 4 前 unsupported
msg send --secure on                    # Phase 6 前 unsupported
group create --message-security-profile group-e2ee
group e2ee publish-key-package
debug raw rpc
debug db query
mail inbox
page list
site page list
runtime heartbeat status
people search
```

### 10.3 Runtime cutover 测试

Phase 5 完成后必须覆盖：

```text
runtime listener run
  -> calls im-core run_until_shutdown
  -> current process/thread blocks until shutdown
  -> shutdown returns RealtimeExit

runtime listener service-run
  -> calls im-core run_until_shutdown
  -> service stop signal maps to ShutdownSignal

runtime listener start/stop/install/uninstall
  -> does not run realtime runner
  -> only manages service process
```

### 10.4 静态边界检查

cutover 完成后建议检查：

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
rg "AWIKI_USE_IM_CORE_MVP|use_im_core_mvp" crates/awiki-cli/src crates/awiki-cli/tests
rg "im_core::compat" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg "crate::message::|message::SendRequest|InboxRequest|HistoryRequest" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg "listener_session_loop|run_listener_session" crates/awiki-cli/src/app crates/awiki-cli/src/runtime crates/awiki-cli/src/im_core_adapter
```

允许例外必须写在 cutover tracking 文档或对应 TODO 中，且不能在默认 dispatch 可达路径上。

---

## 11. 风险和处理策略

| 风险 | 处理策略 |
| --- | --- |
| 旧 CLI 命令很多，全部迁移会拖慢 cutover | 只迁高层 IM 主链路；其他 hidden/unsupported。 |
| 用户误以为旧命令还能用 | unsupported 输出必须明确 required phase / replacement。 |
| im-core 能力缺口导致 handler 半迁移 | 命令 classifier 先落地，缺口走 unsupported。 |
| runtime runner 已迁但 service manager 混入 im-core | 明确 service manager 永远 CLI-owned。 |
| debug/raw/SQL 命令影响 SDK 边界 | 默认 hidden 或 feature-gated，不作为 public API 依据。 |
| im_core_adapter 继续长成第二套业务层 | 只允许 boundary conversion，禁止 legacy bridge。 |
| legacy fallback 掩盖问题 | 默认不 fallback；需要回滚用 git revert 或 release rollback。 |
| Phase 4 未完成但用户需要附件 | 先返回 unsupported；Phase 4 后 C8 单独打开。 |
| Phase 5' 未完成但通知有附件 | 保持 generic notification，不做附件 enrichment。 |
| Phase 6 未完成但命令有 secure | `--secure on` 和 secure commands 返回 unsupported。 |

---

## 12. 回滚策略

cutover 不建议保留默认 runtime fallback。推荐回滚方式：

```text
1. 小 PR 切片，每个 PR 可 git revert。
2. cutover 前保留 release tag。
3. 如果需要 emergency legacy flag，只能短期存在，并且：
   - 默认关闭；
   - 不进入文档主路径；
   - 有删除 PR；
   - 测试覆盖“默认不 fallback”。
4. unsupported 命令不回滚到旧实现。
```

不建议：

```text
1. 在每个 handler 中保留 legacy else 分支。
2. 用 env flag 长期维持两套默认路径。
3. 让 schema/help 显示实际不会走 im-core 的旧命令。
```

---

## 13. 完成定义

本计划完成时，应满足：

```text
1. 默认 CLI IM 主链路通过 im-core。
2. `runtime listener run/service-run` 通过 Phase 5 runner。
3. `runtime listener install/start/stop/restart/uninstall` 仍只在 CLI 管 service。
4. Phase 4 前附件命令明确 unsupported；Phase 4 后再打开。
5. Phase 6 前 secure/group E2EE 明确 unsupported 或 hidden。
6. mail/page/site/people/heartbeat/debug raw 等非当前 im-core 能力不进入默认命令面。
7. im_core_adapter 不再把 SDK DTO 转回旧业务 request。
8. `AWIKI_USE_IM_CORE_MVP` 不再控制默认路径。
9. schema/docs/completion 和 cutover 后的默认命令面一致。
10. 边界检查确认 im-core 不引用 CLI 类型，CLI 默认 handler 不依赖 legacy business path。
```

一句话目标：

```text
awiki-cli 保留“命令行产品壳”的高层体验；
im-core 承接“选择身份后执行 IM 业务”的主实现；
旧 awiki-cli 业务模块不再是默认运行路径。
```
