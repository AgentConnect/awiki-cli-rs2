# Async Cutover Stability Review Report

Commit range: `b1e431a..HEAD`
Review date: 2026-05-29
Reviewer: Codex

## Summary

Decision: pass with documented residual risks

本报告按 `docs/async-core/full-async-cutover-review-plan.md` 复核 full async cutover 的稳定性问题。当前结论已经不再是 release blocker：review 初始发现的高风险项已经通过原地修改代码修复，没有重写 SDK，也没有把 CLI/Dart 专属模型泄漏进 `im-core` public API。

当前完成状态：

- 默认非 `blocking` 构建中的同步 HTTP facade 已 fail-closed，不再创建嵌套 Tokio runtime 执行同步 HTTP。
- 默认非 `blocking` 构建中的同步 service/runtime SQLite 写路径已 fail-closed 或 no-op projection；生产 async path 通过 `LocalStateDb` actor 串行访问主 local state SQLite。
- `ImCoreInner::local_state_db()` 已从 mutex-held await 改为 `tokio::sync::OnceCell`，并补并发首次打开测试。
- async realtime session 已保存 `JoinHandle`，`join()` 能区分 task panic / cancel / missing exit，`Drop` 会请求 worker shutdown，并有 focused lifecycle tests。
- realtime event channel 的 bounded backpressure 策略已有 event-buffer-full focused test。
- Group E2EE summary 写入已增加 epoch / `group_state_version` 单调保护，并有 focused tests 防止旧 summary 覆盖新 summary。
- Dart realtime bridge 已补 duplicate stream attach 和 stop/dispose lifecycle tests；event stream worker 使用 weak session 引用，避免 worker 强持有 session。
- awiki.info non-email system tests 已在当前代码上完整通过：`217 passed, 6 skipped, 4 deselected, 0 failed`。

本报告仍保留 residual risks，主要是：

- `feature = "blocking"` 下 legacy sync compatibility path 仍存在，属于显式兼容面，不是默认 production path。
- identity recovery / replace-did / bootstrap 这类本地维护或迁移路径仍有直接 SQLite 访问或 worker-wrapped sync helper；async recovery 已走 actor，但 replace-did plan 仍是 read-only maintenance boundary。
- ANP dedicated Group MLS store 使用独立 `group_mls/<scope>/mls_state.sqlite` 和 file lock，不属于主 `im.sqlite` actor；作为 accepted dedicated MLS store boundary 记录。

## Current Finding Status

### Finding 1: sync HTTP facade in default build

Severity: high
Layer: runtime
Status: resolved

初始问题：默认非 `blocking` 构建中的 `HttpClient::execute()` 会创建 current-thread Tokio runtime 并 `block_on(self.execute_async(...))`。

当前修复：

- `crates/im-core/src/internal/http.rs` 中真正同步 TCP/rustls HTTP implementation 已限制在 `#[cfg(feature = "blocking")]`。
- 默认非 `blocking` 构建中 `HttpClient::execute()` 返回 `ImError::UnsupportedCapability { capability: "sync-http" }`。
- async path 继续使用 `reqwest` + rustls。

验证：

- `cargo check --workspace --locked` passed。
- `cargo test --workspace --locked` passed。
- `cargo tree --workspace --locked | rg -i "openssl|openssl-sys|native-tls|rustls|webpki|tokio|reqwest|tungstenite"` 未发现 `openssl` / `openssl-sys` / `native-tls`，只看到 `reqwest`、`tokio`、`rustls`、`webpki`、`tokio-tungstenite` 等 async/rustls 依赖。

### Finding 2: default sync service methods directly opening SQLite

Severity: high
Layer: storage
Status: resolved for default production path

初始问题：Directory / Secure Outbox 等同步 service methods 在默认 `sqlite` feature 下直接打开 SQLite，绕过 `LocalStateDb` actor。

当前修复：

- `crates/im-core/src/directory/service.rs`
  - `save_contact()`、`contacts()`、`relation_status()` 等 sync methods 只在 `all(feature = "sqlite", feature = "blocking")` 下直连 contact store。
  - 默认非 `blocking` 构建返回 `sync-directory-*` unsupported。
  - async methods 使用 `local_state_db().await?` actor path。
