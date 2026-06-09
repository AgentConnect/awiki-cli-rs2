# Step 01：user-service DID delegated subkey

主 Plan：[../plan.md](../plan.md)  
Step index：01  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T10:06:33Z |
| Completed | 2026-06-09T10:38:54Z |
| Commit | `user-service` `b3f4c59` (`user-service: add daemon delegated did key`) |
| Review evidence | 2026-06-09 Review：检查 DID Document 兼容性、APP 侧 private key ownership、撤销语义、registry 状态、老客户端 optional 兼容、API/OpenAPI/初始化 SQL 文档同步和 private material 搜索。发现并修复 3 项：撤销缺失 registry 时先改 DID Document 的部分写入风险；revoked registry record 被当作 active 幂等返回的风险；`verification_method` 字段级 unique 与命名 UniqueConstraint 重复风险。 |
| Verification evidence | `cd user-service && uv run python -m pytest tests/app/did -v`：32 passed；`cd user-service && uv run ruff check src/user_service/app/did src/user_service/storage tests/app/did/test_service_managed.py tests/conftest.py`：All checks passed；`cd user-service && uv run python -m py_compile src/user_service/app/did/schemas.py src/user_service/app/did/service.py src/user_service/app/did/repository.py src/user_service/app/did/router.py src/user_service/storage/types.py src/user_service/storage/interfaces.py src/user_service/storage/sqlmodel/models/did.py src/user_service/storage/sqlmodel/models/__init__.py src/user_service/storage/sqlmodel/storage.py tests/app/did/test_service_managed.py tests/conftest.py`：通过；`cd user-service && uv run python scripts/gen_openapi.py`：已更新 `docs/openapi.json`；`cd user-service && git diff --check`：通过；旧 `daemon-key-*` / 设备示例 / private JWK 残留搜索无命中。 |
| Next action | Step 01 已完成；下一步执行 Step 02 ANP SDK / im-core optional params |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：APP 在创建用户 DID Document 时默认本地生成 `user_did#daemon-key-1` 子私钥，并把对应 public verification method 交给 `user-service` 登记到初始 DID Document；`user-service` 不生成、不接收、不返回 daemon subkey private material。
- 用户 / 系统可见行为：APP 创建用户 DID 后，本地持有 daemon key private package，服务端返回包含 daemon public key 的 DID Document 和 registry 记录；后续 APP bootstrap 只通过普通消息发送把 APP 本地持有的既有子私钥传给 Daemon，不再修改 DID Document。
- 非目标：不实现独立 APP ↔ Daemon pairing channel、本地 RPC、局域网通道或第二条传输链路；不让 Daemon 持有用户主私钥；不实现 Agent DID delegation 或 ANP delegated proof。
- 完成标准：DID Document 的 `verificationMethod` 与 `authentication` 包含 daemon public key；registry 能标记 key scope、状态、APP 实例和撤销；老 DID 创建调用兼容；测试和 API 文档更新。

## 3. 设计方法

- 设计边界：APP 是 daemon subkey private material 的唯一生成方和初始持有方；user-service 只登记 public verification method、DID authentication relationship 和本域 registry 状态。message-service MVP 运行时授权只按 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope 校验；后续跨服务 policy client 需单独设计。
- 核心决策：daemon key 是用户 DID 下的附属 authentication key，不替代 did:wba path binding main key，不在 pairing 时追加。MVP 一个 APP 默认只有一个 daemon key，固定 fragment 为 `#daemon-key-1`；fragment 不包含设备型号、设备名、时间戳或其他可识别设备隐私的信息。
- 契约 / API / 数据流：APP 本地生成 `DaemonSubkeyPrivatePackage`，只把 `DaemonDelegatedKeyPublicRegistration` 传给最新 DID 创建 API；服务端返回 `user_did`、DID Document 和 `DelegatedKeyRegistryRecord`，不返回 daemon subkey private key。
- 兼容性：旧客户端不传 daemon public key 时，服务端维持旧 DID 创建行为；新 APP 必须通过 optional 参数显式提交 daemon public key，user-service 不替新 APP 生成 daemon private key。
- 迁移策略：已有用户没有 daemon key 时，APP 通过后续兼容补齐/rotate endpoint 提交新 public key；MVP 对同一个 APP 重复提交 `#daemon-key-1` 必须幂等，提交不同 public key 必须返回冲突或走 rotate/revoke 流程。
- 风险控制：registry 字段应包含 `status=active/revoked/rotated`、`scopes`、`created_at`、`revoked_at`、`app_instance_id`、`key_version`、`last_used_at` 或审计可扩展字段；不得包含 daemon private key、设备名、设备型号或可识别设备隐私字段。

