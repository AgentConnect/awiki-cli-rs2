# Step 08: 整体验证、系统测试与发布门禁

主计划: [../plan.md](../plan.md)  
步骤编号: 08  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-06-01 01:15:27 +0800 |
| 完成时间 | 2026-06-01 01:42:41 +0800 |
| 提交 | 验证记录提交 `b39df1829e1906cae89ba9fad99db2850021e4a3` |
| 审查证据 | 2026-06-01 01:37:15 +0800 完成集成 review：Step 01-07 均有提交和 review 记录；当前仓库格式、daemon 全量、workspace 全量、边界搜索和 secret 搜索通过；focused daemon remote 系统测试通过 3、失败 0、跳过 1；完整 remote `awiki.info` 系统测试已执行但失败 61、通过 66、跳过 68，发布门禁未通过。 |
| 验证证据 | 启动前当前仓库 `git status --short --branch` 无未提交变更；`../awiki-system-test` 无未提交变更；当前仓库 `cargo fmt --all --check`、`cargo test -p awiki-deamon --locked`、`cargo test --workspace --locked`、`git diff --check -- crates/awiki-deamon` 通过；focused daemon 系统测试 `3 passed, 1 skipped, 0 failed, 1 warning in 196.89s`；完整 remote 系统测试 `61 failed, 66 passed, 68 skipped, 1 warning in 918.23s`。 |
| 下一步 | 发布前需要修复完整 remote suite 失败或由 CI/环境补跑通过 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：完成 Hermes Runtime Plugin 的整体验证、系统测试、代码 review、文档同步和发布门禁记录。
- 系统可见结果：当前仓库测试、focused daemon/Hermes 系统测试、最终完整 remote `awiki.info` 系统测试均有实际命令和详细结果；失败或跳过原因被记录；残余风险明确。
- 非目标：不在本步骤实现大功能；只允许修复验证发现的小问题、补系统测试和文档记录。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon` | 修复整体验证发现的小问题；补测试 | 大功能必须回到对应 step 或更新计划。 |
| `crates/awiki-deamon/docs/hermes-plugin/plan/` | 更新执行账本、review 证据、验证证据、残余风险 | 中文。 |
| `crates/awiki-deamon/docs/hermes-plugin/` | 更新设计/运行文档 | 如实现与设计偏离。 |
| `../awiki-system-test/tests_v2/daemon/` | 新增或更新 Hermes focused E2E | 跨仓变更需遵守 awiki-system-test AGENTS。 |
| `../awiki-system-test/README.md` 或 docs | 如新增测试入口或环境变量，更新文档 | 中文优先，遵守该仓规则。 |

## 4. 依赖

- 前置步骤：Step 01-07 全部完成。
- 外部文档或决策：`../awiki-system-test/AGENTS.md`、`../awiki-system-test/README.md`、Harness verification policy。
- 环境前置条件：
  - 当前仓库 Rust toolchain；
  - `../awiki-system-test` 已 `uv sync` 或 runner 可自动处理；
  - remote 服务使用 `awiki.info`；
  - 如真实 Hermes 测试启用，需要 `AWIKI_HERMES_BIN`；
  - 若 remote 注册限额或服务不可用，必须记录，不得伪造通过。

## 5. 设计

### 验证层级

本步骤至少覆盖：

- L1：当前仓库 Rust 格式、unit/integration tests。
- L2：daemon/Hermes focused 系统测试。
- L3：local RPC token、controller DID、recipient scope、DID 私钥隔离、direct-e2ee 边界的安全 review。
- 最终完整系统测试：按用户要求在 `../awiki-system-test` remote 模式、`awiki.info` 域名执行。

### 系统测试设计

建议在 `../awiki-system-test/tests_v2/daemon/` 新增或扩展：

```text
test_awiki_daemon_hermes_runtime_e2e.py
```

覆盖用例：

1. 创建 daemon agent 和 Hermes runtime agent；
2. Hermes profile/Skills 初始化成功；
3. controller 发 text/plain；
4. daemon foreground 消费消息；
5. fake 或真实 Hermes Gateway 收到 prompt；
6. Hermes 通过 local RPC 上报 running 和 final；
7. controller history 收到 status/final；
8. Hermes `send-message` 给目标 DID，目标 DID history/inbox 收到 direct message；
9. non-controller 消息不触发执行；
10. recipient scope 越权返回失败且无外发。

对于 remote `awiki.info`：

- 如果真实 Hermes binary 不适合在 remote suite 中依赖，可使用 fake Hermes gateway 环境变量或 test runtime fixture，但必须明确该测试验证的是 daemon/Hermes adapter contract，不是 Hermes 模型质量。
- 若 direct-e2ee 环境不可用，可 direct plain 作为系统测试最小门禁，direct-e2ee 记录为未运行并说明阻塞。

### 最终完整系统测试报告格式

必须记录：

- 实际命令；
- 模式：`AWIKI_SYSTEM_TEST_MODE=remote`；
- DID 域名：`E2E_DID_DOMAIN=awiki.info`；
- user-service URL；
- message-service HTTP URL；
- message-service WS URL；
- Hermes binary/fake gateway 配置；
- 总体通过、失败、跳过、耗时；
- 失败用例列表、功能域、失败原因；
- 跳过用例列表或 pytest summary、功能域、跳过原因；
- 关键日志或 artifact 路径；
- 残余风险。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 08 标记为 `in_progress`。
2. 运行当前仓库检查：
   - `cargo fmt --all --check`
   - `cargo test -p awiki-deamon --locked`
   - `cargo test --workspace --locked`
   - 边界和 secret 搜索。
3. 对 Step 01-07 所有 review 记录做整合审查：
   - 是否每步都有提交；
   - 是否每步 review 发现已修复；
   - 是否存在 carry-over uncommitted changes。
4. 在 `../awiki-system-test` 新增或确认 focused Hermes 测试：
   - 遵守该仓 AGENTS 报告规则；
   - 增加 cleanup，避免持久测试数据残留；
   - 如果需要 fake Hermes gateway，环境变量命名清楚并写文档。
5. 运行 focused 系统测试：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info \
uv run awiki-system-test tests_v2/daemon
```

