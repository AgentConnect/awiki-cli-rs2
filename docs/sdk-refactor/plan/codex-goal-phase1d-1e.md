# Codex Goal: Phase 1D + Phase 1E

Execute `docs/sdk-refactor/implementation-playbook.md` Phase 1D and Phase 1E together.

Goal: route identity auth / Handle registration and direct text send through the `im-core` SDK façade when `AWIKI_USE_IM_CORE_MVP=1`, while keeping default CLI behavior on the legacy path.

Do not migrate broad legacy modules. This is a CLI adapter / façade goal.

## Required Reading

Read and obey these documents before editing code.

Core docs:

- `docs/sdk-refactor/README.md`
- `docs/sdk-refactor/architecture.md`
- `docs/sdk-refactor/implementation-playbook.md`
- `docs/sdk-refactor/public-api.md`
- `docs/sdk-refactor/cli-boundary.md`
- `docs/sdk-refactor/im-core-cli-boundary.md`
- `docs/sdk-refactor/merge-decisions.md`

All Interface docs:

- `docs/sdk-refactor/Interface/README.md`
- `docs/sdk-refactor/Interface/01-crate-layout.md`
- `docs/sdk-refactor/Interface/02-core-interface.md`
- `docs/sdk-refactor/Interface/03-identity-auth-interface.md`
- `docs/sdk-refactor/Interface/04-message-interface.md`
- `docs/sdk-refactor/Interface/05-cli-adapter-interface.md`
- `docs/sdk-refactor/Interface/06-implementation-map.md`
- `docs/sdk-refactor/Interface/07-phase1-acceptance.md`

All module docs:

- `docs/sdk-refactor/modules/01-core.md`
- `docs/sdk-refactor/modules/02-identity.md`
- `docs/sdk-refactor/modules/03-auth.md`
- `docs/sdk-refactor/modules/04-local-state.md`
- `docs/sdk-refactor/modules/05-discovery.md`
- `docs/sdk-refactor/modules/06-directory.md`
- `docs/sdk-refactor/modules/07-messages.md`
- `docs/sdk-refactor/modules/08-groups.md`
- `docs/sdk-refactor/modules/09-attachments.md`
- `docs/sdk-refactor/modules/10-secure.md`
- `docs/sdk-refactor/modules/11-realtime.md`

## Preconditions

Already completed:

- Phase 1A: `crates/im-core` skeleton exists.
- Phase 1B: `crates/awiki-cli/src/im_core_adapter` exists, and `awiki-cli` depends on `im-core`.
- Phase 1C: `crates/im-core` has identity/auth façade APIs.

Keep the workspace cleanly separated from unrelated changes.

## Phase 1D Scope

Route identity auth and Handle registration through the SDK façade when `AWIKI_USE_IM_CORE_MVP=1`.

Implement:

- `id register` MVP path.
- `id refresh-token` MVP path.

Rules:

- CLI continues to own argument parsing, dry-run, rendering, OTP / phone / email / wait UX, output format, and filesystem UX.
- `im_core_adapter` may call old `identity::register`, `identity::register_plan`, and refresh-token logic internally as the transition adapter.
- The MVP path must translate through SDK-shaped DTOs first, such as `im_core::RegisterHandleRequest`.
- Default path must remain legacy and behavior-compatible.
- `im-core` must not depend on `awiki-cli`.
- Do not expose `identity_name`, auth path, SQLite path, `ActorContext`, or CLI types in SDK public API.

Tests should cover:

- Register DTO construction.
- MVP dry-run register path does not perform real network work.
- Refresh-token MVP path selects identity and maps auth result/error.
- At least one alice/bob selector or path isolation fixture.

## Phase 1E Scope

Route direct text send through SDK DTO / adapter façade when `AWIKI_USE_IM_CORE_MVP=1`.

Implement:

- `msg send --to <peer> --text <text>` MVP path.
- `msg send --to <peer> --text-file <path>` MVP path.

Rules:

- Build `im_core::SendMessageRequest` before converting to legacy request.
- Transition adapter may call old `message::send` internally.
- Support only direct text send in this goal.
- Support `MessageSecurityMode::DefaultPlain` and `Plain`.
- Keep dry-run in CLI.
- Do not migrate group send, inbox, history, mark-read, attachment, secure, realtime, or E2EE.

