# awiki-cli-rs2 README Pre-release Maintainer Notes

[English](maintainer-notes.md) | [简体中文](maintainer-notes.zh-CN.md)

This document is not for end users.

## 1. Repository identity

Consider eventually renaming the repository to `awiki-client` or `awiki-client-workspace`, while retaining `awiki-cli` as the binary name. Until then, use `AWiki Client Workspace` as the README title and explain the historical repository name in the opening.

## 2. Suggested GitHub About

**Description**

```text
AWiki client workspace: CLI, Rust IM SDK, Agent Runtime Daemon, Flutter SDK, and agent skills for ANP messaging.
```

**Topics**

```text
agent, cli, rust, sdk, anp, did, messaging, flutter
```

## 3. P0: fix the public installation entry point

`onboarding.md`, `skills/references/00-installation.md`, and possibly generated onboarding/Skill documents still contain `{{AWIKI_CLI_CHANNEL_BASE_URL}}`. Do not publish a fake executable command before confirming the stable URL.

After publishing, verify the channel's `manifest.json`, `awiki-cli.tgz`, `awiki-cli-skill.tar.gz`, and `.well-known/agent-skills/index.json` under the actual public path produced by `publish-server.toml` and Nginx.

## 4. P0: version consistency

At the review baseline, `crates/awiki-cli/Cargo.toml` is `1.0.16`, the stable version in `scripts/release/cli/release-config.json` is `1.0.18`, and Skill metadata has another version. Before release, establish whether this is intentional layering or drift, and make the binary, wrapper, manifest, Skill, tag, and embedded commit traceable.

## 5. Maturity

Skill metadata describes Messaging, Runtime, Discovery, People, and Debug as partially implemented. The README must not claim a complete, production-ready client stack. Prefer a generated capability snapshot for every stable release over a vague hand-maintained list.

## 6. Naming

Use `AWiki` and `AWiki Daemon` publicly. Use `awiki-deamon` only for existing binaries, crates, and paths. Name the Chinese README `README.zh-CN.md`.

## 7. Default branch

The review baseline is `release/0710`; the default branch is `main`. The final README and installation fixes must reach the default branch.

## 8. Content moved out of the README

Keep complete command groups, Daemon development commands, full Core domain inventories, Dart SDK implementation details, release-server operations, development rules, and the complete documentation map in focused documents. The home page should retain only entry points and boundaries.
