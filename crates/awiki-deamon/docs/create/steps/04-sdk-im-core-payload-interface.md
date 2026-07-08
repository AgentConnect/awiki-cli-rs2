# 步骤 04：`im-core` payload 接口

主计划：[../plan.md](../plan.md)
步骤编号：04
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 2026-05-31 02:09:13 CST |
| 完成时间 | 2026-05-31 02:56:44 CST |
| 提交 | `defc907` |
| 审查证据 | Review 已完成：公开 DTO、direct/group 线协议、local projection、history/inbox/realtime payload 解析、Dart bridge、生成文件同步、CLI 兼容展示分支和文档已审查；发现 SDK 文档行尾空格、旧字段字面量验证会命中测试和历史记录、普通 workspace 测试首次在 `im-core-dart` 链接阶段被系统 SIGKILL，均已处理或用低并发重跑验证。 |
| 验证证据 | `cargo fmt --all --check` 通过；`cargo test -p im-core --locked` 通过；`cargo test -p im-core-dart --locked` 通过；`cargo test -p awiki-deamon --locked` 通过；`scripts/flutter/codegen-check.sh` 通过；`CARGO_BUILD_JOBS=1 cargo test --workspace --locked` 通过；`git diff --check -- crates/awiki-cli crates/awiki-deamon/docs/create crates/im-core crates/im-core-dart docs/sdk-refactor packages/awiki_im_core` 通过；旧字段和旧 content type 搜索无结果；daemon 源码/测试 awiki-cli 边界搜索无结果。 |
| 下一步 | 开始步骤 05：message-service payload 支持。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：`im-core` 和 Dart 桥接层将 JSON payload 暴露为稳定的消息 body，不要求 daemon 或 App 直接拼底层 JSON-RPC 参数。
- 可见行为：daemon 和 App 侧调用方可以通过 SDK DTO 发送和接收 `application/json + body.payload` 消息；未知内容不会被静默转成文本。
- 非目标：不修改协议仓库；不实现 daemon runtime；不让 `im-core` 感知 daemon 插件、workspace policy 或本地 `runtime_rpc_token`。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `crates/im-core/src/messages/dto.rs` | 增加 `MessageBody::Payload` 和 `MessageBodyView::Payload` 或等价结构。 | 保留 `Unsupported` 兼容路径。 |
| `crates/im-core/src/messages/service.rs` | 让 payload send/read 进入 message runtime。 | 保持公开接口表达业务意图。 |
| `crates/im-core/src/internal/wire/direct.rs` | 构造 direct `body.payload`。 | `meta.content_type` 固定为 `application/json`。 |
| `crates/im-core/src/internal/wire/group.rs` | 构造 group `body.payload`。 | 与协议仓库契约一致。 |
| `crates/im-core/src/internal/message_runtime/direct.rs` | 支持 direct payload 发送。 | 去掉 text-only 假设。 |
| `crates/im-core/src/internal/message_runtime/group.rs` | 支持 group payload 发送。 | 去掉 text-only 假设。 |
| `crates/im-core/src/internal/message_runtime/read.rs` | 从 inbox/history 解析 payload body view。 | 保留内容类型和 schema。 |
| `crates/im-core/src/internal/local_state/messages.rs` | 持久化 payload body 和元数据。 | 避免 JSON 丢失或字符串化。 |
| `crates/im-core/src/realtime/` | 投影 incoming payload notification。 | runtime event 必须保留 payload body。 |
| `crates/im-core-dart/src/dto/message.rs` | 增加 Dart payload DTO/view。 | 支持 App 桥接。 |
| `crates/im-core-dart/src/mapping/` | 映射 core payload DTO 到 Dart DTO。 | 保留 JSON 对象表达。 |
| `docs/sdk-refactor/modules/07-messages.md` | 更新 messages 模块设计。 | 文档与公开 DTO 对齐。 |
| `docs/sdk-refactor/public-api.md` | 如有需要，更新公开接口总览。 | 继续保持 SDK 边界清晰。 |

## 4. 依赖

- 前置条件：协议仓库的 JSON payload 改动已经完成。
- 外部契约：普通结构化 JSON 使用 `application/json + body.payload`；command/status/result 由 payload 字段识别。
- 环境前提：当前 Rust workspace 能构建和测试。

## 5. 核心设计

