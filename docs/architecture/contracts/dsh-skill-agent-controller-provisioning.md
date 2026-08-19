# DSH Skill Agent Controller Provisioning 安全契约

跨仓导航见 [Harness DSH Multi-Agent Identity](../../../../awiki-harness/features/dsh-multi-agent-identity.md)。

## 目标与信任边界

可信 Node Host 可在用户批准一次 DSH 工具调用后，用当前主 Human DID 为一个额外的
`agent_kind=skill` 身份签发并立即消费一次性 registration token。该能力只进入 Rust
`im-core` / Node Host，不进入 Browser Remote、模型参数、工具结果或外部 HTTP auth。

## 强制规则

1. Controller 必须是本地 ready identity；User Service 继续权威校验它是 Human DID。
2. 签发前必须检查同源 `/user-service/server-info` 的 `enabled=true`、协议版本 1、固定
   onboarding path 和 `display_name_binding=token_scope_v1`。
3. `issue_token` 使用 canonical `/user-service/v1/agent-registration/rpc` 和 Controller 的
   authenticated transport；exchange 使用既有 anonymous、no-redirect Token transport。
4. issue response 必须回显相同 Controller DID/Handle、display name、Skill purpose、一次性
   状态、同源 service origin 和服务端 Handle；任一不一致 fail closed。
5. DSH provisioning 不声明 `group_membership_v1`，只交付普通 Direct 能力。
6. raw token 只在 Rust `Zeroizing` 内存或 SecretVault pending record 中存在。普通文件、
   SQLite、JS DTO、错误、Debug、日志和浏览器均不得包含它。
7. `operation_id` 只关联 per-operation journal/pending；Core identity alias 仍使用服务端
   Handle local part，新身份保存时固定 `make_default=false`。
8. exchange 已提交但本地保存中断时，必须复用同一 token、DID 和 Document exact replay。
   Host binding 原子提交后调用 acknowledge，Core 才删除 pending token；非敏感完成 journal
   保留以支持幂等结果。
9. `issue_token` 成功后、SecretVault pending 尚未持久化前的可捕获本地失败，必须由同一个
   authenticated Controller transport 调用 `revoke_token` 补偿；revoke 响应必须严格绑定
   token id/status。补偿失败只输出稳定、可重试的 cleanup code。进程在 issue 响应与首次 Vault
   写入之间被强杀是协议无法完全消除的极小窗口，只能由服务端 30 分钟 TTL 最终回收，不能将其
   描述为已被 revoke 彻底解决。
10. 限流、capability 和 scope 失败只输出稳定 code；服务端 message/data 不得越过 Node safe
   error 边界。

## 审批事实

Core 不自行推断用户授权。调用 Host 必须在执行 `provisionSkillAgentIdentity` 前完成一次可审计
的用户审批，明确显示 display name、目标 preset/session route 和“创建永久远端 DID”。重试同一
`operation_id` 不重复申请授权或创建第二个 DID；新 operation 必须重新审批。

## Review 门禁

- token/私钥的序列化、Debug、错误和崩溃恢复路径均有负向测试；
- 两个 operation 的 journal/Vault key 不冲突；
- default identity 在创建前后不变；
- capability 缺失、Agent Controller、scope 回显不一致、限流和响应丢失均 fail closed；
- issue 后的 unstaged 本地失败会 revoke；revoke 失败返回闭合 cleanup code 且不泄漏服务正文；
- Node addon、TypeScript wrapper 和所有平台包使用同一 Native API 版本。
