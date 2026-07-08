# Step 05：ANP SDK DID Document additional authentication optional 参数

主 Plan：[../plan.md](../plan.md)  
Step index：05  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `awiki-cli-rs2`、`user-service` 当前分支；`anp/anp` `master` |
| Started | 2026-06-10T04:26:17Z |
| Completed | 2026-06-10T05:57:51Z |
| Commit | `anp/anp` `c4b2144`；`awiki-cli-rs2` `ac2a0bf`；`user-service` `10ee4d0` |
| Review evidence | Review 完成；确认 SDK optional API 兼容、proof 覆盖最终 DID Document、consumer 主路径不再 post-proof patch；修复 user-service 文件头无关漂移并同步依赖文档 |
| Verification evidence | ANP Python 141 passed；ANP Rust 全量 cargo test 通过；im-core 272 lib tests 与 integration/doc tests 通过；user-service DID/DID auth 140 passed, 32 warnings；格式、ruff、三仓 diff check 通过 |
| Next action | Step 06：最终集成验证、安全 Review 与文档收口 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：ANP Python/Rust SDK 的 DID Document 生成 API 支持可选 additional authentication verification methods，在 proof 生成前加入 DID Document；`awiki-cli-rs2/im-core` 和 `user-service` 不再以产品层 JSON patch 作为主路径。
- 用户 / 系统可见行为：创建 DID Document 时即可包含 `user_did#daemon-key-1`，proof 从一开始就是有效的；旧 SDK 调用不传 additional 参数时行为完全不变。
- 非目标：不修改 ANP wire schema、origin proof 结构、Agent DID delegation 或 ANP delegated proof；不支持 K1 扩展承诺；不把 AWiki runtime storage 放进 ANP SDK。
- 完成标准：Python/Rust SDK tests 证明旧调用兼容、新 optional 参数 proof 有效；im-core/user-service 调用切换到新 optional API；产品层后置 patch 只作为 legacy fallback 或被移除。

## 3. 设计方法

- 设计边界：ANP SDK owns DID WBA document creation and proof generation；产品仓不应在 proof 后修改 DID Document 并自行补救，除非作为临时 legacy fallback。
- 核心决策：新增 generic optional 参数，而不是 AWiki 专用字段名。例如：
  - Python `create_did_wba_document(..., additional_verification_methods=None, additional_authentication=None)`；
  - Rust 不直接给 public `DidDocumentOptions` 增必填字段，避免外部完整 struct literal 旧调用编译失败；新增 `DidDocumentCreationOptions` / builder wrapper，`create_did_wba_document(hostname, DidDocumentOptions::...)` 旧调用继续可用，新调用用 wrapper 携带 `additional_verification_methods` / `additional_authentication`。
- 契约 / API / 数据流：调用方先生成 daemon public key，然后传入 additional method entry 和 authentication reference；SDK 在构建 DID Document 后、生成 proof 前插入；proof 覆盖完整最终文档。
- 兼容性：默认 `None` / empty list 与当前输出字节级或语义级兼容；旧 tests 必须通过。新增参数必须可选，不强制所有调用方更新。
- 迁移策略：先在 ANP SDK 增加能力和 tests；再改 `im-core` identity generation 直接传 additional method；再改 user-service REST DID create 使用 SDK optional 参数或禁用后置 patch；最后移除/限制 `identity_daemon_subkey::apply_to_did_document` 主路径，只保留 migration/legacy fallback。
- 风险控制：只支持 e1；K1 输入 fail closed 或按既有行为，不新增 K1 daemon key tests；additional method 必须校验 owner/controller 与 DID 一致。

## 4. 实现方法

1. Python SDK：
   - 修改 `anp/anp/anp/authentication/did_wba.py` 的 `create_did_wba_document`；
   - 新增 optional 参数；
   - 在 `generate_w3c_proof` 前插入 additional verification method 和 relationship；
   - 校验 method id fragment、controller、type/public key 字段；
   - 增加 proof validity tests 和 old behavior tests。
2. Rust SDK：
   - 阅读 `anp/anp/rust/src` 下 DID WBA options 和 builder；
   - 增加 optional additional methods；为保持 Rust source compatibility，不直接扩展 public `DidDocumentOptions` 的字段集合，而是用可 `From<DidDocumentOptions>` 的 wrapper / builder 承载新增 optional 数据；
   - 保持默认空值不改变旧行为；
   - 增加 Rust tests。
3. im-core 消费：
   - `identity_generation` 生成 daemon key public method 后，通过 ANP Rust SDK options 生成最终 DID Document；
   - 移除“生成后 patch + 重签”的主路径；保留 helper 只用于 legacy migration 或 tests；
   - 更新 tests，证明 proof method 仍是 `#key-1` 且 proof valid。
4. user-service 消费：
   - REST DID create 如果仍保留 public registration，使用 Python SDK optional 参数在 proof 前插入；
   - 不再 signed proof 后调用 `_apply_delegated_key_public_registration` patch；
   - 更新 tests。
5. 文档：
   - ANP SDK docs 或 docstring 说明 additional methods 是 generic DID Document extension，不是 delegated proof；
   - AWiki design docs 说明 SDK 支持后 im-core/user-service 不再后置 patch。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `anp/anp/anp/authentication/did_wba.py` | Python DID WBA additional authentication optional 参数 | proof 前插入 |
