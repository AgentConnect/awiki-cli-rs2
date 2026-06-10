# Step 01：user-service registry 与 DID auth 注册/更新闭环

主 Plan：[../plan.md](../plan.md)  
Step index：01  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` / `user-service` 当前分支 |
| Started | 2026-06-10T01:49:07Z |
| Completed | 2026-06-10T02:11:46Z |
| Commit | `user-service` `dada5a6` (`user-service: sync delegated key registry from did auth`) |
| Review evidence | 已 Review DID Document proof、unsigned mutation、registry 派生、旧调用兼容、private material、撤销语义；发现并修复 storage update 失败仍可能同步 registry、backfill/reconcile 入口缺失、`did/` 新 helper 文档未更新、文件头/CLAUDE.md 漂移。 |
| Verification evidence | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v`：137 passed, 32 warnings；`cd user-service && uv run ruff check src/user_service/app/did src/user_service/app/did_auth src/user_service/storage tests/app/did tests/app/did_auth`：All checks passed；`cd user-service && git diff --check`：通过；secret 搜索只命中既有 DID 主私钥下载/废弃 proof helper/禁止字段校验/文档 warning。 |
| Next action | Step 02：im-core / awiki-me 恢复与已有身份 daemon subkey migration |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`awiki-me` / `im-core` 当前实际使用的 DID auth `register` 路径，在保存包含 `user_did#daemon-key-1` 的 DID Document 后，`user-service` delegated key registry 也有对应 active record。
- 用户 / 系统可见行为：新注册用户可以在 registry 中查询到 daemon delegated key；撤销/轮换通过用户签名 DID Document update 生效；registry 状态与当前 DID Document `authentication` 保持一致；proof 验证不会因为服务端后置 JSON patch 失效。
- 非目标：不让 message-service 查询 user-service registry；不由 user-service 生成 daemon private key；不实现未签名的服务端 DID Document patch 作为撤销主路径。
- 完成标准：DID auth register/update、REST DID create、registry backfill/reconcile、proof validity、revoke/rotate 语义都有测试覆盖；user-service 不接触 daemon private material。

## 3. 设计方法

- 设计边界：message-service MVP 授权只看 DID proof 和当前 DID Document `authentication`。因此 user-service registry 是管理面、审计面和查询面，不是运行时授权事实来源。
- 核心决策：`did-auth.register` 保存 DID Document 前已完成 proof 验证，可以从该已验证 DID Document 中派生 `#daemon-key-1` registry record。`did-auth.update_document` / 等价 signed update 成功后，registry 根据新 DID Document 的 authentication 差异同步 active/revoked/rotated 状态。
- 契约 / API / 数据流：`im-core` 注册请求继续可以只提交 signed DID Document；user-service 解析其中 `id == {did}#daemon-key-1`、`type=Multikey`、`controller={did}`、`authentication` 包含该 id 的 method，生成或更新 registry record。REST DID create 的 `delegated_key_public_registration` 仍可保留，但必须在 proof 生成前插入，或明确只用于未签 proof 的 service-managed 流程；不能在 signed document 后置 patch。
- 兼容性：老 DID Document 无 `#daemon-key-1` 时仍按旧流程注册；registry 派生逻辑只对符合 MVP fragment 的 method 生效；所有新增响应字段 optional。
- 迁移策略：新增 backfill/reconcile 函数，从已有 DID Document 扫描并补 registry；对已不存在于 authentication 的 registry active key 标记 revoked/stale。
- 风险控制：禁止存储或返回 private key；撤销 API 如果只收到 verification method，不应无签名修改 DID Document。它可以返回“需要 signed DID Document update”或只撤销 registry 管理状态，但必须明确不会让 message-service 运行时立即拒绝该 key，除非 DID Document authentication 已更新。

## 4. 实现方法

