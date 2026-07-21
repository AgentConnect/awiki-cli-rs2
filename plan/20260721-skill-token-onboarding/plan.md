# 计划：App 一键安装 AWiki Skill 并用一次性 Token 注册 Agent DID

状态：步骤 01-06 已完成；步骤 07 进行中，AWiki Me `full` 已通过，remote system test 尚有 2 个既有 secure-direct 失败
创建日期：2026-07-21  
文档目录：`plan/20260721-skill-token-onboarding/`  
主实施分支：`awiki-cli-rs2/feature/skill-token-onboarding`  
基线：`origin/release/0714@de44ee74`  
恢复指针：功能分支和 CLI stable artifact 均已提交发布；步骤 07 已解除 device-bound 环境门禁，后续仅处理或确认 remote suite 的 2 个 secure-direct 失败。
行数约束：本文必须少于 500 行。

## 1. 执行摘要

- App 为当前已登录用户签发一个短时、一次性、用途受限的 Skill Agent 注册 Token。
- App 生成一段可复制给 Codex、Claude Code、OpenClaw 等智能体的安装提示词。
- 提示词包含官方 onboarding URL、服务域名、Token、Controller Handle 和过期时间。
- 智能体按官方文档安装 `awiki-cli` 和 AWiki Skill，初始化一个新的或空的 AWiki workspace。
- CLI 新增 `awiki-cli onboarding claim`，先预检 Token，再在本地生成 Agent DID 和私钥。
- CLI 使用 Token 调用 User Service 的注册兑换接口，原子创建 Agent 用户、DID、Handle 和 inventory 归属。
- Agent DID 与人类用户 DID 保持为两个独立认证主体，通过稳定 `controller_user_id` 绑定。
- 注册完成后 CLI 使用 Agent DID 私钥取得自己的 JWT，持久化本地身份并完成只读首检。
- 注册完成后 Agent 必须通过标准 direct message 主动发送一条固定、幂等、不含 Token 的消息，使 App 用户直接得到 IM 会话。
- Message Service 继续把该 DID 当作普通、独立的 IM principal；首期不增加专用消息协议。
- Token 不能登录人类账号、恢复已有身份、读取历史消息、代替 JWT 或直接发送消息。

## 2. 已确认的产品决策

### 2.1 确认结果

| 议题 | 确认结论 | 原因 |
|---|---|---|
| 新身份类型 | 确认新增 `agent_kind = skill` | `runtime` 必须由 daemon 托管，`daemon` 又有宿主机生命周期，二者都不准确。 |
| 用户绑定方式 | 写入 `agent_inventory.controller_user_id` | 保持人类身份和 Agent 身份的密钥、JWT、消息状态隔离。 |
| Agent Handle | User Service 签发时生成并固化 | 不让提示词或智能体自行选择并覆盖其他 Handle。 |
| Controller Handle | 放入提示词作为可见校验提示 | 真值仍来自 Token metadata，不能信任提示词文本。 |
| 自动确认语义 | 用户在 App 点击“复制安装指令”即授权标准安装、空 workspace 初始化和一次 claim | 满足自动注册；任何冲突、恢复、覆盖或可选步骤仍必须询问。 |
| 已有本地身份 | 确认始终 fail closed，不覆盖、不恢复、不静默切换 | 防止把第三方 Agent 安装到已有的人类或其他 Agent 身份上。 |
| Token 默认有效期 | 确认 30 分钟、一次性 | 给依赖下载留出时间，同时限制提示词泄露窗口。 |
| Token 输入 | CLI 优先从 stdin 读取 | 避免 Token 出现在 `ps` 参数和普通 shell history；提示词本身仍是敏感载体。 |
| 注册恢复 | v1 禁止 `allow_existing_agent_did` | Token 只授权新建，不应成为已有身份接管凭据。 |
| IM 能力 | 使用现有 DID/JWT/Message Service 路径 | 不为 Skill Agent 建第二套消息身份或私有消息 API。 |
| App 职责 | 只签发 Token、展示有效期和复制指令 | App 不展示、不轮询、不改名、不删除 Skill Agent，不复用 daemon 管理体验。 |
| 控制归属 | 服务端保存 Controller 与 Skill Agent 绑定 | “受我控制”是服务端授权关系，不等于 App 生命周期管理。 |
| 首次会话 | claim 后必须主动发送一条标准 direct message | 行为与 Daemon Agent 一致，让人类 App 无需管理页即可出现会话并联系 Agent。 |
| 国内外环境 | `awiki.ai` 与 `awiki.info` 完全独立 | 不做映射或跨域兑换；本计划只在国内 `awiki.info` 环境测试。 |
| 模型信任 | 接受 Token 进入当前智能体上下文 | 依靠短 TTL、一次性和最小 scope 控制风险，不宣称 Token 对模型提供商不可见。 |

