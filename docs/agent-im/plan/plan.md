# Plan：Agent IM MVP 可落地实施方案

状态：done
DOC：`awiki-cli-rs2/docs/agent-im/plan`  
Harness：`awiki-harness`  
创建时间：2026-06-09  
恢复指针：Step 01-09 均已完成；若恢复核验，只需读取第 7、15、17 节和 Step 09 文档，并核对最终提交与工作区状态。

## 1. 目标

- 任务目标：把 `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` 与 `awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` 收敛为一个可执行 MVP 实施计划，覆盖 `awiki-cli-rs2`、`awiki-me`、`user-service`、`message-service` 与 ANP SDK / `im-core` 兼容扩展。
- 预期行为：APP 在创建用户 DID Document 时默认本地生成 `user_did#daemon-key-1` 子私钥，并把对应 public verification method 交给 user-service 写入 DID Document；APP 通过 message-service 普通消息发送一次性 `awiki.daemon.bootstrap.v1` system/control payload，MVP body 是明文 JSON，把本地既有子私钥和 `desired_message_agent` 交给 Daemon；Daemon 幂等创建或复用专门处理 APP 普通消息的 Hermes Message Agent；Daemon 使用 user delegated identity 收发普通非 E2EE 消息，并通过最小 APP action allowlist 与 APP 打通。
- 非目标：MVP 不实现 Agent DID delegation / ANP delegated proof；不实现独立 APP ↔ Daemon pairing channel、本地 RPC、局域网通道或第二条传输链路；不实现 bootstrap 普通消息 body 加密；不实现 E2EE 明文、摘要或 metadata 转发给 Agent；不实现完整自动化能力配置、撤销和审计 UI；不把用户主私钥或 E2EE private state 交给 Daemon 或 Runtime。
- 完成标准：所有 Step 已实现、验证、Review、聚焦提交并回填台账；最终全局 Review 完成；系统测试在 `awiki-system-test` remote `awiki.info` 模式记录实际命令、通过/失败/跳过数量、失败或跳过原因和关键环境配置。

## 2. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | Harness 控制面入口与跨仓库约束。 |
| `awiki-harness/README.md` | Harness 使用方式、仓库协作方式。 |
| `awiki-harness/context/00-context-map.md` | 上下文索引。 |
| `awiki-harness/context/02-repo-map.md` | AWiki 多仓库定位。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 跨仓库架构边界。 |
| `awiki-harness/context/20-rules-index.md` | 规则索引。 |
| `awiki-harness/context/30-tools-env.md` | 工具与环境要求。 |
| `awiki-harness/context/40-verification.md` | 验证分层和证据记录。 |
| `awiki-harness/context/50-task-workflow.md` | 任务执行、Review、提交流程。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | Daemon / Runtime Host 路由依据。 |
| `awiki-harness/context/nodes/message-flow.node.md` | 消息流和服务边界依据。 |
| `awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | `awiki-cli-rs2` 仓库职责、测试入口。 |
| `awiki-harness/context/repo-profiles/awiki-me.md` | APP 仓库职责、Flutter 验证入口。 |
| `awiki-harness/context/repo-profiles/message-service.md` | message-service 职责、协议和 Rust workspace 验证入口。 |
| `awiki-harness/context/repo-profiles/user-service.md` | user-service 职责、DID / auth / inventory 入口。 |
| `awiki-cli-rs2/AGENTS.md` | 本 DOC 下规划文档必须中文；最终系统测试必须使用 remote `awiki.info` 并记录证据。 |

## 3. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| 核心设计文档 | 作为需求来源和契约来源，不在本计划中继续修改 | `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md`、`awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` |
| user-service DID Document public method 管理 | 创建用户 DID Document 时只登记 APP 侧生成的 `user_did#daemon-key-1` public verification method；支持从 DID Document 写入、移除或替换该 public method；不生成、不接收、不返回 daemon subkey private material；不作为 message-service MVP 运行时授权来源 | `user-service/src/user_service/app/did/*`、`user-service/src/user_service/storage/sqlmodel/models/did.py`、`user-service/docs/api/*` |
| ANP SDK / `im-core` | 新增向后兼容 optional params，支持 delegated signing 与 delegated inbox/history；老调用行为不变 | `awiki-cli-rs2/crates/im-core/src/messages/*`、`awiki-cli-rs2/crates/im-core/src/internal/proof/origin.rs`、`awiki-cli-rs2/crates/im-core/src/internal/wire/inbox.rs` |
| Dart binding | 如 Rust DTO/API 变更，重新生成并暴露 Dart optional 参数 | `awiki-cli-rs2/packages/awiki_im_core/lib/src/generated/*`、`awiki-cli-rs2/packages/awiki_im_core/lib/src/awiki_im_core_base.dart` |
| awiki-deamon bootstrap | 新增 APP 普通消息 bootstrap payload 解析、明文 key package 存储、幂等状态、user delegated identity profile | `awiki-cli-rs2/crates/awiki-deamon/src/*` |
| awiki-deamon Message Agent | 一次性 `ensure_app_message_agent`，持久化 `app_message_agent_binding`，绑定 runtime agent 与 user delegated inbox/send policy | `awiki-cli-rs2/crates/awiki-deamon/src/runtime/*`、`awiki-cli-rs2/crates/awiki-deamon/src/plugins/hermes/*`、`awiki-cli-rs2/crates/awiki-deamon/src/state/*` |
| awiki-deamon user delegated inbox sync | durable cursor、processed message、普通非 E2EE 投递给绑定 Agent，E2EE opaque notification 丢弃或标记不可处理 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/runtime_inbox.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/inbox/mod.rs` |
| awiki-me | APP 读取 DID 创建时已有 `#daemon-key-1`，一次性通过普通消息发送 bootstrap/session payload；隐藏系统 payload；展示 bootstrap 与 message agent 状态 | `awiki-me/lib/src/application/agent/agent_control_service.dart`、`awiki-me/lib/src/data/im_core/*`、`awiki-me/lib/src/domain/entities/agent/*`、`awiki-me/lib/src/presentation/agents/*` |
| message-service | 接受 `user_did#daemon-key-1` 对普通消息 send/inbox/history 的 proof；同 DID 多连接 fanout；E2EE boundary 不破坏；MVP 只校验 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope | `message-service/crates/*/src`、`message-service/bins/message-service/src`、`message-service/docs/api/*`、`message-service/docs/architecture/*` |
| APP action / control schema | 新增或收敛 `awiki.daemon.bootstrap.v1`、`awiki.message.sync.v1`、`awiki.app.capabilities.v1`、`awiki.app.action.v1`、`awiki.app.action.result.v1`；控制 payload 不显示为普通聊天 | `awiki-cli-rs2/crates/awiki-deamon/src/*`、`awiki-me/lib/src/domain/entities/chat_message.dart`、`awiki-me/lib/src/domain/entities/agent/*` |
| 系统测试 | 覆盖 APP bootstrap -> Daemon -> ensure message agent -> delegated inbox/send -> app sync/action | `awiki-system-test` |

