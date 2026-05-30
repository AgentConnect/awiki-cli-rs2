# 步骤 03：user-service registration token API

主计划：[../plan.md](../plan.md)
步骤编号：03
状态：草稿

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待开始 |
| 分支 | user-service 实现分支 |
| 开始时间 | 待定 |
| 完成时间 | 待定 |
| 提交 | 待定 |
| 审查证据 | 待定 |
| 验证证据 | 待定 |
| 下一步 | 设计并实现 registration token API。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：user-service 提供带作用域、可过期、可撤销的 daemon agent 与 runtime agent DID 注册 token。
- 可见行为：App/Mac 可申请 token；daemon 可用 token 注册 daemon/runtime agent DID 并绑定 `controller_did`；过期、已使用、已撤销、作用域不匹配等失败有明确错误。
- 非目标：不在 user-service 中实现 daemon 本地 `runtime_rpc_token`；该能力属于步骤 05。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `user-service/SPEC.md` | 增加 registration token 能力和安全规则。 | 作为权威说明。 |
| `user-service/docs/api/` | 增加 token 申请、兑换、验证、撤销 API 文档。 | REST 或 JSON-RPC 路由名在此冻结。 |
| `user-service/docs/database-design.md` | 增加 token 表或扩展现有 token storage。 | 只存 token hash，不存原文。 |
| `user-service/src/user_service/app/` | 增加 API route 和 schema。 | FastAPI 层。 |
| `user-service/src/user_service/services/` | 增加 token service 或扩展现有 TokenService。 | 生成、hash、过期、一次性使用。 |
| `user-service/src/user_service/storage/` | 增加 storage 抽象和 SQLModel 实现。 | 如改 schema，需要 migration。 |
| `user-service/tests/` | 增加 API、service、storage、安全测试。 | 包含过期和撤销。 |
| `crates/im-core/src/identity/` | 如需要，后续可增加 token exchange client。 | 只有当前步骤明确需要时才实现。 |

## 4. 依赖

- 前置步骤：无强依赖。
- 外部决策：DID/Handle 注册流程和 daemon 架构文档。
- 环境前提：user-service checkout、Python 3.13+、MySQL/test dependencies 可用。

## 5. 核心设计

Registration token 是服务端签发的 DID/Handle 创建授权凭证：

| token 类型 | 生成方 | 使用方 | 用途 |
|---|---|---|---|
| `daemon_registration_token` | user-service | daemon installer / daemon setup | 注册 daemon agent DID/handle 并绑定 owner/controller。 |
| `runtime_agent_registration_token` | user-service 或 daemon 经 user-service 申请 | daemon | 注册 runtime agent DID/handle。 |

规则：

- DB 只存 token hash 和元数据。
- token 必须有 `token_id`。
- token 必须有作用域、issued actor、agent kind、可选 handle、`controller_did`、`expires_at`、`used_at`、`revoked_at`。
- 注册类 token 默认一次性使用，除非明确标记例外。
- token exchange 必须具备事务原子性，避免并发重复使用。
- audit 只记录 `token_id`、actor、作用域、result、reason，不记录 token 原文。
- 失败码必须区分 expired、revoked、used、invalid、scope mismatch、permission denied。

## 6. 实施指引

1. 先冻结 API：
   - 申请 token。
   - 兑换 token 完成注册。
   - 如需要，验证 token。
   - 撤销 token。
2. 按 user-service 现有风格决定 REST 或 JSON-RPC。
3. 增加存储模型，存 hashed token secret 和生命周期字段。
4. 用安全随机数生成 token。
5. 验证时使用安全比较方式。
6. 兑换过程放在事务中，保证一次性 token 原子失效。
7. 将 token 作用域与 DID/Handle registration request 绑定。
8. 增加成功路径和所有失败模式测试。
9. 只有被 daemon 立即消费时，才补 `im-core` 或 daemon 侧 client。

## 7. 验收标准

- [ ] user-service 文档定义 daemon/runtime registration token 流程。
- [ ] token 原文不入库、不进日志。
- [ ] API 只在创建时返回 token 原文。
- [ ] 一次性 token 的 exchange 原子。
- [ ] 过期和撤销被强制执行。
- [ ] audit 只记录 `token_id`。
- [ ] 测试覆盖成功、过期、已撤销、已使用、无效、作用域不匹配。
- [ ] 审查发现已修复或明确记录。
- [ ] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| Python 测试 | `cd ../user-service && uv run pytest tests -v` | token API/storage/service 测试通过。 |
| API 文档 | `git diff --check -- SPEC.md docs` | 文档 diff 干净。 |
| secret 搜索 | 检查日志、测试和 token 相关代码 | token 原文不被记录或持久化。 |
| DB migration | 按 user-service 文档运行 migration/test setup | token 表或 schema 正确应用。 |
| 安全审查 | 手工审查 token hashing、过期、撤销和 audit | 发现已记录并处理。 |

## 9. 审查过程

实现后、提交前进行审查，重点检查 API 契约、授权、随机数、hash、过期、撤销、audit、日志、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待定 | 待定 |
| 已修复 | 待定 | 待定 |
| 残余风险 | 待定 | 待定 |
| 测试缺口 | 待定 | 待定 |
| 文档缺口 | 待定 | 待定 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：user-service token API、storage、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`user-service: add agent registration token api`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：token API 可能与现有 Handle token 机制重叠；token 原文泄漏风险高。
- 回滚：暂停 registration token issuance，保留手工本地 daemon agent setup。
- 后续：步骤 07 daemon agent 创建必须消费该 API，不能另造 local-only 注册机制。