### 2.2 明确不采用的方案

- 不把临时 Token 做成人类账号 JWT。
- 不把人类用户 JWT、refresh token、DID 私钥或手机号放进提示词。
- 不让 Token 直接代表人类 DID 调用 Message Service。
- 不把 Agent DID 的 `user_id` 直接改成人类用户的 `user_id`。
- 不复用 daemon installer 的 `--token` 实现文件；只复用服务端 Token 状态机和安全规则。
- 不把 Token 放进 onboarding URL query，避免 Web server、代理、Referer 和访问日志记录。
- 不允许兑换接口根据提示词里的 Handle、DID 或 service URL 改写 Token scope。
- 不在 v1 自动恢复、覆盖、导入或删除任何已有 CLI identity。
- 不从 App 提供 Skill Agent 列表、状态轮询、改名、删除或生命周期控制。
- 不把 `awiki.ai` Token 发送到 `awiki.info`，反向也不允许。

## 3. 当前系统事实

- App 已能用当前用户 JWT 调用 `/user-service/agent-registration/rpc` 签发 daemon/runtime Token。
- User Service 已有 `issue_token`、`verify_token`、`exchange_token`、`revoke_token`。
- Token 原文只返回一次，数据库仅保存 hash，并支持过期、撤销、一次消费和审计。
- daemon 现有流程已能完成 Token 预检、本地生成 DID、兑换、inventory 初始化和本地凭据保存。
- `agent_inventory` 已使用 `controller_user_id + controller_full_handle` 表达稳定归属。
- CLI 的普通 `id register` 是人类 Handle 的手机号/邮箱注册流程，不适合 Token onboarding。
- CLI identity 的生成、密钥保管、DID auth、JWT 刷新和 workspace 隔离应继续归 `im-core`。
- 当前 AWiki Skill 要求所有 identity write 再次确认；本功能需要增加一个严格受限的例外。
- 当前 onboarding 文档会安装 CLI、Skill、初始化 workspace、注册/恢复身份并执行首检。

## 4. 目标与非目标

### 4.1 产品目标

- 用户在 App 中最多经过一次明确操作即可复制完整安装指令。
- 智能体不再要求用户输入手机号、邮箱或 OTP 来创建这个 Agent 身份。
- 安装完成后，Agent 拥有独立 DID、Handle、JWT 和本地私钥，可以使用现有 AWiki IM。
- 注册成功后 Agent 必须向 Controller 发送固定的普通 direct message，用户从 App 标准 IM 会话进入沟通。
- 服务端能证明该 Skill Agent 由当前 `controller_user_id` 控制，但 App 不提供管理 UI。
- Token 过期、被撤销、已使用或 scope 不匹配时，智能体停止并提示重新复制。

### 4.2 工程目标

- 复用现有 registration token 表和原子 exchange，不建立平行认证体系。
- Token scope、Agent identity、Controller identity 和服务域名全部 fail closed。
- 注册网络重试不会创建多个 Agent DID 或多个 inventory row。
- CLI 输出、Debug、日志、遥测、错误和本地数据库不出现 Token 原文。
- release staging 后的 `onboarding.md` 和 Skill reference 保持同一契约。

### 4.3 非目标

- 不让 Skill Agent 自动获得 daemon、Runtime Agent 或 Personal Agent 控制权限。
- 除一条注册成功后必须发送的固定 Controller 消息外，不自动建立其他联系人关系或发送测试消息。
- 不自动读取用户历史消息或继承人类账号会话。
- 不支持一个 Token 创建多个 DID。
- 不支持 Token 续期；过期后必须回 App 重新签发。
- 不实现跨租户 Token 搬运。
- 不把这次工作与 EMAS Push provider 耦合。

