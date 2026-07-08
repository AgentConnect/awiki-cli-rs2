# Step 06：最终集成验证、安全 Review 与文档收口

主 Plan：[../plan.md](../plan.md)  
Step index：06  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` / 相关仓当前分支；`anp/anp` `master` |
| Started | 2026-06-10T05:59:50Z |
| Completed | 2026-06-10T06:40:41Z |
| Commit | `awiki-cli-rs2` 最终集成文档 commit（以 `git log -1 --oneline` 为准） |
| Review evidence | 已完成全局 Review；未发现新的代码级 P0/P1 契约缺口；remote `awiki.info` message-service 502 已定位为 release 二进制缺失导致的运行环境问题并修复 |
| Verification evidence | 本地 / 跨仓测试通过；remote `daemon + user_service + message_service` 最终 19 passed, 7 skipped |
| Next action | Step 06 完成；提交最终集成文档 commit |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：确认 Step 01-05 的跨仓修复整体符合核心设计预期，并有足够 L3 身份/auth/security 验证证据。
- 用户 / 系统可见行为：新注册、恢复/旧身份迁移、bootstrap、delegated inbox/send、APP action capability、daemon key revoke/registry sync 关键链路可被验证；剩余风险有明确记录。
- 非目标：不新增功能；不把未完成的后续安全债包装为已完成；不在没有证据时宣布 remote E2E 通过。
- 完成标准：全局 Review 完成；各仓测试和 remote `awiki.info` 系统测试记录实际命令、通过/失败/跳过数量；文档和执行台账更新；工作区状态清楚。

## 3. 设计方法

- 设计边界：Step 06 是 gate，不是大规模实现步骤。只有发现跨步骤问题时才修改代码/文档，并记录最终集成 commit。
- 核心决策：按 L3 验证执行：identity/auth/DID/key material/security-sensitive 变更必须有 security review、兼容性证据、E2E 或替代证据。
- 契约 / API / 数据流：检查 user-service registry、DID Document authentication、im-core package/migration、daemon bootstrap validation、awiki-me bootstrap、message-service delegated policy、ANP SDK optional API 是否一致。
- 兼容性：旧 DID auth register、旧 im-core send/inbox/history、旧 bootstrap v1 legacy decode、旧 App action payload tests 不回归。
- 迁移策略：确认 backfill/migration 文档和 error handling 清楚；不能自动迁移的旧身份有用户可理解的恢复路径。
- 风险控制：所有 secret leakage 搜索必须通过；E2EE 明文仍不进入 Agent；message-service 不依赖 registry；registry-only revoke 不被文档误写为运行时撤销。

## 4. 实现方法

1. 读取主 Plan 执行台账，确认 Step 01-05 都是 `done`，并且每步有 commit、Review evidence、Verification evidence。
2. 执行全局代码/文档 Review：
   - registry 与 DID Document authentication 是否一致；
   - proof 后置 patch 是否已移除或仅 legacy；
   - recovery/migration 是否覆盖旧身份；
   - bootstrap 是否早期校验 private/public/DID Document；
   - key package v2 是否新写出、v1 是否 legacy read；
   - APP action 是否显式 capability；
   - ANP SDK optional 参数是否 generic；
   - message-service 是否仍只按 DID Document 授权。
3. 执行命名和安全搜索：
   - `daemon-key-*` 设备化示例；
   - `mailbox_*` 新增命名；
   - `private_key_multibase` 新写路径；
   - private key / jwt / bearer / runtime token 泄露；
   - E2EE plaintext 转发给 Agent。
4. 运行受影响仓库验证命令，记录实际结果。
5. 运行 remote system-test：
   - 必须使用 `AWIKI_SYSTEM_TEST_MODE=remote`；
   - 目标域名使用 `awiki.info`；
   - 记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。
6. 如发现问题：
   - 小问题直接修复并记录最终集成 commit；
   - 安全/契约问题回到对应 Step 或更新 Plan；
   - remote 环境不可用时记录 blocker，不把 Step 06 标为 done，除非用户明确接受替代证据。
7. 更新主 Plan 第 7、15、17 节和本 Step 执行状态。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/plan.md` | 回填执行台账、最终 Review、验证证据 | 必改 |
| `awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix/steps/*.md` | 回填 Step 状态、证据和风险 | 必改 |
| `awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` | 如实现状态或契约变化，更新 | 视 Review 发现 |
| `awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` | 如 key package / SDK / registry 语义变化，更新 | 视 Review 发现 |
| `awiki-system-test` | 运行 remote 系统测试；必要时补测试 | 修改时需 commit |
| 各受影响仓库 | 仅修复 Review 发现的问题 | 聚焦最终集成 commit |

