# 本地状态 owner scope 设计

## 状态

- 本文描述当前 Rust `awiki-cli` / `im-core` 本地 SQLite owner scope 的权威模型。
- 当前 SQLite schema version：`21`。
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
| `conversation_summaries` | `(owner_identity_id, conversation_id)` |
| `sync_state` | `(owner_identity_id, scope, checkpoint_kind)` |
| `thread_read_state` | `(owner_identity_id, thread_kind, thread_id)` |

`conversation_id` 是稳定会话键。私聊会话不能把本地 owner DID 编进 `conversation_id`，否则 DID replace 后会造成会话分裂。

当前消息显示链路把 `conversation_id` 作为跨 list、timeline、read ack、send local echo 和 repair 的 canonical key。`ThreadRef`、legacy direct DID、old Flutter sorted direct、peer-scope storage thread 和 handle/DID rotation alias 只能在 `im-core` resolver / migration 中收敛，不能由 AWiki Me presentation 层重新拼接作为 correctness key。

## DID history

`identity_did_history` 记录每个 owner identity 的 DID 变化：

- 每个 `owner_identity_id` 只能有一个 `status = 'current'`。
- 同一个 current DID 不能同时属于两个 live identity。
- 旧 DID 保留为 `previous`，用于 legacy rebuild/import 时解析所有权。

## Recover / replace 行为

DID recover 或 replace 的本地状态行为是：

1. 同一完整 Handle 的 recover 保留原有稳定 `owner_identity_id`；CLI 与 Dart/App
   都必须进入同一个 `local-finalize` 路径。
2. 记录 DID history transition。
3. 刷新同一 `owner_identity_id` 下业务表的 `owner_did` snapshot。
4. 保持 `owner_identity_id` 和业务主键不变。
5. 为 Handle-backed 群成员生成幂等 group rebind outbox 任务。
6. 不执行业务行 owner rebind。
7. 不 reset、merge 或泄露 E2EE private state。

Replace-DID dry-run 的 `store_rebind_counts` 和 `e2ee_cleanup_counts` 保持为兼容字段；在 identity-owned schema 中它们不再通过 `owner_did` 扫描业务表。

## 群成员身份连续性

群成员的 ANP 线上身份只有两种：Handle-backed 使用完整 `member_handle`，DID-only 使用 `agent_did`。本地 `group_members.user_id` / `peer_user_id` 是 `im-core` 生成的不透明关联键，不是 Provider User ID，也不得写入 ANP payload，不能假设它与 Group Host 的 `member_user_id` 相等。

Handle-backed snapshot 必须同时具有规范化完整 Handle、当前 DID 和 canonical positive decimal `handle_binding_generation`；`group_members.anchor_value` 保存的是包含 provider domain 的协议 Handle（例如 `alice.awiki.info`），不是 UI 展示用 local-part。字段不完整时 fail closed，不能静默降级为 DID-only。DID recovery 后，本地成员 `user_id`、角色、入群时间和历史消息关联不变，只更新当前 DID、generation 和 DID history。DID-only 指纹型 DID 变化没有 Handle continuity，不自动合并到旧成员。

旧版本曾把完整 Handle 错投影成 local-part。兼容扫描只能在同一 `owner_identity_id` 内进行，并且必须同时满足：成员 DID 精确存在于该 owner 的 `identity_did_history(status='previous')`、旧 DID 的 `did:wba` provider domain 等于当前完整 Handle domain、成员仍为 active Handle-backed、以及本地 canonical generation 严格小于公开 WNS generation。任一条件缺失都不得补建 rebind job；尤其不能用裸 local-part 跨域合并，也不能用 `old_generation + 1` 猜 generation。

`resume_rebind_recovery` 补建的只是 owner-scoped durable outbox。服务端 roster 的实际变更仍由当前新 DID 签名的 `group.rebind_member` 完成；本地 reconcile 和运维修复均不得直接更新 Group Host 成员表。

群快照投影必须保留 Group Host 返回的 `required_security_profile` / `group_policy.message_security_profile`。P4 成功后仅当权威快照明确为 `transport-protected` 才把 outbox 标记 `complete`；未知、畸形或相互冲突的 profile 继续保留 `awaiting_p6`。历史误留的 transport job 可由 high-level resume 刷新群快照后在本地收敛，但不得借此重复 P4、猜测群安全模式或跳过真实 Group E2EE 的 P6。

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
