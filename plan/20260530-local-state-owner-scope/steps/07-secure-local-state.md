# 步骤 07：Secure 本地状态和公开接口门禁

主计划：[../plan.md](../plan.md)  
步骤编号：07  
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
| 下一步 | 验证并调整 secure local state 的 identity-owned storage、redaction 和 disabled discovery posture。 |

## 2. 目标

- 产出：E2EE local state 保持 client-local 和 owner identity scoped；outbox 使用 identity-owned key；Secure 公开接口 继续 redacted 和 discovery-disabled。
- 用户/系统行为：secure retry/drop/status 不能跨 identities；DID replacement 不会混合 secure state；secure diagnostics 有诊断价值但不暴露 raw crypto 或 private data。
- 非目标：改变 ANP cryptographic protocol、service opaque routing、public discovery policy、group E2EE public support status 或 low-level secure command exposure。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `store/e2ee_outbox.rs` | 使用 strict `(owner_identity_id, outbox_id)`，移除 credential/DID 回退。 | retry/drop 按 scoped owner。 |
| `secure_direct/*` | 确认 direct tables 已要求 owner identity；更新 thread/conversation key 使用。 | 保留 CAS `revision`。 |
| `local_state/schema.rs` | 决定 drop/rename/ignore legacy `e2ee_sessions`。 | Runtime 不能从它读取 truth。 |
| `group_e2ee/*` | 验证 SQLite summaries 和 provider state 按 `owner_identity_id + device_id` scoped。 | 包含 path/provider checks。 |
| `crates/awiki-cli/src/**` secure command/status paths | outbox IDs 改为 owner-scoped 时，确认不会暴露 raw secure internals。 | CLI 仍是 `im-core` 外壳。 |
| DID/service discovery/config defaults | 验证 direct/group E2EE discovery 继续 disabled。 | 无默认 capability advertisement。 |
| 测试 | secure outbox cross-owner isolation、direct CAS、group device scoping、redaction/discovery checks。 | Security 审查门禁。 |

## 4. 依赖

- 前置步骤：步骤 03 和 04。

## 5. 核心设计

`direct_e2ee_sessions`、signed prekeys 和 one-time prekeys 已经使用 identity-owned keys，应保持该模型。主要 storage 变化是 `e2ee_outbox`：`outbox_id` 不再全局唯一，所有内部操作都必须携带 owner scope。

Legacy `e2ee_sessions` 不能作为 active runtime truth source。干净 v17 中优先 drop；如果迁移时保留，则重命名为 `legacy_e2ee_sessions` 并只用于 diagnostics。

Secure surface 规则：

- Direct E2EE public discovery 继续 disabled。不得把 `anp.direct.e2ee.v1` 或 `direct-e2ee` 加入默认 DID/service discovery 输出。
- Group E2EE public discovery 继续 disabled。不得把 `anp.group.e2ee.v1` 或 `group-e2ee` 加入默认 discovery 输出；保持既有 blocked-discovery posture。
- Public secure status/outbox DTOs 只暴露 redacted domain summaries。不得返回 `e2ee_outbox.plaintext`、raw ciphertext、direct session ids、send/receive counters、skipped-key counts、private keys、OPK private material、chain/root/message keys、nonces、KeyPackages、Welcome/Commit/Proposal payloads、raw MLS notices、provider stdout/stderr、provider binary paths、JWT 或 raw SQLite rows。
- CLI secure commands 继续是高层 `im-core` wrapper。不得把 low-level group E2EE orchestration 或 diagnostics 暴露为默认产品命令。
- Group MLS provider state 包括 path derivation、lock/state database selection，都必须按 `owner_identity_id + device_id` scoped。

## 6. 实施指南

