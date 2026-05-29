# Full Async Cutover 代码审查计划

> 目标变更：`0c06859 Implement full async core cutover`
> 参考计划：`docs/async-core/full-async-cutover-plan.md`
> 目标：为大规模 async cutover 变更制定可由 Codex goal 执行的稳定性 review 方案。

---

## 0. 审查目标

本次 review 不是普通格式审查，也不是只看编译是否通过。目标是验证这次庞大修改后的系统稳定性，确认 async cutover 没有引入协议漂移、状态损坏、并发死锁、数据丢失、内存膨胀、公开 API 破坏或测试覆盖盲区。

核心问题：

```text
1. 这次修改是否仍然是原地 async 改造，而不是平行重写 SDK？
2. im-core public DTO、wire payload、CLI JSON output、Dart model 语义是否保持稳定？
3. HTTP、WebSocket、attachment transfer、CLI host、FRB bridge 是否真正 async？
4. SQLite 是否隔离在 LocalStateDb actor 和 tests 内？
5. E2EE 状态是否仍然磁盘优先、事务安全、不会重复应用或丢失状态？
6. realtime、heartbeat/status、HTTP fallback 的稳定性是否保持？
7. private key、JWT、E2EE key、plaintext 等敏感信息是否不会泄漏到日志或 raw output？
8. 测试、grep fence、系统测试是否覆盖最高风险路径？
```

审查完成标准：

```text
- 每个 review 层级都有明确结论。
- 每个发现都有文件、行号、影响、建议和测试覆盖说明。
- 所有高风险问题已修复或明确阻塞发布。
- non-email awiki.info system tests 最终 failure 0。
- 所有 skip 都有明确原因，不能把产品回归隐藏成 skip。
```

---

## 1. 审查范围

### 1.1 主审查范围

以 async cutover commit 为主线审查：

```bash
git show --stat --oneline 0c06859
git diff --stat b1e431a..HEAD
git diff --name-status b1e431a..HEAD
```

重点目录：

```text
Cargo.toml
Cargo.lock
crates/im-core
crates/awiki-cli
crates/im-core-dart
packages/awiki_im_core
scripts/flutter/codegen.sh
docs/async-core
```

### 1.2 单独审查范围

系统测试仓库变更要单独审查，不要和主仓库代码稳定性混在一起：

```text
/home/ecs-user/awiki-space/awiki-system-test
```

这部分重点看测试环境策略，例如远端 registration limit、OTP 过期、Group E2EE 测试开关自动配置等，不能用它替代主仓库产品代码 review。

### 1.3 非目标

除非已有明确需求，否则 review 不应推动以下变化：

```text
- 重命名 JSON-RPC 方法
- 改 endpoint 路径
- 改 DID/auth proof 语义
- 改 E2EE wire format
- 改 CLI JSON output shape
- 改 Dart public model 字段和 enum 语义
- 把 CLI DTO 移入 im-core
- 把 FRB/Dart 专属模型泄漏进 im-core public API
```

发现这些变化时，默认视为稳定性风险，需要证明它们是有计划、有测试覆盖的兼容变更。

---

## 2. Review 执行总方案

本次修改文件多、范围大，不适合按文件名线性读。使用四层审查：

```text
Layer A：架构边界审查
Layer B：稳定性核心运行时审查
Layer C：领域行为兼容审查
Layer D：验证门禁和发布就绪审查
```

执行顺序：

```text
1. 先做风险地图，确认哪些文件属于哪些风险层。
2. 先审架构边界，防止方向错误。
3. 再审 runtime/storage/E2EE/realtime 等高风险稳定性逻辑。
4. 再审 CLI、FRB/Dart、测试兼容面。
5. 最后跑完整门禁和 awiki.info non-email 系统测试。
```

Codex goal 执行约束：

```text
1. 每个 goal 只处理一个审查层或一个高风险域。
2. 每次开始先记录 git status。
3. 先 review 后修改；发现问题先记录证据，再修复。
4. 修复后只先跑相关 focused tests，最终再跑全量 gates。
5. 不运行 email 系统测试，除非用户明确要求。
6. 不把 skipped 测试写成 passed；skip 必须列明原因。
```

