# awiki-cli 本地状态升级系统设计

**文档状态**：Draft v1.2
**最后更新**：2026-07-14
**适用范围**：Rust `awiki-cli` 本地 config、identity store、SQLite、本地升级元数据，以及从 `awiki-agent-id-message` Python v1 布局导入 legacy 本地状态。

## 1. 目标

`awiki-cli` 使用统一 workspace upgrade system 管理本地 config、identity store、SQLite、runtime metadata 和历史 Python v1 数据导入。升级不分散在各业务模块里执行，而是在访问本地状态前由统一 runner 检测、备份、迁移和记录结果。

目标：

1. 用一个统一入口管理所有本地状态升级。
2. 支持跨版本跳变，而不是依赖用户按顺序安装每个历史二进制版本。
3. 兼容导入 Python v1 `awiki-agent-id-message` 的 legacy identity / SQLite / settings。
4. 通过 lock、backup、journal 提升升级中断后的可恢复性。
5. 升级流程必须保护本地敏感状态，不在日志、manifest、doctor 或 normal output 中输出 private keys、JWT、message plaintext、secure outbox plaintext、raw E2EE/MLS artifacts、provider stdout/stderr/path、raw SQLite rows 或 backup contents。

## 2. 版本模型

本地状态采用双层版本：

- **App Version**：CLI 发布 semver，例如 `1.0.16`。
- **Workspace Schema Version**：本地工作区格式版本，单调递增整数。

当前 Rust 实现中，`LATEST_WORKSPACE_SCHEMA_VERSION = 4`，定义在：

```text
crates/awiki-cli/src/workspace_upgrade/types.rs
```

当前 SQLite target schema version 为 `31`；canonical conversation cutover 在 schema `28` 引入，定义在：

```text
crates/im-core/src/internal/local_state/schema.rs
```

版本含义：

- `workspace schema 0`：已存在 awiki-cli 本地状态但尚未接入统一升级元数据，或仅存在 Python v1 legacy source，或仅存在未显式版本化的早期 config / DB。
- `workspace schema 1`：已完成 config / identity store / SQLite 的统一升级编排。
- `workspace schema 2`：已在 schema 1 基础上对旧 `awiki-agent-id-message` skill 做 best-effort 清理，包括 listener service、skill 安装目录和 OpenClaw `HEARTBEAT.md` legacy section。
- `workspace schema 3`：已扫描当前 identity store 中的 legacy handle 形态 `k1_...` DID，并按 replace-DID 兼容流程迁到 `e1_...` DID。
- `workspace schema 4`：SQLite 本地状态已收敛到 identity-owned schema 17，业务行使用 `owner_identity_id` 作为 owner partition key。DID recover/replace 只写 `identity_did_history` 并刷新 `owner_did` snapshot，不再做业务行 owner rebind。旧 SQLite schema 通过 backup 后 clean rebuild 进入干净 schema 17，不按 DID、credential、alias 或路径静默迁移业务所有权。

SQLite schema 18 之后逐步增加 conversation summary projection、local-first history hot index、`sync_state`、`thread_read_state` 和当前 message display read/send projection contract。当前消息显示链路的 owner key 是 `owner_identity_id + conversation_id`；升级或 rebuild 不得把 owner DID、legacy direct alias 或 App display thread id 当成新的持久 correctness key。

SQLite schema 28 增加 owner-scoped `peer_personas`、`peer_identifiers`、`peer_profiles`、append-only `conversation_aliases`，并将 conversation registry 的 `lifecycle_state` 与 `resolution_state` 分离。Handle Authority domain 由 Core 统一执行 IDNA/lowercase 归一化；缺少稳定 authority subject 或可用 binding status 时不得回退 DID 创建 Persona，DID 形式的 `peer_user_id` 也必须保持 canonical unresolved。Group fallback `membership_id` 不包含 Handle binding generation，DID rebind 不改变 membership identity。`messages` 新增 immutable `wire_thread_kind + wire_thread_ref + wire_identity_resolution_state`，`conversation_id` 只作为 mutable canonical projection；canonical merge 不得再改写 wire thread、DID/group snapshot 或 `server_seq`，相同 message ID 的 wire facts 冲突必须 fail closed。canonical Persona 暂未解析不会把已经能从 sender/receiver/group 原始事实恢复的 WireIdentity 降级为 unresolved。

