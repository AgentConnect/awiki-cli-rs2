# Step 07：最终集成、文档同步、remote 系统测试

主 Plan：[../plan.md](../plan.md)  
Step index：07  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | TBD |
| Started | TBD |
| Completed | TBD |
| Commit | TBD |
| Review evidence | TBD |
| Verification evidence | TBD |
| Next action | 执行全局 Review、整体验证、idle 对比、文档同步和 `awiki-system-test` remote `awiki.info` 完整系统测试。 |
| Assigned agent | coordinator |
| Parallel group | 串行 |
| Parallel safe | no |
| Parallel with | 无 |
| Conflict resources | 全部已改模块、docs、Harness 摘要、remote `awiki.info` 测试环境 |
| Baseline commit | TBD，必须包含 Step 02-06 完成结果 |
| Worktree / branch | TBD |
| Merge gate | Step 02-06 均 done，所有 step commit 已记录。 |
| Verification gate | workspace tests、idle final evidence、remote system test、global Review。 |
| Gate status | pending |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：确认事件驱动改造端到端可用，CPU / I/O 静默成本相对 Step 01 基线下降，direct/group runtime message、queue、heartbeat、WSS reconnect/fallback、shutdown、docs 和共享 SDK 兼容性均有证据。
- 用户 / 系统可见行为：远端 `awiki.info` 环境下完整系统测试通过或明确记录失败 / 跳过原因；daemon docs 能解释新调度模型；主 Plan 台账可用于恢复和审计。
- 非目标：不在 final 阶段引入新的大功能；不把未验证的共享 API 改动混入最终集成；不跳过 AGENTS 要求的 remote system test。
- 完成标准：所有 step 状态 done、全局 Review 完成、必要 docs 更新或记录无需更新理由、`cargo test --workspace --locked` 和 remote `awiki-system-test` 结果记录、最终 `git status` 清晰、最终集成 commit 只在确实修改 docs / fixes 时创建。

## 3. 设计方法

- 设计边界：final 阶段负责集成验证、补文档和小修，不做架构重写；发现大问题要回到对应 Step 或更新 Plan。
- 核心决策：验证顺序从本仓库 unit / workspace 到 idle 证据，再到 remote system test，最后做 docs / Harness drift 检查和全局 Review。
- 契约 / API / 数据流：重点审计 `im-core` 是否只有批准范围内改动；message-service protocol 是否未变；daemon source metadata 是否只在 daemon 内部。
- 兼容性：如果 `crates/im-core` 有 diff，必须确认 Step 04 证据、`awiki-cli` / `im-core-dart` 回归和 docs 更新。
- 迁移策略：无数据库 schema migration 时记录“无迁移”；如前面步骤引入迁移，final 必须验证升级 / 空状态目录 / 旧状态目录。
- 风险控制：remote test 失败不能简单忽略；必须记录命令、环境、通过/失败/跳过数量、失败或跳过原因和剩余风险。

## 4. 实现方法

1. 执行执行台账审计：
   - 每个 Step 状态必须为 `done` 或明确 `blocked` 且用户接受风险。
   - 每个 Step 记录 commit hash、Review 证据、验证证据和遗留风险。
   - 确认没有未提交的跨 Step 混杂改动。
2. 做全局 diff Review：
   - 检查 foreground 调度是否没有 250ms 全量直接 / group / queue 扫描残留。
   - 检查 per-agent `ImClient` 生命周期是否复用。
   - 检查 queue scheduler、realtime supervisor、fallback coordinator、heartbeat、shutdown、archive finalizer 组合行为。
   - 检查 secret / token / private key / message content 日志泄露。
3. 运行本仓库验证：
   - 先运行 daemon crate tests。
   - 再运行 workspace tests。
   - 如果 `im-core` 修改过，运行 Step 04 指定 shared SDK tests。
4. 运行 idle 对比：
   - 使用 Step 01 相同方法采样 60 秒 idle CPU / I/O / mtime / 日志。
   - 记录 active agent 数、session 数、queue pending 数、fallback mode、服务端环境。
   - 输出改前 / 改后对比表，并解释不可比因素。
5. 运行 remote system test：
   - 必须在 `awiki-system-test` 仓库执行：

```bash
cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote AWIKI_BASE_URL=https://awiki.info uv run python manage_local_test_env.py run-tests
```

   - 记录实际命令、通过数量、失败数量、跳过数量、失败或跳过原因、关键环境配置。
6. 文档同步：
   - 更新 `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/local-dev.md` 中 foreground / queue / realtime / fallback 调试说明。
   - 更新 `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` 中 runtime host 消息接收、per-agent WSS 和可靠 sync 边界。
   - 如果 `im-core` public API 或 endpoint 语义变化，更新 `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md`。
   - 检查 `awiki-harness/context/03-cross-repo-architecture.md`、相关 node card 和 repo profile；如架构摘要仍准确，记录无需更新理由。