6. 按用户要求运行完整系统测试：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info \
uv run awiki-system-test
```

7. 统计测试结果：
   - pytest summary；
   - passed/failed/skipped；
   - 失败用例域和原因；
   - 跳过用例域和原因；
   - 关键环境配置。
8. 做 integration review：
   - 行为契约；
   - local RPC/token；
   - `msg.send` 真实外发；
   - session mapping；
   - docs drift；
   - security/privacy；
   - system-test coverage。
9. 修复验证发现的小问题；若发现范围性设计问题，先更新主计划变更日志，并回到对应步骤。
10. 更新本计划账本和验证记录。
11. 如本步骤有文件变更，创建聚焦提交。

## 7. 验收标准

- [x] Step 01-07 均为 `done`，且每步有 review 和 commit 记录。
- [x] 当前仓库 `cargo fmt --all --check` 通过。
- [x] 当前仓库 `cargo test -p awiki-deamon --locked` 通过。
- [x] 当前仓库 `cargo test --workspace --locked` 通过。
- [x] daemon/Hermes focused 系统测试 有实际命令和结果。
- [x] 完整系统测试已在 `../awiki-system-test` 执行，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名。
- [x] 完整系统测试记录通过/失败/跳过数量、失败或跳过原因、关键环境配置。
- [x] L3 安全 review 完成并记录。
- [x] 文档同步完成。
- [x] 如有本步骤文件变更，review 后创建聚焦提交。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 当前仓库格式 | `cargo fmt --all --check` | 通过。 |
| daemon 测试 | `cargo test -p awiki-deamon --locked` | 通过。 |
| workspace 测试 | `cargo test --workspace --locked` | 通过或明确记录失败原因与替代验证。 |
| 边界搜索 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | 无结果。 |
| 禁止 Hermes plugin | `rg -n "plugin.yaml|plugins/awiki-runtime|tools.py|__init__.py" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无生产安装逻辑。 |
| secret 搜索 | `rg -n "rtok_|runtime_rpc_token.*println|auth_private_key|jwt_token" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无 token/private key/JWT 原文日志。 |
| focused 系统测试 | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info uv run awiki-system-test tests_v2/daemon` | 记录 passed/failed/skipped 和原因。 |
| 完整 system-test | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info uv run awiki-system-test` | 必须执行并记录完整统计。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon ../awiki-system-test` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 集成 review 检查全部行为、contract compatibility、测试覆盖、docs drift、安全边界、残余风险。
- 系统测试报告必须符合 `../awiki-system-test/AGENTS.md`：失败 0 和跳过 0 也要明确写出。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 完整 remote `awiki.info` 系统测试失败 61 个；失败横跨 CLI direct/group/host-notify/listener/core/debug/id/message-service/runtime/page 等域，不是单一 Hermes daemon regression；代表性错误包括 message-service HTTP 502、CLI contract summary drift、Hermes host-notify setup 找不到 `scripts/hermes_notify_adapter.py`、`awiki-cli runtime listener service-run` 子进程长时间不退出。 | 发布门禁未通过，不能声明 remote ready。 |
| 已修复 | 本步骤未做生产代码修复；验证阶段只记录结果。为让完整 pytest 产出 summary，已对卡住的 `awiki-cli runtime listener service-run` 子进程执行 `kill -TERM 2244707`，pytest 随后继续并记录对应失败。 | 不把 focused 通过替代为完整通过。 |
| 残余风险 | 真实 Hermes `StdioHermesGateway` 的 `session.create`/`prompt.submit` 仍未接线；没有真实 `AWIKI_HERMES_BIN` smoke；direct-e2ee remote suite 存在失败；完整系统测试 cleanup 在 remote 模式下尝试访问本地 PostgreSQL `127.0.0.1:5432` 失败并警告可能有测试数据残留。 | 发布前需要修复完整 remote suite 或由合适 CI/远端环境补跑通过。 |
| 测试新增或缺失 | 未新增系统测试文件；复用了现有 `tests_v2/daemon` focused wrapper 和完整 `tests_v2` suite。focused daemon remote 覆盖 daemon Rust contracts；long-running daemon foreground E2E 为 local-only，在 remote 模式被跳过。 | Hermes fake route 已由当前仓库 tests 覆盖；real Hermes 端到端仍缺。 |
| 文档更新或缺失 | 主计划和本步骤记录所有命令、关键环境、通过/失败/跳过数量、失败/跳过原因和残余风险。 | `../awiki-system-test` 无文件变更。 |

