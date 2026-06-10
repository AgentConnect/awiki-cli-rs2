# Step 04：key package schema 与 APP action capability 收口

主 Plan：[../plan.md](../plan.md)  
Step index：04  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2`、`awiki-me` 当前分支 |
| Started | 2026-06-10T03:45:56Z |
| Completed | 2026-06-10T04:22:36Z |
| Commit | `awiki-cli-rs2` `94b1c20`；`awiki-me` `fc1895f` |
| Review evidence | Review 完成；发现并修复 action 测试 fixture 仍把新 binding 伪装成 legacy、`user_delegated` 投影规则与 action 执行规则不一致、APP 未本地拒绝非 `pem` v2 encoding、Flutter 工具生成无关 Android 文件漂移 |
| Verification evidence | `cargo test -p im-core --locked`、`cargo test -p awiki-deamon --locked -j1`、`cargo test -p im-core-dart --locked`、`scripts/flutter/codegen-check.sh`、`packages/awiki_im_core flutter test`、`awiki-me flutter analyze`、`awiki-me flutter test` 均通过 |
| Next action | 启动 Step 05：ANP SDK DID Document additional authentication optional 参数 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：key package 字段名与实际编码一致；新 bootstrap payload 写出 v2 schema；旧 v1 package 可兼容读取；APP action 不再因为缺少/空 capability policy 自动启用全部 MVP allowlist。
- 用户 / 系统可见行为：新 APP bootstrap payload 明确表达 private key 编码，例如 `private_key_pem` 或 `private_key_multibase` 二选一且名实一致；Daemon action 执行以 APP 显式 capability policy 为准，空列表就是禁用。
- 非目标：不实现完整自动化配置 UI；不改变 MVP action 集合本身；不移除所有 legacy v1 数据读取能力。
- 完成标准：schema fixture 更新；Rust/Dart/Daemon 都能读旧写新；新 binding 缺 capability 或空 capability 不再默认全部可执行；旧 binding 有兼容迁移或 legacy 标记。

## 3. 设计方法

- 设计边界：schema 是跨 `im-core`、`awiki-me`、`awiki-deamon` 的契约，不能继续使用与编码不符的字段名作为主写出格式。
- 核心决策：新增 `awiki.daemon.user_subkey_package.v2`，推荐字段：
  - `private_key_encoding`: `"pem"` 或 `"multibase-ed25519-private"`；
  - `private_key_pem`：当编码是 PEM 时使用；
  - `private_key_multibase`：仅在真正 multibase 私钥编码时使用；
  - `public_key_multibase`：继续作为 DID Document public method 编码；
  - `verification_method`、`key_type`、`key_algorithm` 保留。
- 契约 / API / 数据流：im-core 新写 v2；awiki-me 透传 v2；awiki-deamon parser 同时接受 v2 和 legacy v1，legacy v1 中 `private_key_multibase` 若实际是 PEM，则按 PEM legacy decode 并记录 migration warning。
- 兼容性：已有本地 package、已有 bootstrap fixture 和测试数据不应立即失效；新写出的 package 必须是 v2。
- 迁移策略：identity store 加 package schema version；读取 v1 后可懒迁移为 v2；Daemon 状态中已存 identity 可保持原 secret 字段，但新 bootstrap replay 统一验证并写 v2 metadata。
- 风险控制：capability 默认值收紧可能影响旧 binding。为旧 binding 增加 `policy_source=legacy_default` 或 migration；新 binding 必须显式 capability，空列表不自动补全。

## 4. 实现方法

1. 更新 `im-core` DTO：
   - 新增 v2 package struct 或扩展现有 struct；
   - 明确 `private_key_encoding`；
   - 新写出使用 `private_key_pem`；
   - legacy v1 decode 保留。
2. 更新 `awiki-me` domain model：
   - `UserSubkeyPackage` 支持 v2；
   - bootstrap JSON 使用 v2；
   - tests 更新 fixture；
   - 确保 UI/log 不显示 private key。
3. 更新 `awiki-deamon` bootstrap parser：
   - 支持 v2 primary；
   - 支持 v1 legacy；
   - v1 字段名与实际 PEM 不一致时不向外传播，只在内部转换；
   - 所有 validation 使用标准化后的 key material。
4. 收紧 APP action capability：
   - `effective_allowed_actions` 不再在 configured empty set 时扩展全部 MVP actions；
   - 新 binding 缺 `capabilities` / `allowed_actions` 时默认空或拒绝 action；
   - 旧 binding 可根据 migration 标记保留旧默认，但必须审计并可撤销；
   - awiki-me bootstrap 始终显式传五个 MVP action 或用户配置子集。
5. 更新文档和测试：
   - key package v2 schema fixture；
   - legacy decode tests；
   - empty capability rejects action；
   - missing capability on new binding rejects action；
   - old binding migration behavior 明确测试。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/im-core/src/identity/dto.rs` | key package v2 DTO / legacy decode | public SDK surface |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_daemon_subkey.rs` | v2 package write/migration helper | 与 Step 02 ensure 复用 |
| `awiki-cli-rs2/crates/im-core-dart/*` | binding 更新 | 保持 Dart API |
| `awiki-cli-rs2/packages/awiki_im_core/*` | Flutter package codegen/tests | 若 DTO 暴露到 Dart |
| `awiki-me/lib/src/domain/entities/agent/agent_bootstrap.dart` | v2 bootstrap model | APP 写出 v2 |
| `awiki-me/lib/src/data/im_core/*` | mapper 更新 | v1/v2 兼容 |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs` | v2 parser + v1 legacy decode | 标准化后 validation |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/action.rs` | capability default 收紧 | 空列表禁用 |
| `awiki-cli-rs2/docs/agent-im/*` | schema 和 capability 文档更新 | 保持方案一致 |

## 6. 依赖

- 前置步骤：Step 03 完成，validation 逻辑存在标准化 key material 入口。
- 外部文档或决策：MVP action allowlist 仍是 `message.summarize_plain`、`message.create_draft`、`contact.read`、`contact.update_display_name`、`contact.update_note`。
- 环境前提：能运行 Rust/Dart/Flutter tests。

## 7. 验收标准

- [x] 新写出的 key package schema 是 v2，字段名与编码一致。
- [x] v1 legacy package 仍能读取并通过 Step 03 validation。
- [x] 文档和 fixture 不再把 PEM 示例写成 multibase 主路径。
- [x] 新 binding 缺 capability 或 capability 空列表时，APP action 被拒绝，不自动补成全部 MVP actions。
- [x] awiki-me bootstrap 显式传 capability policy。
- [x] 旧 binding 的兼容行为有 migration 标记或测试说明。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| im-core | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | v1/v2 package tests 通过。 |
| daemon | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1` | bootstrap parser/action capability tests 通过。 |
| Dart binding | `cd awiki-cli-rs2 && cargo test -p im-core-dart --locked && scripts/flutter/codegen-check.sh` | binding 和 codegen 通过。 |
| Flutter package | `cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | package tests 通过。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | App model/provider tests 通过。 |
| Naming search | `rg -n "private_key_multibase|awiki.daemon.user_subkey_package.v1|mailbox_" awiki-cli-rs2/docs/agent-im awiki-cli-rs2/crates awiki-me/lib` | v1 只出现在 legacy decode / migration / historical docs；无新增 mailbox 命名。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：schema 是否名实一致；legacy 是否安全读取而非继续写出；secret redaction；APP action 缺配置默认是否收紧；旧 binding 兼容是否有明确标记；文档是否同步。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 4 项 | action 测试 fixture 默认 `desired_agent.allowed_actions` 让“新 binding 缺 capability”错误通过；`user_delegated` 投影未完全复用显式 schema 规则；APP v2 package 允许非 `pem` encoding 写出；`flutter test` 反复改动无关 Android generated registrant。 |
| 已修复问题 | 已修复 | 新 binding fixture 默认不带 legacy actions；action 与 `user_delegated` 都只在显式 `awiki.app.capabilities.v1` 下读取 `capabilities/allowed_actions`，无 schema 时仅保留 legacy `desired_agent.allowed_actions`；APP 本地拒绝非 `pem` v2 encoding；无关 Android generated registrant 未进入提交。 |
| 剩余风险 | 已记录 | 旧 binding 无 schema 时仍允许 legacy `desired_agent.allowed_actions` 兼容路径；bootstrap private package 仍按 MVP 决策通过普通消息明文 JSON 传输；secure storage 和加密通道不在本步骤范围内。 |
| 新增或缺失测试 | 已覆盖 | 新增/更新 v2 serialization、v1 legacy read、Daemon v2/v1 bootstrap parser、empty/missing capability、legacy fallback、`user_delegated` allowed_actions 投影、APP unsupported encoding、awiki-me mapper/bootstrap payload tests。 |
| 已更新或缺失文档 | 已更新 | 更新 `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` 和 `awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` 的 bootstrap 示例和 capability policy 说明。 |

## 9.1 实际验证证据

| 命令 / 检查 | 结果 |
|---|---|
| `cd awiki-cli-rs2 && cargo fmt --check -p im-core -p im-core-dart -p awiki-deamon` | 通过 |
| `cd awiki-cli-rs2 && cargo test -p im-core --locked` | 通过；lib 272 passed，integration/doc tests 通过 |
| `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 bootstrap -- --nocapture` | 22 passed |
| `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 action -- --nocapture` | 9 passed |
| `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 user_delegated -- --nocapture` | 10 passed |
| `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1` | 通过；lib 110 passed，integration tests 21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 23 + 2，doc tests 0 passed |
| `cd awiki-cli-rs2 && cargo test -p im-core-dart --locked` | 6 unit + 13 facade passed |
| `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh` | Done |
| `cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | 12 passed |
| `cd awiki-me && dart format --set-exit-if-changed ...` | 4 files checked，0 changed |
| `cd awiki-me && flutter analyze` | No issues found |
| `cd awiki-me && flutter test test/agents/agent_control_service_test.dart test/data/im_core/awiki_im_core_mappers_test.dart` | 27 passed |
| `cd awiki-me && flutter test` | 273 passed |
| 两仓 `git diff --check` | 通过 |
| 命名 / secret 搜索 | `private_key_multibase` 只命中 legacy decode/tests、历史记录和兼容字段；`mailbox_*` 命中 email/mail 既有模型或历史检查命令；secret 关键词命中 secret handling/redaction、测试 fixture 和既有 JWT/key path 逻辑，无新增未解释泄露。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后已提交。
- Commit 范围：schema/capability 变更按仓库聚焦提交；未混入 ANP SDK Step 05。
- Commit 前状态：`awiki-cli-rs2` 仅暂存 Step 04 代码/生成文件/设计文档，计划台账未暂存；`awiki-me` 仅暂存 Step 04 Dart 代码和测试，无关 Android generated registrant 已恢复。
- 纳入文件：`awiki-cli-rs2` commit `94b1c20` 包含 im-core DTO/internal、im-core-dart/generated Flutter package、awiki-deamon bootstrap/action/message_agent/user_delegated/foreground fixture、两篇 agent-im 设计文档；`awiki-me` commit `fc1895f` 包含 bootstrap model、im-core mapper 和对应测试。
- Commit 后证据：`awiki-cli-rs2` `94b1c20 agent-im: add daemon subkey package v2`；`awiki-me` `fc1895f agent-im: write daemon subkey package v2`。
- 遗留未提交变更：仅本计划台账文档待回填并单独提交。
- 建议消息：已使用 `agent-im: add daemon subkey package v2`、`agent-im: write daemon subkey package v2`。

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| v2 schema 与现有 FFI/codegen 不兼容 | 未发生 | `scripts/flutter/codegen-check.sh` 和 `cargo test -p im-core-dart --locked` 通过；public Dart model 保留 optional legacy `privateKeyMultibase` | 当前步骤 | 无需更新 Plan |
| 旧 binding 行为收紧影响测试 | 已处理 | 新 binding 必须显式 schema；无 schema 仅保留 legacy `desired_agent.allowed_actions` fallback，并有测试覆盖 | 当前步骤 | 兼容策略已记录 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 04 | 收口 schema 命名和 capability 默认值 | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：旧 bootstrap payload 和新 parser 的边界复杂。
- 回滚 / 回退：继续读取 v1，但禁止新写 v1；如 action 收紧影响过大，临时只对新 binding 生效。
- 后续文档：更新核心设计文档中的 bootstrap JSON 示例和 APP action 策略说明。