---

## 3. Layer A：架构边界审查

### 3.1 目标

确认本次修改仍符合 `full-async-cutover-plan.md` 的基本原则：

```text
- 原地改造优先。
- 不引入一套平行业务实现。
- im-core 仍是核心 SDK。
- CLI parsing/rendering 仍归 awiki-cli。
- Dart package public facade/model 语义稳定。
- internal transport/runtime/actor 不泄漏到 public API。
```

### 3.2 重点文件

```text
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
crates/im-core/src/core/mod.rs
crates/im-core/src/core/bootstrap.rs
crates/im-core/src/internal/mod.rs
crates/awiki-cli/src/main.rs
crates/awiki-cli/src/lib.rs
crates/im-core-dart/src/api/mod.rs
packages/awiki_im_core/lib/awiki_im_core.dart
packages/awiki_im_core/lib/src/models/*.dart
```

### 3.3 检查命令

```bash
rg "crate::internal|compat::|diagnostic_raw|raw_response" \
  crates/im-core/src/lib.rs \
  crates/im-core/src/prelude.rs \
  packages/awiki_im_core/lib

rg "pub trait .*async|pub.*Async.*Transport|pub.*LocalStateDbActor" crates/im-core/src
rg "CLI|JsonEnvelope|ExitError" crates/im-core/src
git diff --name-status b1e431a..HEAD -- packages/awiki_im_core/lib/awiki_im_core.dart packages/awiki_im_core/lib/src/models
```

### 3.4 重点判断

必须确认：

```text
- im-core public API 不暴露 low-level async traits。
- prelude/lib 不暴露 internal runtime、transport、DB actor。
- CLI JSON envelope/error mapping 没有移到 im-core。
- Dart 顶层 facade 没有无意改 export。
- Dart model 字段和 enum 没有无计划漂移。
- 新增 docs/async-core 文档和实际代码方向一致。
```

高优先级发现：

```text
- 公开 API 泄漏 internal 模块。
- CLI 专属 DTO 进入 im-core。
- Dart generated bridge 变化影响 stable facade。
- DTO/wire payload 变化没有对应兼容测试。
```

---

## 4. Layer B：稳定性核心逻辑审查

Layer B 是本次 review 的重点。先审这些，再审 CLI/Dart 上层。

### 4.1 Runtime Foundation

重点文件：

```text
crates/im-core/src/internal/runtime/context.rs
crates/im-core/src/internal/runtime/limits.rs
crates/im-core/src/internal/runtime/timeout.rs
crates/im-core/src/internal/runtime/worker.rs
crates/im-core/Cargo.toml
Cargo.toml
Cargo.lock
```

检查点：

```text
- timeout 默认值是否有限且合理。
- cancellation 是否没有暗示服务端回滚或撤回消息。
- blocking worker 是否不会无界膨胀。
- run_blocking 错误是否能正确传播。
- 新依赖是否避免 openssl/native-tls。
```

命令：

```bash
cargo tree --workspace --locked | rg -i "openssl|openssl-sys|native-tls"
cargo tree --workspace --locked | rg -i "rustls|webpki|tokio|reqwest|tungstenite"
```

### 4.2 异步流程和并发风险专项审查

这是 async cutover 的专项稳定性审查。不要只按模块确认“已经改成 async”，必须确认异步化没有引入流程乱序、并发覆盖、任务泄漏、背压失效、取消语义漂移或状态重复应用。

检查点：

