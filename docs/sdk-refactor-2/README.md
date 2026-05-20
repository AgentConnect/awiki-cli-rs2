# SDK Refactor 2：IM SDK 与 CLI 边界方案

**状态**：Draft  
**日期**：2026-05-20  
**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**目标目录**：`docs/sdk-refactor-2/`

## 1. 当前结论

`awiki-cli-rs2` 后续应拆成两个可独立演进的模块：

```text
crates/im-core      # Rust IM SDK / 产品能力层
crates/awiki-cli    # CLI 壳：命令解析、配置、路径、输出、daemon/service 管理
```

`im-core` 不是 wire helper、RPC params builder、SQLite helper、crypto helper 的集合。它应该是 **IM 产品能力 SDK**：调用方选择一个身份，随后调用 profile、联系人、私聊、群聊、消息同步、本地状态等高层能力。CLI 与 App 都只做适配，不直接拼 actor、auth path、owner DID、RPC payload、WebSocket frame 或 SQLite 查询。

第一阶段的落地范围刻意收窄：

```text
第一阶段：基础能力 + 多身份 + 登录/session + directory/profile + 私聊文本 + 群聊文本/群管理 + 基础本地状态
后续阶段：附件、realtime runner、加密 direct E2EE、group E2EE、secure outbox、MLS、provider 抽象
```

这样可以先把 CLI 中已经稳定的普通 IM 能力收敛到 SDK，避免一开始就被加密传输、MLS、runtime daemon、provider 抽象拖大改动面。

## 2. 阅读顺序

1. [整体架构](architecture.md)：crate 边界、分层、第一阶段范围、多身份模型、public/internal 边界。
2. [公共接口](public-api.md)：`ImCore`、`ImClient`、identity/auth/directory/messages/groups/local_state 的对外接口草案。
3. [CLI 边界](cli-boundary.md)：CLI 保留职责、命令到 SDK API 的映射、dry-run 和诊断命令处理。
4. [迁移计划](migration-plan.md)：按最小改动落地的阶段、完成判定和测试要求。
5. 模块设计：
   - [core](modules/01-core.md)
   - [identity-auth](modules/02-identity-auth.md)
   - [directory](modules/03-directory.md)
   - [messages](modules/04-messages.md)
   - [groups](modules/05-groups.md)
   - [local-state](modules/06-local-state.md)
   - [realtime](modules/07-realtime.md)
   - [advanced-secure](modules/08-advanced-secure.md)

## 3. 第一阶段范围

| 范围 | 是否进入第一阶段 | 说明 |
| --- | --- | --- |
| 新增 `crates/im-core` | 是 | 先放高层 DTO、错误、路径、SDK façade，然后逐步迁移业务流程。 |
| 多身份 registry | 是 | `ImCore` 是环境入口，`ImClient` 绑定单个身份。 |
| auth/session | 是 | 登录、刷新、ensure session、logout 属于 SDK；路径来自 CLI/App。 |
| profile/directory | 是 | profile、handle/DID resolve、联系人缓存属于基础能力。 |
| 私聊文本 | 是 | `client.messages().send()`、inbox、history、mark-read、conversation projection。 |
| 群聊文本和群生命周期 | 是 | create/get/list/join/leave/add/remove/update/members/messages。 |
| SQLite 本地状态 | 是 | 作为 SDK 内置实现，外部只通过高层接口访问。 |
| 附件 | 否，建议 Phase 2 | 可以预留 DTO，但第一阶段不迁移完整 upload/download。 |
| realtime runner | 否，建议 Phase 2 | 第一阶段最多保留接口占位，不搬 daemon/runtime。 |
| direct E2EE | 否，Phase 3 | 普通消息安全模式先只支持 `Plain` / `DefaultPlain`。 |
| group E2EE / MLS | 否，Phase 3 | KeyPackage、notice、MLS provider 都不进入第一阶段 public API。 |
| provider 抽象 | 否，Phase 4 | 第一阶段仍使用显式路径、内置 SQLite/HTTP 实现。 |

## 4. 一句话边界

**`im-core` 负责“选择身份后能做哪些 IM 业务，以及这些业务如何编排”；`awiki-cli` 负责“命令行输入、本机配置、本机路径、本机服务和输出格式如何适配这些业务”。**