## 5. 身份与所有权模型

### 5.1 三个身份概念

- `controller_user_id`：人类账号的稳定内部主键，是归属真值。
- `controller_did`：人类当前 DID 快照，用于签发上下文和审计，允许以后轮换。
- `skill_agent_did`：在 Agent 主机本地生成私钥的新 DID，是独立 IM principal。

### 5.2 服务端记录

- `did_documents.user_id` 指向新建的 Agent user，而不是人类 user。
- Agent user role 使用 `agent:skill`。
- `agent_inventory.agent_kind = skill`。
- `agent_inventory.controller_user_id` 指向人类账号。
- `agent_inventory.controller_full_handle` 保存签发时的完整 Handle 快照。
- `agent_inventory.controller_did` 保存当前 Controller DID 快照。
- `agent_inventory.daemon_agent_did = NULL`，明确表示它不由 daemon 托管。
- `created_by_token_id` 保留审计关联，但不保存 Token 原文或 hash 到 inventory。

### 5.3 “绑定用户的 IM”的定义

- 绑定表示服务端能验证该 Agent 的 Controller 是签发 Token 的稳定 `controller_user_id`。
- Agent 使用自己的 DID/JWT 收发消息，不冒充 Controller DID。
- claim 完成后 Agent 使用自己的 DID/JWT，经现有 Message Service direct message 路径主动给 Controller DID 发消息。
- 消息使用由 `token_id` 派生的稳定 message/idempotency key，重试不会产生多条会话消息。
- 消息是 App 正常展示的普通文本消息，不使用隐藏控制事件；建议固定正文为“AWiki Skill Agent 已完成注册，可以开始对话。”。
- App 只把该消息当作普通 IM；不从 inventory 展示或管理 Skill Agent。
- 人类账号和 Agent 账号不共享 JWT、DID 私钥、E2EE 状态或本地消息数据库。

## 6. Token 契约

### 6.1 Token 类型

- 建议前缀：`awsk1_`，便于识别但不承载明文 claims。
- secret 使用至少 256 bit CSPRNG 随机值。
- 服务端仅保存带服务端 pepper 的 hash；原文只在 issue response 返回一次。
- Token 是在线、可撤销 capability，不使用自包含 JWT。

### 6.2 服务端权威 scope

```json
{
  "purpose": "skill_onboarding_v1",
  "agent_kind": "skill",
  "tenant_id": "<server-owned>",
  "service_origin": "https://awiki.info",
  "controller_user_id": "<server-owned>",
  "controller_did": "<current-controller-did>",
  "controller_full_handle": "alice.awiki.info",
  "agent_handle": "skill-<random>.awiki.info",
  "one_time": true,
  "expires_at": "<absolute-time>"
}
```

- App 只提交当前 Controller DID/Handle 和非权威 UI metadata。
- User Service 根据认证上下文重新解析 `controller_user_id`、当前 DID 和完整 Handle。
- `service_origin` 由部署配置生成，不能由 App 或提示词覆盖。
- Agent Handle 由服务端生成、规范化并写入 Token record。
- metadata 只能保存 `client=awiki-me`、平台、版本等脱敏诊断信息。

### 6.3 Token 状态

```text
active -> used
active -> revoked
active -> expired
```

- `verify_token` 不消费 Token，只返回公开、脱敏 metadata。
- `exchange_token` 仅允许 `active + purpose=skill_onboarding_v1 + agent_kind=skill`。
- 首次成功后记录 `used_by_did` 和 DID Document digest。
- 同一 Token、同一 DID、同一 document digest 的短时重试可返回幂等结果。
- 已使用 Token 携带不同 DID 或 document digest 时永久拒绝。
- 幂等 replay 不返回长期 secret；CLI 后续通过 DID proof 获取 JWT。

## 7. App 侧体验

### 7.1 入口与边界

