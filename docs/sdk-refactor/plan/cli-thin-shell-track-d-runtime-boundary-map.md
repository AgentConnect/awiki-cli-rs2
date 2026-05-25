# Track D Runtime Boundary Map

**对应计划**：`docs/sdk-refactor/plan/cli-thin-shell-track-d-runtime-local-state-plan.md`  
**并行分支**：`cutover/thin-shell-track-d-runtime-local-state`  
**适用阶段**：`im-core` 已经具备 realtime runner、event DTO、local_state projection、secure runtime 能力后，用于把 `awiki-cli` runtime 收缩成薄宿主。  
**目标**：给 Track D 提供逐文件 ownership 判定表，避免多机并行时把 IM 底层逻辑继续留在 `awiki-cli`，或把 CLI 自己的进程/服务职责误迁入 `im-core`。

---

## 1. 核心边界

一句话：

```text
awiki-cli runtime 是本机进程/服务/通知宿主，不是 IM runtime engine。
im-core 是 IM 连接、事件、投影、secure 和 local_state 的唯一默认实现来源。
```

判断规则：

| 代码职责 | 最终归属 |
| --- | --- |
| CLI 参数、配置解析、workspace/path 组装 | `awiki-cli` |
| foreground/service-run 进程宿主 | `awiki-cli` |
| systemd/launchd/Windows service 安装、启动、停止、状态 | `awiki-cli` |
| pid/log/socket/status 文件 | `awiki-cli` |
| bridge socket 的本机控制面 | `awiki-cli` |
| OpenClaw/Hermes/file/log host notify delivery | `awiki-cli` |
| WebSocket URL 派生、connect、heartbeat、reconnect、frame classify | `im-core` |
| realtime auth refresh | `im-core` |
| raw notification classify | `im-core` |
| notification -> `ImEvent` | `im-core` |
| message/group/contact local_state projection | `im-core` |
| attachment manifest enrichment for realtime events | `im-core` |
| direct secure incoming decrypt、ack、outbox、replay | `im-core` |
| group E2EE incoming/notice/runtime processing | `im-core` |
| SQLite schema and normal IM local_state writes | `im-core` |

Track D 的默认结论：

```text
如果一段 runtime 代码需要理解 IM wire payload、消息/群/联系人语义、secure payload、SQLite IM projection，
它就不该继续作为 awiki-cli 默认执行路径存在。
```

---

## 2. CLI 保留区

这些文件或模块可以保留在 `awiki-cli`，但要遵守“只做 CLI 宿主，不做 IM 业务”的限制。

