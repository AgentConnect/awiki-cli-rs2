# 2026-05-19 Identity Live Rust Selector Batch

Timestamp: 2026-05-19T02:07:36+0800.

Scope: expose remaining deterministic non-mail identity live Rust Cargo
contract targets to the `awiki-system-test` acceptance surface through the
existing identity Rust-only selector wrapper. This batch does not change
production Rust behavior, does not add dependencies, and does not run or count
mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for identity live selector
  visibility.
- Pre-scanned remaining identity live targets, existing identity Rust-only
  selector coverage, and Go reference tests before editing.
- Reused `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py` because it
  already owns identity Rust-only selector entry points and Cargo helper
  functions.
- Added a separate `test_awiki_cli_identity_live_rust_contracts` selector for
  email registration, recover, and replace-did live subflows.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/identity/service_contract_test.go`, `internal/cli/id.go` | email registration without wait, already-verified wait, send-then-poll wait flow, scoped verification request shape, identity persistence, and legacy config handling around register validation | `crates/awiki-cli/src/identity/service.rs`, `src/identity/client.rs`, `src/app.rs` | implemented and loopback fake-service tested | `identity_register_email_live_contract` passed 3 tests | `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_live_rust_contracts` | low; local CLI subprocess plus `127.0.0.1:0` fake HTTP only |
| `internal/identity/service_test.go`, `internal/cli/id_test.go` | recover without OTP sends OTP and does not create identity, legacy config migration before send-OTP, recover with OTP posts recover-handle and finalizes local identity | `crates/awiki-cli/src/identity/recover.rs`, `src/identity/service.rs`, `src/app/id_recover_handlers.rs` | implemented and loopback fake-service tested | `identity_recover_live_contract` passed 3 tests | same Rust-only selector | low; local CLI subprocess plus `127.0.0.1:0` fake HTTP only |
| `internal/identity/service_test.go`, `internal/upgrade/*`, `internal/cli/id.go` | authenticated replace-did RPC, local store rebind, null optional role/endpoint mapping, DID-auth bootstrap via get-me when JWT is absent, and backup-root failure stopping before remote mutation | `crates/awiki-cli/src/identity/replace_did.rs`, `src/identity/service.rs`, `src/app/id_replace_did_handlers.rs`, `src/store/*` | implemented and loopback fake-service tested | `identity_replace_did_live_contract` passed 4 tests | same Rust-only selector | low; local CLI subprocess plus `127.0.0.1:0` fake HTTP only |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test identity_recover_live_contract --test identity_register_email_live_contract --test identity_replace_did_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/identity ./internal/cli -run 'TestServiceRegisterEmailVerifiedCreatesIdentity|TestServiceRegisterEmailSendsScopedVerificationForHandle|TestRunIDRecoverWithoutOTPReturnsSendOTPSuccess|TestRecoverStagesAndFinalizesSameHandleLiveIdentities|TestReplaceDIDUpdatesIdentityAndLocalStore|TestReplaceDIDConvertsLegacyANPK1KeyWhenJWTMissing|TestReplaceDIDStopsBeforeRemoteWhenBackupFails|TestRunIDReplaceDIDDryRunWarnsAndTargetsIdentity' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_identity_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_identity_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_live_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust direct identity live targets passed 10 tests total:
  `identity_register_email_live_contract` 3,
  `identity_recover_live_contract` 3, and
  `identity_replace_did_live_contract` 4.
- Focused Go identity live guards passed for `internal/identity` and
  `internal/cli`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 5.51s. The selector checks that the three Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the updated system-test wrapper is 112 lines, and
  `tests_v2/cli/CLAUDE.md` is 42 lines. Scoped Rust targets are
  `identity_register_email_live_contract.rs` 442 lines,
  `identity_recover_live_contract.rs` 609, and
  `identity_replace_did_live_contract.rs` 744. `xtask check-structure`
  reported no undocumented Rust files over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_live_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. All scoped targets use loopback fake HTTP servers, not
  external services.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for the remaining
  non-mail identity live Rust contract targets.
- It does not claim full repository-wide acceptance, live external identity
  service behavior, live mail-service behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