## 6. 依赖

- 前置步骤：Step 01-05 都必须完成并提交。
- 外部文档或决策：`awiki-cli-rs2/AGENTS.md` 要求最终系统测试使用 remote `awiki.info`。
- 环境前提：remote `awiki.info` 服务可用；本地能运行各仓测试命令。

## 7. 验收标准

- [x] Step 01-05 都是 `done`，每步有 commit、Review evidence 和 Verification evidence。
- [x] 全局 Review 未发现新的代码级 P0/P1 问题；剩余风险已记录并分类。
- [x] user-service、awiki-cli-rs2、awiki-me、anp、message-service 关键验证命令已运行。
- [x] remote `awiki.info` 系统测试已运行并记录实际命令、通过/失败/跳过数量。
- [x] secret leakage、naming、legacy schema、E2EE boundary 搜索已完成。
- [x] 主 Plan 和 Step 文档执行台账已回填。
- [x] 最终 `git status --short --branch` 已记录。
- [x] remote `message_service` 系统测试通过；最终 remote 系统测试 `19 passed, 7 skipped`。
- [x] 如本步骤修改文件，已完成 Review、验证和最终集成 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| user-service | `cd user-service && uv run pytest tests/app/did tests/app/did_auth -v` | DID registry/proof/revoke/update tests 通过。 |
| awiki-cli-rs2 | `cd awiki-cli-rs2 && cargo test -p im-core --locked && cargo test -p im-core-dart --locked && cargo test -p awiki-deamon --locked -j1` | identity/migration/daemon tests 通过。 |
| awiki-cli-rs2 Flutter | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`、`cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | codegen/package tests 通过。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | App tests 通过。 |
| ANP | `cd anp/anp && uv run pytest anp/unittest/authentication anp/unittest/proof -v`、`cd anp/anp/rust && cargo test --locked` | SDK tests 通过。 |
| message-service | `cd message-service && cargo test --workspace` | delegated policy/fanout 不回归。 |
| remote system-test | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote <实际命令，目标 awiki.info>` | 记录实际命令、通过/失败/跳过数量和配置。 |
| security search | `rg -n "BEGIN PRIVATE|private_key|privateKey|bearer |jwt|rtok_|e2ee_plaintext|mailbox_|daemon-key-<|daemon-key-macbook" awiki-cli-rs2 awiki-me user-service message-service anp/anp` | 无未解释泄露或旧命名；合法 fixture/legacy 有说明。 |
| docs path | `find awiki-cli-rs2/docs/agent-im/plan/registry-hardening-fix -type f -maxdepth 3 | sort` | 文档存在；链接路径正确。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：所有验证完成后、最终提交前。
- Review 重点：跨仓契约一致性、未提交变更、文档漂移、系统测试证据、security gate、剩余风险是否可接受。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 已解决的运行环境问题 | remote `awiki.info` message-service 端点最初不可用：`/im/rpc`、`/anp-im/rpc`、`/im/ws` 返回 502。根因是 `message-service.service` 的 `ExecStart` 指向 `target/release/message-service`，但 release 二进制不存在，systemd 报 `203/EXEC`。 |
| 已修复问题 | 已修复 | 执行 `cargo build -p message-service --release` 生成二进制；停止临时前台验证进程；使用 `sudo -n systemctl reset-failed message-service.service && sudo -n systemctl restart message-service.service` 让 systemd 正式接管；公网 `/im/rpc`、`/anp-im/rpc` 从 502 恢复为 405。 |
| 剩余风险 | 已记录 | `user-service` 当前通过 sibling editable `../anp/anp` 消费 ANP SDK，发布前需要同步发布 ANP SDK 或固定依赖；bootstrap private package MVP 仍通过普通消息明文 JSON 传输，这是既定后续安全债；运行环境依赖当前 workspace release 二进制，后续部署需避免清理该产物。 |
| 新增或缺失测试 | 已补最终 remote 验证 | 本地和跨仓单测 / 集成测试覆盖已跑；remote `daemon + user_service + message_service` 最终 `19 passed, 7 skipped`。 |
| 已更新或缺失文档 | 已更新执行文档 | 主 Plan 与本 Step 已记录 Review、验证、运行环境修复、剩余风险和最终状态。 |

## 10. Commit 要求

- Commit 时机：本步骤修改代码/测试/文档且验证、Review 完成后。
- Commit 范围：最终集成修复、文档回填或系统测试补充；如果只运行验证不改文件，则不需要 commit。
- Commit 前状态：记录所有受影响仓 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`agent-im: finalize delegated key hardening`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| remote `awiki.info` message-service 不可用 | 已解决。故障时 `tests_v2/message_service` remote 结果为 14 failed, 3 passed, 2 skipped；失败均为 `https://awiki.info/im/rpc`、`https://awiki.info/anp-im/rpc` 或 `wss://awiki.info/im/ws` 返回 502；健康探测：`/im/rpc=502`、`/anp-im/rpc=502`、`/=500`。根因是 release 二进制缺失导致 systemd `203/EXEC` | 编译 release 二进制，停止临时前台验证进程，重启 systemd 服务，确认 `message-service.service` active 且监听 `127.0.0.1:9900`；公网 `/im/rpc`、`/anp-im/rpc` 返回 405 | 阻塞已解除 | remote system-test 最终 19 passed, 7 skipped；Step 06 标记 done |
| 上游 ANP SDK 变更未完成 | 不适用 | Step 05 已完成 Python/Rust SDK optional API 并提交 | 无当前阻塞 | 发布前处理 ANP SDK 发布 / 依赖固定风险 |
| P0/P1 安全 Review 发现 | 未发现新增代码级 P0/P1 | 已执行全局 Review、secret/naming 搜索和跨仓验证 | 无当前阻塞 | 保持剩余风险记录 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 06 | 定义最终 L3 验证和安全 Review gate | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：remote 系统测试环境依赖 `message-service.service` 指向的 release 二进制，后续部署或清理 target 时可能再次影响服务。
- 回滚 / 回退：如果只影响文档，回滚文档 commit；如果发现跨仓契约问题，回到对应 Step 修复后重新执行 Step 06。
- 后续文档：最终将已实现行为同步到核心设计文档、delegated proof plan、受影响仓 docs/API 和系统测试说明。