## 10. 提交要求

- 提交时机：验证、review、修复和文档记录完成后。
- 提交范围：系统测试、验证记录、小修复和文档同步；不得混入新大功能。
- 提交前状态：记录当前仓库和 `../awiki-system-test` 的 `git status --short --branch`。
- 纳入文件：按仓库记录纳入提交的文件。
- 提交后证据：记录每个仓库 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`test: verify hermes runtime integration`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| remote `awiki.info` 注册限额或服务不可用 | HTTP status、pytest summary、skip reason | 重跑 focused；检查配置；尝试更小 scope | 最终完整系统测试 | 记录为阻塞/失败/跳过原因，不能写通过 |
| 完整系统测试耗时或资源超限 | 命令输出、被 kill 信号、耗时 | 运行 focused suites 作为替代；记录未完成完整 suite | 发布门禁 | 需要用户或 CI 环境补跑完整 suite |
| 新增系统测试产生持久残留 | cleanup 日志、DB 检查 | 补 cleanup helper | awiki-system-test 提交 | 修复清理后才能提交 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 13. 风险、回滚与后续

- 风险：remote 环境不可控导致完整系统测试失败/跳过；真实 Hermes binary 与 fake gateway 覆盖范围不同；direct-e2ee 可能需要更多前置数据。
- 回滚/fallback：如果 release gate 失败，不发布 Hermes ready；保留 fake/local 证据作为开发验证，不作为生产通过结论。
- 后续文档：将最终验证结果写入 plan 执行账本；如需要可新增 `release-validation.md`。

