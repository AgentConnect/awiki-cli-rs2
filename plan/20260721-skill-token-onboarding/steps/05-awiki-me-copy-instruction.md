# 步骤 05：在 AWiki Me 签发 Token 并复制安装指令

状态：`completed`  
实施仓库：`awiki-me`  
Worktree：`/home/ecs-user/awiki-space/awiki-me-emas-android`  
实施分支：`feature/aliyun-emas-android`  
前置依赖：步骤 02、04 的 RPC 和 prompt 契约稳定  
后续依赖方：步骤 07

## 1. 目标

- 让已登录且有 active Handle 的用户生成一个 Skill Agent onboarding Token。
- 生成一段完整、可复制给智能体的国内安装提示词。
- 清楚展示绑定账号、Agent Handle 和 30 分钟过期时间。
- 保持 App 只是授权入口，不引入 Skill Agent 生命周期管理。

## 2. 不做的内容

- 不展示 Skill Agent inventory、状态、在线情况或详情。
- 不轮询 Token 是否已兑换。
- 不提供 Skill Agent 改名、删除、归档、撤销 DID 或 Handle。
- 不接收 Agent 主动消息以外的专用回调。
- 不修改 daemon/runtime Agent 管理逻辑。
- 不修改 EMAS Push provider；主动消息按现有 IM/Push 路径展示。

## 3. 开始前检查

- 将 feature 分支同步到最新 `origin/release/0714`，保留 EMAS commit。
- 阅读现有 `UserServiceAgentInventoryAdapter`、authenticated RPC client 和 daemon install command。
- 确认当前 session 提供 DID、完整 Handle、bearer token 和国内环境配置。
- 确认 `awiki.info/cli/onboarding.md` 已随支持 claim 的 CLI release 发布。

## 4. Data/port

- 在现有 User Service adapter 增加最小 `issueSkillToken` 方法。
- 不把该方法加入 daemon/runtime inventory polling 状态机。
- 请求只提交：

```text
agent_kind = skill
controller_did = current session DID
controller_handle = current active full Handle
metadata.client = awiki-me
metadata.client_platform
metadata.onboarding_version = 1
```

- App 不提交 controller_user_id、Agent Handle、service origin 或自定义 TTL。
- response DTO 只保留 raw Token、token_id、Controller Handle、Agent Handle、service origin、expires_at。
- DTO 手写/覆盖 Debug 或诊断输出，raw Token 必须脱敏。

## 5. Prompt builder

- 使用纯函数从 typed response 生成复制文本，便于精确 contract test。
- 国内文本固定使用 User Service 返回并校验为 `https://awiki.info` 的 origin。
- onboarding URL 固定为 `https://awiki.info/cli/onboarding.md`。
- 输出 `AWIKI_SKILL_ONBOARDING_V1` block。
- 不包含 Controller DID、内部 user ID、人类 JWT、手机号、邮箱或 App 设备信息。
- raw Token 只在最终复制文本中出现一次。
- 不生成把 Token 放在 URL query 或 shell `--token` 参数中的命令。

## 6. UI 行为

- 提供独立“复制 AWiki Skill 安装指令”入口，与 daemon 安装入口文案区分。
- 未登录、无 active Handle 或 tenant 不支持时禁用并显示现有风格错误。
- 点击后显示 loading，成功后展示：

```text
Controller Handle
Agent Handle
有效期至
复制安装指令
重新生成
```

- 点击复制是用户对 v1 标准流程的明确授权。
- raw Token 只放在当前内存 state 和系统剪贴板。
- 页面离开、登出、tenant 切换或 Token 过期时清空内存 state。
- 重新生成只签发一个新 Token；旧 Token 按原 TTL 失效，不增加 App 撤销入口。

## 7. App 不管理 Skill Agent

