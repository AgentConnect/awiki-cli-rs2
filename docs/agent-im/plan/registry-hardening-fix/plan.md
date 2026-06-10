# Plan：Agent IM Delegated Key Registry 与 Bootstrap 安全收口修复

状态：in_progress  
DOC：`awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix`  
Harness：`awiki-harness`  
创建时间：2026-06-10  
恢复指针：Step 04 已完成，下一步从 Step 05 ANP SDK DID Document additional authentication optional 参数开始。

## 1. 目标

- 任务目标：修复 Agent IM MVP 已实现链路中 delegated key registry、DID Document proof、已有身份迁移、Daemon bootstrap 早期校验、key package schema、APP action capability 默认值和 ANP SDK DID Document 扩展方式的残留问题。
- 预期行为：`awiki-me` / `im-core` 实际 DID auth 注册路径与 `user-service` delegated key registry 闭环一致；任何 DID Document delegated key 新增、撤销、轮换都保持 proof 有效；恢复账号和已有本地身份能幂等生成或迁移 `user_did#daemon-key-1` private package；Daemon 在创建 message agent binding 前验证 private/public/DID Document authentication；key package 编码命名准确；APP action 必须来自显式 capability policy；ANP SDK 提供可选参数生成带额外 authentication verification method 的 DID Document，旧调用不变。
- 非目标：不引入新的 APP ↔ Daemon 传输通道；不在本修复中实现 bootstrap body 加密、secure enclave/keychain、E2EE Agent 处理、Agent DID delegation 或 ANP delegated proof；不改变 message-service MVP 的运行时授权来源，仍以 DID proof、当前 DID Document `authentication`、key owner 一致性和普通非 E2EE scope 为准。
- 完成标准：所有 Step 已实现、验证、Review、聚焦提交并回填台账；最终全局 Review 完成；身份/auth/secret 相关安全 gate 有明确结论；跨仓库验证和 remote `awiki.info` 系统测试证据已记录。

## 2. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | Harness 控制面入口、跨仓库任务约束。 |
| `awiki-harness/README.md` | Harness 使用方式和权威边界。 |
| `awiki-harness/context/00-context-map.md` | 任务路由入口。 |
| `awiki-harness/context/02-repo-map.md` | 工作区仓库职责和最新端侧入口。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | identity、message-service、im-core、daemon、App 依赖方向。 |
| `awiki-harness/context/20-rules-index.md` | 规则加载入口。 |
| `awiki-harness/context/30-tools-env.md` | 各仓验证命令入口。 |
| `awiki-harness/context/40-verification.md` | L3 身份 / auth / 安全敏感验证要求。 |
| `awiki-harness/context/50-task-workflow.md` | 执行、Review、证据记录流程。 |
| `awiki-harness/rules/documentation-principles.md` | 文档权威、路径和同步规则。 |
| `awiki-harness/rules/architecture-principles.md` | 跨仓边界、DID profile 和 E2EE 边界规则。 |
| `awiki-harness/rules/verification-policy.md` | L3 verification 和 security gate 要求。 |
| `awiki-harness/context/nodes/identity.node.md` | user-service / ANP DID WBA 身份边界。 |
| `awiki-harness/context/nodes/message-flow.node.md` | message-service v2、inbox/history、E2EE opaque 边界。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | awiki-deamon、runtime token、local RPC、runtime 不持有私钥边界。 |
| `awiki-cli-rs2/AGENTS.md` | 本仓规划文档中文要求；最终系统测试 remote `awiki.info` 要求。 |
| `awiki-me/AGENTS.md` | App 通过 Dart/Flutter SDK 使用 im-core；不新增 Python 工具。 |
| `message-service/AGENTS.md` | Rust v2 message-service 文档/API 权威和验证要求。 |
| `anp/anp/CLAUDE.md` | Python/Rust ANP SDK DID WBA、proof、测试入口。 |
| `anp/AgentNetworkProtocol/AGENTS.md` | 协议文档仓中文回复、文档和脚本约束。 |