### 3.1 固定契约：key package 与 registry schema

Step 01 必须先冻结以下契约和 fixture，后续 Step 02/03/06 只能消费这些结构，不得各自发明字段。

#### `DaemonDelegatedKeyPublicRegistration`

APP 传给 user-service 的 public registration：

```json
{
  "schema": "awiki.user.delegated_key.public_registration.v1",
  "purpose": "daemon_message_agent",
  "key_fragment": "daemon-key-1",
  "verification_method": "did:wba:example.com:user:alice:e1_xxx#daemon-key-1",
  "key_type": "Multikey",
  "key_algorithm": "Ed25519",
  "public_key_multibase": "z6Mk...",
  "relationships": ["authentication"],
  "scopes": [
    "message.send.plain",
    "message.inbox.read.plain",
    "message.history.read.plain"
  ],
  "app_instance_id": "app_instance_stable_id"
}
```

要求：

- `key_fragment` MVP 固定为 `daemon-key-1`，不带设备信息。
- `verification_method` 可选。APP 如果在创建前不知道最终 key-bound DID，可只传 `key_fragment` 和 public key；user-service 在主 DID 生成后补全为 `{user_did}#daemon-key-1`。如果 APP 传完整 `verification_method`，它必须属于当前 `user_did`。
- `public_key_multibase` 使用 DID Document 中的 Multikey/Ed25519 公钥编码；user-service 不接收 private key 字段。
- `app_instance_id` 是 APP 本地稳定实例 ID，可用于幂等和绑定，但不得直接暴露设备名、设备型号、用户名或硬件序列号。

#### `DaemonSubkeyPrivatePackage`

APP 本地持有并在 bootstrap 时通过普通消息发送明文 JSON 传给 Daemon 的 private package；它不发送给 user-service：

```json
{
  "schema": "awiki.daemon.subkey_private_package.v1",
  "user_did": "did:wba:example.com:user:alice:e1_xxx",
  "verification_method": "did:wba:example.com:user:alice:e1_xxx#daemon-key-1",
  "key_type": "Multikey",
  "key_algorithm": "Ed25519",
  "private_key_multibase": "z...",
  "public_key_multibase": "z6Mk...",
  "key_ref": "local:daemon-key-1",
  "allowed_usage_hint": [
    "message.send.plain",
    "message.inbox.read.plain",
    "message.history.read.plain"
  ]
}
```

要求：

- 只由 APP 生成和持有，MVP 通过普通消息发送明文 JSON bootstrap 交给 Daemon。
- 不进入 user-service request/response、普通查询、日志、审计详情或系统测试明文快照。
- `verification_method`、`key_algorithm`、`public_key_multibase` 必须和 public registration 一致。

#### `DelegatedKeyRegistryRecord`

user-service 保存和返回的 registry record：

```json
{
  "schema": "awiki.user.delegated_key.registry_record.v1",
  "user_did": "did:wba:example.com:user:alice:e1_xxx",
  "verification_method": "did:wba:example.com:user:alice:e1_xxx#daemon-key-1",
  "purpose": "daemon_message_agent",
  "status": "active",
  "scopes": [
    "message.send.plain",
    "message.inbox.read.plain",
    "message.history.read.plain"
  ],
  "app_instance_id": "app_instance_stable_id",
  "key_version": 1,
  "created_at": "2026-06-09T00:00:00Z",
  "revoked_at": null
}
```

要求：

- 不包含 private key。
- 同一 `user_did + app_instance_id + purpose` 在 MVP 只能有一个 active `#daemon-key-1`。
- 重复提交相同 public key 返回同一 active record；重复提交不同 public key 返回 conflict，除非调用 rotate/revoke 流程。

## 4. 实现方法