- `crates/im-core/src/secure/service.rs`
  - `SecureOutboxService::{list_failed,retry,drop}` sync methods 只在 `blocking` 下直连 SQLite。
  - 默认非 `blocking` 构建返回 `sync-secure-outbox` unsupported。
  - async methods 通过 actor commands 操作 outbox。

验证：

- `cargo test --workspace --locked` passed。
- `cargo test -p im-core --features group-e2ee --locked` passed。
- Focused coverage includes:
  - `directory_relationship_sync_methods_fail_closed_by_default`
  - `directory_service_async_uses_actor_projection_and_resolution_api`
  - `secure_service_sync_methods_fail_closed_by_default`
  - `secure_outbox_async_failed_retry_drop_uses_db_actor`

### Finding 3: runtime/projection helpers directly opening SQLite

Severity: high
Layer: storage
Status: resolved for default production path

初始问题：message/group/realtime/secure runtime helpers 保留默认编译的 direct SQLite helpers。

当前修复：

- message projection helpers in `crates/im-core/src/internal/message_runtime/local_projection.rs`:
  - sync helpers with direct SQLite are `blocking` or `test` only.
  - default non-blocking sync helpers return `sync-message-projection`.
  - async helpers use `LocalStateDb` actor.
- mark-read/conversations helpers:
  - default sync direct SQLite path returns `sync-message-mark-read` / `sync-message-conversations`.
  - async classify/list/mark-read path uses actor.
- group runtime projection/cache:
  - sync cache/projection direct SQLite path is `blocking` or `test` only.
  - default sync path is no-op for best-effort projection or returns unsupported where a result is required.
  - async path uses actor.
- secure direct helpers:
  - sync status/prepare/repair/send/incoming paths are `blocking` or `test` gated where they touch SQLite.
  - default non-blocking sync paths fail closed.
  - async send/receive/prepare/status use actor and CAS where state mutation is involved.
- realtime local projection:
  - blocking projection can use direct SQLite.
  - async projection stores message/group/contact updates through actor.

验证：

- `cargo test --workspace --locked` passed。
- `cargo test -p im-core --features group-e2ee --locked` passed。
- Focused coverage includes:
  - `deferred_direct_e2ee_success_projection_uses_db_actor`
  - `deferred_direct_e2ee_pending_outbox_uses_db_actor`
  - `mark_read_runtime_async_marks_direct_remote_and_actor_local_rows`
  - `realtime_async_local_state_projector_uses_db_actor_for_message_projection`
  - `realtime_async_projector_uses_actor_cas_for_direct_cipher`
  - `public_group_e2ee_send_async_uses_async_transport_and_db_actor_projection`

### Finding 4: first local_state_db open awaited under mutex

Severity: medium
Layer: runtime
Status: resolved

初始问题：`ImCoreInner::local_state_db()` 持有 `tokio::sync::Mutex` guard 等待 actor open ready signal。

当前修复：

- `crates/im-core/src/core/mod.rs` 改为 `tokio::sync::OnceCell<LocalStateDb>`。
- `get_or_try_init(...)` 负责并发首次初始化；不再使用 mutex-held await。
- Added `local_state_db_concurrent_first_open_shares_actor`。

验证：

- `cargo test --workspace --locked` passed。
- `cargo test -p im-core --features group-e2ee --locked` passed。

### Finding 5: realtime JoinHandle not retained

Severity: medium
Layer: realtime
Status: resolved

初始问题：async realtime runner `tokio::spawn` 后 `RealtimeSession` 不保存 `JoinHandle`，panic / cancel / drop-without-join 语义不可观察。

当前修复：

- `crates/im-core/src/realtime/session.rs` 中 `RealtimeSession` 保存 `Option<JoinHandle<()>>`。
- `join()` 等待 exit oneshot 后继续 await worker；若 exit channel closed，则通过 worker join error 区分 panic/cancel/missing exit。
- `Drop` 仍请求 shutdown。
- `spawn_default_async()` 将 worker handle 传入 session。