| 文件/模式 | 最终角色 | 允许职责 | 禁止职责 |
| --- | --- | --- | --- |
| `crates/awiki-cli/src/runtime/listener.rs` | listener status DTO 和本机状态渲染 | 读取/写入 CLI listener 状态，转换 status JSON | IM notification classify、消息投影、secure 处理 |
| `crates/awiki-cli/src/runtime/listener_foreground.rs` | `listener run` 前台入口薄封装 | 调用 `listener_supervisor_run::run_foreground` 或后续 thin runner | 自己打开 websocket 或写 local_state |
| `crates/awiki-cli/src/runtime/listener_service.rs` | service manager 抽象 | install/start/stop/restart/uninstall/status | IM session loop、message/group/contact 处理 |
| `crates/awiki-cli/src/runtime/listener_systemd.rs` | Linux systemd service manager | unit 文件、systemctl 调用、service status | IM runtime 业务 |
| `crates/awiki-cli/src/runtime/listener_launchd.rs` | macOS launchd service manager | plist、launchctl、service status | IM runtime 业务 |
| `crates/awiki-cli/src/runtime/listener_windows_service.rs` | Windows service manager | Windows service install/status/control | IM runtime 业务 |
| `crates/awiki-cli/src/runtime/listener_shutdown_signal.rs` | CLI shutdown bridge | Ctrl-C、service shutdown、atomic flag 转换为 `im-core::realtime::ShutdownSignal` | 自己终止 IM session state machine |
| `crates/awiki-cli/src/runtime/listener_supervisor_init.rs` | CLI runtime 目录和状态初始化 | runtime dir、pid/log/socket、host notify config 初始化 | 初始化 IM local_state schema，除非只是迁移期检查 |
| `crates/awiki-cli/src/runtime/listener_supervisor_shutdown.rs` | CLI runtime shutdown | 清理 pid/socket、更新本机 listener status | secure ack/outbox/replay cleanup |
| `crates/awiki-cli/src/runtime/bridge.rs` | 本机 daemon socket | socket endpoint、local bridge accept/connect、status/control envelope | IM RPC 默认转发、raw websocket request dispatch |
| `crates/awiki-cli/src/runtime/listener_bridge_connection.rs` | bridge connection IO | 解析本机 bridge request，返回本机控制面 response | 直接执行 IM wire/RPC 业务 |
| `crates/awiki-cli/src/runtime/listener_bridge_runtime.rs` | bridge session bootstrap/control helper | 等待 runner ready、host notify/local notification queue 接线 | 维护 IM session loop 或 reconnect 策略 |
| `crates/awiki-cli/src/runtime/listener_known_sessions.rs` | CLI status 中的 session 展示辅助 | 已知 session 启动状态、错误记录 | session auth、websocket 连接、notification consume |
| `crates/awiki-cli/src/runtime/listener_identity_watch.rs` | CLI identity 变更观察 | 发现本机 identity 配置变化并触发 runner 重建 | identity/auth 业务流程或 token refresh |
| `crates/awiki-cli/src/runtime/host_notify.rs` | CLI host notify DTO 兼容层 | host notify sink 需要的本机事件格式，短期可从 `im-core` event 转换 | 从 raw notification 构造 IM 语义事件 |
| `crates/awiki-cli/src/runtime/host_notify_sink.rs` | host notify sink 分发 | file/log/openclaw/hermes sink 选择和发送 | message/group/contact projection |
| `crates/awiki-cli/src/runtime/openclaw_host_notify.rs` | OpenClaw delivery | 把 high-level host event 渲染给 OpenClaw | DID lookup、manifest parse、secure decrypt |
| `crates/awiki-cli/src/runtime/openclaw_routes.rs` | OpenClaw local route | 本机 route/webhook glue | IM state machine |
| `crates/awiki-cli/src/runtime/openclaw_webhook.rs` | OpenClaw webhook | webhook envelope、token、HTTP delivery | IM wire/RPC fallback |
| `crates/awiki-cli/src/runtime/hermes_host_notify.rs` | Hermes delivery | 把 high-level host event 渲染给 Hermes | notification classify/projection |
| `crates/awiki-cli/src/runtime/hermes_bridge/*` | Hermes bridge delivery | Hermes service route/delivery | IM runtime 底层 |

保留区的额外约束：

```text
1. 允许读 CLI config 和 runtime path。
2. 允许写 pid/log/socket/status/host-notify 输出。
3. 不允许打开普通 IM local_state SQLite 来写 message/group/contact。
4. 不允许解析 raw IM notification 后决定 message/group/contact/secure 业务含义。
5. 不允许作为 im-core 缺能力时的 legacy fallback。
```

---

## 3. 迁入 im-core 区

这些文件当前属于 `awiki-cli` runtime 底层实现，最终应迁入 `im-core`，或由 `im-core` 现有实现替代。CLI 侧只允许留下 thin adapter、测试夹具，或删除。

