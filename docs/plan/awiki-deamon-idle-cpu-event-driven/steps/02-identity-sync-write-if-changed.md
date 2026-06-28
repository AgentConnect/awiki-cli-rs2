# Step 02：内容感知 identity sync

主 Plan：[../plan.md](../plan.md)  
Step index：02  
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
| Next action | 将 `sync_agent_identity_to_im_core` 改为内容感知写入，避免静默状态反复重写相同 identity 文件。 |
| Assigned agent | agent-storage |
| Parallel group | A |
| Parallel safe | yes |
| Parallel with | Step 03；Step 04 默认只读时也可并行 |
| Conflict resources | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs`、identity 文件权限和 token 写入语义 |
| Baseline commit | TBD，必须来自 Step 01 完成后的 commit |
| Worktree / branch | TBD |
| Merge gate | Step 01 done；合并前确认未修改 foreground 主循环和 queue scheduler。 |
| Verification gate | focused identity tests + `cargo test -p awiki-deamon --locked`。 |
| Gate status | pending |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：当 agent DID、private key、E2EE key、token、registry/default 内容没有变化时，`client_for_agent_identity` 不再导致 `im-core` identity 相关文件 mtime 和 WAL 持续更新。
- 用户 / 系统可见行为：daemon 创建 `ImClient`、发送消息、接收消息、runtime DID auth、controller DID 映射保持不变；首次同步和内容变化时仍能正确写入。
- 非目标：不改变 DID / key / token 格式，不改变 `im-core` identity public API，不改变 state root 目录布局，不引入新的 secret 存储。
- 完成标准：重复调用 identity sync 时，相同内容不写文件；内容变化会写入；文件权限、registry/default、auth 兼容现有行为；测试覆盖 mtime / 内容判断；后续 Step 可安全复用该优化。

## 3. 设计方法

- 设计边界：优化 daemon adapter 内部写入策略，不把 daemon 的身份管理语义推给 `im-core` 或 message-service。
- 核心决策：优先实现 `write_if_changed` / `write_json_if_changed` 风格 helper；比较目标文件现有内容与待写内容，完全一致时跳过写入。
- 契约 / API / 数据流：`client_for_agent_identity` 的调用方签名、返回 `ImClient` 行为和错误类型应保持兼容；如果 helper 需要新增，保持 crate-private。
- 兼容性：首次没有文件、内容不同、权限需要修正、目录不存在、旧格式迁移时仍必须写入；不能因为跳过写入导致 token 过期、DID 切换或 E2EE key 变化不生效。
- 迁移策略：无 schema 迁移；对已有文件采用读现状、比较、必要时覆盖。
- 风险控制：不要把 private key、token、E2EE key 打到日志；测试可使用临时 fixture 和假 key material。

## 4. 实现方法

1. 梳理 `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs`：
   - 找到 `client_for_agent_identity`、`sync_agent_identity_to_im_core` 和所有文件写入点。
   - 标记哪些写入是 identity 文件、registry、default、auth/token 或目录初始化。
2. 新增内容感知写入 helper：
   - 对普通文本 / JSON / bytes 文件，读取现有内容并与待写内容比较。
   - 内容相同且权限符合要求时跳过写入。
   - 文件不存在、读取失败、内容不同、权限不符合要求时写入或修正权限。
   - 错误上下文保留现有风格，不能吞掉真实 I/O 失败。
3. 保持权限和原子性：
   - DID private key、token 等敏感文件必须保留现有权限设置。
   - 如现有实现使用临时文件 + rename，继续沿用；如没有，也不要扩大本步骤为全量存储重写。
4. 增加测试：
   - 首次 sync 会创建文件。
   - 第二次相同内容 sync 不改变 mtime 或 helper 返回 `changed=false`。
   - token / private key / registry / default 变化会触发写入。
   - 目录缺失和权限修正路径有覆盖。
5. 更新执行台账和 Step 证据：
   - 记录 changed / unchanged 测试结果。
   - 对比 Step 01 的 identity mtime 高频变化，说明本步骤是否降低静默写入。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs` | 修改 `sync_agent_identity_to_im_core` 写入策略，新增 crate-private helper 和 focused tests。 | 本步骤主写入范围。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/config.rs` | 默认不修改；只有测试 fixture 需要时可读取现有 config 结构。 | 不改变 transport policy。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/*` | 禁止修改，除非 Step 04 先批准共享接口变更。 | 本步骤目标是在 daemon adapter 内完成。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填 Step 02 状态、验证证据、commit。 | Coordinator 合并时更新。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/steps/02-identity-sync-write-if-changed.md` | 回填本 Step 状态、证据、commit。 | 本规划文档本身。 |

## 6. 依赖与并行约束

- 前置步骤：Step 01 done。
- 可并行步骤：Step 03；Step 04 默认只读或仅做兼容性调查时也可并行。
- 不可并行步骤：Step 05 依赖本步骤完成，因为 realtime task 需要长期复用 `ImClient` 并避免 session 生命周期内重复写 identity。
- 并行安全依据：本步骤只修改 identity sync adapter，不改 foreground 主循环、queue scheduler 或 realtime supervisor。
- 互斥资源 / 冲突路径：`im_core_adapter.rs`；若 Step 04 需要改 `im-core` identity 或 config public API，必须暂停并更新主 Plan。
- 外部文档或决策：不需要用户确认；如果发现必须修改 shared `im-core` API 才能实现，立即 blocked，并转 Step 04 兼容性评审。
- 环境前提：能够运行 `awiki-deamon` crate tests；测试需临时目录支持 mtime 或 helper `changed` 结果。
- 合并前置条件：所有 focused identity tests 通过；确认未越界修改 parallel group 外路径。
- 合并后验证门禁：Wave A 合并时由 coordinator 运行 `cargo test -p awiki-deamon --locked`。

## 7. 验收标准

- [ ] 相同 identity/token 内容重复 sync 不重写目标文件，mtime 或 helper changed 证据稳定。
- [ ] DID、private key、E2EE key、token、registry/default 任一内容变化会触发正确写入。
- [ ] 首次同步、目录缺失、文件缺失、权限修正场景仍正常。
- [ ] 不记录 private key、token、JWT、E2EE key 等敏感值。
- [ ] 未修改 `im-core` public API / DTO / feature gate / transport 默认语义。
- [ ] 如果本步骤标记为 parallel-safe，已确认没有修改互斥资源或超出授权路径。
- [ ] 如果本步骤属于并行组，已记录 Agent、基线 commit、分支 / worktree 和合并门禁状态。
- [ ] 本步骤合并前的 Step gate 已通过，或已记录不能运行的具体原因和风险。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入 Step 05 之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Focused identity tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked im_core_adapter` | commit 前 | 首次写入、重复不写、内容变化写入 tests 通过 | Step gate |
| Daemon unit | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | commit 前 | crate tests 通过或记录原因 | Step gate |
| mtime 手动证据 | 在临时 state root 连续调用相同 sync，比较 identity / registry / default / auth 文件 mtime | Review 前 | 相同内容 mtime 不变；变化内容 mtime 改变 | Step evidence |
| Sensitive logging check | `cd awiki-cli-rs2-cpu && git diff -- crates/awiki-deamon/src/im_core_adapter.rs` 并人工 Review 日志 | Review 前 | diff 中无 secret 打印 | Review gate |
| Parallel scope check | `cd awiki-cli-rs2-cpu && git diff --name-only` | commit 前 | 只包含授权路径和 tests / docs 台账 | Group gate |
| Group Verification | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | Wave A 合并后 | Step 02 + Step 03 组合后通过 | Group gate |

