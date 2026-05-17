# awiki-cli 本地状态升级系统设计

**文档状态**：Draft v1.0  
**最后更新**：2026-04-11  
**适用范围**：`awiki-cli` 本地 config、identity store、SQLite、本地升级元数据，以及从 `awiki-agent-id-message` Python v1 布局导入 legacy 本地状态。

---

## 1. 目标

awiki-cli 不再把本地状态升级拆散到 config、identity、SQLite 三套零散逻辑里，而是统一收敛为一个 **workspace upgrade system**。

系统目标：

1. 用一个统一入口管理所有本地状态升级。
2. 支持跨版本跳变，而不是依赖用户按顺序安装每个历史二进制版本。
3. 兼容导入 Python v1 `awiki-agent-id-message` 的 legacy identity / SQLite / settings。
4. 通过 lock、backup、journal 提升升级中断后的可恢复性。
5. 全部实现保持 **pure Go / no CGO**。

---

## 2. 版本模型

本系统采用双层版本模型：

1. **App Version**：CLI 发布版本，继续使用 semver，例如 `1.8.0`。
2. **Workspace Schema Version**：整个本地工作区的数据格式版本，使用单调递增整数。

设计原则：

- 发布版本只服务于用户可见发布与诊断。
- 本地状态升级只看 `workspace_schema_version`，不关心用户中间装过哪些二进制版本。
- 新二进制必须内置从历史 schema 到当前 schema 的全部迁移链。

当前实现版本：

- `latest workspace schema version = 2`
- `workspace schema 0` 表示：
  - 已存在 awiki-cli 本地状态，但尚未接入统一升级元数据，或
  - 仅存在 Python v1 legacy source，或
  - 仅存在未显式版本化的早期 config / DB
- `workspace schema 1` 表示：
  - 已完成 config / identity store / SQLite 的统一升级编排
- `workspace schema 2` 表示：
  - 在 schema 1 基础上，已对旧 `awiki-agent-id-message` skill 做 best-effort 清理：
    - 停止并卸载旧 websocket listener service
    - 删除旧 skill 安装目录
    - 清理旧 OpenClaw `HEARTBEAT.md` 里的 legacy awiki section

---

## 3. 管理对象与路径

### 3.1 Live workspace

awiki-cli 的 live workspace 固定使用单根目录模型：

- config: `~/.awiki-cli/config.yaml`
- identities: `~/.awiki-cli/identities/`
- sqlite: `~/.awiki-cli/data/awiki-cli.db`
- cache: `~/.awiki-cli/cache/`
- runtime: `~/.awiki-cli/runtime/`
- workspace home: `~/.awiki-cli/`

其中：

- `~/.awiki-cli/` 作为跨平台固定的 workspace home，统一承载 config / identities / data / cache / runtime / upgrade

### 3.2 Upgrade metadata

统一升级元数据位于 **workspace home** 下的 `upgrade/` 目录：

- `~/.awiki-cli/upgrade/meta.json`
- `~/.awiki-cli/upgrade/upgrade_journal.json`
- `~/.awiki-cli/upgrade/upgrade.lock`
- `~/.awiki-cli/upgrade/backups/<timestamp>/`

跨平台约定：

- macOS / Linux：`~/.awiki-cli/`
- Windows：`%USERPROFILE%\\.awiki-cli\\`

### 3.3 Legacy source

Python v1 目录只作为 **legacy source**，不再作为 awiki-cli live workspace：

- legacy credentials: `~/.openclaw/credentials/awiki-agent-id-message/`
- legacy data root: `~/.openclaw/workspace/data/awiki-agent-id-message/`
- legacy settings: `<legacy-data>/config/settings.json`
- legacy sqlite: `<legacy-data>/database/awiki.db`

如果同时存在 awiki-cli live workspace 与 legacy source，则默认 **当前 live workspace 优先**，legacy 仅用于诊断与显式导入候选，不自动合并。

---

## 4. 元数据与局部版本

### 4.1 `meta.json`

`meta.json` 记录整个 workspace 的升级状态：

```json
{
  "workspace_schema_version": 2,
  "app_version": "1.8.0",
  "updated_at": "2026-04-10T10:00:00Z",
  "last_upgrade_id": "20260410T100000Z",
  "last_backup_dir": "/home/me/.awiki-cli/upgrade/backups/20260410T100000Z"
}
```

