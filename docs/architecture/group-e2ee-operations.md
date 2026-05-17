# awiki-cli Group E2EE operations

## Status

- Hidden/test-only Group E2EE orchestration is implemented for focused local validation.
- Harness map: [Group E2EE cross-repo feature map](../../../awiki-harness/features/group-e2ee.md).
- Protocol/SDK: [ANP SDK / anp-mls Group E2EE](../../../anp/anp/docs/e2e/group-e2ee-p6-anp-mls.md).
- Service API: [message-service Group API](../../../message-service/docs/api/ANP-client-server-api-group.md).
- Public discovery remains disabled; CLI must not advertise `anp.group.e2ee.v1` / `group-e2ee` as generally available.

## CLI responsibility

`awiki-cli` owns orchestration, diagnostics, packaging hooks, and local user experience. It does not implement MLS cryptography in Go.

Key boundaries:

- Go main binary remains pure Go / no CGO.
- MLS operations go through `MLSExecProvider` and the Rust `anp-mls` binary.
- JSON request goes over stdin; JSON response comes from stdout; logs/errors come from stderr.
- Sensitive plaintext and MLS private material must not be placed in argv, shell history, service requests, or service logs.
- CLI business SQLite may store group/message indexes and crypto summaries, but MLS private state belongs only in `anp-mls` state databases.

## Local state and binary discovery

`anp-mls` is discovered in this order:

1. `AWIKI_ANP_MLS_BINARY`.
2. Test/runtime injected path.
3. `PATH`.
4. Release-staged helper path when packaged.

The default MLS root is under the CLI workspace, commonly `~/.awiki-cli/mls/`. Real private state is agent/device-scoped, for example:

```text
~/.awiki-cli/mls/agents/<agent-hash>/<device>/state.db
~/.awiki-cli/mls/agents/<agent-hash>/<device>/state.lock
```

`doctor` must report binary version/path and root plus agent/device-scoped `state.db` / `state.lock` health.

## User-facing command surface

Group E2EE is reached through normal group/message commands plus diagnostic E2EE subcommands:

```bash
awiki-cli group create --message-security-profile group-e2ee --e2ee ...
awiki-cli group add --group GROUP_DID --member DID --e2ee ...
awiki-cli group remove --group GROUP_DID --member DID --e2ee ...
awiki-cli group leave --group GROUP_DID --e2ee ...
awiki-cli msg send --group GROUP_DID --secure on --text "..."

awiki-cli group e2ee publish-key-package --purpose normal|recovery|update ...
awiki-cli group e2ee status --group GROUP_DID ...
awiki-cli group e2ee pending --group GROUP_DID ...
awiki-cli group e2ee repair --group GROUP_DID ...
awiki-cli group e2ee recover-member --group GROUP_DID --member DID ...
awiki-cli group e2ee update-key --group GROUP_DID --member DID ...
awiki-cli group e2ee rejoin --group GROUP_DID --member DID ...
awiki-cli group e2ee process-leave-request --group GROUP_DID --member DID ...
```

These commands are hidden/test-only until public discovery is approved. `schema`/help text may expose internal command metadata for tests, but product docs must keep the feature-gated status explicit.

## Orchestration flows

### Create

1. Submit P4 `group.create` with `message_security_profile=group-e2ee`.
2. Run `anp-mls group create` locally.
3. Submit hidden P6 `group.e2ee.create` to initialize the service crypto head.
4. Cache only business summaries locally.

### Add and welcome

1. Ensure target is P4 active through `group add`.
2. Lease target normal KeyPackage via `group.e2ee.get_key_package` with `body.group_did`; do not pass arbitrary non-service KeyPackage JSON into `anp-mls`.
3. Run `anp-mls group add-member` with the service-verified leased package and the cached P4 `group_state_ref.group_state_version`.
4. Submit `group.e2ee.add` with commit, welcome, ratchet tree, epoch, and crypto group ID.
5. Target pulls `group.e2ee.notice` and processes Welcome.

### Send and receive

1. Sender syncs the group snapshot, then encrypts with `anp-mls message encrypt` and canonical P6 AAD metadata including `group_state_ref.group_state_version`.
2. CLI submits `group.e2ee.send` with the opaque `group_cipher_object` as the direct P6 body.
3. Receiver pulls/history-reads the opaque cipher, decrypts with `anp-mls message decrypt`, and stores only the local plaintext view.

### Repair and lifecycle

- `pending` lists durable welcome/commit/update notices.
- `repair` replays missed welcome/commit/update notices and marks delivered only after local success.
- `remove` prepares a local pending MLS remove commit and finalizes only after service acceptance.
- `leave` creates a service-side leave request; owner later processes it through epoch-advancing remove.
- `recover-member` is for still-P4-active same-DID/device crypto recovery and never reactivates removed/left members.
- `update-key` consumes a target `purpose=update` KeyPackage and performs hidden owner-controlled leaf replacement.
- `rejoin` is a wrapper for fresh normal KeyPackage + canonical P4 re-add + P6 add/welcome; it is not `recover_member` and not External Commit.

## Release and packaging

- `scripts/release/build-anp-mls.sh` stages the Rust helper under `dist/anp-mls/<os>-<arch>/`.
- Release notes must describe Group E2EE as hidden/test-only until discovery is approved.
- CI/local tests may inject `AWIKI_ANP_MLS_BINARY` instead of relying on global PATH.

## Validation

Focused CLI checks:

```bash
cd awiki-cli
go test ./internal/message ./internal/cli ./internal/cmdmeta ./internal/doctor -count=1
go vet ./internal/message ./internal/doctor
```

Cross-service validation is owned by [awiki-system-test Group E2EE system tests](../../../awiki-system-test/docs/group-e2ee-system-tests.md).
