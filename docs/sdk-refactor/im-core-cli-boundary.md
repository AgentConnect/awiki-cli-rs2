# im-core 与 cli 边界设计索引

**状态**：Final Draft  
**日期**：2026-05-21  
**适用仓库**：`awiki-cli-rs2`

本文件保留为历史入口和快速索引。最终方案以本目录下的 `README.md`、`architecture.md`、`public-api.md`、`cli-boundary.md`、`implementation-playbook.md` 和 `modules/*` 为准。

## 当前结论

- `im-core` 是 IM 产品能力层，不是低层 helper 集合。
- `awiki-cli` 是命令行适配层，负责命令解析、配置读取、workspace 路径解析、文件权限、输出渲染、daemon/service 管理和 host notify UX。
- 第一阶段采用 **路径参数版**，不先引入 provider。
- 第一阶段 MVP 收窄为 **core 框架 + 多身份 + 身份鉴权 + Handle 注册 + 私聊文本 + 群聊文本 + 必要 inbox/history**。
- 附件、完整 directory/profile、完整 group lifecycle、本地 conversation projection、secure、realtime、provider 抽象后续逐步迁移。
- 依赖方向只能是 `awiki-cli -> im-core`。
- `im-core` 不依赖 `ParsedCommand`、`GlobalOptions`、`ExitError`、CLI config resolver、CLI workspace resolver、OpenClaw/Hermes 或 service manager。

## 推荐阅读顺序

1. [README](README.md)
2. [整体架构](architecture.md)
3. [公共接口](public-api.md)
4. [CLI 边界](cli-boundary.md)
5. [实现执行手册](implementation-playbook.md)
6. [合并决策](merge-decisions.md)
7. `modules/*`

## 模块接口文档

| 模块 | 文档 | 主要内容 |
| --- | --- | --- |
| `core` | [modules/01-core.md](modules/01-core.md) | `ImCore`、`ImClient`、配置、路径、错误、bootstrap |
| `identity` | [modules/02-identity.md](modules/02-identity.md) | 多身份 registry、Handle 注册、恢复、profile、replace DID |
| `auth` | [modules/03-auth.md](modules/03-auth.md) | DID auth、JWT/session、refresh、ensure session |
| `local_state` | [modules/04-local-state.md](modules/04-local-state.md) | SQLite、本地 owner 隔离、schema、projection |
| `discovery` | [modules/05-discovery.md](modules/05-discovery.md) | DID document、capability、endpoint selection |
| `directory` | [modules/06-directory.md](modules/06-directory.md) | 联系人、handle lookup、profile 投影 |
| `messages` | [modules/07-messages.md](modules/07-messages.md) | 私聊/群聊发送、inbox、history、后续 mark-read/conversation |
| `groups` | [modules/08-groups.md](modules/08-groups.md) | 完整群生命周期、成员、群消息，Phase 3 下沉 |
| `attachments` | [modules/09-attachments.md](modules/09-attachments.md) | 附件上传、manifest、发送、下载，Phase 4 下沉 |
| `secure` | [modules/10-secure.md](modules/10-secure.md) | direct E2EE、group E2EE、secure outbox，Phase 6 下沉 |
| `realtime` | [modules/11-realtime.md](modules/11-realtime.md) | 可嵌入 realtime runner、事件流，Phase 5 下沉 |
