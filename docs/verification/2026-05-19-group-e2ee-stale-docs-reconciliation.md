# 2026-05-19 Group E2EE Stale Docs Reconciliation

Timestamp: 2026-05-19T00:00:00+0800.

Scope: reconcile stale documentation after an accelerated `message/group`
E2EE module-batch scan confirmed that Go group E2EE/MLS execution is already
translated in Rust and exposed through focused non-mail selectors. This batch
does not change production Rust code, does not add dependencies, does not run
or count mail selectors, and does not claim new live real-service acceptance.

Pipeline:

- Followed the accelerated module-batch pipeline for the group E2EE docs
  correction instead of treating each stale row as an isolated diff.
- Read-only Native Agents mapped Go group E2EE source behavior, current split
  Rust group E2EE implementation/tests, and the non-mail system-test selector
  surface in parallel.
- The scan found no production Rust gap in the scoped group E2EE command,
  provider, transport, or hidden wire surfaces. The actionable issue was stale
  `docs/known-go-issues.md` text that still described group E2EE as dry-run or
  request-builder-only.
- The leader updated only documentation and preserved the existing acceptance
  boundary: focused Rust-only selectors are deterministic contract evidence,
  live OpenMLS selectors remain separate evidence, foreground listener group
  E2EE handling remains a future acceptance slice, and mail selectors remain
  deferred.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/group_e2ee.go`, `internal/message/group_e2ee_service.go` | Hidden/focused `group e2ee ...` command surface for status, pending, publish-key-package, create, add, send, decrypt, remove, leave/process-leave, recover-member, update-key, repair, and safety boundaries | `crates/awiki-cli/src/message/group_e2ee_{add,create,decrypt,pending,provider,publish,recover,remove,repair,send,status,transport,update,wire}.rs`, `crates/awiki-cli/src/app/group_e2ee_handlers.rs` | implemented in split Rust modules | focused `group_e2ee_*_contract` targets already pass through selector wrappers | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::{test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts,test_awiki_cli_group_e2ee_status_pending_publish_rust_contracts,test_awiki_cli_group_e2ee_create_add_repair_decrypt_rust_contracts}` | low for docs correction; live service and listener edges remain separate |
| `internal/message/group_e2ee_provider.go` | Run local `anp-mls` provider commands and keep private provider state out of public service payloads | `crates/awiki-cli/src/message/group_e2ee_provider.rs` | implemented with local ANP Rust SDK binary boundary | covered by create/add/send/status/publish/recover/update/repair/decrypt contracts | same wrapper selectors | low; this batch did not change provider code |
| `internal/message/group_wire.go`, `internal/message/group_e2ee_service.go` | Hidden P6 HTTP RPCs and selector/security-profile wire shapes for group E2EE control-plane calls | `crates/awiki-cli/src/message/group_e2ee_wire.rs`, `crates/awiki-cli/src/message/group_e2ee_transport.rs` | implemented beyond request construction | `message_group_e2ee_wire_contract`, `group_e2ee_send_contract`, status/pending/publish contracts | same wrapper selectors plus earlier live group E2EE local selectors | low; WebSocket/local bridge hidden E2EE remains future/new behavior rather than Go parity |
| `docs/known-go-issues.md` rows for message/group boundaries | Documentation should distinguish implemented hidden P6 HTTP/MLS parity from future WebSocket/local bridge behavior, foreground listener acceptance, and mail-deferred selectors | `docs/known-go-issues.md` | stale text corrected | docs-only structure and whitespace checks | not applicable | low |

Commands and evidence:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_status_pending_publish_rust_contracts tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_create_add_repair_decrypt_rust_contracts -ra -q
```

Observed result: 3 passed, 0 failed, 0 skipped in 182.01s.

The selector evidence is deterministic Rust contract evidence exposed through
`awiki-system-test`, not a new live real-service acceptance claim. Earlier
live local group E2EE selectors remain the live OpenMLS/service evidence for
send/decrypt, lifecycle, recovery, update/rejoin, and negative local flows.

File-size note:

- Split production group E2EE modules remain below the active 2500-line source
  target. Current `group_e2ee_*.rs` source files range from 175 to 856 lines.
- Focused group E2EE contract test files range from 404 to 1165 lines, below
  the active 3000-line test target.
- No file-size exception is needed for this docs-only batch. The repository's
  active limits are 2500 lines for source files and 3000 lines for test files,
  with documented exceptions allowed when traceable translation requires them.

Dependency note:

- No dependency was added or changed.
- The port continues to use the local ANP Rust SDK / `anp-mls` boundary for
  group MLS provider behavior.
- TLS remains Rustls-first. This batch does not add OpenSSL, `native-tls`, or
  bundled OpenSSL.
- SQLite remains on the approved `rusqlite + bundled` path for runtime
  compatibility. No alternative SQLite backend was introduced.

Coverage boundary:

- This batch corrects stale docs that implied group E2EE execution was still
  local-plan-only or request-builder-only.
- It does not implement new behavior, does not run full repository-wide
  acceptance, does not claim foreground listener group E2EE acceptance, and
  does not claim WebSocket/local bridge hidden group E2EE support.
- WebSocket/local bridge hidden group E2EE remains future/new behavior because
  the Go bridge exposes ordinary `group.send` and `group.list_messages`, not
  hidden `group.e2ee.*` methods.
- Mail selectors remain deferred and were not counted.