## 14. Step 08 执行记录

### 已执行范围

- 当前仓库 L1/L3 验证：格式、daemon crate、workspace、边界搜索、Hermes plugin 禁止项搜索、secret 搜索。
- `../awiki-system-test` focused daemon 验证：remote `awiki.info` 模式，显式指定 `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2`。
- `../awiki-system-test` 完整系统测试：remote `awiki.info` 模式，显式指定 `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2`，执行 `tests_v2 -q -ra` 以输出失败和跳过摘要。
- 集成 review：检查 Step 01-07 账本、提交、review 记录、安全边界和 residual risks。

### 当前仓库验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked` | 通过：58 passed，0 failed，1 ignored，doc tests 0。 |
| `cargo test --workspace --locked` | 通过：`awiki-cli`、`awiki-deamon`、`im-core`、`awiki_im_core`、`xtask` 和 doc-tests 均无失败。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |
| `rg -n "crates/awiki-cli\|awiki_cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过：无命中，命令退出码 1 表示未找到。 |
| `rg -n "plugin.yaml\|plugins/awiki-runtime\|tools.py\|__init__.py" crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过但有预期命中：仅测试断言 `plugins/awiki-runtime/plugin.yaml`、`tools.py` 不存在，以及契约文档测试“不写 plugin.yaml”；无生产安装逻辑。 |
| `rg -n "rtok_\|runtime_rpc_token.*println\|auth_private_key\|jwt_token" crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过但有预期命中：测试 fake token/脱敏断言、既有 agent private key/JWT 状态字段、foreground JWT 选项传递、diagnostic 敏感标记列表和 fake token placeholder；未发现 token 原文 println/log。 |

### Focused daemon 系统测试

实际命令：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info \
AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2 \
uv run awiki-system-test tests_v2/daemon
```

关键环境：

- `AWIKI_SYSTEM_TEST_MODE=remote`
- `E2E_DID_DOMAIN=awiki.info`
- `E2E_USER_SERVICE_URL=https://awiki.info`
- `E2E_MESSAGE_SERVICE_URL=https://awiki.info`
- `E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info`
- `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2`
- `AWIKI_HERMES_BIN` 未设置，未启用真实 Hermes binary smoke。

结果：

- 通过：3
- 失败：0
- 跳过：1
- 警告：1
- 耗时：196.89s

跳过原因：

- `tests_v2/daemon/test_awiki_daemon_long_running_e2e.py`：`This test requires the local tests_v2 topology.`；该 long-running daemon foreground E2E 在 remote 模式被设计为跳过。

警告：

- cleanup warning：`message_service PostgreSQL cleanup failed: psql: error: connection to server at "127.0.0.1", port 5432 failed: Connection refused`；remote 模式下清理器尝试访问本地 PostgreSQL，可能有测试创建数据残留。

### 完整 remote 系统测试

实际命令：

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info \
AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2 \
uv run awiki-system-test tests_v2 -q -ra
```

关键环境：

- `AWIKI_SYSTEM_TEST_MODE=remote`
- `E2E_DID_DOMAIN=awiki.info`
- `E2E_USER_SERVICE_URL=https://awiki.info`
- `E2E_MESSAGE_SERVICE_URL=https://awiki.info`
- `E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info`
- `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/hermes-plugin-cli-rs2`
- `AWIKI_CLI_UNDER_TEST` 未设置，默认 `go`，因此 Rust awiki-cli selector 类用例按设计跳过。
- `AWIKI_ENABLE_GROUP_E2EE_TESTS` 未设置，Group E2EE focused system tests 按设计跳过。
- `AWIKI_HERMES_BIN` 未设置，没有真实 Hermes binary/system E2E。

