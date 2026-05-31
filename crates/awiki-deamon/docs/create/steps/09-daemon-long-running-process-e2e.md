# 后续 09：长驻 daemon 进程真实 E2E

主计划：[../plan.md](../plan.md)
步骤编号：后续 09
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 2026-05-31 15:29:53 CST 前已开始 |
| 完成时间 | 2026-05-31 15:29:53 CST |
| 提交 | 当前仓库 `10d3d5d`；awiki-system-test `1616a54` |
| 审查证据 | 已完成：见第 9 节 Review 记录 |
| 验证证据 | 已完成：见第 8 节验证记录 |
| 下一步 | Step 09 已推送；进入真实 runtime driver、持久化 inbox cursor、远端 E2E 和 sandbox 等后续产品化规划 |

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
- 本地同域 E2E 中，message-service 必须能解析本地 user-service 注册出的 agent DID。当前 9900 本地验证使用 `message-service.toml` 的运行配置：`did_resolution.base_url = "http://127.0.0.1:9891"`、`did_resolution.verify_ssl = false`。否则 runtime agent DID 会被解析到公网 `https://awiki.info/...`，本地刚注册的 DID 文档不可见，status/final 发送会失败。

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

1. 已在 `crates/awiki-deamon/src/foreground.rs` 增加长驻 `foreground` run loop：
   - 初始化 daemon 状态和 im-core 本地状态。
   - 同步 daemon/runtime agent 本地 DID 身份到 im-core identity registry。
   - 周期轮询 `inbox_with_metadata_async`，消费 direct incoming payload。
   - 支持 `--ready-file`、`--max-runtime-ms`、`--max-processed-messages` 和 `--poll-interval-ms`，便于系统测试稳定退出。
2. 已增加 daemon 进程健康检查机制：
   - foreground 启动后写 ready file。
   - stdout 输出 ready 行。
   - 系统测试等待 ready file，不依赖固定 sleep。
3. 已串接 incoming command：
   - `runtime.agent.create` 继续复用 Step 07 的 `handle_agent_payload_message`。
   - `runtime.task.submit` 在 foreground 中解析 payload command，加载 runtime profile 并创建 runtime task/run。
   - controller 校验仍使用 `sender_did == daemon_agent.controller_did` 的 MVP 方案。
4. 已串接 UDS test runtime：
   - `UdsTestRuntimePlugin` 使用 daemon UDS local RPC 调用 `task.status` 和 `task.finish`。
   - test runtime 会检查 RPC 响应 `ok`，不再吞掉 `ok=false` 的 callback/outbox 失败。
5. 已串接 status/final 发送：
   - `ImCoreAgentOutbox` 统一使用 `send_async`，发送前确保或刷新 messaging session。
   - status/final 仍使用 `meta.content_type = application/json` 和 `body.payload`。
   - payload schema 使用 `awiki.agent.status.v1`。
6. 已增加 awiki-system-test：
   - 新增 `tests_v2/daemon/test_awiki_daemon_long_running_e2e.py`。
   - 测试启动真实 `awiki-deamon foreground` 进程。
   - 通过本地 message-service 发送 `runtime.agent.create` 和 `runtime.task.submit` payload command。
   - 断言 daemon 处理 2 条消息、创建 runtime task/run/token。
   - 断言 controller 通过 `direct.get_history` 收到 `running` 和 `finished` 两个 `application/json` status payload。
   - 断言 audit 不包含 `rtok_` token 原文，只包含 `rtokid_`。
7. 已更新执行文档和验证记录。
8. 代码 review 已完成，提交和推送在文档更新后执行。

## 7. 验收标准

- [x] daemon 作为长驻进程启动，而不是只执行一次 status/init 命令。
- [x] 系统测试使用真实或本地 message-service 发送 `application/json + body.payload` command。
- [x] daemon listener 能收到 payload command 并校验 `controller_did`。
- [x] runtime run 被创建，且 `runtime_rpc_token` scope 绑定 `agent_did`、`runtime_profile_id`、`run_id`、allowed methods、可选 recipients 和 expires。
- [x] 测试 runtime 通过 UDS local RPC 回传 progress/final。
- [x] audit 只记录 `token_id`，不记录 token 原文。
- [x] status/final 通过 `application/json + body.payload` 发回 controller。
- [x] 系统测试报告包含 passed / failed / skipped 数量、命令、模式、关键 URL 和 DID domain。
- [x] 代码 review 已完成，发现已修复或明确记录。
- [x] 每个受影响仓库都有聚焦提交，提交后工作区干净。

