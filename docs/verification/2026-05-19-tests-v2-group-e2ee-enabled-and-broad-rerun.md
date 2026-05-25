# 2026-05-19 tests_v2 Group E2EE Enabled and Broad Rerun

Timestamp: 2026-05-19T10:37:00+0800.

Scope: prioritize current `awiki-system-test/tests_v2` acceptance signals
against the Rust CLI. This pass focused on the non-mail Group E2EE selectors
that are skipped in the normal flag-off broad run, then restored flag-off
runtime state and reran the broad non-mail suite.

Pipeline:

- Followed the accelerated system-test-first lane.
- Used a read-only Native Agent to map Group E2EE selector prerequisites,
  smallest focused selectors, failure artifacts, and mail independence.
- Built the local ANP Rust SDK `anp-mls` prerequisite from
  `/home/ecs-user/awiki-space/anp/anp/rust`.
- Temporarily switched the local Cargo registry config from the configured USTC
  mirror to crates.io sparse only for the `anp-mls` build, then restored the
  original Cargo config.
- Temporarily enabled
  `message-service.toml` `security.group_e2ee_contract_test_enabled = true`
  for focused enabled-mode selectors, then restored it to `false` before the
  broad flag-off rerun.
- Did not modify Rust production code, Cargo manifests, lockfiles, ANP SDK
  source, `awiki-system-test` helpers, or mail selectors.

Prerequisite command:

```text
cd /home/ecs-user/awiki-space/anp/anp/rust && CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=6 CARGO_HTTP_TIMEOUT=120 cargo +stable build --manifest-path /home/ecs-user/awiki-space/anp/anp/rust/Cargo.toml --bin anp-mls
```

Prerequisite result:

- `anp-mls` built successfully at
  `/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls`.
- Final executable check:
  `/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls` existed and was
  executable.
- Cargo config was restored to the original USTC mirror configuration:
  `source.crates-io.replace-with = "ustc"`.

Focused enabled-mode system-test commands:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=1 AWIKI_ENABLE_MAIL_TESTS=0 AWIKI_GROUP_E2EE_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls AWIKI_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/cli/test_awiki_cli_group_e2ee_local.py::test_awiki_cli_group_e2ee_alice_bob_real_mls_loop
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=1 AWIKI_ENABLE_MAIL_TESTS=0 AWIKI_GROUP_E2EE_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls AWIKI_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -x -ra -q -s tests_v2/cli/test_awiki_cli_group_e2ee_local.py tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py tests_v2/cli/test_awiki_cli_group_e2ee_update_rejoin_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=1 AWIKI_ENABLE_MAIL_TESTS=0 AWIKI_GROUP_E2EE_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls AWIKI_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -x -ra -q -s tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
```

Focused enabled-mode results:

- Smoke selector
  `tests_v2/cli/test_awiki_cli_group_e2ee_local.py::test_awiki_cli_group_e2ee_alice_bob_real_mls_loop`:
  `1 passed in 5.01s`.
- Enabled local selector batch across
  `test_awiki_cli_group_e2ee_local.py`,
  `test_awiki_cli_group_e2ee_negative_local.py`,
  `test_awiki_cli_group_e2ee_lifecycle_local.py`,
  `test_awiki_cli_group_e2ee_recovery_local.py`, and
  `test_awiki_cli_group_e2ee_update_rejoin_local.py`:
  `12 passed in 36.84s`.
- Rust-only Group E2EE contract wrapper:
  `3 passed in 226.09s (0:03:46)`.
- Focused enabled-mode failures: 0.
- Focused enabled-mode skips: 0.
- Mail selectors: 0 run; mail remains deferred.

Broad flag-off system-test command:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -x -ra -q --ignore=tests_v2/mail tests_v2
```

Broad flag-off result:

- `173 passed, 16 skipped, 0 failed in 667.65s (0:11:07)`.
- Mail selectors were excluded with `--ignore=tests_v2/mail` and were not
  counted as passed.

Final broad system-test configuration context:

- Test mode: Rust CLI under test through `awiki-system-test/tests_v2`.
- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_GROUP_E2EE_CONTRACT_TEST=0` for the broad rerun.
- `AWIKI_ENABLE_MAIL_TESTS=0`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Mail path ignored with `--ignore=tests_v2/mail`.
- User-service health probe before the broad run:
  `https://awiki.info/user-service/health` returned HTTP 200.
- Message-service RPC GET probe before the broad run:
  `https://awiki.info/im/rpc` returned HTTP 405, the expected method guard for
  the JSON-RPC POST endpoint.
- ANP message-service RPC GET probe before the broad run:
  `https://awiki.info/anp-im/rpc` returned HTTP 405, the expected method guard
  for the JSON-RPC POST endpoint.
- DID domain/service DID used by the system-test topology:
  `awiki.info` / `did:wba:awiki.info`.
- WebSocket URL used by topology: `wss://awiki.info/im/ws`.
- `message-service` was restored to
  `security.group_e2ee_contract_test_enabled = false` after the focused
  enabled-mode run and was `active` before the broad flag-off rerun.

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

- Focused enabled-mode failures: 0.
- Broad flag-off failures: 0.
- Broad flag-off skips: 16.
- Mail selectors: deferred by request and excluded from this pass.

Deferred or observed issues:

- The broad flag-off run still skips hidden Group E2EE enabled-mode selectors
  by design. This report records a separate focused enabled-mode pass for those
  selectors.
- One local topology selector and three multi-tenant admission selectors remain
  skipped by environment gates in the broad run.
- Mail selectors remain deferred and were not tested or counted as passed.
- No current Rust CLI blocker was found in this system-test pass.

Dependency note: no dependency was added or changed. The `anp-mls` prerequisite
build used the existing ANP Rust SDK checkout. SQLite remains on the approved
`rusqlite + bundled` path. TLS remains Rustls-first; no OpenSSL,
`native-tls`, bundled OpenSSL, platform service crate, or SQLite backend change
was introduced.
