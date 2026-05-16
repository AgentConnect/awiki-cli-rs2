# Messaging Reference

## Purpose

Use this reference when you are handling direct-message and group-message tasks in `awiki-cli`, including inbox review, direct-message history lookup, attachment send/download, read-state updates, and sending plain-text messages.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves direct messages, group messages, inbox, history, unread state, or the current secure-message contract.

## Current Status

- Status: **partially implemented**
- Currently implemented:
  - `msg send`
  - `msg attachment download`
  - `msg inbox`
  - `msg history`
  - `msg mark-read`
- Reserved but not yet implemented:
  - `msg secure status`
  - `msg secure init`
  - `msg secure repair`
  - `msg secure failed`
  - `msg secure retry`
  - `msg secure drop`
- The contract includes `--secure on`, but the current server returns `unsupported` for secure direct messaging

## When to Use

- Send a direct message
- Send text to an existing group
- Send an attachment in a direct message or group message
- Download a single attachment from a direct message or group message
- View the inbox or direct-message history
- Mark messages as read
- Understand the current secure-message contract and its limitations

## Core Concepts

- **direct message**: one identity sends to one target, selected with `--to`
- **group message**: one identity sends to an existing group, selected with `--group`
- **inbox**: an aggregated read path across direct and group scopes
- **history**: the history of the direct-message thread with a single target
- **read state**: local unread-state tracking
- **secure messaging contract**: a reserved command family for future direct E2EE flows
- **bare handle completion**: when a direct/group handle input is a bare handle like `alice`, `awiki-cli` completes it to `alice.<did_domain>` before handle lookup; explicit full handles keep their explicit domain, and DIDs pass through unchanged

## Current Support Matrix

| Scope × Security | Current Status | Notes |
|---|---|---|
| direct + plain | Implemented | Use `msg send --to ...` |
| direct + secure | Planned | The contract includes `--secure on`, but the current server returns `unsupported` |
| group + plain | Implemented | Use `msg send --group ...` |
| group + secure | Unsupported | Not part of the current repository path |

## Resource Model

- `Identity -> Direct Thread -> Message`
- `Identity -> Group Membership -> Group Message`

## Decision Rules

- Need to send a message to a single target -> `awiki-cli msg send --to <handle|did> --text ...`
- Need to send a message to a group -> `awiki-cli msg send --group <group_did> --text ...`
- Need to send an attachment -> `awiki-cli msg send (--to <handle|did> | --group <group_did>) --file ./hello.txt [--text "..."] [--mime-type ...]`
- Need to save one file from a message -> `awiki-cli msg attachment download ...`
- Need to inspect recent state -> `awiki-cli msg inbox ...`
- Need to inspect one direct-message thread -> `awiki-cli msg history --with <handle|did>`
- Need to clear unread state -> `awiki-cli msg mark-read ...`
- Need to change group lifecycle state -> use `04-groups.md`
- Need to handle transport setup -> use `05-runtime.md`

## Canonical Commands

- `awiki-cli msg send --to <target> --text "Hello"`
- `awiki-cli msg send --group <group_did> --text "Hello group"`
- `awiki-cli msg send (--to <target> | --group <group_did>) --file ./hello.txt [--text "attachment caption"] [--mime-type text/plain]`
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

### Find the Message First, Then Download One Attachment

1. `awiki-cli msg history --with alice --limit 50`
2. `awiki-cli msg attachment download --with alice --message-id <message_id> --output ./downloads/file.bin --dry-run`
3. `awiki-cli msg attachment download --with alice --message-id <message_id> --output ./downloads/file.bin`

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
- secure is requested but currently unsupported -> explain that the secure path is still in the planning stage in the current repository

## Implementation Notes

- Runtime mode is determined by the runtime domain, not by messaging commands
- `msg send` covers both text sending and attachment sending; attachment sending uses `--file` and can optionally include `--text` as a caption
- The `msg secure` subcommand is reserved but not yet implemented
- Do not describe the current repository state as already supporting end-to-end secure direct messaging

## Related References

- `04-groups.md`
- `05-runtime.md`
- `01-onboarding.md`