| 当前 CLI 文件 | 目标归属 | 迁移/替代目标 | CLI 最终残留 |
| --- | --- | --- | --- |
| `listener_wsclient.rs` | `im-core` | `crates/im-core/src/internal/realtime/dispatch.rs`、`frame.rs`、`notification.rs`、`transport.rs`、`ws_transport.rs` | 无；最多 test fixture |
| `listener_ws_transport.rs` | `im-core` | `crates/im-core/src/internal/realtime/ws_transport.rs`、`transport.rs` | 无 |
| `listener_connect_session.rs` | `im-core` | `crates/im-core/src/internal/realtime/transport.rs`、`crates/im-core/src/internal/auth/session.rs`、`RealtimeService::run_until_shutdown` | 无；CLI 只 build `ImClient` |
| `listener_session_loop.rs` | `im-core` | `crates/im-core/src/internal/realtime/session_loop.rs`、`reconnect.rs`、`heartbeat.rs`、`shutdown.rs` | 无 |
| `listener_session_rpc.rs` | `im-core` | `crates/im-core/src/realtime/wire.rs`、`internal/json_rpc.rs`、message/group public services | 无；bridge 不做默认 IM RPC tunnel |
| `listener_session_state.rs` | `im-core` | `RealtimeHandle`、`RealtimeControl`、runner internal state | 无 |
| `listener_session_methods.rs` | `im-core` | `crates/im-core/src/realtime/wire.rs` 或 public services | 无 |
| `listener_service_did.rs` | `im-core` | directory/transport capability discovery service | 无；CLI 不直接向 websocket session 查询 service DID |
| `listener_notification_plan.rs` | `im-core` | `crates/im-core/src/internal/realtime/projection.rs`、`local_projection.rs`、public `ImEvent` | 无；host notify only conversion 可迁到 `host_notify.rs` |
| `listener_notification_execute.rs` | `im-core` | `crates/im-core/src/internal/realtime/local_projection.rs`、`internal/local_state/*` | 无 |
| `listener_notification_handler.rs` | `im-core` | `project_notification` + local projection + event emission | 无 |
| `listener_notification_consume.rs` | `im-core` | `internal/realtime/heartbeat.rs`、`session_loop.rs` | 无 |
| `listener_contact_sync.rs` | `im-core` | `crates/im-core/src/internal/contact_store/*`、`internal/local_state/contacts.rs` | 无；display fallback 通过 `im-core` DTO 提供 |
| `listener_message_records.rs` | `im-core` | `crates/im-core/src/internal/realtime/projection.rs`、`local_projection.rs`、`internal/local_state/messages.rs`、`groups.rs` | 无 |
| `listener_local_notifications.rs` | `im-core` by default | `ImEvent::LocalNotification` 或 runner event queue | CLI 可保留临时 queue 只用于本机 bridge delivery |
| `listener_local_notification_flush.rs` | `im-core` by default | `RealtimeService` event receiver/runner sink | CLI 只保留 host/bridge flush，不处理 IM payload |
| `listener_handle_lookup.rs` | `im-core` | `client.directory()`、contact/profile public service | 只允许 UI-only display fallback，且不得写 local_state |
| `listener_json_helpers.rs` | `im-core` or delete | projection/wire helper 应进入 `im-core` internal；CLI 只留 bridge/host-notify JSON helper | 只允许非 IM wire 的本机 JSON helper |
| `crates/awiki-cli/src/store/messages.rs` | `im-core` | `crates/im-core/src/internal/local_state/messages.rs` | 默认 runtime/app 不引用 |
| `crates/awiki-cli/src/store/groups.rs` | `im-core` | `crates/im-core/src/internal/local_state/groups.rs` | 默认 runtime/app 不引用 |
| `crates/awiki-cli/src/store/contacts.rs` | `im-core` | `crates/im-core/src/internal/local_state/contacts.rs`、`internal/contact_store/*` | 默认 runtime/app 不引用 |
| `crates/awiki-cli/src/store/e2ee_outbox.rs` | `im-core` | `crates/im-core/src/internal/store/e2ee_outbox.rs`、`internal/secure_direct/outbox.rs` | 默认 runtime/app 不引用 |
| `crates/awiki-cli/src/store/schema.rs` | `im-core` | `crates/im-core/src/internal/local_state/schema.rs` | 只允许 migration/diagnostic gate 短期引用 |
| `crates/awiki-cli/src/store/open.rs` | `im-core` | `ImCorePaths` + `im-core` local state open/ensure | 默认 runtime/app 不引用 |
| `crates/awiki-cli/src/store/query.rs` | `im-core` | messages/groups/directory read services | debug-only 或删除 |
| `crates/awiki-cli/src/store/import.rs` | migration-only | identity import/migration track 或 Final | 必须 gate，不进默认 runtime |
| `crates/awiki-cli/src/store/rebind.rs` | migration-only | identity/local_state migration | 必须 gate，不进默认 runtime |
| `crates/awiki-cli/src/store/recover_merge/*` | migration-only | recovery migration | 必须 gate，不进默认 runtime |

