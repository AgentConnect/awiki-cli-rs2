# Phase 1 Interface：可实施接口方案

**状态**：Final draft for Phase 1 implementation  
**目录**：`docs/sdk-refactor/Interface/`  
**适用阶段**：Phase 1 MVP  
**目标**：把 `docs/sdk-refactor/` 的高层方案落成第一阶段可以直接编码的接口清单。

## 1. 结论

当前 `docs/sdk-refactor` 已经说明了架构方向，但真正执行 Phase 1 时还需要一个更具体的接口规格。本目录只做一件事：**给 Phase 1 提供可以照着创建 `crates/im-core`、CLI adapter 和 façade 的接口清单。**

本目录遵守主方案中的约束：

```text
awiki-cli -> im-core
```

`im-core` 不能依赖 `awiki-cli`，不能引用 `ParsedCommand`、`Resolved`、`Manager`、`ExitError` 等 CLI 类型。

## 2. 第一阶段只实现什么

P1 只实现 SDK 主链路：

```text
ImCore / ImClient
多身份选择
身份加载与摘要
auth login / ensure / refresh / status
Handle 注册
普通私聊文本发送
普通群聊文本发送，面向已有 GroupRef
必要 inbox/history
path bootstrap / validate / migrate minimal
CLI adapter 到 SDK DTO
```

P1 不实现：

```text
完整 directory/profile
完整 group lifecycle/member management
conversation projection
mark-read
attachment upload/download
realtime runner
secure direct / group E2EE
provider traits
async API
```

## 3. 文档清单

| 文件 | 内容 |
| --- | --- |
| `01-crate-layout.md` | P1 crate 文件结构、feature、依赖和 compile fence。 |
| `02-core-interface.md` | `ImCore`、`ImClient`、config、paths、ids、error、bootstrap 精确定义。 |
| `03-identity-auth-interface.md` | P1 身份、多身份、Handle 注册、auth/session 接口。 |
| `04-message-interface.md` | P1 私聊/群聊文本、inbox/history、消息 DTO 和 unsupported 能力行为。 |
| `05-cli-adapter-interface.md` | CLI 到 SDK 的 adapter 函数、handler 目标形态、错误映射。 |
| `06-implementation-map.md` | 现有 CLI 模块到 P1 SDK 接口的迁移映射和 public/internal 规则。 |
| `07-phase1-acceptance.md` | P1 验收测试、fixture、边界检查和完成标准。 |
| `08-email-interface.md` | Email / Mail 迁移阶段的 SDK service、DTO、wire contract、CLI adapter 和 Dart facade 边界。 |

## 4. 使用方式

开发顺序建议：

```text
1. 按 01-crate-layout.md 创建 crates/im-core。
2. 按 02-core-interface.md 定义核心类型，先不搬业务逻辑。
3. 按 03-identity-auth-interface.md 封装 identity/auth façade。
4. 按 04-message-interface.md 封装 direct/group text message façade。
5. 按 05-cli-adapter-interface.md 增加 CLI adapter。
6. 用 07-phase1-acceptance.md 做验收。
```

Email / Mail 不属于 Phase 1 MVP。独立 Email 阶段按 `08-email-interface.md` 和 `../plan/email-migration-execution-plan.md` 迁入 SDK；CLI 默认命令面打开后，`mail.*` 必须走 `im-core::email`，不能回退到 legacy mail implementation。

## 5. 兼容原则

Phase 1 分成两个层次，避免破坏依赖方向。

### 5.1 P1-alpha：CLI adapter 调旧模块

P1-alpha 允许：

```text
CLI handler
  -> awiki-cli::im_core_adapter
     -> current awiki-cli low-level identity/auth/message implementation
```

此时：

```text
crates/im-core 只提供 public API / DTO / façade shape
awiki-cli::im_core_adapter 负责把 SDK DTO 转成旧 request
旧 identity/authsdk/message/store 测试继续保留
```

P1-alpha 不允许：

```text
im-core -> awiki-cli old modules
im-core -> ParsedCommand / Resolved / Manager / ExitError
```

### 5.2 P1-beta：im-core 内部 legacy module 调已迁入代码

P1-beta 允许：

```text
im-core public API
  -> im-core internal legacy module
     -> code copied/moved into crates/im-core/internal
```

这里的 legacy module 必须已经在 `crates/im-core` 内部，不能反向依赖 `crates/awiki-cli`。

### 5.3 public API 规则

不允许 SDK public API 暴露：

```text
old SendRequest / InboxRequest
ActorContext
IdentityRuntimePaths
AuthStatePaths
LocalStatePaths as business parameter
RPC params
wire payload
raw serde_json payload
SQLite connection
```

当某个业务能力迁移完成后，应逐步把对应旧模块的 re-export 从 `pub use` 收紧为 `pub(crate)` 或移动到 internal / diagnostics feature。
