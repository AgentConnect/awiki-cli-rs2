# Step 02：服务端 E2EE Access Grant

主 Plan：[../plan.md](../plan.md)  
Step index：02  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `message-service: release/0526` |
| Started |  |
| Completed |  |
| Commit |  |
| Review evidence |  |
| Verification evidence |  |
| Next action | 等 Step 01 完成后启动 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：direct/group E2EE 消息在最终 accepted 后，服务端基于非秘密 `client.attachment_grant_refs` 创建 P7 Access Grant。
- 用户 / 系统可见行为：接收方可对 E2EE 附件获取 Download Ticket；缺少或篡改 grant refs 时 ticket 稳定失败。
- 非目标：服务端不解密 P5/P6 inner plaintext，不解析对象密钥，不公开 E2EE discovery。
- 完成标准：direct 和 group E2EE grant refs 通过正负测试；跨域转发不带 `client` context；ticket binding 支持 `direct-e2ee/group-e2ee`。

## 3. 设计方法

- 设计边界：`client.attachment_grant_refs` 是域内 sender-home 授权提示，不是标准跨域 body。
- 核心决策：grant refs 不含 key/nonce，只含 object_uri、attachment_id、ciphertext size/digest、mode、plaintext_size 等非秘密字段。
- 契约 / API / 数据流：sender-home 在 direct/group E2EE 最终 accepted 后校验 grant refs 和 `attachment_objects`，再写 `attachment_access_grants`。
- 兼容性：base attachment collector 保持不变；无 `client.attachment_grant_refs` 的 E2EE 消息仍可 accepted，但下载 ticket 返回 grant not found。
- 迁移策略：复用 `attachment_access_grants` 表，不新增表。
- 风险控制：正负测试覆盖 owner mismatch、digest mismatch、missing refs、client context 不跨域转发。

## 4. 实现方法

1. 在 `message-service/crates/im-attachment/src/manifest.rs` 新增 E2EE grant refs DTO 和 collector：
   - `collect_direct_e2ee_access_grants(storage, sender_did, recipient_did, req)`
   - `collect_group_e2ee_access_grants(storage, sender_did, group_did, req)`
2. collector 从 `req.client_context.attachment_grant_refs` 读取数组；为空时返回 `Ok(None)` 或 `Ok(Some(vec![]))`，由调用方决定是否强制。
3. 校验每个 ref：
   - `attachment_id`、`object_uri` 非空；
   - `digest.alg = sha-256`；
   - `object_encryption_mode = object-e2ee` 或 `none`；
   - `object-e2ee` 必须有 `plaintext_size`；
   - object 存在、committed、owner_did 等于 sender_did；
   - object 的 attachment_id/object_uri/mime/size/digest/mode/plaintext_size 与 ref 一致；
   - ref 不允许 key/nonce 字段。
4. direct E2EE accepted 后写 direct grants：
   - 为 sender 和 recipient 各写一条 `message_target_did` grant。
   - `message_security_profile = direct-e2ee`。
5. group E2EE accepted 后写 group grant：
   - `group_did = target group`。
   - `message_security_profile = group-e2ee`。
6. 更新 `get_download_ticket`，允许 E2EE profiles 查询 grant。
7. 确认 `build_forwarded_request()` 继续只转发 `meta/auth/body`，不转发 `client`。
8. 补 direct/group service tests。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `message-service/crates/im-attachment/src/manifest.rs` | 新增 E2EE grant refs collector 和校验 | 核心实现 |
| `message-service/crates/im-direct/src/service.rs` | direct E2EE accepted 后调用 collector | 不解密 body |
| `message-service/crates/im-group/src/handlers.rs` | group E2EE accepted 后调用 collector | hidden/test-only P6 保持 |
| `message-service/crates/im-attachment/src/handlers.rs` | ticket profile/grant query 调整 | 承接 Step 01 |
| `message-service/docs/api/ANP-client-server-api-attachment.md` | 文档化 `client.attachment_grant_refs` 为域内实现提示 | 不写成跨域标准字段 |
| `message-service/docs/architecture/direct-e2ee-service-boundary.md` | 补充 E2EE 附件 grant 边界 | 如行为变化需要 |
| `message-service/docs/architecture/group-e2ee-service-boundary.md` | 补充 group E2EE 附件 grant 边界 | 如行为变化需要 |

## 6. 依赖

- 前置步骤：Step 01。
- 外部文档或决策：`e2ee-attachment-cli-rs2/docs/e2ee-attachment/e2ee-attachment-transfer-design.md` 的 Access Grant 设计。
- 环境前提：message-service tests 可运行；group E2EE hidden test config 保持现状。

## 7. 验收标准

- [ ] direct E2EE 消息可基于 grant refs 创建 direct grants。
- [ ] group E2EE 消息可基于 grant refs 创建 group grant。
- [ ] grant refs 不含 key/nonce，且服务端会拒绝未知 key/nonce 字段。
- [ ] `attachment.get_download_ticket` 对 direct-e2ee/group-e2ee 使用正确 grant 绑定。
- [ ] `client` context 不跨域转发。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd message-service && cargo fmt --all --check` | 格式通过。 |
| Attachment grant tests | `cd message-service && cargo test -p im-attachment grant -- --nocapture` | collector 正负测试通过。 |
| Direct tests | `cd message-service && cargo test -p im-direct direct_e2ee -- --nocapture` | direct E2EE 回归和 grant tests 通过。 |
| Group tests | `cd message-service && cargo test -p im-group group_e2ee -- --nocapture` | group E2EE 回归和 grant tests 通过。 |
| Build | `cd message-service && cargo check -p message-service` | 服务入口编译通过。 |
| Secret grep | `cd message-service && rg -n "object_key_b64u|nonce_b64u|plaintext" crates/im-attachment/src crates/im-direct/src crates/im-group/src` | 只允许测试/校验错误消息/文档性字段名，生产不存秘密。 |

## 9. Review 环节

Review 重点：

- `client.attachment_grant_refs` 是否被错误转发给远端。
- E2EE grant 是否只在最终 accepted 后写入。
- grant query 是否绑定 `message_id + attachment_id + object_uri + security_profile + target/group`。
- group ticket 是否仍检查当前 membership。
- 服务端是否仍完全 opaque，不解析 E2EE inner plaintext。

## 10. Commit 要求

- Commit 前记录 `message-service` 的 `git status --short --branch`。
- Commit 只包含 E2EE grant refs、ticket、direct/group 接线、测试和文档。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

## 11. 风险、假设与回滚

- 风险：grant refs 被伪造授权他人对象。缓解：必须校验 object owner 和 authoritative object metadata。
- 风险：E2EE 消息 accepted 但无 grant。缓解：SDK Step 04 保证产品路径带 refs；服务端 ticket 返回明确 grant not found。
- 回滚：移除 E2EE collector 调用，保留 Step 01 object-e2ee 上传但不可下载。
