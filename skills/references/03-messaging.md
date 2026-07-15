# Messaging Reference

## Purpose

Use this reference when you are handling direct-message and group-message tasks in `awiki-cli`, including inbox review, direct-message history lookup, attachment send/download, read-state updates, sending plain-text messages, and sending end-to-end encrypted text or attachment messages.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves direct messages, group messages, inbox, history, unread state, or the current secure-message contract.

## Current Status

- Status: **implemented**
- Available commands:
  - `msg send`
  - `msg attachment download`
  - `msg inbox`
  - `msg history`
  - `msg mark-read`
- Supported secure surface:
  - `msg send --secure required` for direct/group text
  - `msg send --file ... --secure required` for direct/group attachments
  - `msg attachment download` for local decrypting download of E2EE attachments when the high-level SDK has the required secure selection
  - `msg secure status`
  - `msg secure repair`
- Use `msg send --secure required` for secure direct/group text and attachments.

## When to Use

- Send a direct message
- Send text to an existing group
- Send an attachment in a direct message or group message
- Send an E2EE attachment in a direct message or group message
- Download a single attachment from a direct message or group message
- View the inbox or direct-message history
- Mark messages as read
- Inspect or repair secure-message state

## Core Concepts

- **direct message**: one identity sends to one target, selected with `--to`
- **group message**: one identity sends to an existing group, selected with `--group`
- **inbox**: an aggregated read path across direct and group scopes
- **history**: the history of the direct-message thread with a single target
- **read state**: local unread-state tracking
- **secure messaging contract**: high-level direct/group E2EE send, secure attachment send/download, status, and repair flow backed by `im-core`
- **secure attachment**: an attachment sent through the existing `msg send --file ... --secure required` surface; `awiki-cli` delegates to high-level `im-core`, which encrypts the object locally and keeps object key/nonce out of public CLI output
- **bare handle completion**: when a direct/group handle input is a bare handle like `alice`, `awiki-cli` completes it to `alice.<did_domain>` before handle lookup; explicit full handles keep their explicit domain, and DIDs pass through unchanged

## Current Support Matrix

| Scope × Security | Current Status | Notes |
|---|---|---|
| direct text + plain | Implemented | Use `msg send --to ... --text ...` |
| direct text + secure | Implemented with capability gates | Use `msg send --to ... --text ... --secure required`, `msg secure status`, and `msg secure repair` |
| direct attachment + plain | Implemented | Use `msg send --to ... --file ...` and `msg attachment download --with ...` |
| direct attachment + secure | Implemented with capability gates | Use `msg send --to ... --file ... --secure required`; download through `msg attachment download --with ...` |
| group text + plain | Implemented | Use `msg send --group ... --text ...` |
| group text + secure | Implemented with capability gates | Use `msg send --group ... --text ... --secure required`; group lifecycle uses `04-groups.md` |
| group attachment + plain | Implemented | Use `msg send --group ... --file ...` and `msg attachment download --group ...` |
| group attachment + secure | Implemented with capability gates | Use `msg send --group ... --file ... --secure required`; download through `msg attachment download --group ...` |

## Resource Model

- `Identity -> Direct Thread -> Message`
- `Identity -> Group Membership -> Group Message`

## Decision Rules

- Need to send a message to a single target -> `awiki-cli msg send --to <handle|did> --text ...`
- Need to send a message to a group -> `awiki-cli msg send --group <group_did> --text ...`
- Need to send an attachment -> `awiki-cli msg send (--to <handle|did> | --group <group_did>) --file ./hello.txt [--text "..."] [--mime-type ...]`
- Need to send an E2EE attachment -> `awiki-cli msg send (--to <handle|did> | --group <group_did>) --file ./secret.bin --secure required [--text "..."] [--mime-type ...]`
- Need to save one file from a message -> `awiki-cli msg attachment download ...`
- Need to inspect recent state -> `awiki-cli msg inbox ...`
- Need to inspect one direct-message thread -> `awiki-cli msg history --with <handle|did>`
- Need to clear unread state -> `awiki-cli msg mark-read ...`
- Need to change group lifecycle state -> use `04-groups.md`
- Need to handle transport setup -> use `05-runtime.md`
- Need email-style recipients, subjects, mailbox folders, or mail attachments -> use `12-mail.md`

## Canonical Commands

