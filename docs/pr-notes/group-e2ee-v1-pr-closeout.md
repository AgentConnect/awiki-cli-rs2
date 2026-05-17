# Group E2EE v1 PR Closeout Notes — awiki-cli

## Scope

- Extend `awiki-cli doctor` to inspect both legacy/root MLS state and real agent/device-scoped `anp-mls` state directories.
- Clean user-facing E2EE diagnostic wording so status/pending/repair no longer describe the real MLS path as a contract-test scaffold.
- Keep the Go CLI pure Go / no CGO and keep `anp-mls` as an exec provider using stdin/stdout.

## Commits / branch context

- Current branch is ahead of origin with recent Group E2EE work through `b0856f4 Make group E2EE CLI restore named-device MLS state`.
- This closeout is Step A only; it does not implement P6 conformance Step B.

## Config / migration impact

- No CLI business SQLite schema migration.
- No new config key.
- `AWIKI_ANP_MLS_BINARY` remains the explicit binary override.
- Doctor now reports scoped MLS state under `<workspace>/mls/agents/<agent>/<device>/state.db` and `state.lock` when present.

## Validation

- Fresh evidence collected in this Ralph pass:
  - `go test -count=1 ./internal/doctor ./internal/message ./internal/cli` → passed after the deslop pass (`internal/doctor` 3.643s, `internal/message` 1.256s, `internal/cli` 54.811s).
  - `go vet ./internal/doctor ./internal/message` → passed.

## Rollback

- Revert doctor scoped-state scan and CLI wording changes. This does not affect plain messaging.

## Caveats

- `group e2ee pending` / `repair` now use the hidden/test-only P6 notice pull/replay path; public discovery still requires the separate gate in Step B notes.
- Group E2EE remains hidden/test-only; no public discovery.
- No k1 DID compatibility work is included.
