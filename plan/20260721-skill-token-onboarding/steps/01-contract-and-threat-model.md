# 步骤 01：冻结 Skill Token Onboarding 契约和威胁模型

状态：`completed`
实施仓库：`awiki-cli-rs2`，同时由四仓库共同评审
实施分支：`feature/skill-token-onboarding`
前置依赖：无
后续步骤：02、03；未完成本步骤前不得开始业务实现

## 1. 目标

- 把已经确认的产品决策写成不可歧义的 v1 协议。
- 冻结 App 复制文本、Token scope、RPC、CLI claim、主动消息和稳定错误码。
- 明确四个仓库各自拥有的真值，避免实现阶段出现重复状态机。
- 完成一次聚焦安全评审，确认 Token 进入智能体上下文后的最大权限仍然可控。

## 2. MVP 边界

- 身份类型固定为 `agent_kind=skill`。
- Token 固定 30 分钟、一次性、只授权新建一个 Skill Agent DID。
- 非空 workspace 始终 fail closed；仅允许恢复同一 claim journal。
- 国内只使用 `awiki.info` 文档和服务，禁止与 `awiki.ai` 交叉兑换。
- App 只签发 Token 和复制提示词，不管理 Skill Agent。
- 注册成功后 Skill Agent 必须主动向 Controller DID 发一条标准 direct message。
- 不设计第二版的多 Agent 管理、Token 续期、身份恢复或跨租户迁移。

## 3. 需要冻结的协议

### 3.1 Token scope

服务端持久化的权威 scope 固定为以下 JSON；`metadata` 只接受已知的非敏感诊断字段。

```json
{
  "purpose": "skill_onboarding_v1",
  "agent_kind": "skill",
  "service_origin": "https://awiki.info",
  "controller_user_id": "<server-owned>",
  "controller_did": "<current-human-did>",
  "controller_full_handle": "alice.awiki.info",
  "agent_handle": "skill-<random>.awiki.info",
  "one_time": true
}
```

- `issued_at`、`expires_at=issued_at+1800s`、状态和 `used_by_did` 使用现有 Token 顶层列。
- raw Token 格式固定为 `awsk1_<base64url-random>`，随机部分至少 256 bit；数据库只存带服务端 pepper 的 hash。
- `controller_*`、`service_origin`、`agent_handle` 和 TTL 都由服务端生成，客户端不能覆盖。

### 3.2 JSON-RPC

端点固定为 `POST /user-service/agent-registration/rpc`，沿用 JSON-RPC 2.0 envelope。

`issue_token` 需要 Human principal；App 只发送：

```json
{"agent_kind":"skill","controller_did":"did:wba:awiki.info:user:alice","controller_handle":"alice.awiki.info","one_time":true,"metadata":{"client":"awiki-me","onboarding_version":1}}
```

响应沿用现有 Token metadata，并增加 Skill scope；仅此响应包含一次 raw `token`：

```json
{"token_id":"agtok_x","token":"awsk1_<secret>","agent_kind":"skill","handle":"skill-x.awiki.info","controller_did":"did:wba:awiki.info:user:alice","controller_user_id":"usr_x","controller_full_handle":"alice.awiki.info","one_time":true,"expires_at":"2026-07-21T12:00:00Z","status":"active","scope":{"purpose":"skill_onboarding_v1","service_origin":"https://awiki.info","agent_handle":"skill-x.awiki.info"}}
```

`verify_token` 匿名且不消费 Token；CLI 只发送 raw Token 和固定 kind：

```json
{"token":"awsk1_<secret>","agent_kind":"skill"}
```

成功响应与 issue metadata 相同但绝不含 `token`；CLI 必须比较 purpose、kind、origin、Controller Handle、Agent Handle、TTL 和状态。

`exchange_token` 匿名，固定请求字段如下；v1 不发送恢复、展示或 endpoint 扩展字段：

```json
{"token":"awsk1_<secret>","agent_kind":"skill","controller_did":"<verify.controller_did>","handle":"<verify.handle>","did_document":{"id":"did:wba:awiki.info:agent:<id>","verificationMethod":[],"authentication":[],"service":[]},"allow_existing_agent_did":false}
```

成功响应固定为 `token_id,did,user_id,agent_kind,controller_user_id,controller_full_handle,controller_did,handle,status`。服务端同一 Token、同一 DID 和相同 DID Document digest 的 replay 返回同一结果；任一值不同均拒绝。`revoke_token` 沿用现有 owner-only RPC，App v1 不调用。

### 3.3 App 复制文本

```text
Read https://awiki.info/cli/onboarding.md and follow the instructions to install AWiki CLI and Skill, initialize a new or empty workspace, then automatically claim the one-time Skill Agent registration below and complete first-use checks.

AWIKI_SKILL_ONBOARDING_V1
service_base_url=https://awiki.info
token=<one-time-token>
controller_handle=alice.awiki.info
agent_handle=skill-x.awiki.info
expires_at=2026-07-21T12:00:00Z
END_AWIKI_SKILL_ONBOARDING_V1

The token authorizes exactly one new Skill Agent DID and one fixed greeting to its controller. Do not print, persist, send, or reuse it. Stop and ask me if the workspace already has a usable identity, any field does not match verified token metadata, or any optional or uncertain step is required.
```