验证：

- `realtime_session_drop_requests_worker_shutdown_without_join`
- `realtime_session_join_reports_worker_panic`
- `cargo test --workspace --locked` passed。

### Finding 6: realtime event buffer full behavior undocumented/untested

Severity: medium
Layer: realtime
Status: resolved

当前策略：

- `TokioRunnerEvents::emit()` 使用 bounded `tokio_mpsc::Sender::try_send`。
- event buffer full 或 receiver closed 时，runner 记录 warning，并以 `RealtimeExitReason::ConnectionClosed` 退出。
- 这是有意的 bounded backpressure 策略：慢消费者不会造成内存无界增长。

验证：

- `realtime_async_runner_exits_when_event_buffer_is_full`
- `realtime_async_runner_uses_tokio_channels_and_status_watch`
- `realtime_async_runner_stops_on_shutdown_signal`

### Finding 7: attachment streaming memory behavior

Severity: low
Layer: attachments
Status: accepted-risk

当前结论：

- Local file upload uses `tokio::fs::File` + `ReaderStream` / `reqwest::Body::wrap_stream`。
- Download-to-file uses chunked async response + async atomic write temp file + final rename/link.
- Download-to-memory intentionally aggregates object bytes into `Vec<u8>` because that API returns in-memory bytes; this is accepted API semantics, not a regression.

验证：

- `attachments_upload_runtime_local_file_async_streams_explicit_path`
- `attachments_upload_runtime_local_file_reads_only_explicit_path`
- `attachments_download_runtime_local_file_async_streams_to_file`
- `cargo test --workspace --locked` passed。

### Finding 8: Group E2EE summary older state can overwrite newer state

Severity: medium
Layer: e2ee
Status: resolved

初始问题：async Group E2EE lifecycle/repair summary 使用 actor 写入，但底层 `upsert_group()` 对 metadata 没有 epoch/version 单调保护。

当前修复：

- `crates/im-core/src/internal/local_state/groups.rs` 增加 `GroupE2eeSummaryRecord` 和 `upsert_group_e2ee_summary()`。
- 写入前读取现有 group snapshot metadata，并调用 `should_apply_group_e2ee_summary(...)`。
- 新 summary 只有在 epoch 更高、或没有 epoch 时 `group_state_version` 单调推进时才覆盖。
- 缺失 version 的旧 summary 不会覆盖已有 version。
- `crates/im-core/src/internal/group_e2ee/summary.rs` async path 调用 actor 的 `upsert_group_e2ee_summary(...)`。

验证：

- `group_e2ee_summary_upsert_does_not_revert_to_older_epoch`
- `group_e2ee_summary_upsert_uses_version_when_epoch_is_missing`
- `group_e2ee_summary_upsert_rejects_missing_version_over_existing_version`
- `group_e2ee_summary_upsert_accepts_missing_version_for_empty_cache`
- `cargo test -p im-core --features group-e2ee --locked` passed。

### Finding 9: dedicated ANP Group MLS store is outside LocalStateDb actor

Severity: low
Layer: e2ee
Status: accepted-risk

当前结论：

- Group MLS native provider uses ANP `ImCoreSqliteGroupMlsStore` under `local/group_mls/<scope>/mls_state.sqlite`。
- 该 store 是 dedicated MLS state boundary，不是主 `im.sqlite` service/runtime path。
- im-core async Group E2EE operations invoke MLS work through worker isolation; ANP store uses scoped file lock semantics.

验证：

- `native_provider_for_client_uses_identity_scoped_im_core_store`
- `async_resolver_uses_worker_isolated_mls_status_and_async_transport`
- `cargo test -p im-core --features group-e2ee --locked` passed。

Residual risk:

- 还没有单独增加 ANP MLS lock contention focused test。该风险为 dedicated store accepted-risk，不能重新归类为主 LocalStateDb actor violation。

### Finding 10: Dart realtime stream lifecycle

Severity: medium
Layer: dart
Status: resolved

初始问题：Dart realtime event stream bridge spawn worker 后不保存 JoinHandle，stream attach / stop / sink close lifecycle 缺少测试。

