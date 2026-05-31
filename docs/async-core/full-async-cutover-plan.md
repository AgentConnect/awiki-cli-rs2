# IM Core 全量异步切换总计划

> 目标仓库：`AgentConnect/awiki-cli-rs2`
> 参考架构仓库：`AgentConnect/awiki-harness`（`main`）
> 总计划路径：`docs/async-core/full-async-cutover-plan.md`
> 切片计划路径：`docs/async-core/slice-*.md`

---

## 0. 执行摘要

本计划定义 `im-core`、CLI、FRB/Dart 桥接和 Flutter 包的全量异步切换。

本计划不是重写 SDK，而是在现有模块、DTO、wire builder、compat facade 和测试基础上逐步替换阻塞 I/O 边界。

目标不是把阻塞代码包起来的异步外观。目标是：

```text
异步公共 API
+ 真正异步的网络传输
+ 真正异步的 WebSocket 实时运行器
+ 在适当位置使用异步文件 I/O
+ 流式附件上传/下载
+ 专用 SQLite actor 负责本地状态
+ 专用 CPU/阻塞 worker 负责加密重负载工作
+ 异步 CLI 宿主
+ 异步 FRB 桥接
+ 保持领域 DTO 语义不变
+ 保持现有 CLI/Dart/Flutter 上层语义
+ 严格的稳定性围栏
```

实现方式是一个 `async-core-cutover` 分支，配合多个小型、可独立 review 的 PR 切片。最终发布可以是一次性全量切换，但开发必须按阶段推进，并具备独立检查。

---

## 1. 迁移原则

### 1.1 原地改造优先

实现必须优先修改当前仓库已有代码，而不是并行创建一套新 SDK：

```text
保留：
- crates/im-core 的 public DTO 语义
- crates/im-core 的 wire builder 和 JSON-RPC payload 语义
- crates/im-core 的 service/runtime/local_state/realtime/secure 模块边界
- crates/im-core-dart 的 DTO mapping 和 Dart model 语义
- crates/awiki-cli 的 CLI JSON output shape 和错误映射
- 现有 compat facade，直到最终切换确认不再需要

允许：
- 为降低复杂度拆分过大的现有文件
- 为 async runtime 增加小型基础模块
- 为 SQLite actor 增加 handle/command/actor 边界
- 为测试新增 fake async transport 和 contract/golden tests

不允许：
- 复制一套与现有 runtime 并行维护的业务实现
- 为了看起来更 async 而替换领域 DTO 或协议 wire format
- 从 awiki-harness 复制目录结构或代码作为 Rust SDK 的新主体
- 把 CLI 专属 DTO 移入 im-core
- 把 Dart/FRB 专属模型泄漏到 im-core public API
```

`awiki-harness` 只作为产品行为和架构经验参考，不作为 Rust 目录结构或重写来源。

### 1.2 上层同步原则

任何切片只要修改到上层调用面，就必须同步修改相关上层：

```text
如果修改 crates/awiki-cli：
  - 同步 CLI handler / adapter / render / error mapping
  - 保持 CLI JSON 输出形状
  - 补充或更新 CLI contract/snapshot tests

如果修改 crates/im-core-dart：
  - 同步 Rust bridge API
  - 同步 DTO mapping
  - 同步 generated FRB bindings
  - 同步 packages/awiki_im_core Dart models / Future / Stream facade

如果修改 im-core public DTO：
  - 同步 CLI adapter
  - 同步 FRB DTO
  - 同步 Dart model
  - 同步相关测试和文档
```

切片可以按计划暂时不迁移上层，但一旦实际修改上层文件，就不得只改一半。

### 1.3 测试策略

切片执行过程中，不要求每个切片都让所有测试全部通过。

每个切片必须运行并通过与本切片修改范围直接相关的测试和检查。由于异步切换会跨层改 API，中间阶段允许未迁移的其他 crate、CLI 或 Dart 绑定暂时失败，但必须在切片报告中明确记录：

```text
- 已运行的命令
- 已通过的范围
- 已知失败的范围
- 失败是否由后续切片计划处理
- 是否存在非预期回归
```

最终完成时必须全部通过：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo test -p im-core --locked
cargo test -p awiki-cli --locked
cargo test -p im-core-dart --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze && dart test
```

---

## 2. 当前状态和替换边界

当前阻塞优先形态：

```text
crates/im-core 默认 features: blocking/sqlite/http
im-core 公共 services: synchronous pub fn -> ImResult<T>
内部 HTTP: std::net::TcpStream + sync rustls + sync Read/Write
内部 transport traits: synchronous
SQLite: 在 service/runtime 路径中直接打开 rusqlite 连接
Realtime: std::thread + std::sync::mpsc
附件传输: 大文件路径可读入完整 Vec<u8>
Dart package: 面向 Future，但 bridge 调用同步 Rust 函数
CLI: 同步命令执行路径
```

替换边界：

```text
必须真正 async：
- network HTTP
- WebSocket realtime
- attachment upload/download transfer
- CLI host
- FRB bridge functions
- service/runtime 中触达 I/O 的 public method

