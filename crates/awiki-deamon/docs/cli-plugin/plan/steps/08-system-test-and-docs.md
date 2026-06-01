# Step 08：系统测试、文档收口与全局 Review

主 Plan：[../plan.md](../plan.md)  
Step index：08  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/codex-plugin-cli-rs2`；`awiki-system-test` 分支 `release/0526` 未修改 |
| Started | 2026-06-01 18:57:28 +0800 |
| Completed | 2026-06-01 20:28:21 +0800 |
| Commit | 步骤提交：`docs: record generic cli codex validation`，hash 以 `git log` 为准 |
| Review evidence | 已完成最终全局 Review；发现 remote full system test 未通过和一个 service-run hang 干预，作为剩余风险记录；未发现需要修改当前 generic-cli/Codex 实现的阻塞问题 |
| Verification evidence | 本仓 format、daemon crate、workspace cargo、daemon acceptance wrapper 已通过；remote full system test 已按 `remote` + `awiki.info` 执行但结果为 47 failed / 89 passed / 59 skipped |
| Next action | 后续优先处理 remote full system test 剩余失败域，或在环境恢复后重跑 remote suite |

## 2. 目标

- 结果：用本仓测试、daemon acceptance 和远端 `awiki.info` 完整系统测试验证前面步骤；同步设计/运行文档；完成最终全局 Review。
- 用户 / 系统可见行为：计划完成后，有证据证明 generic-cli/Codex create/run/callback/msg.send 不破坏现有 daemon、Hermes、payload、registration token 和系统测试。
- 非目标：不把 remote 环境失败伪装成通过；不在系统测试中引入不清理的持久测试数据。
- 完成标准：记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置、Review 发现和最终 `git status`。

## 3. 设计方法

- 设计边界：`codex-plugin-cli-rs2` 负责 daemon/runtime 实现；`awiki-system-test` 负责跨服务 E2E 证据。
- 核心决策：系统测试分两层：
  1. focused daemon acceptance：验证当前 Rust checkout 的 daemon contract。
  2. remote full system test：验证远端 `awiki.info` 模式整体行为。
- 契约 / API / 数据流：新增或更新 daemon system tests 时，必须通过 `AWIKI_DAEMON_RUST_REPO` 显式选择当前 Rust checkout。
- 兼容性：现有 `test-runtime-uds` long-running E2E 保持，新增 generic-cli/Codex fake driver E2E 不应取代它。
- 迁移策略：如果修改 `awiki-system-test/tests_v2/daemon` 文件列表或语义，同步 `awiki-system-test/tests_v2/daemon/CLAUDE.md`。
- 风险控制：remote 测试跳过/失败按功能域和原因记录；不能只写总数。

## 4. 实现方法

1. 扩展 `awiki-system-test/tests_v2/daemon/test_awiki_daemon_rust_contracts.py`，必要时加入新的 focused cargo test selector。
2. 如需要 E2E，新增或扩展 daemon long-running test：
   - create `runtime=codex` 或 `runtime=generic-cli, driver_id=codex`。
   - 用 fake Codex binary 或 test driver 触发 wrapper/local RPC。
   - 验证 `runtime_plugin_id=generic-cli`、`driver_id=codex`、status/final、msg.send policy/audit。
3. 同步 `awiki-system-test/tests_v2/daemon/CLAUDE.md`。
4. 更新 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/local-dev.md`：
   - CLI family create alias。
   - `generic-cli` plugin type 与 driver_id。
   - Codex callback env 不含 `AWIKI_DAEMON_TASK_TEXT`。
   - msg.send recipient policy。
5. 更新 `generic_cli_runtime_plugin_design.md` 中“当前状态”与“验收标准”，只写已经实现的事实。
6. 执行最终本仓验证：
   - format
   - daemon crate tests
   - workspace cargo tests
   - grep gates
