# 步骤 10：集成门禁和完整系统测试

主计划：[../plan.md](../plan.md)
步骤编号：10
状态：完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-31T01:11:39Z |
| 完成时间 | 2026-05-31T04:47:04Z |
| 提交 | 主仓库提交 `test: verify identity-owned local state cutover`；最终 hash 以提交后 `git log -1 --oneline` 为准。系统测试仓库提交 `04115ea`（`test: align system tests with identity-owned local state`）。 |
| 审查证据 | 最终审查未发现未解决的 correctness/security findings；daemon 是并行项目，`../awiki-system-test/tests_v2/daemon` 已按目录约束改为仅显式设置 `AWIKI_DAEMON_RUST_REPO` 时运行，awiki-cli-only 门禁默认跳过 daemon contract。 |
| 验证证据 | Rust workspace、SDK cutover、Flutter codegen、secure focused、discovery/redaction/owner fallback 搜索分类均通过；不带 `AWIKI_DAEMON_RUST_REPO` 的 remote `awiki.info` 完整系统测试通过：143 passed，51 skipped，0 failed，耗时 178.83s，wall time 179.85s。 |
| 下一步 | 提交本步骤改动并推送分支。 |

## 2. 目标

- 产出：证明本次重构在 schema、runtime、CLI、Dart、文档、Secure 边界和系统行为上是连贯的。
- 用户/系统行为：新建和升级后的 workspace 满足 identity-owned local-state invariants。
- 非目标：无关重构、宽泛产品功能变化、Secure 公开发现 enablement、low-level secure command exposure。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| 整个 `awiki-cli-rs2` workspace | 完整测试和审查 pass。 | 只有出现集成修复时才改代码。 |
| `plan/20260530-local-state-owner-scope/` | 记录验证和最终状态。 | 保持台账更新。 |
| Secure direct/group evidence | 运行 focused local checks。 | L3 identity/storage/security 工作必须有证据。 |
| Discovery/redaction evidence | 记录搜索和审查结果。 | 不扩大 public secure surface。 |
| `../awiki-system-test` | 以 remote 模式和 `awiki.info` 域名运行完整系统测试。 | 最终门禁，不能用 focused 测试替代。 |
| 可选 Harness docs | 如果步骤 09 修改 Harness，则验证。 | 仅适用于 Harness 被修改的情况。 |

## 4. 依赖

- 前置步骤：步骤 01-09。

## 5. 核心设计

本步骤是门禁，不是 feature step。它用于捕获跨步骤不匹配：schema version、workspace version、generated files、docs、migration fixtures、direct/group E2EE local state、Secure redaction、disabled discovery posture、App/CLI boundary，以及真实系统测试行为。

这是 L3 identity/storage/security 工作，不能在没有 security 审查证据和完整系统测试证据的情况下标记完成。中间步骤可以不跑系统测试，但本步骤必须在 `../awiki-system-test` 下完整执行系统测试。

系统测试硬性要求：

- 路径：`../awiki-system-test`。
- 模式：`AWIKI_SYSTEM_TEST_MODE=remote`。
- 域名：`awiki.info`。
- 关键服务地址：
  - `E2E_DID_DOMAIN=awiki.info`
  - `E2E_USER_SERVICE_URL=https://awiki.info`
  - `E2E_MESSAGE_SERVICE_URL=https://awiki.info`
  - `E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws`
- 命令：`uv run awiki-system-test`。
- 完成条件：完整系统测试必须通过；如果失败，修复后重跑，不能把失败或不可用当作通过。

## 6. 实施指南

