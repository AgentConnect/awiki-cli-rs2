# Hermes Host Notify V1（部署联调手册）

**文档作用**
- 这份文档面向实施与运维，提供一套可执行的 Hermes 接入步骤。
- 同时包含“人工操作步骤”和“交给 Codex 代执行的提示词模板”。
- 本文示例主要以 Feishu 为目标平台；当前实现已经支持通过 `awiki-cli runtime host-notify hermes setup --deliver <platform>` 选择其它最终投递平台。
- 最新实现落地说明见 `docs/architecture/hermes-host-notify-implementation-notes.md`。

**适用场景**
- `awiki-cli` 和 Hermes 在同机或跨机部署。
- 需要验证链路：`awiki-cli listener -> hermes sink -> hermes_notify_adapter -> Hermes /webhooks/notify`。

---

## 1. 前置准备

变量约定：
- `A_HOST`: Hermes 机器地址
- `NOTIFY_SECRET`: awiki-cli -> adapter 签名密钥
- `HERMES_ROUTE_SECRET`: adapter -> Hermes route 签名密钥
- `AWIKI_CLI_REPO`: awiki-cli 仓库路径

端口默认：
- adapter: `8765`
- Hermes webhook: `8644`

---

## 2. Hermes 机器（A）配置

### 2.1 Hermes route

确保存在 `notify` route。

如果你只是做最小链路探活，`deliver: "log"` 就足够。
如果你要验证 `awiki-cli -> Hermes -> Feishu` 的真实落地，推荐直接使用 `deliver: "feishu"`，并让 Hermes 自己决定默认投递会话，不要在这里硬编码 `deliver_extra.chat_id`：

```yaml
platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      secret: "${HERMES_WEBHOOK_SECRET}"
      routes:
        notify:
          secret: "${HERMES_ROUTE_SECRET}"
          events: []
          prompt: "{notify_payload}"
          deliver: "feishu"
```

Feishu 目标建议：

- 优先设置 `FEISHU_HOME_CHANNEL`
- 或在 Feishu 中给 Hermes 发送 `/sethome` 或 `/set-home`
- 只有当你明确要把所有通知固定投递到某个会话时，才使用 `deliver_extra.chat_id`

补充说明：

- 邮件通知当前会沿用统一消息主 topic 进入 Hermes
- route prompt 应优先根据 `data.source_kind=mail` 以及 `mailbox_address`、`from_addr`、`subject`、`preview` 等字段判断邮件通知

### 2.2 启动 adapter

```bash
python3 scripts/hermes_notify_adapter.py \
  --host 0.0.0.0 \
  --port 8765 \
  --notify-secret "<NOTIFY_SECRET>" \
  --hermes-webhook-url "http://127.0.0.1:8644/webhooks/notify" \
  --hermes-route-secret "<HERMES_ROUTE_SECRET>" \
  --log-level INFO
```

健康检查：

```bash
curl -sS http://127.0.0.1:8765/healthz
```

---

## 3. awiki-cli 机器（B）配置

### 3.1 推荐新命令（hermes）

```bash
./awiki-cli runtime host-notify hermes guide
./awiki-cli runtime host-notify hermes setup
./awiki-cli runtime host-notify hermes status
```

如果你更偏好拆开执行，下面这些命令仍然可用：

```bash
./awiki-cli runtime host-notify config set --sink hermes
./awiki-cli runtime host-notify hermes set --notify-url http://<A_HOST>:8765/notify/host-event
./awiki-cli runtime host-notify hermes set-secret --value <NOTIFY_SECRET>
./awiki-cli runtime host-notify enable
```

### 3.2 旧命令兼容（webhook）

以下旧命令仍可用（alias）：

```bash
./awiki-cli runtime host-notify webhook set --notify-url http://<A_HOST>:8765/notify/host-event
./awiki-cli runtime host-notify webhook set-secret --value <NOTIFY_SECRET>
```

### 3.3 配置确认

```bash
./awiki-cli runtime host-notify config show
```

期望：
- `sink = hermes`
- `hermes.notify_url = http://<A_HOST>:8765/notify/host-event`
- `hermes.secret_configured = true`

补充说明：

- `awiki-cli` 不负责管理 Hermes 最终投递到哪个 IM 会话
- 因此这里不需要像 OpenClaw 一样再执行额外的 `route add --channel ... --to ...` 命令
- `runtime host-notify hermes guide` 会把 Hermes route、adapter 命令和 Feishu home channel 的推荐做法一起展示出来
- `runtime host-notify hermes setup` 会把 awiki-cli 自己的 host-notify 配置、本地 `~/.hermes/config.yaml` 中的 notify route，以及本地 bridge 一起收敛好
- 用户还需要在 Feishu 中执行一次 `/sethome` 或 `/set-home`

---

## 4. 快速探活（不依赖真实消息）

