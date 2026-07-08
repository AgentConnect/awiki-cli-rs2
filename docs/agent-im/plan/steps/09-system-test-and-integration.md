# Step 09：系统测试与集成收口

主 Plan：[../plan.md](../plan.md)  
Step index：09  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T19:08:53Z |
| Completed | 2026-06-09T20:36:30Z |
| Commit | `agent-im: finalize system integration`；最终短 hash 以提交后 `git rev-parse --short HEAD` 为准 |
| Review evidence | readiness Review：Step 01-08 均已 done 且有 commit/验证证据；`awiki-system-test` 当前入口是 `uv run awiki-system-test`，remote 域名通过 `AWIKI_SYSTEM_TEST_MODE=remote` / `E2E_DID_DOMAIN=awiki.info` 控制。最终全局 Review 发现并修复：DID Document 写入 `#daemon-key-1` 后 W3C proof 失效、老 CLI 和 group-e2ee feature-gated struct literal 缺少 optional 默认值、awiki-cli identity contract 仍断言远端固定 DID、设计文档中 message-service 授权源 / daemon key fragment / user-service public method 边界残留。 |
| Verification evidence | `user-service` DID tests 32 passed；`im-core` 269 lib tests + integration/doc tests passed；`awiki-cli` 全量测试通过；`awiki-cli` offline build 通过；`awiki-deamon` 93 lib passed + integration suites passed；`group-e2ee` feature targeted test 1 passed；`im-core-dart` 6 unit + 13 facade passed；Dart codegen Done；`awiki_im_core` Flutter tests 12 passed；`message-service` workspace tests passed；`awiki-me` analyze clean + 272 tests passed；remote `awiki.info` system test 185 passed / 16 skipped；naming check 和 `git diff --check` 通过。 |
| Next action | 已完成；提交后核对所有仓库状态 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：验证 APP bootstrap -> Daemon user delegated identity -> ensure message agent -> delegated inbox/send -> message sync/action -> APP 展示/执行 的完整链路。
- 用户 / 系统可见行为：真实或接近真实环境中，用户创建 DID 后可一次性启用消息处理 Agent；普通消息可被 Agent 处理；E2EE 消息不进入 Agent；系统 payload 不污染聊天。
- 非目标：不把失败的 remote 系统测试标记为通过；不在此步骤补做大规模功能开发，除非是集成修复且先更新 Plan。
- 完成标准：所有前置 Step done；全局 Review 完成；`awiki-system-test` remote `awiki.info` 模式执行并记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置；最终工作区状态记录。

## 3. 设计方法

- 设计边界：本步骤是集成收口和证据收集，不替代各步骤 unit/integration tests。
- 核心决策：系统测试必须使用 `AWIKI_SYSTEM_TEST_MODE=remote` 与 `awiki.info`；命令示例可调整，但实际命令必须记录。
- 契约 / API / 数据流：测试覆盖 user-service DID key、message-service delegated policy、awiki-me bootstrap、awiki-deamon ensure/sync/action。
- 兼容性：同时跑必要旧链路 smoke，确认普通 APP messaging、Daemon agent command、E2EE opaque boundary 不回归。
- 迁移策略：如涉及数据库迁移，先在测试环境执行迁移并记录版本。
- 风险控制：remote 环境不可用或依赖服务未部署时，记录 blocker 和替代证据，不伪造通过。

## 4. 实现方法

1. 阅读 `awiki-system-test` 的 README、脚本帮助和现有测试用例，确认 remote `awiki.info` 模式实际命令。
2. 如现有系统测试缺少覆盖，新增或扩展测试用例：
   - 用户 DID 创建默认包含 `#daemon-key-1`；
   - APP 侧生成 daemon subkey private package，user-service 只登记 public registration；
   - 同一 APP 重复提交相同 public key 幂等，提交不同 public key conflict；
   - APP 通过 message-service 普通消息发送 `awiki.daemon.bootstrap.v1` 明文 JSON system/control payload；
   - bootstrap private package 在 MVP 中经过普通消息发送路由到 Daemon，但不显示为普通聊天、不进入 Hermes prompt、日志或 audit detail；
   - Daemon 写入 user delegated identity；
   - Daemon `ensure_app_message_agent` 幂等；
   - message-service delegated send/inbox/history 只处理普通非 E2EE；
   - message-service MVP 校验 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope；
   - 同 DID APP/Daemon 多连接 fanout；
   - E2EE opaque notification 到 Daemon 后 ignored；
   - 普通非 E2EE 消息进入 Hermes 前使用 untrusted content envelope，`message_event` 使用最小投影；
   - APP action 最小 allowlist 和 payload filter。
