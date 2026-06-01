# Step 01：Runtime resolution 与创建契约

主 Plan：[../plan.md](../plan.md)  
Step index：01  
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
| Next action | 实现 `RuntimeResolution` 契约和 focused tests |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：建立 `runtime.agent.create` 可使用的结构化 runtime 解析契约，让 CLI 家族 alias 解析为 `runtime_plugin_id=generic-cli` 和对应 `driver_id`。
- 用户 / 系统可见行为：后续创建 `runtime=codex|codex-cli|claude-code|gemini|gemini-cli` 时，不再把这些值当作独立 runtime plugin type。
- 非目标：本步骤不写 `cli_runtime_profile` 表，不改变真实注册写入路径，不实现 Codex driver。
- 完成标准：解析函数和测试明确覆盖 generic-cli、Codex、Claude Code、Gemini、Hermes、OpenClaw、未知 native plugin type 和非法空 runtime。

## 3. 设计方法

- 设计边界：`generic-cli` 是 runtime plugin type；`driver_id` 是 CLI plugin 内部路由字段；消息外部路由仍按 DID。
- 核心决策：新增或替换当前返回字符串的 `runtime_plugin_id(runtime)`，提供结构化结果。
- 契约 / API / 数据流：建议引入：

```rust
pub struct RuntimeResolution {
    pub runtime_plugin_id: String,
    pub driver_id: Option<String>,
    pub legacy_runtime_plugin_id: Option<String>,
    pub defaulted_driver_id: bool,
}
```

- 兼容性：`runtime_plugin_id("hermes") == "runtime.hermes"` 的现有测试继续通过；旧 helper 可作为 native-only wrapper 保留，但新创建路径必须用 `RuntimeResolution`。
- 迁移策略：本步骤只定义解析契约；旧数据迁移在 Step 02。
- 风险控制：测试必须防止 `runtime=codex` 再返回 `runtime.cli.codex`。

## 4. 实现方法

1. 在 `codex-plugin-cli-rs2/crates/awiki-deamon/src/agent/mod.rs` 增加 `RuntimeResolution` 和 `resolve_runtime(runtime, driver_id_override)`。
2. 解析规则：
   - `generic-cli`：`runtime_plugin_id=generic-cli`，`driver_id=driver_id_override.unwrap_or("codex")`，默认时 `defaulted_driver_id=true`。
   - `codex|codex-cli`：`runtime_plugin_id=generic-cli`，`driver_id=codex`。
   - `claude-code`：`runtime_plugin_id=generic-cli`，`driver_id=claude-code`。
   - `gemini|gemini-cli`：`runtime_plugin_id=generic-cli`，`driver_id=gemini`。
   - `hermes`：`runtime_plugin_id=runtime.hermes`，`driver_id=None`。
   - `openclaw`：`runtime_plugin_id=runtime.openclaw`，`driver_id=None`。
   - 其他非空字符串：按 native/plugin-specific runtime type 处理，`driver_id=None`。
3. 扩展 `RuntimeAgentCreateArgs` 解析字段：`driver_id`、`driver_config`、`recipient_policy`，但本步骤只完成 serde contract 和输入校验，不持久化。
4. 增加单元测试或集成测试覆盖所有 alias。
5. 更新现有 Hermes contract 测试，让它确认 Hermes native runtime 不受 CLI family 约束影响。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/agent/mod.rs` | 新增 runtime resolution 结构和解析函数 | 保留旧 helper 时需标注历史命名 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/commands/mod.rs` | 扩展 create args serde 字段 | 暂不持久化 driver profile |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/hermes_contracts.rs` | 更新 Hermes/native runtime contract | Hermes 仍为 `runtime.hermes` |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/agent_registration_management.rs` | 可新增解析契约测试或后续 Step 03 更新 | 本步骤尽量保持 focused |

## 6. 依赖

- 前置步骤：无。
- 外部文档或决策：`codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md`。
- 环境前提：可运行本仓 Rust tests。

## 7. 验收标准

- [ ] `runtime=codex|codex-cli` 解析为 `generic-cli + driver_id=codex`。
- [ ] `runtime=claude-code` 解析为 `generic-cli + driver_id=claude-code`。
- [ ] `runtime=gemini|gemini-cli` 解析为 `generic-cli + driver_id=gemini`。
- [ ] `runtime=generic-cli` 支持显式 `driver_id`，未传时默认 `codex` 并可审计 defaulted 标记。
- [ ] Hermes/OpenClaw/native runtime 不产生 CLI `driver_id`。
- [ ] 新测试证明 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 不再是新建 alias 的解析结果。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Focused test | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test hermes_contracts --locked` | Hermes runtime plugin type 契约通过。 |
| Agent tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test agent_registration_management --locked` | 若本步骤触碰 create args，该测试通过或预期更新。 |
| Legacy grep | `cd codex-plugin-cli-rs2 && rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini" crates/awiki-deamon/src crates/awiki-deamon/tests` | 只允许 legacy 兼容测试或注释命中；新解析正向测试不得期望这些值。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：CLI family 单 plugin type 语义、native runtime 兼容、默认 `driver_id` 是否可审计、错误消息是否清晰、测试是否覆盖旧 bug。
- Review 结论必须在 commit 前记录；必要问题必须修复，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待记录 |  |
| 已修复问题 | 待记录 |  |
| 剩余风险 | 待记录 |  |
| 新增或缺失测试 | 待记录 |  |
| 已更新或缺失文档 | 待记录 |  |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：只包含 runtime resolution、create args serde contract 和直接相关测试。
- Commit 信息建议：`daemon: resolve cli runtimes to generic cli driver`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：旧 helper 名叫 `runtime_plugin_id`，容易继续被误用。缓解：在 Step 01 加测试和注释；Step 03 禁止 create 路径调用旧 helper。
- 假设：`generic-cli` 未传 `driver_id` 时默认 `codex` 可以接受；如果用户要求强制显式，则更新本 Plan。
- 回滚：回退本步骤 commit 可恢复旧解析；但后续步骤不能在旧解析上继续。