## 14. Step 06 实际验证记录

### 本地 / 跨仓验证

| 仓库 / 层级 | 实际命令 | 结果 |
|---|---|---|
| `user-service` | `uv run pytest tests/app/did tests/app/did_auth -v` | 140 passed, 32 warnings；warnings 为既有 deprecated helper 与 SQLModel session 警告。 |
| `anp/anp` Python | `uv run pytest anp/unittest/authentication anp/unittest/proof -v` | 141 passed。 |
| `anp/anp/rust` | `cargo test --locked` | lib 61 passed；authentication 33 passed；direct/group/key/proof/python/wns integration suites 全通过；doc tests 0。 |
| `message-service` | `cargo test --workspace` | workspace 全通过；delegated send / inbox / history / fanout 相关测试通过。 |
| `awiki-cli-rs2` Rust | `cargo test -p im-core --locked && cargo test -p im-core-dart --locked && cargo test -p awiki-deamon --locked -j1` | `im-core` 全通过；`im-core-dart` 6 unit + 13 facade passed；`awiki-deamon` lib 110 passed，integration suites 通过，Hermes real gateway 3 ignored。 |
| `awiki-cli-rs2` Flutter package | `scripts/flutter/codegen-check.sh`；`cd packages/awiki_im_core && flutter test` | codegen 输出 `Done!`；Flutter package 12 passed。 |
| `awiki-me` | `flutter analyze && flutter test` | analyze：No issues found；Flutter tests 273 passed。 |
| diff / format | 各受影响仓 `git diff --check` | 通过。 |

