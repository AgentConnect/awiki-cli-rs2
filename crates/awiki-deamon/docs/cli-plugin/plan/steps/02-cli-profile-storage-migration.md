# Step 02：CLI profile 存储与 legacy 迁移

主 Plan：[../plan.md](../plan.md)  
Step index：02  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 待记录 |
| Completed | 待记录 |
| Commit | 待记录 |
| Review evidence | 待记录 |
| Verification evidence | 待记录 |
| Next action | 新增 CLI profile schema、typed API 和 migration tests |

## 2. 目标

- 结果：新增 `generic-cli` 插件内部 profile 存储，保存 `driver_id`、driver config、recipient policy 和默认 workspace/sandbox 配置。
- 用户 / 系统可见行为：旧库中 `runtime.cli.*` 可被迁移或 alias 成 `generic-cli + driver_id`；新 schema 支持后续 Codex 创建路径。
- 非目标：不接真实 `runtime.agent.create` 写入，不启动 Codex，不实现 handle resolve。
- 完成标准：schema version bump、迁移测试、typed state API 和 legacy 兼容策略全部落地。

## 3. 设计方法

- 设计边界：`runtime_profile` 继续保存 daemon core plugin type；CLI driver 细节进入 `cli_runtime_profile`。
- 核心决策：`cli_runtime_profile.runtime_profile_id` 与 core `runtime_profile.runtime_profile_id` 一一对应。
- 契约 / API / 数据流：建议新增结构：

```rust
pub struct CliRuntimeProfileRecord {
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub binary_path: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
    pub auth_mode: Option<String>,
    pub default_model: Option<String>,
    pub default_sandbox: Option<String>,
    pub default_workspace_mode: WorkspaceMode,
    pub recipient_policy_json: serde_json::Value,
    pub driver_config_json: serde_json::Value,
    pub status: String,
}
```

- 兼容性：legacy `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 迁移为 core `runtime_plugin_id=generic-cli`，并补写 `cli_runtime_profile.driver_id`。
- 迁移策略：schema migration 必须可重复执行；对无法可靠推断 driver 的旧 profile，保留 legacy value 并记录 migration audit 或返回明确错误。
- 风险控制：migration 不得改变 Hermes/OpenClaw/native runtime profile。

## 4. 实现方法

1. 将 `DAEMON_SCHEMA_VERSION` 从当前版本递增。
2. 新增 `cli_runtime_profile` 表，字段至少包含设计文档中的 `driver_id`、`recipient_policy_json`、`driver_config_json`、`status`、时间戳。
3. 可选新增 `cli_driver_run` 表骨架，若 Step 07 再补字段，应在本步骤只建立最小 schema 或延后到 Step 07。
4. 实现 `upsert_cli_runtime_profile`、`load_cli_runtime_profile`、`list_cli_runtime_profiles`。
5. 实现 legacy migration：
   - `runtime.cli.codex` -> `runtime_plugin_id=generic-cli`、`driver_id=codex`
   - `runtime.cli.claude-code` -> `runtime_plugin_id=generic-cli`、`driver_id=claude-code`
   - `runtime.cli.gemini-cli` -> `runtime_plugin_id=generic-cli`、`driver_id=gemini`
6. 增加 migration tests：新库建表、旧库字段补齐、legacy profile 转换、Hermes 不受影响。
7. 确保 debug/status 输出不打印 secret、registration token 或 runtime token。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | schema、migration、typed API | 需保持旧库可升级 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/mod.rs` | 如需新增 CLI profile public structs，可放这里或新模块 | 避免 daemon core 过度了解 driver internals |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/*` | 如新增 profile/policy 类型，可放 plugin 模块 | 推荐按 plugin ownership 放置 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/state_bootstrap.rs` | schema version 和表存在性测试 |  |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/*` | 新增 migration / legacy alias 测试 | 文件名由实现选择 |

## 6. 依赖

- 前置步骤：Step 01。
- 外部文档或决策：`generic_cli_runtime_plugin_design.md` 的数据模型章节。
- 环境前提：可创建临时 SQLite daemon db。

## 7. 验收标准

- [ ] 新初始化 daemon db 包含 `cli_runtime_profile`。
- [ ] CLI profile 记录可 upsert/load，`driver_id` 必须非空且只接受当前支持值或明确的 future driver 值。
- [ ] legacy `runtime.cli.*` 可迁移或读取 alias，且新 core profile 变为 `generic-cli`。
- [ ] Hermes `runtime.hermes`、OpenClaw/native plugin type 不被 CLI migration 改写。
- [ ] recipient policy 默认 deny 或 controller-only 的语义明确，不默认全网放开。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| State tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test state_bootstrap --locked` | schema 初始化通过。 |
| Daemon tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | 全 crate 测试通过。 |
| Migration grep | `cd codex-plugin-cli-rs2 && rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini" crates/awiki-deamon/src crates/awiki-deamon/tests` | 只允许 migration/legacy 兼容命中。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：schema/API/tests 完成后、commit 前。
- Review 重点：schema migration 可重复、legacy data 不丢失、policy 默认值安全、Hermes 不受影响、typed API 不泄漏 driver internals 到 `im-core`。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待记录 |  |
| 已修复问题 | 待记录 |  |
| 剩余风险 | 待记录 |  |
| 新增或缺失测试 | 待记录 |  |
| 已更新或缺失文档 | 待记录 |  |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：state schema、CLI profile record/API、migration tests。
- Commit 信息建议：`daemon: add cli runtime profile storage`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：schema migration 一次性改写旧数据后难以回退。缓解：先用旧库 fixture 测试；migration 只改已知 legacy values。
- 假设：SQLite 单库继续承载 daemon core 和 plugin-owned tables。
- 回滚：回退 commit 后新 schema 不再可用；若已升级本地测试 db，执行者应使用临时 state root 重建。
