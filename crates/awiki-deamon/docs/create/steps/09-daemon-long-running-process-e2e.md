# 后续 09：长驻 daemon 进程真实 E2E

主计划：[../plan.md](../plan.md)
步骤编号：后续 09
状态：待执行

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待执行 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 待执行 |
| 完成时间 | 待执行 |
| 提交 | 待执行；实现时每个受影响仓库都需要聚焦提交 |
| 审查证据 | 待执行 |
| 验证证据 | 待执行 |
| 下一步 | 先实现 daemon 长驻 listener / message adapter 测试能力，再落系统测试 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：证明 daemon 不是只在 crate 单测和命令式 wrapper 中可运行，而是能作为长驻进程接入真实或本地 message-service，完成 controller command 到 runtime callback 再到 status/final payload 的完整闭环。
- 可见行为：controller 发送 `application/json + body.payload` command；daemon 长驻进程收到并校验 `controller_did`；daemon 创建 runtime run；测试 runtime 通过 UDS local RPC 回传 progress/final；controller 收到 status/final payload。
- 非目标：第一版不接 Claude Code / Codex / Gemini CLI；不引入新的安全 proof；不把 `shared-root` 或 `worktree-per-task` 宣称为强安全边界。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `crates/awiki-deamon/src` | 增加长驻 daemon listener / run loop / message adapter 的最小实现。 | 应继续固定在 `crates/awiki-deamon`，不能依赖 awiki-cli 内部模块。 |
| `crates/awiki-deamon/tests` | 增加进程级或进程模拟 E2E。 | 先用测试 runtime，验证 UDS callback 和 outbox。 |
| `awiki-system-test/tests_v2/daemon` | 增加真实进程系统测试。 | 启动 daemon 进程，使用本地 message-service/user-service。 |
| `awiki-system-test/tests_v2/message_service` | 可复用 payload direct/group fixture。 | 不复制协议细节。 |
| `message-service` | 原则上不改。 | 只在发现投递/WS/历史契约缺口时补充聚焦修复。 |
| `user-service` | 原则上不改。 | 只在 registration token / DID auth 真实链路发现缺口时补充聚焦修复。 |
| `crates/awiki-deamon/docs` | 更新本计划、local-dev、发布验证记录。 | 记录命令、passed/failed/skipped 和残余风险。 |

## 4. 依赖

- Step 08 残余验证已经通过：payload suite 2 passed、registration token suite 2 passed、daemon wrapper 3 passed。
- user-service 需要包含 `bcda176` 或等价修复，确保 DID bearer subject 可映射为内部 user_id。
- message-service 需要包含 payload body 支持提交 `30eecf4` 或等价实现。
- awiki-system-test 需要包含 Step 08 测试提交 `95864c1` 或等价测试夹具。

## 5. 核心设计

### 5.1 目标链路

1. 测试准备 controller DID、daemon agent DID、runtime agent DID 和 registration token。
2. 启动本地 user-service 与 message-service。
3. 启动 `awiki-deamon foreground --state-root <tmp>` 作为长驻进程。
4. controller 通过 message-service 向 daemon agent 发送 `application/json + body.payload` command。
5. daemon listener 收到 incoming payload，解析上层 command。
6. daemon 校验 `sender_did == daemon_agent.controller_did`。
7. daemon 创建 runtime task/run，签发短期 `runtime_rpc_token`。
8. 测试 runtime 通过 UDS local RPC 调 `task.status` 和 `task.finish`。
9. daemon 发送 status/final payload message。
10. controller 通过 WS 或 history/inbox 收到 status/final payload。

### 5.2 第一版实现边界

- runtime 使用测试替身，不接真实外部 CLI。
- 传输使用 `application/json + body.payload`。
- message-service 不理解 daemon schema，只作为不透明 JSON payload 的传输与投递层。
- daemon 状态源仍以 Skill / CLI wrapper / local RPC 为权威；不启用第二条 RuntimeEvent 权威状态通道。
- audit 只记录 `token_id`，不能记录 token 原文。
- 本地 UDS 权限、peer credential、method scope、recipient scope 仍按 Step 02 的实现执行。

## 6. 实施指引

1. 在 `crates/awiki-deamon` 增加可测试的 listener 抽象：
   - `MessageSource`：接收 direct/group incoming。
   - `MessageSink`：发送 status/final payload。
   - 真实实现通过 `im-core` 或现有 SDK public API 接 message-service。
   - 测试实现可直接喂入 incoming payload，但系统测试必须最终启动真实 message-service。
2. 扩展 `foreground`：
   - 当前 `foreground` 只初始化状态并返回状态信息。
   - 后续需要进入长驻 run loop，支持健康状态、优雅退出和测试超时。
   - 可以先增加显式测试命令或环境变量控制运行一次 / 运行到收到一个 command 后退出，便于系统测试稳定收敛。
3. 增加 daemon 进程健康检查机制：
   - 最小方案可以用状态文件、stdout ready 行或 local RPC `rpc.ping`。
   - 系统测试不得只靠固定 sleep。
4. 串接 incoming command 到现有 `handle_agent_payload_message` 与 runtime host：
   - 复用 Step 07 的 parser 和 controller 校验。
   - 复用 Step 03 的 runtime run / local RPC / outbox 逻辑。
