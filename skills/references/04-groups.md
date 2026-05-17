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

## Core Concepts

- **group**: a first-class resource with its own DID and policy
- **membership**: who is in the group and what role they have
- **discoverability**: visibility and discovery policy
- **admission mode**: how members join the group
- **group messages**: the read path for group content; sending still uses `msg send --group`

## Resource Model

- `Identity -> Group -> Members`
- `Group -> Policy Fields`
- `Group -> Group Messages`

## Decision Rules

- Need to create a group -> `group create`
- Need to inspect metadata or policy -> `group get`
- Need to join an open group -> `group join`
- Need to add or remove one member -> `group add` / `group remove`
- Need an E2EE member to leave safely -> `group leave --e2ee` creates a hidden leave request, then the group owner processes it with `group e2ee process-leave-request`
- Need to change the name, description, or policy -> `group update`
- Need to send text to the group -> use `03-messaging.md`

## Canonical Commands

- `awiki-cli group create --name "Agent War Room" [...]`
- `awiki-cli group get --group <group_did>`
- `awiki-cli group join --group <group_did> [--reason "..."]`
- `awiki-cli group add --group <group_did> --member <did|handle> [--role ...]`
- `awiki-cli group remove --group <group_did> --member <did|handle> [--reason "..."]`
- `awiki-cli group leave --group <group_did> [--reason "..."] [--e2ee]`
- `awiki-cli group e2ee process-leave-request --group <group_did> --member <did|handle> [--leave-request-id <id>]`
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
- The currently public flags for `group add` are only `--group`, `--member`, and `--role`; `--reason` is not part of the current public flag surface

## Related References

- `03-messaging.md`
- `07-discovery.md`
- `08-debug.md`
