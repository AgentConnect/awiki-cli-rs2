# 步骤 05：message-service payload 支持

主计划：[../plan.md](../plan.md)
步骤编号：05
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/daemon-payload-message-service` |
| 开始时间 | 2026-05-31 03:02:41 CST |
| 完成时间 | 2026-05-31 03:24:37 CST |
| 提交 | message-service `30eecf4` |
| 审查证据 | 已审查 direct/group 协议兼容、存储/realtime 投递、payload 不透明语义、附件和 E2EE 边界、旧字段禁用、测试和文档；发现 group 缺少 content-type/body 严格绑定、group list view 对 binary/attachment/payload 分支不够明确、incoming notification 缺少 payload 保留测试，均已修复。 |
| 验证证据 | `cargo fmt --all --check` 通过；`cargo test -p im-group --locked group_incoming_notification_preserves_json_payload_body` 通过；`cargo test -p im-group --locked group_send` 通过；`cargo test -p im-group --locked group_list_messages_projects_payload_attachment_and_binary_content` 通过；`cargo test -p im-direct --locked json_payload` 通过；`cargo test -p im-direct --locked direct_send_rejects_non_object_json_payload` 通过；`cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`git diff --check -- docs/api crates` 通过；旧字段和旧 content type 搜索无结果。 |
| 下一步 | 开始步骤 06：user-service registration token API。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：message-service 接受、存储、投递并返回 direct/group 的 JSON payload body。
- 可见行为：daemon command/status/result payload 经过发送、WebSocket incoming、history、inbox 后不丢失；message-service 不解释 daemon 命令语义。
- 非目标：不实现 user-service registration token；不实现 daemon 进程；不在 message-service 中授权或执行 daemon command。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `message-service/docs/api/ANP-client-server-api-direct.md` | 更新 direct 请求、响应和 incoming 文档。 | 对齐 `application/json + body.payload`。 |
| `message-service/docs/api/ANP-client-server-api-group.md` | 更新 group 请求、响应和 incoming 文档。 | 保持 group receipt 行为。 |
| `message-service/docs/api/ANP-client-server-api-attachment.md` | 确认 attachment 仍独立于 payload。 | 只在必要时修改。 |
| `message-service/crates/im-direct` | 接受和校验 direct payload body。 | 具体路径执行时确认。 |
| `message-service/crates/im-group` | 接受和校验 group payload body。 | 保持 group policy 和 membership 检查。 |
| `message-service/crates/im-storage` | 存储 body kind、`application/json` 内容类型和 payload JSON。 | 避免 text-only schema 假设。 |
| message-service realtime 模块 | 在 direct/group incoming 通知中投递 payload。 | 不转成文本。 |
| message-service 测试 | 增加 API、存储、realtime 测试。 | 包含兼容性测试。 |

## 4. 依赖

- 前置条件：协议仓库的 JSON payload 改动已经完成。
- 前置步骤：步骤 04 提供 SDK 侧 DTO 和测试夹具。
- 环境前提：message-service 仓库、数据库和测试依赖可用。

## 5. 核心设计

message-service 把 payload 当作消息内容，而不是命令：

- 当 `meta.content_type = application/json` 时，`body.payload` 必须是 JSON object。
- 服务端必须保留 `meta.content_type = application/json`。
- 服务端不得根据内容类型推断 command/status/result；这些语义是不透明的应用层数据。
- proof 校验继续绑定 direct/group 的业务对象。
- 存储、history、inbox、realtime 必须能重建 payload body。
- attachment 对象传输继续独立，不复用 payload。
- direct-e2ee/group-e2ee 对服务端仍是不透明内容；payload 处理只针对服务端可见的 transport-protected body，除非 E2EE 内层契约另有定义。

## 6. 实施指引

1. 阅读 direct/group 处理器和存储 schema。
2. 先更新或同步 API 文档。
3. 如果服务端当前假设 body 只有 text，增加 body enum/model。
4. 增加校验：
   - payload 消息必须使用 `meta.content_type = application/json`。
   - `body.payload` 必须是 JSON object。
   - unsupported 内容类型按协议兼容策略失败或存原始内容。
5. 如当前 schema 只有 text column，设计存储 migration。
6. 更新 WebSocket incoming 和 local view API。
7. 增加 direct payload、group payload、history/inbox 往返校验、incoming notification、text/attachment regression 测试。
8. 增加 payload 为 string/null/array 的负向测试。

## 7. 验收标准

- [x] direct payload 消息能通过 send、存储、read API。
- [x] group payload 消息能通过 send、存储、read API。
- [x] WebSocket incoming 保留 `body.payload`。
- [x] message-service 不授权、不执行 daemon command schema。
- [x] message-service 不按 command/status/result 内容类型分支。
- [x] text 和 attachment 行为不回归。
- [x] API 文档与实现一致。
- [x] 审查发现已修复或明确记录。
- [x] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| Rust 格式 | `cd ../message-service && cargo fmt --all --check` | 格式通过。 |
| Rust 测试 | `cd ../message-service && cargo test --workspace --locked` | 单测和集成测试通过。 |
| API 文档 | `git diff --check -- docs/api` | 文档 diff 干净。 |
| payload 搜索 | `rg -n "body\\.payload|\"payload\"" ../message-service/docs ../message-service/crates` | 文档和代码有预期支持。 |
| 回归测试 | 既有 direct/group/attachment 测试 | 既有路径继续通过。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查协议兼容、proof 边界、存储 migration、安全语义、realtime 行为、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 已处理 | group 原先只校验 `text/payload/payload_b64u` 三选一，未按 `meta.content_type` 绑定 body shape；group 本地消息视图对 JSON payload、附件清单和 binary 的投影分支不够明确；incoming notification 缺少直接验证 `body.payload` 保留的测试。 |
| 已修复 | 已完成 | 为 group.send 增加 `text/plain`、`application/json`、`application/anp-attachment-manifest+json`、其他二进制内容类型的严格 body 字段校验；要求 JSON/附件 `body.payload` 为 object；为 group list view 增加明确投影函数；补 direct payload 往返/负向测试和 group send/list/incoming 测试。 |
| 残余风险 | 可接受 | 本步骤不实现 direct-e2ee/group-e2ee 内层 payload；message-service 现有 storage 已保存完整 `payload_json` / `wire_payload_json`，未新增 migration；跨 SDK 到真实服务的 E2E 留到步骤 08。 |
| 测试缺口 | 已记录 | 已覆盖 direct send/inbox/history/notification、direct 非 object payload 拒绝、group content-type/body 绑定、group list projection、group incoming notification；未跑真实跨进程 WebSocket E2E，后续步骤 08 覆盖。 |
| 文档缺口 | 已修复 | 已更新 group API 文档和 schema examples，说明 `application/json + body.payload`、payload 不透明语义、incoming 保留 payload、本地视图投影规则。 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：message-service payload API、存储、realtime、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`message-service: support payload message bodies`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：存储 schema 可能需要兼容已有 text 消息；proof canonicalization 可能依赖 body shape。
- 回滚：在存储/realtime 能正确保留 payload 前，服务端可临时拒绝 payload。
- 后续：步骤 08 系统测试必须验证 SDK 到 message-service 的 payload 往返校验。
