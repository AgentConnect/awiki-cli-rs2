# 步骤 02：实现 User Service Skill Token 和 Controller 绑定

状态：`completed`  
实施仓库：`user-service`  
Worktree：`/home/ecs-user/awiki-space/user-service-emas-push`  
实施分支：`feature/emas-push-user-service`  
前置依赖：步骤 01 契约冻结  
后续依赖方：步骤 03、05、06、07

## 1. 目标

- 在现有 agent registration token 状态机中增加 `agent_kind=skill`。
- 让认证用户签发一个 30 分钟、一次性、绑定 Controller scope 的 Token。
- 让 CLI 用该 Token 原子创建独立 Agent user、DID、Handle 和 inventory 绑定。
- 支持同 Token、同 DID、同 DID Document digest 的安全幂等重试。

## 2. 不做的内容

- 不新增第二套 Token 表或 JWT 类型。
- 不让 Token 注册人类账号、恢复已有 DID 或兑换多个 DID。
- 不给 App 增加 Skill Agent 查询、改名、删除或状态 API。
- 不在 User Service 发送主动消息；消息由注册成功后的 Skill Agent 发送。
- 不修改 EMAS Push installation 逻辑。

## 3. 开始前检查

- 将该 feature 分支同步到最新 `origin/release/0714`，保留已有 EMAS commit。
- 阅读 `agent_registration/`、`agent_inventory/`、DID auth 和 storage 目录约束。
- 确认现有 daemon/runtime exchange 的事务边界和审计行为。
- 确认数据库已有 `used_by_did`；评估 DID Document digest 是新列还是受控 scope 字段。

## 4. Schema 和类型

- 扩展 `AgentKind` 为 `daemon | runtime | skill`。
- Token record 继续复用 `agent_registration_tokens`。
- `agent_kind` 现有长度足够，不为 `skill` 单独建表。
- Skill scope 必须包含步骤 01 冻结字段。
- 增加或持久化 `used_document_digest`，用于同 DID 幂等 replay。
- `agent_inventory` 继续保存绑定，Skill row 固定：

```text
agent_kind = skill
daemon_agent_did = NULL
controller_user_id = authenticated human user
controller_did = issue-time current DID snapshot
controller_full_handle = issue-time active full handle
created_by_token_id = token_id
```

## 5. issue_token

- 只允许 authenticated principal 调用。
- 通过现有 ControllerScope resolver 得到稳定 user ID、当前 DID 和 active full Handle。
- 忽略客户端提供的 Controller user ID、service origin、Agent Handle 和过期时间。
- Skill Token TTL 服务端固定为 1800 秒。
- 生成 `awsk1_` Token 和随机 Agent Handle。
- Agent Handle 使用足够随机的 local part，避免与普通注册竞争。
- scope 固定 `purpose=skill_onboarding_v1` 和当前部署 `service_origin`。
- metadata 只接受 client/platform/version 等脱敏 allowlist。
- raw Token 只在本次 issue response 返回。

## 6. verify_token

- 保持匿名、只读、不消费 Token。
- 校验 hash、status、kind、purpose 和 expiry。
- 对外返回 token_id、service origin、Controller Handle/DID、Agent Handle 和过期时间。
- 不返回 controller_user_id、token hash、内部数据库 ID 或其他账号 secret。
- invalid token 不提供可用于枚举的差异信息。

## 7. exchange_token

- 只接受 `agent_kind=skill` 和 `purpose=skill_onboarding_v1`。
- `allow_existing_agent_did=true` 固定拒绝。
- Controller DID、Agent Handle 和 service origin 必须与 Token record 完全一致。
- DID 必须是部署域允许的 did:wba。
- DID Document 必须包含正确 public key、authentication 和同域 Handle service。
- Agent Handle 必须等于 Token 生成值。
- DID Document digest 使用规范化 JSON 计算。
- 单事务完成：

```text
lock token
validate active scope
create Agent user(role=agent:skill)
create DID document
create Agent handle
create agent_inventory row
mark token used + used_by_did + document_digest
commit
```

- 任一步失败时不能消费 Token或留下半条业务记录。

## 8. 幂等和并发

- 第一次 exchange 成功后 Token 进入 used。
- replay 只有在 token、DID 和 document digest 全部相同时返回同一公开结果。
- replay 不返回长期 secret；CLI 使用 DID proof 获取 JWT。
- 同 Token 不同 DID/document 永久返回 `skill_onboarding_token_used`。
- 并发 exchange 使用行锁和唯一约束，确保只有一个事务成功。
- 幂等 replay 只在 Token 原有效期加受控 grace 内允许。

## 9. Controller 归属与生命周期

- Agent user 和人类 user 保持独立。
- `controller_user_id` 是控制归属真值，Controller DID/Handle 是可同步快照。
- 现有 inventory list 默认不得让 App 把 `skill` 错误解析成 runtime。
- v1 可在普通 App list API 中排除 `skill`，或确保 App adapter 明确过滤。
- 服务端保留运维归档和 DID/Handle 安全处置能力，但不增加 App UI。

## 10. 审计和限流

- 复用 issued/verified/exchanged/revoked/failed 事件名。
- audit extra 记录 purpose、agent kind、token_id 和脱敏结果。
- 每用户限制 active Skill Token 数和单位时间 issue 次数。
- 对 verify/exchange 失败做 IP 限流，不记录 raw request body。
- 日志、异常和 metrics 不得包含 Token、JWT 或完整 DID Document。

## 11. 测试

### 11.1 Service/RPC

- 当前用户可以签发 `skill` Token。
- user-id JWT、错误 Controller DID/Handle 和 agent principal 签发被拒绝。
- TTL 恒为 30 分钟，客户端不能覆盖。
- verify 返回正确公开 metadata，且不消费 Token。
- expired、revoked、invalid、wrong purpose/kind 返回稳定错误。

### 11.2 Storage/transaction

- exchange 原子创建 Agent user、DID、Handle 和 inventory。
- `daemon_agent_did` 为 NULL，role 为 `agent:skill`。
- 同 DID/document replay 幂等。
- 不同 DID replay 和并发竞争只有一个成功。
- 模拟事务中间失败后 Token 仍 active，业务表无残留。
- 数据库没有 raw Token。

### 11.3 回归

- daemon/runtime issue、verify、exchange、recovery 行为不变。
- DID auth、ControllerScope 和 inventory 现有测试通过。
- EMAS Push installation 测试不回归。

## 12. 完成标准

- App 能签发可供 CLI 预检的 Skill Token。
- CLI 可以原子创建一个绑定正确 Controller 的 Skill Agent DID。
- Token 一次性、30 分钟、跨域和跨 DID fail closed。
- 重试不会创建第二个 DID 或 inventory row。
- 生产日志和存储没有 raw Token。
- 聚焦测试和 `git diff --check` 已通过；全量 User Service 测试按统一节奏留到步骤 07。

## 13. 实施证据

- Service/RPC 定向测试：`35 passed`。
- 真实隔离 MySQL storage/transaction/concurrency：`9 passed`；双 session 竞争只创建一个 DID/inventory。
- 临时数据库与测试账号已在验证后清理。
- `ruff check` 和 `git diff --check` 通过；未执行仓库全量测试。
- 实现复用现有 token 表；DID Document digest 写入 scope，没有新增数据库列或第二套 Token 系统。