迁移区的完成标准：

```text
1. `runtime listener run/service-run` 默认路径不再引用这些文件。
2. `crates/awiki-cli/src/runtime/mod.rs` 不再公开这些模块，除非只作为 gated test/migration helper。
3. 对应测试迁到 `crates/im-core/tests` 或改成 CLI boundary 测试。
4. 删除 CLI 残留前，Final track 可以通过静态门禁确认没有默认路径引用。
```

---

## 4. Secure 迁移地图

Secure 相关逻辑全部属于 IM 底层。Track D 只允许 CLI 接收 `im-core` 已经处理后的 high-level event 或 warning。

| 当前 CLI 文件 | 目标归属 | 目标能力 | CLI 最终残留 |
| --- | --- | --- | --- |
| `listener_secure_notifications.rs` | `im-core` | secure wire content-type 识别、message view -> notification | 无 |
| `listener_secure_normalize.rs` | `im-core` | direct secure incoming normalize/decrypt/init-ack plan | 无 |
| `listener_secure_ack_delivery.rs` | `im-core` | local secure ack delivery plan | 无 |
| `listener_secure_ack_in_process.rs` | `im-core` | decrypt 后 follow-up ack/encrypt/local queue plan | 无 |
| `listener_secure_outbox_flush.rs` | `im-core` | queued secure outbox flush/retry | 无 |
| `listener_secure_replay.rs` | `im-core` | unread/history replay candidate planning | 无 |
| `listener_secure_sessions.rs` | `im-core` | direct secure session/prekey file or SQLite state | 无 |
| `listener_secure_sync.rs` | `im-core` | unread inbox/history sync RPC and replay actions | 无 |
| `listener_secure_inbox_poll.rs` | `im-core` | secure unread inbox polling policy | 无 |

目标实现优先落点：

```text
crates/im-core/src/internal/secure_direct/control.rs
crates/im-core/src/internal/secure_direct/incoming.rs
crates/im-core/src/internal/secure_direct/outbox.rs
crates/im-core/src/internal/secure_direct/file_runtime.rs
crates/im-core/src/internal/secure_direct/sqlite_store.rs
crates/im-core/src/internal/group_e2ee/incoming.rs
crates/im-core/src/internal/group_e2ee/notices.rs
crates/im-core/src/internal/group_e2ee/runtime.rs
crates/im-core/src/realtime/runner.rs
crates/im-core/src/realtime/events.rs
```

Secure fallback 规则：

```text
1. im-core 支持 direct/group secure 时，CLI 必须使用 im-core。
2. im-core 暂不支持某个 secure realtime 子能力时，im-core 返回 unsupported/warning event。
3. CLI 不允许因为 unsupported 再调用 listener_secure_* legacy fallback。
4. CLI 可以把 warning 写入 listener status 或 stderr/log，但不能补做 decrypt/ack/outbox。
```

---

## 5. 人工收缩区

这些文件通常不会整体删除，但必须拆出 IM 底层职责。多人并行时，这些文件最容易冲突，建议由 Track D owner 单独处理。