1. 阅读现有 DID 创建链路，确认 `user-service/src/user_service/app/did/service.py`、`schemas.py`、`repository.py` 和 `router.py` 的职责边界。
2. 在 DID 创建请求/响应 schema 中新增兼容字段，用于接收 APP 侧生成的 `DaemonDelegatedKeyPublicRegistration`；字段必须 optional，且不得包含 private key 字段。`verification_method` 可以由服务端在 DID 生成后根据 `key_fragment` 补全。
3. 在 DID Document 构造逻辑里把 APP 提交的 `#daemon-key-1` public verification method 加入 `verificationMethod` 与 `authentication`。
4. 在 storage model 中新增 daemon delegated public verification method 状态数据结构；如果现有 DID model 可承载，优先扩展现有模型，否则新增表和 repository 方法。
5. 实现 query/revoke/rotate 或至少 query/revoke 的 MVP 服务方法。撤销必须把 public verification method 从 DID Document `authentication` 移除或标记不可用；撤销对 message-service MVP 运行时授权的生效通过 DID Document `authentication` 更新和 DID Document cache 刷新体现。
6. 更新 API 文档，明确：bootstrap 不创建 DID key；user-service 不生成、不返回 daemon subkey private key；private package 只存在于 APP 本地和后续 APP -> Daemon bootstrap。
7. 增加测试：APP 提交 public registration 后 DID Document authentication 包含 daemon key、老请求兼容、重复同 public key 幂等、重复不同 public key conflict、registry 状态、revoke 后不可用、private key 字段在 user-service request/response/普通查询中都不存在。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `user-service/src/user_service/app/did/service.py` | DID 创建时登记 APP 侧生成的 daemon delegated public key | 重点实现入口 |
| `user-service/src/user_service/app/did/schemas.py` | 新增兼容 request/response schema | 字段 optional 或默认兼容 |
| `user-service/src/user_service/app/did/repository.py` | daemon delegated public verification method 状态读写 | 视现有 repository 边界调整 |
| `user-service/src/user_service/app/did/router.py` | 暴露 query/revoke/rotate API 或 RPC | 避免破坏旧 endpoint |
| `user-service/src/user_service/storage/sqlmodel/models/did.py` | 增加 registry 存储模型或字段 | 需要迁移时同步 storage 初始化 |
| `user-service/docs/api/*` | 更新 DID 创建和 delegated key 文档 | 说明 bootstrap 不再修改 DID Document |
| `user-service/tests/app/did*` | 新增/更新测试 | 若目录不同，以仓库现有测试结构为准 |

## 6. 依赖

- 前置步骤：无。
- 外部文档或决策：`awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` 第 0、3、5 节；`awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` 第 3.1、5.6 节。
- 环境前提：能运行 user-service Python 测试；若数据库迁移需要本地 DB，按 user-service README 或现有测试 fixture 配置。

## 7. 验收标准

