# CLI Shell Final Cutover and Command Surface Review Execution Plan

**当前草案路径**：`docs/sdk-refactor/plan/cli-shell-final-cutover-execution-plan2.md`  
**最终落地路径**：`docs/sdk-refactor/plan/cli-shell-final-cutover-execution-plan.md`  
**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用分支**：`main`  
**版本**：Final v2  
**适用阶段**：E2EE / realtime / attachment / email 主迁移基本完成后，执行最终 CLI shell cutover。  
**目标**：完成两件事：  
1. 去掉 `awiki-cli` 默认路径中的旧业务链路，让底层 IM 实现全部进入 `crates/im-core`。  
2. Review 并重构 CLI 命令面，使 `awiki-cli` 成为只负责命令行 UX 的薄壳，CLI 命令映射到 `im-core` 高级 Interface，而不是暴露 wire、store、runtime、E2EE、MLS 等内部细节。

---

## 0. 本版相对上一版的关键修正

本版把 CLI 命令面重构从“建议”提升为“强约束”。主要修正：

```text
1. 默认命令面收窄：runtime service manager、provider token/secret/route、internal service-run 不再进入 default surface。
2. CommandAudience / primary_owner / secondary_owners / CliShellRole / DirectInvocationPolicy 从建议字段改为 CommandSpec 必须字段。
3. dispatch 前必须统一 enforce command policy，不再只靠 ad hoc blocked domain。
4. Hidden / InternalService / DiagnosticOnly / Unsupported / Removed 的 direct invocation 规则固定。
5. F0 静态门禁改为 baseline + allowlist burn-down，不要求第一 PR 就全量清零。
6. 明确 CLI 与 im-core 对本地文件、backup、atomic write 的边界。
7. 命令 alias 统一由 DirectInvocationPolicy 表达，不再维护第二套 AliasPolicy。
8. `runtime host-notify status/setup/target` 作为高层 facade 单独落地，再隐藏 provider-specific internals。
9. `id recover`、`group add/remove/update`、`debug db query` 的最终策略固定，不再保留二选一。
```

执行完成后的判断标准不是“schema 里少展示了旧命令”，而是：

```text
1. 默认 help/schema/completion 只展示高层产品命令。
2. 直接输入旧 detail 命令时，必须按统一 policy 执行、拒绝或诊断开关进入。
3. 默认 dispatch 不会绕过 policy 进入旧 handler。
4. CLI 默认看起来像产品壳，而不是 operator/debug/internal control panel。
```

---

## 1. 必读约束

执行本计划前，先阅读并遵守：

```text
docs/sdk-refactor/README.md
docs/sdk-refactor/architecture.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/cli-boundary.md
docs/sdk-refactor/im-core-cli-boundary.md
docs/sdk-refactor/implementation-playbook.md

docs/sdk-refactor/Interface/README.md
docs/sdk-refactor/Interface/01-crate-layout.md
docs/sdk-refactor/Interface/02-core-interface.md
docs/sdk-refactor/Interface/03-identity-auth-interface.md
docs/sdk-refactor/Interface/05-cli-adapter-interface.md
docs/sdk-refactor/Interface/06-implementation-map.md
docs/sdk-refactor/Interface/07-phase1-acceptance.md
docs/sdk-refactor/Interface/08-email-interface.md

docs/sdk-refactor/modules/01-core.md
docs/sdk-refactor/modules/02-identity.md
docs/sdk-refactor/modules/03-auth.md
docs/sdk-refactor/modules/04-local-state.md
docs/sdk-refactor/modules/05-discovery.md
docs/sdk-refactor/modules/06-directory.md
docs/sdk-refactor/modules/07-messages.md
docs/sdk-refactor/modules/08-groups.md
docs/sdk-refactor/modules/09-attachments.md
docs/sdk-refactor/modules/10-secure.md
docs/sdk-refactor/modules/11-realtime.md

docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
docs/sdk-refactor/plan/email-migration-execution-plan.md
docs/sdk-refactor/plan/phase4-attachments-migration-execution-plan.md
docs/sdk-refactor/plan/phase5-realtime-runner-migration-execution-plan.md
docs/sdk-refactor/plan/phase6-secure-e2ee-migration-execution-plan.md
```

本计划是 **final cutover plan**。执行完成后，默认 CLI 路径不得再回到旧 `awiki-cli` 业务模块。

---

## 2. 总体结论

最终状态：

```text
awiki-cli command
  -> parse flags / args / globals
  -> enforce command policy
  -> resolve CLI config and workspace paths
  -> build ImCore / ImClient
  -> convert CLI input to im-core DTO
  -> call im-core public service
  -> render stdout/stderr / exit code / dry-run plan
```

不再允许默认路径：

```text
awiki-cli command
  -> crate::message::* business logic
  -> crate::identity::* business flow
  -> crate::content::* / crate::site::* as default command surface
  -> crate::runtime listener legacy session loop
  -> im_core::compat as default CLI execution path
  -> old request DTO such as message::SendRequest / InboxRequest / HistoryRequest
```

`awiki-cli` 不是完全没有代码；它保留的是命令行宿主职责：

```text
命令解析
alias / schema / completion / docs
--identity / --format / --dry-run / --verbose
config/workspace/path 解析
ImCoreConfig / ImCorePaths 组装
用户输入文件读取，例如 --text-file / --markdown-file
用户输出路径解析和提示
stdout/stderr 渲染
ExitError / exit code
service manager: systemd / launchd / Windows service
pid/log/socket
OpenClaw / Hermes host notification UX
diagnostic / migration-only 工具
```

`im-core` 承担所有 IM 产品实现：

```text
identity registry / auth / session
Handle register / recover / profile / directory / contacts / relationship
direct/group message send / inbox / history / mark-read / conversations
group lifecycle / members / messages / policy
attachments send/download / digest / upload / manifest / ticket / SDK-managed atomic write
local_state owner isolation / projection / cache merge / retry state
realtime runner / notification classify / ImEvent projection
direct E2EE / group E2EE / secure outbox / incoming decrypt / MLS state
email service
transport / RPC / wire / DID proof / service discovery
```

---

## 3. 当前残留风险基线

按当前 `main` 的阅读结果，默认路径或准默认路径里还有这些残留：

| 编号 | 残留点 | 当前问题 | cutover 目标 |
| --- | --- | --- | --- |
| R1 | attachment fallback | `msg send --file` / `msg attachment download` 先尝试 im-core，失败后 fallback 到 `crate::message::*` | 删除 fallback。im-core 支持则执行，不支持则稳定 unsupported |
| R2 | attachment compat | CLI adapter 调 `im_core::compat::attachments::*_with_details` | 改为 `client.attachments().send/download` public API |
| R3 | message local projection | `im_core_adapter/messages.rs` 在 send 成功后手写 `store::store_message` | projection 下沉到 im-core local_state |
| R4 | group cache projection | `im_core_adapter/groups.rs` 读取/补全 CLI `store` group snapshot/members | group projection/cache 下沉到 im-core |
| R5 | identity recover local finalize | recover 远端调用已走 im-core，但本地 generate/save/merge/finalize 仍走旧 identity/store | recover finalize/merge 下沉到 im-core，或明确变成 migration-only |
| R6 | runtime listener session bootstrap | listener host 仍使用旧 `message::auth_session`、`authsdk::Session`、旧 `WsTransport` | realtime session/auth/connect 下沉到 im-core |
| R7 | runtime secure side effects | secure prekey retry、secure backlog poll、secure outbox flush、local ACK 仍在 `awiki-cli::message` / `anpsdk` | secure runtime side effects 下沉到 im-core |
| R8 | group E2EE commands | `group.e2ee.*` dry-run only，非 dry-run unsupported | `status/repair` 迁到 `group secure *`；其他低层命令 hidden/internal/test-only；高层 secure group API 到 im-core |
| R9 | page/site/debug | 代码仍存在旧 handler | 保持 unsupported/diagnostic/hidden，不进入默认 surface |
| R10 | command surface | 当前命令树仍混有 high-level product 命令、diagnostic 命令、provider internals、stub | 重构命令面，只默认展示高层能力 |
| R11 | direct invocation policy | 当前 default boundary block 是局部 hard-code，不能覆盖全部 Hidden/DiagnosticOnly/InternalService | dispatch 前统一 enforce policy |

---

## 4. 成功标准

### 4.1 代码成功标准

完成后必须满足：

```text
1. 不设置任何 env 时，所有 default IM / mail / people / group 命令通过 im-core public API。
2. app handlers 不直接调用旧 message/content/site/identity business flow。
3. im_core_adapter 只做 CLI boundary，不做 legacy bridge。
4. runtime listener run/service-run 使用 im-core realtime runner；session/auth/connect/secure side effects 不再由 awiki-cli message 模块实现。
5. attachment 不再 fallback 到 crate::message。
6. message/group local projection 不再由 CLI adapter 手写 store mutation。
7. im_core::compat 不在 default CLI execution path 中出现。
8. schema/help/completion default surface 只展示 high-level supported commands。
9. Hidden / DiagnosticOnly / InternalService / Unsupported / Removed 命令有统一 direct invocation policy。
10. im-core 仍不依赖 awiki-cli。
```