| `anp/anp/anp/unittest/authentication/*` | Python tests | old behavior + proof validity |
| `anp/anp/anp/unittest/proof/*` | 如需新增 proof tests | 验证完整 DID Document |
| `anp/anp/rust/src/*` | Rust DID Document creation wrapper / builder | 保持旧 `DidDocumentOptions` 调用兼容 |
| `anp/anp/rust/tests/*` | Rust tests | old behavior + proof validity |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_generation.rs` | 使用 ANP Rust SDK optional 参数 | 主路径下沉 |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_daemon_subkey.rs` | 降级为 package/helper 或 legacy migration | 不再主路径 patch |
| `user-service/src/user_service/app/did/service.py` | 使用 ANP Python SDK optional 参数或禁用 REST delegated patch | 修复 proof 风险 |
| `awiki-cli-rs2/docs/agent-im/*` | 文档更新 | 说明 SDK optional API |

## 6. 依赖

- 前置步骤：Step 01 修复 user-service registry/proof 语义；Step 04 确认 key package schema。
- 外部文档或决策：当前 AWiki 只支持 e1 DID profile；MVP 不实现 ANP delegated proof。
- 环境前提：能运行 ANP Python/Rust tests、im-core tests、user-service DID tests。

## 7. 验收标准

- [x] Python `create_did_wba_document` 新 optional 参数生成的 DID Document 包含 additional method 和 authentication，proof 有效。
- [x] Python 旧调用不传 optional 参数时行为兼容。
- [x] Rust DID Document creation wrapper / builder 新 optional 参数生成的 DID Document proof 有效。
- [x] Rust 旧调用不传 optional 参数时行为兼容。
- [x] im-core 新注册主路径不再依赖 proof 后 JSON patch + 重签。
- [x] user-service REST delegated public registration 不再 proof 后 patch 或被明确禁用。
- [x] 文档说明这是 DID Document creation optional extension，不是 ANP delegated proof。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| ANP Python | `cd anp/anp && uv run pytest anp/unittest/authentication anp/unittest/proof -v` | DID WBA/proof tests 通过。 |
| ANP Rust | `cd anp/anp/rust && cargo test --locked` | Rust SDK tests 通过。 |
| im-core | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | identity generation/proof tests 通过。 |
| user-service | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v` | REST/DID auth delegated tests 通过。 |
| Compatibility | 对比旧 fixture 或新增 snapshot | 默认参数输出保持兼容；如 timestamp/nonce 导致字节不同，记录语义兼容证据。 |
| Docs/search | `rg -n "apply_to_did_document|后置|patch|delegated_key_public_registration" awiki-cli-rs2/docs/agent-im user-service/src awiki-cli-rs2/crates/im-core/src` | 后置 patch 只作为 legacy/migration 出现。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：old SDK API 是否兼容；proof 是否覆盖最终文档；additional method 是否 generic 而非 AWiki runtime-specific；K1 是否未被无意承诺；im-core/user-service 是否真正切换主路径；测试是否覆盖 tamper/negative case。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 发现 user-service `service.py` 文件头无关漂移；发现 `CLAUDE.md` / `SPEC.md` 仍记录旧 ANP SDK 版本；确认 `user-service` 使用 sibling editable source 消费未发布 ANP 0.8.7 是发布风险 | 文件头漂移已恢复；依赖文档已同步；发布风险记录到剩余风险 |
| 已修复问题 | 恢复 `from __future__ import annotations`；同步 `user-service/CLAUDE.md` 和 `user-service/SPEC.md`；Rust SDK API 从直接扩展 `DidDocumentOptions` 改为 `DidDocumentCreationOptions` wrapper / builder | 旧 `create_did_wba_document(hostname, DidDocumentOptions)` 源码兼容 |
| 剩余风险 | `user-service` 当前通过 `../anp/anp` editable source 使用未发布 ANP 0.8.7，发布前需随 ANP SDK 同步发布或切回已发布包；byte-level 输出因随机 key / proof timestamp 不固定，只验证语义兼容 | 不影响本地验证；Step 06 全局 Review 继续检查发布路径 |
| 新增或缺失测试 | 已新增 Python/Rust SDK old behavior、additional auth proof、tamper、controller mismatch、unknown reference tests；user-service 增加 SDK additional auth 前置 proof 测试和 absolute verification_method 拒绝测试；im-core 注册/恢复路径由既有 identity tests 覆盖 | 未新增 byte-level snapshot，理由见剩余风险 |
| 已更新或缺失文档 | 已更新 `agent_im_core_design.md`、`agent_delegated_identity_message_proof_plan.md`、`user-service/CLAUDE.md`、`user-service/SPEC.md` 和本 Step 台账 | ANP README/API docs 未单独更新，当前以 docstring + tests 作为 SDK 行为说明，Step 06 可决定是否补发行文档 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：ANP SDK commit 先行；消费仓库 `awiki-cli-rs2` / `user-service` 后续聚焦 commit。
- Commit 前状态：记录相关仓 `git status --short --branch`。
- 纳入文件：记录每个 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`anp: support additional did authentication methods`、`im-core: use anp did additional auth option`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| ANP Rust DID builder 不易扩展 | 已解决 | 增加 `DidDocumentCreationOptions` wrapper / builder，保留旧 `create_did_wba_document(hostname, DidDocumentOptions)` 签名 | 当前步骤 | 已记录 Plan 变更并完成 Review |
| 默认输出无法做字节级兼容 | 已处理 | 使用 old behavior tests、proof validity、tamper-negative 和默认无 additional method 语义检查 | 当前步骤 | 记录为语义兼容证据，不做随机输出 snapshot |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 05 | 下沉 DID Document additional authentication 到 ANP SDK | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：ANP SDK 是多产品基础库，API 命名和行为要保持 generic。
- 回滚 / 回退：保留 im-core 本地 patch + re-sign fallback，但标记为 legacy；不得继续 user-service unsigned patch。
- 后续文档：更新 ANP SDK docstring / docs 和 AWiki Agent IM 设计文档实现状态。
