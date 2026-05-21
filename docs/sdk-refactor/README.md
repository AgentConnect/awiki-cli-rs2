# SDK Refactor：最终版 IM SDK 与 CLI 边界方案

**状态**：Final Draft  
**日期**：2026-05-21  
**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**目标目录**：`docs/sdk-refactor/`

## 1. 结论

`awiki-cli-rs2` 后续拆成两个 crate：

```text
crates/im-core      # Rust IM SDK / 产品能力层
crates/awiki-cli    # CLI 产品壳：命令解析、配置、路径、输出、daemon/service 管理
```

`im-core` 不是 wire helper、RPC params builder、SQLite helper、crypto helper 的集合，而是一个 **IM 产品能力 SDK**。调用方先选择身份，再调用身份鉴权、Handle 注册、私聊、群聊、后续附件、realtime、secure 等高层能力。

`awiki-cli` 不消失，而是变瘦：

```text
CLI handler = parse flags -> build ImCore/ImClient -> call SDK -> render output
```

CLI/App 都不应该直接拼 actor、auth path、owner DID、RPC payload、WebSocket frame 或 SQLite 查询。

## 2. 第一阶段目标

第一阶段目标不是一次性完成完整 IM SDK，而是先让整体 SDK 能跑起来：

```text
Phase 1 MVP：
core 框架 + 多身份 + 身份鉴权 + Handle 注册 + 私聊文本 + 群聊文本 + 必要 inbox/history
```

第一阶段明确不做：

```text
完整 directory/profile/recover/replace DID
完整 group lifecycle / member management
conversation projection / mark-read 完整本地状态收口
附件 upload/download
realtime runner / daemon
secure direct / group E2EE / MLS / secure outbox
provider 抽象
```

这些能力保留在后续阶段迁移。这样可以先验证 `crates/im-core`、`ImCore` / `ImClient`、显式路径参数、多身份、auth、消息发送这条主链路，而不是一开始就被加密、附件、runtime daemon、完整群管理和 provider 抽象拖大。

## 3. 阅读顺序

1. [整体架构](architecture.md)：crate 边界、Phase 1 MVP、多身份模型、路径参数、blocking-first、public/internal 边界。
2. [公共接口](public-api.md)：按阶段标注的 `ImCore`、`ImClient`、identity/auth/messages/groups/local_state/realtime/secure 接口草案。
3. [CLI 边界](cli-boundary.md)：CLI 保留职责、P1 命令到 SDK API 的映射、handler 目标形态。
4. [迁移计划](migration-plan.md)：P1A ~ P1E 的最小落地顺序，以及后续阶段。
5. [合并决策](merge-decisions.md)：从 `sdk-refactor` 与 `sdk-refactor-2` 吸收/不吸收的设计点和原因。
6. 模块设计：
   - [core](modules/01-core.md)
   - [identity](modules/02-identity.md)
   - [auth](modules/03-auth.md)
   - [local-state](modules/04-local-state.md)
   - [discovery](modules/05-discovery.md)
   - [directory](modules/06-directory.md)
   - [messages](modules/07-messages.md)
   - [groups](modules/08-groups.md)
   - [attachments](modules/09-attachments.md)
   - [secure](modules/10-secure.md)
   - [realtime](modules/11-realtime.md)

## 4. 阶段范围总览

| 能力 | Phase 1 | 后续阶段 | 说明 |
| --- | --- | --- | --- |
| 新增 `crates/im-core` | 是 | - | 先放高层 DTO、错误、路径、SDK façade，再逐步迁移业务流程。 |
| `ImCore` / `ImClient` | 是 | 持续稳定 | `ImCore` 是环境入口，`ImClient` 绑定单个身份。 |
| 多身份 registry | 是 | 持续增强 | P1 支持 list/default/local alias/必要 resolve。 |
| auth/session | 是 | 持续增强 | P1 支持 login/ensure/refresh，按身份隔离 session path。 |
| Handle 注册 | 是 | 持续增强 | `core.identities().register_handle()`。 |
| 私聊文本 | 是 | 持续增强 | `client.messages().send(Direct + Text)`。 |
| 群聊文本 | 是 | 持续增强 | `client.messages().send(Group + Text)`，面向已有 `GroupRef`。 |
| inbox/history | 是，必要子集 | Phase 3 补全 | P1 支持验证消息闭环所需的读取能力。 |
| profile/directory | 否 | Phase 2 | P1 只允许 messages 内部做最小目标解析。 |
| 完整 group lifecycle | 否 | Phase 3 | create/join/add/remove/update/members 后移。 |
| mark-read / conversations | 否 | Phase 3 | 对 App 重要，但不作为 P1 跑通 SDK 的前置条件。 |
| SQLite 本地状态收口 | 最小 bootstrap | Phase 3 | P1 可初始化/校验路径；完整 projection 后移。 |
| 附件 | 否 | Phase 4 | upload/download/manifest 后移。 |
| realtime runner | 否 | Phase 5 | CLI service 管理永远留 CLI。 |
| direct E2EE / group E2EE | 否 | Phase 6 | secure 作为高级能力后移。 |
| provider traits | 否 | Phase 7 | API 稳定后按 App 需求引入。 |

## 5. 一句话边界

**`im-core` 负责“选择身份后能做哪些 IM 业务，以及这些业务如何编排”；`awiki-cli` 负责“命令行输入、本机配置、本机路径、本机服务和输出格式如何适配这些业务”。**
