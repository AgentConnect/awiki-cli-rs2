# SDK Refactor：合并决策与修改原因

## 1. 最终取舍

最终方案以 `docs/sdk-refactor/` 为主方案，吸收 `docs/sdk-refactor-2/` 中更清晰的实施表达。

保留 `sdk-refactor` 的优点：

- 长期模块结构完整：core、identity、auth、local_state、discovery、directory、messages、groups、attachments、secure、realtime。
- Phase 1 范围更小，更符合“先让 SDK 跑起来”的目标。
- 后续附件、secure、realtime 都有独立模块，不会在长期设计中丢失。

吸收 `sdk-refactor-2` 的优点：

- 增加集中式 `public-api.md`。
- 增加 `cli-boundary.md`，给出 handler 目标形态和 adapter 函数。
- 采用 blocking-first 策略。
- 使用 `IdentitySelector::LocalAlias` 替代 `Name`。
- 本地状态长期隔离键优先采用 `owner_identity_id`。
- 明确 public/internal deny list。
- 增加 feature flag 发布规划。
- 把 Phase 1 拆成 P1A ~ P1E，便于执行。

## 2. 为什么收窄 Phase 1

之前的实施草案曾把 directory/profile、完整 group lifecycle、mark-read、conversation projection、本地状态收口都放入第一阶段。这个范围更像“第一轮完整普通 IM SDK”，不适合第一步落地。

最终 Phase 1 只保留：

```text
core 框架
多身份
身份鉴权
Handle 注册
私聊文本
群聊文本
必要 inbox/history
```

原因：

- 可以快速验证 `crates/im-core` 是否能独立编译和测试。
- 可以验证显式路径参数、身份绑定、auth/session、消息发送这条主链路。
- 可以让 CLI P1 handler 尽快切到 SDK façade。
- 避免被 group lifecycle、directory/profile、本地 projection、附件、secure、realtime 拖大。

## 3. 为什么保留完整模块结构

虽然 Phase 1 收窄，但最终方案仍保留 11 个模块文档。原因：

- `im-core` 的长期目标仍然包含附件、secure、realtime、完整群管理和本地状态。
- 模块文档能避免后续迁移时重新争论能力归属。
- 阶段计划可以小，长期边界必须完整。

## 4. 为什么增加 public-api.md

旧方案中接口散落在各模块文档里，阅读成本高。最终方案增加 `public-api.md` 作为接口总览，并用 P1 / P2+ / P3+ 标注阶段。

这样可以同时满足：

- 实现者快速知道 P1 要写什么。
- 架构上提前知道后续接口形态。
- 避免因为接口散落导致 handler 继续调用低层 helper。

## 5. 为什么使用 LocalAlias

不使用：

```rust
IdentitySelector::Name(String)
```

改为：

```rust
IdentitySelector::LocalAlias(String)
```

原因：

- `Name` 容易和 display name 混淆。
- CLI 的 `--identity alice` 本质是本地 credential name / local account alias。
- App 未来也可能有本地账号别名，与 handle/display name 不同。

## 6. 为什么 owner_identity_id 优先

长期本地状态隔离建议：

```text
owner_identity_id 作为稳定主键
owner_did 作为兼容和展示字段
```

原因：

- DID 可能 replace/recover/rebind。
- 如果全部用 `owner_did` 隔离，本地消息、群状态、联系人缓存都需要复杂 rebind。
- `owner_identity_id` 更适合作为多身份本地状态的稳定 owner key。

Phase 1 可以先兼容现有 `owner_did`，但 schema、DTO 和迁移计划应预留 `owner_identity_id`。

## 7. 为什么 blocking-first

Phase 1 使用 blocking API：

```rust
pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
```

原因：

- 当前 CLI 迁移重点是边界收敛，不是异步运行时改造。
- 如果第一阶段同时引入 tokio、async trait、spawn_blocking、lifetime 复杂度，会扩大风险。
- App 可以先在 Flutter plugin/mobile binding/worker thread 中异步调度 blocking SDK。
- 等核心 API 稳定后，再评估 async feature 或 async crate。

## 8. 为什么 P1 不做完整群管理

P1 的“群聊”定义为：

```text
面向已有 GroupRef 发送群文本消息
读取必要 group history
```

不包含：

```text
create/get/list/join/leave/add/remove/update/members
```

原因：

- 私聊和群聊的消息发送主链路更关键。
- 完整群生命周期会引入 policy、profile、member rule、本地 group projection 等大量额外逻辑。
- 这些能力放到 Phase 3 更稳。

## 9. 为什么 P1 不做完整 directory/profile

P1 只允许 `messages().send(Direct)` 内部做最小 target resolve：

```text
DID 直接使用
handle 解析成 DID
```

不公开完整 `DirectoryService`，不做完整 contacts/relation/profile projection。

原因：

- 消息发送需要目标解析，但不需要完整联系人系统。
- `profile` 和 `directory` 对 App 很重要，但不是 SDK 主链路跑通的前置条件。
- 放到 Phase 2 可以避免 P1 范围失控。

## 10. 为什么 P1 不做 conversation projection

`messages().conversations()` 对 App 很重要，但不是第一步跑通 SDK 的前置条件。

最终方案把它放到 Phase 3：

```rust
client.messages().conversations(query)
```

原因：

- conversation projection 涉及 inbox 聚合、thread id 规则、unread、local cache merge、owner isolation。
- 如果放进 P1，会拖大本地状态迁移面。
- Phase 1 只做必要 inbox/history，用于验证消息闭环。
