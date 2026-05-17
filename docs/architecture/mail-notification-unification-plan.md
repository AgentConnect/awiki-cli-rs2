# Mail Notification Unification Plan

## 背景

当前 `awiki-cli` 中“普通私信通知”和“邮件通知”已经能够分别工作，但邮件链路仍然保留了几处独立分支：

- websocket 收到 `mail.notification` 后，会被单独标准化成 `mail.message.received`
- `msg inbox` 会把本地 `mail.notification` 与普通 inbox 结果做合并展示
- OpenClaw / Hermes 下游为了展示邮件，又各自兼容一套 mail 语义

这导致需求本身虽然简单，但实现上存在多处历史分叉，排障时必须跨 listener、inbox、host-notify 和 Hermes route 一起检查。

## 目标

在 **不影响其他平台**、**不影响已经正常工作的私信通知** 的前提下，把邮件通知逐步收口为“普通消息通知”的一个入口适配。

最终希望达到的结构是：

```text
mail.notification -> message-style notification -> websocket/inbox/webhook/Hermes/OpenClaw
```

## 约束

本次重构必须满足以下约束：

1. 不能重写或替换现有普通私信通知主链路。
2. 不能破坏 OpenClaw、Hermes 等其他平台已经正常工作的通知能力。
3. 优先做“加字段、加兼容”的收口，不做“大替换”。
4. 任何阶段都必须能通过回归验证：
   - 普通私信 -> CLI 实时通知正常
   - 普通私信 -> Hermes / OpenClaw 正常
   - 邮件 -> CLI / Hermes 能识别为邮件

## 分阶段方案

### 阶段 1：统一 host-notify 事件语义

目标：

- 邮件不再向下游暴露独立的 `mail.message.received` 主语义
- 邮件改为复用 `im.message.received` 这条已经正常工作的私信通知链路
- 同时保留邮件字段，避免 Hermes / OpenClaw 丢失展示信息

实施方式：

- `listener/host_notify` 中，将邮件标准化为 message-style payload
- 新增 `source_kind=mail`
- 保留邮件字段：
  - `mailbox_address`
  - `from_addr`
  - `subject`
  - `preview`
  - `has_attachments`
- 普通私信显式写 `source_kind=im`
- OpenClaw / Hermes 优先根据 `source_kind=mail` 或邮件字段识别邮件，而不是依赖独立 topic

阶段 1 的结果是：

- 下游主 topic 统一
- 普通私信链路不变
- 邮件仍然能被识别和正确展示

### 阶段 2：统一 inbox 读取模型

目标：

- `msg inbox` 最终只展示一种“通知消息”视图
- 尽量减少 CLI 展示层的 mail 特判和合并逻辑

实施方式：

- 优先在 awiki 自己控制的本地存储读取层做统一
- 展示层只根据 `source_kind=mail` 或 metadata 决定标题前缀 `[邮件]`
- 尽量避免继续扩大 `mail.notification` 专属逻辑

注意：

这一阶段可以晚于阶段 1 执行，因为 inbox 是本地视图问题，不阻塞 Hermes / OpenClaw 通知收口。

### 阶段 3：清理历史 mail 专属分支

目标：

- 删除不再必要的 mail 独立分支
- 将“邮件通知 = 消息通知 + 邮件来源标识”固化为长期结构

候选清理项：

- `mail.message.received` 的旧兼容处理
- 下游只为 mail 额外存在的桥接或格式化分支
- `msg inbox` 中仅用于过渡期的 mail 合并逻辑

## 当前已完成状态

当前代码已经完成了比最初计划更完整的一轮收口，不再只停留在阶段 1。

### 已完成的收口

1. host-notify 主语义已统一

- `mail.notification` 进入 `awiki-cli` 后，会统一标准化为：
  - `topic = im.message.received`
  - `data = DirectMessageNotificationData`
  - `data.source_kind = mail`
- 同时保留邮件字段：
  - `mailbox_address`
  - `from_addr`
  - `subject`
  - `preview`
  - `has_attachments`

2. `msg inbox --scope all` 已统一聚合

- websocket 模式下，`allInbox()` 优先读取本地 unified direct inbox cache
- 邮件通知会直接作为 direct-like 本地消息一起聚合
- 不再依赖“direct inbox 结果 + 独立 mail cache 再拼一次”作为主路径

3. 本地展示结果已统一标识

- 归一化后的本地邮件通知结果会显式带：
  - `source_kind = mail`
- 展示层可以优先用 `source_kind` 判断“这是邮件”，不再只依赖 `content_type`

4. Hermes / OpenClaw 契约已统一

- Hermes 默认 prompt 优先根据 `source_kind=mail` 和邮件字段识别邮件通知
- OpenClaw 只保留一套 mail-like 提取和渲染逻辑
- 旧的 `MailNotificationData` Go 内部 legacy payload 已移除

5. 本地存储表达已进入过渡完成态

- 新落库的邮件通知现在使用：
  - `content_type = "text/plain"`
  - `metadata.source_kind = "mail"`
- 历史数据如果仍是：
  - `content_type = "mail.notification"`
  仍然可以被兼容识别和读取

### 当前等价结构

当前真实结构可以理解为：

```text
message-service websocket
  -> awiki-cli listener 收到 mail.notification
  -> 本地存储为 text/plain + metadata.source_kind=mail
  -> host-notify 标准化为 im.message.received + data.source_kind=mail
  -> msg inbox / Hermes / OpenClaw 按统一消息链路消费
```

## 当前保留的兼容层

虽然主链路已经收口，但以下兼容层仍然保留：

1. 旧本地数据的兼容读取

- 历史 sqlite 数据里可能还存在 `content_type = "mail.notification"`
- 读取逻辑仍兼容这批旧数据

2. `mail notify` 命令

- `awiki-cli mail notify` 仍然保留
- 它读取本地 mail notification 视图，不是本次清理的删除目标

3. fallback mail cache 读取路径

- `readAllLocalMailNotifications()` / `ListNotificationInboxMessages()` 仍保留
- 主要用于非 websocket / fallback 场景，避免邮件通知丢失

更细的可删/不可删结论，见：

- `docs/architecture/mail-notification-compatibility-checklist.md`

## 验证入口

如果你要去另一台机器做真实链路验证，直接看：

- `docs/architecture/mail-notification-validation-runbook.md`