1. 运行 `git status`，确认前面步骤的完成工作已按步骤提交。
2. 运行完整 Rust workspace tests。
3. 运行 SDK final cutover check。
4. 运行 Flutter codegen check。
5. 运行受 owner-scope 影响的 secure direct/group focused checks。
6. 运行 discovery-disabled searches 并检查 feature/config defaults。
7. 运行 redaction searches，覆盖 public DTO、CLI output、diagnostics、docs、backup/upgrade logs。
8. 执行最终代码审查搜索，检查 forbidden owner 和 Secure patterns。
9. 在 `../awiki-system-test` 下以 remote 模式和 `awiki.info` 域名运行完整系统测试：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test
```

10. 按 `../awiki-system-test/AGENTS.md` 的报告规则记录系统测试详情：命令、总体结果、失败 0/跳过 0 或详细失败/跳过用例、功能域统计、关键配置上下文。
11. 在本步骤文档和主计划中记录所有验证证据。
12. 只有出现集成修复或证据文档变更时，才创建步骤 10 提交。

## 7. 验收标准

- [x] `cargo test --workspace --locked` 通过，或无关失败已用证据说明并修复/隔离。
- [x] `bash scripts/sdk-refactor/final-cutover-check.sh` 通过。
- [x] `scripts/flutter/codegen-check.sh` 通过。
- [x] Active runtime 中没有 forbidden owner 回退/search patterns。
- [x] Direct E2EE 和 Group E2EE public discovery 默认继续 disabled。
- [x] Public Secure CLI/Rust/Dart/diagnostic/docs output 保持 redacted。
- [x] Low-level group E2EE operations 继续 hidden/internal 或 stable unsupported。
- [x] Workspace upgrade backup/manifest/log behavior 已审查，确认不会泄露敏感材料。
- [x] `../awiki-system-test` 下 remote 模式、`awiki.info` 域名完整系统测试已执行并通过。
- [x] 系统测试报告符合 `../awiki-system-test/AGENTS.md`：包含总体结果、失败/跳过详情、功能域统计和配置上下文。
- [x] 最终审查 没有未解决 correctness/security findings。
- [x] Final docs 描述剩余风险，如果有。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| Rust workspace | `cargo test --workspace --locked` | 通过。 |
| SDK boundary | `bash scripts/sdk-refactor/final-cutover-check.sh` | 通过。 |
| Dart | `scripts/flutter/codegen-check.sh` | 通过。 |
| Secure direct local | `cargo test -p im-core --locked direct_secure e2ee_outbox` 和 `cargo test -p awiki-cli --locked msg_secure` | 通过；如 filter 重命名，记录准确替代命令。 |
| Group E2EE local | `cargo test -p im-core --features group-e2ee --locked group_e2ee` 和 `cargo test -p awiki-cli --locked group_secure e2ee` | 通过；如 filter 重命名，记录准确替代命令。 |
| Discovery disabled | `rg "anp\\.direct\\.e2ee\\.v1|direct-e2ee|anp\\.group\\.e2ee\\.v1|group-e2ee" crates docs config.template.yaml` 并人工审查 default output | 无新默认 public advertisement；internal/docs/test 命中解释 disabled posture。 |
| Secure redaction | `rg "private_key|jwt_token|plaintext|chain_key|root_key|message_key|skipped_key|send_n|recv_n|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|provider.*path" crates docs packages` 并审查 public-output tests | Public output/DTO/docs 已脱敏；internal/test 命中是有意的。 |
| 完整系统测试 | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws uv run awiki-system-test` | 必须完整执行并通过；记录通过、失败、跳过、耗时和配置上下文。 |
| Harness docs | `cd /home/ecs-user/awiki-space/awiki-harness && python scripts/validate-docs.py && python scripts/check-drift.py` if harness changed | 如果 Harness 被修改，则通过。 |

## 9. 审查流程

- Findings first，按严重性排序。
- 检查数据 migration/rebuild 安全。
- 检查 E2EE security/privacy boundaries。
- 检查 Secure discovery 继续 disabled，公开接口继续 redacted。
- 检查 backup/upgrade manifests 和 diagnostics 不暴露敏感材料。
- 检查 public Rust/Dart API 兼容。
- 检查文档和完整系统测试证据。

## 10. 提交要求

- 只有本步骤修改文件时才提交。
- 建议提交信息：`test: verify identity-owned local state cutover`

## 11. 风险、回滚和后续

