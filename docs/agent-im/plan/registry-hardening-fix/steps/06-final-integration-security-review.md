# Step 06：最终集成验证、安全 Review 与文档收口

主 Plan：[../plan.md](../plan.md)  
Step index：06  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | 相关仓当前分支 |
| Started | - |
| Completed | - |
| Commit | - |
| Review evidence | - |
| Verification evidence | - |
| Next action | 等 Step 01-05 完成后执行全局 Review 和整体验证 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：确认 Step 01-05 的跨仓修复整体符合核心设计预期，并有足够 L3 身份/auth/security 验证证据。
- 用户 / 系统可见行为：新注册、恢复/旧身份迁移、bootstrap、delegated inbox/send、APP action capability、daemon key revoke/registry sync 关键链路可被验证；剩余风险有明确记录。
- 非目标：不新增功能；不把未完成的后续安全债包装为已完成；不在没有证据时宣布 remote E2E 通过。
- 完成标准：全局 Review 完成；各仓测试和 remote `awiki.info` 系统测试记录实际命令、通过/失败/跳过数量；文档和执行台账更新；工作区状态清楚。

## 3. 设计方法

- 设计边界：Step 06 是 gate，不是大规模实现步骤。只有发现跨步骤问题时才修改代码/文档，并记录最终集成 commit。
- 核心决策：按 L3 验证执行：identity/auth/DID/key material/security-sensitive 变更必须有 security review、兼容性证据、E2E 或替代证据。
- 契约 / API / 数据流：检查 user-service registry、DID Document authentication、im-core package/migration、daemon bootstrap validation、awiki-me bootstrap、message-service delegated policy、ANP SDK optional API 是否一致。
- 兼容性：旧 DID auth register、旧 im-core send/inbox/history、旧 bootstrap v1 legacy decode、旧 App action payload tests 不回归。
- 迁移策略：确认 backfill/migration 文档和 error handling 清楚；不能自动迁移的旧身份有用户可理解的恢复路径。
- 风险控制：所有 secret leakage 搜索必须通过；E2EE 明文仍不进入 Agent；message-service 不依赖 registry；registry-only revoke 不被文档误写为运行时撤销。

## 4. 实现方法

1. 读取主 Plan 执行台账，确认 Step 01-05 都是 `done`，并且每步有 commit、Review evidence、Verification evidence。
2. 执行全局代码/文档 Review：
   - registry 与 DID Document authentication 是否一致；
   - proof 后置 patch 是否已移除或仅 legacy；
   - recovery/migration 是否覆盖旧身份；
   - bootstrap 是否早期校验 private/public/DID Document；
   - key package v2 是否新写出、v1 是否 legacy read；
   - APP action 是否显式 capability；
   - ANP SDK optional 参数是否 generic；
   - message-service 是否仍只按 DID Document 授权。
3. 执行命名和安全搜索：
   - `daemon-key-*` 设备化示例；
   - `mailbox_*` 新增命名；
   - `private_key_multibase` 新写路径；
   - private key / jwt / bearer / runtime token 泄露；
   - E2EE plaintext 转发给 Agent。
4. 运行受影响仓库验证命令，记录实际结果。
5. 运行 remote system-test：
   - 必须使用 `AWIKI_SYSTEM_TEST_MODE=remote`；
   - 目标域名使用 `awiki.info`；
   - 记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。
6. 如发现问题：
   - 小问题直接修复并记录最终集成 commit；
   - 安全/契约问题回到对应 Step 或更新 Plan；
   - remote 环境不可用时记录 blocker，不把 Step 06 标为 done，除非用户明确接受替代证据。
