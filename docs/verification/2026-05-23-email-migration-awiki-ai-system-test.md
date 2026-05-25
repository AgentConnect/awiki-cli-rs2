# 2026-05-23 Email Migration awiki.ai System-Test Record

## Scope

This record covers the Email migration from `awiki-cli` legacy mail code to
`im-core::email`, including the Rust CLI adapter and Dart/Flutter package
facade. The system-test requirement is the Email selector in
`/home/ecs-user/awiki-space/awiki-system-test` using the `awiki.ai` domain.

## Local Verification

Passed:

```bash
cargo fmt
cargo check -p awiki-cli --locked
cargo test -p awiki-cli --test mail_contract --test mail_live_contract --test cli_cutover_command_surface_contract --test command_catalog_schema_contract --test m_core_cli_adapter_policy_contract --locked
cargo test -p im-core --test email_wire_contract --test email_notification_contract --locked
cargo test -p im-core --locked
cargo check -p im-core-dart --locked
cargo test -p im-core-dart --test facade_contract --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && /home/ecs-user/develop/flutter/bin/flutter test test/awiki_im_core_stub_test.dart
git diff --check
```

Notes:

- `scripts/flutter/codegen-check.sh` emits the known FRB intermediate
  `#[unsafe(no_mangle)]` rustfmt warning, then the repo post-processing script
  rewrites and formats the generated Rust file. The final command exited 0.
- `im-core` still emits pre-existing local-state dead-code warnings unrelated
  to Email migration.
- Boundary greps returned no matches for CLI types or raw/output request fields
  in the Email SDK implementation:

```bash
rg -n "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|crate::app|crate::cli|crate::output|awiki_cli" crates/im-core/src crates/im-core/tests
rg -n "identity_name|output|serde_json::Value|Resolved|Manager" crates/im-core/src/email crates/im-core/src/internal/email_runtime crates/im-core/src/internal/email_wire crates/im-core/src/internal/local_state/email.rs
```

## awiki.ai System Test

Primary command after resolving the live mail-service deployment:

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.ai \
E2E_USER_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_USER_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_A_DOMAIN=awiki.ai \
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_A_RPC_URL=https://awiki.ai/im/rpc \
E2E_MESSAGE_V2_NODE_A_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_NODE_A_SERVICE_DID=did:wba:awiki.ai \
E2E_MESSAGE_V2_NODE_B_DOMAIN=awiki.ai \
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_B_RPC_URL=https://awiki.ai/im/rpc \
E2E_MESSAGE_V2_NODE_B_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_NODE_B_SERVICE_DID=did:wba:awiki.ai \
E2E_MAIL_DOMAIN=awiki.ai \
E2E_MAIL_SERVICE_URL=https://awiki.ai \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/mail-cli-rs2 \
AWIKI_ENABLE_MAIL_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider -ra -q tests_v2/mail
```

Result:

```text
1 failed, 4 skipped in 15.69s
```

Detailed result:

| Outcome | Count | Test(s) | Reason |
| --- | ---: | --- | --- |
| Failed | 1 | `tests_v2/mail/test_awiki_cli_mail_notification_local.py::test_awiki_cli_mail_notification_flow_local` | `POST /mail/internal/create-account` returned HTTP 403, reported by the fixture as `awiki-mail-service MAIL_INTERNAL_SECRET mismatch`. |
| Skipped | 4 | `tests_v2/mail/test_awiki_cli_mail_local.py::*` | Each test skipped after `POST /mail/internal/create-account` returned HTTP 403 due mail internal secret mismatch. |

Configuration context:

```text
AWIKI_SYSTEM_TEST_MODE=remote
did_domain=awiki.ai
user_service_url=https://awiki.ai
message_service_url=https://awiki.ai
message_service_ws_url=wss://awiki.ai/im/ws
anp_service_endpoint=https://awiki.ai/anp-im/rpc
anp_service_did=did:wba:awiki.ai
mail_service_url=https://awiki.ai
mail_domain=awiki.ai
CLI under test=rust
```

Live endpoint probes:

```text
https://mail.awiki.ai/mail/health -> HTTP 404
https://mail.awiki.ai/health -> HTTP 404
https://awiki.ai/mail/health -> HTTP 200 {"status":"healthy","service":"awiki-mail-service"}
https://awiki.ai/healthz -> HTTP 200
https://awiki.ai/im/healthz -> HTTP 200
```

Interpretation:

- The live awiki.ai mail-service is deployed under the main domain
  `https://awiki.ai`, not `https://mail.awiki.ai`, so the system-test command
  used `E2E_MAIL_SERVICE_URL=https://awiki.ai`.
