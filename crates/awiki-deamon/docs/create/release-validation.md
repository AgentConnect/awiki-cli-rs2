# 发布验证记录：daemon 初始化创建后残余系统测试

主计划：[plan.md](plan.md)
记录时间：2026-05-31 09:12 CST
状态：Step 08 残余 payload/token 系统测试已在本地可控服务上通过

## 1. 背景

Step 08 首次执行时，系统测试入口已经补齐，但远端 `https://awiki.ai` 的 user-service 当时返回 502，导致 payload direct/group 和 registration token 两组系统测试只能记录为 skipped，不能作为发布级通过证据。

本轮目标是重跑这些残余测试，并在必要时使用本地服务代码和本地服务重启权限，把 skipped 转换为可解释的 passed / failed 证据。

## 2. 仓库与服务状态

| 项 | 状态 |
|---|---|
| 当前主仓 | `/home/ecs-user/awiki-space/awiki-deamon-cli-rs2`，分支 `feature/release-0526/awiki-deamon` |
| 系统测试仓库 | `/home/ecs-user/awiki-space/awiki-system-test`，分支 `release/0526` |
| user-service | `/home/ecs-user/awiki-space/user-service`，分支 `feature/release-0526/daemon-registration-token-user-service`，本轮修复并推送 `bcda176` |
| message-service | `/home/ecs-user/awiki-space/message-service`，分支 `feature/release-0526/daemon-payload-message-service` |
| 本地 user-service | `http://127.0.0.1:9891`，健康检查 `{"status":"ok"}` |
| 本地 message-service v2 | `http://127.0.0.1:18080`，健康检查返回 service `message-service`、service DID `did:wba:awiki.info` |

## 3. 远端重跑结果

远端配置：

| 配置 | 值 |
|---|---|
| `AWIKI_SYSTEM_TEST_MODE` | `remote` |
| user-service URL | `https://awiki.ai` |
| message-service URL | `https://awiki.ai` |
| WebSocket URL | `wss://awiki.ai/im/ws` |
| DID domain | `awiki.ai` |

执行结果：

| 测试 | 命令 | 通过 | 失败 | 跳过 | 原因 |
|---|---|---:|---:|---:|---|
| payload direct/group | `uv run --no-project --python .venv/bin/python -m pytest tests_v2/message_service/test_payload_local.py -q -rs` | 0 | 0 | 2 | 远端 user-service 不再返回 502，但 DID 注册触发当前 IP 注册数量限制：`Registration limit exceeded for this IP (max 100)` |
| registration token | `uv run --no-project --python .venv/bin/python -m pytest tests_v2/user_service/test_agent_registration_token_local.py -q -rs` | 0 | 0 | 2 | 同上 |
| daemon contract wrapper | `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/awiki-deamon-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-project --python .venv/bin/python -m pytest tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q` | 3 | 0 | 0 | 已通过 |

远端结论：原 502 风险已经变化为注册限额风险。远端环境仍需要在发布环境或 CI 中使用独立测试账号/IP/清理策略补跑，但这不再阻塞本地可控服务的系统验证。

## 4. 本地服务修复

本地 user-service 首次重跑时，`/user-service/agent-registration/rpc` 已进入当前路由，但 `issue_token` 返回 internal error。日志显示 MySQL 报错：

```text
Data too long for column 'issued_to_user_id'
```

根因是 DID WBA 认证签发的 bearer token 使用 DID 作为 JWT subject。`auth_type = user` 的 JSON-RPC dispatcher 原先直接把 subject 当作内部 `user_id`，导致 registration token 把长 DID 写入 `issued_to_user_id`。

修复：

- user-service `src/user_service/app/rpc/dispatcher.py` 对 `auth_type = user` 增加 DID subject 兼容。
- 如果 bearer token subject 是 `did:wba:*`，dispatcher 通过 active DID document 反查内部 `user_id`，并在 `RpcContext.did` 保留 DID。
- 保留传统内部 `user_id` subject 行为。
- 补充 `tests/app/test_rpc_dispatcher.py` 回归测试。
- 更新 `src/user_service/app/rpc/CLAUDE.md`。

验证与提交：