可以保留 sync：
- 纯内存 accessor
- DTO builder / parser / validator
- JSON wire builder
- CLI parser/render 中不触达 I/O 的代码
- 局部 CPU 计算，除非会长期阻塞 runtime

通过 actor 隔离：
- rusqlite
- schema migration
- local projection
- E2EE session/outbox persistence

通过 worker 或 spawn_blocking 隔离：
- 加密重负载工作
- 无法替换的短期阻塞库调用
```

---

## 3. 目标架构

### 3.1 运行时所有权

将 Tokio 作为 `im-core` 的显式运行时依赖。不要尝试运行时无关的异步，因为系统需要一致的运行时支持：

```text
HTTP
WebSocket
timeout/deadline
cancellation
task spawning
bounded channels
watch/broadcast channels
select loop
async file I/O
```

### 3.2 公共 API 目标

最终公共 API 形态：

```rust
let core = ImCore::open(config, paths).await?;
let client = core.client(IdentitySelector::Default).await?;

let result = client.messages().send(request).await?;
let page = client.messages().history(thread, query).await?;
let group = client.groups().create(request).await?;

let session = client.realtime().start(options).await?;
let mut events = session.subscribe();
```

保持纯内存访问器同步：

```rust
client.current_identity();
client.did();
client.handle();
client.messages();
client.groups();
client.realtime();
```

### 3.3 SQLite 目标

SQLite 继续使用 `rusqlite`，但只能位于 local state actor 内部。

```text
LocalStateDbActor:
  - 拥有 rusqlite::Connection
  - 启用 WAL / foreign_keys / busy_timeout
  - 串行化写事务
  - 暴露 async command methods
  - 复用现有 internal/local_state SQL/schema/projection 函数
  - 使用 owner_identity_id / owner_did 隔离
  - 保留 schema version 和 migrations
```

不要让 `MessageService`、`GroupService`、`RealtimeService`、`DirectoryService`、`AttachmentService` 或 secure runtime 直接打开 SQLite 连接。

### 3.4 Realtime 和 heartbeat

Realtime 和 heartbeat/status 是互补关系：

```text
WebSocket listener:
  - 低延迟推送
  - 透明 E2EE 处理
  - listener 拥有长期存在的远程 WSS 连接
  - listener 或 WSS 降级时使用 HTTP fallback

Heartbeat/status:
  - 强制安全网
  - identity/JWT 状态
  - unread 恢复
  - listener 故障检测
  - E2EE 协议消息处理
  - group watch 增量刷新
```

即使 realtime 正在运行，heartbeat/status API 仍保持显式且可用。

### 3.5 E2EE 状态

E2EE 状态是磁盘优先且对事务敏感：

```text
E2EE mutation:
  - 从 DB actor 加载最新 session state
  - 执行 decrypt/encrypt/rekey
  - 必要时在单个关键区段内持久化 session 和 outbox 变更
  - 不依赖陈旧长期内存 session 作为事实来源