- The Email selector reached both awiki.ai mail-service health and awiki.ai
  message-service health. The remaining blocker is test-fixture permission:
  the available `E2E_MAIL_INTERNAL_SECRET` does not authorize
  `/mail/internal/create-account` on awiki.ai.
- This record does not claim the awiki.ai Email system test passed. It records
  the exact external blocker and the successful local Rust/Dart verification.

### 2026-05-24 Re-run

The awiki.ai Email selector was re-run on 2026-05-24 to verify whether the
mail internal secret blocker had changed. The command intentionally did not
print the secret; `E2E_MAIL_INTERNAL_SECRET` was loaded from the
`awiki-system-test/.env` project env file.

Command:

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.ai \
E2E_USER_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_USER_SERVICE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_A_DOMAIN=awiki.ai \
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_A_RPC_URL=https://awiki.ai/im/rpc \
E2E_MESSAGE_V2_NODE_A_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_NODE_A_SERVICE_DID=did:wba:awiki.ai \
E2E_MESSAGE_V2_NODE_B_DOMAIN=awiki.ai \
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=https://awiki.ai \
E2E_MESSAGE_V2_NODE_B_RPC_URL=https://awiki.ai/im/rpc \
E2E_MESSAGE_V2_NODE_B_WS_URL=wss://awiki.ai/im/ws \
E2E_MESSAGE_V2_NODE_B_SERVICE_DID=did:wba:awiki.ai \
E2E_MAIL_DOMAIN=awiki.ai \
E2E_MAIL_SERVICE_URL=https://awiki.ai \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/mail-cli-rs2 \
AWIKI_ENABLE_MAIL_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider -ra -q tests_v2/mail
```

Result:

```text
1 failed, 4 skipped in 26.44s
```

Detailed result:

| Outcome | Count | Test(s) | Reason |
| --- | ---: | --- | --- |
| Failed | 1 | `tests_v2/mail/test_awiki_cli_mail_notification_local.py::test_awiki_cli_mail_notification_flow_local` | `POST /mail/internal/create-account` returned HTTP 403, reported by the fixture as `awiki-mail-service MAIL_INTERNAL_SECRET mismatch`. |
| Skipped | 4 | `tests_v2/mail/test_awiki_cli_mail_local.py::*` | Each test skipped after `POST /mail/internal/create-account` returned HTTP 403 due mail internal secret mismatch. |

Configuration context:

```text
AWIKI_SYSTEM_TEST_MODE=remote
did_domain=awiki.ai
user_service_url=https://awiki.ai
message_service_url=https://awiki.ai
message_service_ws_url=wss://awiki.ai/im/ws
message_v2_node_a_public_base_url=https://awiki.ai
message_v2_node_a_rpc_url=https://awiki.ai/im/rpc
message_v2_node_a_ws_url=wss://awiki.ai/im/ws
message_v2_node_b_public_base_url=https://awiki.ai
message_v2_node_b_rpc_url=https://awiki.ai/im/rpc
message_v2_node_b_ws_url=wss://awiki.ai/im/ws
mail_service_url=https://awiki.ai
mail_domain=awiki.ai
CLI under test=rust
mail internal secret source=awiki-system-test/.env
```

Endpoint probes on 2026-05-24:

```text
https://mail.awiki.ai/mail/health -> HTTP 404
https://mail.awiki.ai/health -> HTTP 404
https://awiki.ai/mail/health -> HTTP 200 {"status":"healthy","service":"awiki-mail-service"}
https://awiki.ai/healthz -> HTTP 200
https://awiki.ai/im/healthz -> HTTP 200
```

Conclusion:

- The external blocker is still present on 2026-05-24.
- The live awiki.ai mail-service remains reachable through
  `https://awiki.ai/mail/*`.