当前修复：

- `DartRealtimeSession` 用 `Mutex<Option<RealtimeSession>>` 管理 session ownership。
- event stream 只能 attach 一次。
- `stop()` take session and await core stop。
- `Drop` take/drop session，触发 core session shutdown。
- event stream worker uses `Arc::downgrade(session)`，sink add 失败时只在 session 还活着时 stop，不强持有 session。

验证：

- `dart_realtime_session_allows_only_one_event_receiver`
- `dart_realtime_stop_disposes_session_handle`
- `cargo test -p im-core-dart --locked` via workspace passed。
- `cd packages/awiki_im_core && dart test` passed。
- `cd packages/awiki_im_core && dart analyze` passed。
- `timeout 600 env CARGO_NET_OFFLINE=true scripts/flutter/codegen-check.sh` passed。

### Finding 11: awiki.info Group E2EE publish-key-package proof failure

Severity: high
Layer: e2ee-system
Status: resolved in current system-test evidence

初始问题：早期完整 non-email suite 两次在 Group E2EE `publish-key-package` 处失败，service 返回 `did_wba_binding verification failed: Invalid proof value encoding`。

当前结果：

- 当前代码重新运行 awiki.info non-email suite 后未复现该问题。
- Group E2EE contract/local/system CLI path 已通过。
- Final system-test result: `217 passed, 6 skipped, 4 deselected, 0 failed`。

