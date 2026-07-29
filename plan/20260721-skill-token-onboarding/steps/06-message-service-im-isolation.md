# 步骤 06：验证 Message Service Skill Agent IM 和隔离

状态：`completed`
实施仓库：`message-service`
Worktree：`/home/ecs-user/awiki-space/message-service-emas-push`
实施分支：`feature/emas-push-message-service`
前置依赖：步骤 02 已能创建 `agent:skill` identity
后续依赖方：步骤 07

## 1. 目标

- 证明 Skill Agent 使用现有 DID/JWT 即可进行标准 direct message。
- 证明首次主动消息可以到达人类 Controller 的 App inbox/sync/realtime。
- 证明 Agent 与 Controller 的消息、历史和同步 owner scope 完全隔离。
- 只有测试暴露现有 role allowlist 阻断时才做最小生产改动。

## 2. 不做的内容

- 不新增 Skill Agent 专用消息 API、表、队列或协议。
- 不让 Message Service读取 onboarding Token 或 agent inventory。
- 不把 Controller binding 复制到 Message Service 数据库。
- 不增加 App 管理或 Agent lifecycle 能力。
- 不修改 EMAS provider，除非现有普通 direct message Push 测试发现回归。

## 3. 开始前检查

- 将 feature 分支同步到最新 `origin/release/0714`，保留 EMAS commit。
- 搜索 auth/admission 对 `agent:daemon`、`agent:runtime` 和 role 前缀的判断。
- 确认普通 DID JWT 的 sender/owner 校验不依赖 Agent kind。
- 确认 direct send 已有 message ID/idempotency 去重。

## 4. 先写兼容测试

构造两个独立 principal：

```text
Controller: human DID + human JWT
Skill Agent: skill DID + role=agent:skill JWT
```

验证：

- Skill Agent 能向 Controller DID 发标准 direct message。
- Controller inbox/history/sync 可以读取该消息。
- realtime session 在线时收到标准 message event。
- Controller 可以沿同一会话回复 Skill Agent。
- sender DID、receiver DID、thread 和 owner scope 正确。

## 5. 主动消息幂等

- 使用步骤 01 冻结的 message ID/idempotency key。
- 第一次发送 accepted，重复发送返回同一结果或 deduplicated success。
- Controller inbox、history、sync 和 conversation projection 只有一条消息。
- 消息正文固定且不含 Token、JWT、Controller user ID。
- target 只能是请求中已验证的 Controller DID；Message Service 不信任 display Handle。

## 6. 隔离测试

- Skill Agent JWT 不能读取 Controller inbox/history/sync。
- Controller JWT 不能以 Skill Agent DID 作为 sender。
- Skill Agent 不能使用 internal service route。
- `agent:skill` 不能获得 daemon status/inventory 权限。
- 第三方用户不能读取 Controller 与 Skill Agent 的 direct thread。
- Skill Agent 的 websocket session 只收到自己的事件。

## 7. 最小生产改动原则

如果兼容测试全部通过：

- 不修改生产代码。
- 只提交契约/集成测试和必要文档。

如果 role allowlist 阻断：

- 仅在现有 Agent role 解析处加入 `agent:skill`。
- 不新增绕过 DID/JWT proof 的特殊分支。
- 不放宽 service、peer、federation 或 internal token 鉴权。
- 新增负向测试证明权限没有扩大。

## 8. EMAS 回归

- Skill Agent 主动消息进入现有 local_direct/outbox 路径时行为与普通 direct message 一致。
- Controller 有 active Android installation 时可按现有配置触发普通 EMAS 通知。
- Push envelope 继续不包含正文、完整 DID、Token 或 JWT。
- `push.enabled=false` 默认行为不变。
- 不为 Skill Agent 建新的 Push target 类型。

## 9. 测试层级

### 9.1 Unit/contract

- role 解析、sender identity、owner scope、route access。
- fixed greeting 的 message ID/idempotency。
- cross-owner 和 sender spoofing 拒绝。

### 9.2 Storage/integration

- direct accept、inbox append、sync event、history projection 只有一份。
- 重复 request 不重复写入。
- PostgreSQL 测试需要实际 `MESSAGE_SERVICE_STORAGE_TEST_DATABASE_URL`。

### 9.3 App/CLI peer contract

- CLI Skill Agent 发消息，Controller 客户端读取。
- Controller 回复，Skill Agent 通过 history/sync/realtime读取。
- 不使用专用测试后门或直接插数据库。

## 10. 完成标准

- `agent:skill` 可以走标准 direct message 双向通信。
- 主动消息在所有投影中恰好一次。
- Agent 和 Controller owner scope 完全隔离。
- 没有新增专用 API、表或鉴权旁路。
- Message Service workspace tests、Clippy、格式和 `git diff --check` 通过。
- 有数据库时 PostgreSQL 集成用例真实通过；无数据库时明确记录未验证范围。

## 11. 实施结果与验证

- JWT 鉴权测试证明带 `role=agent:skill` 的 RS256 Token 被解析为独立 DID principal；Message Service 不读取或信任角色字段。
- route contract 证明 Skill Agent 只能走本域 user RPC，不能在 federation route 伪装 peer/service principal。
- internal route 测试证明普通 Skill Agent bearer 不能替代内部服务 Token。
- Direct 综合测试覆盖固定 greeting、稳定 operation/message ID、重复发送幂等、Controller inbox/history/realtime、双向回复、sender spoof 拒绝和第三方隔离。
- Sync 测试证明 Controller、Skill Agent 和第三方按 DID owner stream 隔离，跨 owner `user_did` 请求 fail closed。
- PostgreSQL 测试覆盖 direct message/inbox 唯一性、idempotency replay、双向可见 history 和 sync owner 隔离。
- 现有 EMAS mock E2E fixture 改为 Skill Agent greeting，继续证明 Push envelope 不包含完整 DID 或正文。
- 更新 User Service/Message Service 边界文档；兼容测试未发现生产 allowlist 阻断，因此没有修改任何生产实现或协议。

步骤 06 定向验证：

```text
cargo test -p im-identity bearer_auth_accepts_agent_skill_as_an_independent_did_principal
1 passed, 0 failed

cargo test -p im-app skill_agent
2 passed, 0 failed

cargo test -p im-direct skill_agent_greeting_is_idempotent_bidirectional_and_owner_isolated
1 passed, 0 failed

cargo test -p im-sync skill_agent_and_controller_sync_streams_are_owner_isolated
1 passed, 0 failed

cargo test -p im-push --test push_flow skill_greeting_outbox_to_user_service_to_emas_flow_is_redacted
1 passed, 0 failed

cargo test -p im-storage postgres_skill_greeting_is_idempotent_and_owner_isolated -- --nocapture
1 passed；因 MESSAGE_SERVICE_STORAGE_TEST_DATABASE_URL 未设置而明确 skip 真实 PostgreSQL body

cargo clippy -p im-identity -p im-app -p im-direct -p im-sync -p im-storage -p im-push --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
passed
```

真实 PostgreSQL、workspace 全量测试、App/CLI peer 和远端系统验证统一留到步骤 07；本步骤未运行任何全量测试。
