# Hermes Host Notify 实现说明

## 背景

本文记录 `awiki-cli -> Hermes -> IM` 这条通知链路的两轮改动，说明它们分别解决了什么问题，以及现在整条链路是如何工作的。

关于“邮件通知如何低风险收口到普通消息通知链路”的后续方案，见：

- `docs/architecture/mail-notification-unification-plan.md`
- `docs/architecture/mail-notification-validation-runbook.md`

目标场景：

- `awiki-cli` 在 WebSocket 运行模式下收到 awiki 侧的新消息 / 群组事件
- 这些事件被转换为统一的 host notification
- 本机的 Hermes 接收到通知后，再把消息投递到最终 IM 平台
- 最终 IM 平台可以是 Feishu、Telegram、Discord、Slack 等 Hermes 已支持且具备 home channel 的平台

## 第一轮改动：支持 Hermes sink

第一轮改动对应提交：

- `122fe66b822bf799a741c0ae96a07148cdea62ea`

这一轮主要完成了“打通链路”本身，也就是让 `awiki-cli` 可以把 host notification 发给 Hermes。

核心内容：

- 在 `awiki-cli` 中新增 `runtime.host_notify.sink = hermes`
- 在 listener 中新增 Hermes sink，把 host notification 通过 HTTP POST 发到本机 adapter
- 新增 `scripts/hermes_notify_adapter.py`
- 新增 Hermes 侧的文档、schema、OpenAPI 合同
- 支持基础的 `runtime host-notify hermes set` / `set-secret` / `clear-secret`

这一轮之后，链路已经能工作，但仍然有几个明显问题：

- 用户需要手动改 `~/.hermes/config.yaml`
- 用户需要自己启动 adapter
- `awiki-cli` 并不知道本地 Hermes route 是否真的已经配置好
- 默认交付目标基本按 Feishu 示例来写，平台选择能力不完整

## 第二轮改动：本地 Hermes 全自动接管

第二轮改动是在第一轮基础上继续推进，目标变成：

- 用户装好 `awiki-cli`
- 选择 Hermes 作为 host notify sink
- 在目标 IM 平台里给 Hermes 执行一次 `/sethome`
- 其他本地配置由 `awiki-cli` 自动完成

这一轮主要做了以下事情：

### 1. `awiki-cli` 负责写本地 Hermes route

新增了 `internal/runtime/hermesbridge` 包，负责：

- 定位 `HERMES_HOME` / `~/.hermes`
- 读取和写回 `~/.hermes/config.yaml`
- 自动合并 `platforms.webhook.extra.routes.notify`
- 自动清理旧的 `deliver_extra.chat_id`、`thread_id`、`message_thread_id`
- 检查 route 当前的 deliver 平台、home channel、secret、bridge 健康状态

也就是说，用户不再需要手改 `~/.hermes/config.yaml`。

### 2. `awiki-cli` 负责常驻 bridge

第二轮把本地 adapter 从“手动起脚本”升级成了“由 `awiki-cli` 管理的常驻 bridge service”。

对应的隐藏 service 入口：

- `awiki-cli runtime host-notify hermes bridge service-run`

`awiki-cli runtime host-notify hermes setup` 会自动：

- 安装 bridge service
- 启动 bridge service
- 在后续配置变化时重启 bridge

这样用户不需要记住 `python3 scripts/hermes_notify_adapter.py ...` 这类命令。

### 3. 新增一键 setup / status / guide

第二轮新增了三组关键命令：

- `awiki-cli runtime host-notify hermes guide`
- `awiki-cli runtime host-notify hermes setup`
- `awiki-cli runtime host-notify hermes status`

它们的职责分别是：

- `guide`：生成推荐配置和操作指引
- `setup`：一键写 awiki-cli 配置、本地 Hermes route，并启动 bridge
- `status`：检查整条链路是否 ready

### 4. 投递平台不再写死为 Feishu

最开始的自动化实现默认把 Hermes route 写成：

- `deliver: feishu`

后来进一步改成了可配置：

- `runtime host-notify hermes setup --deliver telegram`
- `runtime host-notify hermes set --deliver slack`

`awiki-cli` 现在会把这个 deliver 平台记录到自己的配置里，然后：

- `guide` 按这个平台生成示例
- `setup` 按这个平台写 Hermes route
- `status` 按这个平台检查对应的 `*_HOME_CHANNEL`

目前支持的 home-channel 驱动平台包括：

