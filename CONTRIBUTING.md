# Contributing to AWiki Client Workspace

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

This repository contains the CLI, shared Rust SDK, AWiki Daemon, Dart/Flutter SDK, and Agent Skills. State which product surface a change affects and keep shared boundaries consistent.

## Before you start

- Search existing issues and pull requests.
- Open an issue before substantial API, command-tree, SecretVault, E2EE, Runtime plugin, or release-process changes.
- Do not combine unrelated crate refactors, documentation moves, and release configuration in one pull request.
- For changes spanning `awiki-me`, `awiki-open-server`, or ANP, record matching commits and compatibility verification.

## Rust gates

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p xtask -- check-structure
cargo run -p xtask -- check-version
```

## CLI

```bash
cargo run -p awiki-cli -- --help
cargo run -p awiki-cli -- version
```

When changing commands, flags, or output, update the schema, docs, output contract, tests, Skill references, and onboarding. Never make the Skill guess commands that do not exist.

## AWiki Daemon

```bash
cargo test -p awiki-deamon --locked
```

Runtime plugin, controller scope, local RPC, token, session/audit, and service-management changes require focused security review.

## Flutter/Dart SDK

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core
flutter test
```

Add the native build for the affected platform. SDK DTOs must not contain AWiki Me UI, cache, or presentation fields.

## Architecture rules

- `awiki-im-core` is the shared product SDK.
- The CLI is a thin shell.
- AWiki Daemon owns Runtime Host boundaries.
- The Flutter SDK exposes Core-owned DTOs and high-level APIs.
- Skills provide task routing, safety rules, and on-demand loading only.
- Hosts must not rebuild raw RPC, WebSocket, DID proof, local projection, or E2EE internals.

## Security

Never commit or output DID private keys, root keys, JWTs, bearer tokens, private Direct/Group E2EE state, KeyPackages, prekeys, ciphertext, Runtime RPC tokens, registration tokens, `publish-server.toml`, GitHub tokens, real server paths, user workspaces, SQLite databases, identity directories, logs, real messages, or unredacted test artifacts.

See [SECURITY.md](SECURITY.md).

## Pull request description

Include at least:

```text
Affected component(s)
User / Agent impact
Command or API contract changes
Security boundary changes
Compatibility impact
Tests run
Release or migration implications
```