SQLite schema 29 为内部消息投影增加 `hydration_state`，明确区分已持久化完整正文的 `hydrated`、只由 `sync.delta` 发现 metadata 的 `discovered`，以及需要一次可信远端扫描确认的旧行 `legacy_probe`。timeline 只返回 `hydrated`，conversation activity/unread 仍可由 `discovered` 推进；thread catch-up 必须从最早 hydration gap 之前开始，不能让 metadata 占用 sequence 后永久跳过正文。schema 28 在普通 open 中原子升级到 29，旧 backlog 缺少该字段时按其来源契约恢复状态。

SQLite schema 30 将可靠同步 checkpoint 从业务 `owner_did` snapshot 中拆出，使用 `(owner_identity_id, sync_subject_id, scope, checkpoint_kind)` 分区。当前 message service 的 `sync_subject_id` 是 canonical DID；DID recovery 不改写旧 subject 的 checkpoint，新 DID 第一次从 `0` 拉取。29→30 迁移重建私有 `sync_state`：保留未轮换身份的 checkpoint 和明确属于 previous DID 的历史 namespace；同一 owner 只要存在 previous DID，旧表中被标为 current DID 的 checkpoint 就因缺少 provenance 而一律失效，并从 `0` 幂等补同步，不依赖秒级时间戳猜测其来源。

SQLite schema 31 修复旧版本已经写入的 canonical Direct WireIdentity 错误。迁移只接受 `wire_thread_kind=thread`、`wire_thread_ref=conversation_id` 的精确旧错误形态，并要求消息 sender/receiver snapshot、resolved Direct registry、Persona DID identifier 与 owner-scoped direct route 同时证明同一个 peer DID 和 canonical conversation；满足时改为 `direct + peer DID`。证据不完整、跨 Persona 或 route 冲突的行保持不变，后续可靠重放仍按 `message_wire_identity_conflict` fail closed，不删除消息、不重置 checkpoint。

0710 migration 在创建 canonical Direct/Group registry row 后必须立即把对应 legacy registry row 标记为 `merged + resolved`，包括没有首条消息的空会话；不能依赖 message migration 顺带完成。已经离群或被移除的 Group 迁移为 `left + resolved` 且保持非 active，历史 alias 仍直接指向该 canonical row，不允许升级过程重新激活群会话。

release/0710 的生产 SQLite schema 27 不允许在普通 `open_writable` 中原地自动 bump。当前 Core 在 migration runner 接管前返回 typed `local_state_upgrade_required`，避免在一致性 backup、shadow migration 和 validation gate 完成前修改 source DB。正式 27→当前 schema 31 runner 位于 `crates/im-core/src/internal/local_state/canonical_upgrade/`：只读 structural preflight 和 schema fingerprint 通过后获取跨进程文件锁，使用 SQLite Online Backup API（包含已提交 WAL）生成并复验 backup，再创建独立 shadow。部分 target schema、未知 source schema、source fingerprint 变化和 integrity check 失败均 fail closed，且不修改 source。