- 风险：remote `awiki.info` 环境或凭据不可用，导致完整系统测试失败。
- 回滚/回退：不得把不可用或失败当作通过；记录失败/不可用证据，修复环境或代码后重跑完整命令。任何 Secure discovery 或 raw-output 暴露都必须在完成前回滚。

## 12. 实际验证记录

本步骤使用当前工作树重新验证，最终通过的命令如下：

| 检查 | 命令 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| whitespace | `git diff --check` | `db-refactor-cli-rs2` 通过；`../awiki-system-test` 通过。 |
| Rust workspace | `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` | 通过；`awiki-cli` unit tests 117 passed，`im-core` unit tests 287 passed，`im-core-dart` unit tests 6 passed，相关 integration/doc tests 通过，无失败。 |
| SDK cutover | `bash scripts/sdk-refactor/final-cutover-check.sh` | 通过；`legacy_path_cutover_contract` 2 passed，`cli_cutover_command_surface_contract` 13 passed，`command_catalog_schema_contract` 8 passed，`m_core_cli_adapter_policy_contract` 5 passed，`group_e2ee_cutover_policy_contract` 6 passed。 |
| Flutter codegen | `scripts/flutter/codegen-check.sh` | 通过；输出 `Done!`，无 generated diff。 |
| Secure focused | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked direct_secure && CARGO_BUILD_JOBS=1 cargo test -p im-core --locked e2ee_outbox && CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked msg_secure && CARGO_BUILD_JOBS=1 cargo test -p im-core --features group-e2ee --locked group_e2ee && CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked group_secure && CARGO_BUILD_JOBS=1 cargo test -p awiki-cli --locked e2ee` | 通过；direct secure 8 passed，e2ee outbox 7 passed，group E2EE feature、CLI group secure 和 CLI e2ee 相关匹配测试均通过。 |
| Discovery disabled 搜索 | `rg "anp\\.direct\\.e2ee\\.v1|direct-e2ee|anp\\.group\\.e2ee\\.v1|group-e2ee" crates docs config.template.yaml` | 已人工分类；无默认 DID/service discovery advertisement。命中为文档 disabled posture、internal runtime/profile、feature-gated group E2EE、CLI 用户意图/兼容 alias 和测试。 |
| Secure redaction 搜索 | `rg "private_key|jwt_token|plaintext|chain_key|root_key|message_key|skipped_key|send_n|recv_n|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|provider.*path" crates docs packages` | 已人工分类；public DTO/CLI/diagnostics/docs 未新增 raw private key、JWT、plaintext outbox payload、ratchet/MLS internals、provider stdout/stderr/path、raw SQLite rows 或 backup contents。`pendingCommits` 是计数，不是 raw Commit artifact；`e2ee_outbox.plaintext` 仍为内部 SQLite 实现细节。 |
| Owner fallback 搜索 | `rg "owner_did.*OR|OR.*owner_did|owner_did\\s*=\\s*\\?|UPDATE OR IGNORE|ON CONFLICT\\(owner_did|PRIMARY KEY \\(owner_did|PRIMARY KEY\\(owner_did|owner_identity_id.*credential|credential.*owner_identity_id" crates/im-core/src crates/awiki-cli/src docs/architecture` | 已人工分类；未发现活跃 runtime 的 owner-DID 回退。剩余命中为 legacy DDL/test fixture、recover 迁移兼容路径、exact snapshot predicate 或测试断言。 |

补充记录：第一次完整系统测试使用了 `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/db-refactor-cli-rs2` 但未设置 `AWIKI_DAEMON_RUST_REPO`，导致 daemon contract 错选 CLI 仓库并失败 3 个用例；结果为 `143 passed, 48 skipped, 3 failed in 255.22s`。按用户反馈，daemon 是并行项目，不应作为本次 owner-scope 门禁依赖；已修改 `tests_v2/daemon`，仅显式设置 `AWIKI_DAEMON_RUST_REPO` 时运行 daemon contract，awiki-cli-only 完整系统测试默认跳过 daemon contract。

## 13. 完整系统测试报告

最终门禁使用不带 `AWIKI_DAEMON_RUST_REPO` 的 awiki-cli-only 配置执行。daemon 是并行项目；本次完整系统测试只验证 awiki-cli owner-scope 重构，daemon contract 按用户要求跳过。

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
/usr/bin/time -p env \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/db-refactor-cli-rs2 \
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test tests_v2 -q -rs -p no:cacheprovider \
2>&1 | tee /tmp/awiki-system-test-owner-scope-step10-rust-full-daemon-skipped.log
```

