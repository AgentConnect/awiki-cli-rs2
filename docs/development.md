# AWiki Client Workspace Development Guide

[English](development.md) | [简体中文](development.zh-CN.md)

## 1. Environment

- Rust from the root `rust-toolchain.toml`, minimum 1.88
- Node.js 18+
- Sibling ANP Rust SDK at `../anp/anp/rust`
- Flutter/Dart only for FFI or Flutter SDK work
- Bundled SQLite; CLI/Core normally do not require system SQLite

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 2. Rust gates

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p xtask -- check-structure
cargo run -p xtask -- check-version
```

Quick CLI and Daemon checks:

```bash
cargo run -p awiki-cli -- --help
cargo run -p awiki-cli -- version
cargo test -p awiki-deamon --locked
cargo run -p awiki-deamon -- status --state-root /tmp/awiki-deamon-state
```

## 3. Flutter SDK

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core
flutter test
```

Choose `--macos-only`, `--ios-only`, or `--android-only` for another platform.
Those commands build the complete platform artifact by default. Packaging for
one architecture may additionally use `--macos-arch arm64|x86_64` or
`--android-abi arm64-v8a`. Do not commit generated native artifacts without an
explicit release policy.

## 4. Local workspace state

The default is `~/.awiki-cli/`. For isolated tests:

```bash
export AWIKI_CLI_WORKSPACE_HOME_DIR=/tmp/awiki-cli-test
cargo run -p awiki-cli -- init
```

The CLI isolates backend, DID host, identity, SQLite, runtime, and logs by tenant. Do not construct owner queries manually or reuse private state across tenants.

## 5. Tenants

```bash
awiki-cli tenant list
awiki-cli tenant current
awiki-cli tenant setup acme \
  --backend-base-url https://api.acme.example \
  --did-host acme.example
awiki-cli tenant use acme
```

`tenant setup` is an idempotent onboarding entry point and refuses conflicting overwrites. Create a new tenant instead of reconfiguring one that already contains identities or database data.

## 6. CLI output contract

- JSON is canonical; `pretty`, `table`, and `ndjson` are views.
- Errors use stable `error.code`, `hint`, and `retryable` fields.
- Exit codes agree with the envelope result.
- Writes should support dry runs.
- Output must never contain secret material.

Envelope changes must update contract tests, Skill references, docs, Agent integration, and examples.

## 7. Release

Build CLI artifacts with:

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
scripts/release/build-release-artifact.sh --os darwin --arch arm64
```

Self-hosted channel tooling lives in `scripts/release/cli/`. Prepare a beta tag from a clean pushed release branch, publish and verify beta across platforms/Skill/onboarding/update, then prepare stable from the same commit. Never move an existing tag or promote an old artifact back to latest.

Publish Daemon artifacts with:

```bash
scripts/release/daemon/publish-multi-platform.sh
```

Real server configuration, GitHub tokens, and paths belong only in ignored configuration.

## 8. Development rules

- Keep `awiki-im-core` as the shared product SDK and the CLI as a thin shell.
- Keep Daemon responsible for Runtime plugins, RPC tokens, Agent DIDs, and audit state.
- Keep Dart SDK DTOs Core-owned; do not add AWiki Me presentation fields.
- Preserve DID/handle facts in high-risk output; display names do not replace routing or authorization identities.
- Never log root/private keys, JWTs, E2EE state, or registration/runtime tokens.

## 9. Pre-PR checklist

- [ ] Rust gates pass.
- [ ] Structure and versions agree.
- [ ] Behavior changes have tests.
- [ ] CLI schema/docs match implementation.
- [ ] Skills describe only real commands and flags.
- [ ] No real workspace, token, private key, or release configuration is included.
- [ ] README commands, status, and compatibility are updated when affected.
- [ ] Cross-repository changes record matching commits.