- 在 App 提供独立的“复制 AWiki Skill 安装指令”操作，与“安装 Daemon”区分。
- 点击后 App 调用现有 agent-registration RPC 的 `issue_token`。
- App 只展示绑定账号、Agent Handle、有效期和“复制安装指令”按钮。
- App 仅在内存中保留 raw Token；缓存和状态持久化只保存 `token_id`、过期时间和展示字段。
- App 不启动 inventory polling，也不为 Skill Agent 增加列表、详情、状态、改名或删除入口。
- Token 过期时用户可以重新生成；旧 Token 依靠 30 分钟 TTL 自动失效。

### 7.2 建议复制文本

```text
Read https://awiki.info/cli/onboarding.md and follow the instructions to install
AWiki CLI and Skill, initialize a new or empty workspace, then automatically
claim the one-time Skill Agent registration below and complete first-use checks.

AWIKI_SKILL_ONBOARDING_V1
service_base_url=https://awiki.info
token=<one-time-token>
controller_handle=alice.awiki.info
agent_handle=skill-xxxx.awiki.info
expires_at=2026-07-21T12:00:00Z
END_AWIKI_SKILL_ONBOARDING_V1

The token authorizes exactly one new Skill Agent DID. Do not print, persist,
send, or reuse it. Stop and ask me if the workspace already has a usable
identity, any field does not match verified token metadata, or any optional or
uncertain step is required.
```

- 国内环境固定使用 `https://awiki.info/cli/onboarding.md` 和 `https://awiki.info`。
- 海外环境独立使用 `awiki.ai` 自己的文档、服务和 Token，不与国内环境互相映射。
- App 使用当前部署环境生成 onboarding URL 和 `service_base_url`，两者必须属于同一环境。
- `controller_handle`、`agent_handle` 是用户可读提示，CLI 必须与 verify response 比较。
- 不放 `controller_did`、`controller_user_id`、人类 JWT 或其他账号 secret。

## 8. CLI 与 im-core 设计

### 8.1 命令面

- 新增 `awiki-cli onboarding claim`。
- 参数建议：`--service-base-url`、`--expected-controller-handle`、`--token-stdin`、`--format`。
- 官方路径不提供 `--token <value>`，避免 Token 出现在进程参数和普通 shell history。
- 支持从一次性环境变量读取作为兼容路径，但官方文档优先 stdin。
- 命令 schema、help 和 JSON 输出只显示 `token_present=true`，不回显 secret。

### 8.2 执行顺序

1. 解析可信 HTTPS `service_base_url`，禁用 Token 请求的跨 origin redirect。
2. 检查 workspace 已初始化，且不存在可用或冲突 identity；同一 claim journal 的恢复除外。
3. 调用 `verify_token`，校验 status、purpose、kind、expiry、origin 和可见 Handle。
4. 创建 `pending_skill_claim` journal，不写 Token 原文。
5. 通过 im-core 在本地生成 Agent DID、DID Document 和私钥。
6. 私钥先进入现有 secure identity storage；pending identity 不设为 default。
7. 调用 `exchange_token`，请求中的 Handle 和 Controller DID 取自 verify metadata。
8. 校验响应 DID、Handle、kind 和 controller scope 与本地计划完全一致。
9. 使用本地 DID 私钥调用现有 DID auth 路径取得/刷新 Agent JWT。
10. 原子标记 identity ready、清除 pending journal 并设为当前 Agent identity。
11. 写入非敏感 `controller_greeting_pending` journal。
12. 使用 Agent DID/JWT 和 `token_id` 派生的幂等 key，向 Controller DID 发送固定普通消息。
13. Message Service 接受后标记 `controller_greeting_sent`，再运行只读首检，不发送其他消息。

### 8.3 崩溃与重试

- 网络调用前必须持久化可恢复的 pending identity 和私钥。
- 重跑时复用同一个 pending DID，不能生成第二个 DID。
- exchange 成功但本地提交前崩溃时，依赖服务端同 DID/document digest 幂等 replay。
- Token 已被其他 DID 消费时，将本地 pending identity 标记为 failed，不设 default。
- 本地 final commit 失败时保留安全 journal，提示重试，不删除唯一私钥。
- 注册成功但主动消息失败时保留 identity 和 greeting journal，返回 retryable 状态并用同一 message ID 重试。
- 恢复同一 claim/greeting journal 时允许已有的本次 Skill identity；其他非空 workspace 仍始终 fail closed。
- 任何失败都不得回退到普通手机号/邮箱注册或 identity recovery。

