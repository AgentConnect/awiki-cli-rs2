# 2026-05-19 Identity Rust Selector Batch

Timestamp: 2026-05-19T01:36:16+0800.

Scope: expose deterministic identity Rust Cargo contract targets to the
`awiki-system-test` acceptance surface through a new Rust-only selector. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for identity selector
  visibility.
- Three read-only Native Agents mapped Go identity package/CLI behavior,
  current Rust identity contract targets, and `awiki-system-test` selector
  coverage in parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only
  `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py` and the nearest
  `tests_v2/cli/CLAUDE.md` member-list entry.
- The leader reviewed the scoped diff, ran direct Rust and focused Go guards,
  ran the new focused system selector, and batched docs/evidence.
- Existing dirty helper files in `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.
- `identity_register_email_live_contract`, `identity_recover_live_contract`,
  and `identity_replace_did_live_contract` were intentionally excluded for
  separate live-identity batches.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/identity/{did,handle_input,store}.go`, `internal/cli/id.go` | e1 DID generation, Ed25519 `key-1`, ANP service helpers, handle normalization, local create/list/current/use/status, dry-run validation, and legacy-config migration before local identity boundaries | `crates/awiki-cli/src/identity/*`, `src/app.rs` | implemented and focused-contract tested | `identity_contract` passed 17 tests | `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_rust_contracts` | low; local CLI/filesystem only; file is 1191 lines |
