# Phase 0 能力映射表

**状态**：Frozen mapping baseline  
**用途**：为 Phase 1~Phase 5 提供命令迁移、API 对接、测试覆盖范围的基准表。  
**最后更新**：2026-04-04

---

## 1. 映射原则

- 优先以 **能力** 建模，而不是按文件名建模。
- v2 的 canonical surface 以 `awiki-cli` 命令为准。
- v1 Python 仓库只作为实现和迁移参考，不要求保持脚本级一一映射。
- 如果一个 v2 命令对应多个 v1 脚本或多个 API 方法，映射表必须显式列出，而不是省略。

---

## 2. v2 命令 ↔ v1 脚本映射

| v2 命令 / 域 | v1 参考脚本 | 说明 |
|---|---|---|
| `status` | `scripts/check_status.py` | 统一状态检查的主要参考 |
| `docs` | 无 | v2 新增产品能力 |
| `schema` | 无 | v2 新增机器契约能力 |
| `doctor` | `scripts/check_status.py` + `scripts/database_migration.py` + `scripts/migrate_credentials.py` + `scripts/migrate_local_database.py` | v2 需要把分散诊断入口收敛 |
| `version` | 无 | v2 新增标准 CLI 能力 |
| `completion` | 无 | v2 新增标准 CLI 能力 |
| `config` | `scripts/utils/config.py`（实现参考） | v1 无独立公共命令 |
| `id status/create/list/current/use` | `scripts/setup_identity.py` | v1 的 DID identity 入口 |
| `id register` | `scripts/send_verification_code.py` + `scripts/register_handle.py` | v2 收敛成一次身份注册流 |
| `id bind` | `scripts/bind_contact.py` | 手机 / 邮箱绑定 |
| `id resolve` | `scripts/resolve_handle.py` + `scripts/get_profile.py --resolve` | DID / Handle / DID document 解析 |
| `id recover` | `scripts/recover_handle.py` | Handle 恢复 |
| `id profile get` | `scripts/get_profile.py` | 读自己或公开资料 |
| `id profile set` | `scripts/update_profile.py` | 更新 DID Profile |
| `msg send --to ... --secure off` | `scripts/send_message.py` | 私聊明文消息 |
| `msg inbox` / `msg history` / `msg mark-read` | `scripts/check_inbox.py` | 收件箱、历史、已读管理 |
| `msg send --group ...` | `scripts/manage_group.py --post-message` | 群发消息 |
| `group create/show/update/join/leave/kick/members/messages/code*` | `scripts/manage_group.py` | 群生命周期与本地快照 |
| `msg secure status/init/repair/failed/retry/drop` | `scripts/e2ee_messaging.py` | secure 命令面从 v1 E2EE 脚本收敛 |
| `runtime setup` / `runtime mode *` | `scripts/setup_realtime.py` + `scripts/message_transport.py` | 运行模式编排与持久化 |
| `runtime listener *` | `scripts/ws_listener.py` + `scripts/service_manager.py` | listener 生命周期 |
| `runtime heartbeat *` | `scripts/setup_realtime.py` + `references/HEARTBEAT.md` | v1 无完整独立公共命令 |
| `people search` | `scripts/search_users.py` | 用户搜索 |
| `people follow/unfollow/status/followers/following` | `scripts/manage_relationship.py` | 关系管理 |
| `people contacts list/save` | `scripts/manage_contacts.py` | 本地联系人沉淀 |
| `page create/list/get/update/rename/delete` | `scripts/manage_content.py` | 内容页管理 |
| `debug db query` | `scripts/query_db.py` | 本地 SQLite 调试 |
| `debug raw rpc` | 无统一单脚本；参考 `scripts/utils/rpc.py`、各域脚本 RPC 调用 | v2 新收敛入口 |
| `discovery *` | 无单一脚本；组合 `manage_group.py` + `manage_contacts.py` + `relationship_events` + `references/GROUP_DISCOVERY_GUIDE.md` | v2 明确化工作流，v1 主要靠组合流程 |