| 文件 | 当前混杂职责 | 收缩后保留 | 必须迁走/删除 |
| --- | --- | --- | --- |
| `listener_supervisor_run.rs` | CLI process host、session loop、websocket connect、secure poll/replay、store open、im-core runner adapter | `run_foreground`、`run_service`、build `ImClient`、build `RealtimeOptions`、build shutdown、host notify/status event sink | `connect_session`、manual websocket dialer、session loop、secure poll/replay、store open/schema、message exists lookup、raw notification handling |
| `listener_im_event_adapter.rs` | `ImEvent` sink、host notify conversion、message/group/contact projection、store writes | `ImEvent::HostNotification`/`LocalNotification` -> host notify sink，`ConnectionStateChanged` -> status | `MessageReceived`/`GroupUpdated` local projection、store open/ensure_schema/upsert、attachment manifest parsing |
| `listener_bridge_dispatch.rs` | bridge request planning、RPC call build、identity wire conversion | local control plane request planning: status/start/stop/shutdown/bootstrap | default IM RPC routing、message/group wire param construction、service DID lookup |
| `listener_bridge_connection.rs` | bridge socket IO + runtime callback | local socket read/write and dispatch to CLI-owned control methods | IM RPC execution |
| `listener_bridge_runtime.rs` | bridge bootstrap and host/local queue glue | wait runner ready, expose local daemon status, host notify/local notify queue delivery | session loop ownership, websocket session lifecycle |
| `mod.rs` | exports all runtime modules | only export CLI-owned runtime modules | remove listener_ws/session/notification/secure/store-projection module exports |

Recommended end-state sketch for `listener_supervisor_run.rs`:

```text
run_foreground(resolved)
run_service(resolved)
  -> ensure CLI runtime dirs
  -> write pid/status/log/socket as needed
  -> build ImCore/ImClient from CLI config
  -> build RealtimeOptions
  -> build ShutdownSignal
  -> run client.realtime().run_until_shutdown(options, shutdown)
  -> consume high-level ImEvent only through a small CLI event sink
  -> cleanup CLI runtime artifacts
```

Forbidden end-state in `listener_supervisor_run.rs`:

```text
connect_realtime_with_transport(...)
ListenerRealtimeDialer / ListenerRealtimeAuth
handle_listener_notification(...)
store::open / store::ensure_schema
sync_unread_secure_direct_inbox(...)
flush_secure_outbox(...)
deliver_local_secure_ack(...)
listener_secure_* calls
listener_message_records calls
listener_contact_sync calls
```

---

## 6. Host Notification Contract

Host notification 是 Track D 中 CLI 可以保留的主要 runtime 能力，但它的输入必须是 `im-core` high-level event。

`im-core` 应输出：

```text
ImEvent::HostNotification(HostNotificationEvent)
ImEvent::LocalNotification(LocalNotificationEvent)
ImEvent::MessageReceived(MessageReceivedEvent)
ImEvent::MessageUpdated(MessageUpdatedEvent)
ImEvent::GroupUpdated(GroupUpdatedEvent)
ImEvent::ConnectionStateChanged(ConnectionStateChanged)
ImEvent::UnknownNotification(UnknownNotificationEvent)
```

CLI 可以做：

```text
1. 把 `HostNotificationEvent` 转成 OpenClaw/Hermes/file/log sink payload。
2. 把 `LocalNotificationEvent` 暂存到本机 bridge/local queue。
3. 把 `ConnectionStateChanged` 写入本机 listener status/log。
4. 把 `UnknownNotification` 写入 warnings/status/log。
```

CLI 不可以做：

```text
1. 从 raw notification JSON 构造 direct/group/contact/secure 语义。
2. 读取 attachment manifest 并补齐 host notification。
3. 为了显示 handle 直接执行 DID lookup 并写 contact store。
4. 从 `MessageReceived` 再执行 message/group/contact local_state projection。
```

如果 OpenClaw/Hermes 需要字段，优先扩展 `im-core` 的 `HostNotificationEvent`/`MessageReceivedEvent` DTO，而不是在 CLI 里重建投影逻辑。

---

## 7. Local State Contract

Track D 完成后，默认 runtime path 不再打开 CLI store 做普通 IM projection。

