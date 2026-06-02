# Step 07：系统测试、文档与集成收口

主 Plan：[../plan.md](../plan.md)  
Step index：07  
状态：blocked

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | blocked |
| Branch | `awiki-system-test: release/0526`，实现仓库分支同前序步骤 |
| Started | 2026-06-02 13:20 CST |
| Completed |  |
| Commit | `e2ee-attachment-cli-rs2:5520adb`、`e2ee-attachment-cli-rs2:1745f72`、`e2ee-attachment-cli-rs2:34ac61c`、`awiki-system-test:145d920`、`awiki-system-test:d17e03a`、`awiki-system-test:1a196fa`、`anp/anp:1b83488`、`message-service:bb06bdf`；文档收口 commit 由当前仓库 `git log` 和最终响应记录，避免文档自引用导致 commit hash 循环。 |
| Review evidence | 已完成 Step 07 阶段性 Review：发现并修复 public projection 泄漏、direct async attachment unsupported、direct async attachment init 缺失、系统测试选择器在显式 repo override 下误报、ANP typed group payload 还原、端侧 internal-only group manifest cache，以及服务端 group attachment grant 使用 raw `meta.message_id` 导致取票 6005 的 canonical message id 绑定问题；本轮补充 forwarded accepted response 的 `group_did` 校验，避免远端返回其他 group 时写入本地 grant；剩余 critical finding 为远端 `awiki.info` message-service 尚未部署本轮 canonical grant 修复。 |
| Verification evidence | focused direct secure attachment 已通过；focused group secure attachment 已越过 6013、MLS 重复解密和 public projection 泄漏问题，Bob 能读到 redacted manifest，但下载取票仍因远端未部署 canonical grant 修复返回 `6005 no access grant matched the requested attachment context`。本地 `message-service`、ANP 和 im-core focused tests 已通过，且 forwarded remote group mismatch guard 已通过，具体命令见 9.1。 |
| Next action | 部署包含本轮 `message-service` group attachment canonical grant 修复的版本到 `awiki.info` 后，重跑 focused group 和最终 remote full。 |

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

- [x] direct E2EE 附件 focused E2E 有证据：`secure_direct_attachments` 已通过，覆盖 CLI 高层发送、远端 slot/commit、Bob 下载、digest 校验和本地解密。
- [x] group E2EE 附件 focused E2E 有证据：用例已加入并通过 CLI 高层路径执行；当前远端已越过 `attachment.create_slot` 和 group manifest 投影，下载取票因远端尚未部署 canonical grant 修复返回 6005，未跑通最终下载闭环。
- [x] negative tests 覆盖 grant、digest/decrypt、removed member 或安全边界：`awiki-system-test` Rust wrapper 覆盖 im-core digest/key/plaintext_size negative、public/realtime redaction、wire 控制面不含 key/nonce；服务端 focused tests 覆盖 E2EE grant refs。
- [x] 文档同步实际行为，不把 discovery 写成公开开启。
- [x] 最终 remote mode 使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 并记录 passed/failed/skipped。
- [x] 全局 Review 发现已修复或记录。
- [x] 本步骤代码、系统测试和文档变更创建聚焦 commit。

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

实际 Review 记录：