## 3. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| 核心设计来源 | 作为修复验收依据，不重写 MVP 目标 | `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md`、`awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` |
| user-service DID auth 注册和更新 | 实际 DID auth `register/update_document` 路径必须派生 delegated registry；撤销/轮换必须以用户签名后的 DID Document 为准 | `user-service/src/user_service/app/did_auth/*`、`user-service/src/user_service/app/did/*` |
| user-service REST DID 创建 | 如果继续支持 `delegated_key_public_registration`，新增 public method 必须在 proof 生成前完成，或在保存前重签并验证 proof | `user-service/src/user_service/app/did/service.py`、`user-service/tests/app/did/*` |
| user-service registry/storage | 需要 registry backfill/reconcile、状态同步、审计字段和无 private material guarantee | `user-service/src/user_service/storage/*` |
| awiki-cli-rs2 im-core 身份注册/恢复 | 新注册、恢复、已有身份 bootstrap 前都能获得可用 `DaemonSubkeyPrivatePackage`，并同步服务端 DID Document / registry | `awiki-cli-rs2/crates/im-core/src/internal/identity_*`、`awiki-cli-rs2/crates/im-core/src/identity/*` |
| awiki-me bootstrap 入口 | 当 package 缺失时触发 ensure/migration，而不是直接失败；继续只通过普通消息 JSON 发送 bootstrap | `awiki-me/lib/src/data/im_core/*`、`awiki-me/lib/src/presentation/agents/*` |
| awiki-deamon bootstrap | 接收 private package 后，在创建 binding/message agent 前验证 key 可解析、private/public 匹配、DID Document authentication 存在且 public key 一致 | `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/*`、`awiki-cli-rs2/crates/awiki-deamon/src/inbox/*` |
| key package schema | 当前 `private_key_multibase` 承载 PEM 的命名不严谨；需要新 schema 或兼容迁移 | `awiki-cli-rs2/crates/im-core/src/identity/dto.rs`、`awiki-me/lib/src/domain/entities/agent/*`、`awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs` |
| APP action capability | 新 bootstrap 必须显式 capability；空列表表示无能力，Daemon 不再静默补成全部 MVP actions | `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/action.rs`、`awiki-me/lib/src/domain/entities/agent/*` |
| ANP SDK | DID Document 生成支持可选 additional authentication verification method，避免产品层 JSON patch 后重签 | `anp/anp/anp/authentication/did_wba.py`、`anp/anp/rust/src/*` |
| message-service | 作为兼容验证对象；不改运行时授权模型，确认 revoked DID Document 生效和 delegated inbox/send 仍可用 | `message-service/crates/im-direct/src/service.rs`、`message-service/crates/im-runtime/src/session_registry.rs` |
| awiki-system-test | 最终 remote `awiki.info` 集成验证，覆盖注册、bootstrap、delegated inbox/send、revoke/migration 关键路径 | `awiki-system-test` |

## 4. 假设与开放问题

### 假设

- MVP daemon key fragment 仍固定为 `#daemon-key-1`，一个 APP 默认只有一个 active daemon key，不包含设备名、设备型号、时间戳或其他可识别设备信息。
- message-service MVP 不查询 user-service registry；运行时 key 是否有效只以当前解析到的 DID Document `authentication` 为准。
- 因此，真正撤销或轮换 daemon key 必须通过用户主 DID key 签名后的 DID Document update 删除或替换 `#daemon-key-1`；user-service registry 只做登记、查询、审计、backfill 和状态同步，不能替代 DID Document。
- bootstrap private package 仍通过普通消息 JSON 发送给 Daemon，本计划只强化早期校验和本地存储/日志边界，不解决传输加密。
- 当前 AWiki 只要求支持 e1 DID profile；本修复不得为 K1 增加未经规划的兼容承诺。

### 开放问题

- user-service DID auth `update_document` 的当前请求 schema、认证上下文和 OpenAPI 文档需在 Step 01 实现前再次校准；如果缺少适合的 signed update 入口，Step 01 需先补该入口或明确要求 APP 使用现有等价入口。
- im-core 对已有身份执行 `ensure_daemon_subkey_package` 时，若缺少用户主 `#key-1` 私钥，则无法安全重签 DID Document；此类身份应返回明确错误并要求重新恢复/重新绑定。
- ANP Rust SDK 的 DID WBA 生成 API 具体类型名需在 Step 05 实现时按当前 `anp/anp/rust` 代码校准。
- remote `awiki.info` 系统测试可用性取决于当时环境；若外部服务不可用，必须记录原因、影响和替代证据。