- The available project env secret still does not authorize
  `/mail/internal/create-account` on awiki.ai, so the full awiki.ai Email
  system-test gate cannot pass until the correct secret or test permission is
  provided.

### 2026-05-24 Re-run After Remote Mail Restart

After the remote mail server was restarted, the local
`awiki-system-test/.env` domain configuration was changed to remote
`awiki.ai` values while keeping the existing `E2E_MAIL_INTERNAL_SECRET`
unchanged and unprinted.

Updated local `.env` context:

```text
AWIKI_SYSTEM_TEST_MODE=remote
E2E_DID_DOMAIN=awiki.ai
E2E_USER_SERVICE_URL=https://awiki.ai
E2E_MESSAGE_SERVICE_URL=https://awiki.ai
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.ai/im/ws
E2E_MESSAGE_V2_USER_SERVICE_URL=https://awiki.ai
E2E_MESSAGE_V2_NODE_A_DOMAIN=awiki.ai
E2E_MESSAGE_V2_NODE_A_PUBLIC_BASE_URL=https://awiki.ai
E2E_MESSAGE_V2_NODE_A_RPC_URL=https://awiki.ai/im/rpc
E2E_MESSAGE_V2_NODE_A_WS_URL=wss://awiki.ai/im/ws
E2E_MESSAGE_V2_NODE_A_SERVICE_DID=did:wba:awiki.ai
E2E_MESSAGE_V2_NODE_B_DOMAIN=awiki.ai
E2E_MESSAGE_V2_NODE_B_PUBLIC_BASE_URL=https://awiki.ai
E2E_MESSAGE_V2_NODE_B_RPC_URL=https://awiki.ai/im/rpc
E2E_MESSAGE_V2_NODE_B_WS_URL=wss://awiki.ai/im/ws
E2E_MESSAGE_V2_NODE_B_SERVICE_DID=did:wba:awiki.ai
E2E_MAIL_DOMAIN=awiki.ai
E2E_MAIL_SERVICE_URL=https://awiki.ai
E2E_MAIL_INTERNAL_SECRET=<present, not printed>
```

Endpoint probes before the run:

```text
https://awiki.ai/mail/health -> HTTP 200 {"status":"healthy","service":"awiki-mail-service"}
https://awiki.ai/healthz -> HTTP 200
https://awiki.ai/im/healthz -> HTTP 200
```

Command:

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/mail-cli-rs2 \
AWIKI_ENABLE_MAIL_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider -ra -q tests_v2/mail
```

Result:

```text
1 failed, 4 skipped in 14.79s
```

Detailed result:

| Outcome | Count | Test(s) | Reason |
| --- | ---: | --- | --- |
| Failed | 1 | `tests_v2/mail/test_awiki_cli_mail_notification_local.py::test_awiki_cli_mail_notification_flow_local` | `POST /mail/internal/create-account` still returned HTTP 403, reported by the fixture as `awiki-mail-service MAIL_INTERNAL_SECRET mismatch`. |
| Skipped | 4 | `tests_v2/mail/test_awiki_cli_mail_local.py::*` | Each test skipped after `POST /mail/internal/create-account` returned HTTP 403 due mail internal secret mismatch. |

Conclusion:

- The domain and remote mode configuration are now corrected to `awiki.ai` in
  the local system-test `.env`.
- The restarted remote mail server is healthy at `https://awiki.ai/mail/health`.
- The remaining failure is still the mail internal secret mismatch for
  `/mail/internal/create-account`.

## Migration Evidence

- `client.email()` is present and exported through the Rust SDK.
- Remote `account`, `inbox`, `read`, `mark_read`, `send`, and
  `download_attachment` execute through `im-core::email`.
- `mail notify` uses `im-core` local state owner-scoped Email notification
  queries.
- CLI `mail.*` is on the default im-core command surface and no longer uses the
  legacy mail service path.
- CLI dry-run remains local and does not write attachment files.
- Attachment download decodes bytes in SDK and writes files only in the CLI.
- Legacy `awiki_cli::mail` is `pub(crate)` with an Email E7 cleanup TODO; the
  old `awiki-cli` `mail_wire_contract` selector has moved to
  `im-core` `email_wire_contract`.
- Dart bridge and `packages/awiki_im_core` expose Email through
  `AwikiImClient.email` and `EmailApi`.
