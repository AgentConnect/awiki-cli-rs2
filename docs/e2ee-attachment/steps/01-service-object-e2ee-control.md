# Step 01：服务端 P7 object-e2ee 控制面

主 Plan：[../plan.md](../plan.md)  
Step index：01  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `message-service: release/0526` |
| Started | 2026-06-02 09:06 CST |
| Completed | 2026-06-02 09:13 CST |
| Commit | `message-service:b93964bfe59bcdd200375c69a29f00ff0de55855` |
| Review evidence | Review 覆盖 policy 组合、key/nonce 不进入控制面、ticket 仍依赖 grant、commit 失败路径；发现并修复 focused test 未覆盖新增用例和 `plaintext_size` 解析时机。 |
| Verification evidence | `cd message-service && cargo fmt --all --check` 通过；`cargo test -p im-attachment attachment -- --nocapture` 7 passed, 0 failed, 15 filtered；`cargo test -p im-types attachment -- --nocapture` 0 passed, 0 failed, 0 filtered；`cargo check -p message-service` 通过；`git diff --check` 通过。 |
| Next action | 回到主 Plan，启动 Step 02。 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`message-service` 的 P7 控制面接受合法 `object-e2ee` 对象上传、提交和 ticket 查询上下文。
- 用户 / 系统可见行为：发送端可为 direct/group E2EE 附件申请 slot、上传密文、commit 密文对象；服务端仍拒绝 `transport-protected + object-e2ee`。
- 非目标：不创建 E2EE Access Grant，不处理客户端加密，不开放 public E2EE discovery。
- 完成标准：控制面字段策略、服务端 API 文档、单元测试和 focused crate 验证通过；服务端不保存对象密钥或 nonce。

## 3. 设计方法

- 设计边界：只修改 P7 object control；不改 P5/P6 消息路径。
- 核心决策：`object-e2ee` 只影响对象字节、size/digest/plaintext_size 元数据；密钥仍只在客户端。
- 契约 / API / 数据流：`create_slot` 允许 `intended_message_security_profile in direct-e2ee/group-e2ee` 与 `object_encryption_mode=object-e2ee`；`commit_object` 对 `object-e2ee` 要求 `plaintext_size`。
- 兼容性：`transport-protected + none` 路径保持不变；`transport-protected + object-e2ee` 继续拒绝。
- 迁移策略：现有 migration 已允许枚举，除非发现字段缺失，不新增 migration。
- 风险控制：保留 `deny_unknown_fields`，新增测试证明 `object_key_b64u` / `nonce_b64u` 无法进入控制面。

## 4. 实现方法

1. 更新 `message-service/docs/api/ANP-client-server-api-attachment.md`，把当前 “object-e2ee 未实现” 调整为受限支持，并写明控制面不得传 key/nonce。
2. 在 `message-service/crates/im-attachment/src/handlers.rs` 拆分当前 `validate_base_only_slot_policy`：
   - `transport-protected + none` 允许；
   - `direct-e2ee/group-e2ee + none` 允许；
   - `direct-e2ee/group-e2ee + object-e2ee` 允许；
   - `transport-protected + object-e2ee` 拒绝 `anp.attachment.encryption_policy_violation`。
3. `commit_object_local` 不再把 policy 固定为 `transport-protected`；使用 slot 的 `intended_message_security_profile` 和 body mode 做一致性校验。
4. `CommitObjectBody.plaintext_size_i64()` 对 `object-e2ee` 必须返回 `Some`，对 `none` 可以为 `None`。
5. `get_download_ticket` 的 request validation 接受 `message_security_profile = direct-e2ee | group-e2ee`，但没有 grant 时仍返回 grant not found。
6. 补单元测试：
   - `create_slot` object-e2ee accepted。
   - `transport-protected + object-e2ee` rejected。
   - `commit_object` 缺少 `plaintext_size` rejected。
   - 控制面带未知 key/nonce 字段 rejected。
   - ticket body 接受 E2EE security profile。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `message-service/crates/im-attachment/src/handlers.rs` | 控制面 policy、commit 校验、ticket profile 校验 | 主要实现 |
| `message-service/crates/im-types/src/attachment.rs` | 如需补 serde / DTO 测试则修改 | 不增加密钥存储 |
| `message-service/docs/api/ANP-client-server-api-attachment.md` | 更新 object-e2ee 支持状态和约束 | 文档同步 |
| `message-service/docs/architecture/v1-success-and-proof-policy.md` | 如控制面策略变更需要同步 | 仅更新事实 |

## 6. 依赖

- 前置步骤：无。
- 外部文档或决策：`anp/AgentNetworkProtocol/message/07-attachments-and-object-transfer.md`。
- 环境前提：`message-service` Rust workspace 可 cargo test。

## 7. 验收标准

- [ ] `attachment.create_slot` 支持合法 `direct-e2ee/group-e2ee + object-e2ee`。
- [ ] `attachment.commit_object` 对 `object-e2ee` 要求 `plaintext_size` 且不接受 key/nonce。
- [ ] `attachment.get_download_ticket` request validation 接受 E2EE message security profile。
- [ ] plain attachment 既有行为和测试不回归。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd message-service && cargo fmt --all --check` | 格式通过。 |
| Attachment tests | `cd message-service && cargo test -p im-attachment attachment -- --nocapture` | object-e2ee 控制面正负测试通过。 |
| Types tests | `cd message-service && cargo test -p im-types attachment -- --nocapture` | DTO/serde 如有测试通过。 |
| Build | `cd message-service && cargo check -p message-service` | 服务入口编译通过。 |
| Diff | `cd message-service && git diff --check` | 无 whitespace 错误。 |

## 9. Review 环节

Review 重点：

- 是否有任何路径把 `object_key_b64u` 或 `nonce_b64u` 存入服务端结构、日志或响应。
- `transport-protected + object-e2ee` 是否稳定拒绝。
- `object-e2ee` size/digest 是否仍指向密文字节。
- 既有 plain attachment 行为是否保持。
- API 文档是否和实现一致。

## 10. Commit 要求

- Commit 前记录 `message-service` 的 `git status --short --branch`。
- Commit 只包含本步骤服务端控制面、测试和文档。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

## 11. 风险、假设与回滚

- 风险：误把 `object-e2ee` 当服务端加密模式。缓解：文档和测试明确服务端只存密文。
- 风险：改变 plain attachment。缓解：运行既有 attachment tests。
- 回滚：恢复 base-only policy，E2EE 附件暂不可上传。