## 5. 总体设计方法

- 设计边界：先修身份真相源和 proof 有效性，再修端侧 migration，再修 Daemon 早期拒绝坏 key package，最后整理 schema/capability 与 SDK API。不要通过 message-service 查询 registry 来规避 DID Document `authentication` 的撤销语义。
- 关键决策：`user-service` registry 与 DID Document 同步的主路径是“解析并记录已验证 DID Document 中的 delegated authentication key”；不是“服务端无签名地 patch DID Document”。
- 兼容性策略：新增字段和 SDK 参数必须 optional；旧 DID Document 没有 daemon key 时不自动失败；旧 bootstrap schema 可读但新 schema 写出；现有 binding 可通过 migration 或 legacy fallback 平滑过渡。
- 数据、协议、配置或迁移策略：新增 registry backfill/reconcile；新增 im-core `ensure` API；新增 bootstrap validation proof/key 检查；新增 key package v2；新增 ANP SDK additional authentication method optional 参数；所有变更配套测试和文档。
- 风险控制：所有 secret 不进入日志、audit detail、prompt、final/status text、system-test snapshot；所有 DID/auth/key material 变更必须 security review；每步 commit 前执行 Review。

## 6. 任务拆分

| Step | 标题 | 依赖 | 产出 | 小 Plan 文档 | Commit gate | 状态 |
|---|---|---|---|---|---|---|
| 01 | user-service registry 与 DID auth 注册/更新闭环 | 无 | DID auth register/update 派生 registry；proof 有效性测试；撤销语义修正；backfill/reconcile | [steps/01-user-service-did-auth-registry-proof.md](steps/01-user-service-did-auth-registry-proof.md) | 必须 | done |
| 02 | im-core / awiki-me 恢复与已有身份 daemon subkey migration | Step 01 | `ensure_daemon_subkey_package`；恢复路径保存 package；App bootstrap 前补齐；user-service `update_document` 省略元数据字段时保留旧值 | [steps/02-im-core-awiki-me-daemon-subkey-migration.md](steps/02-im-core-awiki-me-daemon-subkey-migration.md) | 必须 | done |
| 03 | awiki-deamon bootstrap private package 早期校验 | Step 01、Step 02 | private/public 匹配、DID Document authentication、public key 一致性和过期检查 | [steps/03-awiki-deamon-bootstrap-key-validation.md](steps/03-awiki-deamon-bootstrap-key-validation.md) | 必须 | done |
| 04 | key package schema 与 APP action capability 收口 | Step 03 | key package v2 / legacy decode；显式 capability policy；空列表禁用 | [steps/04-schema-capability-hardening.md](steps/04-schema-capability-hardening.md) | 必须 | done |
| 05 | ANP SDK DID Document additional authentication optional 参数 | Step 01、Step 04 | Python/Rust ANP SDK optional API；im-core/user-service 消费新 API；移除产品层 JSON patch 主路径 | [steps/05-anp-sdk-did-additional-authentication.md](steps/05-anp-sdk-did-additional-authentication.md) | 必须 | pending |
| 06 | 最终集成验证、安全 Review 与文档收口 | Step 01-05 | 全局 Review、跨仓测试、remote system-test、文档一致性和执行台账 | [steps/06-final-integration-security-review.md](steps/06-final-integration-security-review.md) | 如修改文件则必须 | pending |