### 4.2 CLI 体验成功标准

用户看到的默认命令面应该表达产品任务，而不是实现细节：

```text
保留：id status / msg send / group create / runtime listener enable / mail inbox
隐藏：raw RPC / raw SQL / provider token / MLS KeyPackage / secure outbox internals
unsupported：尚无 im-core 高级 API 的能力
diagnostic-only：迁移排障、历史导入、本地数据库检查
internal-service：service manager 启动入口，不作为用户命令
operator：service install/start/stop/restart/uninstall 等本机管理能力，默认不展示
```

---

## 5. 强制命令策略模型

### 5.1 CommandSpec 必须字段

最终 `CommandSpec` 不应只靠 `hidden`、`implemented`、`handler`、`phase` 表达命令面。必须新增这些字段，或用等价结构表达：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAudience {
    DefaultUser,
    AdvancedUser,
    Operator,
    Diagnostic,
    MigrationOnly,
    InternalService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOwner {
    CliShell,
    ImCoreIdentity,
    ImCoreAuth,
    ImCoreDirectory,
    ImCoreMessages,
    ImCoreGroups,
    ImCoreAttachments,
    ImCoreRealtime,
    ImCoreSecure,
    ImCoreEmail,
    CliDiagnostic,
    CliMigration,
    ExternalUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliShellRole {
    None,
    ParsesInputOnly,
    WritesDefaultIdentityFile,
    ReadsUserInputFile,
    WritesUserOutputFile,
    RendersDryRunPlan,
    ManagesLocalService,
    ManagesHostNotifyConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectInvocationPolicy {
    Allow,
    AllowWithWarning,
    RequireDiagnosticGate,
    RequireMigrationGate,
    RequireInternalServiceGate,
    StableUnsupported {
        capability: &'static str,
        phase: &'static str,
    },
    Removed {
        replacement: Option<&'static str>,
    },
    DeprecatedAlias {
        replacement: &'static str,
        until: &'static str,
    },
}

pub struct CommandSpec {
    pub name: &'static str,
    pub canonical_name: &'static str,
    pub audience: CommandAudience,
    pub primary_owner: CommandOwner,
    pub secondary_owners: &'static [CommandOwner],
    pub cli_shell_role: CliShellRole,
    pub direct_invocation: DirectInvocationPolicy,
    // existing fields: use_, short, aliases, phase, handler, flags, ...
}
```

`CutoverStatus` 可以继续保留，但它不能替代这三个字段。推荐关系：

```text
CutoverStatus       = broad migration status
CommandAudience     = who should see this command
CommandOwner        = which layer primarily owns the implementation
secondary_owners    = additional services involved in orchestration
CliShellRole        = what non-business shell responsibility remains in CLI
DirectInvocationPolicy = what happens when user directly invokes it
```

复合职责不能写成字符串，例如 `ImCoreIdentity + CliShell default write`。必须拆成：

```text
primary_owner     = ImCoreIdentity
secondary_owners  = []
cli_shell_role    = WritesDefaultIdentityFile
```

再如 `msg send --secure required`：

```text
primary_owner     = ImCoreMessages
secondary_owners  = [ImCoreSecure]
cli_shell_role    = ParsesInputOnly
```

### 5.2 default surface 规则

默认 schema/help/completion 只展示：

```text
audience == DefaultUser
AND primary_owner in {CliShell, ImCoreIdentity, ImCoreAuth, ImCoreDirectory, ImCoreMessages, ImCoreGroups, ImCoreAttachments, ImCoreRealtime, ImCoreSecure, ImCoreEmail}
AND direct_invocation in {Allow, AllowWithWarning}
```

不进入 default surface：

```text
AdvancedUser
Operator
Diagnostic
MigrationOnly
InternalService
Unsupported
Removed
provider-specific internals
raw debug commands
old deprecated low-level aliases
```

### 5.3 direct invocation policy

dispatch 前必须统一执行：

```rust
pub fn dispatch(app: &App, command: &ParsedCommand) -> Result<(), ExitError> {
    enforce_command_policy(command)?;
    match command.name.as_str() {
        ...
    }
}
```

不允许只在 `default_cutover_boundary_error()` 里 hard-code 少数 blocked domain。

规则：

| Policy | direct invocation 行为 | schema/help/completion |
| --- | --- | --- |
| `Allow` | 正常执行 | 按 audience 展示 |
| `AllowWithWarning` | 执行，并输出 warning | 不进 default，除非 audience 是 DefaultUser |
| `RequireDiagnosticGate` | 无 `--diagnostic` 或 env gate 时返回 `diagnostic_gate_required` | 只在 `schema --all` / `schema --audience diagnostic` 展示 |
| `RequireMigrationGate` | 无 `--migration` 或 env gate 时返回 `migration_gate_required` | 只在 migration view 展示 |
| `RequireInternalServiceGate` | 无 internal env/token 时返回 `internal_command` | default 永不展示 |
| `StableUnsupported` | 永远返回 stable unsupported envelope | default 不展示 |
| `Removed` | 返回 removed/unsupported，并给 replacement hint | 不展示 |
| `DeprecatedAlias` | 转发到 replacement 或返回 hint，输出 deprecation warning | default 不展示旧 alias |

新增 policy gate flags / env：

```text
--diagnostic
--migration
AWIKI_CLI_ENABLE_DIAGNOSTIC=1
AWIKI_CLI_ENABLE_MIGRATION=1
AWIKI_CLI_INTERNAL_ENTRY=1     # 防误用 gate，只由 service manager / internal launcher 设置
```

`--diagnostic` 和 `--migration` 是真正 global flags，允许出现在 command 前或 command 后。`--audience <default|advanced|operator|diagnostic|migration|internal|all>` 不是 global flag，只属于 `schema` / `docs` / `completion` 的 local flag；在其他命令上出现必须返回 unknown flag。

`AWIKI_CLI_INTERNAL_ENTRY=1` 只作为防误用边界，不作为安全边界。如果将来需要防止普通用户伪造内部入口，service manager 必须额外传入一次性 token 或受限 socket credential，并在 `RequireInternalServiceGate` 中验证。

### 5.4 stable error envelope

Diagnostic gate 示例：

```json
{
  "ok": false,
  "error": {
    "code": "diagnostic_gate_required",
    "command": "debug.db.handle-history",
    "message": "debug.db.handle-history is a diagnostic command",
    "hint": "Re-run with --diagnostic or inspect schema --audience diagnostic."
  }
}
```

Internal service gate 示例：

```json
{
  "ok": false,
  "error": {
    "code": "internal_command",
    "command": "runtime.listener.service-run",
    "message": "runtime.listener.service-run is an internal service entry",
    "hint": "Use runtime listener start/stop, or let the service manager launch this entry."
  }
}
```

Removed command 示例：

```json
{
  "ok": false,
  "error": {
    "code": "removed_command",
    "command": "debug.raw.rpc",
    "replacement": null,
    "message": "debug.raw.rpc is removed from the im-core CLI cutover path",
    "hint": "Use high-level im-core commands instead of raw RPC."
  }
}
```

---

## 6. CLI 与 im-core 对文件 / backup / atomic write 的边界

为了避免 `cli-boundary.md` 与 final cutover 互相冲突，本计划固定以下边界。

### 6.1 CLI 继续负责

```text
workspace discovery
config file discovery and update
用户显式输入路径，例如 --text-file / --markdown-file / --output
用户显式输出文件写入，例如 mail attachment 保存路径
service pid/log/socket/status file
systemd / launchd / Windows service 文件
OpenClaw / Hermes 本机配置
stdout/stderr / pretty/json/table / jq
exit code
dry-run 文本展示
```

### 6.2 im-core 负责

```text
SDK-managed identity store transactions
SDK-managed local_state SQLite schema and mutation
SDK-managed message/group/contact/conversation projection
SDK-managed attachment temp file / digest / upload / download / atomic write
SDK-managed recover backup and restore consistency
SDK-managed secure session / prekey / MLS state
SDK-managed outbox / retry / failed state
```

### 6.3 `id recover` 的特殊规则

`id recover` 的本地一致性属于 im-core：

```text
generate recovered identity material
save recovered identity
backup old SDK-managed identity/local_state
merge old owner state into new owner state
finalize recovered handle
cleanup stale E2EE / MLS / outbox state
```

CLI 只负责：

```text
flags
dry-run summary
危险提示
用户确认
render backup path
```

如果未来允许用户指定外部 export/backup path：

```text
CLI 负责解析用户 path 和权限提示。
im-core 负责在显式路径中写入 SDK-managed backup payload。
```

---

## 7. 总体执行策略

本计划分两条工作流，但按 PR 交错执行。

```text
Workstream A：旧链路清零
  A0 inventory and static gates baseline
  A1 attachments final cutover
  A2 message local_state projection cutover
  A3 group local_state projection cutover
  A4 identity recover / migration boundary cutover
  A5 realtime session/auth/connect cutover
  A6 secure runtime side effects cutover
  A7 compat and legacy module cleanup

Workstream B：CLI 命令面重构
  B0 command inventory
  B1 mandatory command policy fields
  B2 high-level interface mapping
  B3 default surface shrink
  B4 direct invocation enforcement
  B5 rename/deprecation/alias policy
  B6 schema/help/completion/docs update
  B7 command contract tests
```

推荐 PR 以 **小切片、可测试、无默认 fallback** 为原则。单个 PR 不要同时做 attachment、runtime、command rename、schema cleanup。

---

## 8. Workstream A：旧链路清零

### A0：建立 final cutover baseline 与 allowlist burn-down

F0 不能一开始就要求所有 final gate 为零，因为当前仓库仍有 compat/legacy 过渡。F0 的目标是 **建立基线、记录 offenders、让后续 PR 持续减少 allowlist**。

新增：

```text
docs/sdk-refactor/legacy-path-baseline.md
crates/awiki-cli/tests/cli_shell_boundary_contract.rs
crates/awiki-cli/tests/legacy_path_cutover_contract.rs
crates/awiki-cli/tests/cli_command_surface_contract.rs
scripts/sdk-refactor/final-cutover-check.sh
```

F0 测试策略：

	```text
	1. 扫描 legacy needles。
	2. 与 allowlist 比对。
	3. 如果出现 allowlist 外的新 offender，则 fail。
	4. 当前已知 offender 允许通过，但必须列在 baseline 文件里。
	5. 所有后续 PR 都不得新增 allowlist offender。
	6. Workstream A 的迁移 PR 必须减少对应 area 的 allowlist item，除非该 PR 只改 docs/tests。
	7. Workstream B 的 command-policy/schema PR 不强制减少 legacy allowlist，但不得新增 legacy path。
8. F7/F13 最终要求 allowlist 为空或只剩明确不属于 default path 的 migration-only 项。
	```

baseline 文件建议格式：

```markdown
# Legacy Path Baseline

| Area | File | Needle | Reason | Removal PR |
| --- | --- | --- | --- | --- |
| attachments | crates/awiki-cli/src/app/msg_handlers.rs | legacy_attachment_send | remove in F1 | F1 |
| messages | crates/awiki-cli/src/im_core_adapter/messages.rs | store::store_message | move projection to im-core | F2 |
| runtime | crates/awiki-cli/src/runtime/listener_supervisor_run.rs | use crate::message | move session/secure runtime to im-core | F5/F6 |
```

静态扫描：

```bash
rg 'AWIKI_USE_IM_CORE_MVP|use_im_core_mvp' crates/awiki-cli/src crates/awiki-cli/tests docs

rg 'legacy_|fallback.*legacy|run_.*_legacy'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter   crates/awiki-cli/src/runtime

rg 'crate::message::|use crate::message|message::SendRequest|InboxRequest|HistoryRequest|AttachmentDownloadRequest'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter   crates/awiki-cli/src/runtime

rg 'im_core::compat'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter   crates/awiki-cli/src/runtime

rg 'ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli'   crates/im-core/src   crates/im-core/tests
```

	F0 验收：

	```text
	[ ] baseline 文件存在
	[ ] allowlist 覆盖当前 offenders
	[ ] 新增 offender 会 fail
	[ ] Workstream A burn-down 目标明确
	[ ] Workstream B command-policy PR 不被错误要求删除 legacy allowlist
	```

---

### A1：附件 final cutover

目标：

```text
msg send --file
msg attachment download
```

全部通过 im-core public API：

```rust
client.attachments().send(target, request)
client.attachments().download(request)
```

禁止：

```text
legacy_attachment_send
legacy_attachment_download
crate::message::send
crate::message::download_attachment
im_core::compat::attachments::* in default CLI path
```

具体步骤：

1. 在 `crates/im-core/src/attachments` 中确认 public API 完整。
2. 将当前 `im_core::compat::attachments::send_attachment_with_details` 的差异字段移入 public result 或 internal projection。
3. CLI adapter `send_attachment_via_im_core` 改为调用 `client.attachments().send(...)`。
4. CLI adapter `download_attachment_via_im_core` 改为调用 `client.attachments().download(...)`。
5. 删除 `should_fallback_attachment_send` / `should_fallback_attachment_download`。
6. 删除 `legacy_attachment_send` / `legacy_attachment_download` / old request conversion。
7. 如果某个 attachment capability 仍不支持，返回 `UnsupportedCapability("attachments")`，不 fallback。

验收：

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_attachment_contract
cargo test -p awiki-cli --test m_core_cli_adapter_policy_contract

rg 'legacy_attachment|crate::message::send|download_attachment\('   crates/awiki-cli/src/app/msg_handlers.rs   crates/awiki-cli/src/im_core_adapter/messages.rs

rg 'im_core::compat::attachments'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter
```

---

### A2：message local_state projection cutover

目标：CLI 不再在 adapter 中手动写 message local store。

当前 CLI adapter 应删除这些职责：

```text
store::open
store::ensure_schema
store::store_message
store::make_thread_id
store::touch_group_after_message
manual MessageRecord construction
manual delivery metadata construction
manual peer DID history lookup for rendering
```

im-core 应负责：

```text
send result projection
owner_identity_id / owner_did isolation
direct/group thread id
message persistence
delivery state
operation id
attachment manifest persistence
conversation projection
mark-read state
peer handle/DID history
```

推荐 im-core API：

```rust
client.messages().send(request) -> SendMessageResult
client.messages().history(thread, query) -> Page<Message>
client.messages().inbox(query) -> Page<Message>
client.messages().mark_read(ids) -> MarkReadResult
client.local_state().message_projection_status(...) // optional diagnostics only
```

执行步骤：

1. 在 `im-core` 中补齐 `local_state` projection writer。
2. `MessageService::send` 内部完成 local projection。
3. `MessageService::history/inbox` 返回已带 enough metadata 的 DTO。
4. CLI adapter 只做 `SendMessageResult -> CLI JSON` render，不再写 store。
5. 删除 adapter 中 `persist_send_result` / `persist_group_send_result` / `persist_*_attachment_result`。
6. 删除 adapter 中 `peer_dids_for_handle_from_store` 等本地补数据函数。
7. 如果 CLI 输出历史兼容需要字段，优先让 im-core DTO 提供，而不是 CLI 查 store 补齐。

验收：

```bash
cargo test -p im-core messages local_state
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test message_contract

rg 'store::store_message|MessageRecord|touch_group_after_message|make_thread_id|list_dids_by_handle'   crates/awiki-cli/src/im_core_adapter/messages.rs
```

---

### A3：group local_state projection cutover

目标：群组 snapshot、members、messages 的 cache/projection 由 im-core 管理，CLI 不再读旧 store 补响应。

当前 CLI adapter 应删除：

```text
cached_group_snapshot
cached_group_members
cached_owner_identity_ids
enrich_cached_group_snapshot
store::get_group_snapshot_for_owner_identity
store::list_cached_group_members
sync_group_state as CLI post-processing
```

im-core 应负责：

```text
group create/join/update/member mutation 后刷新 projection
group get/list/members/messages 统一返回 SDK DTO
local cached fallback policy
owner identity isolation
group E2EE status metadata
```

推荐 im-core API：

```rust
client.groups().create(...)
client.groups().join(...)
client.groups().get(...)
client.groups().list(...)
client.groups().members(...)
client.groups().messages(...)
client.groups().update_profile(...)
client.groups().update_policy(...)
client.groups().add_member(...)
client.groups().remove_member(...)
client.groups().leave(...)
```

执行步骤：

1. 把 group remote result -> local projection 逻辑迁入 im-core `groups` / `local_state`。
2. `GroupReadResult` / `GroupMutationResult` DTO 补足 CLI 所需字段。
3. CLI adapter 删除本地 cache lookup。
4. CLI render 使用 im-core DTO 或 `serde_json::to_value`。
5. 如果需要兼容旧输出字段，做 renderer-level mapping，不做 store lookup。

验收：

```bash
cargo test -p im-core groups local_state
cargo test -p awiki-cli --test group_contract

rg 'cached_group_|get_group_snapshot_for_owner_identity|list_cached_group_members|sync_group_state'   crates/awiki-cli/src/im_core_adapter/groups.rs
```

---

### A4：identity recover / migration boundary cutover

身份能力分三类处理：

| 命令 | final 状态 |
| --- | --- |
| `id list/current/use/status/register/refresh-token/resolve/bind/profile get/profile set` | default im-core |
| `id recover` | default im-core，包括本地 finalize/merge |
| `id replace-did` | AdvancedUser 或 Diagnostic，若执行则 im-core high-level API，不执行旧流程 |
| `id create` | MigrationOnly 或 Internal diagnostic，不属于 default surface |
| `id import-v1` | MigrationOnly，CLI-owned，不属于 default surface |

`id recover` 必须重点处理。当前远端 recover 可能已走 im-core，但本地仍由 CLI 做：

```text
generate identity
save identity
backup
merge local state
finalize recovered handle
cleanup old identity indexes
e2ee state cleanup
```

final cutover 目标：

```rust
core.identities().recover_handle(request) -> RecoverHandleResult
core.identities().finalize_recovered_handle(plan/result) -> RecoverFinalizeResult
// 或 recover_handle() 内部完成全部本地 finalize
```

执行步骤：

1. 将 local recover plan / backup / save / merge / finalize 移到 im-core identity/local_state。
2. CLI 只负责 flags、dry-run render、danger warning、用户确认。
3. `store::merge_recovered_handle_local_state` 能力迁入 im-core local_state。
4. `identity::finalize_recovered_handle` 能力迁入 im-core identity。
5. `id create` 和 `id import-v1` 从 default surface 排除，并可迁到 diagnostic / migration namespace。

验收：

```bash
cargo test -p im-core identity recover local_state
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli --test id_recover_contract

rg 'generate_identity_with_path_segments|finalize_recovered_handle|merge_recovered_handle_local_state|identity::import_v1|identity::create_identity'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter
```

---

### A5：realtime session/auth/connect cutover

目标：`runtime listener run` / `runtime listener service-run` 作为 CLI 宿主存在，但 session/auth/connect/reconnect/heartbeat/RPC routing 由 im-core realtime runtime 执行。

CLI 仍负责：

```text
foreground/service-run process
service manager install/start/stop/restart/uninstall
pid/log/socket/status file
host notify sink
shutdown signal bridging
```

im-core 负责：

```text
auth/session refresh
websocket connect
heartbeat
reconnect
request/response routing
notification classify
ImEvent projection
run_until_shutdown
```

当前应迁出 CLI runtime 的典型旧依赖：

```text
authsdk::Session
message::auth_session
runtime::listener_wsclient endpoint planning
WsTransport direct ownership
legacy listener_session_loop decision functions
listener secure replay RPC planning
```

推荐 im-core API：

```rust
client.realtime().run_until_shutdown(options, shutdown)
client.realtime().connect(options)
client.realtime().status()
```

如果 CLI 仍需要 daemon socket bridge：

```text
CLI bridge = process/service host feature
im-core runner = IM realtime engine
bridge request -> im-core RealtimeControl / RequestRouter
```

执行步骤：

1. 在 im-core 增加 `RealtimeTransport` default implementation 或 connect factory。
2. 把 listener connect/auth refresh 从 `awiki-cli::runtime` 迁入 im-core。
3. 把 ping/reconnect/session-loop decision 迁入 im-core。
4. CLI runtime listener 只构造 client + options + shutdown + event sink。
5. CLI host notification 只消费 `ImEvent`，不解析 raw notification body。
6. 删除 `runtime/listener_supervisor_run.rs` 中对 `crate::message` / `authsdk` / `anpsdk` 的依赖。

验收：

```bash
cargo test -p im-core realtime
cargo test -p awiki-cli --test host_runtime_listener_foreground_contract
cargo test -p awiki-cli --test host_runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract

rg 'use crate::message|crate::message::|authsdk::Session|listener_session_loop|listener_notification_consume'   crates/awiki-cli/src/runtime/listener_supervisor_run.rs
```

---

### A6：secure runtime side effects cutover

目标：direct E2EE、group E2EE、secure outbox、incoming decrypt、local ACK、prekey retry、MLS notice processing 全部进入 im-core。

CLI default commands 不得再直接调用：

```text
message::maybe_publish_secure_prekeys
message::new_secure_e2ee_client_for_record
message::MessageServiceE2EEClient
message::flush_queued_secure_outbox_with_sender
message::current_secure_session_id
message::build_secure_ack_payload
anpsdk::DirectE2eeSession
FileSessionStore
MlsExecProvider / AWIKI_ANP_MLS_BINARY as default runtime
```

推荐 im-core public API：

```rust
client.messages().send(SendMessageRequest { security: E2eeRequired, .. })

client.secure().direct(peer).status()
client.secure().direct(peer).repair()
client.secure().group(group).status()
client.secure().group(group).repair()
```

本版本不提供 supported E2EE diagnostic CLI，也不要求 `im-core` 暴露 secure diagnostic facade。E2EE 诊断和恢复依赖以下高层产品能力：

```rust
client.secure().direct(peer).status()
client.secure().direct(peer).repair()
client.secure().group(group).status()
client.secure().group(group).repair()
```

如果后续版本需要 support-grade E2EE diagnostics，必须另起 plan，并遵守以下 future-only 约束：

```text
1. 不进 default prelude。
2. DTO 只能暴露抽象诊断结果，例如 SecureDiagnosticReport / SecureProblem / SecureOperationId。
3. operation_id 必须是 opaque id，不能是 SQLite row id。
4. 不暴露 raw DB row、raw KeyPackage、prekey payload、MLS notice body、provider binary path、ratchet counter、wire RPC method/params。
5. 旧 CLI 低层命令不得反向驱动 im-core public API 变成 wire/store/MLS helper 集合。
```

im-core internal responsibilities：

```text
direct session store
signed prekey / one-time prekey
prekey publish retry
secure outbox queue / failed / retry / drop / flush
incoming direct decrypt
local secure ACK
group MLS store
group E2EE status/repair
group E2EE incoming decrypt
MLS notice processing
secure realtime projection
```

CLI command policy：

```text
msg send --secure required   -> high-level im-core send E2eeRequired
msg send --secure on         -> deprecated alias for --secure required
msg secure status            -> high-level client.secure().direct(...).status()
msg secure repair            -> high-level repair
msg secure outbox list/retry/drop -> Unsupported in this version, or Hidden/Internal/TestOnly
group secure status/repair   -> high-level client.secure().group(...)
group secure diagnostics     -> Unsupported in this version
group e2ee status/repair     -> DeprecatedAlias to group secure status/repair
group e2ee low-level commands -> Hidden/Internal/TestOnly, not supported diagnostic contract
```

执行步骤：

1. 确认 `client.messages().send(... E2eeRequired ...)` 完成 direct/group secure send。
2. 把 prekey retry / secure outbox flush / local ACK 迁入 im-core runtime。
3. 把 incoming decrypt projection 迁入 im-core realtime/local_state。
4. 把 group MLS provider 改为 im-core native provider，不依赖 CLI exec provider。
5. CLI secure default commands 只调用 `client.secure()` 或 `client.messages().send()`。
6. 本版本不新增 supported E2EE diagnostic CLI；`msg secure outbox *` 和 `group secure diagnostics` 返回 unsupported 或走 Hidden/Internal/TestOnly gate。
7. `group.e2ee.*` 不再作为 default command surface；`status/repair` 只作为 deprecated alias；其他低层命令只能 Hidden/Internal/TestOnly，不能作为长期 supported diagnostic contract。

验收：

```bash
cargo test -p im-core secure
cargo test -p im-core realtime secure
cargo test -p awiki-cli --test msg_secure_contract
cargo test -p awiki-cli --test group_e2ee_contract

rg 'new_secure_e2ee_client_for_record|MessageServiceE2EEClient|flush_queued_secure_outbox|maybe_publish_secure_prekeys|DirectE2eeSession|FileSessionStore|AWIKI_ANP_MLS_BINARY'   crates/awiki-cli/src
```

---

### A7：compat 与旧模块清理

目标：旧模块可以短期留作 historical tests / migration-only，但 default path 不可达。

处理原则：

```text
1. im_core::compat 不进入 CLI default execution path。
2. crates/awiki-cli/src/message 可保留为 migration-only 或逐步删除，但 app/runtime/im_core_adapter 不引用。
3. crates/awiki-cli/src/mail 若只为历史 parity，可 dead_code 保留到指定删除 PR。
4. content/site 旧模块不进入 default surface。
5. debug/migration-only 必须 hidden、diagnostic-only 或 migration-only。
6. allowlist burn-down 到空。
```

建议清理顺序：

```text
1. 删除 app/im_core_adapter 中旧 fallback 函数。
2. 删除 default dispatch 到 blocked old handlers。
3. 删除或隐藏 schema 中的 removed commands。
4. 将 diagnostic-only 命令从 default_surface_specs 过滤。
5. 把旧模块标注 deletion TODO，随后按测试覆盖删除。
```

最终静态检查：

```bash
rg 'crate::message::|use crate::message|message::SendRequest|message::InboxRequest|message::HistoryRequest'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter   crates/awiki-cli/src/runtime

rg 'im_core::compat'   crates/awiki-cli/src/app   crates/awiki-cli/src/im_core_adapter   crates/awiki-cli/src/runtime

rg 'crate::content|crate::site'   crates/awiki-cli/src/app   crates/awiki-cli/src/cli

rg 'AWIKI_USE_IM_CORE_MVP|fallback legacy|legacy path'   crates/awiki-cli/src   crates/awiki-cli/tests   docs/sdk-refactor
```

---

## 9. Workstream B：CLI 命令面 Review 与重构

### B0：CLI 命令设计原则

default CLI commands 只保留高层产品任务：

```text
id register
id recover
id profile get/set
people follow
people contacts save
msg send
msg inbox
msg history
msg mark-read
group create
group join
group members
mail inbox
runtime listener enable
runtime host-notify setup
```

default CLI 不展示底层实现细节：

```text
raw RPC
raw SQL
wire params
KeyPackage
prekey
MLS provider binary
secure outbox row
local SQLite table
WebSocket frame
provider token / secret / route internals
daemon service-run internal entry
runtime service install/start/stop/restart operator controls
```

判断规则：

```text
用户是否能用这个命令完成一个产品任务？
命令是否要求用户理解内部 wire/store/E2EE/runtime 细节？
是否有对应 im-core 高级 public API？
如果没有高级 public API，是否应该 hidden / internal / diagnostic facade / unsupported？
```

---

### B1：推荐 default command surface

普通用户 default surface 只展示：

```text
awiki-cli status
awiki-cli doctor
awiki-cli init
awiki-cli config show

awiki-cli id list
awiki-cli id current
awiki-cli id use
awiki-cli id status
awiki-cli id register
awiki-cli id recover
awiki-cli id refresh-token
awiki-cli id resolve
awiki-cli id bind
awiki-cli id profile get
awiki-cli id profile set

awiki-cli people follow
awiki-cli people unfollow
awiki-cli people status
awiki-cli people followers
awiki-cli people following
awiki-cli people contacts list
awiki-cli people contacts save

awiki-cli msg send
awiki-cli msg inbox
awiki-cli msg history
awiki-cli msg mark-read
awiki-cli msg attachment download
awiki-cli msg send --secure required
awiki-cli msg secure status
awiki-cli msg secure repair

awiki-cli group create
awiki-cli group create --secure required
awiki-cli group get
awiki-cli group join
awiki-cli group leave
awiki-cli group add
awiki-cli group remove
awiki-cli group update
awiki-cli group list
awiki-cli group members
awiki-cli group messages
awiki-cli group secure status
awiki-cli group secure repair

awiki-cli mail account
awiki-cli mail inbox
awiki-cli mail read
awiki-cli mail mark-read
awiki-cli mail send
awiki-cli mail attachment download
awiki-cli mail notify

awiki-cli runtime status
awiki-cli runtime listener status
awiki-cli runtime listener enable
awiki-cli runtime listener disable
awiki-cli runtime host-notify status
awiki-cli runtime host-notify setup
awiki-cli runtime host-notify enable
awiki-cli runtime host-notify disable

awiki-cli docs
awiki-cli schema
awiki-cli completion ...
awiki-cli version
awiki-cli upgrade
```

说明：

```text
1. runtime listener install/start/stop/restart/uninstall/config set 是 Operator surface，不是 default surface。
2. runtime mode set 是 Advanced/Operator surface，不是 default surface。
3. runtime host-notify provider-specific commands 是 Advanced/Operator surface，不是 default surface。
4. runtime listener run/service-run 是 InternalService surface。
5. group e2ee status/repair 是 DeprecatedAlias，其他 group e2ee low-level commands 是 Hidden/Internal/TestOnly。
6. 本版本不提供 supported E2EE diagnostic CLI；msg secure outbox * / group secure diagnostics 不进入 supported surface。
7. page/site 不属于当前 im-core default surface。
```

---

### B2：Advanced / Operator / Diagnostic surfaces

AdvancedUser surface：

```text
awiki-cli config set
awiki-cli runtime mode get
awiki-cli runtime mode set
awiki-cli runtime listener config show
awiki-cli runtime listener config set
awiki-cli runtime host-notify config show
awiki-cli runtime host-notify config set
```

Operator surface：

```text
awiki-cli runtime setup
awiki-cli runtime apply
awiki-cli runtime listener install
awiki-cli runtime listener start
awiki-cli runtime listener stop
awiki-cli runtime listener restart
awiki-cli runtime listener uninstall
awiki-cli runtime host-notify hermes guide
awiki-cli runtime host-notify hermes status
awiki-cli runtime host-notify hermes setup
awiki-cli runtime host-notify openclaw set
awiki-cli runtime host-notify openclaw set-token
awiki-cli runtime host-notify openclaw clear-token
awiki-cli runtime host-notify openclaw route add
awiki-cli runtime host-notify openclaw route list
awiki-cli runtime host-notify openclaw route remove
```

Supported Diagnostic surface：

```text
awiki-cli debug db handle-history
```

Unsupported or Hidden / Internal / TestOnly E2EE commands：

```text
awiki-cli msg secure outbox list
awiki-cli msg secure outbox retry
awiki-cli msg secure outbox drop
awiki-cli group secure diagnostics
awiki-cli group secure repair --explain
awiki-cli group e2ee publish-key-package
awiki-cli group e2ee pending
awiki-cli group e2ee process-leave-request
awiki-cli group e2ee recover-member
awiki-cli group e2ee update-key
awiki-cli group e2ee rejoin
```

这些 E2EE 命令在本版本不是 supported diagnostic contract。它们可以返回 stable unsupported，或短期保留给内部测试 / migration-only 排障，但不进入 schema/help/completion，不要求 im-core 提供同名 public API，也不要求 im-core 暴露 diagnostic facade。

Raw SQL 不属于普通 DiagnosticOnly：

```text
awiki-cli debug db query
  -> StableUnsupported("raw-sql") by default
  -> 只有同时满足 AWIKI_CLI_ENABLE_RAW_SQL=1 和 --diagnostic 时才允许执行
  -> 不进入 default surface，不进入普通 diagnostic completion
```

MigrationOnly surface：

```text
awiki-cli debug identity import-v1
awiki-cli debug db import-v1
awiki-cli debug identity create-local
```

InternalService surface：

```text
awiki-cli runtime listener run
awiki-cli runtime listener service-run
awiki-cli runtime host-notify hermes bridge service-run
```

Removed surface：

```text
awiki-cli debug raw rpc
awiki-cli group code *
```

---

### B3：Identity 命令 Review

| 命令 | 建议 | audience | primary owner | secondary owners | CLI shell role | direct policy |
| --- | --- | --- | --- | --- | --- | --- |
| `id list` | default 保留 | DefaultUser | ImCoreIdentity | [] | None | Allow |
| `id current` | default 保留 | DefaultUser | ImCoreIdentity | [] | None | Allow |
| `id use` | default 保留 | DefaultUser | ImCoreIdentity | [] | WritesDefaultIdentityFile | Allow |
| `id status` | default 保留 | DefaultUser | ImCoreIdentity | [ImCoreAuth] | None | Allow |
| `id register` | default 保留 | DefaultUser | ImCoreIdentity | [] | RendersDryRunPlan | Allow |
| `id recover` | default 保留；必须完整进入 im-core，本地 finalize/merge 不得降级为 migration-only | DefaultUser | ImCoreIdentity | [] | RendersDryRunPlan | Allow |
| `id refresh-token` | default 保留 | DefaultUser | ImCoreAuth | [] | None | Allow |
| `id resolve` | default 保留 | DefaultUser | ImCoreDirectory | [] | None | Allow |
| `id bind` | default 保留 | DefaultUser | ImCoreIdentity | [ImCoreDirectory] | None | Allow |
| `id profile get/set` | default 保留 | DefaultUser | ImCoreIdentity | [] | None | Allow |
| `id replace-did` | advanced/diagnostic | AdvancedUser | ImCoreIdentity | [] | RendersDryRunPlan | RequireDiagnosticGate |
| `id create` | 移出 default | MigrationOnly | CliMigration | [] | RendersDryRunPlan | RequireMigrationGate |
| `id import-v1` | 改名迁移 | MigrationOnly | CliMigration | [] | RendersDryRunPlan | RequireMigrationGate |

可选重构：

```text
id replace-did -> id did replace
id import-v1   -> debug identity import-v1
id create      -> debug identity create-local
```

---

### B4：Message 命令 Review

| 命令 | 建议 | audience | primary owner | secondary owners | CLI shell role | direct policy |
| --- | --- | --- | --- | --- | --- | --- |
| `msg send --to --text` | default 保留 | DefaultUser | ImCoreMessages | [] | ReadsUserInputFile | Allow |
| `msg send --group --text` | default 保留 | DefaultUser | ImCoreMessages | [] | ReadsUserInputFile | Allow |
| `msg send --file` | default 保留，Phase 4 完成后 | DefaultUser | ImCoreAttachments | [ImCoreMessages] | ReadsUserInputFile | Allow |
| `msg send --secure required` | default 保留，Phase 6 完成后 | DefaultUser | ImCoreMessages | [ImCoreSecure] | ReadsUserInputFile | Allow |
| `msg send --secure on` | deprecated alias | DefaultUser | ImCoreMessages | [ImCoreSecure] | ReadsUserInputFile | DeprecatedAlias to `--secure required` |
| `msg inbox` | default 保留 | DefaultUser | ImCoreMessages | [] | None | Allow |
| `msg history` | default 保留 | DefaultUser | ImCoreMessages | [] | None | Allow |
| `msg mark-read` | default 保留 | DefaultUser | ImCoreMessages | [] | None | Allow |
| `msg attachment download` | default 保留 | DefaultUser | ImCoreAttachments | [ImCoreMessages] | WritesUserOutputFile | Allow |
| `msg secure status` | default 保留 | DefaultUser | ImCoreSecure | [] | None | Allow |
| `msg secure repair` | default 保留 | DefaultUser | ImCoreSecure | [] | RendersDryRunPlan | AllowWithWarning |
| `msg secure failed/retry/drop` | 旧 detail 命令，不进 default；本版本不支持 supported diagnostic | Diagnostic | ImCoreSecure | [] | None | StableUnsupported 或 RequireInternalServiceGate |
| `msg secure outbox list/retry/drop` | 本版本不支持 supported diagnostic；可 hidden/internal/test-only | InternalService | ImCoreSecure | [] | None | StableUnsupported 或 RequireInternalServiceGate |

本版本不新增 supported `msg secure outbox *`。secure outbox retry/flush/drop 由 im-core runtime 和 repair flow 内部处理。

---

### B5：Group 命令 Review

| 命令 | 建议 | audience | primary owner | secondary owners | CLI shell role | direct policy |
| --- | --- | --- | --- | --- | --- | --- |
| `group create/get/join/leave/list/members/messages` | default 保留 | DefaultUser | ImCoreGroups | [] | RendersDryRunPlan | Allow |
| `group add/remove/update` | default 保留；属于群管理员产品任务，不是底层实现细节 | DefaultUser | ImCoreGroups | [] | RendersDryRunPlan | Allow |
| `group create --secure required` | Phase 6 后 default；产品化 secure flag | DefaultUser | ImCoreGroups | [ImCoreSecure] | RendersDryRunPlan | AllowWithWarning |
| `group create --message-security-profile group-e2ee` | advanced/deprecated alias | AdvancedUser | ImCoreGroups | [ImCoreSecure] | RendersDryRunPlan | DeprecatedAlias to `--secure required` |
| `group add/remove/leave --secure required` | Phase 6 后 default；secure orchestration 由 im-core 内部完成 | DefaultUser | ImCoreGroups | [ImCoreSecure] | RendersDryRunPlan | AllowWithWarning |
| `group add/remove/leave --e2ee` | deprecated alias | AdvancedUser | ImCoreGroups | [ImCoreSecure] | RendersDryRunPlan | DeprecatedAlias to `--secure required` |
| `group secure status` | default 保留 | DefaultUser | ImCoreSecure | [] | None | Allow |
| `group secure repair` | default 保留 | DefaultUser | ImCoreSecure | [] | RendersDryRunPlan | AllowWithWarning |
| `group secure diagnostics` | 本版本不支持；用 `group secure status` / `group secure repair` 覆盖高层诊断和恢复 | Diagnostic | ImCoreSecure | [] | None | StableUnsupported |
| `group e2ee status/repair` | 旧 alias | Diagnostic | ImCoreSecure | [] | None | DeprecatedAlias to `group secure *` |
| `group e2ee publish-key-package` | Hidden/Internal/TestOnly；不承诺 supported diagnostic | InternalService | ImCoreSecure | [] | None | RequireInternalServiceGate |
| `group e2ee pending/process-leave-request/recover-member/update-key/rejoin` | Hidden/Internal/TestOnly；不承诺 supported diagnostic | InternalService | ImCoreSecure | [] | None | RequireInternalServiceGate |
| `group code *` | removed/hidden | InternalService | ExternalUnsupported | [] | None | Removed |

推荐：

```text
默认用户只看到 `group create --secure required`、`group secure status`、`group secure repair` 这类产品命令。
MLS / KeyPackage / notice / pending / provider 细节不进入 default，也不作为 supported diagnostic contract。
```

---

### B6：Runtime / host notify 命令 Review

DefaultUser：

```text
runtime status
runtime listener status
runtime listener enable
runtime listener disable
runtime host-notify status
runtime host-notify setup
runtime host-notify enable
runtime host-notify disable
```

AdvancedUser：

```text
runtime mode get
runtime mode set
runtime listener config show
runtime listener config set
runtime host-notify config show
runtime host-notify config set
```

Operator：

```text
runtime setup
runtime apply
runtime listener install
runtime listener start
runtime listener stop
runtime listener restart
runtime listener uninstall
runtime host-notify hermes guide
runtime host-notify hermes status
runtime host-notify hermes setup
runtime host-notify openclaw set
runtime host-notify openclaw set-token
runtime host-notify openclaw clear-token
runtime host-notify openclaw route add/list/remove
```

InternalService：

```text
runtime listener run
runtime listener service-run
runtime host-notify hermes bridge service-run
```

	新增高层命令建议：

	```text
	runtime host-notify status
	runtime host-notify setup --provider hermes
	runtime host-notify setup --provider openclaw
	runtime host-notify target add/list/remove
	```

	这些高层命令必须在隐藏 provider-specific internals 之前先落地：

	```text
	1. `runtime host-notify status` 聚合当前 sink/provider/route/secret readiness，不输出 secret/token。
	2. `runtime host-notify setup --provider <provider>` 包装 hermes/openclaw 初始化流程。
	3. `runtime host-notify target add/list/remove` 包装 provider route 管理，输出 provider-neutral target DTO。
	4. 旧 `runtime host-notify hermes *` / `openclaw *` 再移到 Operator/Advanced surface。
	```

逐步隐藏 provider-specific internals：

```text
runtime host-notify openclaw set-token
runtime host-notify openclaw route add/list/remove
runtime host-notify hermes set-secret
```

---

### B7：Mail / People 命令 Review

Mail 已作为独立 Email 阶段进入 im-core，default 保留：

```text
mail account
mail inbox
mail read
mail mark-read
mail send
mail attachment download
mail notify
```

People default 保留：

```text
people follow
people unfollow
people status
people followers
people following
people contacts list
people contacts save
```

`people search`：

```text
如果 im-core 没有高级 search API，则继续 StableUnsupported。
不要临时回退旧 directory/search stub。
```

---

### B8：Page / Site / Debug 命令 Review

Page / Site：

```text
page.*
site.*
```

当前不属于 IM core default API。处理策略：

```text
1. 如果未来要做 content-core/site-core，另起 interface 和 migration plan。
2. 在当前 im-core final cutover 中保持 StableUnsupported 或 Hidden。
3. 不允许 default dispatch 进入 crate::content / crate::site。
```

	Debug：

	```text
	debug.db.handle-history -> DiagnosticOnly
	debug.db.import-v1      -> MigrationOnly
	debug.db.query          -> StableUnsupported raw-sql by default；只有 AWIKI_CLI_ENABLE_RAW_SQL=1 + --diagnostic 可执行
	debug.raw.rpc           -> Removed
	debug.schema-cache/logs -> Hidden until implemented
	```

---

## 10. Rename / deprecation / alias 策略

### 10.1 Alias 表达方式

alias 不再使用单独的 `AliasPolicy`。所有 alias 行为必须落在 `CommandSpec` + `DirectInvocationPolicy` 上，避免 parser、schema、completion、dispatch 出现两套真相。

```rust
CommandSpec {
    name: "group.e2ee.status",
    canonical_name: "group.secure.status",
    audience: CommandAudience::Diagnostic,
    primary_owner: CommandOwner::ImCoreSecure,
    secondary_owners: &[],
    cli_shell_role: CliShellRole::None,
    direct_invocation: DirectInvocationPolicy::DeprecatedAlias {
        replacement: "group secure status",
        until: "2026-08-31",
    },
    // existing metadata...
}
```

行为：

| DirectInvocationPolicy | 执行 | default schema | `schema --all` | completion | exit |
| --- | --- | --- | --- | --- | --- |
| DeprecatedAlias | 转发到 replacement，输出 warning | 不展示旧名 | 展示旧名且 deprecated=true | 不补全旧名，除非 `--audience all` | replacement exit |
| RequireDiagnosticGate | 需要 diagnostic gate，可用 `canonical_name` 指向新诊断命令 | 不展示 | 展示 | 只在 diagnostic completion | 无 gate 时 exit 2 |
| StableUnsupported | 不执行，给 replacement/canonical hint | 不展示 | 可展示 unsupported info | 不补全 | exit 2 |
| Removed | 不执行，给 replacement/canonical hint | 不展示 | 可展示 removed info | 不补全 | exit 2 |

如果 alias 转发到 diagnostic 或 migration target，target 的 gate 仍然必须执行。本版本没有 supported E2EE diagnostic target，因此 `msg secure failed/retry/drop` 不转发到 supported outbox command，而是返回 stable unsupported 或走 internal/test-only gate。

### 10.2 建议 alias

```text
group.e2ee.status  -> group secure status      DeprecatedAlias
group.e2ee.repair  -> group secure repair      DeprecatedAlias
msg.secure.failed  -> unsupported or internal/test-only
msg.secure.retry   -> unsupported or internal/test-only
msg.secure.drop    -> unsupported or internal/test-only
msg send --secure on -> msg send --secure required DeprecatedAlias
group create --message-security-profile group-e2ee -> group create --secure required DeprecatedAlias
group add/remove/leave --e2ee -> group add/remove/leave --secure required DeprecatedAlias
id.import-v1       -> debug identity import-v1 MigrationOnly alias
id.create          -> debug identity create-local MigrationOnly alias
```

低层 group e2ee commands：

```text
group.e2ee.publish-key-package
group.e2ee.pending
group.e2ee.process-leave-request
group.e2ee.recover-member
group.e2ee.update-key
group.e2ee.rejoin
```

处理为：

```text
RequireInternalServiceGate 或 Hidden/TestOnly。
不进入 default surface。
不进入普通 diagnostic surface。
不作为 im-core public API 或 supported diagnostic facade 的设计依据。
```

---

## 11. 推荐 PR 拆分

### PR F0：final cutover baseline and allowlist gates

目标：

```text
新增 final boundary tests 和静态扫描脚本。
建立 allowlist baseline。
不要求第一步清零。
不改业务行为。
```

改动：

```text
docs/sdk-refactor/legacy-path-baseline.md
crates/awiki-cli/tests/cli_shell_boundary_contract.rs
crates/awiki-cli/tests/legacy_path_cutover_contract.rs
crates/awiki-cli/tests/cli_command_surface_contract.rs
scripts/sdk-refactor/final-cutover-check.sh
```

验收：

```bash
cargo test -p awiki-cli --test cli_shell_boundary_contract
cargo test -p awiki-cli --test legacy_path_cutover_contract
```

---

### PR F1：attachment no-fallback cutover

目标：

```text
删除 msg attachment old message fallback。
```

改动：

```text
crates/awiki-cli/src/app/msg_handlers.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
crates/im-core/src/attachments/*
```

验收：

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_attachment_contract
```

---

### PR F2：message projection into im-core

目标：

```text
send/inbox/history/mark-read projection 由 im-core local_state 管理。
```

改动：

```text
crates/im-core/src/local_state/*
crates/im-core/src/messages/*
crates/awiki-cli/src/im_core_adapter/messages.rs
```

验收：

```bash
cargo test -p im-core messages local_state
cargo test -p awiki-cli --test msg_contract
```

---

### PR F3：group projection into im-core

目标：

```text
group snapshot/members/messages cache 由 im-core 管理。
```

改动：

```text
crates/im-core/src/groups/*
crates/im-core/src/local_state/*
crates/awiki-cli/src/im_core_adapter/groups.rs
```

验收：

```bash
cargo test -p im-core groups local_state
cargo test -p awiki-cli --test group_contract
```

---

### PR F4：identity recover full im-core cutover

目标：

```text
id recover 远端 + 本地 finalize/merge 全部进入 im-core。
id create/import-v1 移出 default surface。
```

改动：

```text
crates/im-core/src/identity/*
crates/im-core/src/local_state/*
crates/awiki-cli/src/im_core_adapter/identity.rs
crates/awiki-cli/src/app/id_recover_handlers.rs
crates/awiki-cli/src/cmdmeta/mod.rs
```

验收：

```bash
cargo test -p im-core identity recover
cargo test -p awiki-cli --test identity_im_core_mvp_contract
```

---

### PR F5：realtime session/auth/connect im-core cutover

目标：

```text
runtime listener host 不再使用 awiki-cli message/authsdk session bootstrap。
```

改动：

```text
crates/im-core/src/realtime/*
crates/im-core/src/auth/*
crates/awiki-cli/src/runtime/listener_supervisor_run.rs
crates/awiki-cli/src/im_core_adapter/realtime.rs
```

验收：

```bash
cargo test -p im-core realtime
cargo test -p awiki-cli --test host_runtime_listener_foreground_contract
```

---

### PR F6：secure runtime side effects im-core cutover

目标：

```text
secure direct/group E2EE runtime side effects 全部进入 im-core。
```

改动：

```text
crates/im-core/src/secure/*
crates/im-core/src/realtime/*
crates/im-core/src/local_state/*
crates/awiki-cli/src/runtime/listener_supervisor_run.rs
crates/awiki-cli/src/app/msg_handlers.rs
crates/awiki-cli/src/app/group_e2ee_handlers.rs
```

验收：

```bash
cargo test -p im-core secure
cargo test -p im-core realtime secure
cargo test -p awiki-cli --test msg_secure_contract
cargo test -p awiki-cli --test group_e2ee_contract
```

---

### PR F7：im_core_adapter pure boundary cleanup

目标：

```text
im_core_adapter 只保留 config/path/flag/error/render/unsupported 边界职责。
```

删除：

```text
DTO -> old request 转换
manual local store projection
im_core::compat default calls
legacy error mapping from old MessageError where no longer needed
```

验收：

```bash
cargo test -p awiki-cli --test m_core_cli_adapter_policy_contract

rg 'im_core::compat|crate::message::|message::|identity::register\(|identity::refresh_token\('   crates/awiki-cli/src/im_core_adapter
```

---

### PR F8：mandatory command policy fields

目标：

```text
所有命令都有 CommandAudience / primary_owner / secondary_owners / CliShellRole / DirectInvocationPolicy。
```

改动：

```text
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/tests/cli_command_surface_contract.rs
```

验收：

```bash
cargo test -p awiki-cli --test cli_command_surface_contract
```

---

### PR F9：runtime host-notify high-level facade

目标：

```text
先实现 runtime host-notify status/setup/target 高层产品命令，再隐藏 provider-specific internals。
```

改动：

```text
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/src/cli/mod.rs
crates/awiki-cli/src/app/runtime_handlers.rs
crates/awiki-cli/tests/host_runtime_notify_contract.rs
crates/awiki-cli/tests/host_runtime_hermes_cli_contract.rs
crates/awiki-cli/tests/host_runtime_openclaw_cli_contract.rs
```

验收：

```bash
cargo test -p awiki-cli --test host_runtime_notify_contract
cargo test -p awiki-cli --test cli_command_surface_contract

awiki-cli runtime host-notify status
awiki-cli runtime host-notify setup --provider hermes
awiki-cli runtime host-notify setup --provider openclaw
awiki-cli runtime host-notify target list
```

---

### PR F10：unified dispatch policy enforcement

目标：

```text
dispatch 前统一执行 enforce_command_policy。
删除 ad hoc default_cutover_boundary_error blocked-domain-only 模式。
```

改动：

```text
crates/awiki-cli/src/cli/mod.rs
crates/awiki-cli/src/app/unsupported.rs
crates/awiki-cli/tests/cli_direct_invocation_policy_contract.rs
```

验收：

```bash
cargo test -p awiki-cli --test cli_direct_invocation_policy_contract
```

---

### PR F11：default command surface shrink

目标：

```text
default help/schema/completion 只展示 DefaultUser high-level commands。
runtime service manager/provider internals 移入 operator/advanced/diagnostic/internal surfaces。
```

改动：

```text
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/src/docs/*
crates/awiki-cli/tests/cli_schema_contract.rs
```

验收：

```bash
cargo test -p awiki-cli --test cli_schema_contract
awiki-cli schema
awiki-cli schema --audience operator
awiki-cli schema --all
```

---

### PR F12：command rename / deprecated aliases

目标：

```text
group secure / debug identity migration names 落地；E2EE diagnostic/outbox 命令在本版本保持 unsupported 或 internal/test-only。
旧 detail 命令变成 deprecated alias、internal/test-only command 或 unsupported alias。
```

建议处理：

```text
group.e2ee.status              -> group secure status
group.e2ee.repair              -> group secure repair
msg.secure.failed/retry/drop   -> unsupported or internal/test-only
id.import-v1                   -> debug identity import-v1
id.create                      -> debug identity create-local migration-only
runtime host-notify openclaw * -> operator/advanced
debug.raw.rpc                  -> removed
page/site                      -> unsupported/hidden
```

验收：

```bash
cargo test -p awiki-cli --test cli_schema_contract
cargo test -p awiki-cli --test cli_parser_unknown_global_flags
cargo test -p awiki-cli --test cli_direct_invocation_policy_contract
```

---

### PR F13：legacy module retirement

目标：

```text
旧业务模块不在 default path 可达。
能删则删，不能删则标注 migration-only/diagnostic-only。
allowlist 清零。
```

候选清理：

```text
crates/awiki-cli/src/message/*
crates/awiki-cli/src/mail.rs
crates/awiki-cli/src/content/*
crates/awiki-cli/src/site/*
crates/awiki-cli/src/anpsdk/*
crates/awiki-cli/src/authsdk/*
runtime/listener_* legacy helpers
```

不要一次性删除所有文件。按测试覆盖和依赖情况拆 PR。

验收：

```bash
cargo test -p im-core
cargo test -p awiki-cli
./scripts/sdk-refactor/final-cutover-check.sh
```

---

## 12. 新增测试建议

### 12.1 `cli_shell_boundary_contract.rs`

覆盖：

```text
app handlers 不引用 old business modules
im_core_adapter 不引用 old business modules
runtime listener 不引用 old message/auth/e2ee implementation
default command dispatch 不进入 page/site/debug raw old handlers
```

### 12.2 `cli_command_surface_contract.rs`

覆盖：

```text
每个 CommandSpec 有 CommandAudience / primary_owner / secondary_owners / CliShellRole / DirectInvocationPolicy
default_surface_specs 只包含 DefaultUser high-level commands
Advanced / Operator / Diagnostic / MigrationOnly / InternalService 不进入 default schema
completion 不补全 non-default commands
schema --all 可以展示全部命令和 audience/policy
```

### 12.3 `cli_direct_invocation_policy_contract.rs`

覆盖：

```text
Unsupported 永远 stable unsupported
Removed 永远 removed_command
Hidden/InternalService 无 internal gate 不执行
DiagnosticOnly 无 diagnostic gate 不执行
MigrationOnly 无 migration gate 不执行
DeprecatedAlias 有 warning 或 replacement hint
Operator commands 可 direct invoke，但不在 default surface
group e2ee low-level commands 不作为 supported diagnostic；无 internal/test gate 不执行
diagnostic alias 转发后仍执行目标 gate
E2EE diagnostic commands are not supported in this version:
  msg secure outbox *
  group secure diagnostics
  group secure repair --explain
```

### 12.4 `legacy_path_cutover_contract.rs`

覆盖：

```text
msg attachment 不 fallback
secure command 不 fallback
runtime listener run 不 fallback
id recover 不调用 old finalize
no AWIKI_USE_IM_CORE_MVP
no im_core::compat default path
allowlist 最终为空
```

### 12.5 `im-core boundary tests`

覆盖：

```text
im-core 不引用 awiki-cli
im-core public API 不暴露 ParsedCommand / ExitError / config::Resolved / raw RPC / SQL
im-core public API 不暴露 raw KeyPackage / prekey payload / MLS notice body / provider binary / ratchet counter / raw outbox row
本版本不要求 im-core 暴露 secure diagnostic facade
prelude 只 re-export high-level API
compat 不进 prelude
```

---

## 13. Smoke commands

本计划完成后，至少手工跑一轮 default surface：

```bash
awiki-cli id list
awiki-cli id current
awiki-cli id status
awiki-cli id refresh-token

awiki-cli people contacts list
awiki-cli people follow <handle-or-did>
awiki-cli people status <handle-or-did>

awiki-cli msg send --to <peer> --text "hello"
awiki-cli msg send --group <group_did> --text "hello group"
awiki-cli msg send --to <peer> --file ./fixture.txt --text "caption"
awiki-cli msg inbox --limit 5
awiki-cli msg history --with <peer> --limit 5
awiki-cli msg history --group <group_did> --limit 5
awiki-cli msg mark-read <message_id>
awiki-cli msg attachment download --with <peer> --message-id <message_id> --output ./out.bin

awiki-cli msg send --to <peer> --text "secure hello" --secure required
awiki-cli msg secure status --with <peer>
awiki-cli msg secure repair --with <peer>

awiki-cli group create --name "test"
awiki-cli group create --name "secure-test" --secure required
awiki-cli group list
awiki-cli group get --group <group_did>
awiki-cli group add --group <group_did> --member <peer>
awiki-cli group remove --group <group_did> --member <peer>
awiki-cli group update --group <group_did> --name "updated"
awiki-cli group members --group <group_did>
awiki-cli group messages --group <group_did>
awiki-cli group secure status --group <group_did>
awiki-cli group secure repair --group <group_did>

awiki-cli mail account
awiki-cli mail inbox --limit 5
awiki-cli mail send --to <email> --subject "hello" --body "hi"

awiki-cli runtime status
awiki-cli runtime listener status
awiki-cli runtime listener enable
awiki-cli runtime listener disable
awiki-cli runtime host-notify status
awiki-cli runtime host-notify setup --provider hermes
awiki-cli runtime host-notify enable
awiki-cli runtime host-notify disable

awiki-cli schema
awiki-cli docs
```

Non-default surface smoke：

```bash
awiki-cli schema --all
awiki-cli schema --audience operator
awiki-cli schema --audience diagnostic

awiki-cli runtime listener install
awiki-cli runtime listener start
awiki-cli runtime listener stop
awiki-cli runtime mode set websocket

awiki-cli --diagnostic debug db handle-history <handle>
awiki-cli --migration debug identity import-v1
AWIKI_CLI_ENABLE_RAW_SQL=1 awiki-cli --diagnostic debug db query "SELECT 1"
```

Blocked / internal smoke：

```bash
awiki-cli page list
awiki-cli site page list --domain example.com
awiki-cli debug raw rpc
awiki-cli group code create
awiki-cli runtime listener service-run
awiki-cli debug db query "SELECT 1"
awiki-cli msg secure outbox list
awiki-cli group secure diagnostics --group <group_did>
awiki-cli group secure repair --group <group_did> --explain
awiki-cli group e2ee publish-key-package --group <group_did>
```

期望：

```text
page/site/raw rpc/group code -> unsupported or removed
service-run -> internal_command unless internal gate exists
debug db query -> stable unsupported unless AWIKI_CLI_ENABLE_RAW_SQL=1 and --diagnostic are both present
msg secure outbox * -> unsupported or internal/test-only gate
group secure diagnostics / repair --explain -> unsupported in this version
group e2ee low-level -> internal/test-only gate or hidden, not supported diagnostic
```

---

## 14. 最终验收清单

```text
[ ] cargo test -p im-core
[ ] cargo test -p awiki-cli
[ ] cargo test --workspace，或记录已知历史失败
[ ] im-core 不依赖 awiki-cli
[ ] awiki-cli default app handlers 不调用 crate::message business flow
[ ] awiki-cli default app handlers 不调用 crate::content / crate::site
[ ] im_core_adapter 不调用 im_core::compat 作为 default execution path
[ ] attachment send/download 无 legacy fallback
[ ] message send local projection 不由 CLI adapter 手写 store
[ ] group cache/projection 不由 CLI adapter 读取旧 store 补齐
[ ] id recover 本地 finalize/merge 已进入 im-core；未完成时返回 UnsupportedCapability，不降级为 migration-only
[ ] runtime listener session/auth/connect 已进入 im-core
[ ] secure prekey/outbox/incoming decrypt/local ACK/group MLS side effects 已进入 im-core
[ ] CommandSpec 有 CommandAudience / primary_owner / secondary_owners / CliShellRole / DirectInvocationPolicy
[ ] dispatch 前统一 enforce command policy
[ ] default schema/help/completion 只展示 DefaultUser high-level commands
[ ] runtime install/start/stop/restart/uninstall 不在 default surface
[ ] runtime provider token/secret/route 不在 default surface
[ ] runtime listener run/service-run 需要 InternalService gate
[ ] msg secure failed/retry/drop 不在 default surface
[ ] msg secure outbox * 不作为 supported CLI；返回 unsupported 或需要 internal/test-only gate
[ ] group e2ee low-level commands 不在 default surface，不是 supported diagnostic contract
[ ] group secure diagnostics / group secure repair --explain 不作为本版本 supported CLI
[ ] im-core public API 不暴露 raw KeyPackage / prekey / MLS notice / provider binary / ratchet counter / raw outbox row
[ ] 本版本不要求 im-core 暴露 secure diagnostic facade
[ ] page/site 不在 default surface 或稳定 unsupported
[ ] debug raw/sql 不在 default surface
[ ] direct invocation of unsupported/removed/diagnostic/internal command has stable envelope
[ ] 所有 remaining legacy modules 有 migration-only / diagnostic-only / deletion TODO
[ ] legacy-path allowlist 清零或只剩明确不属于 default path 的 migration-only 项
```

---

## 15. 一句话执行原则

**把 `awiki-cli` 变成产品命令壳：CLI 只表达用户意图并渲染结果；所有 IM 业务实现、状态投影、runtime、secure、attachments、email、group 和 local_state 都进入 `im-core`；默认命令面只展示高层产品任务，内部旋钮必须进入 advanced/operator/diagnostic/internal surface。**