`im-core` 应暴露结构化 payload，但不解释 daemon 业务 schema。建议形态：

```rust
MessageBody::Payload {
    payload: serde_json::Value,
}
```

实现时可以保留显式内容类型字段以便扩展，但第一版发送结构化 JSON 时必须归一化为 `application/json`。

必须保留：

- `application/json` 内容类型。
- JSON object payload。
- payload 内的 schema/version 字段。
- payload 内的 command/status/result 业务字段。
- message id、operation id、sender、receiver、group、thread。
- unknown payload schema 或 unsupported content 的兼容路径。

SDK 不应暴露底层 `params`、`auth.origin_proof`、RPC method name 或 daemon-local 授权字段。CLI/App/daemon 只构造 `SendMessageRequest`，由 `im-core` 处理 proof、session、target resolution、线协议参数、本地投影和结果归一化。

## 6. 实施指引

1. 先更新 SDK 设计文档，把 payload 列为 messages 模块的一等 body。
2. 更新 `MessageBody`、`MessageBodyView` 和相关序列化测试。
3. 增加 direct payload 线协议 builder，设置 `meta.content_type = application/json` 和 `body.payload`。
4. 增加 group payload 线协议 builder。
5. 泛化 direct/group sender，让 text 和 payload 共享 proof/session/transport 逻辑。
6. 更新 inbox/history/read 解析，返回 payload body view。
7. 更新 local state schema 或元数据策略，保证 payload JSON 不丢失。
8. 更新 realtime incoming 投影。
9. 更新 Dart DTO 和映射。
10. 增加 direct payload send、group payload send、history/incoming parse、unsupported 回退测试。

## 7. 验收标准

- [x] `MessageBody` 能表达 text、attachment 和 payload。
- [x] `MessageBodyView` 能从 inbox/history/realtime 返回 payload。
- [x] direct 和 group send 能构造 `application/json + body.payload`。
- [x] 本地投影保留 payload JSON 和内容类型，不做字符串化。
- [x] Dart DTO/映射能携带 payload。
- [x] SDK 文档明确 `im-core` 不解释 daemon-specific schema。
- [x] 既有 text 测试继续通过。
- [x] 审查发现已修复或明确记录。
- [x] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| Rust 格式 | `cargo fmt --all --check` | 格式通过。 |
| Rust 测试 | `cargo test --workspace --locked` | 既有和新增 SDK 测试通过。 |
| payload 搜索 | `rg -n "body\\.payload|\"payload\"" crates/im-core crates/im-core-dart docs/sdk-refactor` | 预期层有 payload 支持。 |
| 禁用旧字段 | 搜索旧字段名和 command/status 专用 JSON 内容类型 | 没有旧字段或旧内容类型。 |
| Dart 桥接 | 仓库已有 codegen/check 脚本，或针对性 compile/test | DTO 和映射同步。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查公开接口、线协议兼容、本地持久化、realtime 解析、数据丢失风险、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 已记录 | SDK 文档有一处行尾空格；旧字段字面量会让禁用字段搜索命中测试和阶段 A 历史记录；普通 `cargo test --workspace --locked` 首次在 `im-core-dart` 测试二进制链接阶段被系统 SIGKILL。 |
| 已修复 | 已完成 | 去掉文档行尾空格；删除测试和计划历史记录中的旧字段字面量；使用 `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` 低并发重跑并通过。 |
| 残余风险 | 已明确 | direct-e2ee 和 group-e2ee payload 第一版保持 unsupported；message-service 真实 payload 存储和投递兼容性留到步骤 05 验证。 |
| 测试缺口 | 无阻塞缺口 | 已覆盖 direct/group payload sender、read/history payload 解析、local projection、realtime payload projection、Dart payload DTO 映射和 workspace 回归。 |
| 文档缺口 | 无阻塞缺口 | 已更新 `docs/sdk-refactor/modules/07-messages.md` 和 `docs/sdk-refactor/public-api.md`。 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：`im-core`、`im-core-dart`、直接相关测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`sdk: add payload message body interface`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：local SQLite schema 可能需要迁移；Dart 桥接可能需要生成文件；API 命名可能过早暴露 daemon 语义。
- 回滚：在完整 projection 未完成前，对未知 payload 保留 unsupported/原始 view。
- 后续：步骤 05 必须验证 message-service 返回的 payload shape 能被本步骤 SDK 解析。