## 7. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | 分支 | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `feature/release-0526/agent-im-hutong` / `user-service` | 2026-06-10T01:49:07Z | 2026-06-10T02:11:46Z | `user-service` `dada5a6` | Review 完成；修复 storage update 失败仍可能同步 registry、缺少 reconcile 入口、局部 CLAUDE/文件头漂移；剩余风险为 DID Document 保存与 registry 同步非同一 DB 事务，运行时仍以 DID Document `authentication` 为事实源 | `uv run pytest tests/app/did tests/app/did_auth -v`：137 passed, 32 warnings；`uv run ruff check src/user_service/app/did src/user_service/app/did_auth src/user_service/storage tests/app/did tests/app/did_auth`：All checks passed；`git diff --check`：通过；secret 搜索无 daemon private material 泄露 | Step 02：im-core / awiki-me 恢复与已有身份 daemon subkey migration |
| 02 | done | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2`、`awiki-me`、`user-service` | 2026-06-10T02:13:41Z | 2026-06-10T02:57:20Z | `awiki-cli-rs2` `4562474`；`awiki-me` `6fd1411`；`user-service` `1cafb30` | Review 完成；发现并修复 `update_document` 省略元数据可能破坏旧身份元数据，进一步修正显式 `null` 与省略字段的契约区别；确认 ensure 状态机不会覆盖已有 `#daemon-key-1` public key，不把主私钥交给 Daemon，无新增 secret 日志/UI；剩余风险为远端 signed update 成功后本地 package 保存失败会让下一次 ensure fail closed，需要用户重新恢复或后续补偿流程 | `cd awiki-cli-rs2 && cargo fmt --check && cargo test -p im-core --locked && cargo test -p im-core-dart --locked`：通过，`im-core` 270 lib tests 与 integration tests 通过、`im-core-dart` 6 unit + 13 facade tests 通过；`cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh && cd packages/awiki_im_core && flutter test`：codegen stable，12 tests passed；`cd user-service && uv run pytest tests/app/did_auth -v && uv run ruff check src/user_service/app/did_auth tests/app/did_auth`：105 passed, 32 warnings，ruff 通过；`cd awiki-me && flutter analyze && flutter test`：No issues found，272 tests passed；三仓 `git diff --check` 通过；secret 搜索只命中既有 secret handling、测试 fixture、bootstrap payload 字段和 redaction 逻辑 | Step 03：awiki-deamon bootstrap private package 早期校验 |
| 03 | done | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2` | 2026-06-10T03:00:09Z | 2026-06-10T03:40:25Z | `awiki-cli-rs2` `1596856` | Review 完成；发现并修复 bootstrap envelope 未显式要求 `controller_did == user_subkey_package.user_did` 的绑定缺口；确认坏 package / DID resolve 失败不会写入 `user_delegated_identity` 或 `bootstrap_replay`，`payload_hash` 不再包含 private key material，resolver 不信任 `delegated-inbox-*` shadow identity cache，delegated inbox sync 前会复查当前 DID Document authentication；剩余风险为 DID Document proof 未在 Daemon 本地验证、`did:wba` / `did:web` HTTP resolve fail closed 可能影响首次 bootstrap 可用性，bootstrap private package 仍按 MVP 走普通消息明文 JSON | `cd awiki-cli-rs2 && cargo fmt --check -p awiki-deamon`：通过；`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 bootstrap -- --nocapture`：21 passed；`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 user_delegated -- --nocapture`：9 passed；`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1`：全量通过，lib 105 passed，集成测试套件通过，Hermes 真实环境 3 ignored；`cd awiki-cli-rs2 && git diff --check`：通过；secret 搜索只命中 secret handling、redaction tests、fixtures 和既有状态测试，无新增日志/audit 泄露 | Step 04：key package schema 与 APP action capability 收口 |
| 04 | done | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2`、`awiki-me` | 2026-06-10T03:45:56Z | 2026-06-10T04:22:36Z | `awiki-cli-rs2` `94b1c20`；`awiki-me` `fc1895f` | Review 完成；发现并修复 action 测试 fixture 仍把新 binding 伪装成 legacy `desired_agent.allowed_actions`、`user_delegated` allowed_actions 投影和 action 执行规则不完全一致、`awiki-me` 未本地拒绝非 `pem` v2 private key encoding、`flutter test` 反复改动无关 Android generated registrant；确认 im-core / awiki-me 新写 v2 `private_key_pem`，Daemon 只为 legacy v1 读取 `private_key_multibase`，bootstrap hash/debug/audit 不包含 private material，新 binding 必须显式 `awiki.app.capabilities.v1`，空 capabilities 禁用 APP action；剩余风险为旧 binding 无 schema 时仍允许 legacy `desired_agent.allowed_actions` 兼容路径，bootstrap private package 仍按 MVP 决策通过普通消息明文 JSON 传输 | `cd awiki-cli-rs2 && cargo fmt --check -p im-core -p im-core-dart -p awiki-deamon`：通过；`cargo test -p im-core --locked`：通过，lib 272 passed，integration/doc tests 通过；`cargo test -p awiki-deamon --locked -j1 bootstrap -- --nocapture`：22 passed；`cargo test -p awiki-deamon --locked -j1 action -- --nocapture`：9 passed；`cargo test -p awiki-deamon --locked -j1 user_delegated -- --nocapture`：10 passed；`cargo test -p awiki-deamon --locked -j1`：通过，lib 110 passed，integration tests 21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 23 + 2，doc tests 0 passed；`cargo test -p im-core-dart --locked`：6 unit + 13 facade passed；`scripts/flutter/codegen-check.sh`：Done；`cd packages/awiki_im_core && flutter test`：12 passed；`cd awiki-me && flutter analyze`：No issues found；`cd awiki-me && flutter test`：273 passed；两仓 `git diff --check` 通过；命名/secret 搜索只命中 legacy decode/tests、历史计划记录、email mailbox 既有模型和 secret/redaction 代码，无新增 mailbox 命名或未解释 secret 泄露 | Step 05：ANP SDK DID Document additional authentication optional 参数 |
| 05 | pending | `feature/release-0526/agent-im-hutong` / 相关仓当前分支 | - | - | - | - | - | 等 Step 04 完成 |
| 06 | pending | `feature/release-0526/agent-im-hutong` / 相关仓当前分支 | - | - | - | - | - | 等 Step 01-05 完成 |

