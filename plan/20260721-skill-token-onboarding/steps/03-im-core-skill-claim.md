# 步骤 03：实现 im-core Skill Claim 安全状态机

状态：`completed`  
实施仓库：`awiki-cli-rs2`  
Worktree：`/home/ecs-user/awiki-space/awiki-cli-rs2-skill-token-onboarding`  
实施分支：`feature/skill-token-onboarding`  
前置依赖：步骤 01；与步骤 02 可基于冻结 mock 并行  
后续依赖方：步骤 04、07

## 1. 目标

- 在 im-core 中实现可恢复的 Skill Token claim，而不是在 CLI handler 手写网络和密钥逻辑。
- 本地生成并安全保存 Agent DID 私钥，兑换 User Service Token，取得 Agent JWT。
- 注册成功后主动给 Controller DID 发送一条固定标准 direct message。
- 对崩溃、超时和重复执行保持同 DID、同消息的幂等行为。

## 2. 不做的内容

- 不修改普通人类 `id register` 和 `id recover`。
- 不把 raw Token 暴露到 Dart public API。
- 不增加通用 OAuth/device-code 框架。
- 不支持已有身份恢复、Token 续期或多 DID claim。
- 不把 App 管理语义放进 im-core。

## 3. 类型和接口

- 增加不可 Serialize、手写脱敏 Debug 的 `SkillOnboardingToken`。
- 增加 typed `SkillTokenMetadata`，只含公开 verify 字段。
- 增加 typed `SkillClaimRequest/Result` 和稳定 phase/status。
- User Service client 只调用冻结的 verify/exchange RPC。
- HTTP client 禁止携带 Token 跨 origin redirect。
- `service_base_url` 必须是 HTTPS，并与 verify metadata 的 service origin 完全一致。

## 4. Workspace 规则

- claim 仅允许已初始化且没有可用 identity 的 workspace。
- 发现人类 identity、其他 Agent identity 或无法识别状态时返回 workspace conflict。
- 不删除、不覆盖、不切换已有 identity。
- 唯一例外是同 token_id 对应的 `pending_skill_claim` 或 greeting journal 恢复。
- 恢复时必须再次核对 service origin、DID、document digest 和 Controller scope。

## 5. Pending journal

journal 只保存非敏感恢复信息：

```text
token_id
service_origin
controller_did
controller_full_handle
agent_handle
agent_did
did_document_digest
phase
greeting_message_id
last_error_code
updated_at
```

- 不保存 raw Token、JWT、private key、HTTP body 或完整错误堆栈。
- private key 进入现有 identity vault/secure storage。
- pending identity 在 exchange 成功前不设为 default/ready。

## 6. Claim 状态机

### 6.1 Verify

- 从调用方一次性读取 Token。
- 调用 `verify_token`。
- 校验 active、kind=skill、purpose、expiry、origin 和 expected Handle。
- verify 失败时不生成 key、不写 workspace。

### 6.2 生成本地身份

- 使用现有 did:wba key/DID Document builder。
- DID domain、Handle service 和 public key满足 Token metadata。
- 写入 secure pending identity 和 journal。
- 生成一次后立即固定；重试必须复用。

### 6.3 Exchange

- 请求字段只来自 verified metadata 和本地 identity。
- 校验 response DID、Handle、kind、Controller DID 和公开 scope。
- response mismatch 保留 pending 状态并停止。
- exchange 成功后用 DID proof 调用现有 auth refresh/get_me 取得 Agent JWT。

### 6.4 本地提交

- 原子保存 DID Document、JWT、Handle 和 ready 状态。
- 将该 Skill identity 设为当前 identity。
- 保留 token_id 与注册结果的非敏感审计关联。
- 清理 identity pending phase，但立即进入 greeting pending。

### 6.5 主动消息

