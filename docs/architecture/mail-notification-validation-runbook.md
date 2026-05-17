# Mail Notification Validation Runbook

## 目的

这份文档用于在目标机器上验证“邮件像普通消息一样通知”这条链路是否真正打通。

验证重点：

1. `awiki-cli` 本地是否能收到邮件通知并写入本地状态
2. `msg inbox --scope all` 是否能把邮件作为统一消息视图展示
3. Hermes 是否能沿用普通消息 webhook 链路把邮件投递出去

## 当前实现结论

在当前实现中，邮件通知链路已经收口为：

```text
message-service websocket
  -> awiki-cli listener 收到 mail.notification
  -> 本地存储为 text/plain + metadata.source_kind=mail
  -> host-notify 标准化为 im.message.received + data.source_kind=mail
  -> Hermes/OpenClaw/CLI 展示按统一消息链路处理
```

换句话说：

- 邮件入口协议仍然可能是 `mail.notification`
- 但 awiki-cli 内部后续主链路已经尽量按“普通消息通知”处理

## 验证前提

验证前，目标机器至少满足：

1. 已使用当前分支编译出的 `awiki-cli-dev`
2. runtime listener 正在运行，并已连接到 websocket
3. 如果要验证 Hermes 投递：
   - Hermes 已安装
   - `awiki-cli runtime host-notify hermes setup` 已执行
   - 目标平台的 home channel 已设置好

## 建议的验证顺序

按下面顺序验证，排障最省时间。

### 第一步：确认当前身份与 listener 状态

先确认当前 handle：

```bash
./awiki-cli-dev id current
```

建议确认 listener 已经在运行：

```bash
./awiki-cli-dev runtime listener status
```

如果你本地是以前台方式跑 listener，也可以直接看 listener 终端日志。

### 第二步：发一封测试邮件

向当前 handle 对应的 awiki 邮箱发一封测试邮件。

建议测试邮件具备这些特征，便于识别：

- 明确 subject
- 明确正文前几行
- 最好包含唯一标识，例如时间戳或随机串

例如：

- Subject: `Mail pipeline validation 2026-04-23 23:10`
- Body: `hello from validation machine`

### 第三步：验证本地 `mail notify`

先看本地 mail notification 视图里有没有记录：

```bash
./awiki-cli-dev mail notify --limit 10
```

预期：

- 能看到刚才那封邮件对应的一条本地通知
- 结果里应带邮件标题和摘要
- 新写入的数据应有 `source_kind=mail`

说明：

- `mail notify` 是本地视图，不依赖 Hermes
- 如果这里都没有，问题通常还在 awiki-cli listener 之前或本地落库阶段

### 第四步：验证 `msg inbox --scope all`

这是当前最关键的一步。

执行：

```bash
./awiki-cli-dev msg inbox --scope all --limit 20
```

预期：

- 能看到刚才那封邮件
- 该邮件会和普通消息一起出现在统一 inbox 里
- 标题前应体现邮件属性，例如 `[邮件] ...`
- 结果里应带 `source_kind=mail`

说明：

- 现在 `all` 视图会优先走 unified direct inbox cache
- 邮件应作为 direct-like 本地通知被纳入主视图

### 第五步：如果启用了 Hermes，验证 host-notify 配置

先看 Hermes 配置状态：

```bash
./awiki-cli-dev runtime host-notify hermes status
```

预期：

- `sink = hermes`
- notify route ready
- secret configured
- bridge healthy
- 对应平台 home channel 已配置

如果没配，重新执行：

```bash
./awiki-cli-dev runtime host-notify hermes setup --deliver <platform>
```

例如：

```bash
./awiki-cli-dev runtime host-notify hermes setup --deliver telegram
```

### 第六步：验证 Hermes 最终投递

如果普通私信 webhook 已确认正常，那么只需要看邮件是否也能沿同一路径被 Hermes 投递出去。

重点观察 Hermes 侧是否出现：

- webhook route 命中
- 未再报 `Skill 'notify' not found`
- 最终目标平台收到一条明确表现为“邮件”的通知

当前 Hermes 默认 prompt 应该优先根据以下字段识别邮件：

- `data.source_kind=mail`
- `mailbox_address`
- `from_addr`
- `subject`
- `preview`

所以最终投递的标题或正文里，应能明确看出这是邮件，而不是普通私信。

## 如果验证失败，按这个顺序排查

### 情况 1：`mail notify` 没有

优先排查：

1. message-service websocket 是否真的推送了 `mail.notification`
2. listener 是否在运行
3. listener 是否把邮件通知写进了本地 sqlite

### 情况 2：`mail notify` 有，但 `msg inbox --scope all` 没有

优先排查：

1. 当前是否使用的是新编译的 `awiki-cli-dev`
2. 是否实际走的是 websocket 模式
3. 本地 unified direct inbox 查询是否读到了该记录
4. 该记录是否带 `source_kind=mail`

### 情况 3：`msg inbox --scope all` 有，但 Hermes 没投递

优先排查：

1. `runtime host-notify hermes status`
2. Hermes route 是否已由最新 `setup` 写回
3. Hermes 默认 prompt 是否已经是 `source_kind=mail` aware
4. Hermes 机器上是否还有旧 route 或旧 bridge 在运行

### 情况 4：Hermes 收到了，但目标平台没消息

优先排查：

1. route `deliver` 是否正确
2. 对应平台 home channel 是否已经设置
3. Hermes 日志里是否仍有 route / adapter / platform delivery 报错

## 建议你在目标机器上执行的最小命令集

如果你想最短路径验证，按这个顺序执行就够了：

```bash
./awiki-cli-dev id current
./awiki-cli-dev runtime listener status
./awiki-cli-dev mail notify --limit 10
./awiki-cli-dev msg inbox --scope all --limit 20
./awiki-cli-dev runtime host-notify hermes status
```

如果 Hermes 没配好，再补：

```bash
./awiki-cli-dev runtime host-notify hermes setup --deliver <platform>
```

## 验证通过的判定标准

满足下面这些条件，就可以认为本轮改动通过：

1. 邮件出现在 `mail notify`
2. 邮件出现在 `msg inbox --scope all`
3. 邮件展示明确带有邮件语义
4. 如果启用了 Hermes，目标平台能收到这条邮件通知
5. 普通私信通知链路没有回归