```text
- await 边界：
  - 是否跨 await 持有 Mutex/RwLock guard。
  - 是否跨 await 持有 DB transaction。
  - 是否跨 await 持有可变 session state 并在 await 后写回旧状态。
  - 是否跨 await 持有 CLI/runtime 全局可变状态。

- 任务生命周期：
  - tokio::spawn 的 JoinHandle 是否被保存、观察或能通过 shutdown 停止。
  - cancellation、stop、drop 是否会停止后台任务。
  - task panic 是否会被观察到或转成 exit/error。
  - shutdown 是否会丢 event、丢 outbox flush、丢 DB write。

- channel 和 backpressure：
  - mpsc/broadcast/watch channel 是否 bounded 或有明确丢弃策略。
  - receiver drop 后 sender 是否退出或降级。
  - slow consumer 是否可能导致内存无界增长。
  - event stream 是否会阻塞 realtime reader。

- 顺序和幂等：
  - send -> local projection -> remote ack 顺序是否稳定。
  - receive -> decrypt -> persist -> projection 是否不会重复应用。
  - retry、repair、reconnect 是否不会重复发送、重复 decrypt、重复 mark-read。
  - group lifecycle add/remove/leave/rejoin 是否不会乱序。

- 并发状态竞争：
  - 同一个 identity 多个 client 并发操作是否会覆盖 local state。
  - 同一个 secure session 并发 send/receive 是否会覆盖 session state。
  - group E2EE state_ref 是否会被旧状态覆盖新状态。
  - mark-read、history、inbox 并发是否保持最终一致。

- 阻塞隔离：
  - crypto-heavy work 是否在 worker 中执行。
  - worker 中是否没有持有 actor transaction。
  - async runtime 上是否没有直接大文件同步读写。
  - production path 是否没有 std::thread/std::sync::mpsc 旧 runner。

- 错误传播和取消语义：
  - timeout/cancel 是否不会伪造成功。
  - cancel 后是否不会留下半完成 final file。
  - dropped future 是否不会导致 DB actor 半写。
  - retryable/non-retryable 错误分类是否稳定。
```

命令：

```bash
rg -n "Mutex|RwLock|lock\\(|await|tokio::spawn|JoinHandle|mpsc|broadcast|watch|oneshot|select!|timeout|CancellationToken|run_blocking|spawn_blocking" \
  crates/im-core/src \
  crates/awiki-cli/src \
  crates/im-core-dart/src

rg -n "transaction|BEGIN|COMMIT|ROLLBACK|session|state_ref|outbox|retry|repair|mark_read|projection" \
  crates/im-core/src/internal
```

输出必须包括：

```text
- await-lock inventory
- spawned-task lifecycle inventory
- channel/backpressure inventory
- retry/idempotency risk list
- cancellation/shutdown risk list
- required focused tests
```

流程级审查输出必须包括：

```text
- async flow trace:
  entrypoint -> awaited calls -> state mutation -> persistence -> remote side effect -> returned result。
- concurrency invariant:
  该流程依赖的顺序保证、串行化点、幂等 key 或版本判断。
- race window review:
  哪些 await 前后可能被其他任务穿插，是否会用旧状态覆盖新状态。
- cancellation review:
  future 被 drop、timeout、shutdown、receiver closed 时，是否会留下半写状态或伪成功。
- backpressure review:
  channel 满、consumer 慢、sender/receiver drop 时，是否 bounded、丢弃、降级或退出。
- focused test mapping:
  每个高风险流程至少映射到 focused test、system test，或明确写 accepted-risk。
```

必须单独 trace 的高风险异步流程：

```text
- message send:
  compose/sign/encrypt -> local projection/outbox -> remote send/ack -> retry/reconcile。
- message receive:
  fetch/ws event -> decrypt -> persist -> projection -> mark-read/history visibility。
- secure direct:
  session load -> prekey/keypackage publish -> send/receive -> repair -> state persist。
- group E2EE:
  create/add/remove/leave/rejoin -> epoch/state_ref update -> summary cache -> incoming apply。
- realtime:
  connect -> reader task -> event channel -> local projection -> stop/drop/join/shutdown。
- attachment transfer:
  streaming read/write -> digest -> temp file -> atomic rename -> cancellation cleanup。
- LocalStateDb actor:
  request enqueue -> actor transaction -> response -> shutdown/error propagation。
```

### 4.3 Async HTTP 和 Authenticated Transport

重点文件：

