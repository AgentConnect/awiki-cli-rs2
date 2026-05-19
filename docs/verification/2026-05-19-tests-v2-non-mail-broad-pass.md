# 2026-05-19 tests_v2 Non-Mail Broad Pass

Timestamp: 2026-05-19T09:54:00+0800.

Scope: prioritize failures found by `awiki-system-test/tests_v2` against the
Rust CLI, with mail selectors deferred. This report records one deterministic
Rust CLI fix from the secure direct handle-cache path plus the broad non-mail
system-test evidence after aligning the local `message-service` flag-off
runtime configuration.

Pipeline:

- Continued the accelerated system-test-first lane.
- Used a read-only Native Agent to map Go/Rust secure prekey publish behavior
  while the leader inspected runtime evidence and reran focused selectors.
- Treated mail selectors as deferred and did not count them as passed.
- Did not modify `awiki-system-test` helper dirty files.
- Did not add dependencies or change Cargo manifests/lockfiles.

Rust CLI change covered:

- `crates/awiki-cli/src/message/service.rs`
  - `merge_handle_history_messages` no longer replaces already processed
    remote secure direct rows with local handle-cache rows when the remote rows
    contain decrypted, undecryptable, or secure-control direct E2EE state.
  - Pending raw direct E2EE wire cache rows remain non-preferred after display
    filtering.
- `crates/awiki-cli/src/message/service/tests.rs`
  - Added a focused regression proving cache rows are not preferred over an
    already decrypted secure remote row, while ordinary cache expansion remains
    allowed.

System-test commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_secure_direct_handle_queries_hide_raw_wire_cache_and_mark_read
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_can_send_secure_direct_messages_with_manual_reply_confirmation
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -x -ra -q --ignore=tests_v2/mail tests_v2
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/message_service/test_group_e2ee_flag_off.py::test_group_e2ee_methods_are_rejected_when_contract_flag_is_off
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -x -ra -q --ignore=tests_v2/mail tests_v2
```

Rust verification commands for the CLI fix:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli message::service::tests::direct_cache_is_not_preferred_over_processed_secure_remote_like_go --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Focused secure direct handle-cache selector: `1 passed in 5.77s`.
- Focused secure direct manual-reply selector after an earlier transient
  prekey failure: `1 passed in 3.99s`.
- First broad non-mail `tests_v2` run after the secure direct fix stopped at
  `tests_v2/message_service/test_group_e2ee_flag_off.py::test_group_e2ee_methods_are_rejected_when_contract_flag_is_off`
  with `128 passed, 13 skipped, 1 failed in 549.78s`.
- The flag-off failure root cause was local service configuration, not Rust CLI
  behavior: the pytest run used `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`, but the
  running `message-service` was started from
  `/home/ecs-user/awiki-space/message-service/message-service.toml` with
  `security.group_e2ee_contract_test_enabled = true`. With the hidden P6
  service path enabled, `group.e2ee.publish_key_package` reached request-shape
  validation and returned `anp.invalid_params_shape`; the flag-off selector
  correctly expected `anp.not_supported`.
- Local runtime config was set back to
  `security.group_e2ee_contract_test_enabled = false`, and `message-service`
  was restarted. Health probes after restart: `systemctl is-active
  message-service` returned `active`, `https://awiki.info/user-service/health`
  returned HTTP 200, `https://awiki.info/im/rpc` returned HTTP 405 on GET, and
  `https://awiki.info/anp-im/rpc` returned HTTP 405 on GET.
- Focused flag-off selector after the service config alignment:
  `1 passed in 0.61s`.
- Final broad non-mail `tests_v2` run:
  `173 passed, 16 skipped, 0 failed in 618.98s (0:10:18)`.
- Rust focused unit and package checks passed:
  `cargo +1.79.0 fmt --check`, focused `awiki-cli` unit test,
  `cargo +1.79.0 check -p awiki-cli --locked`,
  `cargo +1.79.0 run --bin xtask --locked -- check-structure`, and
  `git diff --check`.
- `xtask check-structure` reported no undocumented Rust files over 1200 lines.

Final broad system-test configuration context:

- Test mode: Rust CLI under test through `awiki-system-test/tests_v2`.
- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`.
- `AWIKI_ENABLE_MAIL_TESTS=0`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Mail path ignored with `--ignore=tests_v2/mail`.
- User-service URL: `https://awiki.info/user-service/health` returned HTTP 200.
- Message-service URL: `https://awiki.info/im/rpc` returned HTTP 405 on GET,
  which is the expected method guard for a JSON-RPC POST endpoint.
- ANP public message-service URL: `https://awiki.info/anp-im/rpc` returned HTTP
  405 on GET, which is the expected method guard for a JSON-RPC POST endpoint.
- DID domain/service DID used by the system-test topology:
  `awiki.info` / `did:wba:awiki.info`.
- WebSocket URL used by topology: `wss://awiki.info/im/ws`.

Skipped selectors in the final broad run:

- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:84`: group E2EE
  PR-A lifecycle APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:178`: group E2EE
  PR-A lifecycle APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:311`: group E2EE
  PR-A lifecycle APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_local.py:69`: group E2EE P6 APIs
  stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:95`: group E2EE
  P6 APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:438`: group E2EE
  P6 APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:488`: group E2EE
  P6 APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:83`: group E2EE
  PR-B2 recovery APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:263`: group E2EE
  PR-B2 recovery APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:430`: group E2EE
  PR-B2 recovery APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_update_rejoin_local.py:96`: group
  E2EE PR-C update/rejoin APIs stay hidden unless explicitly enabled.
- `tests_v2/cli/test_awiki_cli_group_e2ee_update_rejoin_local.py:202`: group
  E2EE PR-C update/rejoin APIs stay hidden unless explicitly enabled.
- `tests_v2/message_service/test_direct_local.py:324`: requires the local
  tests_v2 topology.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:92`: requires
  `E2E_MESSAGE_V2_DID_ONLY_DOMAIN`.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:132`: requires
  `E2E_MESSAGE_V2_DID_ONLY_DOMAIN`.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:191`: requires
  `E2E_MESSAGE_V2_MESSAGE_ONLY_DID`.

Failure summary:

- Final broad run failures: 0.
- Final broad run skips: 16.
- Mail selectors: deferred by request and excluded from this pass.

Deferred or observed issues:

- One earlier full-run attempt failed in
  `test_awiki_cli_can_send_secure_direct_messages_with_manual_reply_confirmation`
  with message-service error `1404: no available prekey bundle for target DID`.
  A focused rerun of the same selector passed without code changes, and the
  Rust/Go path comparison found that both implementations publish secure
  prekeys before direct reads while downgrading publish failures to warnings.
  Record as a timing/service-state observation rather than a blocking Rust CLI
  gap unless it recurs.
- Hidden group E2EE enabled-mode selectors were intentionally skipped in this
  broad pass because the run was configured with
  `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`. They remain covered only by focused
  enabled-mode passes, not by this flag-off broad run.
- Mail selectors remain deferred and were not tested or counted as passed.

Dependency note: no dependency was added or changed. SQLite remains on the
approved `rusqlite + bundled` path. TLS remains Rustls-first; no OpenSSL,
`native-tls`, bundled OpenSSL, ANP SDK, platform service, or SQLite backend
change was introduced.