- `awiki-cli msg send --to <target> --text "Hello"`
- `awiki-cli msg send --group <group_did> --text "Hello group"`
- `awiki-cli msg send --to <target> --text "Secret" --secure required`
- `awiki-cli msg send --group <group_did> --text "Secret group message" --secure required`
- `awiki-cli msg secure status --with <target>`
- `awiki-cli msg secure repair --with <target>`
- `awiki-cli msg send (--to <target> | --group <group_did>) --file ./hello.txt [--text "attachment caption"] [--mime-type text/plain]`
- `awiki-cli msg send --to <target> --file ./secret.bin --secure required [--text "attachment caption"] [--mime-type application/octet-stream]`
- `awiki-cli msg send --group <group_did> --file ./secret.bin --secure required [--text "attachment caption"] [--mime-type application/octet-stream]`
- `awiki-cli msg attachment download (--with <target> | --group <group_did>) --message-id <message_id> [--attachment-id <attachment_id>] --output ./downloads/file.bin`
- `awiki-cli msg inbox [--scope all|direct|group] [--with <target>] [--group <group_did>] [--unread] [--limit <n>] [--mark-read]`
- `awiki-cli msg history --with <target> [--limit <n>] [--cursor <cursor>]`
- `awiki-cli msg mark-read <MESSAGE_ID...>`

## Common Patterns

### Dry-Run Before Sending a Direct Message

1. `awiki-cli msg send --to alice --text "Hello" --dry-run`
2. `awiki-cli msg send --to alice --text "Hello"`

### Check Membership First, Then Send to a Group

1. `awiki-cli group get --group <group_did>`
2. `awiki-cli msg send --group <group_did> --text "Hello group" --dry-run`
3. `awiki-cli msg send --group <group_did> --text "Hello group"`

### Send an Attachment

1. `awiki-cli msg send --to alice --file ./hello.txt --text "hello attachment" --dry-run`
2. `awiki-cli msg send --to alice --file ./hello.txt --text "hello attachment"`

### Send an E2EE Attachment

1. Confirm the direct secure session or group secure readiness when needed: `awiki-cli msg secure status --with alice` for direct messages, or use the group secure readiness flow from `04-groups.md` for groups.
2. `awiki-cli msg send --to alice --file ./secret.txt --secure required --text "secure attachment" --dry-run`
3. `awiki-cli msg send --to alice --file ./secret.txt --secure required --text "secure attachment"`

For group E2EE attachments:

1. `awiki-cli group get --group <group_did>`
2. `awiki-cli msg send --group <group_did> --file ./secret.txt --secure required --dry-run`
3. `awiki-cli msg send --group <group_did> --file ./secret.txt --secure required`

### Find the Message First, Then Download One Attachment

1. `awiki-cli msg history --with alice --limit 50`
2. `awiki-cli msg attachment download --with alice --message-id <message_id> --output ./downloads/file.bin --dry-run`
3. `awiki-cli msg attachment download --with alice --message-id <message_id> --output ./downloads/file.bin`

For group attachment downloads, use `awiki-cli group messages --group <group_did>` or the relevant group history view to find the message id, then run `awiki-cli msg attachment download --group <group_did> --message-id <message_id> [--attachment-id <attachment_id>] --output ./downloads/file.bin`.

### Read Only Unread Direct-Message Items

`awiki-cli msg inbox --scope direct --unread --limit 20`

## Side Effects and Confirmation

- Require explicit confirmation:
  - `msg send`
  - `msg attachment download`
  - `msg mark-read`
  - `msg inbox --mark-read`
- Prefer `--dry-run` before sending a message or downloading an attachment

## Error Handling

- The target or body is unclear -> check `awiki-cli schema msg send`
- The attachment-download command shape is unclear -> check `awiki-cli schema msg attachment download`
- auth/setup error -> confirm that the active identity has completed registration
- transport unavailable -> use `05-runtime.md`
- secure is requested and fails -> report the error from `im-core` capability, identity, local-state, or transport checks, then use `msg secure status` or `msg secure repair` when appropriate
- secure attachment download fails -> report the stable `im-core` or service error; do not ask the user for object keys, nonces, ratchet keys, MLS secrets, download tickets, or raw manifests

## Implementation Notes

- Runtime mode is determined by the runtime domain, not by messaging commands
- `msg send` covers both text sending and attachment sending; attachment sending uses `--file` and can optionally include `--text` as a caption
- Secure attachments use the same command shape plus `--secure required`; there is no separate public secure-attachment command.
- The CLI must use high-level `im-core` attachment/message APIs. It must not build P7 object transfer, P5 direct E2EE, or P6 group E2EE wire payloads itself.
- Full E2EE attachment manifests, object keys, nonces, P5 ratchet keys, MLS secrets, and download tickets must not be printed in public CLI output. Dry-run output and send/download result envelopes are redacted.

## Related References

- `04-groups.md`
- `12-mail.md`
- `05-runtime.md`
- `01-onboarding.md`
