# AWiki Client Workspace README Asset Plan

[English](screenshot-plan.md) | [简体中文](screenshot-plan.zh-CN.md)

A CLI project does not need many images, but it does need a real terminal demo proving that the CLI is Agent-friendly.

## 1. Hero GIF: first message

- File: `awiki-cli-first-message.gif`
- Length: 25-40 seconds
- Recommended size: 1400x800
- Flow: `awiki-cli status --format json`, `awiki-cli id status`, `awiki-cli msg send ... --dry-run`, actual send, then `awiki-cli msg inbox --format table` or JSON
- Goal: demonstrate both a readable human view and stable machine output
- Never show a template installation URL, real identity, token, phone number, or absolute path

## 2. JSON envelope still (optional)

- File: `awiki-cli-json-envelope.png`
- Show `ok`, `command`, `data`, `warnings`, and `meta`.
- Use an `example.com` demo identity instead of a complete real DID.
- Place it near the output-contract section.

## 3. Workspace architecture

Prefer README Mermaid so the diagram does not drift. Export `awiki-client-workspace-architecture.png` only for a Social Preview. Show Skill/CLI/Daemon/Dart SDK to IM Core to Services, not every internal crate module.

## 4. Agent Host Notification demo (optional)

- File: `awiki-cli-host-notification.gif`
- Flow: remote message, listener, Host Notification, Agent receives the notice
- Hide tokens, session keys, channels, and real platform accounts.
- Put this in the main README only after the complete path is stable and verified; otherwise keep it in focused documentation.

## 5. Social Preview

- File: `awiki-client-workspace-social-preview.png`
- Size: 1280x640
- Copy: `CLI, IM SDKs, Agent Runtime and Skills for ANP messaging`
- Visual: terminal excerpt and simplified component arrows, without a dense crate list

## 6. Recording requirements

- Keep the shell prompt simple and use text of at least 18px.
- Use an isolated workspace and demo handles/DIDs.
- Remove environment variables, user directories, and internal domains.
- Dry-run writes first.
- Do not show `debug db`, raw secrets, or unstable commands.