| `internal/identity/store.go`, key compatibility and legacy import tests | legacy ANP PEM migration, bad-key error shape, flat/indexed legacy scan/import, conflict handling, and E2EE legacy state copy | `crates/awiki-cli/src/identity/{key_compat,legacy,store}.rs` | implemented and focused-contract tested | `identity_key_compat_contract` passed 3 tests; `identity_legacy_import_contract` passed 4 | same Rust-only selector | low; local filesystem only |
| `internal/identity/{client,service,public}.go` | JSON-RPC/REST endpoint constants, request payloads, profile/register/recover/replace/bind/refresh result shapes, handle lookup interpretation, public data stripping, and warning preservation | `crates/awiki-cli/src/identity/wire.rs`, `src/authsdk/*`, `src/transportcfg.rs` | implemented and focused-contract tested | `identity_wire_contract` passed 11 tests | same Rust-only selector | low; pure in-process wire/result assertions |
| `internal/upgrade/*`, `internal/cli/id.go`, identity service tests | profile-set/register/replace-did legacy `config.json` upgrade-before-boundary behavior and validation ordering | `crates/awiki-cli/src/upgrade/*`, `src/config/*`, `src/app.rs`, `src/identity/*` | implemented and focused-contract tested | `identity_profile_set_upgrade_contract` passed 2 tests; `identity_register_upgrade_contract` passed 1; `identity_replace_did_upgrade_contract` passed 1 | same Rust-only selector | low; Go has closest upgrade/service guards rather than exact per-command selector tests |
| `internal/identity/service*.go`, `internal/cli/id.go` | register phone, refresh-token, bind phone/email, profile get/set, resolve by handle/DID, authenticated HTTP request shape, JWT persistence, and non-fatal warning handling | `crates/awiki-cli/src/identity/{service,client,wire,store}.rs`, `src/authsdk/*`, `src/transportcfg/http.rs` | implemented and loopback fake-service tested | `identity_live_contract` passed 16 tests | same Rust-only selector | low; target uses `127.0.0.1:0` fake server, not real external service; file is 1133 lines |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test identity_contract --test identity_key_compat_contract --test identity_legacy_import_contract --test identity_wire_contract --test identity_profile_set_upgrade_contract --test identity_register_upgrade_contract --test identity_replace_did_upgrade_contract --test identity_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/identity ./internal/cli ./internal/upgrade -run 'TestGenerateIdentity|TestNormalizeHandleInput|TestManagerSaveLoadAndCurrent|TestManagerSummaryShowsRegisteredUserState|TestManagerLoadBackfillsFullHandleFromDID|TestManagerLoadDoesNotBackfillFullHandleForNonHandleDID|TestRunIDReplaceDIDDryRunWarnsAndTargetsIdentity|TestRunIDRefreshTokenDryRunPlansDidAuthRefresh|TestRunIDRecoverDryRunUsesHandleAndWarnsWhenIdentityFlagIsIgnored|TestManagerLoadMigratesLegacyANPPrivateKeysToPKCS8|TestReplaceDIDConvertsLegacyANPK1KeyWhenJWTMissing|TestScanLegacyDetectsIndexedFlatInvalidAndOrphanArtifacts|TestImportLegacyRequiresNameWhenMultipleFlatCredentialsExist|TestImportAllLegacyImportsIndexedDefaultAndCopiesFlatE2EEState|TestImportAllLegacySkipsConflictingFlatCredentials|TestImportLegacyFlatIdentity|TestRemoteClientTranslatesRPCAndRESTErrors|TestLookupHandleByDIDHandlesNotFoundEmptyAndSuccess|TestServiceRegisterPhoneSendsNormalizedOTPRequest|TestServiceRegisterEmailVerifiedCreatesIdentity|TestServiceRegisterFullHandleUsesExplicitDomainForDID|TestServiceRegisterEmailSendsScopedVerificationForHandle|TestServiceBindPhoneUsesAuthenticatedRequestAndSanitizesOTP|TestServiceBindEmailStatusUsesAuthenticatedBearer|TestServiceResolveByDIDReturnsWarningsForNonFatalLookupFailures|TestServiceResolveFullHandleUsesLookupThenProfileByDID|TestServiceGetProfileByHandleReturnsBareAndFullHandleSubject|TestPublicDataStripsInternalUserIDFields|TestEvaluateUserStateUsesPublicFriendlyMissingFields|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededReplacesAllImportedLegacyK1Handles|TestUpgradeIfNeededReplacesExistingWorkspaceK1Handles|TestReplaceDIDUpdatesIdentityAndLocalStore|TestReplaceDIDStopsBeforeRemoteWhenBackupFails|TestServiceRegister|TestServiceBind|TestServiceResolve|TestServiceGetProfile|TestRefreshTokenUsesDIDAuthWithoutStoredBearerAndPersistsNewJWT|TestRunIDRecoverWithoutOTPReturnsSendOTPSuccess' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_identity_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_identity_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_rust_contracts -ra -q
```

Observed results:

- Rust direct identity contract targets passed 55 tests total:
  `identity_contract` 17, `identity_key_compat_contract` 3,
  `identity_legacy_import_contract` 4, `identity_wire_contract` 11,
  `identity_profile_set_upgrade_contract` 2,
  `identity_register_upgrade_contract` 1,
  `identity_replace_did_upgrade_contract` 1, and
  `identity_live_contract` 16.
- Focused Go identity guards passed for `internal/identity`, `internal/cli`,
  and `internal/upgrade`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 1.76s. The selector checks that the eight Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the new system-test wrapper is 81 lines, and
  `tests_v2/cli/CLAUDE.md` is 40 lines. The largest scoped Rust targets are
  `identity_contract.rs` at 1191 lines and `identity_live_contract.rs` at 1133
  lines, both below the default 1200-line visibility target. `xtask
  check-structure` reported no undocumented Rust files over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_identity_rust_contracts.py::test_awiki_cli_identity_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. `identity_live_contract` uses loopback fake HTTP
  servers, not external services.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for existing identity
  Rust contract targets that are deterministic local or loopback fake-service
  tests.
- `identity_register_email_live_contract`, `identity_recover_live_contract`,
  and `identity_replace_did_live_contract` remain excluded for later
  live-identity batches.
- This batch does not claim full repository-wide acceptance, live external
  identity service behavior, live mail-service behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
