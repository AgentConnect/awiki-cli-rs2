# People Reference

## Purpose

This reference describes the current people, relationship, and local-contact command boundary in `awiki-cli`.

Load it when the user asks whether people, follower, following, or local-contact capabilities already exist.

## Current Status

- Status: **implemented**
- Relationship and local-contact commands use `im-core` `DirectoryService`.

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

## Usage Guidance

- For relationship discovery, use `07-discovery.md`
- For message history or group inspection, use `03-messaging.md` or `04-groups.md`

## Confirmation Rules

The following commands change relationship state or local-contact state and should be treated as side-effecting commands:

- `people follow`
- `people unfollow`
- `people contacts save`

## Related References

- `07-discovery.md`
- `03-messaging.md`
- `04-groups.md`