```text
crates/im-core/src/internal/http.rs
crates/im-core/src/internal/transport.rs
crates/im-core/src/internal/json_rpc.rs
crates/im-core/src/internal/auth/session.rs
crates/im-core/src/internal/discovery/did_document.rs
```

检查点：

```text
- production HTTP 路径是否使用 async client。
- auth challenge retry 是否保持原有 JWT capture/persist 语义。
- DID proof / request signing payload 是否保持兼容。
- JSON-RPC method、endpoint、params shape 是否不漂移。
- state-changing request 的 retry 是否可能造成重复 mutation。
- 错误和日志中是否不会泄漏 JWT/private key。
```

命令：

```bash
rg "std::net::TcpStream|StreamOwned|std::io::Read|std::io::Write" \
  crates/im-core/src/internal/http.rs \
  crates/im-core/src/internal/transport.rs

rg "Authorization|jwt|private_key|private key|redact|debug|trace" \
  crates/im-core/src/internal/http.rs \
  crates/im-core/src/internal/transport.rs \
  crates/im-core/src/internal/auth
```

### 4.4 LocalStateDb Actor 和 SQLite 隔离

重点文件：

```text
crates/im-core/src/internal/local_state/actor.rs
crates/im-core/src/internal/local_state/mod.rs
crates/im-core/src/internal/local_state/schema.rs
crates/im-core/src/internal/message_runtime/local_projection.rs
crates/im-core/src/internal/secure_direct/sqlite_store.rs
```

检查点：

```text
- rusqlite::Connection 是否只在 actor 内部或 tests 中直接使用。
- schema migration 是否兼容旧 workspace。
- owner_identity_id / owner_did 隔离是否仍成立。
- 写入是否串行化。
- 多表状态更新是否在事务中。
- actor shutdown 时是否可能静默丢写。
- DB actor transaction 中是否不会执行网络 I/O 或重 CPU 加密。
```

命令：

```bash
rg "rusqlite::Connection|Connection::open|open_writable|open_readonly" crates/im-core/src
rg "owner_identity_id|owner_did|transaction|BEGIN|COMMIT|ROLLBACK" \
  crates/im-core/src/internal/local_state \
  crates/im-core/src/internal/message_runtime \
  crates/im-core/src/internal/secure_direct
```

输出分类：

```text
allowed-actor-internal
allowed-test
allowed-blocking-feature
finding
```

### 4.5 Attachment Streaming

重点文件：

```text
crates/im-core/src/attachments/service.rs
crates/im-core/src/internal/attachment_runtime/upload.rs
crates/im-core/src/internal/attachment_runtime/download.rs
crates/im-core/src/internal/attachment_runtime/atomic_write.rs
crates/im-core/src/internal/attachment_runtime/temp_file.rs
crates/im-core/src/internal/blob/source.rs
```

检查点：

```text
- LocalFile upload/download 是否流式处理。
- 大文件是否不会完整读入 Vec<u8>。
- digest 计算是否不会强制加载整文件。
- 临时文件是否原子 rename。
- cancellation/failure 是否不会留下错误 final path。
- destination path 语义是否和旧行为兼容。
```

命令：

```bash
rg "read_to_end|std::fs::read|Vec<u8>|tokio::fs|ReaderStream|ByteStream" \
  crates/im-core/src/internal/attachment_runtime \
  crates/im-core/src/attachments
```

### 4.6 Realtime Runner 和 WebSocket

重点文件：

```text
crates/im-core/src/realtime/runner.rs
crates/im-core/src/realtime/session.rs
crates/im-core/src/realtime/service.rs
crates/im-core/src/internal/realtime/async_ws_transport.rs
crates/im-core/src/internal/realtime/transport.rs
crates/im-core/src/internal/realtime/local_projection.rs
```

检查点：

```text
- production WebSocket 是否使用 Tokio async transport。
- session stop 是否幂等。
- task 是否会泄漏。
- event channel 是否 bounded 或有明确 backpressure 策略。
- reconnect/fallback 行为是否保持。
- heartbeat/status 是否仍然独立可用。
- inbound event 是否作为不可信输入处理。
```

