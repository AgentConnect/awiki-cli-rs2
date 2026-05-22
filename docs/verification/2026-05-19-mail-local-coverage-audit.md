# 2026-05-19 Mail Local Coverage Audit Batch

Timestamp: 2026-05-19T10:25:00+0800.

Scope: close the final local mail coverage gaps found during the completion
audit without enabling or counting mail system-test selectors. This batch keeps
mail system selectors deferred/gated and does not modify `awiki-system-test`.

Pipeline:

- Followed the accelerated module-batch pipeline for a small mail local coverage
  batch.
- Used three read-only Native Agents in parallel to audit docs constraints,
  remaining system-selector gaps, and Go/Rust mail file mapping before editing.
- Kept writes limited to Rust mail local contract tests and verification docs.
- Did not run, modify, or count `tests_v2/mail` selectors.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/mail/client.go` | `NewClient` rejects a blank mail-service URL with `mail service url is required` | `crates/awiki-cli/src/mail/client.rs`, `crates/awiki-cli/tests/mail_wire_contract.rs` | implemented; direct local assertion added | `mail_wire_contract::mail_client_rejects_empty_mail_service_url_like_go` | mail selectors deferred/gated | low; behavior is local constructor validation |
| `internal/mail/service.go` | local mail notifications accept legacy rows where `content_type = "mail.notification"`, not only current metadata `source_kind = "mail"` rows | `crates/awiki-cli/src/mail/service.rs`, `crates/awiki-cli/tests/mail_contract.rs` | implemented; direct local SQLite assertion added | `mail_contract::mail_notify_accepts_legacy_content_type_rows_like_go` | `tests_v2/mail/test_awiki_cli_mail_notification_local.py::test_awiki_cli_mail_notification_flow_local` deferred/gated | medium for full system acceptance because the live selector requires mail-service, message-service v2, listener flow, and cache delivery |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test mail_contract --test mail_wire_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test mail_contract --test mail_wire_contract --test mail_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/mail ./internal/cli -run 'Test.*Mail|TestNewClientRequiresMailServiceURL|TestServiceSendValidatesRequiredFields|TestServiceAttachmentValidatesIndex' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 tree -p awiki-cli --locked --edges normal | rg -i 'openssl|native-tls' || true
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space && for t in $(cd awiki-cli-rs2 && rg --files crates/awiki-cli/tests | sed 's#^crates/awiki-cli/tests/##; s#\.rs$##' | sort); do if ! rg -q "\b${t}\b" awiki-system-test/tests_v2; then echo "$t"; fi; done
```

Observed results:

- Focused local mail subset passed 11 tests: `mail_contract` 6 and
  `mail_wire_contract` 5.
- Combined local mail validation passed 23 tests: `mail_contract` 6,
  `mail_wire_contract` 5, and `mail_live_contract` 12.
- Focused Go guard passed for `internal/mail` and `internal/cli`.
- Dependency audit for OpenSSL/`native-tls` produced no matches. No dependency
  was added; the mail path continues to use the existing Rustls-backed shared
  transport and the approved `rusqlite + bundled` SQLite path.
- Rust formatting, package check, structure check, and whitespace check passed.
- `xtask check-structure` reported no undocumented Rust files over 1200 lines.
- Final selector gap scan printed only the intentionally deferred mail targets:
  `mail_contract`, `mail_live_contract`, and `mail_wire_contract`.

File-size evidence:

- `crates/awiki-cli/tests/mail_contract.rs`: 536 lines.
- `crates/awiki-cli/tests/mail_wire_contract.rs`: 270 lines.
- `crates/awiki-cli/tests/mail_live_contract.rs`: 855 lines.
- All scoped Rust test files remain below the active 3000-line test target.

System-test configuration context:

- No `awiki-system-test` selector was run in this batch.
- Mail selectors under `tests_v2/mail` remain deferred/gated by design and were
  not counted as passed, skipped, or failed.
- The final non-mail selector exposure scan has no non-mail gaps.

Coverage boundary:

- This batch strengthens local mail parity coverage only.
- It does not claim live `awiki-mail-service` acceptance, live notification
  delivery through message-service/listener, external TLS/CA edge behavior, or
  mail system-test acceptance.
- It does not modify production mail behavior; both newly asserted behaviors
  were already present in Rust implementation.