## 4. 假设与开放问题

### 假设

- `user_did#daemon-key-1` 在 MVP 中被放入用户 DID Document 的 `authentication`；一个 APP 默认只有一个 active daemon key，fragment 不包含设备信息。
- APP 创建 DID Document 的流程可升级为调用最新 user-service / DID API，由 APP 本地生成 daemon subkey private package，只把 public verification method 交给 user-service；bootstrap 不再追加修改 DID Document。
- APP 和 Daemon 之间只有一个通信通道：message-service 承载的普通消息发送。bootstrap、status、message.sync、app.action、app.action.result 都是这条通道上的 system/control payload。
- MVP bootstrap 明文传递子私钥是已接受安全债；private package 通过普通消息明文 JSON 路由给 Daemon，message-service 不理解其私钥语义。后续可以把同一普通消息 body 改为加密文本或加密 JSON envelope。实现必须禁止普通聊天 UI、日志、prompt、runtime temp、审计详情泄露 key material。
- Hermes Runtime 不直接持有用户子私钥，只通过 Daemon local RPC / runtime token 调用 send、inbox、action 能力。
- message-service 的 WebSocket DID fanout 可以支持同一 user DID 的 APP 与 Daemon 两个或多个连接；普通消息与 E2EE opaque notification 都可下发，Daemon 自行丢弃 E2EE opaque notification。

### 开放问题

- user-service 当前 DID 创建 API 的具体入参和返回结构需要在 Step 01 实现前确认；计划要求使用可选 public registration 参数或兼容默认值，避免破坏旧客户端。
- message-service 当前 direct/history/inbox API 的具体路径和 crate 边界需要在 Step 07 实现前由执行者根据 `message-service/docs/api/*` 与 `message-service/crates/*/src` 校准。
- `awiki-system-test` 的实际命令参数可能与本文示例不同；Step 09 必须以 `awiki-system-test/README.md` 或当前脚本帮助为准，并在台账记录实际命令。

## 5. 总体设计方法

- 设计边界：MVP 做 user delegated subkey 收发普通非 E2EE 消息和 APP message handler agent；长期 Agent DID delegation、ANP delegated proof、同一普通消息发送路径上的 bootstrap body 加密、secure key store、E2EE Agent participant 只记录后续方向。
- 关键决策：新增命名统一使用 `inbox_owner_did`、`inbox_auth_verification_method`、`inbox_auth_key_ref`、`inbox_auth`、`ScopedInboxToken`、`InboxHistoryOptions`；不再使用容易与普通消息所有权或 email/mail 概念混淆的旧候选命名。
- 兼容性策略：ANP SDK / `im-core` 只新增 optional params；老的 send/inbox/history 调用不传参数时行为完全不变。服务端 API 新增字段也必须 optional 或新增 endpoint，避免破坏老客户端。
- 数据、协议、配置或迁移策略：Step 01 先冻结 `DaemonDelegatedKeyPublicRegistration`、`DaemonSubkeyPrivatePackage`、`DelegatedKeyRegistryRecord` 三个 schema fixture；Daemon 新增 user delegated identity、bootstrap replay、message agent binding、inbox cursor、processed message、message event/outbox 等持久状态；user-service 和 message-service 如需数据库字段或表，使用迁移并保留撤销/审计字段。
- 风险控制：禁止用户主私钥进入 Daemon；MVP 不处理 E2EE 明文/摘要/metadata；runtime token scope 与 APP action allowlist 双重限制；durable cursor 和 processed message 是上线前置，不是优化项。

## 6. 任务拆分

| Step | 标题 | 依赖 | 产出 | 小 Plan 文档 | Commit gate | 状态 |
|---|---|---|---|---|---|---|
| 01 | user-service DID delegated subkey | 无 | APP 侧生成 daemon key；user-service 登记 public key；冻结 key package/schema fixture；registry/revoke/query 契约 | [steps/01-user-service-did-delegated-subkey.md](steps/01-user-service-did-delegated-subkey.md) | 必须 | done |
| 02 | ANP SDK / im-core optional params | Step 01 契约草案 | delegated signing 与 `InboxHistoryOptions` optional API | [steps/02-im-core-delegated-signing-inbox-options.md](steps/02-im-core-delegated-signing-inbox-options.md) | 必须 | done |
| 03 | awiki-deamon bootstrap 与 user delegated identity state | Step 01、Step 02 契约 | 从普通消息发送接收 `awiki.daemon.bootstrap.v1` 明文 JSON body、明文 key package 存储、幂等状态 | [steps/03-awiki-deamon-bootstrap-state.md](steps/03-awiki-deamon-bootstrap-state.md) | 必须 | done |
| 04 | awiki-deamon message agent binding | Step 03 | `ensure_app_message_agent` 与 `app_message_agent_binding` | [steps/04-awiki-deamon-message-agent-binding.md](steps/04-awiki-deamon-message-agent-binding.md) | 必须 | done |
| 05 | awiki-deamon delegated inbox sync | Step 02、Step 04、Step 07 契约 | durable cursor、processed message、普通消息投递、E2EE opaque ignore | [steps/05-awiki-deamon-user-delegated-inbox-sync.md](steps/05-awiki-deamon-user-delegated-inbox-sync.md) | 必须 | done |
| 06 | awiki-me bootstrap UI 与 service | Step 01、Step 03、Step 04 | APP 一次性 bootstrap、状态展示、控制 payload 隐藏 | [steps/06-awiki-me-pairing-bootstrap-ui-service.md](steps/06-awiki-me-pairing-bootstrap-ui-service.md) | 必须 | done |
| 07 | message-service delegated key policy 与 fanout | Step 01 契约、Step 02 契约 | delegated send/inbox/history proof、scope policy、多连接 fanout | [steps/07-message-service-delegated-key-policy-and-fanout.md](steps/07-message-service-delegated-key-policy-and-fanout.md) | 必须 | done |
| 08 | APP action schema 与可见性 | Step 04、Step 06 | 最小 action allowlist、schema、payload 过滤和 result 回传 | [steps/08-app-action-schema-and-visibility.md](steps/08-app-action-schema-and-visibility.md) | 必须 | done |
| 09 | 系统测试与集成收口 | Step 01-08 | remote `awiki.info` 系统测试、全局 Review、文档证据 | [steps/09-system-test-and-integration.md](steps/09-system-test-and-integration.md) | 如修改测试/文档则必须 | done |