## 8. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、当前小 Plan、执行台账和当前 `git status --short --branch`。
- 同一时间只执行一个步骤；当前 Step 都存在顺序依赖，不并行。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤依次执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 上一个依赖步骤的完成工作未提交前，不要开始下一个依赖步骤。
- 改变范围、顺序、验收标准、公开契约、数据模型或验证策略前，先更新本 Plan 和对应小 Plan 的变更记录。
- 每个 Step 的 commit 必须聚焦，不能把所有仓库的修改积累到最后一个大 commit。

## 8.1 Codex Goal 提示词

```text
请以 `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/plan.md` 为唯一规划入口，按文档执行完整修复。

开始前先读取：
- `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/plan.md`
- 当前第一个未 done 的 Step 文档
- 主 Plan 的执行台账、Codex Goal 执行协议、验证策略、Blocked 处理和 Plan 变更记录
- 当前 `git status --short --branch`

请从第一个状态不是 `done` 的步骤开始，一次只执行一个步骤。每步都要按对应小 Plan 实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填主 Plan 执行台账和 Step 执行状态。需要改变范围、顺序、验收标准、公开契约、数据模型或验证策略时，先更新 Plan 变更记录。

所有步骤完成后，执行最终全局 Review 和整体验证，记录实际命令、通过/失败/跳过数量、失败或跳过原因、剩余风险和最终工作区状态。

核心注意点：真正撤销 `user_did#daemon-key-1` 必须通过用户签名后的 DID Document update 让当前 DID Document `authentication` 不再包含该 key；user-service registry 只做登记/审计/查询/同步，不能用未重签 JSON patch 替代 DID Document；message-service MVP 不查询 registry；bootstrap private package 仍走普通消息 JSON，但必须强化本地校验和 secret redaction；所有 SDK/API 新字段必须 optional，旧调用不变；最终系统测试必须使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info`。
```

## 9. 小 Plan 摘要

### Step 01：user-service registry 与 DID auth 注册/更新闭环

- 小 Plan：[steps/01-user-service-did-auth-registry-proof.md](steps/01-user-service-did-auth-registry-proof.md)
- 目标：让 `did-auth.register/update_document` 与 registry 闭环一致，修复 REST delegated public registration 和 revoke 的 proof 风险。
- 设计方法：registry 从已验证 DID Document 派生；撤销和轮换通过 signed DID Document update 生效。
- 实现方法：扩展 did_auth service/repository，同步 delegated key registry，补 backfill/reconcile 和 proof validity 测试。
- 路径：`user-service/src/user_service/app/did_auth/*`、`user-service/src/user_service/app/did/*`、`user-service/tests/app/did*`。
- 验证方式：`cd user-service && uv run pytest tests/app/did tests/app/did_auth -v`，补 registry/proof/revoke/backfill 测试。
- Review 环节：重点看 DID Document proof、unsigned mutation、private material、old RPC 兼容和 audit。
- Commit 要求：user-service 聚焦 commit。
- 风险：已有 registry 缺失数据需 backfill；撤销实时性仍依赖 DID Document cache 刷新。

### Step 02：im-core / awiki-me 恢复与已有身份 daemon subkey migration

- 小 Plan：[steps/02-im-core-awiki-me-daemon-subkey-migration.md](steps/02-im-core-awiki-me-daemon-subkey-migration.md)
- 目标：恢复账号和已有本地身份在 bootstrap 前能幂等获得可用 daemon subkey private package。
- 设计方法：im-core 提供 ensure/migration API，awiki-me 在 package 缺失时调用；无法重签时 fail closed。
- 实现方法：补 recovery 路径、identity store migration、DID auth update_document 调用和 Dart binding。
- 路径：`awiki-cli-rs2/crates/im-core/src/internal/identity_*`、`awiki-cli-rs2/crates/im-core-dart/*`、`awiki-me/lib/src/data/im_core/*`。
- 验证方式：`cargo test -p im-core --locked`、`cargo test -p im-core-dart --locked`、`flutter test`。
- Review 环节：重点看主私钥使用边界、服务端 registry 同步、旧身份兼容和错误恢复。
- Commit 要求：awiki-cli-rs2 / awiki-me 聚焦 commit，必要时按仓拆分。
- 风险：缺少主 key 的身份不能自动补齐，只能提示重新恢复。

### Step 03：awiki-deamon bootstrap private package 早期校验

- 小 Plan：[steps/03-awiki-deamon-bootstrap-key-validation.md](steps/03-awiki-deamon-bootstrap-key-validation.md)
- 目标：Daemon 在创建 binding/message agent 前拒绝坏 private package。
- 设计方法：parse private key、derive public key、对比 package public key、解析当前 DID Document authentication。
- 实现方法：扩展 bootstrap validation、DID resolver/im-core client、测试 bad package case。
- 路径：`awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/inbox/user_delegated.rs`。
- 验证方式：`cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1`。
- Review 环节：重点看 secret redaction、外部 resolver 失败策略、idempotency 和坏状态落库。
- Commit 要求：awiki-cli-rs2 聚焦 commit。
- 风险：网络/解析失败可能影响 bootstrap；需明确 retryable 与 terminal 错误。

### Step 04：key package schema 与 APP action capability 收口

- 小 Plan：[steps/04-schema-capability-hardening.md](steps/04-schema-capability-hardening.md)
- 目标：修正 `private_key_multibase` 承载 PEM 的 schema 问题，并让 APP action 只来自显式 capability policy。
- 设计方法：新增 key package v2 写出，保留 v1 legacy read；新 bootstrap 必须携带 capability，空列表表示禁用。
- 实现方法：更新 Rust/Dart DTO、daemon parser、awiki-me payload、action default 逻辑和文档。
- 路径：`awiki-cli-rs2/crates/im-core/src/identity/dto.rs`、`awiki-me/lib/src/domain/entities/agent/*`、`awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/action.rs`。
- 验证方式：相关 cargo/flutter tests 和 schema fixture tests。
- Review 环节：重点看兼容性、secret 字段命名、能力默认值和旧 binding migration。
- Commit 要求：涉及仓库按聚焦 commit 提交。
- 风险：现有 bootstrap fixture 需要更新；legacy decode 必须覆盖。

### Step 05：ANP SDK DID Document additional authentication optional 参数

- 小 Plan：[steps/05-anp-sdk-did-additional-authentication.md](steps/05-anp-sdk-did-additional-authentication.md)
- 目标：把 DID Document 附加 authentication verification method 下沉到 ANP SDK optional API。
- 设计方法：Python/Rust SDK 在 proof 生成前插入 additional verification method；默认空列表时旧行为完全不变。
- 实现方法：扩展 `create_did_wba_document` / Rust `DidDocumentOptions`，补 proof tests，改 im-core/user-service 消费新 API。
- 路径：`anp/anp/anp/authentication/did_wba.py`、`anp/anp/rust/src/*`、`awiki-cli-rs2/crates/im-core/src/internal/identity_generation.rs`、`user-service/src/user_service/app/did/service.py`。
- 验证方式：ANP Python/Rust tests、im-core identity tests、user-service DID tests。
- Review 环节：重点看旧 API 兼容、proof validity、e1-only 策略和不引入产品专用 runtime storage。
- Commit 要求：ANP SDK commit 先行，消费仓库随后聚焦 commit。
- 风险：跨语言 SDK API 命名需保持一致但不破坏既有发布接口。

### Step 06：最终集成验证、安全 Review 与文档收口

- 小 Plan：[steps/06-final-integration-security-review.md](steps/06-final-integration-security-review.md)
- 目标：跨仓检查 Step 01-05 的契约一致性、验证证据和剩余风险。
- 设计方法：全局 Review + L3 验证 + remote system-test + 文档 drift 检查。
- 实现方法：运行各仓测试、系统测试、schema/naming 搜索、security checklist，并回填台账。
- 路径：`awiki-cli-rs2/docs/agent-im/*`、`awiki-system-test`、各受影响仓库。
- 验证方式：user-service、awiki-cli-rs2、awiki-me、message-service、anp 和 remote system-test。
- Review 环节：重点看 registry/DID Document、bootstrap secret、capability、docs 和未提交变更。
- Commit 要求：如只记录验证不改文件则不提交；如修改文档/测试，创建最终集成 commit。
- 风险：remote 环境不可用时必须记录替代证据和发布阻塞项。

## 10. Review 策略

- 每步骤 Review：实现完成后、commit 前执行；优先看 correctness、回归、公开契约、缺失测试、安全 / 隐私和文档漂移。
- 全局 Review：Step 06 统一检查跨仓契约、schema 命名、撤销语义、旧调用兼容、系统测试证据和工作区状态。
- 契约 / 安全 / 隐私 Review：DID Document proof、private key handling、registry 不保存 private material、bootstrap secret redaction、APP action capability、E2EE 不进入 Agent。
- 文档 Review：更新核心设计文档、delegated proof plan、受影响仓 docs/api、ANP SDK docs 或测试 fixture 说明。

## 11. 验证策略

| 层级 | 命令 / 检查 | 预期证据 |
|---|---|---|
| user-service | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v` | DID auth register/update、registry sync、proof validity、revoke/backfill 测试通过。 |
| awiki-cli-rs2 | `cd awiki-cli-rs2 && cargo test -p im-core --locked && cargo test -p im-core-dart --locked && cargo test -p awiki-deamon --locked -j1` | identity migration、Dart binding、daemon bootstrap validation、action capability 测试通过。 |
| awiki-cli-rs2 Flutter package | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`、`cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | codegen 和 package tests 通过。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | App bootstrap/migration/control payload tests 通过。 |
| anp | `cd anp/anp && uv run pytest anp/unittest/authentication anp/unittest/proof -v`、`cd anp/anp/rust && cargo test --locked` | Python/Rust DID Document additional auth tests 通过。 |
| message-service | `cd message-service && cargo test --workspace` | delegated send/inbox/history 与 fanout 现有测试不回归。 |
| System / E2E | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote ... awiki.info ...` | remote `awiki.info` 关键链路通过；记录实际命令、通过/失败/跳过数量。 |
| Docs / Search | `rg` 检查 `daemon-key-*` 设备化示例、`mailbox_*` 新增命名、`private_key_multibase` 新写路径、未记录风险 | 命名和文档一致；legacy 例外有说明。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 12. 文档更新

- Harness 文档：除非跨仓架构或规则变化，一般不修改 Harness；若行为影响全局边界，更新相关 node/repo profile。
- 子仓库文档：更新 `user-service/docs/api/*`、`awiki-cli-rs2/docs/agent-im/*`、`awiki-cli-rs2/docs/api/im-core-interface/*`、`awiki-me` 相关说明、`anp/anp` SDK docs/tests。
- 本次生成的任务文档：`awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/plan.md` 与 `steps/*.md`。

## 13. Commit 计划

- 每个完成、验证、Review 通过的步骤创建一个聚焦 commit。
- Commit 前记录 `git status --short --branch` 和纳入文件。
- Commit 后记录 commit hash 和工作区状态。
- 多仓 Step 可以按仓拆分 commit，但每个 commit 必须服务于当前 Step。
- 不要把所有步骤的修改积累到一个最终大 commit。

## 14. Blocked 处理

| Blocker | Step | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|---|
| 未出现 | - | - | - | - | - |

- 只有依赖允许且风险已记录时，才继续另一个 pending 步骤；当前计划默认串行。
- 只有没有安全假设、回退方案或独立下一步时，才询问用户。
- 如果 external remote system-test 不可用，Step 06 不应直接标记 done；应记录阻塞或替代证据，并说明是否阻塞发布。

## 15. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-10 | 创建修复专项计划 | 根据实现 Review 发现整理可执行修复方案 | Step 01-06 | 是 |
| 2026-06-10 | Step 02 增加 user-service `update_document` optional metadata preservation 小修 | 实现 im-core 旧身份 migration 时发现 DID Document signed update 若省略 `is_public` / `is_agent` 会被服务端默认落成 `false`，可能破坏既有身份元数据；需先让省略字段保持旧值，旧显式传参行为不变 | Step 02 | 是 |

## 16. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| registry 与 DID Document 状态不一致 | 以当前 DID Document `authentication` 为授权事实；registry 做 reconcile/backfill；测试覆盖 add/remove/replace | 回滚 registry 同步逻辑时保留 DID Document 授权路径，暂停撤销 UI |
| unsigned DID Document mutation 破坏 proof | 禁止服务端无签名 patch；REST create 在 proof 前插入或重签；所有路径补 proof verify test | 回滚到只接受 signed update，禁用 REST delegated public registration |
| 已有身份缺少 main key 无法迁移 | fail closed，提示重新恢复/重新绑定；记录错误码 | 不自动创建 daemon key，禁用 message agent bootstrap |
| bootstrap 早期 DID Document resolve 失败 | 区分 retryable 与 terminal；不创建 binding；保留 bootstrap retry | 回退为仅本地 private/public 校验，但发布前记录风险 |
| key package v2 破坏旧 bootstrap | 保留 v1 legacy decode，新增 fixture | 临时继续写 v1，先只读 v2 |
| APP action 默认值收紧导致旧 binding 行为变化 | legacy binding 明确标记 `policy_source=legacy_default` 或迁移；新 bootstrap 强制显式 policy | 回滚新策略但保留审计 warning |
| ANP SDK optional API 影响旧调用 | 默认空列表；旧 tests 必须通过；新增 API 不改变 wire schema | 回滚消费仓库到本地 patch，但保留 proof tests |

## 17. 最终全局 Review 与整体验证

- 触发条件：Step 01-05 完成、Review、验证并提交后执行。
- Review 范围：`user-service`、`awiki-cli-rs2`、`awiki-me`、`anp/anp`、`message-service`、`awiki-system-test` 相关测试和文档。
- 重点关注：DID Document proof、registry sync、signed revoke/rotate、existing identity migration、bootstrap secret、APP action capability、optional API 兼容、文档漂移、未提交变更。
- 整体验证命令 / 检查：按第 11 节执行，并记录实际命令和结果。
- Review 发现：待 Step 06 回填。
- 已修复问题：待 Step 06 回填。
- 剩余风险：待 Step 06 回填。
- 最终证据：待 Step 06 回填。
- 最终 `git status`：待 Step 06 回填。
- 如果本阶段修改文件：记录 Review、验证和最终集成 commit。
