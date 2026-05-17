# Debug Reference

## Purpose

Use this reference when you are doing local debugging and final fallback inspection in `awiki-cli`, especially for SQLite state inspection, migration-import verification, schema confusion, and situations where normal domain paths are no longer enough to explain the issue.

This file is a **reference**, not an entry skill. Load it only after all safe inspection paths have been exhausted.

## Current Status

- Status: **partially implemented**
- Currently implemented:
  - `debug db query`
  - `debug db import-v1`
- Planned but not yet implemented:
  - `debug raw rpc`
  - `debug schema-cache`
  - `debug logs`

## When to Use

- Local SQLite inspection
- Verify migration-import results
- Understand what the current debug surface actually provides
- Map low-level findings back to domain behavior

## Safety-First Decision Tree

Use debug only when the following paths are still not enough:

1. `awiki-cli status`
2. `awiki-cli docs [topic]`
3. `awiki-cli schema [command]`
4. `awiki-cli doctor`
5. `awiki-cli config show`
6. One matching domain or workflow reference

## Currently Available Commands

- `awiki-cli debug db query "<SQL>"`
- `awiki-cli debug db import-v1 [--path <legacy_dir>]`

## Planned but Not Yet Implemented

- `awiki-cli debug raw rpc`
- `awiki-cli debug schema-cache`
- `awiki-cli debug logs [--follow]`

## Limitations

- Do not execute destructive SQL
- Do not assume raw RPC is already available before the command is implemented
- Do not expose JWTs, private keys, secure session material, or unrelated local files
- Do not use debug to bypass domain-level confirmation rules

## Side Effects and Confirmation

- Safe for narrow, non-destructive inspection:
  - `debug db query`
- Require explicit confirmation and should use dry-run first:
  - `debug db import-v1`

## Recovery Pattern

1. Inspect with `debug db query`
2. Translate the findings back into canonical runtime, identity, or messaging behavior
3. Return to the matching domain reference instead of staying on the debug path for long

## Related References

- `02-identity.md`
- `03-messaging.md`
- `04-groups.md`
- `05-runtime.md`
- `01-onboarding.md`