验证命令：

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
E2E_DID_DOMAIN=awiki.info \
E2E_MESSAGE_V2_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_V2_NODE_A_DOMAIN=awiki.info \
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=https://awiki.info \
E2E_MESSAGE_V2_NODE_A_RPC_URL=https://awiki.info/im/rpc \
E2E_MESSAGE_V2_NODE_A_WS_URL=wss://awiki.info/im/ws \
E2E_MESSAGE_V2_NODE_A_SERVICE_DID=did:wba:awiki.info \
E2E_MESSAGE_V2_NODE_B_DOMAIN=awiki.info \
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=https://awiki.info \
E2E_MESSAGE_V2_NODE_B_RPC_URL=https://awiki.info/im/rpc \
E2E_MESSAGE_V2_NODE_B_WS_URL=wss://awiki.info/im/ws \
E2E_MESSAGE_V2_NODE_B_SERVICE_DID=did:wba:awiki.info \
AWIKI_ENABLE_GROUP_E2EE_TESTS=1 \
AWIKI_GROUP_E2EE_CONTRACT_TEST=1 \
AWIKI_ENABLE_MAIL_TESTS=0 \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/async-sdk-refactor \
CARGO_BUILD_JOBS=1 \
CARGO_PROFILE_TEST_DEBUG=0 \
uv run awiki-system-test tests tests_v2 --ignore=tests_v2/mail -k 'not mail and not email' -rs
```

## Async Flow And Concurrency Review

### Await-lock inventory

- No broad pattern of holding `MutexGuard` / `RwLockGuard` across `.await` was found in the reviewed runtime/service paths.
- The previous `local_state_db()` first-open mutex-held await was replaced with `OnceCell`.
- DB transactions are held inside actor/store synchronous execution, not across async `.await` boundaries.

### Spawned-task lifecycle inventory

- Realtime async runner:
  - `spawn_default_async()` stores the worker `JoinHandle` in `RealtimeSession`.
  - `join()` observes exit receiver and worker result.
  - `Drop` requests shutdown.
- Dart realtime stream:
  - event stream worker uses weak session reference.
  - duplicate receiver attach fails.
  - stop/dispose closes the underlying core session handle.
- LocalStateDb actor:
  - dedicated thread owns `rusqlite::Connection`.
  - async callers communicate through bounded command channel + oneshot replies.

### Channel/backpressure inventory

- LocalStateDb command channel is bounded.
- Realtime event channel is bounded by `RealtimeOptions.event_buffer`.
- Realtime status uses `watch`, keeping latest status only.
- Realtime exit uses `oneshot`.
- Event buffer full is fail-closed with warning and `ConnectionClosed`, not unbounded buffering.

### Flow traces

Message send:

```text
public async send -> validate/build wire -> optional secure/group E2EE encrypt on worker
-> remote async RPC -> actor local projection/outbox update -> typed result
```

Concurrency invariant:

- async local projection uses actor serialization.
- direct secure state mutation uses revision CAS.
- group E2EE local summary uses epoch/version monotonic guard.

Message receive:

```text
history/inbox or realtime event -> classify/decrypt/normalize -> actor persist
-> projection visible through history/inbox/local cache -> returned event/page
```

Concurrency invariant:

- decrypt state updates go through actor/CAS for direct secure.
- group E2EE incoming normalization uses async worker + projection path and redacts raw cipher material on output.

Secure direct:

```text
session/prekey load -> worker crypto -> async RPC publish/send/receive
-> actor save session with revision CAS -> outbox/projection update
```

Concurrency invariant:

- stale direct secure session revision is rejected.
- pending cipher replay and init session are actor-backed.

Group E2EE:

```text
lifecycle/repair/status/send -> service head/local state_ref resolution
-> MLS worker operation -> async transport side effect -> actor summary/projection
```

Concurrency invariant:

- local summary cannot revert to older epoch/version.
- service-first resolver ignores stale local snapshot when service head is newer.
- dedicated MLS store is scoped and lock-protected outside main `im.sqlite`.

Realtime:

```text
start_async -> auth token preflight -> tokio worker connect/read loop
-> bounded event channel/status watch -> actor-backed local projection
-> stop/drop/join shutdown observation
```

Concurrency invariant:

- slow event consumer causes bounded disconnect, not memory growth.
- panic/missing exit is visible through `join()`.

Attachment transfer:

```text
local file upload -> tokio file stream -> reqwest body stream -> commit/send
download-to-file -> chunked response -> temp file async write -> atomic finalization
```

Concurrency invariant:

- file sink uses temp path and final rename/link.
- memory sink intentionally materializes bytes.

LocalStateDb actor:

```text
caller awaits command send -> actor thread owns connection and transaction/helper call
-> oneshot reply -> caller resumes
```

Concurrency invariant:

- `rusqlite::Connection` is not shared across async tasks.
- actor serializes commands and owner scopes writes by identity/did.

## Grep Fence Classification

`std::net::TcpStream|StreamOwned|std::io::Read|std::io::Write`

- Allowed: `feature = "blocking"` legacy HTTP/realtime compatibility.
- Allowed: small file/config/credential maintenance reads.
- Finding status: no default async HTTP fallback to blocking socket path.

`open_writable|rusqlite::Connection::open`

- Allowed actor/store internals:
  - `crates/im-core/src/internal/local_state/actor.rs`
  - `crates/im-core/src/internal/local_state/*`
  - local store helpers invoked by actor.
- Allowed tests/fixtures.
- Allowed blocking compatibility:
  - sync Directory / SecureOutbox / message/group/realtime projection helper paths under `feature = "blocking"` or `test`.
- Allowed maintenance/migration boundary:
  - identity recovery merge sync fallback.
  - replace-did plan read-only local-state counting.
  - bootstrap sync schema init under `feature = "blocking"`; default sync bootstrap fails closed.
- Accepted dedicated MLS store:
  - ANP group MLS `group_mls/<scope>/mls_state.sqlite` with scoped file lock.
- Finding status: no default production async service/runtime path bypasses `LocalStateDb` actor for main local state writes.

`tokio::spawn|mpsc|watch|oneshot`

- Allowed:
  - realtime runner worker with retained `JoinHandle`.
  - bounded event channel.
  - status watch.
  - exit oneshot.
  - LocalStateDb actor command channel + reply oneshots.
- Finding status: lifecycle/backpressure findings resolved by code and tests.

`lock().await`

- Previous `local_state_db()` mutex-held await resolved with `OnceCell`.
- No new blocking finding opened from current grep review.

TLS/runtime dependencies:

- Allowed: `tokio`, `reqwest`, `hyper-rustls`, `rustls`, `rustls-webpki`, `webpki-roots`, `tokio-rustls`, `tokio-tungstenite`, `tungstenite`.
- Not found in fence output: `openssl`, `openssl-sys`, `native-tls`.

## Verification Matrix

Rust and workspace:

- `git diff --check`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --locked`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace --locked`: passed.
- `cargo test -p im-core --features group-e2ee --locked`: passed.

Dart / FRB:

- `cd packages/awiki_im_core && dart test`: passed, 7 tests.
- `cd packages/awiki_im_core && dart analyze`: passed, no issues.
- `timeout 600 env CARGO_NET_OFFLINE=true scripts/flutter/codegen-check.sh`: passed, `Done!`.

System tests:

- Target: `awiki.info`
- Mail/email exclusion:
  - `AWIKI_ENABLE_MAIL_TESTS=0`
  - `--ignore=tests_v2/mail -k 'not mail and not email'`
- Result: `217 passed, 6 skipped, 4 deselected, 0 failed`
- Elapsed: `551.36s / 0:09:11`

System-test skipped items:

- `tests_v2/cli/test_awiki_cli_store_rust_contracts.py:117`: Rust store contract targets were removed from awiki-cli-rs2; store internals are no longer an awiki-cli acceptance surface.
- `tests_v2/message_service/test_direct_local.py:324`: requires local tests_v2 topology.
- `tests_v2/message_service/test_group_e2ee_flag_off.py:43`: flag-off guard requires `AWIKI_GROUP_E2EE_CONTRACT_TEST` to be unset.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:92`: requires `E2E_MESSAGE_V2_DID_ONLY_DOMAIN`.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:132`: requires `E2E_MESSAGE_V2_DID_ONLY_DOMAIN`.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:191`: requires `E2E_MESSAGE_V2_MESSAGE_ONLY_DID`.

## API And Boundary Review

Architecture boundary:

- `im-core` public facade does not expose low-level async transport traits or `LocalStateDbActor`.
- CLI parsing/rendering and JSON envelope remain in `awiki-cli`.
- Dart top-level facade file `packages/awiki_im_core/lib/awiki_im_core.dart` is not changed by this review fix set.
- Dart model public facade semantics are not intentionally changed; generated bridge includes realtime status support.

CLI:

- CLI host is async-first for command execution.
- Sync shell compatibility entry exists but is CLI boundary, not SDK runtime rewrite.
- CLI JSON output compatibility is covered by awiki-cli contract tests in `cargo test --workspace --locked`.

Dart:

- FRB bridge uses async im-core APIs for runtime operations.
- Realtime session status/event stream lifecycle has Rust-side tests and Dart package stub tests.

ANP / `anp/anp/rust`:

- No changes were made in `/home/ecs-user/awiki-space/anp/anp/rust` during this fix pass.
- The only ANP-related risk is documented dedicated MLS store behavior consumed by im-core.

## Residual Risks And Follow-up

Residual risks accepted for this cutover:

- Blocking compatibility feature remains a supported legacy surface. It must not be enabled for the default async production path without understanding the sync I/O and direct SQLite semantics.
- Dedicated ANP MLS store lock contention lacks a focused contention test in this repo. Current coverage proves identity-scoped store construction and worker-isolated MLS status usage, but not simultaneous lock contention behavior.
- Replace-did plan still uses a read-only direct SQLite count helper. It is a local maintenance/planning boundary and is worker-wrapped for async API, but it is not actor-backed.
- Download-to-memory for attachments materializes the object into memory by API design.

Suggested follow-up tests:

- ANP Group MLS store lock contention test for `state_locked` / retryable behavior.
- Attachment cancellation test confirming temp file cleanup during interrupted download-to-file.
- Replace-did plan async actor-backed read-only count, if the project wants a stricter "all local state SQLite through actor" rule even for maintenance planning commands.

## Release Recommendation

The code is no longer blocked by the review findings in this report. The current evidence supports proceeding with release review, subject to the residual risks above being accepted as explicit compatibility or maintenance boundaries.

Do not treat old awiki.info `publish-key-package` failures as current evidence: the current full non-email awiki.info suite passed with failure 0 on 2026-05-29.
