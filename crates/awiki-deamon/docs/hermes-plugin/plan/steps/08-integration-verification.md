# Step 08: 整体验证、系统测试与发布门禁

主计划: [../plan.md](../plan.md)  
步骤编号: 08  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 未开始 |
| 完成时间 | 未完成 |
| 提交 | 未提交 |
| 审查证据 | 待记录 |
| 验证证据 | 待记录 |
| 下一步 | 等 Step 01-07 完成后，执行 repo、focused E2E 和完整 remote 系统测试 |

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

- [ ] Step 01-07 均为 `done`，且每步有 review 和 commit 记录。
- [ ] 当前仓库 `cargo fmt --all --check` 通过。
- [ ] 当前仓库 `cargo test -p awiki-deamon --locked` 通过。
- [ ] 当前仓库 `cargo test --workspace --locked` 通过，或失败原因与替代验证被清楚记录。
- [ ] daemon/Hermes focused 系统测试 有实际命令和结果。
- [ ] 完整系统测试已在 `../awiki-system-test` 执行，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名。
- [ ] 完整系统测试记录通过/失败/跳过数量、失败或跳过原因、关键环境配置。
- [ ] L3 安全 review 完成并记录。
- [ ] 文档同步完成。
- [ ] 如有本步骤文件变更，review 后创建聚焦提交。

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
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

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
