# 2026-05-18 Identity Profile/Resolve/Bind/Refresh Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the identity
  profile/resolve/bind/refresh cluster.
- Three read-only Native Agents mapped Go reference behavior, current Rust
  implementation/tests, and non-mail `awiki-system-test` selector coverage in
  parallel before implementation.
- The scan found no production Rust gap for `id profile get/set`,
  `id resolve`, `id bind`, or `id refresh-token`. The remaining gaps were
  focused evidence gaps: service-level warning coverage for `resolve --did`
  optional lookup failures, and Rust-only legacy-config migration selectors for
  public handle/DID/email command variants.
- Two bounded GPT-5.5 xhigh Native Agents worked in non-overlapping write
  scopes: one modified only `crates/awiki-cli/tests/identity_live_contract.rs`,
  and one modified only `awiki-system-test/tests_v2/id/test_identity_cli.py`.
- Mail selectors remained deferred and were not run or counted.
- No Cargo dependency, ANP SDK dependency, manifest, or Rust production file
  changed.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/identity/service.go` `Resolve` | `id resolve --did` requires profile `resolve`, then treats handle lookup and public profile lookup as non-fatal warnings; failed optional payloads are omitted | `src/identity/service.rs`, `tests/identity_live_contract.rs` | implemented; focused negative-path contract added | `identity_resolve_did_live_warns_for_non_fatal_lookup_failures_like_go` | existing and new `tests_v2/id` resolve selectors cover public CLI migration boundaries | low after direct warning contract |
| `internal/cli/id.go`, `internal/identity/service.go` profile get | `profile get` defaults to self, and handle/DID targets cross the service boundary only after workspace resolution | `src/app.rs`, `src/identity/service.rs`, `tests/identity_live_contract.rs` | implemented; selector visibility expanded | existing self/handle/DID profile live contracts | `test_id_profile_get_rust_migrates_legacy_config_json_before_public_service_boundary[handle]`, `[did]` | low; selectors use a missing migrated CA bundle to stop before remote service dependence |
| `internal/cli/id.go`, `internal/identity/service.go` bind | `bind --phone` and `bind --email` share the active identity/auth boundary after config resolution | `src/app.rs`, `src/identity/service.rs`, `tests/identity_live_contract.rs` | implemented; email selector visibility added | existing bind phone/email live contracts | `test_id_bind_email_rust_migrates_legacy_config_json_before_identity_boundary`; existing phone selector retained | low |
| `internal/cli/id.go`, `internal/identity/service.go` refresh-token | non-dry-run refresh upgrades before active identity/auth lookup; dry-run plans DID-auth refresh without workspace upgrade, matching Go `resolveConfigForWorkspace` dry-run behavior | `src/app.rs`, `src/identity/service.rs`, `tests/identity_contract.rs`, `tests/identity_live_contract.rs` | implemented; no new dry-run migration selector because Go and Rust both skip upgrade in dry-run | existing dry-run and live refresh-token contracts | `test_id_refresh_token_rust_migrates_legacy_config_json_before_identity_boundary`; `test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh` | low; dry-run no-upgrade is recorded as parity, not a gap |
| `internal/cli/id.go`, `internal/identity/service.go` resolve handle/DID | target validation happens before service calls, while valid handle/DID targets cross service setup after workspace upgrade | `src/app.rs`, `src/identity/service.rs`, `tests/identity_contract.rs`, `tests/identity_live_contract.rs` | implemented; selector visibility expanded | existing resolve validation, handle, DID, and new warning contracts | `test_id_resolve_rust_migrates_legacy_config_json_before_target_validation`; `test_id_resolve_rust_migrates_legacy_config_json_before_service_boundary[handle]`, `[did]` | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/identity ./internal/cli ./internal/authsdk -run 'Test.*(Profile|Resolve|Bind|Refresh|JWT|GetMe|Lookup|Phone|Email)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --locked identity_resolve_did_live_warns_for_non_fatal_lookup_failures_like_go
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --test authsdk_contract --test identity_contract --test identity_live_contract --test identity_register_email_live_contract --test identity_profile_set_upgrade_contract --locked
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/id/test_identity_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/id/test_identity_cli.py --collect-only -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/id/test_identity_cli.py::test_id_profile_get_rust_migrates_legacy_config_json_before_public_service_boundary tests_v2/id/test_identity_cli.py::test_id_bind_email_rust_migrates_legacy_config_json_before_identity_boundary tests_v2/id/test_identity_cli.py::test_id_refresh_token_rust_migrates_legacy_config_json_before_identity_boundary tests_v2/id/test_identity_cli.py::test_id_resolve_rust_migrates_legacy_config_json_before_service_boundary -ra -q
```

Observed results:

- Focused Go identity/profile/resolve/bind/refresh guard passed for
  `internal/identity`, `internal/cli`, and `internal/authsdk`.
- Focused Rust warning contract passed: 1 passed, 0 failed.
- Rust identity batch contracts passed 64 tests across `authsdk_contract`,
  `identity_contract`, `identity_live_contract`,
  `identity_profile_set_upgrade_contract`,
  `identity_register_email_live_contract`, and `identity_wire_contract`.
- System-test Python compile passed for `tests_v2/id/test_identity_cli.py`.
- System-test collect-only found 34 identity tests.
- Focused non-mail Rust-under-test identity selectors passed with 6 passed, 0
  failed, and 0 skipped in 0.61s.
- `identity_live_contract.rs` is 1133 lines after the new test, still below the
  1200-line visibility target. `tests_v2/id/test_identity_cli.py` is 1286
  lines, above the older 1200-line preference but well below the current
  ordinary 3000-line limit.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
AWIKI_CLI_UPDATE_CACHE_ONLY=1
PYTHONDONTWRITEBYTECODE=1
AWIKI_SYSTEM_TEST_MODE=local
user-service URL=https://anpclaw.com
message-service URL=https://anpclaw.com
WebSocket URL=ws://127.0.0.1:9900
DID domain=anpclaw.com
```

Boundary note: this batch strengthens already implemented identity service
coverage and public local selector visibility. It does not claim recover,
replace-did, legacy identity import, page ownership, mail selectors, live
user-service/email verification availability, or broad repository-wide
acceptance. `refresh-token --dry-run` legacy-config migration was intentionally
not added as a selector because both Go and Rust return after raw config
resolution in dry-run mode before workspace upgrade.