### 8.4 Skill 授权规则

- 更新 `skills/SKILL.md` 和 `skills/references/01-onboarding.md`。
- 经 verify 成功的 `skill_onboarding_v1` Token 视为用户对“一次新建 Skill Agent DID”的明确授权。
- 该授权额外覆盖一条注册成功后必须发送的固定 Controller direct message，不覆盖任意消息发送。
- 该授权不覆盖已有身份、恢复、删除、Runtime 写配置或其他 identity write。
- workspace 非空、Token metadata 冲突或服务域不可信时必须停下并询问用户。
- 无 Token 时继续使用现有“身份写操作必须确认”的规则。

## 9. User Service 设计

- 扩展 `AgentKind`：`daemon | runtime | skill`。
- `issue_token` 对 `skill` 强制当前 principal、当前 Controller DID 和 active Handle。
- `skill` Token 不接受 daemon DID、runtime driver、已有 Agent DID 或 recovery scope。
- issue 时生成唯一 Agent Handle，并在 active Token 范围防止重复。
- `verify_token` 返回 `purpose`、service origin、Controller Handle、Agent Handle 和过期时间。
- `exchange_token` 验证 did:wba domain、DID Document key、Handle service 和 Token Handle。
- `allow_existing_agent_did=true` 对 `skill` 固定拒绝。
- 单事务完成 Token 消费、Agent user、DID Document、Handle 和 inventory 创建。
- 审计事件继续使用 issued/verified/exchanged/revoked/failed，并记录 `purpose=skill_onboarding_v1`。
- 服务端保留 inventory 归档和 DID/Handle 安全处置能力，但首期不向 App 暴露。
- 上述能力只作为服务端运维和安全事件响应边界，不复用 daemon 的 App 生命周期管理体验。

## 10. Message Service 设计

- 预计不新增生产 API；Skill Agent 使用现有 DID auth、JWT、direct/group/sync/realtime。
- 增加兼容测试，确认 `role=agent:skill` 不被错误识别为 service、daemon 或 human controller。
- 确认 Skill Agent 只能读取自己的 inbox/history/sync，不能读取 Controller 的消息。
- 确认首次消息是 Agent DID/JWT 发送的标准 direct message，App 按普通消息展示 sender DID/Handle。
- 确认该消息按 `token_id` 幂等，且只发给 Token scope 中的 Controller DID。
- 若当前授权代码只允许 `agent:daemon`/`agent:runtime`，只扩展角色 allowlist，不增加旁路。

## 11. 安全模型

### 11.1 信任边界

- 点击复制的用户信任当前智能体和其模型提供商在 Token 有效期内持有该 capability。
- 若不能信任模型提供商，不应使用“Token 进入提示词”模式；v1 不声称消除该风险。
- onboarding 文档是远程代码执行指引，只允许来自 App 环境配置的官方 release origin。
- Token 只能发送到 scope 中绑定的 User Service origin。

### 11.2 主要威胁与控制

| 威胁 | 控制 |
|---|---|
| 提示词/剪贴板泄露 | 30 分钟 TTL、一次性、最小 scope、可撤销、App 显示过期时间。 |
| Token 被抢先兑换 | 审计记录 IP/User-Agent 和绑定结果；Agent 仍绑定签发用户；安全处置走服务端撤销/归档而非 App UI。 |
| Token 被当成人类登录凭据 | 独立 token type/purpose；人类 auth、recovery 和 Message Service 拒绝。 |
| Token 发往错误域名 | scope 绑定 origin；CLI 禁止跨 origin redirect；提示字段与 verify metadata 比较。 |
| Handle/DID 参数篡改 | Token record 是真值；exchange 对全部 scope 字段做精确校验。 |
| 覆盖本地身份 | 非空 workspace fail closed；pending identity 不自动覆盖 default。 |
| 重试创建多个 DID | 本地 journal + 服务端同 DID/document digest 幂等。 |
| 日志泄露 | secret wrapper 手写 Debug；输出、错误、HTTP tracing、analytics 全部脱敏。 |
| Agent 越权读取 Controller IM | 独立 Agent user/JWT/DID；Message Service owner scope 测试。 |
| 恶意 onboarding 内容扩大授权 | Skill 只承认固定命令和固定 purpose；额外写操作仍需确认。 |