结果：

- 通过：66
- 失败：61
- 跳过：68
- 警告：1
- 耗时：918.23s，即 0:15:18
- 结论：完整 remote 发布门禁未通过。

执行中的异常处理：

- pytest 执行期间 `awiki-cli runtime listener service-run` 子进程 `pid=2244707` 长时间不退出，超过对应测试自身 10s 观测窗口后仍运行约 12 分钟；为让 pytest 继续产出完整 summary，执行 `kill -TERM 2244707`。随后 pytest 继续运行，并将 `tests_v2/cli/test_awiki_cli_service_run_local.py::test_awiki_cli_runtime_listener_service_run_starts_and_exposes_runtime_artifacts` 记为失败。

失败用例与功能域：

| 功能域 | 失败数 | 用例 |
|---|---:|---|
| CLI direct / secure / attachment | 8 | `tests_v2/cli/test_awiki_cli_direct_local.py::{test_awiki_cli_msg_secure_status_failed_and_drop_use_local_outbox_without_services, test_awiki_cli_msg_secure_status_migrates_legacy_config_json_before_local_outbox_read, test_awiki_cli_can_send_direct_messages_and_mark_them_read, test_awiki_cli_can_send_secure_direct_messages_with_manual_reply_confirmation, test_awiki_cli_secure_direct_handle_queries_hide_raw_wire_cache_and_mark_read, test_awiki_cli_can_send_and_download_direct_attachments, test_awiki_cli_msg_and_attachment_commands_validate_local_arguments, test_awiki_cli_inbox_scope_all_limit_and_mark_read_work}` |
| CLI group / group attachment | 4 | `tests_v2/cli/test_awiki_cli_group_local.py::{test_awiki_cli_can_create_group_add_member_send_and_list_messages, test_awiki_cli_can_update_members_remove_and_leave_groups, test_awiki_cli_can_join_open_group_and_use_show_alias, test_awiki_cli_can_send_and_download_group_attachments}` |
| CLI host-notify probes | 6 | `tests_v2/cli/test_awiki_cli_host_notify_failure_local.py::test_awiki_cli_host_notify_failure_local_probe_succeeds`; `tests_v2/cli/test_awiki_cli_host_notify_file_sink_local.py::test_awiki_cli_host_notify_file_sink_local_probe_succeeds`; `tests_v2/cli/test_awiki_cli_host_notify_hermes_local.py::test_awiki_cli_host_notify_hermes_local_probe_succeeds`; `tests_v2/cli/test_awiki_cli_host_notify_openclaw_local.py::test_awiki_cli_host_notify_openclaw_local_probe_succeeds`; `tests_v2/cli/test_awiki_cli_host_notify_openclaw_main_only_local.py::test_awiki_cli_host_notify_openclaw_main_only_probe_succeeds`; `tests_v2/cli/test_awiki_cli_host_notify_openclaw_webhook_only_probe.py::test_awiki_cli_host_notify_openclaw_webhook_only_probe_succeeds` |
| CLI listener / local cache / secure commands | 6 | `tests_v2/cli/test_awiki_cli_msg_inbox_local_cache.py::test_awiki_cli_msg_inbox_cutover_does_not_fallback_to_local_cache`; `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::{test_awiki_cli_runtime_listener_local_probe_succeeds, test_awiki_cli_runtime_listener_local_identity_reports_registration_error}`; `tests_v2/cli/test_awiki_cli_secure_init_local.py::test_awiki_cli_msg_secure_init_is_stable_unsupported_without_direct_init_wire`; `tests_v2/cli/test_awiki_cli_secure_repair_local.py::test_awiki_cli_msg_secure_repair_resets_state_and_sends_manual_direct_init`; `tests_v2/cli/test_awiki_cli_secure_retry_local.py::test_awiki_cli_msg_secure_retry_is_stable_unsupported_without_outbox_flush` |
| CLI service-run | 1 | `tests_v2/cli/test_awiki_cli_service_run_local.py::test_awiki_cli_runtime_listener_service_run_starts_and_exposes_runtime_artifacts` |
| Core / output contracts | 4 | `tests_v2/core/test_basic_commands.py::{test_config_show_rejects_deprecated_service_url_fields, test_go_planned_stub_commands_return_frozen_contract_hints}`; `tests_v2/core/test_output_contracts_cli.py::{test_representative_dry_run_commands_emit_plan_payloads, test_trace_timing_preserves_cli_output_channels}` |
| Debug CLI | 5 | `tests_v2/debug/test_debug_cli.py::{test_debug_db_query_is_cutover_unsupported_without_opening_store, test_debug_db_query_unsupported_boundary_keeps_legacy_config_untouched, test_debug_db_import_v1_imports_seeded_legacy_sqlite_without_message_service, test_debug_db_handle_history_reads_contact_bindings, test_debug_db_import_v1_supports_dry_run_and_missing_path_errors}` |
| Identity CLI | 9 | `tests_v2/id/test_identity_cli.py::{test_id_create_list_current_use_and_status, test_id_create_generates_default_anp_message_service, test_id_create_and_use_support_dry_run_and_argument_validation, test_id_use_unknown_identity_returns_not_found, test_id_bind_email_send_requires_auth_and_supports_registered_identity, test_id_import_v1_imports_flat_legacy_identity, test_id_import_v1_all_imports_flat_and_indexed_legacy_identities, test_id_import_v1_reports_missing_legacy_layout, test_id_replace_did_diagnostic_command_dry_run_warns_about_danger_and_backup}` |
| message-service attachment | 2 | `tests_v2/message_service/test_attachment_local.py::{test_same_domain_attachment_control_and_download, test_same_domain_group_attachment_ticket_and_download}` |
| message-service direct / E2EE / capabilities | 5 | `tests_v2/message_service/test_direct_local.py::{test_get_capabilities_exposes_direct_group_and_attachment_profiles, test_same_domain_direct_send_inbox_mark_read_and_history, test_direct_e2ee_get_prekey_bundle_returns_top_level_one_time_prekey_when_available, test_direct_e2ee_get_prekey_bundle_require_opk_returns_unavailable_without_opks, test_direct_e2ee_rejects_legacy_hpke_style_wire_objects}` |
| message-service group | 1 | `tests_v2/message_service/test_group_local.py::test_same_domain_group_create_invite_send_and_list` |
| message-service auth / payload / ws | 4 | `tests_v2/message_service/test_jsonrpc_auth_http_status.py::test_message_service_jsonrpc_missing_bearer_auth_uses_http_401`; `tests_v2/message_service/test_payload_local.py::{test_direct_json_payload_round_trips_in_inbox_history_and_ws, test_group_json_payload_round_trips_in_message_list_and_ws}`; `tests_v2/message_service/test_ws_notifications.py::test_same_domain_ws_receives_direct_incoming` |
| multi-tenant CLI | 1 | `tests_v2/multi_tenant/test_awiki_cli_tenant_config.py::test_awiki_cli_id_create_uses_tenant_did_domain_but_platform_message_service` |
| page CLI | 1 | `tests_v2/page/test_page_cli.py::test_page_required_flags_are_enforced_like_go_cobra` |
| runtime / Hermes host-notify CLI | 4 | `tests_v2/runtime/test_runtime_cli.py::{test_runtime_host_notify_validates_inputs_and_supports_dry_run, test_runtime_host_notify_hermes_guide_status_and_config_commands_work, test_runtime_host_notify_webhook_bridge_service_run_reaches_hermes_preflight, test_runtime_host_notify_hermes_setup_writes_local_files}` |