Unsupported / out of scope:

- `--file` / attachment: fixed unsupported behavior or explicit legacy fallback with tests.
- `--secure on`, `--secure direct`, `SecureDirect`: unsupported or explicit legacy fallback with tests.
- `--group`: do not count as Phase 1E; keep legacy or fixed unsupported behavior with tests.

## Hard Boundaries

Do not:

- Migrate whole `identity/*`, `authsdk/*`, `message/*`, `store/*`, or `runtime/*`.
- Migrate group lifecycle.
- Migrate attachments.
- Migrate realtime runner / daemon.
- Migrate direct secure, group secure, MLS, or E2EE.
- Migrate `recover_handle`, `replace_did`, profile get/set, contacts, relation status.
- Modify `debug.db.*`.
- Introduce async runtime, async trait, tokio, spawn_blocking, or provider traits.
- Change default CLI behavior outside `AWIKI_USE_IM_CORE_MVP=1`.

`crates/im-core/src` must not reference:

- `ParsedCommand`
- `GlobalOptions`
- `ExitError`
- `config::Resolved`
- `identity::Manager`
- `crate::app`
- `crate::cli`
- `crate::config`
- `awiki_cli`

`im-core` public API must not expose:

- `ActorContext`
- `StoredIdentity`
- `ClientIdentityRuntime`
- `IdentityRuntimePaths`
- raw RPC params
- SQLite connection
- WebSocket frame
- secure session / prekey / MLS paths
- raw `serde_json::Value` as default message public field

## Suggested Work Tracks

Track A: Read docs and inspect current code.

- `crates/im-core`
- `crates/awiki-cli/src/im_core_adapter`
- `crates/awiki-cli/src/app.rs`
- `crates/awiki-cli/src/app/msg_handlers.rs`
- current identity and message legacy implementations

Track B: Phase 1D implementation.

- Add gated MVP branches for `id register` and `id refresh-token`.
- Keep legacy branches unchanged.
- Use `im_core_adapter` for DTO conversion, error mapping, and old implementation bridging.

Track C: Phase 1E implementation.

- Add gated MVP branch for direct text `msg send`.
- Convert CLI flags to `SendMessageRequest`.
- Convert SDK DTO to old send request inside adapter.
- Keep group/inbox/history out.

Track D: Tests and validation.

- Add focused unit/contract tests for new gated paths and unsupported boundaries.
- Preserve existing tests.

## Required Validation

Run:

```bash
cargo fmt --all --check
cargo test -p im-core --locked
cargo test -p awiki-cli im_core_adapter --locked
cargo test -p awiki-cli identity --locked
cargo test -p awiki-cli msg --locked
cargo run --bin xtask --locked -- check-structure
```

If time allows, also run:

```bash
cargo test -p awiki-cli --locked
```

Boundary grep:

```bash
rg -n "ParsedCommand|GlobalOptions|ExitError|config::Resolved|identity::Manager|crate::app|crate::cli|crate::config|awiki_cli|ActorContext|StoredIdentity|ClientIdentityRuntime|IdentityRuntimePaths|serde_json::Value" crates/im-core/src || true
```

Do not run real production CLI commands against real services or real user workspaces.
Do not run real `awiki-cli id register`, `awiki-cli id refresh-token`, or `awiki-cli msg send` unless using an isolated test fixture / dry-run / mock server.

## Commit Guidance

Do not mix unrelated changes.

Preferred commits:

1. `feat: route identity auth mvp through im-core facade`
2. `feat: route direct text send mvp through im-core facade`

If implementation is strongly coupled, one commit is acceptable:

`feat: route phase 1d 1e through im-core facade`

Before committing, confirm `git status --short` has no unrelated files.

## Final Report

Report:

- Modified files.
- Phase 1D completed behavior.
- Phase 1E completed behavior.
- Key functions added/changed.
- Unsupported/fallback strategy for attachment, secure, and group send.
- Test commands and results.
- Whether any real CLI commands were run.
- Remaining work for Phase 1F / 1G.
