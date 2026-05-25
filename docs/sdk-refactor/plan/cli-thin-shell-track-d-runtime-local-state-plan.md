# Track D: Runtime Runner and Local State Ownership Cleanup

**并行分支**：`cutover/thin-shell-track-d-runtime-local-state`  
**依赖**：可直接开始；最终删除 shared store/module 需等 Final。  
**详细边界图**：`docs/sdk-refactor/plan/cli-thin-shell-track-d-runtime-boundary-map.md`  
**目标**：让 `runtime listener run/service-run` 只宿主 `im-core` realtime runner；IM notification classify、message/group/contact projection、secure ack/outbox/replay 等底层逻辑归 `im-core`。`awiki-cli` runtime 只保留 service manager、pid/log/socket、shutdown、host notify delivery。

---

## 1. 范围

本 track 处理：

```text
runtime.listener.run
runtime.listener.service-run
runtime listener ImEvent handling
runtime local notification / host notification bridge boundary
contact/message/group local projection
secure notification normalize / ack / outbox flush / replay
awiki-cli store wrappers used by runtime
```

主要文件：

```text
crates/awiki-cli/src/runtime/listener_supervisor_run.rs
crates/awiki-cli/src/runtime/listener_im_event_adapter.rs
crates/awiki-cli/src/runtime/listener_contact_sync.rs
crates/awiki-cli/src/runtime/listener_notification_*.rs
crates/awiki-cli/src/runtime/listener_message_records.rs
crates/awiki-cli/src/runtime/listener_secure_*.rs
crates/awiki-cli/src/runtime/listener_session_*.rs
crates/awiki-cli/src/runtime/listener_ws*.rs
crates/awiki-cli/src/store/*
crates/im-core/src/realtime/*
crates/im-core/src/internal/realtime/*
crates/im-core/src/internal/local_state/*
crates/im-core/src/internal/contact_store/*
crates/im-core/src/internal/store/*
crates/im-core/tests/*realtime*
crates/im-core/tests/*local_state*
```

不处理：

```text
systemd / launchd / Windows service manager deletion
OpenClaw / Hermes host notify config deletion
msg/group command output shape
identity/auth command behavior
mail/page/site command behavior
```

---

## 2. 边界目标

`im-core` 负责：

```text
WebSocket connect / heartbeat / reconnect
auth refresh for realtime
raw notification classify
notification -> ImEvent
message/group/contact projection
local_state owner isolation
attachment enrichment when supported
secure direct incoming decrypt / ack planning
secure outbox flush/retry state
group E2EE notice processing when supported
```

`awiki-cli` runtime 负责：

```text
run foreground process
service-run process
install/start/stop/restart/uninstall service process
pid/log/socket/status files
shutdown signal bridge
OpenClaw/Hermes host notify sink delivery
host notify config and secrets
CLI status rendering
```

---

## 3. 执行步骤

### D1. Runner invocation 固定

`runtime listener run/service-run` 必须最终等价于：

```text
build ImClient
build RealtimeOptions
build ShutdownSignal
client.realtime().run_until_shutdown(options, shutdown)
consume high-level ImEvent only for CLI-owned host notify/status
```

禁止：

```text
old listener session loop
manual websocket frame classify in awiki-cli
manual notification JSON route as default runner
```

检查：

```bash
rg "listener_session_loop|connect_realtime_with_transport|handle_listener_notification|listener_wsclient|listener_ws_transport" \
  crates/awiki-cli/src/runtime
```

这些应被删除、迁到 im-core、或只留 internal/migration test 不在 run/service-run path。

### D2. ImEvent adapter 缩小

当前 `listener_im_event_adapter` 做 store open/schema/projection。目标缩到：

```text
ImEvent::HostNotification / LocalNotification -> CLI host notify sink
ConnectionStateChanged -> CLI status file, if needed
UnknownNotification warnings -> CLI status warning, if not already in im-core RealtimeExit warnings
```

不再做：

```text
message local projection
group upsert
contact sync
secure ack/outbox
attachment manifest parsing
```

如果需要 host notify event 内容，应由 `im-core` event DTO 提供足够高层字段。

### D3. Contact/message/group projection 下沉

把这些 CLI runtime helper 的业务逻辑迁入 `im-core` internal local_state/realtime：

```text
listener_contact_sync.rs
listener_message_records.rs
listener_notification_plan.rs
listener_notification_execute.rs
runtime direct store_message/upsert_group/upsert_group_member calls
```

CLI 侧只保留 host notify handle display fallback，如果这是 UX 需求；DID/handle lookup 和 contact projection应通过 `client.directory()` 或 `im-core` projection 完成。

### D4. Secure runtime 下沉

迁移或删除：

```text
listener_secure_normalize.rs
listener_secure_ack_delivery.rs
listener_secure_ack_in_process.rs
listener_secure_outbox_flush.rs
listener_secure_replay.rs
listener_secure_sessions.rs
listener_secure_sync.rs
listener_secure_inbox_poll.rs
```

归属：

```text
secure notification classify -> im-core secure/realtime internal
incoming decrypt projection -> im-core secure direct/group E2EE internal
ack planning/delivery -> im-core secure internal, exposed as high-level event/result only
outbox flush -> im-core secure outbox service/internal runtime
```

### D5. Store module普通 IM路径断引用

检查：

```bash
rg "crate::store::|use crate::store\\b|store::open|store::ensure_schema|store_message|upsert_group|upsert_contact" \
  crates/awiki-cli/src/runtime \
  crates/awiki-cli/src/app \
  crates/awiki-cli/src/im_core_adapter
```

允许短期残留：

```text
doctor
debug.db.*
id.import-v1
upgrade migration
Final 删除前的 compat wrapper tests
```

不允许残留：

```text
runtime listener run/service-run normal path
msg/group/people normal path
mail/page/site normal path
```

### D6. Service manager 保留

不要迁移：

```text
runtime/listener_systemd.rs
runtime/listener_launchd.rs
runtime/listener_windows_service.rs
runtime/listener_service.rs
runtime/bridge.rs if it manages CLI daemon socket
runtime/openclaw_*.rs
runtime/hermes_*.rs
runtime/host_notify*.rs
```

但要确认这些文件不直接承担 IM business projection。

---

## 4. 验证

最小验证：

```bash
cargo test -p im-core
cargo check -p awiki-cli
rg "crate::store::|use crate::store\\b|store_message|upsert_group|upsert_contact" \
  crates/awiki-cli/src/runtime \
  crates/awiki-cli/src/app \
  crates/awiki-cli/src/im_core_adapter
```

推荐测试：

```bash
cargo test -p im-core --test realtime_contract
cargo test -p im-core --test local_state_contract
cargo test -p awiki-cli --test runtime_contract
cargo test -p awiki-cli --test runtime_listener_contract
```

如果 runner 需要 live websocket，live/system 测试不作为本 track 默认 blocker；记录未运行原因。

---

## 5. 完成定义

本 track 完成后：

```text
1. runtime listener run/service-run 只宿主 im-core realtime runner。
2. runtime 不再直接写 message/group/contact local_state。
3. secure runtime 底层 normalize/ack/outbox/replay 已迁入 im-core 或默认 unsupported。
4. CLI store 在默认 runtime path 中断引用。
5. Final 可以安全删除 store 普通 IM projection wrapper 和旧 runtime listener internals。
```