失败原因概括：

- 远端 service 可用性：多处 direct/group/message-service 用例返回 `message service http error 502` 或相关 HTTP/WS 失败，说明 `https://awiki.info` message-service remote 不是全量测试的稳定通过环境。
- CLI contract drift：部分本地 contract 期望与当前 CLI 输出不一致，例如 secure status summary 从 `Loaded direct secure status` 变为 `Loaded 0 secure session(s) and N secure outbox record(s)`。
- Hermes host-notify 环境/安装：`runtime host-notify hermes setup` 失败，提示 `could not locate scripts/hermes_notify_adapter.py next to the awiki-cli installation`。
- Listener 进程生命周期：`awiki-cli runtime listener service-run` 在完整 suite 中长期不退出，需要人工 SIGTERM 才能让 pytest 继续。
- cleanup 警告：remote 模式 cleanup 尝试访问本地 PostgreSQL `127.0.0.1:5432`，连接失败，可能留下测试创建数据。

跳过用例与原因汇总：

| 跳过类别 | 数量 | 原因 |
|---|---:|---|
| Rust awiki-cli selector | 30 | `AWIKI_CLI_UNDER_TEST` 未设置为 `rust`，相关 Rust contract/focused selector 按设计跳过。 |
| Group E2EE focused system tests | 11 | `AWIKI_ENABLE_GROUP_E2EE_TESTS` 未设置，Group E2EE focused validation 按设计跳过。 |
| focused Rust-under-test workspace upgrade selector | 23 | workspace upgrade selector 仅在 focused Rust-under-test 模式运行。 |
| mail tests | 4 | `awiki-mail-service /mail/health returned HTTP 502`。 |
| local-only tests | 3 | daemon long-running foreground、mail notification、message-service direct local-only 用例要求 local tests_v2 topology。 |
| multi-tenant optional env | 3 | 未设置 `E2E_MESSAGE_V2_DID_ONLY_DOMAIN` / `E2E_MESSAGE_V2_MESSAGE_ONLY_DID`。 |
| runtime optional selector | 3 | Hermes bridge service management / local host-notify selector 需要 `AWIKI_CLI_UNDER_TEST=rust`。 |

