# 步骤 09：回退移除、Diagnostics、文档和 Dart

主计划：[../plan.md](../plan.md)  
步骤编号：09  
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
| 下一步 | 移除残留 runtime 回退，并更新 diagnostics、文档和 generated bindings。 |

## 2. 目标

- 产出：仓库文档、diagnostics 和 generated SDK bindings 与最终 owner model 一致。
- 用户/系统行为：doctor/diagnostics 能解释 local-state owner issues；App/Dart facade 保持一致。
- 非目标：除 generated package contracts 必须要求外，不做宽泛 UI/App 变化；不启用 公开 Secure 发现；不暴露 low-level secure API。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| Active runtime code | 移除剩余 owner DID/credential 回退。 | Migration-only 回退可以保留在 legacy modules 中并加注释。 |
| `crates/awiki-cli/src/diagnostics/*` | 增加 v17 invariants 的 doctor checks。 | 不泄露 private material。 |
| `docs/architecture/*` | 新增 owner-scope 文档并更新 upgrade 文档。 | 子仓库文档是权威。 |
| `docs/sdk-refactor/*` | 更新 local-state module/boundary notes。 | CLI/App 不使用 raw SQLite。 |
| `docs/sdk-refactor/modules/10-secure.md` | 仅在 secure owner-scope/outbox status wording 变化时更新。 | 保持 redacted public API posture。 |
| `docs/architecture/direct-e2ee-operations.md` | 仅在 secure direct local-state wording 变化时更新。 | 保持 disabled discovery posture。 |
| `docs/architecture/group-e2ee-operations.md` | 仅在 group owner/device scope wording 变化时更新。 | 保持 hidden/internal low-level command posture。 |
| `crates/im-core-dart`, `packages/awiki_im_core` | 如果 message/conversation shape 变化，重新生成/更新 DTO。 | 提交 generated files。 |
| Harness docs | 仅当 routing summary 变化时更新。 | 避免重复子仓库文档。 |

## 4. 依赖

- 前置步骤：步骤 01-08。

## 5. 核心设计

v17 和 migration 激活后，runtime 回退 patterns 就是技术债。Legacy owner-DID 回退只允许保留在 migration/import modules 中，并通过名称/注释明确隔离。Diagnostics 应报告：

- empty owner identity rows；
- active table key shape；
- duplicate identity-owned natural keys；
- duplicate live DID；
- direct conversation ids containing owner DID；
- legacy secure tables being used unexpectedly。

Diagnostics 和 generated DTOs 必须保持脱敏：

- Doctor/diagnostics 可以报告 table names、counts、schema versions、owner identity ids、DID history status、invariant names 和 repair hints。
- Doctor/diagnostics 不得打印 private PEMs、JWTs、local message plaintext、`e2ee_outbox.plaintext`、raw ciphertext、direct session ids/counters、skipped-key counts、chain/root/message keys、OPK private material、KeyPackages、Welcome/Commit/Proposal payloads、raw MLS notices、provider stdout/stderr、provider binary paths 或 raw SQLite rows。
- Dart/Rust public DTO 必须遵守 `docs/sdk-refactor/modules/10-secure.md` 中的 secure module contract：只暴露 high-level secure state 和 problems，不暴露 raw cryptographic artifacts。
- 文档必须说明 Direct E2EE 和 Group E2EE discovery 继续 disabled，除非另有单独安全评审通过的 enablement plan。

## 6. 实施指南

1. 运行 targeted `rg` 搜索回退 patterns，将每个命中分类为 runtime、test、migration 或 doc。
2. 移除 runtime 回退，或替换为 `OwnerScope`。
3. 增加 diagnostics helpers 和 CLI output fields。
4. 新增 `docs/architecture/local-state-owner-scope.md`。
5. 将 `docs/architecture/local-state-upgrade.md` 从旧版本表述更新到当前 workspace version。
6. 更新 SDK refactor local-state docs。
7. 只有实现改变 owner-scope wording 或 public redacted status fields 时，才更新 secure docs；不要复制 raw implementation details。
8. 增加 diagnostics tests，使用 sentinel secret/plaintext values，并断言输出中不存在这些值。
9. 如果 Rust FFI DTO 变化，运行 `scripts/flutter/codegen.sh`，然后运行 codegen check。

## 7. 验收标准

- [ ] Runtime code 没有 owner-DID 或 credential-name owner 回退。
- [ ] Migration/import-only 回退清晰隔离。
- [ ] Doctor/diagnostics 覆盖 owner-scope invariants。
- [ ] Doctor/diagnostics 已脱敏，并用 sentinel private/secure/plaintext values 测试。
- [ ] 文档说明 `owner_identity_id` 是本地 owner partition key。
- [ ] 文档保持 Direct E2EE 和 Group E2EE public discovery disabled posture。
- [ ] 文档和 generated DTO 不把 raw secure artifacts 或 low-level group E2EE operations 暴露为默认 public API。
- [ ] 如果 DTO 变化，generated Dart/Rust bridge files 是最新的。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 搜索 | `rg "owner_identity_id = \\? OR|credential_name.*owner|WHERE owner_did|ON CONFLICT\\(owner_did|ON CONFLICT\\(event_id\\)" crates/im-core/src crates/awiki-cli/src` | 只有 migration/import/test/docs 命中且均为有意保留。 |
| Redaction search | `rg "private_key|jwt_token|plaintext|chain_key|root_key|message_key|skipped_key|send_n|recv_n|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|provider.*path" crates/awiki-cli/src/diagnostics crates/im-core-dart packages/awiki_im_core docs` | Public docs/diagnostics/DTO 命中已脱敏或明确 internal/unsupported。 |
| Discovery search | `rg "anp\\.direct\\.e2ee\\.v1|direct-e2ee|anp\\.group\\.e2ee\\.v1|group-e2ee" docs crates config.template.yaml` | 命中保持 disabled discovery 或 internal/test-only posture。 |
| 文档 | 检查新文档链接 | 路径存在，链接可解析。 |
| Dart | `scripts/flutter/codegen-check.sh` | Generated bindings 是最新的。 |
| 单元 | `cargo test -p awiki-cli --locked diagnostics` | 如存在 diagnostics tests，则通过。 |

## 9. 审查流程

- 审查 文档权威边界：子仓库文档负责实现 truth；Harness 只做路由。
- 审查 diagnostics，确认没有 secret/private E2EE data exposure。
- 按 `docs/sdk-refactor/modules/10-secure.md` 的 redaction rules 审查 generated Dart/Rust public DTO。
- 审查 文档不暗示 public direct/group E2EE discovery enablement。

## 10. 提交要求

- 建议提交信息：`docs: document identity-owned local state`

## 11. 风险、回滚和后续

- 风险：generated Dart diff 可能较大。
- 回滚/回退：如果 public model churn 过高，通过 aliases 保持 DTO 兼容；回滚任何 raw secure DTO/diagnostic exposure。
- 后续文档：如发现 secure posture gap，继续实现前先更新本计划。
