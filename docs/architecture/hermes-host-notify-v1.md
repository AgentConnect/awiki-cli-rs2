# Hermes Host Notify V1（架构与契约）

**文档作用**
- 这份文档是 Hermes 接入的“单一事实来源（SSOT）”。
- 面向架构评审、开发联调和后续维护，统一说明：为什么这样设计、协议是什么、OpenClaw 与 Hermes 如何兼容共存。
- 本文中的最终投递示例主要使用 Feishu；当前实现已经支持通过 `awiki-cli runtime host-notify hermes setup --deliver <platform>` 选择其它目标平台。
- 最新实现落地说明见 `docs/architecture/hermes-host-notify-implementation-notes.md`。

**状态**: Current v1
**适用范围**: `awiki-cli` websocket listener 的 host_notify 输出链路

---

## 1. 背景与目标

`awiki-cli` 已有标准化事件 `HostNotificationEvent`，并支持 `noop | log | file | openclaw` 等 sink。
当我们新增 Hermes 时，关键目标是：

1. 新增 Hermes 能力，不破坏 OpenClaw 现有行为。
2. 外部入站协议稳定，后续可演进到更多宿主。
3. 把宿主差异留在 adapter 层，不污染上游发布方。

---

## 2. 两层设计

### 2.1 Notification Surface（统一通知面）

规范入口是 `POST /notify`，请求体为统一事件壳。
为了兼容当前 awiki-cli listener 的 `HostNotificationEvent`，adapter 还提供 `POST /notify/host-event`，并在入站后先转换成统一事件壳，再走同一套校验与转发流程。

核心契约文件：
- `docs/architecture/contracts/notification-surface-v1.schema.json`
- `docs/architecture/contracts/notify-hermes-v1.openapi.yaml`

统一事件最小外壳：

```json
{
  "version": "1.0",
  "id": "ntf_01J...",
  "kind": "message",
  "topic": "im.message.received",
  "time": "2026-04-12T10:30:00Z",
  "binding_key": "awiki:direct:did_wba_bob:conv_alice_bob",
  "source": {
    "network": "awiki",
    "account_id": "did:wba:b.example:agents:bob:e1_bob",
    "conversation_id": "conv-alice-bob",
    "thread_id": "msg-direct-text-001"
  },
  "data": {}
}
```

### 2.2 Host Adapters（宿主适配层）

- OpenClaw adapter：转成 `/hooks/agent` 需要的 payload。
- Hermes adapter：转成 `/webhooks/notify` route 需要的 payload。

这层负责 6 件事：
1. 校验 Content-Type。
2. 校验统一事件结构。
3. 校验签名。
4. 按 `id` 做去重。
5. 做宿主字段映射。
6. 转发到宿主 webhook。

---

## 3. 从 awiki-cli 事件到统一事件的映射

源事件（awiki-cli）：
- `version`
- `id`
- `topic`
- `received_at`
- `data`

建议映射：
- `time <- received_at`
- `topic <- topic`
- `data <- data`
- `id <- ntf_ + event.id`
- `kind` 根据 topic 推导：
  - `im.message.received` / `im.group.message.received` -> `message`
  - `im.group.state.changed` -> `state`
- `binding_key` 使用稳定路由键（direct/group 分别构造）

---

## 4. OpenClaw v1（保持现有能力）

链路：

```text
publisher -> /notify -> openclaw-notify-adapter -> http://127.0.0.1:18789/hooks/agent
```

建议配置（OpenClaw hooks）：
- `allowRequestSessionKey: true`
- `allowedSessionKeyPrefixes: ["hook:notify:"]`
- `allowedAgentIds: ["notify"]`

关键映射：
- `message`: 完整统一事件的 minified JSON 字符串
- `sessionKey`: `hook:notify:` + `sha256(binding_key)[:16]`
- `agentId/name`: 固定 `notify`
- `wakeMode`: `now`

说明：
- 这是 hook-owned session continuity，不是“注入已有聊天会话”。
- OpenClaw 逻辑在 `openclaw` sink 路径保持不变。

---

## 5. Hermes v1（新增能力）

链路：

```text
publisher -> /notify (or /notify/host-event) -> hermes-notify-adapter -> http://127.0.0.1:8644/webhooks/notify
```

Hermes route 推荐：

```yaml
platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: "${HERMES_ROUTE_SECRET}"
          events: []
          prompt: "{notify_payload}"
          deliver: "feishu"
```

补充说明：

- 对 Feishu 目标，优先使用 `FEISHU_HOME_CHANNEL`
- 或在 Feishu 中给 Hermes 发送 `/sethome` / `/set-home`
- 不建议默认把 `deliver_extra.chat_id` 硬编码进 route

关键映射：
- payload:
  - `event_type = topic`
  - `notify_payload = <统一事件的 minified JSON 字符串>`
  - 邮件通知已开始复用消息主 topic；默认应优先根据 `data.source_kind=mail` 和邮件字段判断，而不是只依赖 `mail.message.received`
- headers:
  - `X-Webhook-Signature`
  - `X-Request-ID = id`

为什么 `notify_payload` 用字符串：
- 避免模板展开/raw dump 的截断风险。
- 保证统一事件完整性。

---

## 6. 兼容策略（重点）

为了“新增 Hermes，不影响现有功能”，v1 保持以下兼容：

1. `runtime.host_notify.sink=webhook` 仍可用，内部归一化到 `hermes`。
2. CLI 旧命令入口 `runtime host-notify webhook ...` 仍可用（作为 `hermes` alias）。
3. OpenClaw sink 与 route 逻辑保持独立，不被 Hermes 逻辑侵入。

---

## 7. 推荐联调顺序

1. 先验证 OpenClaw 现网链路不回归。
2. 本地启动 `scripts/hermes_notify_adapter.py`。
3. 先用 `POST /notify/host-event` 做 awiki-cli 兼容探活，再根据需要验证 `POST /notify` 的统一事件入口。
4. 再跑真实 listener 事件链路验证。
5. 在 `awiki-cli` 侧优先使用 `runtime host-notify hermes setup` 和 `runtime host-notify hermes status`，让 awiki-cli 代管本地 `~/.hermes/config.yaml` 的 notify route 与本地 adapter bridge。

---

## 8. 关联文档

- 运行与联调：`docs/architecture/hermes-host-notify-v1-runbook.md`
- Review 规范：`docs/harness/review-spec.md`
- 契约：
  - `docs/architecture/contracts/notification-surface-v1.schema.json`
  - `docs/architecture/contracts/notify-hermes-v1.openapi.yaml`