## 7. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | 分支 | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T10:06:33Z | 2026-06-09T10:38:54Z | `user-service` `b3f4c59` | Review 发现并修复：撤销缺失 registry 时先改 DID Document 的部分写入风险；revoked registry record 被当作 active 幂等返回的风险；模型重复唯一约束风险。剩余风险：MVP 撤销实时性仍依赖 message-service DID Document cache 刷新；Step 01 未实现独立 rotate endpoint。 | `cd user-service && uv run python -m pytest tests/app/did -v`：32 passed；`cd user-service && uv run ruff check src/user_service/app/did src/user_service/storage tests/app/did/test_service_managed.py tests/conftest.py`：All checks passed；`cd user-service && uv run python -m py_compile ...`：通过；`cd user-service && uv run python scripts/gen_openapi.py`：生成 `docs/openapi.json`；`cd user-service && git diff --check`：通过。 | 启动 Step 02：ANP SDK / im-core optional params |
| 02 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T10:39:30Z | 2026-06-09T12:28:11Z | `f0a5389 im-core: add delegated inbox signing options` | 2026-06-09 Review：检查 optional 参数兼容、ANP proof 语义、owner/key 校验、Dart binding 同步、E2EE projection 拒绝、错误命名残留。发现并修复：`history.rs` move/borrow 风险；delegated inbox/history proof target 需使用配置化 service DID；delegated group history 不应静默忽略 `InboxHistoryOptions`；Plan/设计文档 registry/device/default 旧措辞残留。剩余风险：`ScopedInboxToken` 为 MVP 后路径，Step 07 才实现服务端策略。 | `cd awiki-cli-rs2 && cargo test -p im-core --locked`：267 lib tests passed，所有 integration/doc tests passed；`cd awiki-cli-rs2 && cargo test -p im-core-dart --locked`：6 unit + 13 facade + 0 doc tests passed；`cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`：Done；`cd awiki-cli-rs2/packages/awiki_im_core && flutter test`：12 tests passed；`cd awiki-cli-rs2 && git diff --check`：通过；Step 02 naming check 和设计残留检查：无命中。 | 启动 Step 03：awiki-deamon bootstrap 与 user delegated identity state |
| 03 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T12:46:56Z | 2026-06-09T13:26:47Z | `eac62bd awiki-deamon: add app bootstrap state` | 2026-06-09 Review：检查 secret handling、control payload redaction、幂等冲突、状态恢复、schema dispatch、E2EE / main key 禁止、Step 04 边界。发现并修复：control payload / extra Debug redaction 不够硬；bootstrap replay 查重在 transaction 外存在并发写入窗口；schema version 升级后集成测试仍期望 15。剩余风险：MVP 仍按现有 daemon secret 存储方式保存 delegated subkey，明文 bootstrap body 加密和 secure key store 留到后续版本。 | `cargo test -p awiki-deamon --locked -j1`：72 lib tests passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。`cargo test -p awiki-deamon --locked -j1 app_bridge -- --nocapture`：8 passed；`cargo test -p awiki-deamon --locked -j1 delegated_identity -- --nocapture`：2 passed；`git diff --check`：通过。 | 启动 Step 04：awiki-deamon message agent binding |
| 04 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T13:28:24Z | 2026-06-09T14:04:10Z | `ccf84b5 awiki-deamon: ensure app message agent` | 2026-06-09 Review：检查 active binding 唯一性、bootstrap replay、runtime token scope、secret/token 泄漏、Hermes 私钥边界、Step 05 非目标。发现并修复：重复 bootstrap 去掉 `runtime_registration_token` 时 payload hash 冲突；专用 Message Agent 沿用 Hermes 默认 runtime token recipient scope 过宽。剩余风险：Step 04 不实现 user delegated inbox poller；Runtime Agent profile 缺失时重复 bootstrap 会失败并等待后续 repair 流程。 | `cargo test -p awiki-deamon --locked -j1`：78 lib tests passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。定向：`cargo test -p awiki-deamon --locked -j1 app_bridge -- --nocapture`：11 passed；`cargo test -p awiki-deamon --locked -j1 daemon_bootstrap_replay_reuses_message_agent_without_runtime_token -- --nocapture`：1 passed；`cargo test -p awiki-deamon --locked -j1 app_message_agent_runtime_token_scope_is_limited_to_bound_user -- --nocapture`：1 passed；`git diff --check`：通过。 | 启动 Step 05：awiki-deamon delegated inbox sync |
| 05 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T14:16:19Z | 2026-06-09T15:33:31Z | `59ec9b2 awiki-deamon: sync user delegated inbox` | 2026-06-09 Review：检查 cursor/processed message、dispatch 失败、E2EE opaque、system/control payload、prompt 包装、message_event retention、runtime status/final outbox、delegated key material 和 DID shadow。发现并修复：失败 dispatch retryable/cursor 语义；message_event 写入时机；生产 dispatcher 实际运行 Runtime Host；failed run retry id；`private_key_multibase` 到 PEM normalization；DID shadow 刷新；runtime status/final 进入 `message_sync_outbox` 且不保存 final 明文。剩余风险：Step 07 server policy 与 Step 08 APP schema 消费仍未完成。 | `cargo check -p awiki-deamon --locked`：通过；`cargo test -p awiki-deamon --lib --locked -j1 user_delegated -- --nocapture`：11 passed；`cargo test -p awiki-deamon --lib --locked -j1 delegated_inbox_sync_state -- --nocapture`：1 passed；`cargo test -p awiki-deamon --locked -j1`：lib 88 passed；main 0 passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。 | 启动 Step 06：awiki-me bootstrap UI 与 service |
| 06 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T16:12:00Z | 2026-06-09T17:26:44Z | `awiki-cli-rs2` `98c50ac im-core: expose daemon subkey package`；`awiki-me` `25d8cbb awiki-me: load daemon subkey for bootstrap` | 2026-06-09 Review：检查 APP bootstrap 是否只走普通消息、private package 是否只在 bootstrap 读取、不进入 UI/log、老注册 API 是否保持兼容、Dart native/web API 是否一致、generated platform churn 是否排除。发现并修复：`SessionIdentity.localAlias` 不存在导致 provider 编译失败；`awiki_im_core` web stub 缺少 `loadDaemonSubkeyPackage` 同名 unsupported API；`im-core` prelude 未导出 `DaemonSubkeyPrivatePackage`；`flutter test` 产生的 Android `GeneratedPluginRegistrant.java` 无关 churn 已恢复。剩余风险：recovered / 既有本地身份缺少 daemon subkey package 时仍需后续补齐或 rotate flow；Step 07 server policy 未完成前 delegated inbox/send 只能依赖客户端与 daemon 本地链路验证。 | `cd awiki-cli-rs2 && cargo test -p im-core --locked register_handle_generates_and_saves_daemon_subkey_package -- --nocapture`：1 passed；`cd awiki-cli-rs2 && cargo check -p im-core-dart --locked`：通过；`cd awiki-cli-rs2 && cargo test -p im-core-dart --locked`：6 unit + 13 facade passed；`cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`：Done；`cd awiki-cli-rs2/packages/awiki_im_core && flutter test`：12 passed；`cd awiki-me && flutter analyze`：No issues found；`cd awiki-me && flutter test`：267 passed；targeted awiki-me tests：28 passed + 10 passed；`git diff --check`：两仓通过。 | 启动 Step 07：message-service delegated key policy 与 fanout |
| 07 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T17:45:37Z | 2026-06-09T18:24:04Z | `message-service` `7afa621 message-service: support delegated inbox policy` | 2026-06-09 Review：检查 delegated send / inbox / history 的 DID proof、key owner、`keyid` 一致性、DID Document `authentication`、老本地 view 兼容、E2EE filtering、同 DID fanout 和文档契约。发现并修复：delegated local view 补 `validate_auth_scheme`；增加 key 不在 DID Document `authentication` 的拒绝测试；补 delegated send 测试；确认运行时授权输入只来自请求 proof、当前 DID Document `authentication`、key owner 一致性和普通非 E2EE scope。剩余风险：delegated `inbox.mark_read` MVP 明确不开放；撤销实时性依赖 DID Document `authentication` 更新和 message-service DID Document cache/refresh；workspace clippy 被无关 `im-group` 测试 lint 阻塞。 | `cd message-service && cargo test -p im-direct -- --nocapture`：40 passed；`cd message-service && cargo test -p im-storage -- --nocapture`：6 passed，Postgres integration helper 因未设置 `MESSAGE_SERVICE_STORAGE_TEST_DATABASE_URL` 打印 skipped；`cd message-service && cargo test -p im-runtime notify_agent_delivers_to_all_matching_sessions -- --nocapture`：1 passed；`cd message-service && cargo test --workspace`：208 passed，doc tests 0；`cd message-service && cargo clippy -p im-direct --all-targets -- -D warnings`：通过；`cd message-service && cargo clippy -p im-storage --all-targets -- -D warnings`：通过；`cd message-service && cargo clippy --workspace --all-targets -- -D warnings`：失败，既有无关 `crates/im-group/src/handlers.rs:6746` `await_holding_lock`；`cd message-service && git diff --check`：通过。 | 启动 Step 08：APP action schema 与可见性 |
| 08 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T18:28:37Z | 2026-06-09T19:08:53Z | `awiki-cli-rs2` `8c9e128 awiki-deamon: add app action schemas`；`awiki-me` `3841b76 awiki-me: add app action payload models`；相关设计收口 `awiki-cli-rs2` `f1d3cb5 docs: align message authorization boundary` | 2026-06-09 Review：检查 APP action schema、runtime token scope、no-side-effect RPC 路径、payload filter、未知 `awiki.*` 可见性、联系人写确认、E2EE/private material 拒绝和文档授权边界。发现并修复：`foreground.rs` 测试需适配新的 `AppControlOutcome`；`app.action.request` 不能走无 outbox 副作用 RPC 路径，已改为必须经 `execute_runtime_rpc_request_with_outbox`；Flutter test 产生的 Android registrant 无关 churn 已恢复；两篇设计文档继续收紧 message-service MVP 授权源为 DID proof + 当前 DID Document `authentication`。剩余风险：APP 侧当前落地为 domain model/reducer、payload hiding 和 confirmation state，尚未实现完整用户确认 UI / 自动化策略面板，按 MVP 后 UI 工作记录。 | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1`：lib 93 passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 23 + 2；doc tests 0 passed；0 failed。定向：`app_bridge` 15 passed；`action` 4 app action tests passed；`user_delegated` 11 passed；`app_action_request` 2 passed；`app_capabilities_and_action_result` 1 passed。`cd awiki-me && flutter analyze`：No issues found；`cd awiki-me && flutter test`：272 passed；targeted im-core mapper tests 20 passed；`git diff --check`：相关仓通过；旧命名/设备化 key/registry 残留检查通过。 | 启动 Step 09：系统测试与集成收口 |
| 09 | done | `feature/release-0526/agent-im-hutong` | 2026-06-09T19:08:53Z | 2026-06-09T20:36:30Z | `agent-im: finalize system integration`；最终短 hash 以提交后 `git rev-parse --short HEAD` 为准 | 全局 Review：Step 01-08 均已 done 且有 commit/验证证据；确认 `awiki-system-test` 当前入口为 `uv run awiki-system-test`；发现并修复 DID Document 追加 `#daemon-key-1` 后 proof 失效、老 CLI / group-e2ee struct literal 缺少 optional 默认值、远端 mock DID 断言不再适配本地生成 key-bound DID、两篇设计文档和 Plan 中 message-service 授权源 / daemon key fragment / user-service public method 边界残留。剩余风险已记录：MVP 明文 bootstrap、daemon subkey 仍沿用现有 secret 存储、E2EE Agent 处理不进入 MVP、mail service remote skip HTTP 502、撤销实时性依赖 DID Document 刷新。 | `cd user-service && uv run pytest tests/app/did -v`：32 passed；`cd awiki-cli-rs2 && cargo test -p im-core --locked`：269 lib tests passed，integration/doc tests passed；`cargo test -p awiki-cli --locked`：awiki-cli 全量测试通过；`cargo build --bin awiki-cli --offline`：通过；`cargo test -p awiki-deamon --locked -j1`：93 lib passed，integration tests 21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 23 + 2，doc tests 0；`cargo +stable test -p im-core --lib ... --features group-e2ee --locked`：1 passed；`cargo test -p im-core-dart --locked`：6 unit + 13 facade passed；`scripts/flutter/codegen-check.sh`：Done；`packages/awiki_im_core && flutter test`：12 passed；`cd message-service && cargo test --workspace`：25 + 27 + 10 + 9 + 40 + 52 + 16 + 22 + 1 + 6 passed，doc tests 0；`cd awiki-me && flutter analyze`：No issues found；`cd awiki-me && flutter test`：272 passed；remote system test：185 passed, 16 skipped；naming check / `git diff --check`：通过。 | 已完成；提交后核对所有仓库状态 |

