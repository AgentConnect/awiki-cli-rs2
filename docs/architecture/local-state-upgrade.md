# awiki-cli 本地状态升级系统设计

**文档状态**：Draft v1.1
**最后更新**：2026-05-31
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

当前 local SQLite schema version 为 `19`，定义在：

```text
crates/im-core/src/internal/local_state/schema.rs
```

版本含义：

- `workspace schema 0`：已存在 awiki-cli 本地状态但尚未接入统一升级元数据，或仅存在 Python v1 legacy source，或仅存在未显式版本化的早期 config / DB。
- `workspace schema 1`：已完成 config / identity store / SQLite 的统一升级编排。
- `workspace schema 2`：已在 schema 1 基础上对旧 `awiki-agent-id-message` skill 做 best-effort 清理，包括 listener service、skill 安装目录和 OpenClaw `HEARTBEAT.md` legacy section。
- `workspace schema 3`：已扫描当前 identity store 中的 legacy handle 形态 `k1_...` DID，并按 replace-DID 兼容流程迁到 `e1_...` DID。
- `workspace schema 4`：SQLite 本地状态已收敛到 identity-owned schema 17，业务行使用 `owner_identity_id` 作为 owner partition key。DID recover/replace 只写 `identity_did_history` 并刷新 `owner_did` snapshot，不再做业务行 owner rebind。旧 SQLite schema 通过 backup 后 clean rebuild 进入干净 schema 17，不按 DID、credential、alias 或路径静默迁移业务所有权。

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

后续阶段可继续演进：

- listener runtime 私有状态纳入统一备份。
- 更细粒度的 migration phase 持久化。
- 显式 restore 命令与更完整的升级诊断输出。
