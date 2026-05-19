# 2026-05-19 tests_v2 Non-Mail Broad Pass After Config Selector

Timestamp: 2026-05-19T23:05:00+0800.

Scope: run one broad non-mail `awiki-system-test/tests_v2` pass after the
config-set legacy OpenClaw identity plus legacy SQLite selector and the
corresponding Rust evidence docs were committed and pushed. This run follows
the user's latest priority: prefer problems found by `tests_v2`, fix main-flow
failures first, and record non-main-flow or gated issues separately.

Command run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_GROUP_E2EE_CONTRACT_TEST=0 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q --ignore=tests_v2/mail tests_v2
```

Observed result:

- Overall: 175 passed, 0 failed, and 16 skipped in 778.04s (0:12:58).
- Failures: 0.
- Main-flow system-test blockers found: 0.
- Mail selectors: deferred by request, explicitly disabled with
  `AWIKI_ENABLE_MAIL_TESTS=0`, and excluded with `--ignore=tests_v2/mail`.

System-test configuration context:

- Test mode from `.env`: `AWIKI_SYSTEM_TEST_MODE=remote`.
- Rust CLI under test: `AWIKI_CLI_UNDER_TEST=rust`.
- Rust repository: `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- Update check mode: `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- Hidden Group E2EE broad-run gate: `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`.
- Mail gate: `AWIKI_ENABLE_MAIL_TESTS=0`.
- Python bytecode disabled: `PYTHONDONTWRITEBYTECODE=1`.
- User-service URL from `.env`: `https://awiki.info`; health probe returned
  HTTP 200 for `https://awiki.info/user-service/health`.
- Message-service URL from `.env`: `https://awiki.info`; GET probe returned
  HTTP 405 for `https://awiki.info/im/rpc`, which is the expected method guard
  for a JSON-RPC POST endpoint.
- ANP public message-service URL: `https://awiki.info/anp-im/rpc`; GET probe
  returned HTTP 405, which is the expected method guard for a JSON-RPC POST
  endpoint.
- WebSocket URL from `.env`: `wss://awiki.info/im/ws`.
- DID domain/service DID used by the remote topology: `awiki.info` /
  `did:wba:awiki.info`.
- Local `message-service` process state before the run: `systemctl is-active
  message-service` returned `active`.

Skipped selectors:

- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:84`: group E2EE
  PR-A lifecycle APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:178`: group E2EE
  PR-A lifecycle APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_lifecycle_local.py:311`: group E2EE
  PR-A lifecycle APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_local.py:69`: group E2EE P6 APIs
  stay hidden unless a focused target explicitly enables them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:95`: group E2EE
  P6 APIs stay hidden unless a focused target explicitly enables them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:438`: group E2EE
  P6 APIs stay hidden unless a focused target explicitly enables them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_negative_local.py:488`: group E2EE
  P6 APIs stay hidden unless a focused target explicitly enables them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:83`: group E2EE
  PR-B2 recovery APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:263`: group E2EE
  PR-B2 recovery APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_recovery_local.py:430`: group E2EE
  PR-B2 recovery APIs stay hidden unless a focused target explicitly enables
  them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_update_rejoin_local.py:96`: group
  E2EE PR-C update/rejoin APIs stay hidden unless a focused target explicitly
  enables them.
- `tests_v2/cli/test_awiki_cli_group_e2ee_update_rejoin_local.py:202`: group
  E2EE PR-C update/rejoin APIs stay hidden unless a focused target explicitly
  enables them.
- `tests_v2/message_service/test_direct_local.py:324`: this test requires the
  local tests_v2 topology.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:92`: set
  `E2E_MESSAGE_V2_DID_ONLY_DOMAIN` to run DID-only tenant admission coverage.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:132`: set
  `E2E_MESSAGE_V2_DID_ONLY_DOMAIN` to run DID-only message-service denial
  coverage.
- `tests_v2/multi_tenant/test_message_tenant_admission.py:191`: set
  `E2E_MESSAGE_V2_MESSAGE_ONLY_DID` to run message-only admission coverage.

Failure summary:

- Failures: 0.
- Failures by domain: none.
- No Rust production fix was needed after this broad pass.

Deferred or gated issues:

- Mail selectors remain deferred and were not run or counted.
- Hidden Group E2EE enabled-mode selectors remain skipped in this normal
  flag-off broad run; they are covered only by focused enabled-mode passes.
- One direct local topology selector remains skipped because this run used the
  remote topology.
- Three multi-tenant selectors remain gated on additional DID-only or
  message-only tenant environment/data values.

Dependency note: no dependency was added or changed. Cargo manifests and
lockfile remain unchanged. SQLite remains on the approved `rusqlite + bundled`
path. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled OpenSSL, ANP
SDK, platform service, or SQLite backend change was introduced.