### 11.3 速率与审计

- 每个 Controller 用户限制 active Skill Token 数量和单位时间签发次数。
- 每个 IP 限制 verify/exchange 失败次数，但不能泄露 Token 是否存在。
- 服务端保留按 `token_id` 撤销 active Token 的运维能力，App 首期不提供撤销入口。
- 审计和 metrics 只记录 token_id、状态、kind、结果和脱敏原因。
- 禁止在 Sentry、OpenTelemetry span、HTTP body log 和 CLI crash report 中记录 raw Token。

## 12. 跨仓库分支与职责

| 模块 | Worktree | 分支 | 当前基线/提交 | 计划职责 |
|---|---|---|---|---|
| AWiki CLI | `/home/ecs-user/awiki-space/awiki-cli-rs2-skill-token-onboarding` | `feature/skill-token-onboarding` | `911fc51d` | claim 命令、im-core identity transaction、Skill/onboarding 文档、CLI 测试和 stable 发布。 |
| Android/App | `/home/ecs-user/awiki-space/awiki-me-emas-android` | `feature/aliyun-emas-android` | `bb96617` | Token 签发、有效期展示、复制提示词和 App 测试；不管理 Skill Agent。 |
| User Service | `/home/ecs-user/awiki-space/user-service-emas-push` | `feature/emas-push-user-service` | `57c63ec` | `skill` Token scope、原子 exchange、inventory 归属、审计和测试。 |
| Message Service | `/home/ecs-user/awiki-space/message-service-emas-push` | `feature/emas-push-message-service` | `2deba55` | `agent:skill` 鉴权隔离契约测试；现有生产授权无需 Skill 专用分支。 |

- 实现前分别同步最新 `origin/release/0714`，处理现有 feature commit 与新基线的冲突。
- 不把四个仓库压成一个提交；每个仓库保持可独立 Review 和回滚。
- AWiki CLI 文档契约先冻结，再并行实现 User Service 和 CLI，之后接 App。

## 13. 分步实施计划

### [步骤 01：冻结契约和威胁模型](steps/01-contract-and-threat-model.md)

- 状态：`completed`。
- 将第 2 节已确认决策固化到协议，禁止实现阶段重新解释。
- 冻结 Token scope、prompt block、RPC shape、幂等语义和稳定错误码。
- 为 User Service、CLI、App 和 Message Service 分配协议 owner。
- 产出 API 示例和错误矩阵后再写代码。

### [步骤 02：扩展 User Service Token 和 inventory](steps/02-user-service-skill-token.md)

- 状态：`completed`；依赖步骤 01。
- 增加 `skill` kind、purpose scope、server origin 和 Agent Handle 生成。
- 实现一次性 exchange、同 DID/document digest 幂等和跨 DID 拒绝。
- 原子创建 Agent user、DID、Handle、inventory 和审计。
- 增加 revoke、expiry、rate limit 和敏感字段拒绝测试。

### [步骤 03：实现 im-core Token claim transaction](steps/03-im-core-skill-claim.md)

- 状态：`completed`；依赖步骤 01，可与步骤 02 并行开发 mock contract。
- 增加 redacted Token 类型、verify/exchange client 和 typed request/response。
- 复用 identity vault、DID Document builder、JWT refresh 和 owner-scoped storage。
- 增加 pending journal、崩溃恢复和 final commit。
- 不向 Dart public API 暴露 raw Token。

### [步骤 04：增加 CLI 命令和 onboarding/Skill 契约](steps/04-cli-onboarding-and-skill.md)

- 状态：`completed`；依赖步骤 03。
- 增加 `onboarding claim` command catalog、schema、help、同步/异步 handler。
- 支持 stdin secret、稳定 JSON 输出和脱敏错误。
- 更新 `onboarding.md`、`skills/SKILL.md`、installation/onboarding reference。
- 更新 release staging tests，保证发布文档没有未替换 placeholder。