禁止出现在默认 runtime path：

```text
crate::store::
use crate::store
store::open
store::ensure_schema
store_message
upsert_group
upsert_group_member
upsert_contact
GroupRecord
MessageRecord
ContactRecord
```

允许短期存在，但必须 gated：

```text
debug.db.*
doctor
id.import-v1
upgrade/migration/recover
Final 删除前的 compat wrapper test
```

本地文件职责划分：

| 文件类型 | 归属 | 说明 |
| --- | --- | --- |
| pid/status/log/socket | `awiki-cli` | service manager 和 daemon control 需要 |
| host notify file sink | `awiki-cli` | 用户配置的本机通知输出 |
| IM local_state SQLite schema | `im-core` | messages/groups/contacts/conversations/email projection |
| secure direct state/outbox | `im-core` | direct session/prekey/outbox/ack |
| group E2EE state | `im-core` | MLS/native provider/local group state |

---

## 8. Bridge Contract

CLI bridge 是本机 control plane，不是 IM RPC fallback。

允许 bridge 支持：

```text
status
start/stop/restart/shutdown
bootstrap runner/session readiness
host notify/local notification queue flush
listener runtime diagnostics
```

不允许 bridge 默认支持：

```text
message direct send as raw websocket RPC fallback
group RPC dispatch as raw websocket RPC fallback
session auth refresh
service DID capability discovery
notification projection
secure inbox sync/replay/outbox
```

如果 CLI 命令需要发送消息、群操作、mark-read、history、secure send：

```text
CLI command -> im_core_adapter thin conversion -> im-core public service
```

不要走：

```text
CLI command -> bridge -> active websocket session -> raw RPC -> legacy projection
```

---

## 9. `im-core` 目标落点

当前仓库已经存在这些可承接 Track D 的 `im-core` 模块。迁移时优先复用，不新建第二套 runtime。

| 能力 | 目标模块 |
| --- | --- |
| public realtime service | `crates/im-core/src/realtime/service.rs` |
| realtime options/status/exit DTO | `crates/im-core/src/realtime/dto.rs` |
| high-level event DTO | `crates/im-core/src/realtime/events.rs` |
| runner loop | `crates/im-core/src/realtime/runner.rs` |
| shutdown/control handle | `crates/im-core/src/realtime/control.rs`、`handle.rs` |
| websocket transport/connect/auth refresh | `crates/im-core/src/internal/realtime/transport.rs`、`ws_transport.rs` |
| frame classify/dispatch | `crates/im-core/src/internal/realtime/frame.rs`、`dispatch.rs` |
| heartbeat/reconnect/session loop | `crates/im-core/src/internal/realtime/heartbeat.rs`、`reconnect.rs`、`session_loop.rs` |
| notification projection | `crates/im-core/src/internal/realtime/projection.rs` |
| realtime local projection | `crates/im-core/src/internal/realtime/local_projection.rs` |
| attachment event enrichment | `crates/im-core/src/internal/realtime/attachment_projection.rs` |
| local_state schema/store | `crates/im-core/src/internal/local_state/*` |
| contact projection | `crates/im-core/src/internal/contact_store/*` |
| direct secure runtime | `crates/im-core/src/internal/secure_direct/*` |
| group E2EE runtime | `crates/im-core/src/internal/group_e2ee/*` |

Public API 边界：

```text
awiki-cli 只能依赖 `im_core::prelude`、public service、public DTO。
awiki-cli 不应该直接依赖 `im_core::internal::*`。
如果 CLI 需要 internal 能力，先补 `im-core` public service/DTO，再接 CLI。
```

---

## 10. 并行 PR 切片

Track D 可以再拆成 6 个更细 PR。D0-D4 可部分并行，D5 建议最后串行收口。