> 注：pytest summary 总跳过数为 68，上表按 summary 条目人工归类；部分条目为 `SKIPPED [2]` 或 `SKIPPED [4]`，已按数量累计。

### L3 安全 review

- Controller DID：Step 04/07 的 controller text path 仍通过 `run_controller_text_task` 校验 `sender_did == controller_did`；Step 07 非 controller foreground route 测试确认 gateway 前拒绝。
- Runtime token：local RPC 授权仍从 token 和内部 state 反查 context，不信任请求体 spoof 字段；secret 搜索未发现 token 原文日志。
- Recipient scope：Step 05 后 controller text run token 默认 `allowed_recipients = Some(controller_did)`；Hermes `msg.send` 不能默认发任意 DID。
- DID 私钥隔离：Hermes profile/session 表不保存 DID private key/JWT/runtime token；daemon 持有 agent identity 并通过 `im-core` 发送。
- E2EE 边界：daemon 只把 `direct_e2ee` 映射到 `MessageSecurityMode::SecureDirect`，不处理 E2EE key；完整 remote suite 中 direct-e2ee 相关用例失败，不能声明 remote L3 通过。
- Prompt 边界：prompt wrapper 不含 runtime token/private key/JWT；`agent-status` `last_error` 已对 token/JWT/private key/secret 片段 fail-closed。

### 最终结论

- 当前仓库实现与 focused daemon contracts 通过。
- 完整 remote `awiki.info` 系统测试已实际执行，但失败 61；本步骤记录为 release gate failed。
- 不发布 Hermes real-ready 结论；fake/local 证据只证明 daemon 内部 contract，不能替代真实 Hermes binary、remote message-service 和 direct-e2ee 端到端通过。

### 提交前状态

- 当前仓库 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 15]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/08-integration-verification.md
```

- `../awiki-system-test git status --short --branch`：

```text
## release/0526...origin/release/0526
```

- 纳入当前仓库提交文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/08-integration-verification.md`

### 提交后状态

- 验证记录提交：`b39df1829e1906cae89ba9fad99db2850021e4a3`。
- 当前仓库提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 16]
```
- `../awiki-system-test`：无文件变更，无提交。