- `bluebubbles`
- `discord`
- `email`
- `feishu`
- `log`
- `matrix`
- `mattermost`
- `qqbot`
- `signal`
- `slack`
- `sms`
- `telegram`
- `wecom`
- `weixin`

## 现在的链路原理

当前链路分成三段，全部运行在本机。

### 第一段：awiki-cli listener -> Hermes notify adapter

当 awiki 侧有新消息事件时，`awiki-cli` 的 websocket listener 会调用 Hermes sink，把标准化后的 host notification 发送给本机 adapter：

- URL：`http://127.0.0.1:8765/notify/host-event`

这一步是本机 loopback 调用，不经过外网。

发送内容：

- `HostNotificationEvent` JSON

安全措施：

- `awiki-cli` 会用共享 secret 计算 HMAC
- 请求头带上时间戳和签名
- adapter 会校验签名和时间窗口

### 第二段：Hermes notify adapter -> Hermes webhook platform

本机 adapter 收到 `HostNotificationEvent` 后，会把它转换成 Hermes webhook 能理解的 Notification Surface v1，再转发给本机 Hermes gateway：

- URL：`http://127.0.0.1:8644/webhooks/notify`

这一步同样是本机 loopback 调用，不经过外网。

adapter 的职责：

- 校验来自 `awiki-cli` 的签名
- 做去重
- 把 `HostNotificationEvent` 转成 Hermes route 的 webhook payload
- 使用 Hermes route secret 调用 `webhooks/notify`

当前邮件通知已经开始按“统一消息通知”语义收口：

- 邮件进入 host-notify 后，主 topic 复用 `im.message.received`
- `data.source_kind=mail`
- 同时仍保留 `mailbox_address`、`from_addr`、`subject`、`preview` 等邮件字段

因此 Hermes route 的默认 prompt 不应再强依赖 `mail.message.received`，而应优先根据 `source_kind=mail` 和邮件字段判断邮件通知。

### 第三段：Hermes webhook route -> 最终 IM 平台

Hermes 的 webhook route `notify` 收到这条通知后，会：

- 读取 route 的 `prompt`
- 调用 Hermes agent 格式化消息
- 根据 route 的 `deliver` 字段决定最终投递平台

例如：

- `deliver: feishu`
- `deliver: telegram`
- `deliver: slack`

如果 route 里没有写死 `deliver_extra.chat_id`，Hermes 会回退到该平台的 home channel。

例如：

- `deliver: feishu` -> 使用 `FEISHU_HOME_CHANNEL`
- `deliver: telegram` -> 使用 `TELEGRAM_HOME_CHANNEL`

而 `/sethome` 命令本质上就是给当前来源平台写入对应的 home channel。

## 为什么使用 127.0.0.1

本链路选择 `127.0.0.1` / `localhost` 有几个原因：

- `awiki-cli` 和 Hermes 在同一台机器上运行时，不需要暴露公网接口
- 只用 loopback 可以减少误配置和暴露面
- 签名和 secret 仍然保留，避免本机上其他进程随意伪造请求
- 配置和排障都更简单，端口和方向非常明确

当前默认端口：

- `127.0.0.1:8765`：`awiki-cli` -> Hermes adapter
- `127.0.0.1:8644`：adapter -> Hermes webhook route

## 现在用户需要做什么

在当前实现下，用户通常只需要两步：

1. 运行：

```bash
awiki-cli runtime host-notify hermes setup --deliver <platform>
```

2. 在目标 IM 平台里给 Hermes 发一次：

```text
/sethome
```

例如：

- 发到飞书：`awiki-cli runtime host-notify hermes setup --deliver feishu`
- 发到 Telegram：`awiki-cli runtime host-notify hermes setup --deliver telegram`

然后可以用：

```bash
awiki-cli runtime host-notify hermes status
```

检查是否 ready。

## 这两轮改动分别解决了什么

第一轮解决的是：

- `awiki-cli` 能不能把 host notification 发给 Hermes

第二轮解决的是：

- 用户能不能几乎不手改 Hermes 配置就把链路跑起来
- `awiki-cli` 能不能自查 route / bridge / home channel 是否真的 ready
- 最终投递平台能不能不写死为 Feishu

## 当前结论

现在这套实现已经从“支持 Hermes”演进成了“本地 Hermes 自动托管”：

- `awiki-cli` 负责把 awiki 事件送到本机 adapter
- adapter 负责把标准化事件送到本机 Hermes webhook
- Hermes 再按 route 的 `deliver` 投递到最终 IM 平台
- 默认目标通过各平台的 home channel 管理
- 整条链路默认只依赖本机 `127.0.0.1`