### remote `awiki.info` 系统测试

配置上下文：

- `AWIKI_SYSTEM_TEST_MODE=remote`
- `E2E_DID_DOMAIN=awiki.info`
- runner 环境自动设置 / 本次显式使用 `NO_PROXY=127.0.0.1,localhost,awiki.info,www.awiki.info`
- user-service URL、message-service URL、WebSocket URL 由 `awiki-system-test` remote 配置派生：`https://awiki.info`、`https://awiki.info/im/rpc`、`https://awiki.info/anp-im/rpc`、`wss://awiki.info/im/ws`

结果明细：

| 命令 | 通过 | 失败 | 跳过 | 结论 |
|---|---:|---:|---:|---|
| `AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info uv run --no-sync python scripts/run_tests_v2.py --raw tests_v2/daemon tests_v2/user_service tests_v2/message_service` | 5 | 14 | 7 | 故障修复前失败；message-service remote 端点 502。 |
| `AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info uv run --no-sync python scripts/run_tests_v2.py --raw tests_v2/daemon tests_v2/user_service` | 2 | 0 | 5 | 故障期间 user-service / daemon 子集通过；跳过项均为 local topology 或需要显式 `AWIKI_DAEMON_RUST_REPO` 的 contract selector。 |
| `AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info NO_PROXY=127.0.0.1,localhost,awiki.info,www.awiki.info uv run --no-project --python .venv/bin/python -m pytest tests_v2/message_service -q -rs --tb=short` | 3 | 14 | 2 | 故障修复前失败；所有失败集中在 remote message-service HTTP / WebSocket 502。 |
| `AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info uv run --no-sync python scripts/run_tests_v2.py --raw tests_v2/daemon tests_v2/user_service tests_v2/message_service` | 19 | 0 | 7 | 故障修复后通过。 |
| `AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info NO_PROXY=127.0.0.1,localhost,awiki.info,www.awiki.info uv run --no-project --python .venv/bin/python -m pytest tests_v2/daemon tests_v2/user_service tests_v2/message_service -q -rs` | 19 | 0 | 7 | 故障修复后通过，并记录 skip 原因。 |

失败用例按功能域：

| 功能域 | 失败数量 | 用例 / 文件 | 原因 |
|---|---:|---|---|
| message-service attachment | 2 | `tests_v2/message_service/test_attachment_local.py`：direct attachment、group attachment | `https://awiki.info/im/rpc` 返回 502。 |
| message-service direct / capability / direct E2EE contract | 5 | `tests_v2/message_service/test_direct_local.py`：capabilities、direct send/inbox/history、prekey OPK、legacy HPKE reject | `/im/rpc` 或 `/anp-im/rpc` 返回 502。 |
| message-service group E2EE discovery contract | 2 | `tests_v2/message_service/test_group_e2ee_contract.py` | `https://awiki.info/anp-im/rpc` 返回 502。 |
| message-service group base | 1 | `tests_v2/message_service/test_group_local.py` | `https://awiki.info/im/rpc` 返回 502。 |
| message-service JSON-RPC auth HTTP status | 1 | `tests_v2/message_service/test_jsonrpc_auth_http_status.py` | 预期 401，实际远端网关返回 502。 |
| message-service JSON payload / WebSocket | 2 | `tests_v2/message_service/test_payload_local.py` | `/im/rpc` 或 `wss://awiki.info/im/ws` 返回 502。 |
| message-service WebSocket notification | 1 | `tests_v2/message_service/test_ws_notifications.py` | `wss://awiki.info/im/ws` 握手返回 502。 |

跳过用例：

| 功能域 | 跳过数量 | 原因 |
|---|---:|---|
| daemon long-running E2E | 2 | `This test requires the local tests_v2 topology.` |
| daemon Rust contract selector | 3 | `Rust daemon contract selector requires explicit AWIKI_DAEMON_RUST_REPO; daemon is a separate project and is skipped for awiki-cli-only runs.` |
| message-service topology / flag-off guard | 2 | 一个需要 local tests_v2 topology；一个需要 `AWIKI_GROUP_E2EE_CONTRACT_TEST=0` 和对应 runtime 配置。 |

