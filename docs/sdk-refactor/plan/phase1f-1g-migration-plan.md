# Phase 1F / Phase 1G Migration Plan

本方案承接 Phase 1D / 1E 的过渡策略：在 `AWIKI_USE_IM_CORE_MVP=1` 时，CLI 先构造 `im-core` SDK DTO，再通过 `awiki-cli::im_core_adapter` 转回 legacy request 调用旧实现；默认 legacy 行为不变。

Phase 1F / 1G 的目标不是一次性迁完整消息系统，而是把 Phase 1 主链路从“能发 direct text”扩展到“能发 group text，并能读必要 inbox/history”。

## 目标范围

Phase 1F：

- 迁移 `msg send --group <group> --text <text>`。
- 迁移 `msg send --group <group> --text-file <path>`。
- MVP 路径先构造 `im_core::SendMessageRequest`：
  - `target = MessageTarget::Group(GroupRef)`。
  - `body = MessageBody::Text`。
  - `security = MessageSecurityMode::DefaultPlain` 或 `Plain`。
- adapter 再转换为 legacy `message::SendRequest { group, text, message_type, ... }`，继续调用旧 `message::send`。

Phase 1G：

- 迁移 `msg inbox` 的必要子集：
  - `--scope all|direct|group`。
  - `--limit`。
  - `--unread`。
  - 当前命令没有 `--cursor` 时不新增命令面。
- 迁移 `msg history --with <peer> --limit <n> [--cursor <cursor>]`。
- MVP 路径先构造：
  - `im_core::InboxQuery`。
  - `im_core::ThreadRef::Direct(PeerRef)` + `im_core::HistoryQuery`。
- adapter 再转换为 legacy `message::InboxRequest` / `message::HistoryRequest`，继续调用旧 `message::inbox` / `message::history`。

## 当前基线

Phase 1D / 1E 之后，预期基线如下：

- `crates/im-core` 已包含 `SendMessageRequest`、`MessageTarget`、`InboxQuery`、`HistoryQuery`、`ThreadRef` 等公开 DTO。
- `crates/awiki-cli/src/im_core_adapter/messages.rs` 已有：
  - `send_message_request(...)`。
  - `inbox_query(...)`。
  - `history_request(...)`。
  - direct text 的 legacy conversion helper。
- `msg send --to` 在 `AWIKI_USE_IM_CORE_MVP=1` 时已经走 DTO + adapter 过渡路径。
- `msg send --group` 当前仍在 legacy 路径，或者 direct-only helper 会拒绝 group target。
- `msg inbox` / `msg history` 当前仍在 legacy 路径。
- 当前 `cmdmeta` 中 `msg.history` 只声明 `--with`，没有 `--group`；group history 能力已有 `group messages` 命令承载。Phase 1G 不应擅自扩展 `msg history --group` 命令面。

## 不迁移内容

本轮不迁移：

- `msg send --file` / attachment upload or download。
- `msg secure *`、`--secure on`、`SecureDirect`。
- `GroupE2ee`、group secure send、MLS、E2EE。
- `msg mark-read` 和 `msg inbox --mark-read` 的状态收口。
- conversation projection。
- 复杂本地 cache merge。
- 完整 unread count 统计。
- `group create/get/list/join/leave/add/remove/update/members/messages` 等 group lifecycle 命令。
- realtime runner / daemon。
- `id recover`、`id replace-did`、profile、contacts。
- `debug.db.*`。

## 边界决策

### Gate

所有行为变化都必须受 `AWIKI_USE_IM_CORE_MVP=1` 控制。未设置该环境变量时，CLI 继续走现有 legacy path。

### 依赖方向

依赖方向只能是：

```text
awiki-cli -> im-core
```

`im-core` 不能依赖 `awiki-cli`，不能引用 CLI 类型、CLI config、legacy manager、底层 wire/store/runtime 类型。

### 过渡调用

Phase 1F / 1G 可以继续通过 adapter 调用旧实现：

```text
CLI flags
  -> im-core DTO
  -> awiki-cli::im_core_adapter legacy conversion
  -> old message::* implementation
  -> existing CLI renderer
```