## 8. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、当前小 Plan、执行台账和当前 `git status --short --branch`。
- 同一时间只执行一个步骤，除非本 Plan 明确标记多个步骤彼此独立且可以并行；当前 Step 默认不并行。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤依次执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 上一个依赖步骤的完成工作未提交前，不要开始下一个依赖步骤。
- 改变范围、顺序、验收标准、公开契约、数据模型或验证策略前，先更新本 Plan 和对应小 Plan 的变更记录。
- 每个 Step 的 commit 必须聚焦，不能把所有仓库的修改积累到最后一个大 commit。

## 8.1 Codex Goal 提示词

```text
请以 `awiki-cli-rs2/docs/agent-im/plan/plan.md` 为唯一规划入口，按文档执行完整实现。

开始前先读取：
- `awiki-cli-rs2/docs/agent-im/plan/plan.md`
- 当前第一个未 done 的 Step 文档
- 主 Plan 的执行台账、Codex Goal 执行协议、验证策略、Blocked 处理和 Plan 变更记录
- 当前 `git status --short --branch`

请从第一个状态不是 `done` 的步骤开始，一次只执行一个步骤。每步都要按对应小 Plan 实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填主 Plan 执行台账和 Step 执行状态。需要改变范围、顺序、验收标准、公开契约、数据模型或验证策略时，先更新 Plan 变更记录。

所有步骤完成后，执行最终全局 Review 和整体验证，记录实际命令、通过/失败/跳过数量、失败或跳过原因、剩余风险和最终工作区状态。

核心注意点：MVP 只使用 `user_did#daemon-key-1` 子私钥，不导入用户主私钥；新增 SDK/API 字段必须是 optional，不破坏老调用；新增 inbox 参数命名固定为 `inbox_owner_did`、`inbox_auth_verification_method`、`inbox_auth_key_ref`、`inbox_auth`、`ScopedInboxToken`、`InboxHistoryOptions`；MVP 不处理 E2EE 明文/摘要/metadata；最终系统测试必须使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info`。
```

## 9. 小 Plan 摘要

### Step 01：user-service DID delegated subkey

- 小 Plan：[steps/01-user-service-did-delegated-subkey.md](steps/01-user-service-did-delegated-subkey.md)
- 目标：APP 创建用户 DID Document 时默认本地生成 `user_did#daemon-key-1` 子私钥，user-service 登记对应 public verification method，提供 registry、query、revoke、rotate 契约。
- 设计方法：把 daemon key 当作用户 DID 下的附属 authentication key；APP 拥有 private material，user-service 只保存 public key 和 registry record。
- 实现方法：冻结 key package/schema fixture，扩展 DID 创建服务、storage model、API schema、测试和文档；bootstrap 不再追加 DID key。
- 路径：`user-service/src/user_service/app/did/*`、`user-service/src/user_service/storage/sqlmodel/models/did.py`。
- 验证方式：`cd user-service && uv run pytest tests/app/did -v`，并补充 public registration、幂等/conflict、public verification method 状态/撤销测试。
- Review 环节：重点看 DID Document 兼容性、APP 侧 private key ownership、撤销语义、老客户端默认行为和 key material 泄露。
- Commit 要求：一个 user-service 聚焦 commit。
- 风险：`authentication` key 权限偏大；依靠 registry/policy、撤销和审计缓解。