命令：

```bash
rg "std::thread::spawn|std::sync::mpsc|tokio::spawn|broadcast|watch|mpsc" \
  crates/im-core/src/realtime \
  crates/im-core/src/internal/realtime

rg "heartbeat|status|fallback|inbox|history" \
  crates/im-core/src/realtime \
  crates/im-core/src/internal/realtime \
  crates/im-core/src/messages/service.rs
```

### 4.7 E2EE 和 Secure State

这是最高风险层，必须单独 review。

重点文件：

```text
crates/im-core/src/secure/service.rs
crates/im-core/src/internal/secure_direct/async_send.rs
crates/im-core/src/internal/secure_direct/async_receive.rs
crates/im-core/src/internal/secure_direct/incoming.rs
crates/im-core/src/internal/secure_direct/outbox.rs
crates/im-core/src/internal/secure_direct/prepare.rs
crates/im-core/src/internal/secure_direct/send.rs
crates/im-core/src/internal/secure_direct/status.rs
crates/im-core/src/internal/group_e2ee/lifecycle.rs
crates/im-core/src/internal/group_e2ee/repair.rs
crates/im-core/src/internal/group_e2ee/status.rs
crates/im-core/src/internal/group_e2ee/incoming.rs
crates/im-core/src/internal/group_e2ee/state_ref.rs
crates/im-core/src/internal/group_e2ee/runtime.rs
crates/im-core/src/internal/group_e2ee/notices.rs
crates/im-core/src/internal/group_e2ee/summary.rs
```

检查点：

```text
- mutation 前是否从 DB actor 加载最新 session state。
- encrypt/decrypt/rekey 后是否事务性持久化 state/outbox/projection。
- pending confirmation、retry、repair、incoming 是否不会重复应用。
- group add/remove/leave/rejoin 是否走 secure-required lifecycle。
- key package publish/update/recover 是否保持 DID-WBA binding/proof 语义。
- 加密重负载是否在 worker 中执行，且不持有 DB transaction。
- plaintext/private key/E2EE material 是否不会进入日志、raw_response、debug output。
```

命令：

```bash
rg "run_blocking|spawn_blocking|transaction|session|outbox|state_ref" \
  crates/im-core/src/internal/secure_direct \
  crates/im-core/src/internal/group_e2ee

rg "private_key|plaintext|ciphertext|jwt|raw_response|debug|trace" \
  crates/im-core/src/internal/secure_direct \
  crates/im-core/src/internal/group_e2ee \
  crates/im-core/src/secure/service.rs
```

---

## 5. Layer C：领域行为和兼容性审查

### 5.1 Identity / Auth / Directory / Profile / Site / Email

重点文件：

```text
crates/im-core/src/identity/service.rs
crates/im-core/src/identity/registry.rs
crates/im-core/src/auth/service.rs
crates/im-core/src/directory/service.rs
crates/im-core/src/internal/directory_runtime.rs
crates/im-core/src/internal/profile_runtime.rs
crates/im-core/src/internal/relationship_runtime.rs
crates/im-core/src/content/service.rs
crates/im-core/src/site/service.rs
crates/im-core/src/email/service.rs
crates/im-core/src/internal/email_runtime/mod.rs
```

检查点：

```text
- handle/DID resolution 行为是否保持。
- identity registry 写入是否原子。
- auth session 持久化是否兼容旧 workspace。
- profile/site/content DTO 字段和默认值是否稳定。
- email 代码变化是否不会阻塞当前 non-email 验收。
```

建议 focused tests：

```bash
cargo test -p im-core phase1c_identity_auth --locked
cargo test -p im-core phase2_identity_directory --locked
cargo test -p im-core phase2_relationship_directory --locked
cargo test -p im-core content_api --locked
cargo test -p im-core site_api --locked
```

### 5.2 Messages / Groups

重点文件：