- greeting target 只能使用 verified `controller_did`。
- sender 使用新 Agent DID/JWT。
- 使用现有 im-core direct message service，不发 raw RPC。
- 固定文本：“AWiki Skill Agent 已完成注册，可以开始对话。”
- message ID/idempotency key 从 token_id 确定性派生。
- Message Service 接受后标记 `controller_greeting_sent`。
- greeting 成功前 claim 返回 pending/retryable，而不是重新注册。

## 7. 崩溃与重试

- key 生成后、exchange 前崩溃：读取 pending identity，继续 exchange。
- exchange 成功、JWT 前崩溃：同 DID/document replay exchange，再取 JWT。
- identity ready、greeting 前崩溃：跳过 exchange，重试同一 greeting。
- greeting 已发送、final commit 前崩溃：同 message ID 重试，由 Message Service 去重。
- Token 被其他 DID 使用：标记 claim failed，不设 default，不删除审计 journal。
- 本地 secure storage 失败：在任何远端副作用前失败。

## 8. 错误映射

- 将 User Service reason 映射为步骤 01 冻结的稳定 im-core error code。
- 网络 timeout/5xx 标记 retryable。
- invalid/expired/revoked/scope mismatch 标记 permanent。
- greeting failure 使用 `skill_onboarding_greeting_pending`。
- Display/Debug 不出现 Token、JWT、private key 或完整 HTTP response。

## 9. 测试

### 9.1 状态机

- 空 workspace 完成 verify -> exchange -> JWT -> greeting -> completed。
- 非空 workspace 在任何远端调用前拒绝。
- 同一 journal 恢复被允许，其他 identity 被拒绝。
- verify 失败不生成本地 key。
- response mismatch 不提交 identity。

### 9.2 崩溃点

- 每个 phase 后模拟崩溃并恢复。
- 恢复始终复用同一 DID 和 greeting message ID。
- exchange/greeting 网络超时不会创建第二个 DID 或第二条消息。

### 9.3 安全

- Token/私钥/JWT 的 Debug 和 error redaction。
- 跨 origin redirect 被拒绝。
- prompt Handle 与 verify metadata 不符时拒绝。
- journal、SQLite 和普通配置文件没有 raw Token。

### 9.4 回归

- identity register/recover/refresh-token 现有测试通过。
- direct message、owner scope 和 identity vault 测试通过。
- 同步和异步 im-core API 行为一致。

## 10. 完成标准

- im-core 提供一个完整、typed、可恢复的 Skill claim 操作。
- CLI handler 不需要直接处理 DID 私钥、User Service JSON 或消息 idempotency。
- 成功结果包含 Agent DID、Handle、Controller Handle 和 greeting 状态，不含 secret。
- 所有崩溃点都能恢复且不重复注册/发消息。
- 聚焦测试、相关 im-core 全量测试、格式和 Clippy 通过。

## 11. 实施结果与验证

- 已新增 environment-level `ImCore::onboarding()` 与 typed、脱敏的 claim API。
- 已实现 verify、pending identity、exchange、DID auth、本地提交、greeting pending 和 completed 状态恢复。
- pending key 使用现有 SecretVault；FileCompat 兼容路径使用 `0600` 文件。journal 不保存 Token、JWT 或私钥。
- Token RPC 使用 no-redirect HTTP client；服务 origin、Controller/Agent Handle、DID domain 和非空 workspace 均 fail closed。
- greeting 使用新 Agent 身份、固定正文和由 `token_id` 确定性派生的 message/idempotency key。

步骤 03 定向验证：

```text
cargo test -p awiki-im-core skill_onboarding
12 passed, 0 failed

cargo fmt --all -- --check
passed

git diff --check
passed
```

`cargo clippy -p awiki-im-core --lib -- -D warnings` 被 18 个既有、与本步骤无关的 lint 阻断；错误均位于未修改的 group、message、local-state 等文件，本步骤改动未出现 lint。全量测试和统一 Clippy 基线处理按用户要求留到步骤 07。
