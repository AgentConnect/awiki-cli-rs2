# Codex Goal: Phase 1F + Phase 1G

Execute `docs/sdk-refactor/implementation-playbook.md` Phase 1F and Phase 1G together.

Goal: route group text send and the necessary inbox/history read subset through the `im-core` SDK façade when `AWIKI_USE_IM_CORE_MVP=1`, while keeping default CLI behavior on the legacy path.

Follow the migration plan in `docs/sdk-refactor/phase1f-1g-migration-plan.md`.

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
- `docs/sdk-refactor/phase1f-1g-migration-plan.md`

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

Already completed or expected before starting:

- Phase 1A: `crates/im-core` skeleton exists.
- Phase 1B: `crates/awiki-cli/src/im_core_adapter` exists, and `awiki-cli` depends on `im-core`.
- Phase 1C: `crates/im-core` has identity/auth façade APIs.
- Phase 1D: `id register` and `id refresh-token` MVP paths exist.
- Phase 1E: direct text `msg send --to` MVP path exists.

Keep the workspace separated from unrelated changes. Do not revert existing user or previous-phase changes.

## Phase 1F Scope

Route group text send through SDK DTO / adapter façade when `AWIKI_USE_IM_CORE_MVP=1`.

Implement:

- `msg send --group <group> --text <text>` MVP path.
- `msg send --group <group> --text-file <path>` MVP path.

Rules:

- Build `im_core::SendMessageRequest` before converting to legacy request.
- Use `MessageTarget::Group(GroupRef)` for group target.
- Use `MessageBody::Text` for `--text` / `--text-file`.
- Support `MessageSecurityMode::DefaultPlain` and `Plain`.
- Adapter may call old `message::send` internally.
- Keep dry-run in CLI.
- Direct text send from Phase 1E must keep working.
- Default path without `AWIKI_USE_IM_CORE_MVP=1` must remain legacy and behavior-compatible.

Unsupported / out of scope:

- `--file` / attachment.
- `--secure on`, `--secure direct`, `SecureDirect`.
- `GroupE2ee` / group secure send.
- group lifecycle commands.
- `group messages` migration.

## Phase 1G Scope

Route necessary inbox/history reads through SDK DTO / adapter façade when `AWIKI_USE_IM_CORE_MVP=1`.

Implement:

- `msg inbox` necessary subset:
  - `--scope all|direct|group`
  - `--limit`
  - `--unread`
- `msg history --with <peer> --limit <n> [--cursor <cursor>]`

Rules:

- Build `im_core::InboxQuery` before converting to legacy inbox request.
- Build `(im_core::ThreadRef::Direct(peer), im_core::HistoryQuery)` before converting to legacy history request.
- Adapter may call old `message::inbox` and `message::history` internally.
- Keep dry-run in CLI.
- Preserve current output shape where practical.
- Default path without `AWIKI_USE_IM_CORE_MVP=1` must remain legacy and behavior-compatible.

Current command-surface boundary:

- Current `msg.history` command only declares `--with`; do not add `msg history --group` in this goal unless the command contract and legacy behavior are explicitly updated as part of a documented scope change.
- `ThreadRef::Group` may remain supported in adapter-level DTO tests, but it is not a required CLI contract for this goal.

Fallback / unsupported strategy:

- `msg inbox --with ...` and `msg inbox --group ...` should stay on legacy path under MVP flag unless `InboxQuery` is formally extended first. Do not silently drop these filters.
- `msg inbox --mark-read` should stay on legacy path under MVP flag or return explicit unsupported with tests. Prefer legacy fallback to preserve current side effect.
- Do not put `mark_read`, `with`, or `group` filter fields into `im_core::InboxQuery` only to satisfy CLI migration.

## Hard Boundaries

Do not:

- Migrate whole `message/*`, `store/*`, `runtime/*`, `group/*`, `identity/*`, or `authsdk/*`.
- Migrate attachments.
- Migrate direct secure, group secure, MLS, or E2EE.
- Migrate realtime runner / daemon.
- Migrate group lifecycle.
- Migrate full mark-read semantics.
- Migrate conversation projection.
- Migrate complex local cache merge.
- Migrate complete unread count semantics.
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

- `crates/im-core/src/messages`
- `crates/awiki-cli/src/im_core_adapter`
- `crates/awiki-cli/src/app/msg_handlers.rs`
- `crates/awiki-cli/src/message`
- `crates/awiki-cli/src/cmdmeta/mod.rs`
- current message contract tests

Track B: Phase 1F implementation.

- Route group plain text `msg send` into MVP branch under `AWIKI_USE_IM_CORE_MVP=1`.
- Extend direct-only legacy conversion into direct-or-group text conversion.
- Keep attachment and secure boundaries explicit.
- Keep dry-run rendering in CLI.

Track C: Phase 1G implementation.

- Add gated MVP branches for `msg inbox` necessary subset and `msg history --with`.
- Convert CLI flags to `InboxQuery` / `ThreadRef + HistoryQuery` first.
- Convert SDK DTO to old request inside adapter.
- Keep inbox filters and mark-read out of DTO path unless explicitly documented.

Track D: Tests and validation.

- Add focused adapter tests for group send, inbox query, history query, and unsupported boundaries.
- Add CLI contract tests for MVP group send, inbox, and history dry-run / fixture behavior.
- Preserve existing legacy tests.

## Required Validation

Run:

```bash
cargo fmt --all --check
cargo test -p im-core --locked
cargo test -p awiki-cli im_core_adapter --locked
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
Do not run real `awiki-cli msg send`, `awiki-cli msg inbox`, or `awiki-cli msg history` unless using an isolated test fixture / dry-run / mock server.

## Commit Guidance

Do not mix unrelated changes.

Preferred commits:

1. `feat: route group text send mvp through im-core facade`
2. `feat: route inbox history mvp through im-core facade`

If implementation is strongly coupled, one commit is acceptable:

`feat: route phase 1f 1g through im-core facade`

Before committing, confirm `git status --short` has no unrelated files.

## Final Report

Report:

- Modified files.
- Phase 1F completed behavior.
- Phase 1G completed behavior.
- Key functions added/changed.
- Unsupported/fallback strategy for attachment, secure, inbox filters, mark-read, and group history command surface.
- Test commands and results.
- Whether any real CLI commands were run.
- Remaining work for Phase 1H and later phases.