1. 阅读 `user-service/src/user_service/app/did_auth/service.py` 的 `register_did`、`update_document`、`replace_did`、`recover_handle` 相关逻辑，确认 DID Document 验证和保存点。
2. 抽出 delegated key parser：输入 DID Document dict，输出符合 MVP 的 `DelegatedKeyRegistryCandidate`，要求：
   - method id 等于 `{did}#daemon-key-1`；
   - `controller` 等于 DID；
   - `authentication` 包含该 method；
   - key type / algorithm 是 MVP 支持的 Multikey / Ed25519；
   - 提取 `publicKeyMultibase`；
   - 不解析或接受任何 private 字段。
3. 在 `did_auth.register` 保存 DID Document 后，同事务或同一服务流程内 upsert delegated registry record。重复相同 public key 幂等，public key 不同按 conflict 或 rotated policy 处理，具体以现有 registry model 能力为准。
4. 在 signed DID Document update 成功后执行 registry reconcile：
   - 新文档包含 `#daemon-key-1` 且 registry 不存在：创建 active；
   - registry active 但新文档 authentication 不含该 key：标记 revoked/stale；
   - key public value 改变：按 rotate 记录旧 key revoked、新 key active，或按 MVP 单 key conflict 策略处理。
5. 修复 REST DID create 的 proof 风险：如果 `create_did_wba_document` 已生成 proof，则不要再后置 patch；改为在调用 DID factory 前传入 additional auth method，或在 Step 05 前先临时重签并补 proof verification test。若短期无法重签，禁用 REST delegated public registration 并在 API 文档中标记使用 signed DID auth path。
6. 修正 `revoke_delegated_key` 语义：
   - 不再把未重签 `_remove_verification_method` 作为最终撤销路径；
   - 要求用户提交 signed DID Document update 删除 key，或提供明确的 `revoke_registry_only` 管理状态；
   - 文档中说明 message-service 运行时拒绝依赖 DID Document authentication 更新。
7. 增加 backfill/reconcile 管理函数和测试，可由后续 migration/job 调用。
8. 更新 OpenAPI/API docs，明确 registry 与 DID Document 的关系、signed update 要求和 private material 禁止项。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `user-service/src/user_service/app/did_auth/service.py` | 在 register/update_document 成功后同步 delegated registry；修正撤销语义入口 | 实际 APP 注册路径关键入口 |
| `user-service/src/user_service/app/did_auth/rpc_handlers.py` | 如 response 或 error code 变化，更新 handler/schema | 保持旧 RPC 兼容 |
| `user-service/src/user_service/app/did/service.py` | 修复 REST DID create 后置 patch proof 风险；调整 revoke 行为 | 不再 unsigned patch DID Document |
| `user-service/src/user_service/app/did/repository.py` | 增加 registry upsert/reconcile/backfill 方法 | 复用现有 storage |
| `user-service/src/user_service/storage/*` | 必要时补 registry 状态字段或 migration | 保持 no private material |
| `user-service/docs/api/*` | 更新 DID delegated key 管理语义 | 说明 signed update 才能撤销运行时授权 |
| `user-service/tests/app/did*` | REST DID create/revoke/backfill/proof tests | 覆盖 proof validity |
| `user-service/tests/app/did_auth*` | DID auth register/update registry tests | 覆盖实际 APP 注册路径 |

## 6. 依赖

- 前置步骤：无。
- 外部文档或决策：主 Plan 第 4 节假设；核心设计文档中 message-service MVP 授权只看 DID Document `authentication`。
- 环境前提：能运行 user-service unit tests；如涉及数据库迁移，需按仓库现有 fixture 或 local DB 运行。

## 7. 验收标准

