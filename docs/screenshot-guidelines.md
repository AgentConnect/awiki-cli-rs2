# README Screenshot and Demo Guidelines

[English](screenshot-guidelines.md) | [简体中文](screenshot-guidelines.zh-CN.md)

## 1. General purpose

Screenshots are not decoration. Every image must answer a user question:

- What does the product look like?
- How is the first operation completed?
- How does working with an agent differ from ordinary chat?
- Why is the CLI output suitable for automation?
- How does a self-hosted service prove real interoperability?

## 2. Files and dimensions

Recommended asset directory:

```text
docs/assets/readme/
```

| Asset | Recommended dimensions | Format |
| --- | --- | --- |
| README hero | 1440x900 or 1600x1000 | PNG/WebP |
| Feature screenshot | At least 1200x750 | PNG/WebP |
| Social Preview | 1280x640 | PNG |
| Terminal still | About 1400x800 | PNG |
| Short demo | 20-40 seconds, 1200-1440 pixels wide | MP4 converted to GIF/WebP, or a hosted video thumbnail |

Use lowercase English filenames with hyphens:

```text
awiki-me-hero-conversation.png
awiki-cli-first-message.gif
open-server-cross-domain-smoke.png
```

## 3. Privacy and security

Replace or obscure all of the following before capture:

- real names, avatars, phone numbers, and email addresses;
- complete DIDs, handles, and internal domains;
- access tokens, refresh tokens, JWTs, private keys, and verification codes;
- local usernames, absolute paths, IP addresses, and Team IDs;
- test account pools, real group members, and internal messages; and
- sensitive browser bookmarks, notifications, and menu-bar information.

Use consistent safe examples:

```text
alice@example.com
bob@example.com
alice.example
bob.example
did:wba:example.com:users:alice:e1_demo
```

## 4. Product screenshots

- Capture a real running version, not an obsolete design mockup.
- Keep annotations light and do not obscure the UI.
- Use a consistent scale, window size, and theme.
- Do not show errors, debug panels, or unfinished entry points unless the image documents a limitation.
- Add one sentence below each image explaining what the user should notice.

## 5. Terminal demos

- Use a clean shell prompt.
- Reveal commands step by step and leave output visible long enough to read.
- Do not record download waits or irrelevant logs.
- Show only the important output fields.
- Demonstrate `--dry-run` before executing a write operation.
- Use a font size of at least 18px so output remains legible on GitHub.

## 6. Alt-text templates

Recommended:

```markdown
![AWiki Me conversation view with a conversation list on the left and human-Agent task and authorization messages on the right](docs/assets/readme/awiki-me-hero-conversation.png)
```

Not recommended:

```markdown
![screenshot](image.png)
```

## 7. Update responsibility

Any pull request that changes one of the following must check whether the README assets need to be recaptured:

- top-level navigation;
- login or registration flow;
- core commands or output structure;
- Agent authorization or task cards;
- installation method;
- public service domain;
- compatibility status; or
- branding and icons.
