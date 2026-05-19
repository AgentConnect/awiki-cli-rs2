# 2026-05-19 Attachment Contract Timeout System-Test Fix

Timestamp: 2026-05-19T21:30:00+0800.

Scope: close the only failure found by a broad non-mail
`awiki-system-test/tests_v2` pass after the K1 subprocess selector. This batch
changes only the deterministic Rust attachment test fixture lifetime. It does
not change production attachment behavior, does not add dependencies, and does
not run or count mail selectors.

Pipeline:

- Followed the system-test-first lane and prioritized the concrete
  `tests_v2` failure before starting another translation batch.
- Treated the broad non-mail run result as the acceptance signal to triage:
  173 passed, 1 failed, and 16 skipped in 1016.72s.
- Scoped the failure to the Rust-only wrapper
  `tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts`
  and its underlying Cargo target
  `cargo +1.79.0 test -p awiki-cli --test attachment_live_contract --locked`.
- Confirmed the failing production assertion was not a Go/Rust attachment
  semantic mismatch. The fake HTTP server in
  `crates/awiki-cli/tests/attachment_live_contract.rs` could close its accept
  loop before the subprocess made the request under broad-run load, so the CLI
  saw `Connection refused` and mapped that transport failure to
  `internal_error`.
- Extended only the local `TestServer` accept deadline from 30 seconds to 120
  seconds so the fake service covers slow broad/system-test subprocess startup.
- Did not touch `awiki-system-test` helper files. Existing dirty helper files
  in that repository remain unrelated and unstaged.

Failure evidence:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q --ignore=tests_v2/mail tests_v2
```

Observed broad-run result:

- Overall: 173 passed, 1 failed, and 16 skipped in 1016.72s.
- Failed selector:
  `tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts`.
- Functional domain: message attachment deterministic Rust contracts.
- Underlying failing Rust test:
  `attachment_live_contract::msg_attachment_download_error_mapping_matches_go_attachment_codes`.
- Failure symptom: expected attachment missing-object mapping was masked by
  loopback transport setup timing.

Observed failing JSON before the fixture fix:

```json
{
  "error": {
    "code": "internal_error",
    "hint": "Make sure the message id, attachment id, and target context are correct.",
    "message": "Connection refused (os error 111)",
    "retryable": false
  },
  "meta": {
    "dry_run": false,
    "format": "json",
    "version": "dev"
  },
  "ok": false
}
```

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/attachment_service.go`, `internal/cli/msg.go` | attachment download maps service missing-object failures to the Go-shaped attachment error code instead of a generic transport failure | `crates/awiki-cli/src/message/attachment_service.rs`, `crates/awiki-cli/tests/attachment_live_contract.rs` | production behavior already implemented; test fake-server lifetime stabilized | `attachment_live_contract::msg_attachment_download_error_mapping_matches_go_attachment_codes` passed | `tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts` passed | low after fixture lifetime extension; full broad rerun still pending |

Commands run after the fix:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test attachment_live_contract msg_attachment_download_error_mapping_matches_go_attachment_codes --locked -- --exact
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test attachment_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q --ignore=tests_v2/mail tests_v2
```

Observed results:

- Focused failing Rust function passed with 1 passed, 0 failed, and finished in
  31.18s.
- Full `attachment_live_contract` target passed with 4 passed, 0 failed, and
  finished in 76.69s.
- Focused non-mail `awiki-system-test` wrapper passed with 1 passed, 0 failed,
  and 0 skipped in 74.86s.
- Rust formatting, package check, structure check, and whitespace check passed.
- `xtask check-structure` reported `structure ok: no undocumented Rust files
  over 1200 lines`.
- Full broad non-mail `tests_v2` rerun after the fixture fix passed with 174
  passed, 0 failed, and 16 skipped in 841.35s.

System-test configuration context:

- Test mode: Rust CLI under test through `awiki-system-test/tests_v2`.
- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_GROUP_E2EE_CONTRACT_TEST=0` for the failed broad run.
- `AWIKI_ENABLE_MAIL_TESTS=0` for the failed broad run.
- `PYTHONDONTWRITEBYTECODE=1`.
- Broad run ignored mail selectors with `--ignore=tests_v2/mail`.
- Focused fix verification used only the deterministic Rust attachment wrapper
  selector and loopback fake services. It did not use external user-service,
  message-service, WebSocket, or mail endpoints.

Skipped selectors in the failed broad run:

- 12 Group E2EE enabled-mode selectors remained skipped because the broad run
  used `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`.
- 1 local topology selector remained skipped:
  `tests_v2/message_service/test_direct_local.py`.
- 3 multi-tenant admission selectors remained skipped because DID-only or
  message-only tenant environment/data gates were not configured.

Coverage boundary:

- This fix keeps attachment error-mapping contracts deterministic under slow
  broad/system-test subprocess startup.
- It does not change production message attachment execution or CLI error
  mapping.
- The focused wrapper proves the previously failing domain, and the full broad
  non-mail `tests_v2` rerun after this fixture fix is green.
- Mail selectors remain deferred and were not run or counted.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for
runtime portability. TLS remains Rustls-first; no OpenSSL, `native-tls`,
bundled OpenSSL, platform service, ANP SDK, or SQLite backend change was
introduced.