```text
crates/im-core/src/messages/service.rs
crates/im-core/src/internal/message_runtime/direct.rs
crates/im-core/src/internal/message_runtime/group.rs
crates/im-core/src/internal/message_runtime/read.rs
crates/im-core/src/internal/message_runtime/mark_read.rs
crates/im-core/src/internal/message_runtime/conversations.rs
crates/im-core/src/groups/service.rs
crates/im-core/src/internal/group_runtime/lifecycle.rs
crates/im-core/src/internal/group_runtime/read.rs
crates/im-core/src/internal/group_runtime/projection.rs
crates/im-core/src/internal/group_runtime/cache.rs
```

检查点：

```text
- send/history/inbox/mark-read RPC method 和 payload 是否稳定。
- direct/group projection 的排序和 read 状态是否稳定。
- group create/add/remove/leave policy 是否稳定。
- member DID/handle resolution 是否使用正确 async directory path。
- retry/duplicate delivery 是否保持幂等。
```

建议 focused tests：

```bash
cargo test -p awiki-cli --test msg_im_core_mvp_contract --locked
cargo test -p awiki-cli --test group_contract --locked
cargo test -p awiki-cli --test group_live_contract --locked
```

### 5.3 CLI async host 兼容性

重点文件：

```text
crates/awiki-cli/src/main.rs
crates/awiki-cli/src/lib.rs
crates/awiki-cli/src/cli_shell.rs
crates/awiki-cli/src/cli_parser/mod.rs
crates/awiki-cli/src/cli_shell/*_handlers.rs
crates/awiki-cli/src/m_core_cli_adapter/*.rs
crates/awiki-cli/src/host_runtime/*.rs
```

检查点：

```text
- execute() 和 execute_async() exit code 语义是否一致。
- dispatch_async 是否覆盖所有已支持命令。
- dry-run 是否不会执行 live async calls。
- internal/hidden command gate 是否仍然有效。
- CLI JSON output shape 和 error envelope 是否兼容。
- listener foreground/service lifecycle 是否可控退出。
```

命令：

```bash
rg "dispatch_async|execute_async|dry_run|render_success|handle_error|unsupported" crates/awiki-cli/src
cargo test -p awiki-cli --test cli_parser_contract --locked
cargo test -p awiki-cli --test m_core_cli_adapter_policy_contract --locked
cargo test -p awiki-cli --test cli_cutover_command_surface_contract --locked
```

### 5.4 FRB / Dart / Flutter 兼容性

重点文件：

```text
crates/im-core-dart/src/api/*.rs
crates/im-core-dart/src/dto/*.rs
crates/im-core-dart/src/frb_generated.rs
packages/awiki_im_core/lib/awiki_im_core.dart
packages/awiki_im_core/lib/src/models/*.dart
packages/awiki_im_core/lib/src/generated/*.dart
packages/awiki_im_core/pubspec.yaml
scripts/flutter/codegen.sh
```

检查点：

```text
- Rust bridge 是否直接调用 async SDK APIs。
- Dart public facade 是否仍是 Future/Stream 语义。
- Dart model 字段和 enum 语义是否保持。
- generated 文件是否来自 codegen，而不是手改。
- object_closed/dispose 语义是否确定。
- internal diagnostic/raw transport type 是否没有泄漏到 Dart model。
```

命令：

```bash
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze && dart test
```

如果当前环境需要 offline：

```bash
CARGO_NET_OFFLINE=true scripts/flutter/codegen-check.sh
```

---

## 6. Layer D：验证门禁和发布就绪

### 6.1 Rust 门禁

