# Agent 可见消息 V1 契约

状态：`Local implementation candidate / receiver authority contract frozen / real-runtime acceptance pending`

## 1. 唯一 owner 与范围

`docs/architecture/contracts/awiki-agent-message-v1.schema.json` 是
`awiki.agent.message.v1` 的唯一 schema。Rust `im-core` 是 decoder、投影分类、消息正文
安全化、conversation/unread/summary 与 Rust→Dart typed projection 的唯一实现 owner。
CLI 只复用现有 `msg send --payload` 输入面，不增加第二个 notification API。

V1 只允许 Direct、`application/json`、`transport-protected` 普通消息。Group、raw Thread、
附件、Direct E2EE、Group E2EE、URL/object URL、平台 channel/sound/priority/wake/DND 字段
均不属于本契约。`level=normal|urgent` 只表示请求语义；不得直接映射到平台 priority、铃声、
振动或免打扰绕过。

## 2. 闭合 decoder 与安全投影

Core 按以下固定顺序分类：

```text
exact schema == awiki.agent.message.v1
  -> valid visible | invalid visible
other schema startsWith awiki.
  -> control / hidden / read
other JSON
  -> existing ordinary payload behavior
```

compact JSON 最大 8192 UTF-8 bytes；`event_id`、`task_name`、`kind`、`level`、`content`、
`action` 和所有子对象均闭合。`task_name` 是所有 `kind` 必填的 Coding Agent 任务显示名称，
最多 120 Unicode scalar values；它不是任务 ID、路由或授权事实。`summary` 最多 240 Unicode
scalar values，`detail` 最多 2000；出现未知字段、
`detail:null`、空白边界、不可见/control/bidi 字符、credential 形态、绝对路径、fenced code、
`file:` 或 `blob:` 均判为 invalid。

valid projection 只暴露 `event_id`、安全 `task_name`、闭合 `kind`、`requested_level`、安全
`summary/detail` 与 `open_conversation`。`agent_name` 不属于 payload；接收端只能从已验证的
sender/Agent profile 投影显示名称，不能信任 payload 自报。invalid projection 只暴露
`state=invalid`，不暴露 reason、raw JSON 或原文；
App 必须渲染本地化通用占位并按 normal 处理。已知 Group、E2EE 或 raw Thread context 即使正文
有效，也由 Core scoped projector 强制输出 invalid generic，App 不再实现第二套 scope classifier。

UI 在卡片与 App 内全屏紧急态中必须显示 `task_name`。单行空间不足时允许视觉省略，但不得改写
Core typed projection 中的原值，也不得用 `summary`、conversation 标题或 Agent 名称猜测任务名。

## 3. 标准示例

普通任务结果：

```json
{
  "schema": "awiki.agent.message.v1",
  "event_id": "evt_awiki_release_20260811_01",
  "task_name": "AWiki Me 发布验证",
  "kind": "task_result",
  "level": "normal",
  "content": {
    "summary": "发布检查已完成",
    "detail": "12 项检查均通过"
  },
  "action": {
    "type": "open_conversation"
  }
}
```

紧急任务告警：

```json
{
  "schema": "awiki.agent.message.v1",
  "event_id": "evt_awiki_production_20260811_01",
  "task_name": "AWiki Me 生产发布",
  "kind": "alert",
  "level": "urgent",
  "content": {
    "summary": "生产服务连续 3 分钟不可用",
    "detail": "请尽快查看当前对话并处理"
  },
  "action": {
    "type": "open_conversation"
  }
}
```

## 4. 兼容、迁移与时间权威

V1 尚未发布，也没有已授权的 structured send。`task_name` 在首次发布前进入 required 集合；实现
阶段必须让缺失、`null`、空白边界或不安全的 `task_name` fail closed 为 visible invalid，不保留
optional/legacy alias，也不自动从 `summary` 补值。

未知和既有 `awiki.*` control 继续 hidden/read，不改变旧客户端行为。旧 broad classifier 已把
exact visible 行标记 read 的数据库，在 summary rebuild 后允许显示该行，但必须保留原
`is_read=true`；不得补 unread，也不得生成 retro-notification。

Rust→Dart projection 在 live `Message`、conversation snapshot message 和
`CommittedIncomingMessage` 上都显式提供 `authoritative_received_at`（Dart package 为
`authoritativeReceivedAt`），这是 App 做 urgent 年龄判断的唯一时间输入。Core 只从 Message
Service 的 `received_at` 取值；live hydration 缺失时先由 Core 用认证 `accepted_at` 补齐，绝不
回退到 sender-controlled `sent_at`。缺失、解析失败、未来时间或
`age < 0 / age > 15 分钟` 必须降为 normal；App 不得用 `sentAt ?? receivedAt` 自行猜测。

## 5. 发送、接收端权威、幂等与旧客户端 fallback

exact visible send 必须同时携带稳定 `client_message_id` 与 `idempotency_key`，同一 `event_id`
重试必须复用；结果不明确时不得生成新 ID 或盲目重试。接收能力、混合版本、紧急三态授权、
Receiving Home 入库裁决、稳定错误码和 CLI dry-run 语义已经冻结在
[`agent-message-v1-receiver-authority-contract.md`](agent-message-v1-receiver-authority-contract.md)。

User Service 私有聚合 active capable installation；至少一台支持即可 committed message，Push 只向
capable targets。urgent `enabled|disabled|unset` 只影响接收端展示，不影响结构化消息能否发送，也不
向发送者公开。当前只有 authenticated App foreground get/set 与 presentation 消费该 preference；
`unset` 或读取失败均降为 normal。Message Service 不读取 preference，background/killed urgent 延后到
server trust 与 conversation mute 权威同时具备。Receiving Home Message Service 只负责入库前 capability
裁决；Core/CLI 只保留 schema、Direct、security 与幂等本地 preflight，dry-run 不访问远端且不得声称
receiver 已验证。

在 User Service、Message Service、App 与测试尚未同时接入该契约前，当前实现继续在 target
resolution、网络 I/O、local echo 之前返回稳定 `receiver_capability_unverified`，避免无裁决窗口。
完成接入后，同一 blocker 从本地 validator 移除，真实拒绝由 Receiving Home 返回
`receiver_capability_unverified|receiver_capability_unsupported`；不得增加 caller-supplied
capability boolean/version 或第二发送栈。

获授权 workflow 可以在 structured send 未执行时只发送一条普通文本 fallback；不得同时发送
structured 与 text。

## 6. 当前证据边界

本地候选已由 Rust Core 唯一 decoder/classifier/sanitizer 解析必填 `task_name`，并通过
Rust→Dart/package typed DTO 投影到 AWiki Me；App mapper 不解析 raw JSON，卡片与 App 内全屏紧急态
显示 typed 任务名，System Test 保持 Message Service 透明传输、Core fail-closed 的 owner 边界。
deterministic Core、Dart、CLI、Flutter package、App unit/widget/视觉 fixture 与 codegen consistency
证据已恢复。

这些本地证据不授权也不证明 structured send 已开放，更不证明真实 App 卡片、振动、铃声、后台
通知、Push、设备、远程 E2E、CI、部署或发布。background/killed urgent 也未进入当前契约实现范围。
receiver authority 的实现与真实运行证据仍缺失，
因此在完整接入前发送 preflight 必须继续返回 `receiver_capability_unverified`。权威契约冻结本身
不等于功能已经开放。
