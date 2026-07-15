# Debug Reference

## Purpose

Use this reference when you are doing local debugging and final fallback inspection in `awiki-cli`, especially for SQLite state inspection, migration-import verification, schema confusion, and situations where normal domain paths are no longer enough to explain the issue.

This file is a **reference**, not an entry skill. Load it only after all safe inspection paths have been exhausted.

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

## Available Commands

- `awiki-cli --diagnostic debug db handle-history <handle>`
- `awiki-cli --migration debug db import-v1 [--path <legacy_db>]`

## Limitations

- Do not expose JWTs, private keys, secure session material, or unrelated local files
- Do not use debug to bypass domain-level confirmation rules

## Side Effects and Confirmation

- Safe for narrow, non-destructive inspection:
  - `debug db handle-history`
- Require explicit confirmation and should use dry-run first:
  - `debug db import-v1`

## Recovery Pattern

1. Inspect with `debug db handle-history` or the matching domain command/schema.
2. Translate the findings back into canonical runtime, identity, or messaging behavior
3. Return to the matching domain reference instead of staying on the debug path for long

## Related References

- `02-identity.md`
- `03-messaging.md`
- `04-groups.md`
- `05-runtime.md`
- `01-onboarding.md`