| Slice | 可并行性 | 范围 | 输出 |
| --- | --- | --- | --- |
| D0 static guard | 可先做 | 增加或更新边界测试/静态检查脚本/allowlist | 明确当前剩余 legacy runtime 引用 |
| D1 runner host shrink | 与 D2/D3 低冲突 | `listener_supervisor_run.rs` 改成调用 `client.realtime().run_until_shutdown` | CLI 不再自己 host websocket/session loop |
| D2 event/host notify shrink | 与 D3 可并行，和 D1 需对接 event sink | `listener_im_event_adapter.rs`、`host_notify*.rs` | CLI 只消费 high-level host/local/status event |
| D3 projection migration | 与 D2 可并行 | `listener_notification_*`、`listener_message_records.rs`、`listener_contact_sync.rs` | message/group/contact projection 迁入 `im-core` |
| D4 secure runtime migration | 与 D2/D3 可并行，但需共享 event DTO | `listener_secure_*` | secure normalize/ack/outbox/replay 迁入 `im-core` |
| D5 bridge/store delete prep | 串行收口 | `listener_bridge_*`、`runtime/mod.rs`、`store/*` 引用 | 默认 runtime path 清零 CLI store/legacy runtime 引用 |

建议合并顺序：

```text
D0 -> D1
D2/D3/D4 可在 D1 后或基于 D0 并行推进
D5 等 D1-D4 合并后做
Final track 再删除共享 legacy module/Cargo 依赖
```

---

## 11. 静态门禁

Track D 每个 PR 至少跑：

```bash
cargo test -p im-core
cargo check -p awiki-cli
```

runtime legacy 引用检查：

```bash
rg "listener_session_loop|listener_session_rpc|listener_session_state|listener_session_methods|listener_wsclient|listener_ws_transport|connect_realtime_with_transport|handle_listener_notification" \
  crates/awiki-cli/src/runtime
```

store projection 检查：

```bash
rg "crate::store::|use crate::store\\b|store::open|store::ensure_schema|store_message|upsert_group|upsert_group_member|upsert_contact|GroupRecord|MessageRecord|ContactRecord" \
  crates/awiki-cli/src/runtime \
  crates/awiki-cli/src/app \
  crates/awiki-cli/src/im_core_adapter
```

secure legacy 检查：

```bash
rg "listener_secure_|secure_unread_direct_inbox|secure_pending_confirmation|flush_secure_outbox|deliver_local_secure_ack|normalize_secure_notification" \
  crates/awiki-cli/src/runtime
```

raw wire/RPC fallback 检查：

```bash
rg "build_bridge_.*rpc|send_rpc|anp\\.message|anp\\.group|anp\\.get_capabilities|service_did" \
  crates/awiki-cli/src/runtime
```

`im-core` 反向依赖检查：

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|awiki_cli|crate::runtime|crate::store" \
  crates/im-core/src crates/im-core/tests
```

中间阶段允许 allowlist，但每个 PR 必须说明：

```text
1. 新增 allowlist 项是什么。
2. 为什么暂时不能删除。
3. 归属哪个后续 slice。
4. 删除条件是什么。
```

---

## 12. 完成定义

Track D 完成时应满足：

```text
1. `listener run` 和 `listener service-run` 默认路径只宿主 `im-core` realtime runner。
2. `awiki-cli` runtime 不再自己连接 websocket、维护 heartbeat/reconnect/session loop。
3. `awiki-cli` runtime 不再 classify raw IM notification。
4. `awiki-cli` runtime 不再写 message/group/contact local_state。
5. `awiki-cli` runtime 不再执行 direct secure decrypt/ack/outbox/replay。
6. `awiki-cli` bridge 不再是默认 IM RPC fallback。
7. `awiki-cli` 只把 high-level `ImEvent` 转成本机 host notify/status/log。
8. `crates/awiki-cli/src/runtime/mod.rs` 不再公开已迁走的 listener_ws/session/notification/secure projection 模块。
9. `crates/awiki-cli/src/store/*` 只剩 migration/diagnostic/test gate，或留给 Final 删除。
10. `im-core` 不依赖任何 `awiki-cli` 类型。
```