最终必须运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo test -p im-core --locked
cargo test -p awiki-cli --locked
cargo test -p im-core-dart --locked
```

执行 Cargo 命令前，先确认没有并发 Cargo 任务：

```bash
ps -ef | rg 'cargo metadata|cargo check|cargo test|cargo fmt|cargo clippy|cargo tree|target/debug/deps/im_core|target/debug/deps/awiki_cli' || true
```

### 6.2 Grep Fence 门禁

运行并分类输出：

```bash
rg "std::net::TcpStream|std::thread::spawn|std::sync::mpsc" crates/im-core/src
rg "StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
rg "std::fs::read|std::fs::write|std::fs::File" crates/im-core/src
rg "rusqlite::Connection|Connection::open|open_writable" crates/im-core/src
rg "diagnostic_raw|raw_response|compat::|crate::internal" crates/im-core/src/prelude.rs crates/im-core/src/lib.rs packages/awiki_im_core/lib
```

分类规则：

```text
allowed-test
allowed-blocking-feature
allowed-actor-internal
allowed-generated
finding
```

任何 `finding` 都必须进入 review report。

### 6.3 awiki.info non-email 系统测试门禁

最终使用 `awiki.info` 跑 non-email 系统测试。除非用户明确要求，不跑 email 相关用例。

推荐命令：

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
uv run awiki-system-test tests tests_v2 --ignore=tests_v2/mail -rs
```

报告必须包含：

```text
- 实际命令
- AWIKI_SYSTEM_TEST_MODE
- user-service URL
- message-service HTTP URL
- message-service WebSocket URL
- DID domain
- passed 数量
- failed 数量
- skipped 数量
- elapsed time
- 失败用例明细
- skip 用例明细和原因
```

### 6.4 发布准入标准

只有以下条件都满足，才建议通过：

```text
1. 架构边界无 high/critical finding。
2. production network/WebSocket 没有未隔离阻塞路径。
3. SQLite direct access 只存在于 actor internal、tests 或明确 cfg-gated legacy。
4. E2EE send/receive/repair/lifecycle 通过 focused review 和 focused tests。
5. CLI JSON compatibility tests 通过。
6. FRB/Dart codegen 和 Dart tests 通过，或有明确环境 blocker。
7. awiki.info non-email system tests failure 0。
8. 所有 skip 都有合理原因，且不是产品回归。
```

---

## 7. Codex Goal 执行计划

### Goal 1：建立风险地图

目标：

```text
按文件和模块建立风险地图，确定 review 顺序。
```

步骤：

```bash
git status --short --branch
git diff --stat b1e431a..HEAD
git diff --name-status b1e431a..HEAD
```

输出：

```text
- 文件分组
- 每组风险等级
- review 顺序
- 计划执行的测试命令
```

### Goal 2：架构边界审查

目标：

```text
确认 public API、CLI、Dart、internal 边界没有破坏。
```

步骤：

```text
1. 检查 lib/prelude/core/CLI/Dart facade。
2. 运行 Layer A grep fences。
3. 记录 public surface 变化。
```

输出：

```text
- findings with file:line
- boundary compatibility decision
- required follow-up tests
```

### Goal 3：Runtime 和 Storage 审查

目标：

```text
审查 async HTTP、transport、runtime worker、LocalStateDb actor 和 SQLite 隔离。
```

步骤：

```text
1. 检查 runtime foundation。
2. 检查 HTTP/authenticated transport。
3. 执行异步流程和并发风险专项审查。
4. 检查 LocalStateDb actor 和调用点。
5. 分类 grep fence 输出。
```

输出：

```text
- blocking-path inventory
- SQLite direct-access inventory
- lock-across-await risk notes
- spawned-task lifecycle inventory
- channel/backpressure inventory
- retry/idempotency risk list
- findings and fixes required
```

### Goal 4：Messages / Groups / Attachments 审查

目标：

```text
审查 message、group、attachment 行为兼容性。
```

步骤：

```text
1. 对比 async service methods 和旧 sync 语义。
2. 检查 projection/read/mark-read 幂等性。
3. 检查 group lifecycle policy branches。
4. 检查 streaming upload/download 内存行为。
```

输出：

```text
- behavior compatibility findings
- large-file memory risk assessment
- focused test results
```

### Goal 5：E2EE 和 Realtime 审查

目标：

```text
审查 secure direct、group E2EE、realtime 等最高风险状态路径。
```

步骤：

```text
1. 检查 direct secure send/receive/retry/repair。
2. 检查 group E2EE lifecycle/status/repair/incoming。
3. 检查 realtime session lifecycle、event stream、fallback。
4. 检查 logging/redaction 和 inbound untrusted data handling。
```