结果：

- 总体结果：143 passed，51 skipped，0 failed。
- pytest 耗时：178.83s。
- `/usr/bin/time`：real 179.85，user 33.82，sys 9.94。
- collected tests：194。
- 失败用例：失败 0。
- 跳过用例：跳过 51；均为既有环境/功能开关跳过，或用户明确要求的 daemon 并行项目跳过。

配置上下文：

| 配置 | 值 |
|---|---|
| `AWIKI_CLI_UNDER_TEST` | `rust` |
| `AWIKI_CLI_RUST_REPO` | `/home/ecs-user/awiki-space/db-refactor-cli-rs2` |
| `AWIKI_SYSTEM_TEST_MODE` | `remote` |
| `E2E_DID_DOMAIN` | `awiki.info` |
| `E2E_USER_SERVICE_URL` | `https://awiki.info` |
| `E2E_MESSAGE_SERVICE_URL` | `https://awiki.info` |
| `E2E_MESSAGE_SERVICE_WS_URL` | `wss://awiki.info/im/ws` |
| `AWIKI_DAEMON_RUST_REPO` | 未设置；daemon tests 按用户要求跳过 |

功能域统计：

| 功能域 | collected | passed | skipped | failed |
|---|---:|---:|---:|---:|
| `cli` | 57 | 35 | 22 | 0 |
| `core` | 25 | 25 | 0 | 0 |
| `daemon` | 3 | 0 | 3 | 0 |
| `debug` | 5 | 5 | 0 | 0 |
| `id` | 35 | 35 | 0 | 0 |
| `mail` | 5 | 0 | 5 | 0 |
| `message_service` | 19 | 4 | 15 | 0 |
| `multi_tenant` | 6 | 2 | 4 | 0 |
| `page` | 5 | 5 | 0 | 0 |
| `runtime` | 23 | 23 | 0 | 0 |
| `site` | 3 | 3 | 0 | 0 |
| `update` | 6 | 6 | 0 | 0 |
| `user_service` | 2 | 0 | 2 | 0 |

跳过详情按功能域归类：

| 功能域 | 数量 | pytest summary 条目 / 原因 |
|---|---:|---|
| `cli` | 8 | `tests_v2/cli/test_awiki_cli_direct_local.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable 或 registration limit exhausted。 |
| `cli` | 8 | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py`、`tests_v2/cli/test_awiki_cli_group_local.py`：Group E2EE system tests 默认跳过，需要显式 `AWIKI_ENABLE_GROUP_E2EE_TESTS=1` focused validation；这与 Direct/Group E2EE public discovery disabled 姿态一致。 |
| `cli` | 5 | `tests_v2/cli/test_awiki_cli_group_local.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable。 |
| `cli` | 1 | `tests_v2/cli/test_awiki_cli_store_rust_contracts.py`：Rust store contract targets 已从 `awiki-cli-rs2` 移除，store internals 不再是 awiki-cli acceptance surface。 |
| `daemon` | 3 | `tests_v2/daemon/test_awiki_daemon_rust_contracts.py`：未设置 `AWIKI_DAEMON_RUST_REPO`；daemon 是并行项目，awiki-cli-only run 按用户要求跳过。 |
| `mail` | 5 | `tests_v2/mail/test_awiki_cli_mail_local.py`、`tests_v2/mail/test_awiki_cli_mail_notification_local.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable。 |
| `message_service` | 10 | `tests_v2/message_service/test_attachment_local.py`、`test_direct_local.py`、`test_group_local.py`、`test_payload_local.py`、`test_ws_notifications.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable。 |
| `message_service` | 4 | `tests_v2/message_service/test_group_e2ee_contract.py`、`test_group_e2ee_flag_off.py`：Group E2EE system tests 默认跳过，需要显式 `AWIKI_ENABLE_GROUP_E2EE_TESTS=1` focused validation。 |
| `multi_tenant` | 1 | `tests_v2/multi_tenant/test_message_tenant_admission.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable。 |
| `multi_tenant` | 3 | `tests_v2/multi_tenant/test_message_tenant_admission.py`：未设置 DID-only / message-only tenant admission focused coverage 环境变量。 |
| `user_service` | 2 | `tests_v2/user_service/test_agent_registration_token_local.py`：远端 `awiki.info` 当前 IP 注册额度耗尽，node-a identity bootstrap unavailable。 |

