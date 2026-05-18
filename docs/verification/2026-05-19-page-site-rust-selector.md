# 2026-05-19 Page Site Rust Selector Batch

Timestamp: 2026-05-19T01:26:40+0800.

Scope: expose deterministic page/site Rust Cargo contract targets to the
`awiki-system-test` acceptance surface through a new Rust-only selector. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for page/site selector
  visibility.
- Three read-only Native Agents mapped Go page/site CLI and service behavior,
  current Rust page/site contract targets, and `awiki-system-test` selector
  coverage in parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only
  `tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py` and the nearest
  `tests_v2/cli/CLAUDE.md` member-list entry.
- The leader reviewed the scoped diff, ran direct Rust and focused Go guards,
  ran the new focused system selector, and batched docs/evidence.
- Existing dirty helper files in `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.
- `site_contract` was intentionally excluded because it is already exposed by
  `tests_v2/cli/test_awiki_cli_parser_rust_contracts.py`.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/page.go`, `internal/cli/page_test.go` | page schema, dry-run plan envelopes, body-source handling, visibility pass-through, required flag validation, non-dry-run active-identity boundary, legacy config migration before identity lookup | `crates/awiki-cli/src/app/page_handlers.rs`, `src/content/*` | implemented and focused-contract tested | `page_contract` passed 7 tests | `tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py::test_awiki_cli_page_site_rust_contracts` | low; local CLI subprocess only |
| `internal/content/service.go`, `service_test.go` | `/content/rpc` authenticated JSON-RPC payloads, bearer auth, service error mapping, visibility normalization, JWT bootstrap and persistence | `crates/awiki-cli/src/content/*`, `src/authsdk/*`, `src/transportcfg/http.rs` | implemented and fake-service tested | `page_live_contract` passed 4 tests | same Rust-only selector | low; loopback fake HTTP server, not live service |
| `internal/site/service.go`, `service_test.go` | `/site/rpc` builders, domain/slug validation, summaries, action/result shapes, endpoint/method/profile params | `crates/awiki-cli/src/site/wire.rs`, `src/site/types.rs` | implemented and focused-contract tested | `site_wire_contract` passed 4 tests | same Rust-only selector | low; deterministic local wire/helper target |
| `internal/site/service.go`, `internal/cli/site.go`, related tests | authenticated site JSON-RPC execution through `/site/rpc`, domain-normalized payloads, forbidden/business error mapping, JWT bootstrap and persistence | `crates/awiki-cli/src/app/site_handlers.rs`, `src/site/*`, `src/authsdk/*`, `src/transportcfg/http.rs` | implemented and fake-service tested | `site_live_contract` passed 4 tests | same Rust-only selector | low; loopback fake HTTP server, not live service |
| `internal/cli/site.go`, `site_test.go` | site schema and dry-run plan contracts | `crates/awiki-cli/tests/site_contract.rs` | already exposed by parser/Cobra wrapper; not duplicated in this batch | `site_contract` remains covered by parser selector | `tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts` | low; avoid duplicate failure surface |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test page_contract --test page_live_contract --test site_wire_contract --test site_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli ./internal/content ./internal/site -run 'TestPageDryRunPlansRenderStableContracts|TestRunPageCreateValidatesRequiredFlagsBeforeService|TestCreatePageCallsContentRPC|TestDeletePageMapsRPCError|TestNormalizeVisibility|TestUpdatePageRejectsEmptyMutation|TestSiteDryRunPlansRenderStableContracts|TestSiteExitMapsForbiddenRPCCode|TestSiteExitMapsBusinessErrorRPCCode|TestRunSiteRootSetRequiresExplicitBodySource|TestGetRootCallsSiteRPC|TestNormalizeDomainRejectsURLs' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py::test_awiki_cli_page_site_rust_contracts -ra -q
```

Observed results:

- Rust direct page/site contract targets passed 19 tests total:
  `page_contract` 7, `page_live_contract` 4, `site_wire_contract` 4, and
  `site_live_contract` 4.
- Focused Go page/site guards passed for `internal/cli`, `internal/content`,
  and `internal/site`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 19.83s. The selector checks that the four Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the new system-test wrapper is 77 lines, and
  `tests_v2/cli/CLAUDE.md` is 39 lines. Scoped Rust targets are below the
  default 1200-line visibility target: `page_contract.rs` 562,
  `page_live_contract.rs` 452, `site_wire_contract.rs` 368, and
  `site_live_contract.rs` 485. `xtask check-structure` reported no undocumented
  Rust files over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_page_site_rust_contracts.py::test_awiki_cli_page_site_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. The `page_live_contract` and `site_live_contract`
  targets use loopback fake HTTP servers, not external services.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for existing
  page/site Rust contract targets not already exposed elsewhere.
- `site_contract` remains covered by the parser/Cobra Rust selector and is not
  duplicated here.
- This batch does not claim full repository-wide acceptance, live external
  content/site service behavior, live mail-service behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