- 发现 public read/realtime projection 曾把 direct/group E2EE 内层 attachment manifest 原样放入 public message/event，存在 `object_key_b64u` / `nonce_b64u` 泄漏风险；已在 `e2ee-attachment-cli-rs2:5520adb` 修复为 public projection redacted、internal download projection 保留 full manifest。
- 发现 `MessageService::send_direct_e2ee_async` 在无 `blocking` feature 时对 direct secure attachment 返回 `unsupported capability: async-secure-direct-attachment-send`；已在 `e2ee-attachment-cli-rs2:5520adb` 增加 async direct attachment follow-up 路径。
- remote full 进一步发现 direct secure attachment 在无本地 established session 时仍不能发 init；已在 `e2ee-attachment-cli-rs2:1745f72` 增加 async direct attachment init 路径，并补 init/follow-up 不泄漏 key/nonce 单测。
- remote full 发现 `AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2` 下 selection defaults 测试仍固定断言 sibling `awiki-cli-rs2`；已在 `awiki-system-test:d17e03a` 修复为显式 repo override 时断言 override 路径。
- 进一步发现 group E2EE 附件在 Bob 已能读到 redacted manifest 后，下载取票返回 `6005 / no access grant matched the requested attachment context`。排查确认服务端在 group accepted 后使用请求 raw `meta.message_id` 写 Access Grant，而 group 历史和 CLI 下载使用 `{group_did}:{group_event_seq}` canonical message id；已在 `message-service` 本轮修改中统一 group base/group-e2ee 本地 accepted 和 forwarded accepted 的 grant `message_id` 绑定，并补本地/forwarded focused 测试。
- Review 追加发现 forwarded group accepted response 如果返回不同 `group_did`，本地 sender-home 不应把 grant 绑定到远端返回的其他 group；已补 `expected_group_did` 校验和 focused 测试，确保不匹配时返回 unauthorized。
- 剩余 critical finding：`awiki.info` 当前 message-service 尚未部署本轮 canonical group attachment grant 修复；focused group 系统测试仍在取票阶段返回 6005。部署后需重跑 focused group 和最终 remote full。

## 9.1 实际验证证据

通过：

