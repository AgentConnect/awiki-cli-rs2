# awiki-cli

[![Rust Version](https://img.shields.io/badge/rust-1.88.0-blue.svg)](https://www.rust-lang.org/)
English

awiki-cli is the official Awiki command-line client and Skill backend. This repository is a Rust workspace that contains the CLI, `im-core`, Flutter/Dart bindings, and the awiki-me host daemon package; this README focuses on the CLI product surface.

Quick links: [Onboarding](./onboarding.md) · [Command Tree](./docs/architecture/awiki-command-v2.md) · [Architecture](./docs/architecture/awiki-v2-architecture.md)

## What is awiki-cli?

- Single binary CLI and Skill runner for the Awiki platform
- Built for "human + AI Agent" co-usage
- Handles identity (DID/handle), messaging, groups, pages, and runtime configuration
- Provides structured JSON output suitable for Agents
- Uses `im-core` for business workflows; the CLI layer owns parsing, config, dry-run plans, file IO, local runtime UX, and output envelopes
- Keeps awiki-me host daemon packaging separate from `awiki-cli runtime listener`; see `docs/publish.md` for daemon release details

## Installation & Onboarding

Basic development requirements:

- Rust toolchain from `rust-toolchain.toml` (`1.88.0`)
- Node.js 18+
- Sibling ANP Rust SDK at `../anp/anp/rust`
- Network access to the Awiki backend (e.g. `https://awiki.ai` or an internal test environment)

Build the CLI locally:

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
```

Build a release archive:

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
```

Initialize the workspace:

```bash
awiki-cli init
```

For the full first-time flow (identity registration or recovery, runtime setup, and status checks), please follow the onboarding guide:

- Onboarding: [onboarding.md](./onboarding.md)

## Project Layout

- `crates/awiki-cli/src/` — Rust CLI implementation, command metadata, handlers, runtime, storage, and update logic
- `crates/im-core/` — shared business service layer used by the CLI and app bindings
- `crates/awiki-deamon/` — awiki-me host daemon package; not managed by `awiki-cli runtime listener`
- `crates/awiki-cli/tests/` — Rust integration and contract tests
- `xtask/` — repository checks such as structure and version consistency
- `scripts/` — release, install, and verification scripts
- `skills/` — Awiki Skills exposed to AI Agents
- `docs/` — architecture and command-level documentation

## Config Template

- Standard config template: `./config.template.yaml`
- Default workspace config path: `~/.awiki-cli/config.yaml`

## Getting Help

- For architecture and command surface, see `docs/architecture/awiki-v2-architecture.md` and `docs/architecture/awiki-command-v2.md`.
- For Agent usage, refer to the Skill documentation visible in your environment (for example, entry/bundle Skills and identity/messaging Skills).
- For issues or feature requests, please contact the Awiki team using your normal project channels.