如果 mtime 精度受文件系统影响，测试可以优先断言 helper `changed=false`、文件内容哈希不变和无写入路径被调用；必须在证据中说明。

## 9. Review 环节

- Review 时机：本步骤实现完成后、commit 前；并行合并时由 coordinator 再做一次 group Review。
- Review 重点：内容比较是否可靠、权限是否保持、错误是否可见、首次同步是否正常、敏感信息是否泄露、是否误改共享 SDK 接口。
- Review 必须抽查 `client_for_agent_identity` 的调用路径，确认不会因为跳过写入导致 `ImClient` 拿到旧 token 或旧 DID。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | TBD | TBD |
| 已修复问题 | TBD | TBD |
| 剩余风险 | TBD | TBD |
| 新增或缺失测试 | TBD | TBD |
| 已更新或缺失文档 | TBD | 通常无需长期文档；final 记录即可。 |
| 并行安全是否仍成立 | TBD | 不应修改 foreground / queue / realtime 路径。 |
| Agent 是否越界修改 | TBD | TBD |
| 互斥资源是否被修改 | TBD | `im_core_adapter.rs` 为本步骤授权路径。 |
| 合并风险 | TBD | 与 Step 03 理论低冲突。 |
| Group gate 影响 | Wave A | 合并后跑 daemon tests。 |

## 10. Commit 要求

- Commit 时机：focused tests、daemon tests、Review 都完成后。
- Commit 范围：只包含 identity sync 写入优化、直接相关 tests、执行台账回填。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 并行步骤的 commit 必须基于 Step 01 的基线 commit 或说明 rebase / merge 过程。
- Commit 后必须记录是否 `ready_for_group_merge`。
- 如果 commit 修改了原计划未授权路径，必须先更新主 Plan 的 parallel-safe 判定和变更记录。
- 建议消息：`daemon: avoid unchanged identity writes`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| 现有写入路径无法可靠比较内容 | helper tests 无法区分 unchanged / changed | 增加内部 changed 返回值、使用临时文件 mock、哈希比较 | 当前步骤 | 否 | 是 | 记录 blocker；不要用不可靠 mtime 作为唯一依据。 |
| 必须修改 `im-core` identity API 才能避免写入 | daemon adapter 无法覆盖的证据 | 尝试 crate-private helper、client cache、启动时 sync | 共享 SDK 契约 | 是 | 是 | 暂停并转 Step 04；等待兼容性评审和用户确认。 |
| 权限修正与跳过写入冲突 | 测试发现权限不符合时未修正 | 拆分内容比较与权限检查 | 当前步骤 | 否 | 是 | 修复后重新跑 focused tests。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 02 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：错误跳过写入会造成 DID/token/key 更新不生效；过度日志会泄露敏感信息。
- 并行执行风险：如果 Step 03 越界修改 `client_for_agent_identity` 调用点，会影响本步骤 Review，需暂停 Wave A 合并。
- 合并冲突风险：低；主要集中在 `im_core_adapter.rs`。
- Group gate 失败回退：回退内容感知 helper，恢复无条件写入；保留测试说明当前风险。
- Agent 交接说明：Step 05 复用 `ImClient` 时仍应减少 `client_for_agent_identity` 调用次数，不要把本步骤当作允许高频创建 client 的理由。
- 回滚 / 回退：可回退为启动时强制 sync + 运行期内容感知 sync；如果出现 auth 问题，优先保守恢复写入。
- 后续文档：如果只是内部优化，最终在 Step 07 记录无需长期 docs；如果暴露新诊断或配置，更新 daemon docs。
