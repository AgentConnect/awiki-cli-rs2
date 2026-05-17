# Group E2EE Step B P6 Conformance Notes — awiki-cli

## Scope

- Update CLI P6 wire builders to match the current method matrix:
  - publish/get/notice: `transport-protected` and service/agent target.
  - create: `group-e2ee` with service target plus `body.group_did`.
  - add/send: `group-e2ee` with group target.
- Pass `ratchet_tree_b64u` through add/welcome processing.
- Pass stable message/operation IDs and P6 AAD metadata into `anp-mls` encrypt/decrypt.
- Refresh the service group state before encrypted send and avoid treating MLS epoch as P4 `group_state_version`.
- Replace pending/repair skeletons with hidden/test-only `group.e2ee.notice` pull, welcome replay, and mark-delivered.

## Public discovery stance

- CLI commands remain diagnostics/maintenance for hidden Group E2EE v1 focused validation.
- Do not update public help/docs to imply broad Group E2EE support; discovery remains controlled by message-service and must stay hidden by default.

## Config / packaging impact

- No new config key.
- `AWIKI_ANP_MLS_BINARY` and release-staged `anp-mls` remain the install path.
- Go CLI remains pure Go / no CGO; plaintext and MLS private state stay out of argv and service storage.

## Fresh validation evidence

- `go test ./internal/message ./internal/cli ./internal/cmdmeta ./internal/doctor -count=1` → passed (`internal/message` 3.329s, `internal/cli` 55.483s, `internal/cmdmeta` 3.412s, `internal/doctor` 6.761s).
- `go vet ./internal/message ./internal/doctor` → passed.
- Focused CLI system loop via `awiki-system-test` with `--with-message-v2 --use-local-anp` → passed: 2 passed in 18.65s.
- After focused system tests, the local environment was stopped and published ANP Python/Rust dependencies were restored.

## Rollback

- Revert the Step B wire/service additions. Pending/repair would return to diagnostic-only behavior and public discovery must remain disabled.

## Caveats

- Owner-only remove is routed through hidden PR-A `group.e2ee.remove` orchestration with local pending commit finalize/abort semantics. E2EE `group leave` is now routed to hidden/test-only `group.e2ee.leave_request`; owner processing uses `group e2ee process-leave-request` and the existing epoch-advancing `group.e2ee.remove` orchestration instead of submitting a same-epoch local-terminal leave artifact. Still no External Commit, attachment group E2EE, cloud snapshot, or product-wide public beta claim.
- No k1 DID compatibility is included.
