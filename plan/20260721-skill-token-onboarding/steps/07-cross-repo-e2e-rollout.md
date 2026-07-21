# 步骤 07：国内环境跨仓库 E2E、灰度和回滚

状态：`in_progress`（AWiki Me `full` 已通过；2 个 secure-direct 回归已修复，待公开 `awiki.info` 最终全量复验）
实施仓库：`awiki-system-test`，并验证四个功能仓库  
目标环境：国内 `awiki.info`  
前置依赖：步骤 02-06 全部完成  
后续依赖：无

## 1. 目标

- 验证真实链路：App 签发 -> 智能体安装 -> CLI claim -> Agent 主动消息 -> App 回复。
- 验证 30 分钟 Token、空 workspace、幂等恢复和跨 owner 隔离。
- 建立最小灰度和可回滚发布顺序。
- 留下可复核的命令、配置、通过/失败/跳过数量和日志证据。

## 2. 不做的内容

- 不测试 `awiki.ai` 海外环境，也不做国内外映射。
- 不测试 App 对 Skill Agent 的列表、改名、删除或状态管理。
- 不增加 v2 功能作为 E2E 前置条件。
- 不用本地数据库注入代替真实 API。
- 不因测试方便打印 raw Token、JWT 或私钥。

## 3. 发布前版本顺序

1. User Service：部署 `skill` Token/API/schema，issue feature flag 默认关闭。
2. Message Service：部署角色兼容和隔离改动；没有生产改动时只验证现网版本。
3. AWiki CLI/Skill：发布支持 `onboarding claim` 的国内 stable artifact 和 onboarding。
4. AWiki Me：发布 Token 签发和复制入口，但仍受服务端 flag 控制。
5. 对测试用户打开 `skill_onboarding_v1`。

- App 不能早于 CLI stable artifact 开启入口。
- User Service 停止新 issue 时，已签发 Token 保留 exchange grace。

## 4. 正向 E2E

### 4.1 App 签发

- 使用真实国内测试账号登录 App。
- 点击复制 Skill 安装指令。
- 断言 prompt 使用 `awiki.info`、30 分钟过期、`agent_kind=skill` 对应 Token。
- 测试日志只保存 token_id，raw Token 只在受控测试进程内存中流转。

### 4.2 CLI claim

- 使用全新的临时 workspace。
- 按发布的 `https://awiki.info/cli/onboarding.md` 安装 CLI 和 Skill。
- 通过 stdin 把 Token 交给 `awiki-cli onboarding claim`。
- 断言生成一个 Agent DID/Handle、自己的 JWT 和 ready identity。
- 断言 User Service 绑定正确 controller_user_id，daemon_agent_did 为空。

### 4.3 主动消息

- claim 完成后等待 Skill Agent 标准 direct message。
- App 通过真实 sync/realtime 路径看到固定消息。
- 断言 sender 是 Skill Agent DID，receiver 是 Controller DID。
- 断言正文不含 Token，conversation 是普通 direct conversation。
- App 回复一条测试消息，Skill Agent 用标准 CLI history/sync/realtime 读取。

## 5. 幂等和恢复 E2E

- exchange response 前断网，恢复后仍是同一 DID。
- identity ready 后、主动消息前中断，恢复后只产生一条消息。
- 重复执行 claim，不重新注册或重复 greeting。
- Message Service timeout 后重试使用同一 message ID。
- journal 恢复成功后清理 pending 状态但保留非敏感结果。

## 6. 负向 E2E

- Token 过期后 claim 拒绝并提示 App 重新生成。
- Token 被撤销后拒绝。
- Token 被另一个 DID 抢先使用后原 workspace 拒绝。
- `awiki.info` Token 指向 `awiki.ai` service URL 时拒绝，反向同样拒绝。
- Controller Handle 或 Agent Handle 被修改时拒绝。
- 非空 workspace 在任何远端写操作前拒绝。
- Controller JWT 冒充 Agent sender 被拒绝。
- Agent JWT 读取 Controller history/sync 被拒绝。