7. 更新主 Plan 第 7、15、17 节和本 Step 执行状态。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/plan.md` | 回填执行台账、最终 Review、验证证据 | 必改 |
| `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/steps/*.md` | 回填 Step 状态、证据和风险 | 必改 |
| `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` | 如实现状态或契约变化，更新 | 视 Review 发现 |
| `awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` | 如 key package / SDK / registry 语义变化，更新 | 视 Review 发现 |
| `awiki-system-test` | 运行 remote 系统测试；必要时补测试 | 修改时需 commit |
| 各受影响仓库 | 仅修复 Review 发现的问题 | 聚焦最终集成 commit |

## 6. 依赖

- 前置步骤：Step 01-05 都必须完成并提交。
- 外部文档或决策：`awiki-cli-rs2/AGENTS.md` 要求最终系统测试使用 remote `awiki.info`。
- 环境前提：remote `awiki.info` 服务可用；本地能运行各仓测试命令。

## 7. 验收标准

- [ ] Step 01-05 都是 `done`，每步有 commit、Review evidence 和 Verification evidence。
- [ ] 全局 Review 无未解决 P0/P1 问题；剩余风险已记录并分类。
- [ ] user-service、awiki-cli-rs2、awiki-me、anp、message-service 关键验证命令已运行或明确记录不能运行原因。
- [ ] remote `awiki.info` 系统测试已运行并记录实际命令、通过/失败/跳过数量。
- [ ] secret leakage、naming、legacy schema、E2EE boundary 搜索已完成。
- [ ] 主 Plan 和 Step 文档执行台账已回填。
- [ ] 最终 `git status --short --branch` 已记录。
- [ ] 如本步骤修改文件，已完成 Review、验证和最终集成 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| user-service | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v` | DID registry/proof/revoke/update tests 通过。 |
| awiki-cli-rs2 | `cd awiki-cli-rs2 && cargo test -p im-core --locked && cargo test -p im-core-dart --locked && cargo test -p awiki-deamon --locked -j1` | identity/migration/daemon tests 通过。 |
| awiki-cli-rs2 Flutter | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`、`cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | codegen/package tests 通过。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | App tests 通过。 |
| ANP | `cd anp/anp && uv run pytest anp/unittest/authentication anp/unittest/proof -v`、`cd anp/anp/rust && cargo test --locked` | SDK tests 通过。 |
| message-service | `cd message-service && cargo test --workspace` | delegated policy/fanout 不回归。 |
| remote system-test | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote <实际命令，目标 awiki.info>` | 记录实际命令、通过/失败/跳过数量和配置。 |
| security search | `rg -n "BEGIN PRIVATE|private_key|privateKey|bearer |jwt|rtok_|e2ee_plaintext|mailbox_|daemon-key-<|daemon-key-macbook" awiki-cli-rs2 awiki-me user-service message-service anp/anp` | 无未解释泄露或旧命名；合法 fixture/legacy 有说明。 |
| docs path | `find awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix -type f -maxdepth 3 | sort` | 文档存在；链接路径正确。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：所有验证完成后、最终提交前。
- Review 重点：跨仓契约一致性、未提交变更、文档漂移、系统测试证据、security gate、剩余风险是否可接受。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待回填 | - |
| 已修复问题 | 待回填 | - |
| 剩余风险 | 待回填 | - |
| 新增或缺失测试 | 待回填 | - |
| 已更新或缺失文档 | 待回填 | - |

## 10. Commit 要求

- Commit 时机：本步骤修改代码/测试/文档且验证、Review 完成后。
- Commit 范围：最终集成修复、文档回填或系统测试补充；如果只运行验证不改文件，则不需要 commit。
- Commit 前状态：记录所有受影响仓 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`agent-im: finalize delegated key hardening`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| remote `awiki.info` 不可用 | 待回填 | 健康检查 / 重试 / focused local tests | 整体验证 | 记录 blocker，不默认 done |
| 上游 ANP SDK 变更未完成 | 待回填 | legacy fallback | 整体计划 | 返回 Step 05 |
| P0/P1 安全 Review 发现 | 待回填 | 修复或回退 | 发布 gate | 不得标记完成 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 06 | 定义最终 L3 验证和安全 Review gate | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：remote 系统测试环境不稳定导致验证受阻。
- 回滚 / 回退：如果只影响文档，回滚文档 commit；如果发现跨仓契约问题，回到对应 Step 修复后重新执行 Step 06。
- 后续文档：最终将已实现行为同步到核心设计文档、delegated proof plan、受影响仓 docs/API 和系统测试说明。
