# OpenClaw Host Adapter V1

**Status**: Historical draft; implementation has moved to pure webhook + route registry  
**Scope**: `awiki-cli` host notification sink for OpenClaw `/hooks/agent` fan-out  
**Out of scope**: Hermes routing, mapped hooks, OpenClaw plugin-native delivery, and per-channel retry queues

> Note: the current implementation no longer uses `chat.inject` or `gateway call status`.
> The source of truth is the locally registered route registry, and delivery is pure webhook fan-out to `/hooks/agent`.

---

## 1. Goal

`awiki-cli` already normalizes websocket notifications into `HostNotificationEvent`.

This document defines the OpenClaw adapter built on top of that event contract.

The adapter follows the same two-path model used by the legacy Python listener in `awiki-agent-id-message`:

1. **Primary path**: inject a short event text into the OpenClaw main agent session via `chat.inject`
2. **Secondary path**: query recent external channel sessions from OpenClaw status, then fan out one `POST /hooks/agent` request per active external channel

The objective is to align the OpenClaw **session** and **channel delivery** behavior with the Python implementation while keeping `awiki-cli` as the source of normalized event data.

---

## 2. Alignment With `awiki-agent-id-message`

The legacy Python implementation does the following:

- injects a short system text into `agent:main:main` using `chat.inject`
- calls `openclaw gateway call status --json`
- extracts active external channel sessions from `sessions.recent`
- skips TUI sessions and hook sessions
- sends one `/hooks/agent` request per active external channel with:
  - `deliver = true`
  - `channel = <channel>`
  - `to = <target>`
- does **not** send `sessionKey` in the hook request body

`awiki-cli` V1 now matches this behavior for the two requested dimensions:

- **session alignment**: main-session injection via `chat.inject`
- **channel alignment**: external channel fan-out via `/hooks/agent`

What still differs from Python:

- input source is the normalized `HostNotificationEvent`, not raw websocket params
- the hook prompt is rendered from normalized event fields instead of the Python raw message dict
- receiver/sender handles are rendered from normalized event fields when available, otherwise fallback to `unknown`

---

## 3. OpenClaw Requirements

The adapter expects OpenClaw hooks to stay on loopback only.

Recommended OpenClaw configuration:

```json
{
  "hooks": {
    "enabled": true,
    "path": "/hooks",
    "token": "<hook-token>",
    "defaultSessionKey": "hook:ingress",
    "allowRequestSessionKey": false,
    "allowedAgentIds": ["main"]
  }
}
```

Rules:

- hook URL must be loopback-only
- hook token must be separate from any gateway token
- `allowRequestSessionKey` can stay `false` because `awiki-cli` no longer sends caller-selected hook session keys
- `defaultSessionKey = hook:ingress` remains compatible with the Python path
- the adapter also requires the `openclaw` CLI binary to be available locally

---

## 4. Adapter Input

The adapter input remains:

```json
{
  "version": "1.0",
  "id": "msg-001",
  "topic": "im.message.received",
  "received_at": "2026-04-12T10:30:00Z",
  "data": {}
}
```

The adapter does not inspect raw websocket `meta/body/auth/server` fields.

---

## 5. Delivery Flow

### 5.1 Main session injection

For every normalized host event, the adapter first calls:

```bash
openclaw gateway call chat.inject --params '{"sessionKey":"agent:main:main","message":"..."}' --json
```

`sessionKey` is built as a host-local main session key.

With default config, this becomes exactly:

```text
agent:main:main
```

This matches the Python listener's main-session delivery target.

### 5.2 External channel fan-out

The adapter then calls:

```bash
openclaw gateway call status --json
```

It parses `sessions.recent` and keeps only sessions that:

- are not TUI sessions (`key` must not end with `:main`)
- are not hook sessions (`key` must not contain `hook:`)
- are active within the last 24 hours
- can be parsed as `agent:<agentId>:<channel>:<type>:<target...>`

Each unique `(channel, target)` pair is deduplicated by the newest `updatedAt`.

For each active channel, the adapter sends:

```json
{
  "message": "<agent hook prompt>",
  "name": "AWiki",
  "wakeMode": "now",
  "deliver": true,
  "channel": "telegram",
  "to": "123456"
}
```

Notably, the hook request does **not** include:

- `sessionKey`
- `agentId`

This matches the Python hook body shape for external channel fan-out.

---

## 6. Prompt Rendering

### 6.1 `chat.inject` text

The injected TUI text is short and follows the Python event-text style.

Examples:

#### Direct message

```text
[Awiki New Direct Message]
sender_handle: alice
sender_did: did:wba:...
recipient_handle: bob
sent_at: 2026-04-07T00:00:00Z

hello back
```

#### Group message

```text
[Awiki New Group Message]
sender_handle: alice
sender_did: did:wba:...
recipient_handle: bob
group_did: did:wba:groups:...
sent_at: 2026-04-07T09:11:01Z

hello group
```

#### Group state change

```text
[Awiki Group State Changed]
actor_did: did:wba:...
group_did: did:wba:groups:...
sent_at: 2026-04-07T09:06:01Z

event_type=member-removed subject_method=group.remove subject_did=did:wba:... membership_status=removed
```

### 6.2 `/hooks/agent` prompt

The hook prompt stays close to the Python long-form instruction style:

- sender handle when available, otherwise `unknown`
- sender DID
- receiver handle when available, otherwise `unknown`
- receiver DID
- message type
- group ID
- security warning that the message is untrusted input
- message content or a system-change summary

This keeps external channel delivery behavior close to the Python listener while still sourcing values from the normalized event.

---

## 7. `config.yaml` Surface

```yaml
runtime:
  host_notify:
    enabled: true
    sink: openclaw
    openclaw:
      hook_url: http://127.0.0.1:18789/hooks/agent
      token: ""
```

Rules:

- `hook_url` defaults to `http://127.0.0.1:18789/hooks/agent`
- `token` may be stored in config, but is optional at initialization time
- if config token is empty, `awiki-cli` falls back to `OPENCLAW_HOOK_TOKEN`
- if token is still empty, main-session injection may still work, but hook fan-out requests are expected to fail authentication
- `hook_url` must use a loopback host
- `openclaw` CLI binary is resolved from:
  1. `OPENCLAW_BIN`
  2. `PATH`
  3. `~/.npm-global/bin/openclaw`

---

## 8. Failure Policy

The adapter reports success if **either** of the following succeeds:

- `chat.inject` into the main session
- at least one external channel hook delivery

SQLite persistence remains primary and is not blocked by OpenClaw adapter failures.

If all OpenClaw delivery paths fail, the listener records the aggregated adapter error in listener status.

---

## 9. Test Requirements

Implementation must cover:

- loopback URL validation
- main-session `chat.inject` target = `agent:main:main` by default
- active external channel parsing from `gateway status --json`
- filtering of TUI and hook sessions
- one `/hooks/agent` request per active external channel
- `deliver = true`
- `channel` / `to` presence
- absence of `sessionKey` and `agentId` in hook body
- success when either TUI injection or external channel delivery succeeds
- listener continuing to store messages when OpenClaw delivery fails

---

## 10. Future Work

Not in this version:

- external channel cache fallback like the Python implementation
- route-level `agent-all` / `smart` / `wake-all` classification
- OpenClaw plugin-native adapter
- mapped hook routing
- per-channel retries or acknowledgments