### [步骤 05：增加 App 复制安装指令](steps/05-awiki-me-copy-instruction.md)

- 状态：`completed`；依赖步骤 02 和步骤 04 契约稳定。
- 扩展 App 的 User Service adapter，只签发 Skill Token。
- 增加独立 UI 入口、确认信息、复制按钮、过期和重新生成状态。
- raw Token 只存在于当前内存对象和用户主动复制的剪贴板内容。
- 不增加 inventory polling、Skill Agent 列表、详情、状态、改名或删除能力。

### [步骤 06：验证 Message Service IM 隔离](steps/06-message-service-im-isolation.md)

- 状态：`completed`；依赖步骤 02。
- 添加 `agent:skill` auth、direct、history、sync 和跨 owner 拒绝测试。
- 验证 Controller 和 Skill Agent 可以通过标准 direct message 往返。
- 验证注册后必须主动发送的固定消息只到达 Controller，重复 claim 不产生重复消息。
- 只有测试证明现有角色检查阻断时才做最小生产改动。

### [步骤 07：跨仓库 E2E 和发布门禁](steps/07-cross-repo-e2e-rollout.md)

- 状态：`in_progress`；依赖步骤 02-06；AWiki Me `full` 已通过，remote suite 为 `255 passed, 2 failed, 51 skipped`。
- E2E：App 签发 -> 解析复制 prompt -> CLI 安装后 claim -> Agent 主动消息到达 App -> 双向 IM。
- 覆盖过期、撤销、重复兑换、Token 抢占、错误域名和非空 workspace。
- 在 `../awiki-system-test` 使用 remote `awiki.info` 完整系统测试并记录数量和原因。
- 灰度开关按 tenant/user 控制 App 入口和 `skill` Token 签发。

## 14. 验证矩阵

- 步骤 01-06 只运行与当前改动直接相关的定向测试、格式和静态检查，不运行仓库全量套件。
- 步骤 07 在四仓实现完成后统一执行各仓全量测试、AWiki Me `full` E2E 和国内 remote system test。

### 14.1 User Service

- issue 只允许当前 authenticated Controller scope。
- 数据库不保存 raw Token，日志和 audit 不含 raw Token。
- verify 对 invalid/expired/revoked/used 返回稳定、不可枚举的错误。
- exchange 拒绝 wrong kind、purpose、origin、Handle、Controller、DID domain 和 document。
- 并发 exchange 只有一个 DID 成功。
- 同 DID/document digest 重试幂等，不同 DID 永久拒绝。
- 事务失败不消费 Token，不留下半个 User/DID/Handle/inventory。

### 14.2 CLI/im-core

- `schema onboarding claim` 不暴露 Token value。
- Token 的 `Debug`、错误、JSON 输出、HTTP trace 全部脱敏。
- verify 发生在本地 key 生成前；scope 冲突不写本地状态。
- exchange 前 key 已安全落入 pending identity，可在 crash 后恢复。
- 成功后 identity 有 DID Document、私钥、JWT、Handle 和 ready 状态。
- 非空 workspace、used-by-other、跨 origin redirect 和 response mismatch fail closed。
- 重试复用同一 DID；不会生成多个 default identity。

### 14.3 App

- issue 请求使用当前 DID/Handle，不能由 UI 注入 controller user id。
- copied prompt 包含正确环境、Token、Handle 和过期时间。
- copied prompt 不含人类 JWT、Controller DID、内部 user id 或手机号。
- Token 不写入 cache、analytics、debug state 或 crash report。
- 过期后复制按钮失效并允许重新生成。
- App 没有 Skill Agent inventory、轮询、详情、改名和删除入口。

### 14.4 Message Service

- Skill Agent JWT 只能访问自己的 direct/group/inbox/history/sync。
- Controller JWT 不能以 Skill Agent DID 冒充发送。
- Skill Agent 不因 `agent:skill` role 获得 internal service 或 daemon 权限。
- 标准 direct message 能在 Controller 与 Skill Agent 间往返。
- 首次主动消息是普通 direct message，内容固定且不含 Token，目标来自服务端 scope，重复执行保持幂等。