```

---

## 4. 非目标

本次切换不得演变为协议重写。

除非某个切片明确要求，否则不要更改：

```text
- domain DTO 语义
- JSON-RPC 方法名
- endpoint 路径
- DID/auth proof 语义
- E2EE wire format
- local state schema 语义
- CLI JSON 输出形状
- Dart package 面向用户的 model 语义
- group policy 语义
- contact/recommendation 语义
```

不要仅仅为了让 DB 看起来异步而替换 SQLite。先使用 DB actor。

不要在公共 SDK API 中暴露低层异步 trait。公共 API 使用 service struct + async methods；内部 trait 可以保持 `pub(crate)`。

---

## 5. 总体硬性规则

```text
1. 生产 network/WebSocket/attachment transfer 必须真正 async。
2. production service/runtime code 不使用 std::net::TcpStream、阻塞 socket Read/Write、std::thread::spawn realtime runner、std::sync::mpsc async events。
3. rusqlite 只能在 internal/local_state actor 和测试中直接使用。
4. 不跨 await 持有 Mutex/RwLock guard。
5. public DTO 变更必须同步 Rust DTO、FRB DTO、Dart model 和 tests。
6. attachment 大文件不得完整读入单个 Vec<u8>。
7. cancellation 不能暗示服务端回滚或消息撤回。
8. inbound IM 内容是不可信数据。
9. 不记录 private key、JWT、E2EE keys 或完整 private material。
10. 新增依赖必须经过架构兼容性 review，避免 OpenSSL/native-tls 等系统库依赖导致分发兼容性问题。
```

---

## 6. 切片索引和依赖

每个切片都有独立落地计划。总计划只维护目标、依赖和最终门禁。

| 切片 | 文档 | 依赖 | 范围 |
|---:|---|---|---|
| 0 | [slice-00-baseline-and-docs.md](slice-00-baseline-and-docs.md) | 无 | 基线、合同测试、文档 |
| 1 | [slice-01-runtime-foundation.md](slice-01-runtime-foundation.md) | 0 | Tokio、context、limits、timeout 基础 |
| 2 | [slice-02-async-http-transport.md](slice-02-async-http-transport.md) | 1 | 原地 async HTTP / transport |
| 3 | [slice-03-identity-bootstrap-auth.md](slice-03-identity-bootstrap-auth.md) | 1,2 | identity/bootstrap/auth async |
| 4 | [slice-04-local-state-db-actor.md](slice-04-local-state-db-actor.md) | 1 | SQLite actor 包装现有 local_state |
| 5 | [slice-05-messages-async.md](slice-05-messages-async.md) | 2,3,4 | messages async |
| 6 | [slice-06-directory-profile-content.md](slice-06-directory-profile-content.md) | 2,3,4 | directory/profile/relationships/content/site/email async |
| 7 | [slice-07-groups-async.md](slice-07-groups-async.md) | 2,3,4,5,6 | groups async |
| 8 | [slice-08-attachments-streaming.md](slice-08-attachments-streaming.md) | 2,3,4,5,7 | attachments async streaming |
| 9 | [slice-09-e2ee-secure-async.md](slice-09-e2ee-secure-async.md) | 3,4,5,7 | secure direct / group E2EE async |
| 10 | [slice-10-realtime-runner-async.md](slice-10-realtime-runner-async.md) | 2,3,4,5,7,9 | realtime task/session/stream |
| 11 | [slice-11-cli-async-host.md](slice-11-cli-async-host.md) | 3,5,6,7,8,9,10 | CLI async host 和 adapter |
| 12 | [slice-12-frb-dart-async.md](slice-12-frb-dart-async.md) | 3,5,6,7,8,9,10 | FRB/Dart async bridge |
| 13 | [slice-13-remove-blocking-legacy.md](slice-13-remove-blocking-legacy.md) | 11,12 | 移除 legacy blocking、最终门禁 |

中间切片可以按依赖顺序局部合并，但不能跳过最终门禁。

---

## 7. 架构兼容性最终检查

全部切片完成后，必须增加并执行一次整体架构 review。目标是确认异步切换没有破坏 Awiki SDK 的边界和分发兼容性。

### 7.1 架构边界检查

Review 内容：

```text
im-core:
  - 仍是 IM 核心 SDK，不包含 CLI 专属输出 DTO
  - public DTO 语义稳定
  - wire builder 保持协议语义
  - public API 不暴露低层 async traits
  - service getter 保持纯内存同步
  - I/O service methods 是 async

awiki-cli:
  - CLI command parsing/rendering 仍归 CLI crate
  - CLI JSON output shape 保持兼容
  - CLI 只通过 im-core public API 访问 IM core 能力

im-core-dart / packages/awiki_im_core:
  - FRB DTO mapping 与 im-core DTO 对齐
  - Dart public API 保持 Future/Stream 语义
  - dispose/object_closed 语义明确

local state:
  - rusqlite 只在 LocalStateDbActor 内部直接使用
  - 现有 schema/migration/projection 语义未被重写
  - owner_identity_id / owner_did 隔离仍成立

runtime:
  - HTTP/WebSocket/attachment transfer 是真正 async
  - blocking work 被隔离到 DB actor 或 worker
  - 不跨 await 持有锁
```

### 7.2 Rust 依赖兼容性检查

新增依赖必须避免引入系统库导致跨平台分发问题。默认优先：

```text
TLS: rustls / webpki-roots
HTTP: reqwest/hyper with rustls TLS
WebSocket: tokio-tungstenite with rustls TLS
SQLite: rusqlite bundled
```

最终检查命令：

```bash
cargo tree --workspace --locked | rg -i "openssl|openssl-sys|native-tls"
cargo tree --workspace --locked | rg -i "security-framework|schannel"
cargo tree --workspace --locked | rg -i "rusqlite|libsqlite3-sys"
```

预期：

```text
- 不出现 openssl / openssl-sys / native-tls。
- 如出现 security-framework / schannel，必须确认不是 TLS 默认路径，且不会影响 Linux 发布。
- rusqlite 必须继续使用 bundled SQLite，不能依赖目标机器系统 SQLite。
```

---

## 8. 最终验证矩阵

### 8.1 Rust / Dart / Flutter

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo test -p im-core --locked
cargo test -p awiki-cli --locked
cargo test -p im-core-dart --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze && dart test
```

