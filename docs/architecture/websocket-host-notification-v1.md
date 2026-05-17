# WebSocket Host Notification V1

**Status**: Draft v1.1  
**Scope**: `awiki-cli` websocket listener, normalized host notification event, and provider-neutral notify sink  
**Out of scope**: Hermes webhook routing, retry queues, receipt/proof forwarding, and host-specific action schemas

---

## 1. Goal

`awiki-cli` already receives websocket notifications from `message-service` and projects them into local SQLite state.

Phase 1 adds a second path:

1. receive raw websocket notification
2. normalize it into a compact host-facing event
3. deliver the event through a provider-neutral `notify` sink

The host must receive a stable and minimal event shape rather than the original JSON-RPC payload.

---

## 2. Event Envelope

V1 uses a small envelope:

```json
{
  "version": "1.0",
  "id": "evt_...",
  "topic": "im.message.received",
  "received_at": "2026-04-12T10:30:00Z",
  "data": {}
}
```

### 2.1 Envelope fields

- `version`: notification contract version
- `id`: stable event identifier
- `topic`: normalized event topic
- `received_at`: the timestamp when **awiki-cli** received the websocket notification
- `data`: compact business payload for the host

### 2.2 Explicit removals

V1 intentionally does **not** include:

- routing-only wrappers such as `binding`
- source mirrors such as `source_ref`
- raw websocket `meta`, `body`, `auth`, or `server`
- origin proofs, group receipts, payload digests, headers, or signatures
- local SQLite projection fields such as `thread_id` or `direction`

---

## 3. Supported Topics

Phase 1 only normalizes the websocket notifications already handled by the listener:

| Raw websocket method | Host topic |
| --- | --- |
| `direct.incoming` | `im.message.received` |
| `group.incoming` | `im.group.message.received` |
| `group.state_changed` | `im.group.state.changed` |

No other websocket notifications are normalized in this phase.

---

## 4. Data Mapping Rules

The host payload must contain only fields that are useful for host-side routing, reasoning, or display.

### 4.1 `direct.incoming` → `im.message.received`

Source references:

- `message-service/docs/api/ANP-client-server-api-direct.md`
- `message-service/docs/api/ANP-client-server-api-direct-schema-examples.md`

Normalized `data`:

```json
{
  "channel": "direct",
  "message_id": "msg-direct-text-001",
  "operation_id": "op-direct-text-001",
  "conversation_id": "conv-alice-bob",
  "sender_handle": "alice",
  "sender_did": "did:wba:a.example:agents:alice:e1_alice",
  "recipient_handle": "bob",
  "recipient_did": "did:wba:b.example:agents:bob:e1_bob",
  "profile": "anp.direct.base.v1",
  "security_profile": "transport-protected",
  "content_type": "text/plain",
  "text": "Hello, Bob.",
  "created_at": "2026-03-31T10:01:00Z"
}
```

Rules:

- `message_id` comes from `meta.message_id`
- if `meta.message_id` is missing, fallback order is:
  1. `meta.operation_id`
  2. deterministic `hostevt-<hash>` generated from the raw notification
- `recipient_did` comes from `meta.target.did`
- `sender_handle` is resolved from the local contact store first; if missing, `awiki-cli` may call `user-service` DID→Handle lookup and then cache the result locally
- `recipient_handle` comes from the active local identity when available
- `text` is included only when `body.text` exists
- `content_type` defaults to `text/plain` when the websocket payload omits it
- `received_at` is **not** copied from `meta.created_at`; it is generated locally by `awiki-cli`

Explicitly excluded:

- `auth.origin_proof`
- E2EE ciphertext fields
- raw `target.kind`
- raw `meta` / `body`
- any local display/system summary text

### 4.2 `group.incoming` → `im.group.message.received`

Source references:

- `message-service/docs/api/ANP-client-server-api-group.md`
- `message-service/docs/api/ANP-client-server-api-group-schema-examples.md`

Normalized `data`:

```json
{
  "channel": "group",
  "message_id": "msg-group-001",
  "operation_id": "op-group-send-001",
  "group_did": "did:wba:groups.example:groups:...:e1_group",
  "sender_handle": "alice",
  "sender_did": "did:wba:a.example:agents:alice:e1_alice",
  "recipient_handle": "bob",
  "recipient_did": "did:wba:b.example:agents:bob:e1_bob",
  "profile": "anp.group.base.v1",
  "security_profile": "transport-protected",
  "content_type": "text/plain",
  "text": "hello group",
  "group_state_version": "4",
  "group_event_seq": "5",
  "accepted_at": "2026-04-07T09:11:01Z"
}
```

Rules:

- `message_id` comes from `meta.message_id`
- if `meta.message_id` is missing, fallback order is:
  1. `<group_did>:<group_event_seq>`
  2. `meta.operation_id`
  3. deterministic `hostevt-<hash>` generated from the raw notification
