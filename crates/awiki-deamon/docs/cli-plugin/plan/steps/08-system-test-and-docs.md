# Step 08：系统测试、文档收口与全局 Review

主 Plan：[../plan.md](../plan.md)  
Step index：08  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `feature/release-0526/codex-plugin-cli-rs2`；如修改系统测试则记录 `awiki-system-test` 分支 |
| Started | 待记录 |
| Completed | 待记录 |
| Commit | 待记录 |
| Review evidence | 待记录 |
| Verification evidence | 待记录 |
| Next action | 补 daemon acceptance、remote full system test 和文档同步 |

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

- [ ] 本仓 `cargo fmt --all --check` 通过。
- [ ] 本仓 `cargo test -p awiki-deamon --locked` 通过。
- [ ] 本仓 `cargo test --workspace --locked` 通过或记录非本任务失败原因。
- [ ] daemon acceptance wrapper 通过或记录跳过/失败原因。
- [ ] remote full system test 在 `AWIKI_SYSTEM_TEST_MODE=remote`、`awiki.info` 下执行，并记录 pass/fail/skip 详情。
- [ ] `generic-cli` 文档只说 runtime plugin type，不说 plugin id 或消息 routing key。
- [ ] `AWIKI_DAEMON_TASK_TEXT` 不出现在真实 Codex run 生产路径。
- [ ] `runtime.cli.*` 只作为 legacy migration/alias 出现。
- [ ] `awiki-system-test` 如有修改，测试数据清理和 `CLAUDE.md` 已同步。
- [ ] 最终全局 Review 发现已修复或明确记录。
- [ ] 如本步骤修改文件，已经创建聚焦 commit。

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

## 9. Review 环节

- Review 时机：全部验证完成后、最终 commit 前；如果系统测试或文档在 Review 后修改，需重新做 targeted Review。
- Review 重点：跨步骤一致性、schema compatibility、agent DID routing、runtime plugin type 语义、local RPC security、recipient policy、Codex sandbox/token/prompt、system-test cleanup、文档漂移。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待记录 |  |
| 已修复问题 | 待记录 |  |
| 剩余风险 | 待记录 |  |
| 新增或缺失测试 | 待记录 |  |
| 已更新或缺失文档 | 待记录 |  |

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
