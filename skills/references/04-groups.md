# Groups Reference

## Purpose

Use this reference when you are handling group lifecycle tasks in `awiki-cli`, including group creation, membership changes, policy updates, and group-state inspection.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves groups, members, admission, policies, or group-level history.

## Current Status

- Status: **implemented**
- `group` is a first-class domain
- Group messages can be viewed here, but sending still uses `msg send --group`

## When to Use

- Create a group
- Join or leave a group
- Add or remove members
- Update group profile or policy fields
- View members or group messages
- Check or repair group secure state

## Core Concepts

- **group**: a first-class resource with its own DID and policy
- **membership**: who is in the group and what role they have
- **discoverability**: visibility and discovery policy
- **admission mode**: how members join the group
- **group messages**: the read path for group content; sending still uses `msg send --group`
- **group secure lifecycle**: create/add/remove/leave operations with `--secure required`, implemented through high-level `im-core` group APIs

## Resource Model

- `Identity -> Group -> Members`
- `Group -> Policy Fields`
- `Group -> Group Messages`

## Decision Rules

- Need to create a group -> `group create`
- Need to inspect metadata or policy -> `group get`
- Need to join an open group -> `group join`
- Need to add or remove one member -> `group add` / `group remove`
- Need secure group lifecycle -> use `--secure required` on `group create`, `group add`, `group remove`, or `group leave`
- Need secure state health or recovery -> `group secure status` / `group secure repair`
- Need to change the name, description, or policy -> `group update`
- Need to send text to the group -> use `03-messaging.md`

## Canonical Commands

- `awiki-cli group create --name "Agent War Room" [...] [--secure off|required]`
- `awiki-cli group get --group <group_did>`
- `awiki-cli group join --group <group_did> [--reason "..."]`
- `awiki-cli group add --group <group_did> --member <did|handle> [--role ...] [--secure off|required]`
- `awiki-cli group remove --group <group_did> --member <did|handle> [--reason "..."] [--secure off|required]`
- `awiki-cli group leave --group <group_did> [--reason "..."] [--secure off|required]`
- `awiki-cli group secure status --group <group_did>`
- `awiki-cli group secure repair --group <group_did>`
- `awiki-cli group update --group <group_did> [--name ...] [--description ...] [...]`
- `awiki-cli group members --group <group_did> [--limit <n>]`
- `awiki-cli group messages --group <group_did> [--limit <n>] [--cursor <cursor>]`

## Common Patterns

### Dry-Run Before Creating a Group

1. `awiki-cli group create --name "Agent War Room" --dry-run`
2. `awiki-cli group create --name "Agent War Room"`

### Review Before Changing Members

1. `awiki-cli group get --group <group_did>`
2. `awiki-cli group members --group <group_did>`
3. `awiki-cli group add --group <group_did> --member <did> --dry-run`
4. `awiki-cli group add --group <group_did> --member <did>`

## Side Effects and Confirmation

- Require explicit confirmation:
  - `group create`
  - `group join`
  - `group add`
  - `group remove`
  - `group leave`
  - `group update`
- Prefer reviewing before changing membership

## Error Handling

- The group identifier is missing or malformed -> check `awiki-cli schema group <subcommand>`
- Access or role problems -> check `group get` and `group members` first
- Transport or auth problems -> route to the runtime or identity reference as appropriate

## Implementation Notes

- In the current repository, `group` is an independent domain
- `group messages` is a read-only inspection path; sending still happens through `msg send --group`
- `--e2ee` and `--message-security-profile group-e2ee` are deprecated aliases for `--secure required`; prefer the canonical flag.
- Low-level `group e2ee publish-key-package/pending/process-leave-request/recover-member/update-key/rejoin` commands are hidden/internal or unsupported and should not be recommended as product workflows.

## Related References

- `03-messaging.md`
- `07-discovery.md`
- `08-debug.md`