复制文本禁止包含 Controller DID、内部 user ID、人类 JWT、手机号、邮箱或私钥；Token 只出现一次。

### 3.4 CLI claim journal

| phase | 持久化字段 | 可恢复行为 |
|---|---|---|
| `identity_pending` | token_id、scope 摘要、DID、document digest、加密私钥引用 | 仅复用同一 pending DID 后 exchange |
| `identity_registered` | exchange 结果、identity 引用 | 获取 Agent JWT，不再次创建 DID |
| `controller_greeting_pending` | Controller DID、固定正文、message_id | 用同一 Agent identity 和 message_id 重试 |
| `controller_greeting_sent` | accepted receipt 摘要 | 只读首检 |
| `completed` | DID、Handle、Controller Handle、完成时间 | 幂等返回完成结果 |

raw Token、JWT、私钥和完整 HTTP body不得进入 journal。除恢复同一 token_id 的 journal 外，只要 workspace 已有 identity 或其他 pending claim，就在任何远端写入前 fail closed。

### 3.5 主动消息

- sender 为 Skill Agent DID，target 只取 scope 的 `controller_did`，正文固定为 `AWiki Skill Agent 已完成注册，可以开始对话。`。
- `digest=lower_hex(SHA-256("awiki:skill-onboarding:v1:greeting:" + token_id))`。
- `client_message_id="skill-greeting-" + digest[0:32]`，delivery idempotency key 与其相同。
- 使用标准 direct send；Message Service accepted/deduplicated 后才进入 `controller_greeting_sent`。

## 4. 稳定错误矩阵

JSON-RPC code 沿用 User Service：`-32000` 未认证、`-32001` 禁止、`-32003` 冲突、`-32004` 业务错误。CLI `2` 表示本地输入/状态冲突，`3` 表示远端永久拒绝，`74` 表示本地提交失败，`75` 表示可重试远端失败。

| reason | RPC / CLI | retryable | 脱敏提示 |
|---|---|---:|---|
| `skill_onboarding_token_invalid` | `-32000` / 3 | 否 | Token 无效，请从 App 重新生成 |
| `skill_onboarding_token_expired` | `-32000` / 3 | 否 | Token 已过期，请重新生成 |
| `skill_onboarding_token_revoked` | `-32003` / 3 | 否 | Token 已撤销，请重新生成 |
| `skill_onboarding_token_used` | `-32003` / 3 | 否 | Token 已被其他身份使用 |
| `skill_onboarding_scope_mismatch` | `-32004` / 3 | 否 | Token scope 与请求不一致 |
| `skill_onboarding_workspace_conflict` | local / 2 | 否 | workspace 已有身份或其他 claim |
| `skill_onboarding_response_mismatch` | local / 3 | 否 | 服务响应与已验证 scope 不一致 |
| `skill_onboarding_local_commit_failed` | local / 74 | 是 | 本地提交失败，请在原 workspace 重试 |
| `skill_onboarding_greeting_pending` | local / 75 | 是 | 注册已完成，Controller 消息等待重试 |

所有错误只允许出现 reason、phase、token_id 和非敏感 handle；不得附带 Token、JWT、私钥、内部 user ID 或原始请求/响应 body。

## 5. 仓库所有权

| 真值 | Owner |
|---|---|
| Token 状态、Controller 归属、Agent Handle | User Service |
| 私钥、pending claim、当前 CLI identity | im-core |
| 命令参数、stdin、输出和 Skill 行为 | awiki-cli |
| Token 签发和复制文本 | awiki-me |
| 标准 direct message 和 owner 隔离 | Message Service |

## 6. 安全评审清单

- Token 泄露后不能登录人类账号或恢复已有 DID。
- Token 不能跨 `awiki.info`/`awiki.ai` 使用。
- Token 不能被 redirect 到其他 origin。
- prompt 字段不能改变服务端 scope。
- raw Token 不进入日志、遥测、错误、数据库或本地 journal。
- 并发兑换最多产生一个 Agent DID。
- 注册成功但客户端崩溃时可复用同一 DID 完成收尾。
- 主动消息不能改 target、正文或 sender。
- Skill Agent JWT 不能读取 Controller inbox/history/sync。

## 7. 本步骤产物

- 一份四仓库共同认可的 RPC 示例。
- 一份 Token scope schema。
- 一份 prompt block 示例。
- 一份 claim 状态机表。
- 一份稳定错误矩阵。
- 一份威胁与控制清单。

这些内容可以写入本计划目录或对应权威 API 文档，但不得开始生产代码修改。

## 8. 验证

- 文档中的字段名称在四仓库中没有第二种拼写。
- 所有 confirmed decision 都能追溯到总计划第 2、18 节。
- 没有把 App 生命周期管理重新引入 Skill Agent。
- 没有新增 v1 目标以外的通用授权平台设计。
- `git diff --check` 通过。

## 9. 完成标准

- 四仓库实现人员可以只依赖冻结契约并行开发。
- RPC、Token、prompt、claim 和主动消息没有未决语义。
- 安全评审没有阻断项。
- 步骤 02 和步骤 03 可以开始。