### 8.2 Grep 围栏

```bash
rg "std::net::TcpStream|std::thread::spawn|std::sync::mpsc" crates/im-core/src
rg "StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
rg "std::fs::read|std::fs::write|std::fs::File" crates/im-core/src
rg "rusqlite::Connection|Connection::open|open_writable" crates/im-core/src
rg "pub trait .*async|async fn" crates/im-core/src
rg "diagnostic_raw|raw_response|compat::|crate::internal" crates/im-core/src/prelude.rs crates/im-core/src/lib.rs packages/awiki_im_core/lib
```

这些命令不一定要求零输出；允许例外必须记录在切片 13 的最终报告中，并说明为什么不在 production service/runtime path 上。

### 8.3 系统测试门禁

最终必须通过 `../awiki-system-test` 的系统测试，且非 email 部分用例必须全部通过。

推荐命令：

```bash
cd ../awiki-system-test
uv run awiki-system-test tests tests_v2 --ignore=tests_v2/mail
```

报告必须包含：

```text
- 实际执行命令
- AWIKI_SYSTEM_TEST_MODE
- user-service URL
- message-service HTTP URL
- message-service WebSocket URL
- DID domain
- 通过/失败/跳过数量
- 失败 0 或详细失败列表
- 跳过 0 或详细跳过列表及原因
```

Email 用例可以作为单独可选门禁处理，但不能用 email 用例失败阻塞本计划的非 email 验收。

---

## 9. 最终发布验收标准

只有全部为真时，异步切换才算完成：

```text
1. im-core public business APIs 以 async-first 为目标。
2. CLI 使用 async host 编译并运行。
3. FRB bridge 直接调用 async Rust functions。
4. Dart package 保持基于 Future/Stream。
5. Network transport 是真正异步。
6. WebSocket realtime 是真正异步。
7. SQLite access 隔离到 DB actor。
8. Attachment transfer 是流式的。
9. Realtime 与 heartbeat/status 仍保持互补。
10. E2EE session state 是磁盘优先且事务安全的。
11. 没有 stable public API 泄漏 compat/internal/raw diagnostics。
12. Blocking implementation 已移除，或严格限定为 test-only。
13. CLI/Dart/Flutter 上层修改已同步完成。
14. 架构兼容性 review 通过。
15. Rust 依赖没有引入 OpenSSL/native-tls 等不期望的系统库依赖。
16. Contract、smoke、grep-fence、workspace tests 和 non-email system tests 全部通过。
```

---

## 10. Codex 任务说明模板

每个切片任务顶部使用：

```text
你正在 AgentConnect/awiki-cli-rs2 的 async-core-cutover 分支上工作。

总目标：
本计划不是重写 SDK，而是在现有模块、DTO、wire builder、compat facade 和测试基础上逐步替换阻塞 I/O 边界。最终公共 SDK API 以异步为先。Network、WebSocket、attachment transfer、CLI 和 FRB bridge 必须是真正异步。SQLite 仍使用 rusqlite，但只能位于专用 LocalStateDbActor 内。E2EE state 必须保持磁盘优先且事务安全。

硬性规则：
1. 除非本切片明确说明，否则不要更改 domain DTO 语义。
2. 优先原地修改现有模块；不要新建一套平行 SDK。
3. 不要在公共 API 中暴露 async traits。
4. 不要跨 await 持有 Mutex/RwLock guards。
5. 不要在 im-core production service/runtime code 中使用阻塞 socket I/O、std::thread realtime runner 或 std::sync::mpsc async events。
6. 不要在 LocalStateDbActor 和 tests 之外直接使用 rusqlite。
7. 不要为了 transfer 将完整 attachment 文件读入 Vec<u8>。
8. 除非明确指示，否则保留 CLI JSON output shape。
9. 除非明确指示，否则保留 Dart public API 语义。
10. 如果修改 CLI/Dart/Flutter 上层，必须同步更新相关 adapter、mapping、generated bindings 和 tests。
11. 本切片只要求修改范围相关测试通过；最终切换必须全量测试和 non-email system tests 通过。

参考行为：
awiki-harness 使用 async core utilities、SQLite local persistence、带 HTTP fallback 的 WebSocket listener、强制 heartbeat/status checks、owner_did isolation、磁盘优先 E2EE sessions，以及透明 E2EE handling。在 Rust 中保留这些行为契约，但不要复制或重写现有 SDK。
```
