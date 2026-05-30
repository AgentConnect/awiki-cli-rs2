# 步骤 04：稳定消息会话

主计划：[../plan.md](../plan.md)  
步骤编号：04  
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
| 下一步 | 用稳定 `conversation_id` 替换包含 owner DID 的 direct thread key。 |

## 2. 目标

- 产出：direct conversation identity 不再包含本地 owner DID。
- 用户/系统行为：DID 替换不会拆分同一个 direct conversation。
- 非目标：改变 ANP wire message ids、service-side thread semantics、secure protocol semantics 或 public discovery。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `schema.rs` | 增加/确认 `conversation_id`，并让 `threads`/conversation view 按 `owner_identity_id, conversation_id` 分组。 | `thread_id` 可保留为 alias。 |
| `messages.rs` | 存储/读取 `conversation_id`；classification/mark-read 按 owner identity。 | 按需要保持 DTO 兼容。 |
| `local_projection.rs` | 生成 `dm:<peer_did>` 和 `group:<group>` 稳定 key。 | 移除 direct key 中的 owner DID。 |
| `conversations.rs` | 按 owner identity 和 stable conversation id 列出 conversations。 | 不再回退到 `owner_did`。 |
| `message_runtime/conversations.rs` | 将 records 映射为 public `Conversation`。 | 过渡期 public `ThreadRef` 可保留 alias。 |
| Dart facade | 如果 DTO 暴露新字段，则更新映射。 | 如变更则运行 codegen。 |

## 4. 依赖

- 前置步骤：步骤 03。

## 5. 核心设计

Storage 增加稳定 `conversation_id`：

- direct: `dm:<peer_did>`
- group: `group:<group_id_or_group_did>`
- mail: `mail:<source>`

因为 `owner_identity_id` 已经是分区键，direct conversation id 不能包含本地 owner DID。如果 public DTO 暂时仍使用 `thread_id`，则设置 `thread_id = conversation_id`，并把它记录为 legacy alias。

Secure direct/group projection 使用同一套稳定 `conversation_id` 规则，但 public conversation DTO 不得暴露 P5 session ids、ratchet counters、MLS epochs、raw ciphertext、raw notices 或 provider paths。

## 6. 实施指南

1. 如果步骤 03 尚未完成，在 `MessageRecord` 和 storage SQL 中加入 `conversation_id`。
2. 更新 local projection helpers：
   - 将 `direct_thread_id(owner_did, peer_did)` 替换为 `direct_conversation_id(peer_did)`；
   - group helper 保持 owner-independent。
3. 更新 `threads` view，或替换为按 `conversation_id` 分组的 identity-owned `conversations` query。
4. 更新 mark-read、inbox/history 回退、realtime local projection tests。
5. 如果 Rust public DTO 增加 `conversation_id`，更新 `im-core-dart` 映射和生成文件。
6. 审查 secure direct/group conversation mappings，确保 redacted secure status 与 raw crypto/session/provider details 分离。

## 7. 验收标准

- [ ] 活跃 direct conversation key 不包含本地 owner DID。
- [ ] 如果存在 `thread_id`，新 rows 中它等于 stable `conversation_id`。
- [ ] 模拟 DID replacement 后，conversation list 仍返回一个 direct conversation。
- [ ] 如果 secure direct/group conversation DTO 有变化，只暴露 stable IDs 和 redacted status fields。
- [ ] Mark-read 和 local message classification 仍工作。
- [ ] 审查发现 已处理或明确记录。
- [ ] 已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked conversation` | Conversation tests 通过。 |
| 单元 | `cargo test -p im-core --locked local_projection` | Stable id tests 通过。 |
| 搜索 | `rg "direct_thread_id\\(|dm:\\{owner|owner_did.*peer_did" crates/im-core/src/internal/message_runtime crates/im-core/src/internal/secure_direct` | 不再有 owner-DID direct key generation。 |
| 搜索 | `rg "session_id|send_n|recv_n|skipped_key|KeyPackage|Welcome|Commit|Proposal|provider.*path" crates/im-core/src/messages crates/im-core-dart packages/awiki_im_core` | Public DTO 命中已 审查，保持 redacted/internal-only。 |
| Dart | `scripts/flutter/codegen-check.sh` if DTO changed | Generated bindings 是最新的。 |

## 9. 审查流程

- 检查 public API 兼容和 Dart model mapping。
- 检查 old/new DID mixed fixture 会去重 conversations。
- 检查 secure conversation mappings 不暴露 raw direct/group E2EE internals。

## 10. 提交要求

- 建议提交信息：`im-core: stabilize local conversation keys`

## 11. 风险、回滚和后续

- 风险：用户或测试可能假设旧的 `dm:<owner>:<peer>` 形状。
- 回滚/回退：步骤 08 保留从旧 key 到新 key 的 read-time migration；新写入不得重新引入 owner DID。
