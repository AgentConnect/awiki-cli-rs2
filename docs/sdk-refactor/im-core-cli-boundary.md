# im-core 与 cli 边界设计索引

**状态**：Draft  
**日期**：2026-05-20  
**适用仓库**：`awiki-cli-rs2`  
**目标读者**：CLI 开发者、App 接入方、IM 引擎开发者、后续重构执行者

本目录记录 `awiki-cli-rs2` 内部拆分为 `crates/im-core` 和 `crates/awiki-cli` 的设计。原来的单一边界文档已经拆成整体架构、模块接口和迁移计划三类文档，避免后续重复维护。

## 1. 当前结论

- `im-core` 是 IM 产品能力层，承载身份、登录、消息、群组、附件、secure direct、group E2EE、realtime 和本地状态等业务能力。
- `cli` 是命令行适配层，保留命令解析、配置读取、workspace 路径解析、数据库初始化触发、私钥文件布局和权限、system service、OpenClaw/Hermes UX、输出渲染。
- 第一阶段采用 **路径参数版**，不先引入 provider：CLI 或 App 传入 DID document、私钥、auth/session、SQLite、E2EE/MLS 等显式路径。
- SQLite、HTTP、WebSocket 等当前底层实现依赖继续保留在 `im-core`；禁止的是依赖上层 CLI 类型和 CLI workspace/config 语义。
- `realtime` 是可嵌入 runner，同一套运行循环同时支持 CLI 后台进程和 App 线程/task。
- 旧的 sibling `awiki-im-core` 失败版本不作为设计输入，不作为目标 API，不作为迁移基线。

## 2. 阅读顺序

1. [整体架构](architecture.md)：crate 边界、依赖方向、CLI 保留职责、Phase A 路径参数、Phase B 可选外部能力。
2. [迁移计划](migration-plan.md)：分阶段迁移顺序、命令映射、完成判定。
3. 模块接口文档：按需要阅读对应模块。

## 3. 模块接口文档

| 模块 | 文档 | 主要内容 |
| --- | --- | --- |
| `core` | [modules/core.md](modules/core.md) | `ImCore`、`ImCoreConfig`、路径总入口、错误、上下文 |
| `identity` | [modules/identity.md](modules/identity.md) | 多身份模型、注册、恢复、绑定、解析、profile、replace DID |
| `auth` | [modules/auth.md](modules/auth.md) | DID auth、JWT/session、refresh、logout |
| `directory` | [modules/directory.md](modules/directory.md) | 联系人、关系、handle lookup、profile 投影 |
| `messages` | [modules/messages.md](modules/messages.md) | 发送、收件箱、历史、已读、本地消息投影 |
| `groups` | [modules/groups.md](modules/groups.md) | 群生命周期、成员、群消息 |
| `attachments` | [modules/attachments.md](modules/attachments.md) | 附件上传、manifest、发送、下载 |
| `secure` | [modules/secure.md](modules/secure.md) | direct E2EE、group E2EE、secure outbox、修复 |
| `realtime` | [modules/realtime.md](modules/realtime.md) | WebSocket、notification、可嵌入 runner、CLI/App 启动模式 |
| `local_state` | [modules/local-state.md](modules/local-state.md) | SQLite 状态、schema、迁移、本地缓存 |
| `discovery` | [modules/discovery.md](modules/discovery.md) | DID document、service capabilities、endpoint 选择 |

## 4. 一句话边界

**im-core 负责“AWiki IM 能做什么以及业务流程怎么走”；cli 负责“这个能力在命令行、本机文件、本机数据库、本机服务和本机密钥环境里怎么运行”。**
