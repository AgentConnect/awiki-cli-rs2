# 2026-05-19 tests_v2 Remote System-Test Audit

Scope: run a broad non-mail `awiki-system-test/tests_v2` pass against the Rust
CLI, classify the failures, and separate actionable Rust CLI regressions from
remote service or local prerequisite blockers.

## Configuration Context

- Rust CLI under test:
  `/home/ecs-user/awiki-space/awiki-cli-rs2`
- System-test repo:
  `/home/ecs-user/awiki-space/awiki-system-test`
- `AWIKI_SYSTEM_TEST_MODE`: `remote` from `.env`.
- User-service URL: `https://awiki.info`.
- Node A domain: `awiki.info`.
- Node A public URL: `https://awiki.info`.
- Node A RPC URL: `https://awiki.info/im/rpc`.
- Node A WebSocket URL: `wss://awiki.info/im/ws`.
- DID domain: `awiki.info`.
- Mail selectors: deferred; `tests_v2/mail` was ignored and mail results were
  not counted as passed.

The system-test `.env` currently enables hidden Group E2EE focused acceptance:

```text
AWIKI_GROUP_E2EE_CONTRACT_TEST=1
AWIKI_ANP_MLS_BINARY=/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls
AWIKI_ENABLE_MAIL_TESTS=1
```

That configuration is useful for focused local Group E2EE work, but it is not
the intended broad non-mail selector baseline while the configured `anp-mls`
binary is absent and mail selectors are deferred.

## Commands And Results

Initial broad non-mail run, using the current `.env` gates:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider tests_v2 --ignore=tests_v2/mail -ra -q
```

Observed result:

```text
35 failed, 139 passed, 5 skipped, 10 errors in 740.44s
```

The 10 errors were all Group E2EE focused selectors failing during fixture setup
because `AWIKI_ANP_MLS_BINARY` pointed at a non-executable or missing binary:

```text
/home/ecs-user/awiki-space/anp/anp/rust/target/debug/anp-mls
```

Attempted local prerequisite build:

```text
cd /home/ecs-user/awiki-space/anp/anp/rust
cargo +1.79.0 build --bin anp-mls --locked
```

Observed result: the build did not reach Rust compilation. Cargo dependency
fetching was blocked by the configured USTC crates.io mirror, first with SSL
connect errors to `crates-io.proxy.ustclug.org`, and then with HTTP 429 rate
limiting from `mirrors.ustc.edu.cn`. No dependency, source, or lockfile was
changed.

Direct endpoint probe:

```text
curl -k -sS -o /tmp/awiki-user-health.out -w 'user-health %{http_code} %{time_total}\n' https://awiki.info/user-service/health
curl -k -sS -o /tmp/awiki-im-rpc.out -w 'im-rpc %{http_code} %{time_total}\n' https://awiki.info/im/rpc
curl -k -sS -o /tmp/awiki-anp-im-rpc.out -w 'anp-im-rpc %{http_code} %{time_total}\n' https://awiki.info/anp-im/rpc
```

Observed result:

```text
user-health 200 0.018687
im-rpc 502 0.015847
anp-im-rpc 502 0.019704
```

Focused local debug DB selectors:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_migrates_legacy_config_json_before_opening_store \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_imports_seeded_legacy_sqlite_without_message_service \
  tests_v2/debug/test_debug_cli.py::test_debug_db_handle_history_reads_contact_bindings \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  -ra -q
```

Observed result:

```text
5 passed in 4.98s
```

Focused runtime/listener local Rust contract selectors:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_batch1_non_mail_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_local_bridge_deterministic_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_lifecycle_reader_error_shutdown_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_service_foreground_status_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_contact_notification_lookup_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_message_direct_ws_local_cache_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_message_group_ws_local_cache_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_session_secure_replay_host_notify_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_session_local_queue_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_outbox_local_queue_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_foreground_signal_cli_contracts \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_local_identity_reports_registration_error \
  -ra -q
```

Observed result:

```text
12 passed in 108.69s
```

Second broad non-mail run, with hidden Group E2EE and mail gates explicitly
disabled for the broad baseline:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_GROUP_E2EE_CONTRACT_TEST=0 \
AWIKI_ENABLE_MAIL_TESTS=0 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider tests_v2 --ignore=tests_v2/mail -ra -q
```

Observed result:

```text
34 failed, 139 passed, 16 skipped in 537.52s
```

The second run had zero setup errors. The additional skips are the expected
hidden Group E2EE focused selectors plus existing topology/env-gated selectors.

## Failure Classification

All remaining 34 failures in the second broad run depend on the remote
message-service RPC or WebSocket path that was returning HTTP 502 at test time.
Representative CLI stderr:

```text
service http error 502: <html>
<head><title>502 Bad Gateway</title></head>
...
nginx/1.18.0 (Ubuntu)
```

Failure domains:

| Domain | Failed tests | Classification |
| --- | ---: | --- |
| CLI direct live send/inbox/attachments | 5 | remote `https://awiki.info/im/rpc` 502 |
| CLI group live create/add/send/attachments | 4 | remote `https://awiki.info/im/rpc` 502 |
| Host-notify and runtime listener live probes | 7 | remote RPC/WS 502 before listener side effects can be judged |
| Secure direct init/repair/retry live selectors | 3 | remote `https://awiki.info/im/rpc` 502 |
| Debug DB live tests | 2 | initial real `msg send` blocked by remote 502; local debug DB selectors passed |
| Message-service protocol tests | 13 | remote `/im/rpc`, `/anp-im/rpc`, or WebSocket returned 502 |

Skipped domains in the second run:

| Domain | Skipped tests | Reason |
| --- | ---: | --- |
| Hidden Group E2EE focused system tests | 12 | `AWIKI_GROUP_E2EE_CONTRACT_TEST=0` broad baseline; avoids missing `anp-mls` prerequisite |
| Message-service direct local topology | 1 | existing local-topology gate |
| Multi-tenant admission variants | 3 | existing env gates for DID-only/message-only tenant coverage |

## Conclusion

This system-test batch found no actionable Rust CLI code change in the
selectors that can run without the unhealthy remote message-service path:

- Local debug DB Rust-under-test selectors passed.
- Runtime/listener local Rust contract selectors passed.
- The first broad run's 10 Group E2EE setup errors were caused by a test
  environment gate and missing `anp-mls` prerequisite, not by `awiki-cli-rs2`
  code.
- The second broad run reduced the setup errors to expected skips by using the
  documented broad-baseline gate `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`.
- The remaining 34 failures are blocked by remote `awiki.info` message-service
  HTTP 502 and WebSocket 502 responses.

No Rust code, dependency, Cargo manifest, lockfile, or system-test helper was
changed in this batch. Mail selectors remain deferred and were not counted as
passed.

## Follow-up Items

- When `https://awiki.info/im/rpc`, `https://awiki.info/anp-im/rpc`, and
  `wss://awiki.info/im/ws` are healthy, rerun the second broad non-mail command
  before making live direct/group/message-service code changes.
- For focused Group E2EE acceptance, first build or provide an executable
  `anp-mls` binary. The current Cargo fetch path is blocked by the configured
  USTC mirror SSL/rate-limit behavior.
- Consider a future `awiki-system-test` harness improvement that probes node-a
  RPC and WebSocket readiness before host-notify/runtime listener live probes,
  so remote 502 outages are reported as environment skips rather than product
  failures. This is a harness improvement note, not part of the current Rust
  port translation batch.