7. 最终收口：
   - 修复 final Review 发现的小问题。
   - 如 final 阶段修改文件，运行对应 focused tests 并创建 final integration commit。
   - 更新主 Plan 最终全局 Review 与整体验证 section。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/*` | 只允许 final Review 发现的小修；大改回到对应 Step。 | 任何代码改动需补 focused tests。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/*` | 默认不改；只审计 Step 04 批准范围。 | 未批准 public API 变更必须回退或 blocked。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/local-dev.md` | 更新新调度模型、调试和验证说明。 | final docs 目标。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` | 更新 runtime host event-driven message flow。 | final docs 目标。 |
| `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` | 仅当 `im-core` public API / endpoint 语义变化时更新。 | 否则记录无需更新理由。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 如跨 repo 架构摘要变化则更新。 | 否则记录检查结果。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | 如 runtime host 节点描述需要同步则更新。 | 否则记录无需更新理由。 |
| `awiki-harness/context/nodes/message-flow.node.md` | 如 message flow / realtime / sync 边界描述变化则更新。 | 否则记录无需更新理由。 |
| `awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | 如验证入口或 repo profile 变化则更新。 | 否则记录无需更新理由。 |
| `awiki-system-test` | 运行 remote `awiki.info` 系统测试；不默认改代码。 | 若测试 fixture 需修复，先更新 Plan。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填最终 Review、验证结果、最终状态。 | 必须更新。 |

## 6. 依赖与并行约束

- 前置步骤：Step 02、Step 03、Step 04、Step 05、Step 06 done；Step 01 baseline 可用。
- 可并行步骤：无。
- 不可并行步骤：全部实现步骤必须先完成；remote system test 环境串行使用。
- 并行安全依据：不适用，本步骤串行。
- 互斥资源 / 冲突路径：全局 docs、主 Plan 台账、remote `awiki.info` 系统测试环境。
- 外部文档或决策：如果 remote system test 长期失败且非本次改动原因，需要用户确认是否接受 residual risk。
- 环境前提：能访问 sibling repos `awiki-system-test`、`awiki-harness`；能访问 `https://awiki.info`；有 `uv` 和测试依赖。
- 合并前置条件：全局 Review 完成；所有必要验证命令已运行或明确记录不能运行的原因。
- 合并后验证门禁：最终 `git status --short --branch` 清晰，主 Plan 和 Step 07 状态更新。

## 7. 验收标准

- [ ] 主 Plan 执行台账中 Step 01-06 均有状态、commit、Review 证据、验证证据和剩余风险。
- [ ] 全局 Review 已检查 correctness、回归、shared API、security/privacy、docs drift、parallel wave 合并和未提交变更。
- [ ] `awiki-cli-rs2-cpu` daemon crate tests 已运行并记录结果。
- [ ] `awiki-cli-rs2-cpu` workspace tests 已运行并记录结果，或记录不可运行原因。
- [ ] 如修改 `im-core`，已运行 shared SDK gate 或记录用户接受的风险。
- [ ] idle CPU / I/O / mtime / 日志对比使用 Step 01 同一口径，并记录 active agent 数和 session 数。
- [ ] 已在 `awiki-system-test` 使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `AWIKI_BASE_URL=https://awiki.info` 执行完整系统测试，并记录通过 / 失败 / 跳过数量、原因和关键环境配置。
- [ ] 子仓库 docs 和 Harness docs 已更新，或记录检查过且无需更新的理由。
- [ ] 没有未授权 `im-core` public API、message-service 协议、state schema 或 secret handling 变更。
- [ ] Review 发现已经修复或明确记录。
- [ ] 如果 final 阶段修改文件，已创建聚焦 final integration commit；否则记录无需 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Final status | `cd awiki-cli-rs2-cpu && git status --short --branch && git log --oneline -n 8` | final 开始和结束 | step commits 和工作区状态清晰 | Final gate |
| Daemon crate | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | final tests | 通过数量 / 失败原因 | Final gate |
| Workspace | `cd awiki-cli-rs2-cpu && cargo test --workspace --locked` | final tests | 通过数量 / 失败原因 | Final gate |
| Im-core focused | `cd awiki-cli-rs2-cpu && cargo test -p im-core --locked realtime sync` | 如 Step 04-06 改过 `im-core` 或 sync/realtime behavior | 通过数量 / 失败原因 | Shared SDK gate |
| Shared caller regression | `cd awiki-cli-rs2-cpu && cargo test -p awiki-cli --locked && cargo test -p im-core-dart --locked` | 仅当 `im-core` public API / DTO / transport 语义变化 | 通过数量 / 失败原因 | Shared SDK gate |
| Message-service | `cd message-service && cargo test --workspace` | 仅当实际修改 message-service | 通过数量 / 失败原因；未修改时记录跳过原因 | Cross-repo gate |
| Idle final evidence | 复用 Step 01 CPU / I/O / mtime / 日志采样命令，采样 60 秒 | final tests | 改前 / 改后对比表 | Performance evidence |
| Remote system test | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote AWIKI_BASE_URL=https://awiki.info uv run python manage_local_test_env.py run-tests` | final tests | pass / fail / skip 数量、原因、关键环境 | Required system gate |
| Docs check | 检查 daemon docs、`im-core` docs、Harness docs 是否同步；必要时运行 `cd awiki-harness && python scripts/validate-docs.py && python scripts/check-drift.py` | final Review 前 | docs 更新或无需更新理由 | Docs gate |
| Secret/log Review | 人工 diff Review + 运行日志抽样 | final Review | 无 token / private key / JWT / message plaintext 泄露 | Security gate |

如果某个命令不能运行，必须记录实际命令、失败输出摘要、环境原因、影响范围、替代证据和剩余风险。

## 9. Review 环节

- Review 时机：全部实现步骤完成后，final tests 前做一次预 Review；final tests 后做最终 Review。
- Review 重点：跨步骤一致性、foreground 是否真正事件驱动、queue scheduler 与 realtime supervisor 是否互相干扰、fallback 是否低频可靠、`im-core` 兼容性、安全 / 隐私、docs drift、system test 失败项。
- Review 必须逐项检查主 Plan 第 19 节，并把最终结论回填到主 Plan。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | TBD | TBD |
| 已修复问题 | TBD | TBD |
| 剩余风险 | TBD | TBD |
| 新增或缺失测试 | TBD | TBD |
| 已更新或缺失文档 | TBD | TBD |
| 并行安全是否仍成立 | no | final 串行。 |
| Agent 是否越界修改 | TBD | 审计 Wave A / B。 |
| 互斥资源是否被修改 | TBD | 审计 `im-core`、foreground、docs。 |
| 合并风险 | TBD | final 后应清零或记录。 |
| Group gate 影响 | Final gate | 必须记录全部验证结果。 |

## 10. Commit 要求

- Commit 时机：final docs / 小修、验证、Review 都完成后；如果 final 阶段没有修改文件，可以不创建新 commit，但必须记录原因。
- Commit 范围：只包含 final docs、执行台账、必要小修和直接相关 tests。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录 final commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 如果 final commit 修改了代码，必须重新运行对应 focused tests；如果影响全局行为，重新运行必要 final gate。
- 建议消息：`daemon: document event driven runtime verification`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| remote `awiki.info` system test 失败 | system test 输出、失败 case、环境配置 | 重试、检查服务可用性、定位是否本次改动 | 整体计划 | 是 | 是 | 修复本次改动或记录环境 blocker；未获用户接受前不能标记 final pass。 |
| workspace tests 编译失败 | cargo 输出 | 定位失败 crate、回到对应 Step 修复 | 整体计划 | 是 | 是 | 修复并重跑。 |
| idle 指标未下降或 I/O 仍高 | Step 01 / Step 07 对比表 | 检查 identity mtime、queue drain、fallback busy loop、日志 storm | 整体计划 | 是 | 是 | 回到对应 Step 修复或记录未达成目标。 |
| docs drift | Review 发现 docs 与实现不一致 | 更新 daemon docs / Harness docs | final docs | 否 | 是 | 补文档后重新 docs Review。 |
| shared SDK 未授权 diff | `git diff -- crates/im-core` | 回查 Step 04 结论、回归 tests、用户确认 | shared SDK | 是 | 是 | 回退或补兼容性评审。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 07 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：remote system test 环境不稳定可能掩盖真实回归；final 阶段发现大问题会需要回到前面 Step。
- 并行执行风险：final 串行，避免 docs 和台账冲突。
- 合并冲突风险：中；docs 和主 Plan 可能有并行更新残留，必须 coordinator 统一处理。
- Group gate 失败回退：不标记计划完成；按失败来源回到 Step 02-06 修复，或记录用户接受的 blocker。
- Agent 交接说明：最终回复必须说明实际写入 docs、运行命令、通过 / 失败 / 跳过数量、未运行项和剩余风险。
- 回滚 / 回退：如果 final 小修引入回归，回退小修；如果架构改造整体不稳定，可配置降级到低频 fallback 并记录性能风险。
- 后续文档：最终必须同步 daemon docs；Harness docs 如未变更也要记录检查项和无需更新理由。