### Step 02：ANP SDK / im-core optional params

- 小 Plan：[steps/02-im-core-delegated-signing-inbox-options.md](steps/02-im-core-delegated-signing-inbox-options.md)
- 目标：给 send/inbox/history 增加 delegated signing/inbox optional 参数，老调用不变。
- 设计方法：`logical_sender_did` 与 signing key 分离；inbox/history 使用 `InboxHistoryOptions`。
- 实现方法：扩展 Rust DTO、proof builder、wire client、Dart binding 和接口文档。
- 路径：`awiki-cli-rs2/crates/im-core/src/messages/*`、`awiki-cli-rs2/crates/im-core/src/internal/wire/inbox.rs`。
- 验证方式：`cd awiki-cli-rs2 && cargo test -p im-core --locked`，binding 改动时运行 codegen/check 和 Dart tests。
- Review 环节：重点看 optional 兼容、scope 校验和 E2EE projection 拒绝。
- Commit 要求：一个 `awiki-cli-rs2` 聚焦 commit。
- 风险：公开 API 命名漂移；本计划固定使用 `inbox_*` 命名。

### Step 03：awiki-deamon bootstrap 与 user delegated identity state

- 小 Plan：[steps/03-awiki-deamon-bootstrap-state.md](steps/03-awiki-deamon-bootstrap-state.md)
- 目标：Daemon 接收 `awiki.daemon.bootstrap.v1`，存储 user delegated identity，处理幂等。
- 设计方法：bootstrap 是普通消息发送上的 system/control desired state，不是反复命令；MVP body 是明文 JSON，传 key package 并严格防普通聊天、日志和 prompt 泄露。
- 实现方法：新增 app_bridge/bootstrap/message_control/state 模块和测试。
- 路径：`awiki-cli-rs2/crates/awiki-deamon/src/*`。
- 验证方式：`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked`。
- Review 环节：重点看 secret handling、幂等、状态恢复和日志审计。
- Commit 要求：一个 `awiki-deamon` 聚焦 commit。
- 风险：明文 bootstrap 安全债；记录为后续普通消息 body 加密。

### Step 04：awiki-deamon message agent binding

- 小 Plan：[steps/04-awiki-deamon-message-agent-binding.md](steps/04-awiki-deamon-message-agent-binding.md)
- 目标：Daemon 自动 `ensure_app_message_agent(role=app_message_handler)`，持久化绑定并保证重放不重复创建 Agent。
- 设计方法：Message Agent 是 bootstrap 后置 desired state，由 Daemon 管理，不由 APP 反复 create runtime。
- 实现方法：新增 `message_agent.rs`、binding table、runtime/Hermes ensure 流程、重启恢复。
- 路径：`awiki-cli-rs2/crates/awiki-deamon/src/runtime/*`、`awiki-cli-rs2/crates/awiki-deamon/src/plugins/hermes/*`、`awiki-cli-rs2/crates/awiki-deamon/src/state/*`。
- 验证方式：`cargo test -p awiki-deamon --locked`，补幂等和重启恢复测试。
- Review 环节：重点看 active binding 唯一性、runtime token scope 和重复 bootstrap。
- Commit 要求：一个 `awiki-deamon` 聚焦 commit。
- 风险：Agent 生命周期与用户手动创建 runtime 混淆；通过 `role=app_message_handler` 分离。

### Step 05：awiki-deamon delegated inbox sync

- 小 Plan：[steps/05-awiki-deamon-user-delegated-inbox-sync.md](steps/05-awiki-deamon-user-delegated-inbox-sync.md)
- 目标：Daemon 用 user delegated identity 拉取普通非 E2EE inbox/history，持久化 cursor 和 processed message，投递给绑定 Agent。
- 设计方法：接收链路与 runtime own inbox 分开；E2EE opaque notification 只丢弃或标记不可处理。
- 实现方法：新增 `process_user_delegated_inbox_once`、durable cursor、processed_message、message.sync/outbox。
- 路径：`awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/runtime_inbox.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/inbox/mod.rs`。
- 验证方式：`cargo test -p awiki-deamon --locked`，补 cursor/retry/idempotency 测试。
- Review 环节：重点看重复处理、遗漏消息、E2EE 边界和 crash recovery。
- Commit 要求：一个 `awiki-deamon` 聚焦 commit。
- 风险：服务端 delegated inbox 契约未落地前只能 mock；记录依赖。

### Step 06：awiki-me bootstrap UI 与 service

- 小 Plan：[steps/06-awiki-me-pairing-bootstrap-ui-service.md](steps/06-awiki-me-pairing-bootstrap-ui-service.md)
- 目标：APP 使用 DID 创建时已有 daemon key，通过普通消息发送一次性发送 bootstrap/session payload，展示 bootstrap/message agent 状态。
- 设计方法：APP 负责用户交互和授权边界，只通过 message-service 普通消息发送与 Daemon 通信，不反复发送 create runtime command。
- 实现方法：扩展 agent control service、identity adapter、payload model、UI provider 和 control payload 过滤。
- 路径：`awiki-me/lib/src/application/agent/agent_control_service.dart`、`awiki-me/lib/src/data/im_core/*`、`awiki-me/lib/src/presentation/agents/*`。
- 验证方式：`cd awiki-me && flutter analyze && flutter test`。
- Review 环节：重点看 bootstrap 幂等、私钥不进 UI/log、系统 payload 隐藏和错误恢复。
- Commit 要求：一个 `awiki-me` 聚焦 commit。
- 风险：本地 key package 生命周期不清；Step 01/03 契约必须明确。

