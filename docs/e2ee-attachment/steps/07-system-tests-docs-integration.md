# Step 07：系统测试、文档与集成收口

主 Plan：[../plan.md](../plan.md)  
Step index：07  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `awiki-system-test: release/0526`，实现仓库分支同前序步骤 |
| Started |  |
| Completed |  |
| Commit |  |
| Review evidence |  |
| Verification evidence |  |
| Next action | 等 Step 01-06 完成后启动 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：跨服务 E2E 覆盖 direct/group E2EE 附件发送、下载、解密和 negative paths；文档与 Harness 状态同步；最终全局 Review 完成。
- 用户 / 系统可见行为：系统测试证明完整链路可跑通或明确记录环境性失败/跳过原因。
- 非目标：不在本步骤补大范围功能，只做测试、文档和必要集成修复。
- 完成标准：focused E2E 和 remote mode 验证有证据；所有步骤台账和 commit 记录完整；最终 Review 无未处理 critical findings。

## 3. 设计方法

- 设计边界：`awiki-system-test` 只验证跨服务行为，不承载业务实现。
- 核心决策：focused tests 先证明 direct/group E2EE 附件闭环，再执行 remote full 或指定 suite。
- 契约 / API / 数据流：测试使用 CLI 高层命令，不直接调用 P7/P5/P6 raw RPC，除非 negative service boundary test 明确需要。
- 兼容性：plain attachment、direct secure、group secure 既有 suites 保持通过或记录非本任务失败。
- 迁移策略：如 message-service migration 未改，不跑数据库迁移 smoke；如改 migration，必须 clean DB migrate。
- 风险控制：remote 环境失败需要记录 HTTP status、域名、关键 env、passed/failed/skipped。

## 4. 实现方法

1. 在 `awiki-system-test` 增加或扩展 tests：
   - direct E2EE 附件：Alice `msg send --file --secure required` -> Bob history/inbox decrypt -> Bob download -> 校验明文 bytes。
   - group E2EE 附件：secure group ready -> member send file -> other member download -> 校验明文 bytes。
   - negative：grant missing、digest mismatch、removed group member ticket denied、plain path不能携带 object key。
2. 更新 system-test docs：
   - `awiki-system-test/docs/direct-e2ee-system-tests.md`
   - `awiki-system-test/docs/group-e2ee-system-tests.md`
   - 如新增附件专门文档，可建 `docs/e2ee-attachment-system-tests.md`。
3. 更新实现仓库 docs：
   - `message-service/docs/api/ANP-client-server-api-attachment.md`
   - `message-service/docs/architecture/direct-e2ee-service-boundary.md`
   - `message-service/docs/architecture/group-e2ee-service-boundary.md`
   - `e2ee-attachment-cli-rs2/docs/api/im-core-public-api.md`
   - `e2ee-attachment-cli-rs2/docs/flutter-sdk/awiki-im-core-flutter-sdk.md`
   - `e2ee-attachment-cli-rs2/docs/e2ee-attachment/e2ee-attachment-transfer-design.md`
4. 如公开发现没有改变，明确记录 discovery 仍关闭。
5. 执行 repo-local broad checks：
   - `message-service` workspace tests。
   - `e2ee-attachment-cli-rs2` workspace tests。
6. 执行 system tests：
   - focused local/remote E2EE attachment suites。
   - 最终 remote mode，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info`。
7. 最终全局 Review：
   - 检查所有 changed repos。
   - secret grep。
   - `git status`。
   - 回填主 Plan 第 17 节。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-system-test/tests_v2/cli/*` | direct/group E2EE attachment E2E | 路径按现有 suite 命名 |
| `awiki-system-test/docs/direct-e2ee-system-tests.md` | 补 direct attachment 测试说明 |  |
| `awiki-system-test/docs/group-e2ee-system-tests.md` | 补 group attachment 测试说明 |  |
| `message-service/docs/api/ANP-client-server-api-attachment.md` | 最终 API 状态 | 若前序未完整更新 |
| `e2ee-attachment-cli-rs2/docs/api/im-core-public-api.md` | SDK public API 状态 |  |
| `e2ee-attachment-cli-rs2/docs/e2ee-attachment/plan.md` | 回填台账和最终 Review | source of truth |

## 6. 依赖

- 前置步骤：Step 01-06 全部 done。
- 外部文档或决策：remote 测试环境、`awiki.info` 域名。
- 环境前提：`awiki-system-test` 可运行，必要服务/凭据按现有 suite 要求配置。

## 7. 验收标准

- [ ] direct E2EE 附件 focused E2E 有证据。
- [ ] group E2EE 附件 focused E2E 有证据，或记录 feature gate/环境原因。
- [ ] negative tests 覆盖 grant、digest/decrypt、removed member 或安全边界。
- [ ] 文档同步实际行为，不把 discovery 写成公开开启。
- [ ] 最终 remote mode 使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 并记录 passed/failed/skipped。
- [ ] 全局 Review 发现已修复或记录。
- [ ] 本步骤和最终文档变更创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| message-service workspace | `cd message-service && cargo test --workspace` | 通过或记录非本任务失败。 |
| CLI workspace | `cd e2ee-attachment-cli-rs2 && cargo test --workspace --locked` | 通过或记录非本任务失败。 |
| Focused direct E2E | `cd awiki-system-test && AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/cli -k "direct and attachment and e2ee" -q -rs` | direct focused 通过/失败/跳过统计。 |
| Focused group E2E | `cd awiki-system-test && AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/cli -k "group and attachment and e2ee" -q -rs` | group focused 通过/失败/跳过统计。 |
| Remote full | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync awiki-system-test` | 记录 passed/failed/skipped、耗时、关键配置。 |
| Secret grep | `rg -n "object_key_b64u|nonce_b64u|download_ticket|private_key|JWT|ratchet|MLS" message-service e2ee-attachment-cli-rs2 awiki-system-test` | 命中需解释；生产输出/docs 不泄漏秘密。 |
| Status | `git status --short --branch` in changed repos | 工作区状态清楚，无遗漏变更。 |

## 9. Review 环节

Review 重点：

- E2E 是否真的通过 CLI 高层路径验证，而不是只测内部函数。
- 系统测试是否覆盖接收端下载和明文校验。
- negative tests 是否能证明服务端授权和客户端校验有效。
- 文档是否和实现一致，且 discovery 仍关闭。
- 各仓库 commit 是否聚焦，无无关修改。

## 10. Commit 要求

- `awiki-system-test` 修改创建独立 commit。
- 文档收口如果只在 `e2ee-attachment-cli-rs2`，在该仓创建聚焦 docs/integration commit。
- 如果最终集成修复了 `message-service` 或 `e2ee-attachment-cli-rs2` 代码，在对应仓库创建聚焦 commit。
- Commit 后回填主 Plan 执行台账和第 17 节最终证据。

## 11. 风险、假设与回滚

- 风险：remote 环境不稳定。缓解：记录 focused local/remote 证据和具体远端失败。
- 风险：group E2EE hidden/test-only gate 导致 remote skip。缓解：记录 skip 原因，不把 skip 写成通过。
- 回滚：保留 unit/CLI 能力，暂不宣称系统 E2E 完成；主 Plan 标记 blocked 或 residual risk。
