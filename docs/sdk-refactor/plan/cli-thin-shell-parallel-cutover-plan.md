# awiki-cli Thin Shell Parallel Cutover Plan

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：`im-core` 已具备对应 public service 后，执行 `awiki-cli` 底层实现删除和最终薄壳化。  
**目标**：只要 `im-core` 已有相关实现，`awiki-cli` 默认路径必须使用 `im-core` public API；`awiki-cli` 只保留 CLI 壳职责。  
**执行方式**：多机并行处理多个低冲突 track，最后通过一个串行收口 track 删除共享 legacy 模块、依赖和 allowlist。

---

## 1. 总目标

最终默认执行链路固定为：

```text
awiki-cli command
  -> parse args / flags / globals
  -> enforce command policy
  -> resolve CLI config / workspace / paths
  -> build ImCore / ImClient
  -> convert CLI input to im-core DTO
  -> call im-core public service
  -> render stdout/stderr / exit code / dry-run plan
```

默认路径不得再进入：

```text
crate::message::* business logic
crate::identity::* business flow
crate::mail::* business logic
crate::store::* as IM local_state implementation
crate::runtime listener legacy session loop / projection
crate::authsdk / crate::anpsdk as IM command implementation
im_core::compat as default CLI execution path
raw RPC / raw SQL / wire payload as default user surface
```

允许 `awiki-cli` 保留：

```text
command parsing / alias / schema / completion / docs
--identity / --format / --dry-run / --verbose
config file read/write and workspace resolution
ImCoreConfig / ImCorePaths assembly
input file read and output file write
stdout/stderr rendering and ExitError mapping
dry-run plan rendering
runtime service manager: systemd / launchd / Windows service
listener process pid/log/socket management
OpenClaw / Hermes host notify config and delivery
migration-only / diagnostic-only commands behind gates
```

---

## 2. 参考文档

执行任何 track 前先阅读：

```text
docs/sdk-refactor/implementation-playbook.md
docs/sdk-refactor/cli-boundary.md
docs/sdk-refactor/im-core-cli-boundary.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
docs/sdk-refactor/plan/cli-shell-final-cutover-execution-plan2.md
```

按 track 额外阅读：

```text
Track A:
  docs/sdk-refactor/plan/content-site-migration-execution-plan.md
  docs/sdk-refactor/plan/email-migration-execution-plan.md
  docs/sdk-refactor/plan/phase2-people-profile-migration-execution-plan.md

Track B:
  docs/sdk-refactor/plan/phase2-phase3-migration-execution-plan.md
  docs/sdk-refactor/modules/02-identity.md
  docs/sdk-refactor/modules/03-auth.md
  docs/sdk-refactor/modules/06-directory.md

Track C:
  docs/sdk-refactor/plan/phase2-phase3-migration-execution-plan.md
  docs/sdk-refactor/plan/phase4-attachments-migration-execution-plan.md
  docs/sdk-refactor/plan/phase6-secure-e2ee-migration-execution-plan.md
  docs/sdk-refactor/modules/07-messages.md
  docs/sdk-refactor/modules/08-groups.md
  docs/sdk-refactor/modules/09-attachments.md
  docs/sdk-refactor/modules/10-secure.md

Track D:
  docs/sdk-refactor/plan/phase5-realtime-runner-migration-execution-plan.md
  docs/sdk-refactor/plan/phase5-attachment-enrichment-follow-up-plan.md
  docs/sdk-refactor/modules/04-local-state.md
  docs/sdk-refactor/modules/11-realtime.md
```

---

## 3. 并行拆分

建议并行开 4 条分支：

| Track | 文档 | 主要范围 | 可并行原因 |
| --- | --- | --- | --- |
| A | `cli-thin-shell-track-a-surface-adapters-plan.md` | mail / page / site / people.contacts / command surface 小清理 | 主要触碰 app handlers、adapter、cmdmeta，和 B/C/D 冲突低 |
| B | `cli-thin-shell-track-b-identity-auth-plan.md` | identity/auth adapter 去旧 `identity::Manager` 业务依赖 | 主要触碰 identity/auth adapter 与 id handlers |
| C | `cli-thin-shell-track-c-message-group-secure-plan.md` | msg/group/attachment/secure 默认路径收敛，旧 `message` 模块删除准备 | 主要触碰 msg/group handlers、message/group adapter、message tests |
| D | `cli-thin-shell-track-d-runtime-local-state-plan.md` | runtime runner/local_state projection/store 收敛 | 主要触碰 runtime/store/local_state，和 C 通过 DTO/事件边界对接 |
| Final | `cli-thin-shell-final-serial-cleanup-plan.md` | 串行删除共享 legacy 模块、Cargo 依赖、静态门禁、全量测试 | 必须等 A-D 合并后执行 |