当前只接受实际部署到 `awiki.info` 的 release/0710 daemon 0.1.76：source ref `d7c853a986a29e0c0457284a6b2c3d81ec637e10`、artifact SHA-256 `3134862f360acb73ca61867fe7d547f4ecd100369ba2bd4153d724251b45ce95`、schema fingerprint `sha256:0b8b6b902f8460ff1ea6c122d6b8b687722890136d9b7adb6e52d9d636ef6690`。脱敏 fixture 位于 `crates/im-core/tests/fixtures/release_0710/`，由 `scripts/generate_release_0710_fixture.py` 调用该发布二进制的 `init-state` 在隔离目录生成 schema 后，只写入确定性的 synthetic rows；未复制线上数据库、身份、消息、凭证或密钥。新增可支持的生产 fingerprint 必须逐个审计并显式加入白名单，不能放宽为任意 schema 27。

canonical upgrade journal 只记录 upgrade ID、schema/fingerprint、相对 artifact 名和阶段，不记录 owner DID、消息内容、凭证或密钥。升级阶段固定为 `detected → preflight_passed → backup_verified → shadow_migrated → validation_passed → cutover_started → completed`；恢复阶段为 `restore_started → restored`。runner 在 shadow transaction 中先增加 target 物理字段，再从 0710 verified route 建 Persona/identifier/alias、按 Group DID 收敛群会话、回填 immutable WireIdentity、保留 unresolved 行、重建 summary 并迁移 read-state canonical reference。validation 对 message、outbox、read、contacts、Handle bindings、groups、members、group rebind/P6 jobs、DID history、relationship 和旧 sync facts 做逐行 hash/计数守恒，检查空会话、WireIdentity 完整性、SQLite integrity 和 canonical invariant doctor；守恒通过后在同一 shadow transaction 中迁移 subject-scoped checkpoint，最后设置 schema 31 并进入 cutover。

cutover 先把完整 live SQLite file set 移到 owner-scope upgrade 目录的 rollback artifact，再把已验证 shadow 放到原路径。`cutover_started` 中断后，下一次 runner 会优先完成已验证 shadow，或在 shadow 不可用时恢复 rollback；backup 始终保留。相同 source/journal 可重复执行，已经完成 canonical cutover 的 schema 28/29/30/31 再次调用返回 NotRequired，不重复创建 alias、消息或 outbox；schema 28 的 hydration、checkpoint 与 WireIdentity 修复由普通 open 依次原子完成。

`restore_local_state_backup` / Dart `AwikiImCore.restoreLocalStateBackup` 是 Core 未打开时使用的完整降级入口。它只接受 journal 已完成 cutover 的已验证 backup，先把当前 schema 31 SQLite file set 保存为 private safety copy，再恢复并复验 schema 27；中断后可以按 journal 幂等续跑。旧 0710 二进制不得直接打开 schema 28/29/30/31，也不得按表回写或局部降级。

## 3. 工作区路径

默认 live workspace：

```text
~/.awiki-cli/
  config.yaml
  identities/
  data/awiki-cli.db
  cache/
  runtime/
  mls/
  logs/
  upgrade/
```

只支持 `AWIKI_CLI_WORKSPACE_HOME_DIR` 切换整个根目录。Python v1 目录只作为 legacy source，不作为 live workspace：

```text
~/.openclaw/credentials/awiki-agent-id-message/
~/.openclaw/workspace/data/awiki-agent-id-message/
```

如果 live workspace 与 legacy source 同时存在，默认以 live workspace 为准，legacy 仅用于诊断和显式导入候选。

## 4. Upgrade Metadata

升级元数据位于 `<workspace>/upgrade/`：

```text
upgrade/meta.json
upgrade/upgrade_journal.json
upgrade/upgrade.lock
upgrade/backups/<timestamp>/
```

`meta.json` 示例：

```json
{
  "workspace_schema_version": 4,
  "app_version": "1.0.16",
  "updated_at": "2026-05-31T10:00:00Z",
  "last_upgrade_id": "20260531T100000Z",
  "last_backup_dir": "/home/me/.awiki-cli/upgrade/backups/20260531T100000Z",
  "warnings": []
}
```

`upgrade_journal.json` 记录正在执行的迁移，用于中断恢复和 `doctor` 诊断。

## 5. 触发时机