## 7. 仓库级验证

以下全量验证只在本步骤统一执行；步骤 01-06 仅运行定向测试和必要静态检查。

### 7.1 User Service

- 运行 agent registration、inventory、DID auth、storage 聚焦测试。
- 运行全量 `uv run pytest -q`。
- 使用真实 MySQL 运行 Skill exchange transaction/concurrency 测试。

### 7.2 AWiki CLI

- 运行 command catalog、onboarding claim、identity vault、direct message 聚焦测试。
- 运行 `cargo fmt --all --check`。
- 运行相关 crate tests 和 `cargo clippy --workspace --all-targets -- -D warnings`。
- 运行 release staging tests，确认国内 onboarding artifact。

### 7.3 AWiki Me

- 运行 Skill Token adapter/prompt/widget 聚焦测试。
- 运行完整真实后端 App + CLI peer E2E：

```bash
cd /home/ecs-user/awiki-space/awiki-me-emas-android
dart run tests/e2e/runner.dart --case full
```

### 7.4 Message Service

- 运行 auth/direct/sync/storage 和 Skill role 聚焦测试。
- 运行 `cargo check --workspace`、`cargo test --workspace` 和 workspace Clippy。
- 使用真实 PostgreSQL 运行 greeting idempotency/inbox/sync 测试。

## 8. AWiki remote system test

按 workspace 约束执行完整国内远端测试：

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test --show-command
```

必须记录：

- 实际 commit 和部署版本。
- 实际命令和关键非敏感环境配置。
- passed/failed/skipped 数量。
- 每个失败或跳过的原因。
- raw Token、JWT 和私钥均不得进入报告。

## 9. 观测和灰度

- User Service metrics：issue、verify、exchange、expiry、revoke、scope mismatch、idempotent replay。
- CLI metrics：claim phase、greeting pending/sent、恢复次数；不含 DID 全文和 secret。
- Message Service metrics：greeting accepted/deduplicated/failure。
- 先对内部测试 user allowlist 开启，再扩大到小比例国内用户。
- 观察至少一个完整 Token TTL 窗口后再扩大。

## 10. 回滚

- 先关闭 User Service 新 Token issue flag。
- 保留已签发 Token 30 分钟 exchange grace。
- App 隐藏复制入口，不需要删除已注册 Skill Agent。
- CLI 回滚不影响已经注册的 DID/JWT 和普通 IM。
- Message Service 回滚前确认旧版本仍接受 `agent:skill`；否则先停止灰度。
- 安全事件通过服务端运维归档/撤销处理，不临时增加 App 删除功能。

## 11. 完成标准

- 真实 App 签发的 Token 在空 workspace 完成 claim。
- Skill Agent 主动消息到达人类 App，App 回复可被 Agent 读取。
- 注册和主动消息在中断重试后各只有一份。
- 所有负向安全场景 fail closed。
- 国内 remote system test 和 AWiki Me full E2E 有完整证据。
- 灰度、监控和回滚均经过演练或桌面复核。
- 四仓库工作区只包含本功能相关提交和文档。

## 12. 2026-07-21 实施记录

### 12.1 提交与部署

- CLI/im-core：功能 `a2180bb0`，发布修复至 `911fc51d`，secure projection 修复至 `850c4edf`，`feature/skill-token-onboarding`。
- AWiki Me：功能 `bb96617`，E2E harness 修复 `9708c4c`，`feature/aliyun-emas-android`，已推送。
- User Service：`57c63ec`，`feature/emas-push-user-service`，已推送并部署到国内服务。
- Message Service：EMAS 功能 `93b4ce8`、Skill 隔离测试 `2deba55`，`feature/emas-push-message-service`，已推送。
- 最终联调使用该功能分支 release 二进制和独立临时 PostgreSQL；现网较新 schema 不做降级或回滚，测试后已恢复原 unit。

### 12.2 通过结果

- 国内真实 Skill 链路通过：30 分钟一次性 Token、空 workspace claim、固定 greeting 唯一到达、Controller 回复可由 Skill CLI 读取、重复 claim 复用同一 DID 和 greeting ID、本地无 raw Token。
- User Service：`829 passed, 10 skipped`；真实 MySQL Skill storage `10 passed`；任务相关 Ruff 通过。
- Message Service：workspace check、`274` 个测试、Clippy、fmt 全部通过；真实 PostgreSQL Skill greeting 隔离测试 `1 passed`。
- AWiki Me：`dart analyze` 通过，完整 unit suite `1180 passed`。
- AWiki Me 真实后端 `full` E2E：24 个 case 全部通过、失败 0、跳过 0，耗时 `4m 6s`；覆盖 direct、group、contact、unread、sequence、attachment 和 profile refresh。
- CLI：`im-core-dart` `42 passed`，release staging `2 passed`，fmt 和 diff check 通过。
- 正式国内 CLI stable `1.0.23` 已发布：tag `cli-v1.0.23`、commit `911fc51d`、GitHub Actions run `29852384648`；Linux amd64、macOS Intel、macOS arm64、Windows amd64 的 archive、版本 smoke 和 artifact upload 全部通过。
- `https://awiki.info/cli/stable/manifest.json`、`/cli/onboarding.md` 和 `/onboarding.md` 均可访问；两个 onboarding 入口内容一致，公开 Linux 包 checksum、版本和 commit 已复核。
- secure-direct 定向 system test 使用功能 worktree 与隔离的 feature User/Message Service，两个原失败用例合并结果为 `2 passed`；未执行中间全量。