输出：

```text
- E2EE state-transition findings
- realtime task leak/backpressure findings
- security logging findings
- focused test results
```

### Goal 6：CLI 和 FRB/Dart 审查

目标：

```text
确认上层 async 迁移后仍保持兼容。
```

步骤：

```text
1. 检查 CLI execute/dispatch/handler/adapter。
2. 验证 dry-run 和 internal command gates。
3. 检查 im-core-dart async bridge 和 generated bindings。
4. 运行 CLI/Dart focused tests。
```

输出：

```text
- CLI JSON compatibility findings
- Dart facade/model compatibility findings
- codegen verification result
```

### Goal 7：最终验证门禁

目标：

```text
运行最终本地门禁和 awiki.info non-email 系统测试，输出稳定性结论。
```

步骤：

```text
1. 运行 Rust fmt/clippy/check/test gates。
2. 运行 grep fences 并分类输出。
3. 运行 Dart/Flutter gates。
4. 运行 awiki.info non-email system tests。
5. 产出最终稳定性报告。
```

输出：

```text
- pass/fail/skip table
- exact commands
- failure details
- skip details
- residual risks
- release recommendation
```

---

## 8. Finding 记录格式

每个发现使用以下格式：

```text
Severity: critical | high | medium | low
Layer: architecture | runtime | storage | domain | e2ee | realtime | cli | dart | tests
File: path:line
Issue:
Impact:
Evidence:
Recommendation:
Test coverage:
Status: open | fixed | accepted-risk
```

严重级别定义：

```text
critical:
  数据丢失、private material 泄漏、auth bypass、E2EE state corruption、
  production deadlock、non-email system tests 无法运行。

high:
  public API 兼容破坏、CLI JSON shape 破坏、大附件无界内存、
  SQLite direct access 越过 actor、重复发送消息、丢 realtime event、
  production blocking network path。

medium:
  focused test 缺失、错误映射不清晰、retry 语义漂移、shutdown leak 风险、
  generated binding drift。

low:
  文档不一致、命名混淆、非关键清理。
```

---

## 9. 最终 Review Report 模板

Codex goal 的最终报告使用以下结构：

```text
# Async Cutover Stability Review Report

Commit range:
Review date:
Reviewer:

## Summary

Decision: approve | approve-with-fixes | block

## Commands Run

- command:
  result:
  log:

## Findings

- Severity:
  File:
  Issue:
  Status:

## Grep Fence Classification

- fence:
  allowed:
  findings:

## Test Matrix

- Rust fmt:
- Rust clippy:
- Rust workspace check:
- Rust workspace tests:
- im-core:
- awiki-cli:
- im-core-dart:
- Flutter codegen:
- Dart analyze/test:
- awiki.info non-email system tests:

## System Test Details

- mode:
- user-service:
- message-service HTTP:
- message-service WS:
- DID domain:
- passed:
- failed:
- skipped:
- elapsed:
- failures:
- skips:

## Residual Risk

## Release Recommendation
```

---

## 10. 审查 Checklist

```text
[ ] 风险地图已生成。
[ ] 架构 public boundary 已审查。
[ ] 依赖和 TLS 兼容性已检查。
[ ] async HTTP/auth transport 已审查。
[ ] LocalStateDb actor 和 SQLite isolation 已审查。
[ ] attachment streaming 已审查。
[ ] realtime runner/session 已审查。
[ ] identity/auth/directory/profile/site/email 已审查。
[ ] messages/groups 已审查。
[ ] secure direct 已审查。
[ ] group E2EE 已审查。
[ ] CLI async host 和 command compatibility 已审查。
[ ] FRB/Dart bridge 和 generated files 已审查。
[ ] grep fences 已运行并分类。
[ ] Rust gates 已运行。
[ ] Dart/Flutter gates 已运行。
[ ] awiki.info non-email system tests 已运行。
[ ] findings 已 triage，修复或 accepted-risk。
[ ] final stability report 已写。
```
