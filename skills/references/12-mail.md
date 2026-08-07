# Mail Reference

## Purpose

Use this reference for the top-level `awiki-cli mail ...` mailbox workflow. Mail is separate from AWiki direct/group messaging: use `mail` for email-style recipients, subjects, folders, and mailbox attachments; use `msg` for DID/handle and group conversations.

## Current Status

- Status: **implemented**
- Read operations: inbox, notifications, one message, and account details
- Write operations: send, mark read, and attachment download to a local file

## Canonical Commands

- `awiki-cli mail inbox [--folder <folder>] [--unread] [--limit <n>] [--offset <n>]`
- `awiki-cli mail notify [--limit <n>]`
- `awiki-cli mail read --id <message_id>`
- `awiki-cli mail mark-read <message_id> [message_id...]`
- `awiki-cli mail account`
- `awiki-cli mail send --to <address[,address...]> --subject <subject> --body <text> [--cc <address[,address...]>] [--html <html>]`
- `awiki-cli mail attachment download --message-id <message_id> [--attachment-index <zero_based_index>] [--output <path>]`

## Decision Rules

- Need mailbox rows -> `mail inbox`
- Need recent notification rows -> `mail notify`
- Need the full content of one message -> `mail read`
- Need mailbox identity/configuration -> `mail account`
- Need to send email-style content -> `mail send`
- Need to save an attachment -> inspect the message first, confirm the output path, then use `mail attachment download`
- Need an AWiki handle, DID, or group conversation -> use `references/03-messaging.md`, not `mail`

## Side Effects and Confirmation

Require explicit confirmation before:

- `mail send`
- `mail mark-read`
- `mail attachment download`

Attachment download writes a local file. Confirm the message, zero-based attachment index, and destination path before execution.

## Error Handling

- Command shape is unclear -> `awiki-cli schema mail <subcommand>`
- Mail service endpoint or active identity is unclear -> `awiki-cli config show` and `awiki-cli id status`
- A message or attachment identifier is unclear -> inspect it with `mail inbox` and `mail read` before writing

## Related References

- `03-messaging.md`
- `02-identity.md`
- `08-debug.md`
