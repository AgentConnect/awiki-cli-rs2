# 步骤 10：集成门禁和完整系统测试

主计划：[../plan.md](../plan.md)  
步骤编号：10  
状态：草案

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | |
| 完成时间 | |
| 提交 | |
| 审查证据 | |
| 验证证据 | |
| 下一步 | 运行完整验证和 remote `awiki.info` 系统测试，修复集成问题，并记录最终证据。 |

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

- [ ] `cargo test --workspace --locked` 通过，或无关失败已用证据说明并修复/隔离。
- [ ] `bash scripts/sdk-refactor/final-cutover-check.sh` 通过。
- [ ] `scripts/flutter/codegen-check.sh` 通过。
- [ ] Active runtime 中没有 forbidden owner 回退/search patterns。
- [ ] Direct E2EE 和 Group E2EE public discovery 默认继续 disabled。
- [ ] Public Secure CLI/Rust/Dart/diagnostic/docs output 保持 redacted。
- [ ] Low-level group E2EE operations 继续 hidden/internal 或 stable unsupported。
- [ ] Workspace upgrade backup/manifest/log behavior 已审查，确认不会泄露敏感材料。
- [ ] `../awiki-system-test` 下 remote 模式、`awiki.info` 域名完整系统测试已执行并通过。
- [ ] 系统测试报告符合 `../awiki-system-test/AGENTS.md`：包含总体结果、失败/跳过详情、功能域统计和配置上下文。
- [ ] 最终审查 没有未解决 correctness/security findings。
- [ ] Final docs 描述剩余风险，如果有。

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
