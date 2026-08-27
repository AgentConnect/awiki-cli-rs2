# Identity Reference

## Purpose

Use this reference when you are handling identity lifecycle tasks in `awiki-cli`, including local identity inspection, handle-backed registration, recovery, contact binding, identity switching, and profile management.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves identity, DID, handle, contact binding, recovery, or profile data.

## Current Status

- Status: **implemented**
- Current public binary: `awiki-cli`
- External explanations should remain **handle-first**

## When to Use

- Create or import local identities
- Register or recover handle-backed identities
- Bind contact methods such as phone numbers or email addresses
- Switch the default identity
- Read or update DID profile

## Core Concepts

- **identity**: the local awiki identity container selected with `--identity`
- **DID**: the protocol-level identifier used by the server
- **handle**: the human-readable public identifier
- **contact binding**: adding a phone number or email address to an existing identity
- **current identity**: the default local identity used when `--identity` is omitted

## Lifecycle

`status -> register/import -> bind -> profile set -> current/use`

## Decision Rules

- No local identity yet -> use `awiki-cli id register ...`
- A local identity exists but does not yet have a handle-backed user state -> use `awiki-cli id register ...`
- A handle exists but contact bindings are incomplete -> use `awiki-cli id bind ...`
- The handle is lost but the recovery phone number is still available -> use `awiki-cli id recover ...`
- Need to inspect multiple local identities -> use `awiki-cli id list`
- Need to switch the default identity -> use `awiki-cli id use <identity>`
- Token state is abnormal, or the current identity authentication needs to be reacquired -> use `awiki-cli [--identity <identity>] id refresh-token`
- Need to inspect public profile data -> use `awiki-cli id profile get ...`

## Canonical Commands

- `awiki-cli id status`
- `awiki-cli id list`
- `awiki-cli id current`
- `awiki-cli id use <identity>`
- `awiki-cli [--identity <identity>] id refresh-token`
- `awiki-cli id register --handle <handle> (--phone <phone> [--otp <code>] | --email <email> [--wait])`
- `awiki-cli id bind (--phone <phone> [--otp <code>] | --email <email> [--wait])`
- `awiki-cli id resolve (--handle <handle> | --did <did>)`
- `awiki-cli id recover --handle <handle> --phone <phone> --otp <code>`
- `awiki-cli id profile get [--self | --handle <handle> | --did <did>]`
- `awiki-cli id profile set [--display-name ...] [--bio ...] [--tags ...] [--markdown ...] [--markdown-file ...]`
- `awiki-cli --migration id import-v1 [--name <identity> | --all]`

## Common Patterns

### Recommended Registration Flow

1. `awiki-cli id status`
2. `awiki-cli id register --handle alice --phone +8613800138000 --otp 123456`
3. `awiki-cli id current`
4. `awiki-cli id bind --email alice@example.com --wait`
5. `awiki-cli id profile set --display-name "Alice"`

### Import from v1 and Then Switch

1. `awiki-cli --migration id import-v1 --all --dry-run`
2. `awiki-cli --migration id import-v1 --all`
3. `awiki-cli id list`
4. `awiki-cli id use <identity>`

### Explicitly Refresh the JWT When the Token Is Abnormal

Applicable situations:

- The command indicates that current identity authentication has expired
- An identity clearly exists, but calls to authenticated APIs still fail
- You want to refresh the current identity authentication first before continuing with subsequent commands

Recommended usage:

1. `awiki-cli id current`
2. `awiki-cli --identity <identity> id refresh-token --dry-run`
3. After a human confirms the target identity, run `awiki-cli --identity <identity> id refresh-token`

## Side Effects and Confirmation

- Require explicit confirmation:
  - `id register`
  - `id bind`
  - `id refresh-token`
  - `id recover`
  - `id use`
  - `id profile set`
  - `id import-v1`
- Prefer `--dry-run` when a write operation supports it

## Error Handling

- The command shape for register or bind is unclear -> check `awiki-cli schema id register` or `awiki-cli schema id bind`
- The auth or token state is unclear -> try `awiki-cli [--identity <identity>] id refresh-token` first
- Identity is missing -> use `awiki-cli id list` and `awiki-cli id current`
- The state of the local store is unclear -> use `awiki-cli doctor`

## Implementation Notes

- `id import-v1` requires the global `--migration` gate
- External explanations should remain handle-first
- The public contract of this reference does not include `user_id`

## Related References

- `01-onboarding.md`
- `08-debug.md`
- `00-installation.md`