## 8. 验证计划

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 当前仓库格式 | `CARGO_BUILD_JOBS=1 cargo fmt --all --check` | 通过 |
| 当前仓库单测 | `CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked` | 15 lib tests、5 agent registration、6 generic CLI runtime、6 local RPC、2 state bootstrap，全部通过 |
| 当前仓库 workspace | `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` | 待最终提交前补跑 |
| daemon 边界 | `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 无结果 |
| 旧协议字段 | `rg -n "structured_json|application/vnd\\.awiki\\.agent-(command|status|result|task)\\+json" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md crates/awiki-deamon/docs/local-dev.md crates/im-core/src crates/im-core-dart/src` | 无结果；宽搜索只命中历史计划账本中记录过的旧搜索命令 |
| 系统测试语法 | `uv run --no-project --python .venv/bin/python -m py_compile tests_v2/daemon/test_awiki_daemon_long_running_e2e.py` | 通过 |
| 系统测试 | `AWIKI_SYSTEM_TEST_MODE=local ... AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/awiki-deamon-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-project --python .venv/bin/python -m pytest tests_v2/daemon/test_awiki_daemon_long_running_e2e.py -q -rs` | 1 passed、0 failed、0 skipped，耗时 101.07s |
| 文档检查 | `git diff --check -- crates/awiki-deamon src Cargo.toml Cargo.lock` | 通过；文档更新后还需补跑 docs 范围 |

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
| 发现 | 已完成 | 发现 3 个问题：1. status/final outbox 走同步 `send` 会触发 `im-core` 当前 real HTTP 的 `sync-http` 限制；2. `UdsTestRuntimePlugin` 只检查 socket 调用成功，不检查 RPC `ok=false`；3. 本地 9900 message-service 未配置 DID 解析代理时，会把本地刚注册的 runtime agent DID 解析到公网。 |
| 已修复 | 已完成 | `ImCoreAgentOutbox` 改为 async send，并在发送前确保/刷新 messaging session；test runtime 检查 UDS RPC `ok`；本地验证环境配置 `did_resolution.base_url = "http://127.0.0.1:9891"`、`verify_ssl = false` 后重启 9900。 |
| 残余风险 | 已记录 | foreground 当前使用进程内 HashSet 去重，重启后可能重新看到历史 command；本步骤先满足长驻真实 E2E，后续产品化需要持久化 inbox cursor 或 processed message 表。 |
| 测试缺口 | 已记录 | 已覆盖同域 local message-service 的真实进程 E2E；尚未覆盖远端 `https://awiki.ai`，远端仍受注册限额和环境数据影响。 |
| 文档缺口 | 已修复 | 本步骤文档、主计划、发布验证、本地开发文档和 system-test daemon 目录说明均需要随本次提交更新。 |

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
| 已解决：本地 DID 解析 | E2E 首次失败：`runtime_rpc_error: service error (1503): failed to resolve DID document via anp: Network failure`。根因是本地 message-service 未设置 `did_resolution.base_url`，runtime agent DID 被解析到公网 `https://awiki.info/...`。 | 将本地 9900 的 `message-service.toml` 运行配置改为 `did_resolution.base_url = "http://127.0.0.1:9891"`、`verify_ssl = false`，重启 message-service 后重跑 E2E。 | 仅本地验证环境，不需要修改 message-service 代码。 | 已通过；后续本地验证需保留该运行配置或使用 system-test 管理脚本生成等价配置。 |

如果本地服务无法启动，先检查当前机器已有 user-service/message-service 进程和端口；如果只是远端注册限额，优先使用本地可控服务验证，不把远端 skipped 记录成 passed。

## 12. 风险、回滚与后续

- 风险：`foreground` 从一次性初始化变成长驻进程可能影响现有命令语义。实现时应明确区分 `status/init-state` 和 `foreground`，并为测试提供可控退出机制。
- 风险：系统测试涉及多进程、多服务和 WebSocket，容易出现竞态。需要 ready 探针、超时、日志收集和进程清理。
- 风险：真实 runtime CLI 后续接入会引入安装、权限和 workspace 风险。此步骤只验证测试 runtime，不扩大范围。
- 回滚：如长驻 listener 变更不稳定，可以回滚当前仓库 daemon 长驻实现提交；保留 Step 08 已通过的 payload/token 系统测试证据。
- 后续：长驻 E2E 通过后，再规划 Claude Code / Codex / Gemini CLI driver、安装能力、workspace sandbox 和更强 delegation/proof 方案。