### 4.2 `upgrade_journal.json`

`upgrade_journal.json` 用于中断恢复与诊断：

```json
{
  "upgrade_id": "20260410T100000Z",
  "from_version": 1,
  "to_version": 2,
  "current_step": "workspace_1_to_2_remove_legacy_skill_and_listener",
  "phase": "applying",
  "backup_dir": "/home/me/.awiki-cli/upgrade/backups/20260410T100000Z",
  "started_at": "2026-04-10T10:00:00Z",
  "app_version": "1.8.0"
}
```

### 4.3 工件局部版本

- `config.yaml` 顶层新增 `schema_version`
- identity store 继续使用 `index.json.schema_version`
- SQLite 继续使用 `PRAGMA user_version`

说明：

- `meta.json` 负责整个 workspace 的全局入口判断
- 工件局部版本只用于局部自检与中断恢复，不单独驱动全局升级顺序

---

## 5. 升级入口与执行时机

升级入口固定为：

- `internal/upgrade.UpgradeIfNeeded(ctx, resolved, appVersion)`

触发时机：

- 在需要访问本地状态的 CLI 服务初始化前执行
- 例如 identity / message / runtime / debug store 等命令
- `doctor`、`config show` 等命令可以只做 inspection，不强制触发升级

设计约束：

- config / identity / SQLite 模块不得在自身 load/open 过程中偷偷自升级
- 所有状态变更只能通过统一 upgrade runner 编排

---

## 6. 检测逻辑

升级器按以下顺序检测当前状态：

1. 加载 `meta.json`
2. 加载 `upgrade_journal.json`
3. 检测 awiki-cli live workspace：
   - `config.yaml`
   - `identities/index.json`
   - `awiki-cli.db`
4. 检测 Python v1 legacy source：
   - legacy credentials / indexed layout / flat layout
   - legacy sqlite
   - legacy `settings.json`

检测结果规则：

- 若 `meta.json` 存在，则以其 `workspace_schema_version` 为准
- 若 `meta.json` 不存在，但检测到 awiki-cli live workspace 或 legacy source，则视为 `workspace schema 0`
- 若完全没有本地状态，也没有 legacy source，则视为空工作区，不触发升级写入

---

## 7. 锁、备份与恢复

### 7.1 全局锁

升级前必须先拿锁：

- 锁锚点文件：`upgrade.lock`
- 真正互斥由 OS 级 advisory lock 承担：
  - macOS / Linux / Unix：`flock`
  - Windows：`LockFileEx`
- `upgrade.lock` 可以常驻；文件存在本身不代表升级未完成，也不代表当前有升级进程。
- 锁内容仅作为诊断 metadata，记录 `lock_scheme / pid / app_version / started_at / hostname / executable`。
- 新锁格式使用 `lock_scheme = "os_file_lock_v1"`。
- 旧格式残留锁只用于兼容判断：
  - JSON 损坏、PID 不存在、PID 非法，或 `started_at` 超过兼容 TTL 时视为 stale，可覆盖后重试。
  - PID 仍存在且 `started_at` 很新时，保守视为旧版本升级仍在运行，避免并发修改 workspace。
- 解锁时只释放 OS lock 并关闭文件句柄，不删除 `upgrade.lock`，避免 inode race。

### 7.2 备份

升级器统一创建备份目录：

- `backups/<upgrade-id>/`

备份范围：

- `config.yaml`
- `identities/`
- `awiki-cli.db`
- `meta.json`
- `upgrade_journal.json`

SQLite 备份要求：

- 不依赖裸拷贝主库文件
- 使用 SQLite 一致性备份方式，当前实现使用 `VACUUM INTO`

### 7.3 恢复原则

- 首选 **继续执行**，而不是 down migration
- 若升级中途崩溃，下次启动先读取 journal，再依赖工件局部状态决定跳过已完成部分
- 若用户需要回退，依赖 backup snapshot 恢复，不依赖通用反向迁移链

---

## 8. Migration 抽象

统一接口：

```go
type Migration interface {
    From() int
    To() int
    Name() string
    IsDone(ctx context.Context, uc *UpgradeContext) (bool, error)
    Apply(ctx context.Context, uc *UpgradeContext) error
    Validate(ctx context.Context, uc *UpgradeContext) error
}
```