5. 串接 status/final 发送：
   - 统一发送 `meta.content_type = application/json`。
   - JSON 对象固定在 `body.payload`。
   - payload 内部使用 `awiki.agent.status.v1` / `awiki.agent.result.v1` 等上层 schema。
6. 增加 awiki-system-test：
   - 启动本地 daemon 进程。
   - 使用本地 user-service/message-service 生成身份和 token。
   - 发送 command，等待 daemon ready。
   - 模拟 runtime callback。
   - 断言 controller 收到 progress/final。
7. 更新文档和执行账本。
8. 完成代码 review、修复发现、聚焦提交并推送。

## 7. 验收标准

- [ ] daemon 作为长驻进程启动，而不是只执行一次 status/init 命令。
- [ ] 系统测试使用真实或本地 message-service 发送 `application/json + body.payload` command。
- [ ] daemon listener 能收到 payload command 并校验 `controller_did`。
- [ ] runtime run 被创建，且 `runtime_rpc_token` scope 绑定 `agent_did`、`runtime_profile_id`、`run_id`、allowed methods、可选 recipients 和 expires。
- [ ] 测试 runtime 通过 UDS local RPC 回传 progress/final。
- [ ] audit 只记录 `token_id`，不记录 token 原文。
- [ ] status/final 通过 `application/json + body.payload` 发回 controller。
- [ ] 系统测试报告包含 passed / failed / skipped 数量、命令、模式、关键 URL 和 DID domain。
- [ ] 代码 review 已完成，发现已修复或明确记录。
- [ ] 每个受影响仓库都有聚焦提交，提交后工作区干净。

## 8. 验证计划

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 当前仓库格式 | `CARGO_BUILD_JOBS=1 cargo fmt --all --check` | 通过 |
| 当前仓库单测 | `CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked` | daemon 单测通过 |
| 当前仓库 workspace | `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` | workspace 通过；如资源限制失败，记录具体失败和聚焦替代验证 |
| daemon 边界 | `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 无结果 |
| 旧协议字段 | 搜索旧结构化字段和旧专用 content type | 产品代码和产品文档不出现旧字段；历史账本命中需说明来源 |
| user-service | `cd ../user-service && uv run python -m pytest tests/app/test_rpc_dispatcher.py tests/app/agent_registration -q` | 相关认证/token 测试通过 |
| message-service | `cd ../message-service && cargo test -p im-direct --locked json_payload && cargo test -p im-group --locked group_incoming_notification_preserves_json_payload_body` | payload 服务端测试通过 |
| 系统测试 | `cd ../awiki-system-test && <daemon long-running e2e command> -q -rs` | 长驻 daemon E2E passed、0 skipped |
| 文档检查 | `git diff --check -- crates/awiki-deamon/docs/create crates/awiki-deamon/docs/local-dev.md` | 通过 |

## 9. Review 要求

实现后、提交前必须做代码 review，重点检查：

- daemon 是否仍只在 `crates/awiki-deamon` 实现。
- daemon 是否没有依赖 awiki-cli 内部模块。
- listener / sink 抽象是否没有把 message-service 业务语义下沉到服务端。
- `controller_did` MVP 校验是否没有被绕过。
- 本地 RPC token 是否不信任请求体身份字段。
- token 原文是否不写日志、不进 audit。
- status/final 是否仍使用 `application/json + body.payload`。
- 系统测试是否能稳定等待进程 ready 和结果，不靠固定 sleep。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待执行 | 待执行 |
| 已修复 | 待执行 | 待执行 |
| 残余风险 | 待执行 | 待执行 |
| 测试缺口 | 待执行 | 待执行 |
| 文档缺口 | 待执行 | 待执行 |

## 10. 提交要求

- 提交时机：实现、验证、review、修复完成后。
- 提交范围：一个仓库一个聚焦提交；跨仓不要混成一个无法回滚的大提交。
- 提交前记录：`git status --short --branch`、纳入文件、关键验证命令。
- 提交后记录：commit hash、push 状态、工作区状态。
- 建议提交信息：
  - 当前仓库：`daemon: add long-running message e2e path`
  - 系统测试：`test: cover daemon long-running message e2e`
  - 子服务如有必要：按实际修复命名。

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待执行 | 待执行 | 待执行 | 当前步骤 / 整体计划 | 待执行 |

如果本地服务无法启动，先检查当前机器已有 user-service/message-service 进程和端口；如果只是远端注册限额，优先使用本地可控服务验证，不把远端 skipped 记录成 passed。

## 12. 风险、回滚与后续

- 风险：`foreground` 从一次性初始化变成长驻进程可能影响现有命令语义。实现时应明确区分 `status/init-state` 和 `foreground`，并为测试提供可控退出机制。
- 风险：系统测试涉及多进程、多服务和 WebSocket，容易出现竞态。需要 ready 探针、超时、日志收集和进程清理。
- 风险：真实 runtime CLI 后续接入会引入安装、权限和 workspace 风险。此步骤只验证测试 runtime，不扩大范围。
- 回滚：如长驻 listener 变更不稳定，可以回滚当前仓库 daemon 长驻实现提交；保留 Step 08 已通过的 payload/token 系统测试证据。
- 后续：长驻 E2E 通过后，再规划 Claude Code / Codex / Gemini CLI driver、安装能力、workspace sandbox 和更强 delegation/proof 方案。