- `cd e2ee-attachment-cli-rs2 && cargo fmt --all --check`：通过。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core async_direct_secure_attachment_sender --locked`：2 passed, 0 failed；覆盖 direct attachment async init/follow-up，外层 body 和 `client.attachment_grant_refs` 不泄漏 key/nonce。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core async_direct_secure_sender --locked`：3 passed, 0 failed；覆盖 direct async text init/follow-up/queued 既有路径。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachments_download_runtime --locked`：13 passed, 0 failed；覆盖 object-e2ee 下载、digest/key/plaintext_size negative 和 local-file 不写出失败明文。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_projection --locked`：7 matched tests passed, 0 failed；覆盖 public direct E2EE attachment projection redaction 和 realtime attachment projection。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core realtime_notification_normalizer_redacts_attachment_manifest_secrets --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core secure_group_attachment_public_projection_redacts_and_sets_group_profile --features group-e2ee --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core realtime_notification_projection_redacts_attachment_manifest_secrets --features group-e2ee --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_manifest_cache --locked`：2 matched tests passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core local_state_schema --locked`：6 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachments_download_runtime_group_object_e2ee_uses_internal_manifest_cache --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core group_attachment_manifest_cache_keeps_internal_full_manifest_while_public_redacts --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core group_result_projects_secure_attachment_manifest_payload --locked`：1 passed, 0 failed。
- `cd e2ee-attachment-cli-rs2 && cargo test -p im-core message_body_projects_attachment_manifest_as_payload --locked`：1 passed, 0 failed。
- `cd anp/anp/rust && cargo fmt --all --check`：通过。
- `cd anp/anp/rust && cargo test --features mls --test group_e2ee_typed_operations_tests typed_operations_create_finalize_add_finalize_without_binary_exec`：1 passed, 0 failed。
- `cd message-service && cargo test -p im-attachment attachment -- --nocapture`：12 passed, 0 failed；覆盖 object-e2ee control policy 和 E2EE grant refs。
- `cd message-service && cargo fmt --all --check`：通过。
- `cd message-service && cargo test -p im-attachment grant -- --nocapture`：5 passed, 0 failed。
- `cd message-service && cargo test -p im-group forwarded_group_attachment_grants_use_canonical_group_message_id -- --nocapture`：1 passed, 0 failed。
- `cd message-service && cargo test -p im-group forwarded_group_attachment_grants_reject_different_remote_group -- --nocapture`：1 passed, 0 failed。
- `cd message-service && cargo test -p im-group group_e2ee_send_attachment_grant_refs_write_group_e2ee_grant_after_acceptance -- --nocapture`：1 passed, 0 failed。
- `cd message-service && cargo test -p im-group group_e2ee -- --nocapture`：22 passed, 0 failed。
- `cd message-service && cargo check -p message-service`：通过。
- `cd awiki-system-test && uv run --no-sync python -m py_compile tests_v2/cli/test_awiki_cli_direct_local.py tests_v2/cli/test_awiki_cli_group_local.py tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py tests_v2/helpers/__init__.py tests_v2/helpers/awiki_cli.py tests_v2/helpers/awiki_cli_rust_contracts.py`：通过。
- `cd awiki-system-test && AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py -q -rs`：2 passed, 0 failed。
- `cd awiki-system-test && uv run --no-sync python -m pytest tests_v2/cli/test_awiki_cli_selection_defaults.py -q`：2 passed, 0 failed。
- `cd awiki-system-test && AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/cli/test_awiki_cli_direct_local.py -k "secure_direct_attachments" -q -rs`：1 passed, 9 deselected, 0 skipped；配置为 remote/`awiki.info`，direct E2EE 附件完整下载闭环已通过。

失败 / 阻断：

- `cd awiki-system-test && AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/cli/test_awiki_cli_group_local.py -k "group_secure_attachment" -q -rs`：1 failed, 12 deselected, 0 skipped；失败用例 `test_awiki_cli_group_secure_attachment_downloads_plaintext`，配置 `AWIKI_SYSTEM_TEST_MODE=remote`、`E2E_DID_DOMAIN=awiki.info`、message-service `https://awiki.info`，Bob 已能读到 redacted group E2EE attachment manifest，下载取票阶段返回 `service rpc error 6005: no access grant matched the requested attachment context`。本地 `message-service` 已修复 canonical group grant 绑定，远端部署后需复测。
- 本轮提交后复测同一 focused group 命令：1 failed, 12 deselected, 0 skipped, 5.70s；失败点仍为 `attachment.get_download_ticket` 返回 `service rpc error 6005: no access grant matched the requested attachment context`。因此未继续执行 final remote full，避免在已知 `awiki.info` 部署未包含 `message-service:bb06bdf` 时重复扩大失败面。
- 继续复测同一 focused group 命令：1 failed, 12 deselected, 0 skipped, 4.17s；Bob 仍已读到 redacted group E2EE attachment manifest，失败仍发生在下载阶段 `attachment.get_download_ticket` 返回 `service rpc error 6005: no access grant matched the requested attachment context`。远端阻塞未变化，final remote full 仍需等 `awiki.info` 部署 `message-service:bb06bdf` 后执行。
- 历史 remote full：`cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync awiki-system-test`：183 passed, 2 failed, 15 skipped, 0 xfailed/xpassed, 372.22s；当时两个失败分别为 direct/group secure attachment 用例，均由远端 `awiki.info` message-service 未部署 `object-e2ee` attachment policy 触发。当前 direct focused 已通过，因此 remote full 需在 group canonical grant 修复部署后重跑。

## 10. Commit 要求

- `awiki-system-test` 修改创建独立 commit。
- 文档收口如果只在 `e2ee-attachment-cli-rs2`，在该仓创建聚焦 docs/integration commit。
- 如果最终集成修复了 `message-service` 或 `e2ee-attachment-cli-rs2` 代码，在对应仓库创建聚焦 commit。
- Commit 后回填主 Plan 执行台账和第 17 节最终证据。

## 11. 风险、假设与回滚

- 风险：remote 环境不稳定。缓解：记录 focused local/remote 证据和具体远端失败。
- 风险：group E2EE hidden/test-only gate 导致 remote skip。缓解：记录 skip 原因，不把 skip 写成通过。
- 回滚：保留 unit/CLI 能力，暂不宣称系统 E2E 完成；主 Plan 标记 blocked 或 residual risk。