统一规则：

- 只允许 `N -> N+1` 链式迁移
- 不允许实现 `0 -> latest` 的超级迁移
- 每步迁移可以同时修改 config / identity / db
- 每步迁移必须幂等
- 每完成一步即写 `meta.workspace_schema_version`

---

## 9. 首版迁移：`0 -> 1`

首版 `workspace 0 -> 1` 的职责：

1. 若当前已存在 canonical `config.yaml`，则补写/规范化 `schema_version: 1`
2. 若当前工作区仍保留旧版 awiki-cli 的 `config.json`，则在首次升级时迁移到 canonical `config.yaml`
3. 若当前没有 awiki-cli config，但存在 legacy `settings.json`，则从中导入：
   - `services.service_base_url`
   - `services.did_domain`
   - `runtime.mode`（由 legacy `message_transport.receive_mode` 推导）
4. 若当前没有 awiki-cli live workspace，但存在 legacy identities，则导入到 awiki-cli 单根目录 identity store
5. 若当前没有 awiki-cli live workspace，但存在 legacy sqlite，则导入到 `awiki-cli.db`
6. 若本次从 Python v1 导入的 identity 中存在 handle 形态的 `k1_...` DID，则自动调用 `POST /did-auth/rpc` 的内部方法 `replace_did`，把其替换为新的 `e1_...` DID，并同步重绑本地 SQLite `owner_did`
7. 校验 config schema 与 SQLite schema 正确后，写入 `meta.json`

明确不做：

- 不删除 legacy source
- 不自动合并已存在的 awiki-cli workspace 与 legacy source
- 不在本阶段迁移 listener 私有路由细节

`replace_did` 自动迁移策略：

- 只针对本次从 Python v1 导入的 identity 执行
- 只对 handle 形态的 `k1_...` DID 执行；非 handle DID 跳过并记录 warning
- 迁移循环必须覆盖本次导入的所有 handle identity，不能只处理 default
- 调用 `replace_did` 前必须先把旧 identity 目录备份到 `.legacy-backup/replace-did/`，包括旧 DID document 与旧私钥材料
- 认证继续使用旧 DID 的现有凭证（Bearer / DID 鉴权链路）
- 若单个 identity 替换失败，不中断整次 workspace upgrade；warning 会落到 `meta.json`

---

## 10. 后续迁移：`1 -> 2` 与 `2 -> 3`

`workspace 1 -> 2` 的职责：

- 删除 legacy skill 安装目录
- 停止并清理 legacy websocket listener 服务
- 移除 legacy heartbeat 注入片段

`workspace 2 -> 3` 的职责：

- 针对已经完成旧版本迁移的既有 workspace，扫描当前 identity store 中的全部 identities
- 对仍为 handle 形态 `k1_...` DID 的 identity 自动调用 `replace_did` 换绑为 `e1_...` DID
- 替换前同样先备份旧 identity 目录到 `.legacy-backup/replace-did/`；成功后同步执行本地 SQLite `owner_did` rebind；单个 identity 失败仍只记录 warning，不阻断 workspace upgrade

---

## 11. 校验与健康检查

每步迁移后至少做：

- config 可按最新结构读取
- 若 SQLite 存在，则 `PRAGMA user_version == store.SchemaVersion`
- `PRAGMA integrity_check`
- `PRAGMA foreign_key_check`
- 若本次发生 legacy identity 导入，则至少存在一个可列出的 identity

`doctor` 应额外暴露：

- 当前 workspace schema 来源
- `meta.json` 内容
- 是否存在 `upgrade_journal.json`
- 是否仍检测到 legacy source

---

## 12. 当前实现边界

当前落地实现包含：

- `internal/upgrade` 统一入口
- `meta / journal / lock / backup / detection`
- 真实迁移 `0 -> 1`、`1 -> 2`、`2 -> 3`
- 状态型 CLI 命令在本地状态初始化前触发升级检查
- `doctor` / `config show` 可检查升级元数据

后续阶段可继续演进：

- listener runtime 私有状态纳入统一备份
- 更细粒度的 migration phase 持久化
- 显式 restore 命令与更完整的升级诊断输出