- [x] DID auth `register` 接收已包含 `#daemon-key-1` 的 signed DID Document 后，registry 生成 active record。
- [x] DID auth signed update 删除 `#daemon-key-1` 后，registry 同步为 revoked/stale，且 DID Document proof 仍有效。
- [x] REST DID create 不再生成 proof 失效的 DID Document；测试验证 proof validity。
- [x] `revoke_delegated_key` 不再依赖未重签 JSON patch 作为运行时撤销主路径。
- [x] backfill/reconcile 能从已有 DID Document 补 registry，缺失 registry 的旧用户可修复。
- [x] user-service request/response/storage 不包含 daemon subkey private material。
- [x] 老 DID auth register/update 调用兼容。
- [x] API 文档说明 registry、DID Document、message-service 授权边界。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v` | DID auth register/update、REST DID create/revoke、registry reconcile/backfill 全部通过。 |
| Lint | `cd user-service && uv run ruff check src/user_service/app/did src/user_service/app/did_auth src/user_service/storage tests/app/did tests/app/did_auth` | 无 lint error；如仓库未配置 ruff，记录原因。 |
| Proof | 新增测试中调用 ANP proof verifier 或现有 helper | 追加/删除 daemon key 后 DID Document proof 仍有效。 |
| Security | `rg -n "private_key|privateKey|BEGIN PRIVATE|daemon.*private" user-service/src/user_service/app/did user-service/src/user_service/app/did_auth user-service/docs/api` | 只有禁止字段校验、测试夹具或文档 warning；无 request/response 暴露 private material。 |
| Docs | 检查 `user-service/docs/api/*` 和 OpenAPI 更新 | signed update / registry / runtime auth 边界描述准确。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：DID Document proof 是否有效；是否存在 unsigned DID Document mutation；registry 是否从已验证文档派生；old DID auth RPC 是否兼容；private material 是否泄露；撤销语义是否误导为 registry-only 运行时撤销。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 4 项 | storage update 返回 false 时不应同步 registry；缺少公开 backfill/reconcile 入口；新增 `did/delegated_key_registry.py` 后 `did/CLAUDE.md` 未更新；did_auth/service/repository 文件头和 `did_auth/CLAUDE.md` 未反映 registry 同步职责。 |
| 已修复问题 | 4 项 | `update_did_document` 仅在 storage update 成功后同步 registry，并补测试；新增 `reconcile_daemon_delegated_key_registry` 和 backfill/revoke 测试；更新 `did/CLAUDE.md`；更新 did_auth 文件头和 `did_auth/CLAUDE.md`。 |
| 剩余风险 | 已记录 | register/update/replace/recover 的 DID Document 保存与 registry 同步仍不是数据库单事务；如同步失败会留下短时不一致，运行时授权仍以 DID Document `authentication` 为事实源。REST create 的重签是 Step 05 前的临时兼容路径，后续需由 ANP SDK optional additional authentication method 替代。 |
| 新增或缺失测试 | 已新增 | 新增 register 派生 registry、signed update 删除 key 后 revoke registry、reconcile backfill/revoke、REST create 重签 proof、revoke 必须先 signed update、storage update 失败不同步 registry 等测试。 |
| 已更新或缺失文档 | 已更新 | 更新 `user-service/docs/api/did-auth.md`、`user-service/docs/api/did-internal.md`、`user-service/src/user_service/app/did/CLAUDE.md`、`user-service/src/user_service/app/did_auth/CLAUDE.md`。OpenAPI JSON 未手写更新，保留由仓库生成流程统一刷新。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 user-service registry / DID auth / DID proof 修复及直接测试文档。
- Commit 前状态：记录 `cd user-service && git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`user-service: sync delegated key registry from did auth`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 无 | `did-auth.update_document` 已存在并通过测试覆盖 signed DID Document update；Step 01 已提交 | - | - | 继续 Step 02 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 01 | 修复 registry 与实际 DID auth 注册路径断点 | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：registry backfill 与当前 DID Document cache 可能短时不一致。
- 回滚 / 回退：回滚 registry 同步代码时，保留 DID Document `authentication` 授权主路径；禁用管理面撤销 UI。
- 后续文档：如 API 行为变化，同步 user-service docs 和 Agent IM 核心设计文档的实现状态。