Rust 实现入口：

```text
crates/awiki-cli/src/workspace_upgrade/upgrader.rs
upgrade_if_needed(resolved, app_version)
```

触发原则：

- 在需要访问本地状态的 CLI 服务初始化前执行。
- identity、message、runtime、debug store 等命令可以触发升级。
- `doctor`、`config show` 可以只做 inspection，不强制写入。
- config、identity、SQLite 子模块不得在自身 load/open 过程中偷偷自升级。

## 6. 检测逻辑

升级器检测：

- `meta.json` 和 `upgrade_journal.json`。
- live workspace：`config.yaml`、`identities/index.json`、`data/awiki-cli.db`。
- legacy source：旧 credentials、legacy SQLite、legacy settings。

判断规则：

- `meta.json` 存在时，以 `workspace_schema_version` 为准。
- `meta.json` 不存在但检测到 live workspace 或 legacy source 时，视为 schema `0`。
- 完全空工作区不触发升级写入。
- 检测到 workspace schema 高于当前二进制支持版本时，必须失败并提示升级 CLI。

## 7. 锁、备份与恢复

升级前必须获取 `<workspace>/upgrade/upgrade.lock` 对应的 OS file lock。锁文件可常驻，文件存在不代表升级未完成；真正互斥由 OS lock 决定。

备份目录：

```text
upgrade/backups/<upgrade-id>/
```

备份范围：

- `config.yaml`
- `identities/`
- `data/awiki-cli.db`
- `upgrade/meta.json`
- `upgrade/upgrade_journal.json`

SQLite 备份必须保持一致性，不能只裸拷贝主库文件。当前实现使用 SQLite 一致性备份方式。

如果 journal 显示升级中断，后续运行应优先给出可诊断状态，并在可安全重试时继续或重新执行迁移。

## 8. 迁移实现锚点

当前代码位置：

```text
crates/awiki-cli/src/workspace_upgrade/
  detect.rs
  lock.rs
  backup.rs
  journal.rs
  meta.rs
  migration_v0_to_v1.rs
  migration_v1_to_v2.rs
  migration_v2_to_v3.rs
  migration_v3_to_v4.rs
  upgrader.rs
```

相关诊断：

```text
crates/awiki-cli/src/diagnostics/mod.rs
```

## 9. 后续迁移：`1 -> 2`、`2 -> 3` 与 `3 -> 4`

`workspace 1 -> 2` 的职责：

- 删除 legacy skill 安装目录。
- 停止并清理 legacy websocket listener 服务。
- 移除 legacy heartbeat 注入片段。

`workspace 2 -> 3` 的职责：

- 针对已经完成旧版本迁移的既有 workspace，扫描当前 identity store 中的全部 identities。
- 对仍为 handle 形态 `k1_...` DID 的 identity 自动调用 `replace_did` 换绑为 `e1_...` DID。
- 替换前同样先备份旧 identity 目录到 `.legacy-backup/replace-did/`。
- 对已升级到 identity-owned schema 的 workspace，replace-DID 成功后只记录 DID history 并刷新同一 `owner_identity_id` 的 `owner_did` snapshot，不执行跨 owner 的业务行 rebind。
- 单个 identity 失败仍只记录 warning，不阻断 workspace upgrade。

`workspace 3 -> 4` 的职责：

- 确认 live SQLite 是否存在。
- 若 SQLite 不存在，保持空工作区语义。
- 若 SQLite schema 已经是 `17`，执行 owner invariant 检查。
- 若 SQLite schema 高于当前支持版本，fail closed。
- 若 SQLite schema 低于 `17`，必须先完成 workspace SQLite backup，然后删除旧 DB 文件集并创建干净 schema 17 DB。
- 迁移完成后写入 workspace schema 4 metadata。
- 不按旧 `owner_did`、`credential_name`、identity alias 或 path 静默迁移业务行。系统尚未上线时，备份后 clean rebuild 是默认安全策略。

