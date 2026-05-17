# Discovery Reference

## Purpose

Use this reference when you are handling "review first, draft later" workflows in `awiki-cli`. It is especially suitable when the user wants to inspect a group, understand possibly relevant people, review existing relationship context, and draft an introduction or follow-up message.

This file is a **workflow reference**, not an entry skill. Load it only when the task clearly involves discovery, group review, candidate selection, or manually drafting introductions.

## Current Status

- Status: **partially implemented workflow**
- Currently available:
  - group inspection
  - member review
  - direct-message history review
  - profile lookup
- Planned next:
  - `people` search, follow, contacts, and relationship management

## When to Use

- Review a group before contacting its members
- Gather context for manually drafting an introduction or follow-up message
- Understand current group activity and potentially relevant people

## Prerequisites

- The user provides the target group DID, or provides a small set of candidate people
- The current active identity can read the relevant group or direct-message history
- The current workflow is for review and drafting, not for automatic outreach

## Workflow Steps

### 1. Inspect the Group Itself

- `awiki-cli group get --group <group_did>`

### 2. Review Current Members

- `awiki-cli group members --group <group_did> --limit 100`

### 3. Review Recent Group Activity

- `awiki-cli group messages --group <group_did> --limit 50`

### 4. Inspect a Candidate's Profile

- `awiki-cli id profile get --did <member_did>`
- or `awiki-cli id profile get --handle <handle>`

### 5. If a Relationship Already Exists, Inspect Existing Direct-Message History

- `awiki-cli msg history --with <handle|did> --limit 50`

### 6. Manually Draft Outreach Content

After collecting the structured output, draft the introduction or direct message in the assistant response.

Do not send messages automatically. If the user requests sending, switch to `03-messaging.md` and prefer a dry-run first.

## Planned Follow-Up Capabilities

The following command families are reserved but not yet implemented:

- `awiki-cli people search <QUERY>`
- `awiki-cli people contacts save --did <did> [...]`

When the user requests these operations, explain that the contract exists, but the current repository does not implement the corresponding handlers yet.

## Safety Notes

- Review first, send later
- Do not auto-follow, auto-save contacts, or auto-message anyone
- Do not infer sensitive personal traits from group activity

## Related References

- `02-identity.md`
- `03-messaging.md`
- `04-groups.md`
- `09-people-planned.md`