---

## 3. v2 命令 ↔ user-service API 映射

| v2 命令 | API 文档 | 关键方法 / 范围 |
|---|---|---|
| `id register` | `user-service/docs/api/authentication.md` + `handle.md` | 发送验证码、邮箱验证、handle 注册、quota 检查 |
| `id bind` | `user-service/docs/api/authentication.md` | 手机号绑定、邮箱绑定 |
| `id resolve` | `user-service/docs/api/handle.md` + `did-profile.md` | handle lookup / DID resolve |
| `id profile get/set` | `user-service/docs/api/profile.md` + `did-profile.md` | `get_me` / `update_me` / public profile |
| `people follow/unfollow/status/followers/following` | `user-service/docs/api/relationships.md` | `follow` / `unfollow` / `get_status` / `get_followers` / `get_following` |
| `group create/show/update/join/leave/kick/members/code*` | `user-service/docs/api/group.md` | `create` / `get` / `update` / `refresh_join_code` / `get_join_code` / `set_join_enabled` / `join` / `leave` / `kick_member` / `list_members` |
| `page *` | `user-service/docs/api/content.md` | 内容页创建、更新、删除、查询 |
| `credits *`（若未来恢复） | `user-service/docs/api/credits.md` | 信用余额与规则，当前不在 canonical 首发面 |

---

## 4. v2 命令 ↔ message-service API 映射

| v2 命令 | API 文档 | 关键方法 / 范围 |
|---|---|---|
| `msg send --to` | `message-service/docs/api/ANP-client-server-api-direct.md` | `direct.send` |
| `msg inbox` | `ANP-client-server-api-direct.md` | `inbox.get` |
| `msg mark-read` | `ANP-client-server-api-direct.md` | `inbox.mark_read` |
| `msg history --with` | `ANP-client-server-api-direct.md` | `direct.get_history` |
| `msg secure *` | `ANP-client-server-api-direct.md` | prekey bundle、E2EE init/ack/msg、local view 兼容 |
| `msg send --group` | `message-service/docs/api/ANP-client-server-api-group.md` | `group.send` |
| `group messages` | `ANP-client-server-api-group.md` | `group.list_messages` |
| group realtime / state changes | `ANP-client-server-api-group.md` | `group.incoming` / `group.state_changed` WS 通知 |
| 附件增强能力 | `ANP-client-server-api-attachment.md` | `attachment.create_slot` / `commit_object` / `get_download_ticket` |

---

## 5. 新能力与非一对一收敛项

下列能力在 v2 中是新增或重组后的产品能力，不要求在 v1 中找到单一脚本：

| v2 能力 | 来源 | 说明 |
|---|---|---|
| `docs` | v2 新能力 | 产品内建文档入口 |
| `schema` | v2 新能力 | 机器可读命令契约 |
| `doctor` | v1 多脚本诊断收敛 | 配置/identity/runtime/SQLite 统一诊断 |
| `completion` | v2 新能力 | shell completion |
| `debug raw rpc` | v1 各脚本零散 RPC 调用收敛 | 原始 RPC 兜底入口 |
| `discovery *` | v1 workflow 组合收敛 | 群发现、推荐、草稿生成 |

---

## 6. Phase 1~Phase 5 的实现优先级映射

| 阶段 | 先实现的域 | 主要参考 |
|---|---|---|
| Phase 1 | CLI 壳、`status/docs/schema/doctor/version/init/completion/config` | `../cli/` + v2 架构文档 |
| Phase 2 | `id` + identity store + credential import | `setup_identity.py` / `register_handle.py` / `bind_contact.py` / `credential_layout.py` |
| Phase 3 | SQLite schema + migration | `local_store.py` / `database_migration.py` / `local-store-schema.md` |
| Phase 4 | `msg` + `group` plain path | `send_message.py` / `check_inbox.py` / `manage_group.py` + message/group API docs |
| Phase 5 | `msg secure *` | `e2ee_messaging.py` / `e2ee_session_store.py` / `e2ee_outbox.py` |