## 10. 用户可见行为

用户通常不需要手动运行升级。访问本地状态时，CLI 会按需要执行升级并输出 warnings。

常用诊断：

```bash
awiki-cli doctor
awiki-cli config show
awiki-cli debug db handle-history alice
```

从旧 Python CLI 导入：

```bash
awiki-cli id import-v1
```

导入和自动升级可能生成包含敏感密钥材料的备份目录，不要上传或分享。

## 11. 校验与健康检查

每步迁移后至少做：

- config 可按最新结构读取。
- 若 SQLite 存在，则 `PRAGMA user_version == store.SchemaVersion`。
- `PRAGMA integrity_check`。
- `PRAGMA foreign_key_check`。
- SQLite schema 17 下 owner invariant 检查通过：
  - required owner tables 的 `owner_identity_id` 非空。
  - identity-owned natural key 没有重复。
  - 每个 identity 只有一个 current DID。
  - current DID 不跨 live identity 重复。
  - direct conversation id 不包含本地 owner DID。
- 若本次发生 legacy identity 导入，则至少存在一个可列出的 identity。

`doctor` 应额外暴露：

- 当前 workspace schema 来源。
- `meta.json` 内容。
- 是否存在 `upgrade_journal.json`。
- 是否仍检测到 legacy source。
- SQLite owner invariant 摘要，仅包含 table、invariant、row_count。
- legacy secure table 是否存在和计数。

`doctor` 不得暴露 raw SQLite rows、message plaintext、secure outbox plaintext、private keys、JWT、raw ciphertext、ratchet/MLS state、provider stdout/stderr/path 或 backup contents。

## 12. 安全要求

- 升级日志和错误不得输出私钥、JWT、secure state、raw MLS state。
- 备份目录默认视为敏感数据。
- 升级失败必须保留足够诊断信息，但不应把 legacy source 内容直接打印到普通输出。
- 多身份本地状态不得在升级中互相污染。

## 13. 当前实现边界

当前落地实现包含：

- `workspace_upgrade` 统一入口。
- `meta / journal / lock / backup / detection`。
- 真实迁移 `0 -> 1`、`1 -> 2`、`2 -> 3`、`3 -> 4`。
- 状态型 CLI 命令在本地状态初始化前触发升级检查。
- `doctor` / `config show` 可检查升级元数据。
- SQLite schema 17 的 identity-owned owner invariant 检查。
- SQLite schema 18 的 `conversation_summaries` 本地会话摘要投影。
- SQLite schema 19 的 owner/conversation/timestamp 本地历史热路径索引。
- SQLite schema 20 的 `sync_state` reliable sync checkpoint 表。

`sync_state` 是 `im-core` 内部可靠同步状态，不是 CLI/App/Dart public API：

- schema 30 主键为 `(owner_identity_id, sync_subject_id, scope, checkpoint_kind)`。
- `event_seq` 保存该服务端同步主体 reliable sync checkpoint 的十进制字符串。
- 当前 `sync_subject_id` 是 canonical DID，它是服务端事件流 owner，不是可随身份恢复改写的业务 snapshot；未来服务端提供稳定 account subject 时只替换该映射。
- `updated_at` 和 `metadata_json` 只用于诊断、兼容和后续扩展。
- `sync.delta` 由 Rust runtime 从 `sync_state` 读取 checkpoint，服务端事件页成功应用到
  本地 SQLite 后再在同一事务中推进 checkpoint。
- Dart SDK、Flutter App、CLI adapter 不暴露 checkpoint load/store，不允许调用方传
  `since_event_seq` 或手动推进 `next_event_seq`。

后续阶段可继续演进：

- listener runtime 私有状态纳入统一备份。
- 更细粒度的 migration phase 持久化。
- 面向 CLI 用户的 restore 命令与更完整的升级诊断输出；底层 Core/Flutter restore API 已提供。
