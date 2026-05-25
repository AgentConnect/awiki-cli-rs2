# awiki-cli Group E2EE operations

## Status

- Supported Group E2EE product actions are exposed through high-level `im-core` APIs and canonical CLI flags.
- Low-level `group e2ee *` orchestration commands remain hidden/internal or stable unsupported; they are not a product contract.
- Harness map: [Group E2EE cross-repo feature map](../../../awiki-harness/features/group-e2ee.md).
- Protocol/SDK: [ANP SDK / anp-mls Group E2EE](../../../anp/anp/docs/e2e/group-e2ee-p6-anp-mls.md).
- Service API: [message-service Group API](../../../message-service/docs/api/ANP-client-server-api-group.md).
- Public discovery and service capability gates still decide whether secure operations are available for a concrete identity/workspace/service.

## CLI responsibility

`awiki-cli` owns argument parsing, workspace/config selection, dry-run rendering, errors, schema/help/completion, and user-facing output. Supported E2EE behavior must execute through `im-core` public services; the CLI must not orchestrate MLS or expose raw crypto artifacts.

Key boundaries:

- Group lifecycle uses `client.groups().create/add_member/remove_member/leave` with `GroupSecurityRequirement::Required`.
- Group secure state uses `client.secure().group(group).status()/repair()`.
- Group secure send uses `client.messages().send(... MessageSecurityMode::E2eeRequired ...)`.
- Sensitive plaintext and MLS private material must not be placed in argv, shell history, service requests, or service logs.
- CLI output must not include raw KeyPackage, prekey payloads, MLS notice bodies, provider stdout/stderr, session counters, ratchet counters, or raw secure outbox rows.

## Runtime and local state

The default supported path is `im-core` native group E2EE runtime/storage. Historical CLI exec-provider controls such as `AWIKI_ANP_MLS_BINARY` are not part of the default supported product path.

The CLI business store may cache group/message indexes and high-level secure summaries. Private MLS state and secure outbox internals are owned by `im-core`.

## User-facing command surface

Group E2EE is reached through normal group/message commands and high-level secure status/repair commands:

```bash
awiki-cli group create --name "Secure Group" --secure required
awiki-cli group add --group GROUP_DID --member DID --secure required
awiki-cli group remove --group GROUP_DID --member DID --secure required
awiki-cli group leave --group GROUP_DID --secure required
awiki-cli msg send --group GROUP_DID --secure required --text "..."
awiki-cli group secure status --group GROUP_DID
awiki-cli group secure repair --group GROUP_DID
```

Deprecated compatibility aliases:

- `group create/add/remove/leave --e2ee` maps to `--secure required`.
- `group create --message-security-profile group-e2ee` maps to `--secure required`.
- `group e2ee status/repair` maps to `group secure status/repair`.

Blocked/internal commands:

- `group secure diagnostics` and `group secure repair --explain` are stable unsupported in this version.
- `group e2ee publish-key-package/pending/process-leave-request/recover-member/update-key/rejoin` are hidden/internal or stable unsupported and must not appear in the default surface.

## Orchestration flows

### Create

1. CLI passes `GroupSecurityRequirement::Required` through the im-core group create request.
2. `im-core` submits the high-level group create request, initializes local group secure state, and submits the service secure head/bootstrap request.
3. CLI renders only the high-level group result and warnings.

### Add and welcome

1. CLI passes group, member, role, reason, and `GroupSecurityRequirement::Required`.
2. `im-core` performs directory lookup, member mutation, KeyPackage lease, commit/welcome generation, service secure mutation, notice handling, and local projection.
3. CLI renders only high-level group/member results and warnings.

### Send and receive

1. CLI sends `MessageSecurityMode::E2eeRequired` through `client.messages().send`.
2. `im-core` handles group snapshot/state lookup, encryption, transport, incoming decrypt, MLS notice processing, and local projection.
3. CLI does not build MLS payloads or wire RPC params.

### Repair and lifecycle

- `group secure status` returns high-level secure state and redacted problems.
- `group secure repair` converges pending notices, local MLS state, and service head comparison through `im-core`.
- `group remove --secure required` and `group leave --secure required` use secure-aware lifecycle APIs; low-level process-leave/update/rejoin commands are not the supported interface.

## Release and packaging

- `awiki-cli` must enable the `im-core/group-e2ee` feature in the default Linux/macOS build.
- The release artifact script verifies the Linux/macOS feature graph includes both `im-core` feature `group-e2ee` and `anp` feature `mls`.
- Release notes should state that low-level E2EE diagnostics are blocked/internal and that Windows E2EE package validation is not a blocker for this stage.
- Windows artifacts may still be produced by the generic release matrix, but Windows E2EE package/release validation is explicitly deferred and must not block Linux/macOS rollout.

## Validation

Focused CLI checks:

```bash
cargo fmt --all
cargo +stable check -p im-core --features group-e2ee --locked
cargo +stable check -p awiki-cli --locked
cargo +stable test -p im-core --features group-e2ee --locked lifecycle_
cargo +stable test -p awiki-cli --locked msg_secure
cargo +stable test -p awiki-cli --locked group_secure
cargo +stable test -p awiki-cli --locked e2ee
```

Cross-service validation is owned by [awiki-system-test Group E2EE system tests](../../../awiki-system-test/docs/group-e2ee-system-tests.md). Local CLI work does not require connecting to real domains during unit/contract validation.