1. 将 `e2ee_outbox.rs` 中的 `owner_scope_predicate` 回退替换为 strict `owner_identity_id = ?`。
2. 更新 queue/retry/drop/mark-sent/failure tests，使其覆盖 `(owner_identity_id, outbox_id)`。
3. 更新 secure direct outbox message projection，使用 stable conversation id。
4. 运行 direct session CAS tests，并调整测试中的 owner DID predicates。
5. 审计 group E2EE local summary writes 和 provider/device id flows。
6. 审计 public `SecureOutboxEntry`、direct/group secure status DTOs、CLI output、Dart mappings 和 diagnostics 的 redaction。
7. 检查 DID/service discovery generation 和 feature defaults，证明本次 storage refactor 没有公开 advertise direct/group E2EE。
8. 将任何有意保留的 legacy secure tables 标记为 migration-only。

## 7. 验收标准

- [ ] Outbox primary key 是 `(owner_identity_id, outbox_id)`。
- [ ] 一个 identity 的 outbox retry/drop 不能影响另一个 identity 下相同 outbox id。
- [ ] Direct E2EE CAS revision tests 仍通过。
- [ ] Group E2EE state 在适用处按 owner identity 和 device id scoped。
- [ ] Legacy `e2ee_sessions` 不是 active runtime truth source。
- [ ] Public secure outbox/status APIs 不暴露 plaintext outbox payloads、private keys、ratchet/MLS internals、raw secure payloads、provider stdout/stderr、provider paths、JWT 或 raw SQLite rows。
- [ ] Direct E2EE 和 Group E2EE public discovery 在默认 DID/service discovery 和 config 中继续 disabled。
- [ ] Hidden/internal low-level group E2EE commands 继续 hidden/internal 或 stable unsupported。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked e2ee_outbox` | Scoped outbox tests 通过。 |
| 单元 | `cargo test -p im-core --locked direct_secure` | Direct secure tests 通过。 |
| 单元 | `cargo test -p im-core --features group-e2ee --locked group_e2ee` | Group E2EE focused tests 可用时通过。 |
| CLI contract | `cargo test -p awiki-cli --locked msg_secure group_secure e2ee` | Secure command/status tests 通过；如 filter 重命名或不可用，记录替代命令。 |
| Redaction search | `rg "plaintext|private_key|jwt|chain_key|root_key|message_key|skipped_key|send_n|recv_n|KeyPackage|Welcome|Commit|Proposal|provider.*stdout|provider.*stderr|provider.*path" crates/im-core/src crates/awiki-cli/src crates/im-core-dart packages/awiki_im_core` | Public output/DTO 命中被移除或记录为 internal/test-only。 |
| Discovery search | `rg "anp\\.direct\\.e2ee\\.v1|direct-e2ee|anp\\.group\\.e2ee\\.v1|group-e2ee" crates docs config.template.yaml` | 没有新的默认 public advertisement；docs/internal/test 命中是有意的。 |
| 搜索 | `rg "credential_name.*owner.*fallback|owner_scope_predicate|e2ee_sessions" crates/im-core/src/internal` | 没有 active 回退；legacy references 有解释。 |

## 9. 审查流程

- Security/privacy 审查：确认 private keys、chain keys、MLS material、raw secure payloads、provider output、provider paths、JWT、backup contents 或 plaintext outbox payloads 没有被 log 或暴露。
- Discovery 审查：证明 direct/group E2EE public discovery 继续 disabled，且没有 feature default 改动。
- API 审查：确认 public Rust/Dart/CLI secure outputs 仍是高层 redacted。
- Regression 审查：secure retry 和 listener paths 仍然 scoped 到当前 client。

## 10. 提交要求

- 建议提交信息：`im-core: scope secure local state by owner identity`

## 11. 风险、回滚和后续

- 风险：低层 secure tests 可能依赖 global outbox ids。
- 回滚/回退：给 helpers 增加显式 owner scope；不要恢复 global outbox ownership，也不要扩大 public secure/discovery surfaces。
- 后续文档：只有 redacted public status contract 变化时才更新 `docs/sdk-refactor/modules/10-secure.md`、`direct-e2ee-operations.md` 或 `group-e2ee-operations.md`。