| 项 | 结果 |
|---|---|
| `uv run ruff format src/user_service/app/rpc/dispatcher.py tests/app/test_rpc_dispatcher.py` | 通过，2 files left unchanged |
| `uv run ruff check src/user_service/app/rpc/dispatcher.py tests/app/test_rpc_dispatcher.py` | 通过 |
| `uv run python -m pytest tests/app/test_rpc_dispatcher.py tests/app/agent_registration -q` | 13 passed、0 failed、0 skipped |
| user-service 提交 | `bcda176 user-service: map did bearer subjects for user rpc` |
| push | 已推送到 `origin/feature/release-0526/daemon-registration-token-user-service` |

## 5. 本地系统测试配置

本地系统测试使用可控 user-service 和 message-service：

```bash
AWIKI_SYSTEM_TEST_MODE=local
E2E_USER_SERVICE_URL=http://127.0.0.1:9891
E2E_MESSAGE_SERVICE_URL=http://127.0.0.1:18080
E2E_MESSAGE_SERVICE_WS_URL=ws://127.0.0.1:18080/im/ws
E2E_DID_DOMAIN=awiki.info
E2E_MESSAGE_V2_USER_SERVICE_URL=http://127.0.0.1:9891
E2E_MESSAGE_V2_NODE_A_DOMAIN=awiki.info
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=http://127.0.0.1:18080
E2E_MESSAGE_V2_NODE_A_RPC_URL=http://127.0.0.1:18080/im/rpc
E2E_MESSAGE_V2_NODE_A_WS_URL=ws://127.0.0.1:18080/im/ws
E2E_MESSAGE_V2_NODE_A_SERVICE_DID=did:wba:awiki.info
E2E_MESSAGE_V2_NODE_B_DOMAIN=awiki.info
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=http://127.0.0.1:18080
E2E_MESSAGE_V2_NODE_B_RPC_URL=http://127.0.0.1:18080/im/rpc
E2E_MESSAGE_V2_NODE_B_WS_URL=ws://127.0.0.1:18080/im/ws
E2E_MESSAGE_V2_NODE_B_SERVICE_DID=did:wba:awiki.info
```

本地 user-service 启动时临时提高 `MAX_REGISTRATIONS_PER_IP`，避免测试数据量再次触发本机注册限额。该环境变量只用于本地验证，不是仓库代码改动。

## 6. 本地系统测试结果

| 测试 | 命令 | 通过 | 失败 | 跳过 | 说明 |
|---|---|---:|---:|---:|---|
| registration token | `uv run --no-project --python .venv/bin/python -m pytest tests_v2/user_service/test_agent_registration_token_local.py -q -rs` | 2 | 0 | 0 | 覆盖 token issue / verify / exchange / reuse reject 和 scope mismatch |
| payload direct/group | `uv run --no-project --python .venv/bin/python -m pytest tests_v2/message_service/test_payload_local.py -q -rs` | 2 | 0 | 0 | 覆盖 direct 和 group 的 `application/json + body.payload` inbox/history/list/WS 投递 |
| daemon contract wrapper | `AWIKI_DAEMON_RUST_REPO=/home/ecs-user/awiki-space/awiki-deamon-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-project --python .venv/bin/python -m pytest tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q` | 3 | 0 | 0 | daemon Rust contract wrapper 保持通过 |

系统测试结论：

- payload direct/group 系统测试已经不再 skipped。
- registration token 系统测试已经不再 skipped。
- daemon contract wrapper 保持 passed。
- 本地验证期间没有引入旧版结构化 JSON 同义字段，也没有引入 command/status/result/task 专用 JSON content type。

## 7. Review 结论

| 审查项 | 结论 |
|---|---|
| payload 契约 | 仍统一为 `application/json + body.payload`，message-service 只做传输、存储、投递，不解释 daemon command/status 语义。 |
| registration token | user-service 能在 DID bearer 下正确得到内部 user_id；token 原文仍只在签发响应中出现，不写入数据库、日志或 audit。 |
| daemon/CLI 边界 | daemon contract wrapper 使用 `AWIKI_DAEMON_RUST_REPO` 指向当前 Rust 仓库，不依赖 awiki-cli 内部模块。 |
| 测试报告完整性 | 已记录 passed / failed / skipped 数量、命令、模式、关键 URL、DID domain 和跳过原因。 |
| 残余风险 | 远端仍受 IP 注册限额影响；长驻 daemon 进程接真实 message-service 的完整活体 E2E 仍未执行。 |

## 8. 后续

下一步执行 [steps/09-daemon-long-running-process-e2e.md](steps/09-daemon-long-running-process-e2e.md)，目标是把“daemon 作为长驻进程接真实 message-service 的完整链路”从计划推进到可运行系统测试或明确的实现任务。
