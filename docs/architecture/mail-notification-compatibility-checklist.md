# Mail Notification Compatibility Checklist

## 目标

这份清单用于回答两个问题：

1. 现在邮件通知链路里哪些兼容层还能删。
2. 哪些兼容层当前还不能删，否则会影响已正常工作的能力。

评估原则：

- 不影响普通私信通知
- 不影响 OpenClaw / Hermes 等其他平台
- 不改变 message-service 当前协议输入
- 优先删除“内部已无生产路径会生成”的旧代码

## 可以直接删除

### 1. `MailNotificationData` 旧的 Go 内部 payload 类型

结论：可以直接删除。

原因：

- 当前 `NormalizeHostNotification()` 处理 `mail.notification` 时，已经统一输出为：
  - `topic = im.message.received`
  - `data = DirectMessageNotificationData`
  - `data.source_kind = mail`
- 当前运行时已没有生产路径继续构造 `MailNotificationData`
- 旧类型只剩 OpenClaw 内部兼容分支和对应测试在使用
- 删除它不会影响 websocket 输入协议，也不会影响 Hermes / OpenClaw 实际收到的通知

删除后需要同步移除：

- OpenClaw 中基于 `MailNotificationData` 的 legacy 分支
- 仅验证 legacy payload 的测试

状态：

- 已删除

## 现在不能删

### 2. 对旧数据 `content_type = "mail.notification"` 的本地兼容读取

结论：现在不能删。

原因：

- 新落库的邮件通知已经改为：
  - `content_type = "text/plain"`
  - `metadata.source_kind = "mail"`
- 但本地 sqlite 中仍可能存在历史记录使用 `content_type = "mail.notification"`
- `msg inbox`、`mail notify`、本地归一化逻辑仍需兼容识别这批旧数据

后续删除前置条件：

- 提供历史数据迁移策略，或确认旧数据窗口可以接受
- 移除依赖旧 content_type 的兼容查询条件

### 3. `readAllLocalMailNotifications()` / `ListNotificationInboxMessages()`

结论：现在不能删。

原因：

- `msg inbox --scope all` 在非 websocket / fallback 路径下，仍会用这条本地 mail cache 读取作为兜底
- 目前它仍承担“不是统一主路径时，邮件不要丢”的责任

后续删除前置条件：

- 先把所有 `allInbox()` 读取场景统一为单一路径
- 确认 fallback 场景也不会丢邮件通知

### 4. `mail notify` CLI 命令及其 `store.ListNotifications()`

结论：现在不能删。

原因：

- `mail notify` 仍然是公开命令入口
- `mail.Service.Notifications()` 和 `store.ListNotifications()` 仍被该命令直接调用
- 删除会造成 CLI 能力回退，不属于“纯兼容清理”

### 5. Hermes prompt 的旧 prompt 迁移识别逻辑

结论：现在不能删。

原因：

- `shouldReplaceNotifyPrompt()` 仍需要识别旧版 prompt 文本并自动迁移
- 这是已有本地 Hermes 配置的兼容能力

后续删除前置条件：

- 确认线上 / 用户侧已没有旧 prompt 存量，或完成版本窗口约定

## 建议的下一批清理顺序

1. 先观察一轮真实邮件通知联调，确认 `source_kind=mail` 语义稳定。
2. 然后再考虑统一本地存储表达，替换 `content_type = mail.notification`。
3. 最后再删除 `readAllLocalMailNotifications()` 这类 fallback 专属路径。