### Step 07：message-service delegated key policy 与 fanout

- 小 Plan：[steps/07-message-service-delegated-key-policy-and-fanout.md](steps/07-message-service-delegated-key-policy-and-fanout.md)
- 目标：message-service 支持 `user_did#daemon-key-1` 的普通 send/inbox/history proof 和同 DID 多连接 fanout。
- 设计方法：E2EE boundary 不变；delegated inbox/history pull 只返回普通非 E2EE 消息；MVP 只校验 DID proof、DID Document `authentication`、key owner 一致性和本域普通非 E2EE scope。
- 实现方法：扩展 auth/proof policy、direct/history/inbox handler、WebSocket session routing、docs 和 tests。
- 路径：`message-service/crates/*/src`、`message-service/bins/message-service/src`、`message-service/docs/api/*`。
- 验证方式：`cd message-service && cargo test --workspace`，必要时 `cargo clippy --workspace --all-targets -- -D warnings`。
- Review 环节：重点看 DID proof、DID Document authentication、scope、fanout 和 E2EE opaque 处理。
- Commit 要求：一个 message-service 聚焦 commit。
- 风险：MVP 撤销实时性依赖 DID Document cache 刷新；跨服务 policy client 留到后续版本。

### Step 08：APP action schema 与可见性

- 小 Plan：[steps/08-app-action-schema-and-visibility.md](steps/08-app-action-schema-and-visibility.md)
- 目标：实现最小 APP action allowlist、action/result schema、message.sync 和 payload 可见性规则。
- 设计方法：Agent 可以强，但 MVP 只开放少量可控能力；高风险动作不进入 allowlist。
- 实现方法：收敛 JSON schema、runtime token scope、APP reducer、UI confirmation 和 payload filter。
- 路径：`awiki-cli-rs2/crates/awiki-deamon/src/*`、`awiki-me/lib/src/domain/entities/agent/*`、`awiki-me/lib/src/domain/entities/chat_message.dart`。
- 验证方式：`cargo test -p awiki-deamon --locked`、`cd awiki-me && flutter test`。
- Review 环节：重点看普通聊天污染、权限绕过、确认策略和审计字段。
- Commit 要求：可按仓库拆两个 commit；如跨仓契约必须同批完成则在台账说明。
- 风险：schema 变更造成现有 daemon 忽略或 APP 显示为普通聊天；通过 schema dispatch 和 filter 测试控制。

### Step 09：系统测试与集成收口

- 小 Plan：[steps/09-system-test-and-integration.md](steps/09-system-test-and-integration.md)
- 目标：完成跨仓库端到端验证、全局 Review、系统测试证据和最终收口。
- 设计方法：以真实 remote `awiki.info` 模式验证核心用户旅程。
- 实现方法：补系统测试用例或运行现有脚本，记录通过/失败/跳过数量和原因。
- 路径：`awiki-system-test`、本 Plan 和步骤台账。
- 验证方式：`cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote uv run python manage_local_test_env.py run-tests --domain awiki.info`，若脚本参数不同按仓库 README 修正并记录实际命令。
- Review 环节：全局 Review 覆盖公开契约、安全、隐私、E2EE boundary、文档和未提交变更。
- Commit 要求：若修改测试或文档，创建最终集成 commit。
- 风险：remote 环境不可用时不能伪造通过，必须记录 blocker、替代证据和重试条件。

## 10. Review 策略

- 每步骤 Review：实现完成后、commit 前执行。优先看正确性、回归、公开契约、数据安全、安全/隐私、测试覆盖、文档漂移和兼容性。
- 全局 Review：Step 01-09 完成后执行，检查跨仓库契约是否一致，台账与 commit 是否一致，是否仍有错误命名或过期假设。
- 契约 / 安全 / 隐私 Review：重点检查 `user_did#daemon-key-1` 使用范围、key material handling、runtime token scope、APP action allowlist、E2EE boundary、message-service deny-by-default。
- 文档 Review：检查两篇设计文档、API 文档、系统测试记录、Plan 与 Step 状态是否一致；禁止出现旧命名和用户主私钥导入路径。

## 11. 验证策略

| 层级 | 命令 / 检查 | 预期证据 |
|---|---|---|
| user-service unit | `cd user-service && uv run pytest tests/app/did -v` | APP 侧 public registration、DID 创建默认 daemon public key、registry、revoke、query、幂等/conflict 测试通过；记录通过/失败/跳过数量。 |
| awiki-cli-rs2 unit | `cd awiki-cli-rs2 && cargo test -p im-core --locked`、`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | delegated signing/inbox、bootstrap、message agent binding、cursor/idempotency 测试通过。 |
| Dart binding | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`、`cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | Rust/Dart API 同步；optional 参数可用，老调用不变。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | bootstrap service、payload filter、action UI/provider 测试通过。 |
| message-service | `cd message-service && cargo test --workspace`、必要时 `cd message-service && cargo clippy --workspace --all-targets -- -D warnings` | delegated proof、DID Document authentication、fanout、E2EE boundary 测试通过。 |
| System / E2E | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote uv run python manage_local_test_env.py run-tests --domain awiki.info` | remote `awiki.info` 完整链路测试证据；若命令不同，记录实际命令。 |
| Docs / naming | `PATTERN="$(printf '%s|%s|%s|%s|%s|%s' 'message_''owner|message_''auth' 'Message''Access' 'Scoped''Message' 'mailbox_''owner' 'Scoped''Mailbox' 'Scoped''MailboxToken')" && rg -n "$PATTERN" awiki-cli-rs2/docs/agent-im/plan awiki-cli-rs2/docs/agent-im/*.md` | 不出现旧候选命名残留；若检查命令本身被命中，需调整为脚本变量形式。 |

## 12. 文档更新

- Harness 文档：本计划不直接修改 Harness；如果实现发现跨仓库边界与 Harness 不一致，先更新 Plan 变更记录，再决定是否提交 Harness 文档 PR。
- 子仓库文档：Step 01 更新 user-service DID/API 文档；Step 02 更新 `awiki-cli-rs2/docs/api/im-core-interface/*`；Step 07 更新 message-service API/architecture 文档；Step 09 更新系统测试证据。
- 本次生成的任务文档：`awiki-cli-rs2/docs/agent-im/plan/plan.md` 与 `awiki-cli-rs2/docs/agent-im/plan/steps/*.md`。

## 13. Commit 计划

- 每个完成、验证、Review 通过的步骤创建一个聚焦 commit。
- Commit 前记录 `git status --short --branch` 和纳入文件。
- Commit 后记录 commit hash 和工作区状态。
- 只有最终集成确实修改文件时才创建最终集成 commit。
- 不要把所有步骤的修改积累到一个最终大 commit。
- 若一个 Step 必须跨仓库同步提交，台账必须说明原因、每个仓库纳入文件、每个仓库 commit hash 和剩余未提交变更。

## 14. Blocked 处理