- [x] 新 APP 创建 DID 时能提交 `DaemonDelegatedKeyPublicRegistration`，生成的 DID Document 包含 `user_did#daemon-key-1` 的 `verificationMethod`。
- [x] 新 daemon key 被加入 `authentication`，但不替代用户主 key。
- [x] user-service 不生成、不接收、不返回 daemon subkey private key；APP 本地生成 `DaemonSubkeyPrivatePackage`。
- [x] registry 能查询 daemon key 状态、scope、APP 实例绑定信息和 `key_version`。
- [x] 同一 APP 默认只有一个 active `#daemon-key-1`；重复相同 public key 幂等，重复不同 public key conflict。
- [x] revoke 后该 daemon key 被 policy 视为不可用。
- [x] 老 DID 创建调用行为兼容，测试覆盖。
- [x] API 文档明确 bootstrap 不再追加 DID key。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit | `cd user-service && uv run pytest tests/app/did -v` | public registration、DID 创建、幂等/conflict、registry、revoke 测试通过；记录通过/失败/跳过数量。 |
| API schema | 检查新增字段默认值和旧 fixture | 老客户端请求不失败，新响应字段符合文档。 |
| Security | 搜索 daemon delegated key API 中是否存在 private key 字段 | user-service daemon delegated key request/response/普通查询不含 private key；如存在必须修复。 |
| Docs | 检查 `user-service/docs/api/*` | 明确 APP 本地生成 key package、user-service 只登记 public verification method，以及 bootstrap 边界。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：DID Document 合规性、authentication key 风险、老客户端兼容、APP 侧 private key ownership、撤销语义、registry record 契约、单 APP 单 daemon key 幂等/冲突、测试覆盖。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 已发现 3 项 | 1. `revoke_delegated_key` 原实现会先从 DID Document 移除 key，再标记 registry；registry 不存在时会返回 404 但 DID Document 已被改动。2. existing registry record 为 `revoked` 时，重复提交相同 public key 可能被当作幂等 active record 返回。3. `SQLModelDIDDelegatedKey.verification_method` 同时有字段级 `unique=True` 和命名 `UniqueConstraint`，有重复约束风险。 |
| 已修复问题 | 已全部修复 | 撤销前先确认 registry record 存在且属于当前 DID；拒绝复用非 active registry record；去掉字段级 `unique=True`，保留命名 `UniqueConstraint`。 |
| 剩余风险 | 已记录 | Step 01 只实现 query/revoke 和 public registration；独立 rotate endpoint 未进入本步骤。MVP 撤销实时性仍依赖 message-service DID Document cache 刷新，后续 Step 07 需要继续处理。 |
| 新增或缺失测试 | 已新增测试 | 新增 public registration 写入 DID Document、拒绝 daemon private material、幂等注册、conflict、revoked record reuse 拒绝、撤销移除 authentication、registry 缺失时不改 DID Document 等测试。DID 测试 32 passed。 |
| 已更新或缺失文档 | 已更新 | 更新 `user-service/docs/api/did-internal.md`、`user-service/docs/openapi.json`、`user-service/scripts/migrations/init_all_tables_mysql.sql`、`user-service/src/user_service/app/did/CLAUDE.md`、`user-service/src/user_service/storage/CLAUDE.md`。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 user-service delegated public key registration、registry、测试和直接相关文档。
- Commit 前状态：`user-service` `## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong`，包含 16 个 Step 01 相关修改文件。
- 纳入文件：`user-service/docs/api/did-internal.md`、`user-service/docs/openapi.json`、`user-service/scripts/migrations/init_all_tables_mysql.sql`、`user-service/src/user_service/app/did/CLAUDE.md`、`user-service/src/user_service/app/did/repository.py`、`user-service/src/user_service/app/did/router.py`、`user-service/src/user_service/app/did/schemas.py`、`user-service/src/user_service/app/did/service.py`、`user-service/src/user_service/storage/CLAUDE.md`、`user-service/src/user_service/storage/interfaces.py`、`user-service/src/user_service/storage/sqlmodel/models/__init__.py`、`user-service/src/user_service/storage/sqlmodel/models/did.py`、`user-service/src/user_service/storage/sqlmodel/storage.py`、`user-service/src/user_service/storage/types.py`、`user-service/tests/app/did/test_service_managed.py`、`user-service/tests/conftest.py`。
- Commit 后证据：`user-service` commit `b3f4c59 user-service: add daemon delegated did key`；commit 后状态 `## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 1]`，无未提交文件。
- 遗留未提交变更：`user-service` 无。`awiki-cli-rs2/docs/agent-im` 是 Plan/设计文档目录，作为执行台账和后续步骤入口单独保留在 `awiki-cli-rs2` 工作区。
- 实际消息：`user-service: add daemon delegated did key`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| DID 创建 API 当前只能由服务端生成私钥 | 待填写 | 增加 optional public registration 输入，保持旧路径兼容；新 APP 路径必须 APP 侧生成 private key | 当前步骤 / Step 06 | 先更新 Plan 变更记录，再实现兼容方案 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 01 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 允许 public registration 省略完整 `verification_method` | user-service 现有 DID 创建由服务端生成最终 key-bound DID；APP 仍生成 daemon private/public key，user-service 只补全 DID URL 并登记公钥 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：daemon key 是 user DID authentication key，服务外可能按完整用户认证能力处理。
- 回滚 / 回退：撤销 registry 状态，移除 DID Document authentication 中对应 key，通知 message-service 清理 token/cache，APP 重新创建或 rotate key。
- 后续文档：实现后更新 user-service API 文档，并在主 Plan 台账记录实际 endpoint、字段和测试证据。
