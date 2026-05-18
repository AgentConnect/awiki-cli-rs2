# 2026-05-18 CLI Parser Required Empty Values Batch

Timestamp: 2026-05-19T00:05:38+0800.

Scope: preserve Go/Cobra required-flag presence semantics for explicit empty
values on non-mail group/site dry-run commands. In Go, `MarkFlagRequired`
checks whether a flag was explicitly provided; `--group=`, `--name=`,
`--domain=`, and `--slug=` therefore satisfy the parser-required boundary and
reach the dry-run handler with an empty string value. This batch does not
implement Cobra help output, `-h`, or a broad parser rewrite.

Pipeline:

- Continued the accelerated module-batch pipeline for the `internal/cli/root.go`
  parser/Cobra-depth cluster.
- Three read-only Native Agents mapped Go Cobra required-flag behavior, current
  Rust group/site/page handling, and non-mail system-test selector exposure in
  parallel.
- Main-thread Go probes confirmed explicit empty required flags are accepted by
  Go dry-run plans for group create/add/get and site root/page commands.
- The Rust gap was localized to group/site handler required helpers that used
  trimmed non-empty values instead of parser presence. Page read/update/rename
  helpers already use presence through `changed_flags`; `page create` keeps its
  handler-level blank `slug`/`title` validation and was not changed.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/root.go`, `internal/cli/group.go` | Cobra-required `group.*` flags count explicit empty values as present and dry-run handlers render empty strings in plans | `crates/awiki-cli/src/app/group_handlers.rs` | implemented by checking `changed_flags` for required helper presence | `group_contract` passed 6 tests | `tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts` | low for dry-run parser boundary; non-dry-run service validation remains semantic |
| `internal/cli/root.go`, `internal/cli/site.go` | Cobra-required `site.*` flags count explicit empty values as present and dry-run handlers render empty strings in plans | `crates/awiki-cli/src/app/site_handlers.rs` | implemented by checking `changed_flags` for required helper presence | `site_contract` passed 6 tests | same Rust-only selector | low for dry-run parser boundary; body-source validation remains handler-owned |
| `internal/cli/page.go` | `page create` has handler-owned semantic blank checks, while page read/update/rename/delete Cobra-required flags are presence-based | existing `crates/awiki-cli/src/app/page_handlers.rs` | unchanged; existing page behavior already separates parser-required and handler-required blank semantics | existing `page_contract` coverage | not changed in this selector | low; explicitly out of this code change |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --dry-run group create --name=
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --dry-run group add --group= --member=
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --dry-run site page get --domain= --slug=
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --dry-run site root set --domain= --markdown body
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli ./internal/cmdmeta -run 'Test.*(Group|Site|Page|Required|Command|Root|Catalog)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_contract --test site_contract --test cli_parser_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_parser_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_parser_rust_contracts.py tests_v2/cli/CLAUDE.md
```

Observed results:

- Go probes returned successful dry-run plans for explicit empty group/site
  required flags, including empty `Name`, `Group`, `Member`, `domain`, and
  `slug` values.
- Focused Go `internal/cli` and `internal/cmdmeta` guard: passed.
- Rust focused contracts: `cli_parser_contract` 2 passed, `group_contract` 6
  passed, and `site_contract` 6 passed.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- System-test wrapper syntax and scoped whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  0.54s.
- File sizes stayed below the default review-size limit:
  `group_handlers.rs` 629 lines, `site_handlers.rs` 453 lines,
  `group_contract.rs` 757 lines, `site_contract.rs` 575 lines,
  `cli_parser_contract.rs` 148 lines, and
  `test_awiki_cli_parser_rust_contracts.py` 78 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts`.
- The selector runs deterministic Rust Cargo contracts only. It does not start
  live mail services and does not count mail selectors.

Coverage boundary:

- This batch covers parser-required explicit empty values on translated group
  and site dry-run commands. It intentionally does not change non-dry-run
  service validation, page-create handler blank validation, content body-source
  validation, help output, `-h`, or broader argument-order behavior.
- The new system-test wrapper exposes `cli_parser_contract`, `group_contract`,
  and `site_contract` to the Rust acceptance surface, but it does not replace
  live group/site service selectors.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The change is std-only and does not affect the approved
`rusqlite + bundled` SQLite path, Rustls-first TLS policy, ANP SDK wiring, or
mail selector deferral.