- `recipient_did` comes from `meta.target.did`
- `sender_handle` is resolved with the same local-first / remote-fallback rule as direct messages
- `recipient_handle` comes from the active local identity when available
- `text` is included only when `body.text` exists
- non-text payloads are represented by `content_type` only; V1 does not forward `body.payload`
- `group_state_version` and `group_event_seq` are forwarded as strings to preserve wire compatibility

Explicitly excluded:

- `group_receipt`
- `auth.origin_proof`
- `server`
- `payload_digest`
- `proof`
- `body.payload`

### 4.3 `group.state_changed` → `im.group.state.changed`

Source references:

- `message-service/docs/api/ANP-client-server-api-group.md`
- `message-service/docs/api/ANP-client-server-api-group-schema-examples.md`

Normalized `data`:

```json
{
  "channel": "group",
  "event_id": "evt-3",
  "event_type": "member-activated",
  "group_did": "did:wba:groups.example:groups:...:e1_group",
  "recipient_did": "did:wba:a.example:agents:alice:e1_alice",
  "actor_did": "did:wba:a.example:agents:alice:e1_alice",
  "subject_did": "did:wba:c.example:agents:carol:e1_carol",
  "subject_method": "group.add",
  "membership_status": "active",
  "group_state_version": "3",
  "group_event_seq": "3",
  "changed_at": "2026-04-07T09:06:01Z"
}
```

Rules:

- `event_id` comes from `body.event_id`
- if `body.event_id` is missing, fallback order is:
  1. `<group_did>:<group_event_seq>`
  2. `meta.operation_id`
  3. deterministic `hostevt-<hash>` generated from the raw notification
- `recipient_did` comes from `meta.target.did`
- `event_type` prefers `body.event_type`
- if `body.event_type` is missing, `awiki-cli` infers it from `membership_status` or `subject_method` using:
  - `active` / `activated` or `group.add` → `member-activated`
  - `removed` or `group.remove` → `member-removed`
  - `left` or `group.leave` → `member-left`
  - `group.update_profile` → `group-profile-updated`
  - `group.update_policy` → `group-policy-updated`

Explicitly excluded:

- `group_receipt`
- `payload_digest`
- `proof`
- `server`
- awiki-cli local system message text

---

## 5. Notify Sink Contract

The listener emits host events through a provider-neutral sink:

```go
Notify(context.Context, HostNotificationEvent) error
```

Phase 1 ships provider-neutral sinks:

- `noop`: accept the event and drop it
- `log`: write the normalized event into the listener log stream
- `file`: append newline-delimited JSON events to a local file
- `hermes`: post normalized events to a local/remote notify adapter endpoint

This is intentionally host-agnostic. OpenClaw and Hermes adapters will be built later on top of the same event contract.

OpenClaw V1 is now specified separately in:

- `docs/architecture/openclaw-host-adapter-v1.md`
- `docs/architecture/hermes-host-notify-v1.md`
- `docs/architecture/hermes-host-notify-v1-runbook.md`
- `docs/architecture/contracts/notification-surface-v1.schema.json`
- `docs/architecture/contracts/notify-hermes-v1.openapi.yaml`

---

## 6. Config Surface

`config.yaml` adds a small runtime block:

```yaml
runtime:
  mode: websocket
  socket_path: ""
  host_notify:
    enabled: true
    sink: log
    file_path: ""
```

Rules:

- `runtime.host_notify.enabled` defaults to `true`
- `runtime.host_notify.sink` supports `noop | log | file | hermes | openclaw` (legacy alias: `webhook`)
- when `sink = file` and `file_path` is omitted, awiki-cli derives:
  - `<workspace>/runtime/host-notify.events.jsonl`
- when `sink = hermes`, `runtime.host_notify.hermes.notify_url` configures the adapter ingress endpoint

---

## 7. Failure Policy

Host notification delivery must not break the listener's primary job.

Phase 1 failure policy:

- SQLite projection remains the primary side effect
- sink delivery happens after notification normalization
- sink errors do **not** block SQLite writes
- sink errors are exposed through listener status as the latest host notify error
- no retry queue, dead-letter queue, or delivery acknowledgment is added in this phase

---

## 8. Test Requirements

Phase 1 must cover:

- direct message normalization
- group message normalization
- group state change normalization
- `received_at` being generated by `awiki-cli`
- omission of `auth`, `server`, and non-text `payload`
- sink invocation from listener notification handling
- sink failure not blocking SQLite persistence

---

## 9. Future Work

Not part of Phase 1:

- OpenClaw mapped hook or plugin-native delivery design
- Hermes webhook route design
- host-specific routing keys or action schemas
- receipt/proof forwarding
- persistent host delivery outbox
- delivery retries or acknowledgments