不要在本轮强行接通 `im-core` 内部真实 transport。若 `im-core::MessageService::{send,inbox,history}` 仍是 placeholder，CLI MVP path 可以继续通过 adapter bridge 完成过渡。

### Inbox filter 策略

`im_core::InboxQuery` 当前不表达 `--with` / `--group` filter，也不表达 `--mark-read`。因此 Phase 1G 推荐：

- `msg inbox` 无 `--with`、无 `--group`、无 `--mark-read` 时走 MVP DTO + adapter bridge。
- `msg inbox --with ...`、`msg inbox --group ...`、`msg inbox --mark-read` 在 MVP flag 下继续显式 fallback 到 legacy path，避免 silently ignoring filter 或 mark-read side effect。
- 不为迁移方便把 CLI filter 字段塞进 `InboxQuery`。

### History group 策略

实现手册中写到 `ThreadRef::Group`，但当前 `msg.history` 命令面没有 `--group`。因此 Phase 1G 推荐：

- 迁移当前实际存在的 `msg history --with <peer>`。
- 保留 / 测试 adapter 对 `ThreadRef::Group` 的 DTO 解析能力，前提是不扩大当前 CLI 命令面。
- 不新增 `msg history --group`，除非先更新命令契约、文档和 legacy 行为，并确认这不是本轮的范围扩大。

## 实施步骤

### Track A：重新核对文档和代码面

阅读并确认：

- `docs/sdk-refactor/implementation-playbook.md` 的 Phase 1F / 1G。
- `docs/sdk-refactor/Interface/04-message-interface.md`。
- `docs/sdk-refactor/Interface/05-cli-adapter-interface.md`。
- `docs/sdk-refactor/Interface/07-phase1-acceptance.md`。
- `docs/sdk-refactor/modules/07-messages.md`。
- `docs/sdk-refactor/modules/08-groups.md`。
- `docs/sdk-refactor/cli-boundary.md`。

代码检查重点：

- `crates/im-core/src/messages/*`。
- `crates/awiki-cli/src/im_core_adapter/messages.rs`。
- `crates/awiki-cli/src/app/msg_handlers.rs`。
- `crates/awiki-cli/src/message/{service,inbox,history,types}.rs`。
- `crates/awiki-cli/src/cmdmeta/mod.rs`。
- `crates/awiki-cli/tests/msg_contract.rs`。

### Track B：Phase 1F group text send

建议改动：

- 调整 `App::run_msg_send` 的 MVP gate，使 group plain text 在 `AWIKI_USE_IM_CORE_MVP=1` 时进入 `run_msg_send_im_core_mvp`。
- 保留 attachment / secure 的明确 unsupported 或已测试 fallback 策略；不要让这些能力意外进入 MVP adapter。
- 将 direct-only conversion helper 扩展为 text send conversion helper，例如从：

```rust
legacy_direct_text_send_request(...)
```

演进为：

```rust
legacy_text_send_request(...)
```

转换规则：

- `MessageTarget::Direct(peer)` -> legacy `target = peer`、`group = ""`。
- `MessageTarget::Group(group)` -> legacy `target = ""`、`group = group`。
- `MessageBody::Text` -> legacy `text` + `message_type`。
- `MessageBody::Attachment` -> `UnsupportedCapability("attachments")`。
- `SecureDirect` -> `UnsupportedCapability("secure-direct")`。
- `GroupE2ee` -> `UnsupportedCapability("group-e2ee")`。

auth scope：

- direct text 使用 `AuthScope::Messaging`。
- group text 可使用 `AuthScope::GroupMessaging`；若旧实现已经统一处理 auth，也至少保证 adapter path 不绕过现有 auth/session 行为。

dry-run：

- dry-run 仍由 CLI 渲染。
- MVP path 必须先构造 `SendMessageRequest`，再渲染 plan。
- group dry-run 的现有输出 shape 尽量不变：`action = "group.send"`，target kind 仍为 `group`。

### Track C：Phase 1G inbox / history

建议新增 gated handler：

- `run_msg_inbox_im_core_mvp`。
- `run_msg_history_im_core_mvp`。

`run_msg_inbox` gate 推荐：

