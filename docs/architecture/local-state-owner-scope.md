# 本地状态 owner scope 设计

## 状态

- 本文描述当前 Rust `awiki-cli` / `im-core` 本地 SQLite owner scope 的权威模型。
- 当前 SQLite schema version：`17`。
- 当前 workspace schema version：`4`。
- 本地业务状态已经收敛到 `owner_identity_id` 分区；`owner_did` 只保留为当前 DID snapshot。

## 核心原则

`owner_identity_id` 是本地状态的 owner partition key。它对应 identity registry 中稳定的 identity id，不随 DID recover 或 replace 改变。

`owner_did` 是展示、调试和 wire 兼容用的当前 DID snapshot。DID recover 或 replace 只能刷新 snapshot 和写入 `identity_did_history`，不能把业务行从一个 owner 迁移到另一个 owner。

活跃运行时不能用 credential name、owner DID、路径或 alias 猜测 owner。只有 migration/import 兼容路径可以读取旧 owner DID，并且必须在模块命名、注释和执行计划中明确为 legacy-only。

## SQLite key 形态

活跃表使用 identity-owned 主键：

| 表 | 主键 |
|---|---|
| `contacts` | `(owner_identity_id, did)` |
| `contact_handle_bindings` | `(owner_identity_id, handle, did)` |
| `messages` | `(owner_identity_id, msg_id)` |
| `groups` | `(owner_identity_id, group_id)` |
| `group_members` | `(owner_identity_id, group_id, user_id)` |
| `relationship_events` | `(owner_identity_id, event_id)` |
| `e2ee_outbox` | `(owner_identity_id, outbox_id)` |
| `identity_did_history` | `(owner_identity_id, did)` |
| `direct_e2ee_sessions` | `(owner_identity_id, peer_did)` |
| `direct_e2ee_signed_prekeys` | `(owner_identity_id, key_id)` |
| `direct_e2ee_one_time_prekeys` | `(owner_identity_id, key_id)` |

`conversation_id` 是稳定会话键。私聊会话不能把本地 owner DID 编进 `conversation_id`，否则 DID replace 后会造成会话分裂。

## DID history

`identity_did_history` 记录每个 owner identity 的 DID 变化：

- 每个 `owner_identity_id` 只能有一个 `status = 'current'`。
- 同一个 current DID 不能同时属于两个 live identity。
- 旧 DID 保留为 `previous`，用于 legacy rebuild/import 时解析所有权。

## Recover / replace 行为

DID recover 或 replace 的本地状态行为是：

1. 记录 DID history transition。
2. 刷新同一 `owner_identity_id` 下业务表的 `owner_did` snapshot。
3. 保持 `owner_identity_id` 和业务主键不变。
4. 不执行业务行 owner rebind。
5. 不 reset、merge 或泄露 E2EE private state。

Replace-DID dry-run 的 `store_rebind_counts` 和 `e2ee_cleanup_counts` 保持为兼容字段；在 identity-owned schema 中它们不再通过 `owner_did` 扫描业务表。

## Migration / import 边界

SQLite 不能原地修改主键，因此旧 schema 进入 v17 必须通过重建表或重建数据库完成。

当前 workspace `3 -> 4` 策略：

- 已经是 SQLite schema 17：执行 owner invariant 检查。
- 旧 SQLite schema：先创建 workspace backup，再删除旧 DB 文件集并创建干净 schema 17 DB。
- 高于当前支持 schema：fail closed。
- legacy import 只能使用可解析到 `owner_identity_id` 的 owner；显式未知 `owner_did` 或 `credential_name` 必须 fail closed。

## Diagnostics

`awiki-cli doctor` 的 SQLite check 可以输出：

- schema version；
- owner invariant violation 的 table / invariant / row_count；
- legacy secure table 是否存在和行数；
- contact handle binding 计数。

Diagnostics 不能输出 raw SQLite rows、message plaintext、`e2ee_outbox.plaintext`、private keys、JWT、raw ciphertext、ratchet/MLS state、KeyPackage、Welcome、Commit、Proposal、provider stdout/stderr/path 或 backup contents。

## Secure posture

Direct E2EE 和 Group E2EE 的 public discovery 继续 disabled。默认 DID/service discovery 不能 advertise `anp.direct.e2ee.v1`、`direct-e2ee`、`anp.group.e2ee.v1` 或 `group-e2ee`。

Group MLS provider state/path 必须继续按 `owner_identity_id + device_id` scoped。CLI、Dart DTO、doctor、日志和文档只能暴露 high-level secure status / repair summary，不能暴露 raw cryptographic artifacts。