| Blocker | Step | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|---|
| remote `awiki.info` 不可用 | 09 | 待执行者填写命令输出 | 待执行者填写重试和替代检查 | 整体计划上线验收 | 不标记通过；记录 blocker、环境、重试条件 |
| user-service DID 创建 API 无法兼容 APP 侧 daemon public registration | 01/06 | 待执行者填写 | 待执行者评估新增 optional public registration 字段或兼容 endpoint | 当前步骤 | 先更新 Plan，再实现兼容接口；不得让 user-service 生成或返回 daemon private key |
| DID Document cache 无法及时反映 daemon key 撤销 | 07 | 待执行者填写 | 缩短 cache、触发刷新或记录后续跨服务 policy client | 当前步骤 / Step 05 | 记录撤销实时性风险，不在 MVP 引入跨服务 registry RPC |

- 只有依赖允许且风险已记录时，才继续另一个 pending 步骤。
- 只有没有安全假设、回退方案或独立下一步时，才询问用户。
- Blocked 的 Step 不得被标记为 `done`；解除 blocker 后必须记录解决方式和验证证据。

## 15. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-09 | 创建 MVP 实施 Plan 和 9 个 Step 小 Plan | 用户要求根据两篇设计文档生成可落地方案 | Step 01-09 | 是 |
| 2026-06-09 | 修正 Step 01/02/03/05/06/07 关键契约 | Review 发现 daemon subkey 私钥所有权边界、key package 契约、SDK 验证矩阵、普通消息投递给 Agent 的 prompt/retention 边界需要收紧；用户确认 APP 侧生成私钥、单 APP 单 daemon key、不带设备信息 | Step 01、02、03、05、06、07、09 | 是 |
| 2026-06-09 | Step 01 public registration 允许省略完整 `verification_method` | 当前 user-service DID 创建由服务端 factory 生成最终 key-bound DID，APP 在请求前可能不知道最终 DID URL；服务端仍不生成 daemon 私钥，只在 DID 生成后把 `key_fragment=#daemon-key-1` 补成 `did#daemon-key-1` 并校验公钥 | Step 01、02、03、06、07 | 是 |
| 2026-06-09 | Step 02 Review 后收紧 delegated inbox/history SDK 边界 | 实现 Review 发现 delegated inbox/history proof target 不能硬编码，delegated group history 不能静默进入 group/E2EE projection；同时清理 Plan/设计文档中运行时授权来源、设备化 key fragment 和私钥所有权边界残留 | Step 01、02、03、07 | 是 |
| 2026-06-09 | Step 04 明确 `desired_message_agent.runtime_registration_token` 可选字段 | 现有 `awiki-deamon` Runtime Agent 创建必须经 user-service runtime registration token；bootstrap 仍是唯一 APP ↔ Daemon 普通消息通道。首次创建 `app_message_handler` 时 APP 在 bootstrap desired state 中携带 runtime token；已有 active binding 时不需要 token且不得重复创建。该 token 不持久化到 binding / audit detail。 | Step 04、06、09 | 是 |
| 2026-06-09 | Step 05 Review 后补齐 delegated inbox 本地运行边界 | 实现 Review 发现 daemon 生产 adapter 需要把 bootstrap `private_key_multibase` 标准化为 im-core 可解析 PEM，DID shadow 需要随当前 identity 刷新，runtime status/final 需要进入 APP 可消费 outbox 且不能保存 final 明文。该变更不改变 APP-Daemon 普通消息通道和 user-service public registration 边界。 | Step 05、08、09 | 是 |
| 2026-06-09 | Step 06 扩展为跨 `awiki-me` 与 `awiki-cli-rs2` identity registration 收口 | Step 06 实现 Review 发现 `awiki-me` 当前 `IdentityCorePort` / `AwikiImCoreIdentityAdapter` 只能调用 `awiki_im_core.registerHandle*`，底层 im-core identity registration 尚未生成、保存或暴露 `#daemon-key-1` private package，也未把 daemon public registration 注入注册 DID Document。不能在 APP 层伪造 key package；需要先由 im-core DID 创建路径本地生成 daemon subkey、写入 DID Document authentication、保存 private package，并以 optional API 暴露给 awiki-me bootstrap。 | Step 02、06、09 | 是 |
| 2026-06-09 | Step 07 收口为 DID Document authentication 授权源 | Step 07 实现和设计文档 Review 后确认：message-service MVP 运行时授权输入只来自请求 proof、当前 DID Document `authentication`、key owner 一致性和普通非 E2EE scope。delegated `inbox.mark_read` 不进入 MVP。 | Step 07、09 | 是 |
| 2026-06-09 | Step 09 系统测试命令按 `awiki-system-test` 当前入口校准 | `awiki-system-test` README 和 `uv run awiki-system-test --help` 显示当前入口为 `uv run awiki-system-test`，没有 `manage_local_test_env.py run-tests --domain` 参数。remote `awiki.info` 通过 `AWIKI_SYSTEM_TEST_MODE=remote` 与默认 `E2E_DID_DOMAIN=awiki.info` / URL fallback 控制。 | Step 09 | 是 |
| 2026-06-09 | Step 09 增加 `awiki-cli` optional 字段兼容修复 | remote system test 构建 `awiki-cli` 时发现 Step 02 新增的 `delegated_signing`、`inbox_history_options` 和 realtime wire `auth` optional 字段没有在老 CLI struct literal 中显式填默认值，导致 `cargo build --bin awiki-cli --offline` 失败。修复方式是老调用显式传 `None`，保持新增字段 optional 且不改变老行为。 | Step 02、09 | 是 |
| 2026-06-09 | Step 09 修复 DID Document proof 重新签名 | 最终集成 Review 发现 im-core 在生成并签名 DID Document 后再追加 `#daemon-key-1` 会导致 W3C proof 失效。修复为 APP 本地追加 daemon public verification method 后，继续使用 `#key-1` 对更新后的 DID Document 重新签名，并保留原 proof options。 | Step 01、02、06、09 | 是 |
| 2026-06-09 | Step 09 清理最终设计文档残留 | 用户要求两篇设计文档继续去掉 message-service 依赖 user-service 运行时授权来源、设备化 daemon key fragment 示例、以及 user-service 默认生成/登记的模糊表述。最终统一为 APP 本地生成 private/public key package，user-service 只登记 public verification method，message-service 只校验 DID proof 与 DID Document `authentication`。 | Step 09 | 是 |