7. 执行 daemon acceptance wrapper。
8. 在 `awiki-system-test` 执行 remote full system test：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 \
CARGO_BUILD_JOBS=1 \
uv run --no-sync awiki-system-test
```

9. 记录测试报告：
   - 总体结果：通过、失败、跳过、耗时、实际命令。
   - 失败用例：测试文件/用例名、功能域、失败数量、原因。
   - 跳过用例：测试文件/用例名或 pytest summary、功能域、跳过数量、原因。
   - 配置上下文：`AWIKI_SYSTEM_TEST_MODE`、user-service URL、message-service URL、WebSocket URL、DID domain、`AWIKI_DAEMON_RUST_REPO`。
10. 执行最终全局 Review，修复或记录残余风险。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-system-test/tests_v2/daemon/test_awiki_daemon_rust_contracts.py` | 如有新增 cargo contract，扩展 selector | 需要显式 repo selector |
| `awiki-system-test/tests_v2/daemon/test_awiki_daemon_long_running_e2e.py` | 可新增 generic-cli/Codex fake driver E2E | 避免破坏现有 UDS test runtime |
| `awiki-system-test/tests_v2/daemon/CLAUDE.md` | daemon acceptance 范围变化时同步 | 受 AGENTS 约束 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/docs/local-dev.md` | 更新 local dev 和验证说明 | 不复制本机绝对路径 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md` | 同步已实现事实和残余缺口 | 不把未实现写成已完成 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan/plan.md` | 回填执行台账、Review 和验证证据 | 本 Plan source of truth |

## 6. 依赖

- 前置步骤：Step 01-07 全部完成并提交。
- 外部文档或决策：`awiki-system-test/AGENTS.md`、`awiki-system-test/README.md`、Harness verification policy。
- 环境前提：remote `awiki.info` 测试所需凭据、网络和服务可用；否则按跳过/失败规则记录。

## 7. 验收标准

- [x] 本仓 `cargo fmt --all --check` 通过。
- [x] 本仓 `cargo test -p awiki-deamon --locked` 通过。
- [x] 本仓 `cargo test --workspace --locked` 通过或记录非本任务失败原因。
- [x] daemon acceptance wrapper 通过或记录跳过/失败原因。
- [x] remote full system test 在 `AWIKI_SYSTEM_TEST_MODE=remote`、`awiki.info` 下执行，并记录 pass/fail/skip 详情；本次未通过，失败域已记录为剩余风险。
- [x] `generic-cli` 文档只说 runtime plugin type，不说 plugin id 或消息 routing key。
- [x] `AWIKI_DAEMON_TASK_TEXT` 不出现在真实 Codex run 生产路径。
- [x] `runtime.cli.*` 只作为 legacy migration/alias 出现。
- [x] `awiki-system-test` 未修改；无需同步测试数据清理和 `CLAUDE.md`。
- [x] 最终全局 Review 发现已修复或明确记录。
- [x] 如本步骤修改文件，已经创建聚焦 commit；提交信息为 `docs: record generic cli codex validation`，hash 以 `git log` 为准。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 通过。 |
| Daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | 通过。 |
| Workspace | `cd codex-plugin-cli-rs2 && cargo test --workspace --locked` | 通过或记录非本任务失败。 |
| Daemon acceptance | 从 `codex-plugin-cli-rs2` 执行：`cd ../awiki-system-test && AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q -rs` | focused daemon acceptance 通过或记录跳过/失败。 |
| Remote full system test | 从 `codex-plugin-cli-rs2` 执行：`cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync awiki-system-test` | 记录通过、失败、跳过、耗时和关键配置。 |
| Legacy grep | `cd codex-plugin-cli-rs2 && rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 只允许 legacy migration/alias/docs 说明。 |
| Secret grep | `cd codex-plugin-cli-rs2 && rg -n "AWIKI_DAEMON_TASK_TEXT|rtok_|runtime_rpc_token.*println|danger-full-access|dangerously-bypass" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 生产路径不泄漏 token；危险 flags 不默认启用。 |

如果某个命令不能运行，必须记录原因、影响和替代证据；remote full system test 不能用 focused test 直接替代。

### 8.1 实际执行记录

| 检查项 | 实际命令 / 环境 | 结果 |
|---|---|---|
| Format | `cargo fmt --all --check` | 通过。 |
| Daemon crate | `cargo test -p awiki-deamon --locked` | 通过；unit 29 passed；integration：`agent_registration_management` 10 passed，`generic_cli_runtime_mvp` 21 passed，`hermes_contracts` 5 passed，`hermes_gateway` 6 passed + 1 ignored，`hermes_message` 8 passed，`hermes_profile` 3 passed，`local_rpc_security` 13 passed，`state_bootstrap` 2 passed。 |
| Workspace | `cargo test --workspace --locked` | 通过；覆盖 `awiki-cli`、`awiki-deamon`、`im-core`、`im-core-dart` 和 doc-tests；无失败。 |
| Daemon acceptance 首次运行 | `cd ../awiki-system-test && AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q -rs` | 首次失败：1 passed、2 failed；原因是构建/链接阶段磁盘空间不足。已清理本仓生成的 `target/debug`、`target/tmp` 后重跑。 |
| Daemon acceptance 重跑 | 同上 | 通过：3 passed in 321.42s，0 failed，0 skipped。 |
| Remote full system test | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync awiki-system-test` | 已执行但未通过：89 passed、47 failed、59 skipped in 959.46s。关键环境：`AWIKI_SYSTEM_TEST_MODE=remote`、`E2E_DID_DOMAIN=awiki.info`、remote user-service `https://awiki.info`、remote message-service WebSocket `wss://awiki.info/im/ws`、`AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2`、`CARGO_BUILD_JOBS=1`。 |
| Remote hang 干预 | remote full system test 期间检查长时间运行进程 | `tests_v2/cli/test_awiki_cli_service_run_local.py::test_awiki_cli_runtime_listener_service_run_starts_and_exposes_runtime_artifacts` 启动的 `awiki-cli runtime listener service-run` 约 12 分钟无进展；已对该测试子进程发送 `SIGTERM`，pytest 随后继续并产出最终 summary。 |
| Legacy grep | `rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini|runtime\\.cli/" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 剩余命中均为 legacy migration、alias、兼容测试或计划/兼容说明；未发现新建路径把 CLI 家族写成新的 `runtime.cli.*` type。 |
| Secret / sandbox grep | `rg -n "AWIKI_DAEMON_TASK_TEXT|rtok_|runtime_rpc_token.*println|danger-full-access|dangerously-bypass" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 剩余命中为测试断言、字段名、redaction/secret handling、Hermes fake token 或文档说明；未发现真实 Codex run 生产路径注入 `AWIKI_DAEMON_TASK_TEXT` 或默认危险 sandbox。 |
| Path grep | 使用本机绝对路径、file/vscode URI 和工作区目录名模式搜索 `crates/awiki-deamon/docs/cli-plugin`、`crates/awiki-deamon/docs/local-dev.md`、`crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md`、`crates/awiki-deamon/src`、`crates/awiki-deamon/tests` | 无命中；本步骤相关文档没有新增本机绝对路径。 |
| Diff whitespace | `git diff --check` | 通过。 |