推荐分支命名：

```text
cutover/thin-shell-track-a-surface-adapters
cutover/thin-shell-track-b-identity-auth
cutover/thin-shell-track-c-message-group-secure
cutover/thin-shell-track-d-runtime-local-state
cutover/thin-shell-final-serial-cleanup
```

---

## 4. 共享硬约束

所有 track 必须遵守：

```text
1. 如果 im-core 有 public service，awiki-cli 默认路径必须调用该 service。
2. 不新增 awiki-cli 旧业务 fallback。
3. 不把 ParsedCommand / ExitError / config::Resolved / GlobalOptions 搬进 im-core。
4. 不让 im-core 依赖 awiki-cli。
5. im_core_adapter 只做 CLI boundary conversion，不做 wire、auth retry、projection、target resolve、legacy request bridge。
6. migration-only / diagnostic-only 能力必须 gate，不进入 default schema/help/completion。
7. raw RPC / raw SQL / provider secret / MLS internals / KeyPackage / prekey / ciphertext 不进入默认 CLI 产品面。
8. 删除代码前先迁移或删除对应测试，不留下测试必须依赖 awiki_cli::message / awiki_cli::mail / awiki_cli::store 的默认路径。
```

---

## 5. 冲突规避规则

为方便多机并行：

```text
1. Track A 不改 runtime/store/message internals。
2. Track B 不改 msg/group runtime behavior，只改 identity/auth 选择和 adapter 边界。
3. Track C 不删除 crate::store 和 crate::identity 根模块，只删除或收敛 message 相关引用；真正删除共享模块留给 Final。
4. Track D 不重写 msg/group command output shape，只负责 runtime/local_state ownership。
5. A-D 都可以新增 allowlist 项，但不得扩大默认 legacy fallback；allowlist 只能 burn-down。
6. Cargo.toml 依赖大删除留给 Final，避免并行分支反复冲突。
7. `crates/awiki-cli/src/lib.rs` 的大规模 module 删除留给 Final。
```

---

## 6. Track 间契约

Track B 为其他 track 提供：

```text
build_im_core/build_im_client 不再依赖旧 identity business flow
ImCorePaths 从 CLI Resolved paths 直接组装
identity selector / default identity 行为稳定
ImError -> ExitError 映射稳定
```

Track C 为 Track D 提供：

```text
message/group/attachment/secure DTO 使用 im-core public DTO
旧 awiki-cli message request/response 类型不再是 runtime 或 adapter 的公共边界
secure outbox / group E2EE 只通过 im-core secure/messages API 暴露
```

Track D 为 Final 提供：

```text
runtime listener run/service-run 只宿主 im-core runner
runtime 不再直接执行 message/group/contact local_state projection
store 普通 IM projection 已迁到 im-core local_state/internal store
```

Final 只在 A-D 都完成后执行：

```text
删除旧模块
删除 Cargo 依赖
清理 allowlist
全量测试和 schema 快照确认
```

---

## 7. 静态门禁

所有 track 合并前至少运行：

```bash
cargo test -p im-core
cargo check -p awiki-cli
```

Track 局部门禁：

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" \
  crates/im-core/src crates/im-core/tests

rg "crate::message::|use crate::message\\b|crate::store::|use crate::store\\b|crate::identity::service|crate::identity::client|crate::authsdk|crate::anpsdk|im_core::compat" \
  crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime
```

允许在中间阶段使用 burn-down allowlist，但每个 track 的 PR 描述必须列出新增/剩余项以及归属 track。Final 完成后，默认 app/adapter/runtime 路径中的这些引用应清零，除非明确是 migration/diagnostic/internal gate。

---

## 8. 完成定义

本计划完成后应满足：

```text
1. awiki-cli 默认命令只调用 im-core public services 或 CLI-owned shell functions。
2. awiki-cli/src/message、mail、旧 identity business、普通 IM store projection 可删除或只剩 gated migration/diagnostic。
3. runtime listener 不再包含 IM realtime 状态机、notification classify、message/group/contact projection。
4. schema default surface 不展示 removed/hidden/diagnostic/internal/stub 命令。
5. cargo check/test 通过，且 im-core 不引用 CLI 类型。
6. 用户能通过同一套高层命令完成原产品任务，但底层实现来自 im-core。
```