```bash
TS=$(date +%s)
BODY='{"version":"1.0","id":"msg-probe-001","topic":"im.message.received","received_at":"2026-04-12T10:30:00Z","data":{"channel":"direct","message_id":"msg-probe-001","conversation_id":"conv-probe-001","sender_did":"did:wba:a.example:agents:alice:e1_alice","recipient_did":"did:wba:b.example:agents:bob:e1_bob","content_type":"text/plain","text":"hello from probe"}}'
SIG=$(printf '%s.%s' "$TS" "$BODY" | openssl dgst -sha256 -hmac '<NOTIFY_SECRET>' -hex | awk '{print $2}')

curl -sS -X POST "http://<A_HOST>:8765/notify/host-event" \
  -H 'Content-Type: application/json' \
  -H "X-Notify-Timestamp: $TS" \
  -H "X-Notify-Signature: sha256=$SIG" \
  -d "$BODY"
```

期望返回：

```json
{"accepted":true,"id":"ntf_msg-probe-001","host":"hermes","ref":"notify"}
```

---

## 5. 真链路验证

1. 给机器 B 身份发送一条消息。
2. 机器 A 观察 adapter 日志：`forwarded id=... status=200`。
3. 机器 A 观察 Hermes 日志：命中 `/webhooks/notify` route。
4. 如果 route 使用的是 `deliver: "feishu"`，确认消息最终出现在 Hermes 当前的 Feishu home channel 中。

---

## 6. 常见问题

1. `401 unauthorized`
- 原因：签名或时间戳超窗。
- 检查：`NOTIFY_SECRET` 一致、两机时间同步。

2. `502 upstream_failed`
- 原因：adapter 到 Hermes 失败。
- 检查：`--hermes-webhook-url`、route secret、Hermes 端口监听。

3. `secret_configured=false`
- 修复：
  - `runtime host-notify hermes set-secret --value ...`
  - 或环境变量 `AWIKI_HOST_NOTIFY_HERMES_SECRET`（兼容旧值 `AWIKI_HOST_NOTIFY_WEBHOOK_SECRET`）

4. 跨机不通
- 检查：adapter 是否 `--host 0.0.0.0`，防火墙是否放通 `8765`。

5. Hermes 命中了 route，但 Feishu 没有收到
- 检查：Hermes route 是否使用 `deliver: "feishu"`。
- 检查：`FEISHU_HOME_CHANNEL` 是否已设置，或是否已经在 Feishu 中执行过 `/sethome` / `/set-home`。
- 检查：是否误把目标会话写死在旧的 `deliver_extra.chat_id` 上，导致消息发往了别的会话。

6. 在沙箱、受限容器或 CI 里调试时出现 `exit status 1` / `service status unavailable` / `bridge could not be started`
- 先怀疑执行环境限制，而不是立即判定为 awiki-cli 代码缺陷。
- `runtime host-notify hermes setup` 与 `runtime host-notify hermes status` 在 Linux 上会依赖 `systemctl --user`、user dbus、`XDG_RUNTIME_DIR`、`DBUS_SESSION_BUS_ADDRESS` 等本机用户态服务能力。
- 如果这些能力在沙箱里被拦截，CLI 可能报“bridge 启动失败”或“状态不可用”，但用户真实机器上的 Hermes bridge 实际是正常的。
- 排查时优先以目标机器上直接执行的结果为准，再对照：
  - `./awiki-cli-dev runtime host-notify hermes setup --deliver <platform>`
  - `./awiki-cli-dev runtime host-notify hermes status`
  - `systemctl --user status <bridge-service>`
- 只有当问题能在真实环境里稳定复现时，才进入代码修复流程；如果只是沙箱里失败、但真实机器执行成功，应记录为环境差异。

---

## 7. 可直接发给 Codex 的提示词模板

### 7.1 发给 Hermes 机器 Codex

```text
你在一台已安装 Hermes 的机器上操作。请按“可回滚、可验证”的方式完成 awiki Hermes webhook 接入，并直接执行。
固定参数：A_HOST={{A_HOST}}，NOTIFY_SECRET={{NOTIFY_SECRET}}，HERMES_ROUTE_SECRET={{HERMES_ROUTE_SECRET}}，AWIKI_CLI_REPO={{AWIKI_CLI_REPO}}。
目标：确保 notify route 可用；启动 hermes_notify_adapter.py 监听 8765 并转发到 /webhooks/notify；完成健康检查和签名联调；输出可给 awiki-cli 机器执行的命令清单。
```

### 7.2 发给 awiki-cli 机器 Codex

```text
你在 awiki-cli 机器上操作。请直接执行并验证 host_notify 的 Hermes 接入。
固定参数：A_HOST={{A_HOST}}，NOTIFY_SECRET={{NOTIFY_SECRET}}，AWIKI_CLI_REPO={{AWIKI_CLI_REPO}}。
步骤：1) sink 设为 hermes；2) hermes.notify_url 指向 http://{{A_HOST}}:8765/notify/host-event；3) 写入 hermes secret；4) 执行 config show 并给出结论。
```
