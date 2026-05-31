# 步骤 06：user-service registration token API

主计划：[../plan.md](../plan.md)
步骤编号：06
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/daemon-registration-token-user-service` |
| 开始时间 | 2026-05-31 03:33:21 CST |
| 完成时间 | 2026-05-31 04:22:36 CST |
| 提交 | `4087e51`（user-service，`user-service: add agent registration token api`） |
| 审查证据 | Review 已完成：API 契约、token hash 存储、一次性兑换、过期/撤销、scope mismatch、audit 隐私、DID/User 原子创建、文档一致性和测试覆盖已审查；发现 `one_time=false` 暴露了首版不支持的复用语义、过期 token 可被撤销为 revoked、测试文件名会加重仓库既有 pytest 顶层模块名冲突，均已修复。 |
| 验证证据 | `uv run ruff format ...` 通过；`uv run ruff check ...` 通过；`uv run python -m py_compile ...` 通过；`uv run python -m pytest tests/app/agent_registration -v` 通过，9 passed；`git diff --check -- SPEC.md docs src tests` 通过；secret/audit 搜索确认生产代码无 registration token 原文日志，audit 只记录 `token_id` 和 scope 元数据。全量测试见第 8 节说明。 |
| 下一步 | 执行阶段 B Review。 |

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

### 6.1 本步骤冻结的局部方案

本步骤采用 user-service 现有 JSON-RPC 风格，新增端点：

```text
POST /user-service/agent-registration/rpc
```

方法：

| 方法 | 认证 | 用途 |
|---|---|---|
| `issue_token` | `user` | App/Mac 或已登录用户为 daemon/runtime agent 注册签发短期 token。 |
| `verify_token` | `none` | daemon setup 预检 token 状态和 scope，不消费 token，不返回 token hash。 |
| `exchange_token` | `none` | daemon 使用 token 完成 DID 文档注册；默认一次性消费。 |
| `revoke_token` | `user` | 签发者或同一用户撤销未使用 token。 |

新增专用表 `agent_registration_tokens`，不复用 legacy `auth_tokens`：

- `id`：内部记录 ID。
- `token_id`：对外审计与 API 使用的稳定 ID。
- `token_hash`：服务端 secret keyed hash，唯一索引，不存 token 原文。
- `issued_to_user_id`：签发用户。
- `issued_by_did`：可选，签发来源 DID。
- `agent_kind`：`daemon` 或 `runtime`。
- `handle`：可选，期望注册的 handle local-part 或展示 handle。
- `controller_did`：注册完成后绑定的 controller。
- `expires_at`、`used_at`、`revoked_at`。
- `used_by_did`：兑换成功的 DID。
- `scope`：JSON，保留 `issued_actor`、`agent_kind`、`handle`、`controller_did` 和后续扩展字段。
- `created_at`、`updated_at`。

token 状态机：

```text
active -> used
active -> revoked
active -> expired（由 expires_at 判断，不需要单独状态列）
used/revoked/expired 不允许再兑换
```

错误原因固定为：

- `invalid`
- `expired`
- `revoked`
- `used`
- `scope_mismatch`
- `permission_denied`

审计事件写入 `event_history`，事件类型：

- `agent_registration_token.issued`
- `agent_registration_token.verified`
- `agent_registration_token.exchanged`
- `agent_registration_token.revoked`
- `agent_registration_token.failed`

审计上下文只记录 `token_id`、actor、scope、result、reason、agent_kind、handle、controller_did、did，不记录 token 原文或 token hash。

`exchange_token` 第一版直接创建 DID 文档记录，并按 scope 约束：

- `params.token` 是唯一可信 token 输入。
- `params.did_document.id` 必须等于注册 DID。
- `agent_kind` 必须匹配 token scope。
- 如果 token 绑定 `handle`，请求中 handle 必须一致或省略。
- 如果 token 绑定 `controller_did`，请求中的 `controller_did` 必须一致。
- 注册写入 `did_documents.is_agent = true`。
- `role` 使用 `agent:daemon` 或 `agent:runtime`。
- `endpoint_url`、`name`、`avatar` 作为可选元数据；不在本步骤设计 daemon 本地 runtime_rpc_token。

事务要求：

- `exchange_token` 必须在一次数据库事务中完成 token 状态检查、DID 文档重复检查、User 创建、DID 文档创建、token `used_at` 标记和审计记录。
- 兑换实现需要用条件更新保证一次性 token 只有一个并发请求能成功。
- 如果后续发现现有 `SQLModelStorage` 的通用接口无法表达事务，应在 registration token repository 中使用同一个 `AsyncSession` 实现聚合操作，而不是拆成多个会自动 commit 的 storage 调用。

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

- [x] user-service 文档定义 daemon/runtime registration token 流程。
- [x] token 原文不入库、不进日志。
- [x] API 只在创建时返回 token 原文。
- [x] 一次性 token 的 exchange 原子。
- [x] 过期和撤销被强制执行。
- [x] audit 只记录 `token_id`。
- [x] 测试覆盖成功、过期、已撤销、已使用、无效、作用域不匹配。
- [x] 审查发现已修复或明确记录。
- [x] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| Python 测试 | `cd ../user-service && uv run pytest tests -v` | token API/storage/service 测试通过。 |
| API 文档 | `git diff --check -- SPEC.md docs` | 文档 diff 干净。 |
| secret 搜索 | 检查日志、测试和 token 相关代码 | token 原文不被记录或持久化。 |
| DB migration | 按 user-service 文档运行 migration/test setup | token 表或 schema 正确应用。 |
| 安全审查 | 手工审查 token hashing、过期、撤销和 audit | 发现已记录并处理。 |

实际验证记录：

| 命令或检查 | 结果 |
|---|---|
| `uv run ruff format src/user_service/app/agent_registration src/user_service/storage/sqlmodel/models/agent_registration.py tests/app/agent_registration` | 通过。 |
| `uv run ruff check src/user_service/app/agent_registration src/user_service/app/app.py src/user_service/app/container.py src/user_service/storage/interfaces.py src/user_service/storage/types.py src/user_service/storage/sqlmodel/storage.py src/user_service/storage/sqlmodel/models/__init__.py src/user_service/storage/sqlmodel/models/agent_registration.py tests/app/agent_registration tests/conftest.py` | 通过。 |
| `uv run python -m py_compile src/user_service/app/agent_registration/*.py src/user_service/storage/sqlmodel/models/agent_registration.py` | 通过。 |
| `uv run python -m pytest tests/app/agent_registration -v` | 通过，9 passed。 |
| `git diff --check -- SPEC.md docs src tests` | 通过。 |
| secret/audit 搜索 | 生产代码无 registration token 原文日志；审计上下文只写 `token_id`、scope、result、reason、agent_kind、handle、controller_did、did、user_id 等元数据，不写 token 原文或 `token_hash`。 |
| `uv run python -m pytest tests -v` | 未通过：仓库既有同名测试文件导致 pytest 顶层模块收集冲突。新增 agent_registration 测试文件改为唯一文件名后，冲突从 5 个降到 3 个，剩余来自既有 `content` / `tenant_site` / `core` 测试文件。 |
| `uv run python -m pytest tests --import-mode=importlib -q` | 635 passed、10 skipped、10 failed；失败集中在既有 DID profile / DID relationship 测试缺少 `did_auth_service` 注入，以及 Telegram bot-bound ticket 两个既有失败，不在 Step 06 改动路径。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查 API 契约、授权、随机数、hash、过期、撤销、audit、日志、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 已处理 | `one_time=false` 会让调用方误以为第一版支持可复用 registration token；撤销路径未先检查过期状态，过期 token 可能被撤销成 revoked；新增测试文件名 `test_service.py` / `test_rpc_handlers.py` 会加重仓库已有 pytest 顶层模块名冲突。 |
| 已修复 | 已完成 | schema 限制 `one_time` 必须为 true，并更新 API 文档；revoke 前调用 `_ensure_active()`，过期/已使用/已撤销 token 不再被撤销；新增 reusable token 拒绝测试和 expired revoke 测试；新增测试文件改为 `test_agent_registration_*` 唯一名称。 |
| 残余风险 | 已记录 | `exchange_token` 第一版直接创建 agent User/DID 文档，但未实现额外 DID proof 校验；该步骤聚焦 registration token 授权，DID proof 细节后续在注册链路整体设计中继续收敛。全量测试存在既有失败，已记录具体失败范围。 |
| 测试缺口 | 已接受 | 已覆盖成功、无效、过期、已撤销、已使用、scope mismatch、owner 权限、一次性兑换、原文不入审计；未做真实 HTTP app 全链路数据库测试，后续步骤 07/08 做 daemon 注册闭环和系统测试。 |
| 文档缺口 | 已修复 | 已更新 `SPEC.md`、`docs/api/agent-registration.md`、`docs/api/README.md`、`docs/database-design.md`、`docs/installation.md`、`src/user_service/app/CLAUDE.md`、`src/user_service/storage/CLAUDE.md`。 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：user-service token API、storage、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`user-service: add agent registration token api`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 全量 `uv run python -m pytest tests -v` 无法作为干净通过证据 | pytest 收集阶段仍有 3 个既有同名测试文件冲突，来自 `content` / `tenant_site` / `core`，不来自 Step 06 新增测试文件。 | 将 Step 06 新增测试文件改为唯一名称；运行 `tests/app/agent_registration` targeted 测试；补跑 `--import-mode=importlib` 替代全量测试。 | 不阻塞 Step 06 提交；阶段 B Review 需记录该全量测试残余风险。 | 后续单独整理 user-service 测试命名/pytest import mode。 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 2026-05-31 | 第一版 registration token 明确限制为一次性 token。 | 当前表结构只有单个 `used_at` / `used_by_did`，不能安全表达可复用 token 的多次消费记录。 | 步骤 06 执行账本已记录。 |

## 13. 风险、回滚与后续

- 风险：token API 可能与现有 Handle token 机制重叠；token 原文泄漏风险高。
- 回滚：暂停 registration token issuance，保留手工本地 daemon agent setup。
- 后续：步骤 07 daemon agent 创建必须消费该 API，不能另造 local-only 注册机制。