这也是为什么它既安全、可调试，又适合做一键 setup 的原因。

## 本地 release 包验证建议

如果开发阶段想验证“本地改过的 `awiki-cli` 被 Hermes 安装后是否仍然能跑通通知链路”，建议走下面这条路径：

1. 先在本地编译当前工作区的 CLI 二进制：

```bash
cd /home/ecs-user/awiki-cli
/usr/local/go/bin/go build -o /home/ecs-user/awiki-cli/dist/local/awiki-cli ./cmd/awiki-cli
```

2. 再打 npm 包：

```bash
cd /home/ecs-user/awiki-cli
npm pack
```

3. 安装本地 tarball 时，通过环境变量明确告诉 `postinstall` 使用这个本地二进制，而不是去拉线上 GitHub release：

```bash
AWIKI_CLI_LOCAL_BINARY=/home/ecs-user/awiki-cli/dist/local/awiki-cli npm install /home/ecs-user/awiki-cli/awiki-cli-1.0.0.tgz
```

说明：

- `AWIKI_CLI_LOCAL_BINARY` 是开发态本地验证入口
- 它会在 `postinstall` 阶段把指定二进制复制到包内 `bin/awiki-cli`
- 这样验证到的就是“当前工作区代码编译出的本地版本”，而不是线上已发布版本
- 如果 Hermes 的安装过程本质上也是调用 `npm install`，那么只要 Hermes 安装命令继承了这个环境变量，同样适用

不建议只靠 `npm pack` + `--ignore-scripts` 做验证，因为当前 npm tarball 默认并不会稳定携带 `bin/awiki-cli`。

## 验证结果

本轮开发完成后，已经完成以下三层验证，并且全部通过：

### 1. 本地包安装验证通过

验证目标：

- 本地改动打成 npm tarball 后，Hermes 实际安装到的是当前工作区编译出来的 `awiki-cli`
- 安装过程不再被线上 GitHub release 二进制覆盖

验证方式：

- 先本地 `go build`
- 再 `npm pack`
- 安装时通过 `AWIKI_CLI_LOCAL_BINARY` 指向本地编译产物
- 安装后执行 `awiki-cli version`

验证结论：

- 安装后的 CLI 输出 `version: "dev"`
- 说明 Hermes / npm 实际运行的是本地构建版本，而不是线上 release 版本

### 2. 真实消息通知验证通过

验证目标：

- 不再只用 mock payload
- 真实 awiki 消息进入本机后，可以沿着 `awiki-cli -> Hermes bridge -> Hermes webhook -> Feishu` 这条链路送达飞书

验证方式：

- 使用真实 awiki 身份收发消息
- 观察 Hermes webhook、bridge 与 Feishu 的联动结果
- 在飞书中确认收到通知

验证结论：

- 飞书侧已经收到真实 awiki 消息通知
- 说明整条业务链路已经打通，不只是 adapter / webhook 的局部验证通过

### 3. 重启回归验证通过

验证目标：

- 在 Hermes gateway 重启后，通知链路仍然可用

验证方式：

- 重启 Hermes gateway
- 再次发送真实 awiki 消息
- 重新检查飞书通知是否到达

验证结论：

- 重启后通知仍然正常送达
- 说明当前方案不依赖一次性的临时状态，具备基本的可重复运行能力

## 评审结论

本轮收尾 review 后，当前建议保留的改动有：

- `scripts/install.js`
  - 增加 `AWIKI_CLI_LOCAL_BINARY`，用于本地 tarball 验证
- `scripts/hermes_notify_adapter.py`
  - 修复 Python 3.10 兼容性，避免在常见 Linux 环境中启动失败
- `.gitignore`
  - 忽略 `*.tgz`，避免 `npm pack` 产物被再次打进包里
- `internal/runtime/hermesbridge/hermes_config.go`
  - 默认通知模板改为中文
  - 对旧英文默认模板做自动迁移
- `internal/runtime/hermesbridge/hermes_config_test.go`
  - 增加中文模板迁移与“保留用户自定义模板”的测试

本轮 review 后，没有发现必须回退的试验性代码改动。

需要注意的是：

- `~/.hermes/config.yaml` 属于本机运行态配置，不属于 `awiki-cli` 仓库提交内容
- 本地测试时生成的 tarball / build 产物应作为工作区临时文件清理，不应进入版本控制