### 8.2 Goal 完成审计重跑记录

2026-06-01 20:28:21 +0800，按 Goal 完成审计又基于当前 checkout 重跑核心门禁：

| 检查项 | 实际命令 / 环境 | 结果 |
|---|---|---|
| Format | `cargo fmt --all --check` | 通过。 |
| Diff whitespace | `git diff --check` | 通过。 |
| Daemon crate | `CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked` | 通过；unit 29 passed；integration：`agent_registration_management` 10 passed，`generic_cli_runtime_mvp` 21 passed，`hermes_contracts` 5 passed，`hermes_gateway` 6 passed + 1 ignored，`hermes_message` 8 passed，`hermes_profile` 3 passed，`local_rpc_security` 13 passed，`state_bootstrap` 2 passed。 |
| Workspace | `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` | 通过；覆盖 `awiki-cli`、`awiki-deamon`、`im-core`、`im-core-dart` 和 doc-tests。重跑前一次 workspace 验证因本机磁盘空间被旧 checkout 构建产物占满，在 `awiki-cli` test binary 链接阶段报 `No space left on device`；清理 sibling repo 的生成目录后可用空间恢复，随后同一单 job workspace 命令通过。 |
| Legacy grep | `rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini|runtime\\.cli/" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 剩余命中均为 legacy migration、alias、兼容测试或文档说明；未发现新建路径把 CLI 家族写成新的 `runtime.cli.*` type。 |
| Secret / sandbox grep | `rg -n "AWIKI_DAEMON_TASK_TEXT|rtok_|runtime_rpc_token.*println|danger-full-access|dangerously-bypass" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 剩余命中为测试断言、字段名、redaction/secret handling、Hermes fake token 或文档说明；未发现真实 Codex run 生产路径注入 `AWIKI_DAEMON_TASK_TEXT` 或默认危险 sandbox。 |
| 最终工作区状态 | `git status --short --branch` | 本仓：`feature/release-0526/codex-plugin-cli-rs2...origin/feature/release-0526/codex-plugin-cli-rs2 [ahead 9]`，无未提交文件；`awiki-system-test`：`release/0526...origin/release/0526`，无未提交文件。 |

Remote full system test 失败域：

