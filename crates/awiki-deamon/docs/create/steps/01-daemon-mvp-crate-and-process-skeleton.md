# 步骤 01：daemon MVP crate 与进程骨架

主计划：[../plan.md](../plan.md)
步骤编号：01
状态：草稿

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待开始 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 待定 |
| 完成时间 | 待定 |
| 提交 | 待定 |
| 审查证据 | 待定 |
| 验证证据 | 待定 |
| 下一步 | 创建 daemon crate 骨架。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：`crates/awiki-deamon` 成为 Rust crate 和 workspace member，具备最小 daemon 进程、配置加载、状态根目录、`daemon.db` schema 和 `im-core` 初始化。
- 可见行为：开发者可以前台启动 daemon、查看状态、加载本地配置并初始化 state，不需要先运行 runtime 插件。
- 非目标：不实现完整 daemon agent 注册、本地 RPC 安全、CLI runtime 插件或 installer。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `Cargo.toml` | 添加 `crates/awiki-deamon` workspace member。 | 目录名按用户路径保留。 |
| `crates/awiki-deamon/Cargo.toml` | 新建 daemon crate metadata 和依赖。 | 依赖 `im-core`，不依赖 `awiki-cli`。 |
| `crates/awiki-deamon/src/main.rs` | 最小 binary 入口。 | 前台启动优先。 |
| `crates/awiki-deamon/src/config.rs` | 配置加载与校验。 | 显式路径，不读取 CLI config。 |
| `crates/awiki-deamon/src/state/` | 状态根目录和 SQLite 初始化。 | 首版一个 `daemon.db`。 |
| `crates/awiki-deamon/src/im_core_adapter.rs` | 基于 daemon config 构造 `ImCore` / `ImClient`。 | 复用 SDK 边界。 |
| `crates/awiki-deamon/docs/` | daemon local dev 和 state docs。 | 链接架构文档和本计划。 |
| `crates/awiki-deamon/tests/` | config/state 初始化冒烟验证 tests。 | 尽量不依赖网络。 |

## 4. 依赖

- 前置步骤：无。
- 外部文档：daemon 架构文档、SDK refactor 文档。
- 环境前提：Rust workspace 能构建。

## 5. 核心设计

crate 边界：

- daemon 是 `crates/awiki-deamon` 下的独立 executable 和 library modules。
- daemon 依赖 `im-core`。
- `awiki-cli` 不依赖 daemon。
- daemon 不 import `crates/awiki-cli/src/*`。
- daemon 在自己的状态根目录下维护 runtime host 状态；identity/auth/message state 优先复用或兼容 `im-core` 路径。

最小 `daemon.db` 表：

- `agent_definition`
- `runtime_profile`
- `workspace_binding`
- `runtime_run`
- `runtime_rpc_tokens` 占位表，步骤 02 填充行为。
- `audit_log`
- `schema_migrations`

首版 schema 可以保持最小，但必须包含后续步骤需要的 `agent_did`、`runtime_profile_id`、`runtime_plugin_id`、`workspace_id`、timestamps 和 status 字段。

## 6. 实施指引

1. 创建 crate 骨架并加入 workspace。
2. 增加最小命令：
   - `awiki-deamon foreground`
   - `awiki-deamon status` 或等价诊断命令
   - `awiki-deamon init-state`
3. 增加配置模型：
   - 状态根目录
   - `im-core` endpoints
   - identity selector
   - local socket path 占位
   - logging/audit path
4. 增加 `daemon.db` 初始化和 migration table。
5. 增加 `im-core` adapter，从显式路径构造 `ImCore`。
6. 增加 config parsing、invalid path、DB 初始化测试。
7. 增加本地前台启动和 state layout 文档。

## 7. 验收标准

- [ ] workspace 包含 `crates/awiki-deamon` 后能构建。
- [ ] daemon crate 依赖 `im-core`，不依赖 `awiki-cli`。
- [ ] foreground/init-state/status 命令能作用于临时状态根目录。
- [ ] `daemon.db` 初始化预期表。
- [ ] 配置校验能拒绝不安全或缺失路径。
- [ ] 测试覆盖 config 和 DB 初始化。
- [ ] 审查发现已修复或明确记录。
- [ ] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 格式通过。 |
| workspace 测试 | `cargo test --workspace --locked` | 既有和 daemon 测试通过。 |
| daemon 冒烟验证 | `cargo run -p awiki-deamon -- init-state --state-root <tmp>` 或实际命令 | 状态根目录和 DB 创建成功。 |
| 依赖边界 | `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部。 |
| 文档 | `git diff --check -- crates/awiki-deamon` | diff 干净。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查 crate 边界、路径处理、SQLite schema、config defaults、错误处理、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待定 | 待定 |
| 已修复 | 待定 | 待定 |
| 残余风险 | 待定 | 待定 |
| 测试缺口 | 待定 | 待定 |
| 文档缺口 | 待定 | 待定 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：daemon crate 骨架、workspace member、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`daemon: add mvp process skeleton`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：crate 名称拼写可能成为公开 API；state layout 可能与 `im-core` 路径冲突。
- 回滚：crate 保持私有，发布前如需改名再由用户确认。
- 后续：步骤 02 填充本地 RPC 安全和 token 表行为。
