# Phase 1 Interface：可实施接口方案

**状态**：Draft for implementation  
**目录**：`docs/sdk-refactor/Interface/`  
**适用阶段**：Phase 1 MVP  
**目标**：把现有 `sdk-refactor` 中的高层方案落成第一阶段可以直接编码的接口清单。

## 1. 结论

当前 `docs/sdk-refactor` 已经足够说明架构方向，但还不能直接作为开发接口逐文件实现。主要原因是：

- `public-api.md` 是接口总览，不是 crate 文件级接口规格。
- 部分类型仍是草案形态，例如 `Url`、`RuntimePaths`、`PeerRef(String)` 与后续领域类型混用。
- P1 / P2+ 虽然有标注，但没有给出 P1 必须实现的最小方法集合、DTO 字段、错误映射和测试验收。
- CLI adapter 的边界写清楚了，但没有落到具体 adapter 文件、函数签名和 handler 迁移顺序。
- 没有明确哪些现有 CLI 类型只能作为 legacy adapter 内部输入，不能进入 `im-core` public API。

所以本目录只做一件事：**给 Phase 1 提供可以照着建 `crates/im-core` 的接口规格。**

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

## 4. 使用方式

开发顺序建议：

```text
1. 按 01-crate-layout.md 创建 crates/im-core。
2. 按 02-core-interface.md 定义核心类型，先不搬业务逻辑。
3. 按 03-identity-auth-interface.md 封装 identity/auth façade。
4. 按 04-message-interface.md 封装 direct/group text message façade。
5. 按 05-cli-adapter-interface.md 改第一批 CLI handler。
6. 用 07-phase1-acceptance.md 做验收。
```

## 5. 兼容原则

Phase 1 可以先在 `im-core` 内部通过 legacy adapter 调用 `awiki-cli` 当前底层实现，但 public API 必须是本目录定义的高层接口。也就是说：

```text
允许：im-core internal adapter -> old message/auth/identity implementation
不允许：SDK public API 暴露 old SendRequest / InboxRequest / ActorContext / RPC params
```

当某个业务能力迁移完成后，应同步把对应旧模块的 re-export 从 `pub use` 收紧为 `pub(crate)` 或移动到 `internal`。
