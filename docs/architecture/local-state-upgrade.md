# awiki-cli 本地状态升级系统

## 1. 目标

`awiki-cli` 使用统一 workspace upgrade system 管理本地 config、identity store、SQLite、runtime metadata 和历史 Python v1 数据导入。升级不分散在各业务模块里执行，而是在访问本地状态前由统一 runner 检测、备份、迁移和记录结果。

目标：

- 支持跨版本跳变，不要求用户逐个安装历史版本。
- 兼容导入 Python v1 `awiki-agent-id-message` 的 legacy identity、SQLite 和 settings。
- 通过 lock、backup、journal 支持中断恢复与诊断。
- 保持 Rust workspace 内的本地升级逻辑可测试、可回滚、可审计。

## 2. 版本模型

本地状态采用双层版本：

- **App Version**：CLI 发布 semver，例如 `1.0.16`。
- **Workspace Schema Version**：本地工作区格式版本，单调递增整数。

当前 Rust 实现中，`LATEST_WORKSPACE_SCHEMA_VERSION = 3`，定义在：

```text
crates/awiki-cli/src/workspace_upgrade/types.rs
```

版本含义：

- `0`：检测到旧工作区、未版本化早期 config/DB，或仅存在 Python v1 legacy source。
- `1`：完成 config、identity store、SQLite 的统一升级编排。
- `2`：完成旧 `awiki-agent-id-message` skill/listener/OpenClaw section 的 best-effort 清理。
- `3`：完成后续 workspace metadata / migration_v2_to_v3 中定义的当前格式收敛。

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
  "workspace_schema_version": 3,
  "app_version": "1.0.16",
  "updated_at": "2026-05-27T10:00:00Z",
  "last_upgrade_id": "20260527T100000Z",
  "last_backup_dir": "/home/me/.awiki-cli/upgrade/backups/20260527T100000Z",
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
  upgrader.rs
```

相关诊断：

```text
crates/awiki-cli/src/diagnostics/mod.rs
```

## 9. 用户可见行为

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

## 10. 安全要求

- 升级日志和错误不得输出私钥、JWT、secure state、raw MLS state。
- 备份目录默认视为敏感数据。
- 升级失败必须保留足够诊断信息，但不应把 legacy source 内容直接打印到普通输出。
- 多身份本地状态不得在升级中互相污染。
