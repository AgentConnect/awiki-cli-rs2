# People Planned Reference

## Purpose

This reference exists only to describe the current contract boundary for future people and relationship capabilities in `awiki-cli`.

This file is a **planned appendix**, not a normal operational reference. Load it only when the user asks whether people, follower, following, or local-contact capabilities already exist.

## Current Status

- Status: **planned**
- The corresponding command handlers are not implemented in the current repository

Do not describe these commands as working current features.

## Future Planned Scope

- people search
- follow / unfollow
- relationship status
- followers / following
- local contacts list and save

## Planned Command Contracts

- `awiki-cli people search <QUERY>`
- `awiki-cli people follow <TARGET>`
- `awiki-cli people unfollow <TARGET>`
- `awiki-cli people status <TARGET>`
- `awiki-cli people followers`
- `awiki-cli people following`
- `awiki-cli people contacts list`
- `awiki-cli people contacts save --did <did> [--handle <handle>] [--reason <text>]`

## Usage Guidance

- If the user needs relationship discovery today, use `07-discovery.md`
- If the user needs real message history or group inspection today, use `03-messaging.md` or `04-groups.md`
- If the user asks whether `people` is available, answer that the contract is reserved but currently unimplemented

## Future Confirmation Rules

If these commands are implemented in the future, the following commands will require explicit confirmation because they change relationship state or local-contact state:

- `people follow`
- `people unfollow`
- `people contacts save`

## Related References

- `07-discovery.md`
- `03-messaging.md`
- `04-groups.md`
