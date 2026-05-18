# 2026-05-19 CLI Parser Unknown Global Long Flags Batch

Timestamp: 2026-05-19T00:19:56+0800.

Scope: preserve Go/Cobra's public error boundary for unknown long flags that
appear before a command path, such as `--bogus status`, bare `--bogus`, and
`--format json --bogus status`. This is a non-mail parser slice only. It does
not implement Cobra help rendering, `-h`, short application aliases, or a
broader parser rewrite.

Pipeline:

- Continued the accelerated module-batch pipeline for the `internal/cli/root.go`
  parser/Cobra-depth cluster.
- Two read-only Native Agents mapped Go Cobra behavior, current Rust parser
  behavior/tests, and non-mail system-test selector coverage in parallel.
- The pre-scan found that command-local unknown long flags and unknown
  shorthand flags already had focused coverage from earlier parser batches.
- The actionable gap was leading/root unknown long flags: Go returns
  `internal_error` exit 1 with `unknown flag: --bogus`, while Rust previously
  reported `invalid_argument` exit 2 with `missing command.` because command
  discovery stopped at the leading `--...` token.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/root.go` | Cobra rejects unknown root/global long flags before command handlers run, including before a command path and with no command path | `crates/awiki-cli/src/cli/mod.rs` | implemented for unknown long flags encountered before any command word | `cli_parser_contract::unknown_global_long_flags_fail_like_go_cobra_before_missing_command` | `tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts` | low for the probed root/global unknown-long-flag placements; full Cobra argument-order matrix remains deferred |
| `internal/cli/root.go`, `internal/cmdmeta/catalog.go` | Known persistent globals such as `--format` remain accepted before unknown flag rejection, and command-local unknown flags still validate against selected command metadata | existing `crates/awiki-cli/src/cli/mod.rs` global scan and local validation | preserved | `cli_parser_contract` covers global, local, and shorthand cases | same Rust-only selector | low; this batch intentionally avoided changing local flag parsing |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --bogus status
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli status --format json --bogus
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --bogus
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli group --bogus get
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --format json --bogus status
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli ./internal/cmdmeta -run 'Test.*(Root|Command|Flag|Required|Catalog|Config)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test cli_parser_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_parser_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_parser_rust_contracts.py::test_awiki_cli_parser_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_parser_rust_contracts.py
```

Observed results:

- Go probes returned exit 1 with `error.code=internal_error` and
  `message="unknown flag: --bogus"` for all probed unknown long-flag
  placements, including root/global, command-local, and parent-depth cases.
- Before the fix, Rust already matched `status --format json --bogus` and
  `group --bogus get`, but returned `missing command.` for the three
  root/global placements.
- Rust now returns the same Go/Cobra-shaped unknown flag envelope for
  `--bogus status`, bare `--bogus`, and `--format json --bogus status`.
- Focused Go `internal/cli` and `internal/cmdmeta` guard: passed.
- Rust focused parser contract: 3 passed, 0 failed, 0 ignored.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- System-test wrapper syntax and scoped whitespace checks: passed.
- Focused non-mail Rust parser contract system-test selector: 1 passed,
  0 failed, 0 skipped in 0.75s.
- File sizes stayed below the default source review-size limit:
  `src/cli/mod.rs` 589 lines and `tests/cli_parser_contract.rs` 165 lines.
  The updated `docs/parity-matrix.md` is 1291 lines, below the current
  ordinary 3000-line relaxed limit. The existing verification index is already
  oversized and remains a documentation aggregate file rather than a Rust
  source file.
- Disk remained stable after verification: `/home/ecs-user/awiki-space` had
  26G available and 73% used before the batch edits, so no cleanup was needed
  in this batch.

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

- This batch covers unknown long-flag rejection before any command word. It
  intentionally does not change recognized global flag semantics,
  `id create --identity` local-flag behavior, command-local flag validation,
  help output, `-h`, or every argument-order edge.
- The Rust parser still uses an explicit hand-written command table. Future
  parser work should continue as focused Cobra parity slices unless a separate
  architecture pass redesigns the parser/catalog boundary.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The parser change is std-only and does not affect the approved
`rusqlite + bundled` SQLite path, Rustls-first TLS policy, ANP SDK wiring, or
mail selector deferral.