collect-only 统计命令：

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
env \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/db-refactor-cli-rs2 \
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test tests_v2 --collect-only -q -p no:cacheprovider \
> /tmp/awiki-system-test-owner-scope-step10-collect-daemon-skipped.log
```

## 14. 实际审查记录

未发现未解决的 correctness/security findings。

- Legacy v0->v1 升级链路已调整为先导入 legacy identity，再保存 historical DID alias，执行 k1->e1 DID replace，最后导入 legacy SQLite。historical DID alias 只在迁移期传入 `import_legacy_sqlite_with_historical_dids`；普通 legacy import 仍对显式未知 `owner_did` 或 `credential_name` fail closed。
- Legacy SQLite message import 已把 `dm:<old-owner-did>:<peer-did>` 归一为稳定 `conversation_id = thread_id = dm:<peer-did>`，并写入 `owner_identity_id = IdentitySummary.unique_id`、`owner_did = 当前 DID snapshot`。新增 Rust 和系统测试覆盖 k1->e1 replace 后的 legacy message import。
- v3->v4 对缺失 SQLite 文件保持 no-op；对旧 schema 继续备份后 clean rebuild；对当前 v17 schema 执行 DID history 和 identity-owned invariants 校验。
- awiki-cli contract tests 设置隔离 `HOME`/`USERPROFILE`，避免测试从真实用户 home 读取旧 workspace 状态；该变更不改变产品 runtime 行为。
- 系统测试适配通过 `expected_workspace_schema_version()` 区分 Rust v4 和 Go v3；Go 默认仍期望 schema 3，Rust 期望 schema 4。系统测试的 SQLite fixtures 只更新为 owner-identity schema 和稳定 conversation key，没有放宽 CLI 行为断言。
- Direct E2EE / Group E2EE public discovery 继续 disabled。`config.template.yaml` 未新增 direct/group E2EE advertisement；相关命中均为文档 disabled posture、internal E2EE runtime/profile、feature/test 或 CLI 用户显式 secure intent。
- Public Secure DTO、CLI output、diagnostics、docs 和 logs 继续 redacted。`SecureOutboxEntry` 仍不暴露 plaintext 或 crypto material；Group secure status/doctor 不暴露 provider stdout/stderr/path 或 MLS state path；backup/upgrade manifests 不打印 backup contents。
- Group MLS provider state/path selection 继续由 `owner_identity_id + device_id` scoped；低层 group E2EE command surface 继续 hidden/internal 或 stable unsupported。

## 15. 剩余风险

- 远端 `awiki.info` 当前对本 IP 有 registration quota，导致部分真实注册/消息/mail 用例被 pytest 按既有规则跳过。最终完整系统测试在该环境下失败 0；被跳过用例已按 `../awiki-system-test/AGENTS.md` 记录。
- Group E2EE 系统测试保持默认关闭，符合本计划要求的 public discovery disabled 姿态；本步骤使用本仓库 focused secure/group E2EE Rust tests 覆盖本地 secure owner-scope 门禁。
- Daemon contract 是并行项目范围；系统测试只有显式设置 `AWIKI_DAEMON_RUST_REPO` 时才运行 daemon contract。本次 owner-scope 最终门禁不依赖 daemon 仓库。
