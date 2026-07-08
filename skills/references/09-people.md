# People Reference

## Purpose

This reference describes the current people, relationship, and local-contact command boundary in `awiki-cli`.

Load it when the user asks whether people, follower, following, or local-contact capabilities already exist.

## Current Status

- Status: **partially implemented**
- `people follow`, `people unfollow`, `people status`, `people followers`, `people following`, `people contacts list`, and `people contacts save` are implemented through `im-core` `DirectoryService`.
- `people search` remains unsupported until a search API is designed.

Do not describe people search as implemented.

## Current Implemented Scope

- follow / unfollow
- relationship status
- followers / following
- local contacts list and save

## Command Contracts

- `awiki-cli people follow <TARGET>`
- `awiki-cli people unfollow <TARGET>`
- `awiki-cli people status <TARGET>`
- `awiki-cli people followers [--limit N] [--offset N] [--profile]`
- `awiki-cli people following [--limit N] [--offset N] [--profile]`
- `awiki-cli people contacts list`
- `awiki-cli people contacts save --did <did> [--handle <handle>] [--display-name <name>] [--relationship <label>] [--reason <text>]`

`--name` is retained as a deprecated alias for `people contacts save`, but
new references should use `--display-name`.

## Future Planned Scope

- `awiki-cli people search <QUERY>`
- block / unblock
- recommendation and discovery flows

## Usage Guidance

- If the user needs relationship discovery today, use `07-discovery.md`
- If the user needs real message history or group inspection today, use `03-messaging.md` or `04-groups.md`
- If the user asks whether `people` is available, answer that relationship and local-contact commands are available, while `people search` is still unsupported.

## Confirmation Rules

The following commands change relationship state or local-contact state and should be treated as side-effecting commands:

- `people follow`
- `people unfollow`
- `people contacts save`

## Related References

- `07-discovery.md`
- `03-messaging.md`
- `04-groups.md`