```text
if AWIKI_USE_IM_CORE_MVP=1
  and --with is empty
  and --group is empty
  and --mark-read is false
then MVP adapter path
else legacy path
```

MVP inbox path：

- 构造 `InboxQuery`。
- dry-run 继续使用 CLI 现有 plan renderer / output shape。
- 非 dry-run 构造 `ImClient` 并确保 messaging session。
- adapter 转 `message::InboxRequest`，继续调用 `message::inbox`。

`run_msg_history` gate：

- `AWIKI_USE_IM_CORE_MVP=1` 时迁移当前实际命令面 `msg history --with <peer>`。
- 继续要求 `--with`，不在本轮新增 `--group`。

MVP history path：

- 构造 `(ThreadRef::Direct(peer), HistoryQuery)`。
- dry-run 继续保持当前 direct history plan shape。
- 非 dry-run 构造 `ImClient` 并确保 messaging session。
- adapter 转 `message::HistoryRequest`，继续调用 `message::history`。

### Track D：测试

adapter unit tests：

- group `SendMessageRequest` converts to legacy group text request。
- direct text conversion 仍保持 Phase 1E 行为。
- attachment / secure / group-e2ee 保持 unsupported 策略。
- `InboxQuery` parses `scope`、`limit`、`unread`。
- legacy inbox conversion 不包含 `mark_read` side effect，或 `mark_read` 明确不进入 MVP path。
- direct `history_request` builds `ThreadRef::Direct` + `HistoryQuery`。
- 如果 adapter 已支持 group thread parsing，可保留 `ThreadRef::Group` 单元测试，但不要把它变成 CLI contract requirement。

CLI contract tests：

- `AWIKI_USE_IM_CORE_MVP=1 msg send --group ... --text ... --dry-run` 输出仍为 `group.send`。
- `AWIKI_USE_IM_CORE_MVP=1 msg send --group ... --text-file ... --dry-run`。
- MVP flag 下 direct text 仍通过原 1E contract。
- MVP flag 下 attachment / secure direct 仍按既定策略失败或 fallback，并有明确测试。
- `AWIKI_USE_IM_CORE_MVP=1 msg inbox --limit 5 --dry-run`。
- `AWIKI_USE_IM_CORE_MVP=1 msg inbox --scope group --unread --limit 5 --dry-run`。
- `AWIKI_USE_IM_CORE_MVP=1 msg inbox --mark-read --dry-run` 走 legacy/fallback contract，不能被 DTO path 吞掉 side effect。
- `AWIKI_USE_IM_CORE_MVP=1 msg history --with bob --limit 5 --cursor seq-2 --dry-run`。
- `msg history --group` 不作为本轮 contract，除非命令契约先被正式扩展。

## 验证命令

必跑：

```bash
cargo fmt --all --check
cargo test -p im-core --locked
cargo test -p awiki-cli im_core_adapter --locked
cargo test -p awiki-cli msg --locked
cargo run --bin xtask --locked -- check-structure
```

边界 grep：

```bash
rg -n "ParsedCommand|GlobalOptions|ExitError|config::Resolved|identity::Manager|crate::app|crate::cli|crate::config|awiki_cli|ActorContext|StoredIdentity|ClientIdentityRuntime|IdentityRuntimePaths|serde_json::Value" crates/im-core/src || true
```

时间允许可加：

```bash
cargo test -p awiki-cli --locked
```

不要运行真实生产 CLI 命令。只允许测试、fixture、mock server、dry-run 或 isolated temp workspace。

## 提交建议

推荐拆成两个提交：

```text
feat: route group text send mvp through im-core facade
feat: route inbox history mvp through im-core facade
```

若实现中 group send 与 inbox/history 强耦合，也可以一个提交：

```text
feat: route phase 1f 1g through im-core facade
```

提交前必须确认没有混入无关改动，尤其不要把 Phase 1D / 1E 以外的工作树残留误改或回滚。

## 完成报告

最终报告应包含：

- 修改文件。
- Phase 1F 完成行为。
- Phase 1G 完成行为。
- 关键函数。
- unsupported / fallback 策略。
- 测试命令和结果。
- 是否运行真实 CLI 命令。
- Phase 1H 及后续剩余事项。