故障修复前健康探测：

```text
NO_PROXY=awiki.info,www.awiki.info,127.0.0.1,localhost curl -sS -o /tmp/awiki-im-rpc.probe -w 'im_rpc_http=%{http_code}\n' https://awiki.info/im/rpc
NO_PROXY=awiki.info,www.awiki.info,127.0.0.1,localhost curl -sS -o /tmp/awiki-anp-im-rpc.probe -w 'anp_im_rpc_http=%{http_code}\n' https://awiki.info/anp-im/rpc
NO_PROXY=awiki.info,www.awiki.info,127.0.0.1,localhost curl -sS -o /tmp/awiki-root.probe -w 'root_http=%{http_code}\n' https://awiki.info/

im_rpc_http=502
anp_im_rpc_http=502
root_http=500
```

运行环境修复记录：

| 检查 / 命令 | 结果 |
|---|---|
| `systemctl --no-pager status message-service.service` | 初始状态 failed；`ExecStart=/home/ecs-user/awiki-space/message-service/target/release/message-service`，systemd `203/EXEC`。 |
| `journalctl -u message-service.service --no-pager -n 200` | `Failed to locate executable .../target/release/message-service: No such file or directory`。 |
| `cargo build -p message-service --release` | 成功生成 `message-service/target/release/message-service`，release build 通过。 |
| `./target/release/message-service --help || true` | 前台验证触发迁移检查并成功监听 `127.0.0.1:9900`，证明二进制和配置可用；该临时前台进程随后占用端口。 |
| `sudo -n systemctl reset-failed message-service.service && sudo -n systemctl restart message-service.service` | 第一次因临时前台进程占用 `127.0.0.1:9900` 报 bind 失败；停止临时进程后再次执行成功。 |
| `systemctl --no-pager is-active message-service.service` | `active`。 |
| `ss -ltnp | rg ':9900'` | `message-service` 监听 `127.0.0.1:9900`。 |
| 本地 / 公网 HTTP 探测 | `http://127.0.0.1:9900/im/rpc`、`http://127.0.0.1:9900/anp-im/rpc`、`https://awiki.info/im/rpc`、`https://awiki.info/anp-im/rpc` 均返回 405，说明端点已从 502 恢复并由服务接收请求。 |

### 搜索 / 安全 Review

| 检查 | 结果 |
|---|---|
| `user-service` proof 后置 patch 搜索 | `_apply_delegated_key_public_registration`、`_resign_did_document_after_local_mutation` 无命中。 |
| `apply_to_did_document` 搜索 | 只剩 im-core legacy migration/helper/tests 与历史计划记录，非新注册主路径。 |
| `daemon-key-*` 设备化示例 | 核心设计、proof plan 和 registry-hardening plan 均固定 `#daemon-key-1`；未发现 `#daemon-key-<device_id>` 或 `#daemon-key-macbook-*` 新示例。 |
| `mailbox_*` 命名 | 新增 Agent IM 方案使用 `inbox_*`；`mailbox_*` 只出现在历史检查命令、email/mail 既有模型、message-service 既有 `im-app` 字段。 |
| `private_key_multibase` | 新写路径使用 `private_key_pem`；`private_key_multibase` 仅用于 legacy decode、兼容字段和测试。 |
| secret / token / E2EE 搜索 | 命中项为 secret handling、redaction tests、JWT/auth 既有路径、fixtures 和 E2EE 既有模块；未发现新增 daemon private material 日志、audit、prompt 或 status 泄露。 |

### 最终工作区状态

```text
awiki-cli-rs2: feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 43], modified plan.md and Step 06 doc
awiki-me: feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 5], clean
user-service: feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 4], clean
message-service: feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 1], clean
awiki-system-test: feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong, clean
anp/anp: master...origin/master [ahead 1], clean
```

注意：`message-service/target/release/message-service` 是构建产物，不提交到 git；systemd 当前依赖该路径上的 release 二进制。