### 14.5 系统测试

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test --show-command
```

## 15. 稳定错误码

| 错误码 | 含义 | 智能体行为 |
|---|---|---|
| `skill_onboarding_token_invalid` | Token 不存在或格式错误 | 停止，请用户重新复制。 |
| `skill_onboarding_token_expired` | Token 已过期 | 停止，请用户在 App 重新生成。 |
| `skill_onboarding_token_revoked` | Token 已撤销 | 停止，不重试。 |
| `skill_onboarding_token_used` | 已被其他 DID 使用 | 停止；提示用户检查是否收到 ready 会话，否则重新生成并联系服务端排查。 |
| `skill_onboarding_scope_mismatch` | kind/purpose/Handle/origin 不一致 | 停止并报告脱敏差异。 |
| `skill_onboarding_workspace_conflict` | workspace 已有可用身份 | 停止并询问，不覆盖。 |
| `skill_onboarding_response_mismatch` | 服务端响应与本地 DID/scope 不一致 | 保留 pending journal，停止。 |
| `skill_onboarding_local_commit_failed` | 远端成功但本地提交失败 | 使用同一 pending DID 重试，不生成新 DID。 |
| `skill_onboarding_greeting_pending` | 账号成功但主动消息暂未送达 | 保留已注册身份，用同一 message ID 重试，不重新兑换 Token。 |

## 16. 发布与回滚

- User Service 先部署 schema/API，但默认关闭 `skill` issue。
- Message Service 先部署兼容角色检查。
- CLI/Skill release 发布支持 claim 的版本，并验证 stable onboarding snapshot。
- App 最后开启复制入口，确保不会签发给旧 CLI 无法消费的 Token。
- 灰度期间记录 issue、verify、exchange、success、expiry、revoke 和 conflict 比例。
- 回滚 App 时关闭 Token 签发和复制入口；已签发 Token 按原 30 分钟 TTL 自然失效。
- 回滚 CLI 不影响已注册 Agent；现有 DID/JWT 继续走普通 IM。
- 回滚 User Service issue 开关时保留 exchange grace，避免已复制 Token 瞬间失效。

## 17. 完成标准

- 用户从 App 复制一次提示词后，智能体可以完成标准安装和 Agent DID 注册。
- 用户无需向智能体提供手机号、邮箱、OTP、人类 JWT 或私钥。
- Agent DID 在服务端稳定绑定到正确 `controller_user_id`，但不进入 App Agent 管理体验。
- Agent 使用独立 JWT/私钥收发自己的 IM，不能读取 Controller 私有状态。
- 注册成功后 Skill Agent 必须主动发送消息，Controller 在 App 中收到唯一普通消息并可直接对话。
- 一次 Token 不能创建第二个 DID，不能恢复已有 DID，不能跨租户使用。
- Token 不出现在服务端数据库明文、CLI 本地状态、日志、遥测和错误输出中。
- 非空 workspace、scope 冲突和不可信 origin 全部 fail closed。
- 四仓库聚焦测试、跨仓库 E2E 和 remote AWiki system test 均有可复核记录。
- 每个仓库独立提交、独立 Review、可独立回滚。

## 18. 已确认决策记录

- 2026-07-21：采用 `agent_kind=skill`。
- 2026-07-21：Token 默认有效期为 30 分钟。
- 2026-07-21：非空 workspace 始终 fail closed。
- 2026-07-21：`awiki.ai` 与 `awiki.info` 是独立海外/国内服务，不做域名映射。
- 2026-07-21：开发和系统测试以国内 `awiki.info` 环境为准。
- 2026-07-21：App 只负责产生 Token 和复制安装指令，不直接管理 Skill Agent。
- 2026-07-21：Skill Agent 通过服务端 Controller 绑定表达“由用户控制”。
- 2026-07-21：接受短时 Token 进入当前智能体上下文的风险边界。
- 2026-07-21：Skill Agent 账号成功后必须像 Daemon Agent 一样主动给人类 App 发送标准 IM 消息。