## 16. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| `user_did#daemon-key-1` 是 authentication key，权限过宽 | APP 侧生成 private key、user-service 只登记 public key、撤销/过期/审计、E2EE deny | 撤销 key、移除 DID Document authentication、删除 Daemon local secret、停用 binding |
| MVP 明文 bootstrap 泄露子私钥 | 普通聊天 UI/system filter；禁止日志/prompt/runtime temp；secret_store 封装；后续在同一普通消息发送路径上改为加密文本或加密 JSON envelope | 立即 revoke key 并重新 bootstrap 新 key |
| delegated inbox 重复处理或漏消息 | durable cursor + processed_message + idempotency_key + retry outbox | 回滚 poller，保留原 runtime inbox；重建 cursor 后重放普通消息 |
| E2EE boundary 被破坏 | 服务端 delegated pull 不返回 E2EE 明文/metadata/private state；Daemon 丢弃 E2EE opaque notification | 停用 delegated inbox，清理可能写入的 message_event，补安全测试 |
| 新 schema 显示为普通聊天 | APP/Daemon schema dispatch 和 payload filter 测试 | 回滚 schema rollout，隐藏未知 `awiki.*` system payload |
| API optional 参数破坏老调用 | 老调用回归测试；字段 optional；默认逻辑不变 | 回滚 SDK API 变更或加兼容 adapter |

## 17. 最终全局 Review 与整体验证

- 触发条件：Step 01-08 已完成、Review、验证并提交；Step 09 已完成最终本地验证和 remote `awiki.info` 系统测试。
- Review 范围：`awiki-cli-rs2`、`awiki-me`、`user-service`、`message-service`、`awiki-system-test` 的相关变更，公开契约、测试、文档、执行台账、遗留风险和工作区状态。
- 重点关注：跨步骤一致性、回归风险、兼容性、安全/隐私、文档漂移、未提交变更、每个步骤 Review 发现是否已解决或记录。
- Review 发现：
  - im-core 注册路径先生成/签名 DID Document，再追加 `#daemon-key-1` 到 `verificationMethod` / `authentication`，会导致提交给 user-service 的 DID Document W3C proof 失效。
  - Step 02 新增 optional 字段后，`awiki-cli` 老调用和 group-e2ee feature-gated 测试 struct literal 仍缺少显式默认值。
  - awiki-cli 身份 live/mock contract 仍假设远端返回固定 `e1_remote` DID，但当前正确行为是持久化 APP 本地生成的 key-bound DID。
  - 两篇设计文档仍有少量容易误读为 message-service 依赖 user-service 运行时授权状态、设备化 daemon key fragment 或 user-service 生成 daemon key 的残留措辞。
- 已修复问题：
  - `crates/im-core/src/internal/identity_daemon_subkey.rs` 新增 DID Document 重新签名逻辑，保留原 proof options，并补单元测试和 integration proof 验证。
  - `crates/im-core/src/internal/identity_registration_runtime.rs` 在 daemon public method 写入 DID Document 后重新签名，保证注册提交的 DID Document proof 有效。
  - 老 CLI / feature-gated group-e2ee struct literal 显式填入 `delegated_signing: None`、`inbox_history_options: None`、`auth: None`，保持 optional 字段兼容。
  - 身份 contract 测试改为断言持久化本地生成的 key-bound DID，并校验发送给 user-service 的 DID Document id 与本地身份一致。
  - `agent_im_core_design.md`、`agent_delegated_identity_message_proof_plan.md`、Plan / Step 文档统一：APP 本地生成 private/public key package；user-service 只登记 public verification method；message-service MVP 只以 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope 判定授权；daemon key fragment 固定 `#daemon-key-1`。
- 剩余风险：
  - MVP 仍通过普通消息明文 JSON bootstrap 传递 `#daemon-key-1` private package；后续必须在同一普通消息发送路径上升级为加密文本或加密 JSON envelope。
  - daemon delegated subkey 本地存储暂沿用现有 daemon secret 存储方式；secure key store / OS keychain / KMS 是后续版本。
  - `user_did#daemon-key-1` 仍位于 DID Document `authentication`，外部验证方可能把它当作完整用户 authentication key；MVP 通过本域普通消息 scope、E2EE deny、审计和撤销边界缓解。
  - MVP 不处理 E2EE 明文、摘要、metadata 或 private state；Daemon 收到 E2EE opaque notification 只能丢弃或标记不可处理。
  - remote system test 中 mail local 相关 4 项因 `awiki-mail-service /mail/health` HTTP 502 跳过；该 skip 不属于 Agent IM 本次验收面，但已记录。
  - 撤销实时性仍依赖 DID Document `authentication` 更新和 message-service DID Document 重新解析/刷新。
- 最终证据：
  - `cd user-service && uv run pytest tests/app/did -v`：32 passed。执行前发现 `.venv/bin/pytest` shebang 指向旧工作区路径，已通过 `uv sync --all-groups --reinstall-package pytest` 修复本地虚拟环境入口；修复后原命令通过。
  - `cd awiki-cli-rs2 && cargo test -p im-core --locked`：269 lib tests passed；所有 integration/doc tests passed。
  - `cd awiki-cli-rs2 && cargo test -p awiki-cli --locked`：awiki-cli 全量测试通过。
  - `cd awiki-cli-rs2 && cargo build --bin awiki-cli --offline`：通过。
  - `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1`：93 lib passed；integration tests passed：21、22、5、19 passed / 3 ignored、15、3、23、2；doc tests 0。
  - `cd awiki-cli-rs2 && cargo +stable test -p im-core --lib internal::group_e2ee::incoming::tests::realtime_notification_projection_redacts_attachment_manifest_secrets --features group-e2ee --locked`：1 passed。
  - `cd awiki-cli-rs2 && cargo test -p im-core-dart --locked`：6 unit + 13 facade passed；doc tests 0。
  - `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`：Done。
  - `cd awiki-cli-rs2/packages/awiki_im_core && flutter test`：12 passed。
  - `cd message-service && cargo test --workspace`：crate tests 25 + 27 + 10 + 9 + 40 + 52 + 16 + 22 + 1 + 6 passed；doc tests 0。
  - `cd awiki-me && flutter analyze`：No issues found；`cd awiki-me && flutter test`：272 passed。
  - `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 uv run awiki-system-test --show-command`：实际底层命令 `pytest tests_v2 -q -rs`；185 passed, 16 skipped in 232.63s。
  - skip 原因：Rust store contract 目标已移除 1；local tests_v2 topology 相关 4；daemon rust contract selector 未设置 `AWIKI_DAEMON_RUST_REPO` 3；mail health HTTP 502 4；group E2EE flag-off guard 1；multi-tenant 额外环境变量未设置 3；message direct local topology 1。
  - 文档 / 命名检查：旧收件箱授权命名族、设备化 daemon key fragment、user-service 运行时授权来源残留检查无命中；`cd awiki-cli-rs2 && git diff --check` 通过。
- 最终 `git status`：
  - `awiki-cli-rs2`：Step 09 代码修复与文档证据已纳入 `agent-im: finalize system integration`；最终短 hash 以提交后 `git rev-parse --short HEAD` 为准，工作区需保持 clean。
  - `user-service`：clean，ahead 1。
  - `message-service`：clean，ahead 1。
  - `awiki-me`：clean，ahead 3；`flutter test` 产生的 Android generated registrant churn 已恢复。
  - `awiki-system-test`：clean。
- 本阶段修改文件：已创建 Step 09 最终集成 commit `agent-im: finalize system integration`；提交后以 `git status --short --branch` 和 `git rev-parse --short HEAD` 核对，最终响应记录实际短 hash。