- 不启动 inventory auto-sync。
- 不向现有 daemon/runtime Agent list 插入 skill row。
- 不新增 Skill Agent detail route。
- 不根据 Token exchange 状态改变 App Agent 管理 UI。
- 注册成功后的固定主动消息按普通 direct message 进入正常会话列表。
- 用户从该会话直接与 Skill Agent 沟通。

## 8. 安全处理

- 禁止在 logger、analytics、Crashlytics/Sentry、Riverpod debug state 中输出 raw Token。
- HTTP error 不附带 request params/body。
- clipboard 内容包含 secret，UI 显示 30 分钟有效期和不要转发提示。
- service origin 非 `awiki.info` 时国内构建 fail closed。
- Controller Handle 必须来自当前 session，不能由文本框输入覆盖。

## 9. 测试

### 9.1 Adapter

- authenticated RPC path、method 和 `agent_kind=skill` 正确。
- 请求包含当前 DID/Handle，不包含 controller_user_id 和 TTL。
- response 正确解析，Debug 不含 Token。
- unauthorized、no handle、unsupported tenant 映射稳定错误。

### 9.2 Prompt

- 文本与步骤 01 冻结示例一致。
- 国内 URL 只出现 `awiki.info`，不出现 `awiki.ai`。
- Token 恰好出现一次。
- 不含 Controller DID、user ID、JWT、手机号和 URL query token。

### 9.3 UI

- loading、成功、过期、复制和重新生成状态正确。
- 页面离开/登出/tenant 切换清除 secret state。
- 没有 inventory polling、Skill detail、改名和删除入口。
- daemon install UI 和现有 EMAS 功能不回归。

## 10. 完成标准

- 国内用户能在 App 中生成并复制可用的 Skill onboarding 提示词。
- App 只承担签发和复制，不管理注册后的 Skill Agent。
- Token 不进入持久化 App 状态或日志。
- 过期和 tenant/session 边界 fail closed。
- 聚焦单测、相关 widget 测试、静态检查和 `git diff --check` 通过。

## 11. 实施结果与验证

- 新增独立 `SkillOnboardingPort`、User Service adapter、typed grant/instruction 和内存 Riverpod controller，没有接入 daemon inventory 状态机。
- App 仅向 `issue_token` 提交当前 session DID/Handle、`agent_kind=skill`、`one_time=true` 和 allowlist metadata；不提交 user ID、Agent Handle 或 TTL。
- prompt builder 严格校验国内 `https://awiki.info`、Controller/Agent Handle 和过期时间，输出步骤 01 冻结 block，Token 只出现一次。
- Agents 页增加独立 Skill 安装指令入口、临时弹窗、复制和重新生成；页面离开、session 变化和过期会清理内存状态及计时器。
- UI 没有增加 Skill Agent 列表、详情、改名、删除或 exchange 轮询，也不会触发 daemon 安装。
- 增加 App Shell 定向 E2E，用 mock Token 验证从已认证 App 进入 Agents、复制国内 prompt，并确认不触发 daemon 安装。

步骤 05 定向验证：

```text
flutter test --no-pub tests/unit/agents/skill_onboarding_test.dart tests/unit/data/agent/user_service_skill_onboarding_adapter_test.dart
5 passed, 0 failed

flutter test --no-pub tests/unit/agents/agents_page_layout_test.dart --name 'skill onboarding copies a scoped prompt without managing inventory'
1 passed, 0 failed

flutter test --no-pub tests/e2e/flutter/app/app_smoke_test.dart --name 'AwikiMeApp copies domestic Skill Agent onboarding prompt'
1 passed, 0 failed

本次涉及的 application/domain/data/presentation/test 文件 dart analyze
No issues found

dart format（本次涉及文件）
git diff --check
passed
```

定向 E2E 通过时 Flutter 提示直接运行源文件未检测到 `integration_test` plugin；该提示不影响本地 Widget 结果。真实平台 E2E 仍由步骤 07 的标准 runner 执行。本步骤未执行 App 全量单测、全量 analyze、smoke 套件或 `full` E2E，统一留到步骤 07。