| 领域 | 数量 / 用例范围 | 主要原因 |
|---|---|---|
| Direct secure / attachment CLI | 5 | local outbox secure status 输出契约与期望不一致；secure repair 遇到 `message service rpc error 1404: no available prekey bundle for target DID`；attachment dry-run 参数契约已变化。 |
| Group / group E2EE CLI | 7 | group create/member/send 和 group E2EE live flows 在 remote 模式下失败，疑似远端服务或 CLI contract drift。 |
| Host notify probes | 4 | file sink、Hermes、OpenClaw、failure probe 失败；部分命令在当前 Rust CLI 中不可用或配置缺失。 |
| Runtime listener / service-run | 3 | runtime listener local probe、identity registration error、service-run hang；其中 service-run 已人工发送 `SIGTERM` 解除卡住。 |
| Secure init / repair / retry | 3 | direct init / repair / retry remote 契约不稳定或远端 prekey/outbox 条件不满足。 |
| Core output / debug / identity / runtime / page / tenant | 25 | dry-run 输出、debug DB、identity import/create/use、runtime host-notify、page required flags、tenant config 等 CLI contract drift。 |

Remote skip 主要原因：

- 多个用例因 `AWIKI_CLI_UNDER_TEST=rust` 未设置而跳过。
- daemon long-running E2E 要求 local topology，在 `remote` 模式下跳过。
- mail 相关用例因 `awiki-mail-service /mail/health` 返回 HTTP 502 跳过。
- multi-tenant 可选环境 selector 未设置。
- local-only topology 用例在 remote 模式下跳过。
- group E2EE flag-off selector 用例按条件跳过。

## 9. Review 环节

- Review 时机：全部验证完成后、最终 commit 前；如果系统测试或文档在 Review 后修改，需重新做 targeted Review。
- Review 重点：跨步骤一致性、schema compatibility、agent DID routing、runtime plugin type 语义、local RPC security、recipient policy、Codex sandbox/token/prompt、system-test cleanup、文档漂移。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 发现 remote full system test 未通过；发现 service-run hang 需要人工 `SIGTERM`；发现架构和 local dev 文档中仍有旧 `runtime.cli.*` / 泛化 `runtime.cli/` 表述 | 这些不是当前本仓 cargo/focused daemon acceptance 的失败，但属于最终远端验收剩余风险和文档漂移 |
| 已修复问题 | 已把 `awiki_agent_runtime_host_architecture.md` 的 CLI 示例改为 `generic-cli + driver_id`；已把 `local-dev.md` 和 `generic_cli_runtime_plugin_design.md` 同步到当前实现；已明确消息按 Runtime Agent DID 路由，`generic-cli` 是 runtime plugin type 不是消息 routing key | 本步骤没有修改应用代码 |
| 剩余风险 | remote full system test 仍有 47 failed / 59 skipped；真实远端 Codex smoke 和 direct-e2ee `msg.send` 端到端证据仍不足；container/sandbox 硬隔离、cleanup job、完整 Codex JSONL parser、失败 final 仍是后续缺口 | 已在设计文档和本步骤验证记录中明确 |
| 新增或缺失测试 | 未新增 `awiki-system-test` 用例；focused daemon acceptance 已通过；remote full suite 已执行但未通过 | `awiki-system-test` 工作区未修改，因此无需同步 `CLAUDE.md` |
| 已更新或缺失文档 | 已更新 `generic_cli_runtime_plugin_design.md`、`local-dev.md`、`awiki_agent_runtime_host_architecture.md`、主 Plan 和本 Step 文档 | Harness 文档未改；当前变更只收口子仓文档 |

## 10. Commit 要求

- Commit 时机：系统测试/文档/Review 完成后。
- Commit 范围：如果修改 `codex-plugin-cli-rs2` 文档或 tests，创建本仓聚焦 commit；如果修改 `awiki-system-test`，在该仓创建独立聚焦 commit。
- Commit 信息建议：
  - `test: cover generic cli daemon runtime`
  - `docs: record generic cli codex validation`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：remote full system test 受环境影响失败或跳过。缓解：按系统测试报告规则记录具体用例和原因；等待环境恢复后重跑。
- 假设：`awiki-system-test` 可用 `AWIKI_DAEMON_RUST_REPO` 指向当前 checkout。
- 回滚：如果系统测试文件引入不稳定用例，回退 `awiki-system-test` commit；本仓功能 commit 不应因测试 flaky 被无证据回退。