### 12.3 已确认阻塞

- CLI workspace 全量测试命中未修改的 message read 基线：`32 passed, 5 failed`；同一用例在 `origin/release/0714@de44ee74` 结果完全相同，另有一个未修改 sync 用例长期不结束。
- CLI workspace Clippy 被未修改文件中的 `13` 个既有 lint 阻断；未扩大修改范围。
- AWiki Me 历史 `anp.device_binding_required` 已通过正式 hidden-rollout 测试窗口解除；Flutter 3.44 隔离 build-dir 和持续 frame 等待也已修复，最终 `full` 通过。
- 国内 remote system suite 历史执行结果为 `69 passed, 50 failed, 41 skipped, 148 errors`；当时包含基础 CLI worktree 选择错误、磁盘不足、远端请求超时及 local/provider-only 用例。CLI 源码选择和磁盘问题已解除，但按统一测试节奏不在中间阶段重跑全量。
- 2026-07-22 首次最终 remote suite：`255 passed, 2 failed, 51 skipped`，耗时 `9m 0s`；失败 2 均属 CLI secure-direct：回复历史解密为 `undecryptable`，Handle history 的明文记录丢失 `secure` 标记。
- 51 个 skip 按功能域归因：daemon 17、listener 7、mail 5、MCP 9、multi-tenant 7、message-service local/flag 3、admin-controller 1、已移除 store contract 1、search 1；均由显式 capability、凭据或 local topology 门禁触发。
- 两项失败根因已在 im-core 修复：unresolved Persona backlog 复用已解密明文，local record 保留/恢复 E2EE 安全标记；相关 4 个定向单元测试和 2 个 system test 均通过。
- 当前公开 `awiki.info` 被并行 multi-device 任务路由到 9891/9902，尚未包含本任务 feature 服务；未覆盖该部署，最终公开 remote 全量留到合并/切回后一次执行。

### 12.4 清理与安全

- 独立临时 MySQL/PostgreSQL 数据库、9889 proxy、9890 User Service 和 feature Message Service unit 均已删除或卸载。
- 私密 E2E 配置、临时联调脚本和构建缓存已删除。
- Message Service 原 unit、三项 hidden rollout/root control 配置均已恢复，国内 health 为 `200`。
- 实施记录不包含 raw Token、JWT、私钥或数据库凭据。