3. 执行所有相关仓库的最终验证命令，并把结果回填主 Plan 第 7、11、17 节和本 Step 执行状态。
4. 执行全局 Review：检查所有 changed files、public schema、文档、测试、迁移、安全/隐私边界、错误命名和未提交变更。
5. 如果系统测试或全局 Review 需要小修，先更新 Plan 变更记录，再做修复、验证、Review、commit。
6. 最终记录每个仓库 `git status --short --branch`，确保没有遗漏未提交完成工作。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-system-test` | 新增或运行端到端测试 | 按仓库 README 和实际脚本 |
| `awiki-cli-rs2/docs/agent-im/plan/plan.md` | 回填执行台账、最终 Review、验证证据 | 文档更新属于本步骤 |
| `awiki-cli-rs2/docs/agent-im/plan/steps/*.md` | 回填各 Step 状态或最终证据 | 如实现过程中已回填则本步骤核对 |
| `user-service` | 最终 status 和必要 smoke | 不做新功能 |
| `message-service` | 最终 status、迁移和 smoke | 不做新功能 |
| `awiki-cli-rs2` | 最终 status、cargo tests | 不做新功能 |
| `awiki-me` | 最终 status、Flutter tests | 不做新功能 |

## 6. 依赖

- 前置步骤：Step 01-08 已完成、Review、验证并提交。
- 外部文档或决策：`awiki-cli-rs2/AGENTS.md` 对系统测试的强制要求；`awiki-harness/context/40-verification.md` 与 `50-task-workflow.md`。
- 环境前提：`awiki-system-test` 可访问 remote `awiki.info`；必要服务已部署或可由测试脚本配置。

## 7. 验收标准

- [x] Step 01-08 的执行状态均为 `done`，commit hash 和验证证据完整。
- [x] 所有相关仓库最终 unit/integration 验证完成，无法运行的命令记录原因和替代证据。
- [x] remote `awiki.info` 系统测试执行并记录实际命令。
- [x] 系统测试记录通过/失败/跳过数量、失败或跳过原因、关键环境配置。
- [x] 全局 Review 没有未处理 P0/P1 问题；剩余风险已记录。
- [x] 错误命名检查通过，不出现新增旧候选 naming family，所有新增收件箱授权命名保持 `inbox_*`。
- [x] 最终 `git status --short --branch` 已记录。
- [x] 如果本步骤修改测试或文档，已经创建聚焦最终集成 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| user-service | `cd user-service && uv run pytest tests/app/did -v` | 记录通过/失败/跳过数量。 |
| awiki-cli-rs2 | `cd awiki-cli-rs2 && cargo test -p im-core --locked && cargo test -p awiki-deamon --locked` | 记录通过/失败/跳过数量或 Rust test summary。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | analyze clean，tests 通过。 |
| message-service | `cd message-service && cargo test --workspace` | workspace tests 通过；如运行 clippy 也记录。 |
| system remote | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 uv run awiki-system-test --show-command` | 实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。当前 `awiki-system-test` 入口没有 `--domain` 参数，remote `awiki.info` 通过环境变量控制。 |
| naming | `PATTERN="$(printf '%s|%s|%s|%s|%s|%s' 'message_''owner|message_''auth' 'Message''Access' 'Scoped''Message' 'mailbox_''owner' 'Scoped''Mailbox' 'Scoped''MailboxToken')" && rg -n "$PATTERN" awiki-cli-rs2/docs/agent-im/plan awiki-cli-rs2/docs/agent-im/*.md` | 不出现错误命名残留；若检查命令本身被命中，需调整为脚本变量形式。 |
| final status | 对每个相关仓库运行 `git status --short --branch` | 没有遗漏未提交完成工作；允许用户既有无关变更但必须记录。 |

如果 `awiki-system-test` 当前命令参数不同，执行者必须先查看仓库 README 或脚本帮助，使用实际命令，并在主 Plan 和本 Step 中记录差异。

## 9. Review 环节

- Review 时机：系统测试前做一次 readiness Review；系统测试后做最终全局 Review；最终 commit 前再核对工作区。
- Review 重点：跨仓库契约一致性、E2EE boundary、子私钥安全、runtime token scope、APP action allowlist、schema 可见性、system-test 证据、Plan 台账完整性、未提交变更。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 发现 4 类集成问题 | DID Document proof 在 daemon key 写入后失效；老 CLI / group-e2ee struct literal 缺少新增 optional 字段默认值；身份 contract 仍断言远端 mock DID；设计文档仍有 message-service 授权源、daemon key fragment、user-service public method 边界残留。 |
| 已修复问题 | 已修复并验证 | im-core 对更新后的 DID Document 重新签名并补 proof 测试；老调用显式传 `None` 保持 optional 兼容；identity tests 改为断言本地生成 key-bound DID；两篇设计文档和 Plan 统一授权/命名/所有权边界。 |
| 剩余风险 | 已记录，无未处理 P0/P1 | MVP 明文 bootstrap；daemon subkey 本地存储安全升级后置；`#daemon-key-1` 仍是 DID authentication key；E2EE Agent 处理不进入 MVP；remote mail health HTTP 502 skip；撤销实时性依赖 DID Document 刷新。 |
| 新增或缺失测试 | 已补充必要测试 | 新增 DID proof 重新签名单元测试和 registration integration proof 校验；补老 CLI / feature-gated 编译覆盖；未新增 awiki-system-test 用例，使用当前 remote 全量套件作为系统证据。 |
| 已更新或缺失文档 | 已更新 | 主 Plan 第 7、15、17 节、本 Step、Step 02/04 小 Plan、两篇设计文档已更新；最终集成提交为 `agent-im: finalize system integration`，实际短 hash 在提交后由 `git rev-parse --short HEAD` 核对。 |

## 10. Commit 要求

- Commit 时机：本步骤新增/修改系统测试、计划台账或收口文档并验证后。
- Commit 范围：只包含系统测试、最终文档证据和必要集成修复；大功能修复必须回到对应 Step 或更新 Plan。
- Commit 前状态：记录每个相关仓库的 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`agent-im: add system integration coverage`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| remote `awiki.info` 环境不可用 | 待填写实际错误 | 重试、检查脚本帮助、确认环境变量、运行局部替代测试 | 整体上线验收 | 标记 blocked，不伪造通过，等待环境恢复或用户决策 |
| 系统测试脚本缺少目标用例 | 待填写 | 新增 focused E2E 用例或记录 manual evidence | 当前步骤 | 修改测试后 Review/commit |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 09 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 校准 remote 系统测试入口 | `awiki-system-test` README 和 `uv run awiki-system-test --help` 显示当前入口为 `uv run awiki-system-test`，没有 `manage_local_test_env.py run-tests --domain` 参数；remote `awiki.info` 通过环境变量控制。 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 增加 `awiki-cli` optional 字段兼容修复 | remote system test 构建 `awiki-cli` 时发现 Step 02 新增 optional 字段没有在老 CLI struct literal 中显式填默认值，导致 `cargo build --bin awiki-cli --offline` 失败；修复为显式 `None` 默认值，保持老调用行为不变。 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 修复 DID Document proof 重新签名 | 最终集成 Review 发现 im-core 在签名 DID Document 后追加 `#daemon-key-1` 会导致 W3C proof 失效；改为追加 daemon public method 后用 `#key-1` 重新签名。 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 完成 remote `awiki.info` 系统测试和最终文档收口 | 按 AGENTS 和主 Plan 要求执行 remote system test，记录实际命令、通过/跳过数量、skip 原因、剩余风险和最终状态。 | [../plan.md#17-最终全局-review-与整体验证](../plan.md#17-最终全局-review-与整体验证) |

## 13. 风险、回滚与后续文档

- 风险：MVP 明文 bootstrap 和 daemon subkey 现有 secret 存储方式仍是已接受安全债；remote system test 中 mail local 相关 4 项因 `awiki-mail-service /mail/health` HTTP 502 跳过；撤销实时性依赖 DID Document 刷新。
- 回滚 / 回退：如 Step 09 集成修复回滚，需要同时回滚 DID Document 重新签名测试、老 CLI optional 默认值修复和 identity contract 期望；若 remote 环境异常，保留当前 185 passed / 16 skipped 证据并重跑。
- 后续文档：后续版本需单独设计普通消息 body 加密、secure key store、E2EE Agent participant / explicit forward、Agent DID delegation / ANP delegated proof。
