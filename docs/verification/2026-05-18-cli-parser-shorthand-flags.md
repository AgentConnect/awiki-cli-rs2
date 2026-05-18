# 2026-05-18 CLI Parser Shorthand Flag Batch

Timestamp: 2026-05-18T23:58:00+0800.

Scope: preserve the Go/Cobra public error boundary for unknown shorthand
flags such as `-v`, `-g`, and `-vh` in the Rust hand-written parser. This is a
non-mail parser slice only. It does not implement Cobra help rendering, `-h`,
short application aliases, or a broader parser rewrite.

Pipeline:

- Followed the accelerated module-batch pipeline for the `internal/cli/root.go`
  parser/Cobra-depth cluster.
- Three read-only Native Agents mapped Go Cobra behavior, existing Rust parser
  behavior/tests, and non-mail system-test selector coverage in parallel.
- The pre-scan found that representative persistent global flag placement,
  including `group --dry-run get --group ...`, already matched Go. That path
  was not changed.
- The actionable gap was pflag/Cobra shorthand rejection: Go returns
  `internal_error` exit 1 with `unknown shorthand flag: '<x>' in -...`, while
  Rust previously treated these tokens as positionals or reached handler-level
  validation.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/root.go` | Cobra/pflag rejects unknown shorthand flags before command handlers run, including root-position and command-local tokens | `crates/awiki-cli/src/cli/mod.rs` | implemented for unknown shorthand tokens other than the help shorthand boundary | `cli_parser_contract::unknown_shorthand_flags_fail_like_go_cobra_before_handler_execution` | `tests_v2/core/test_basic_commands.py::test_unknown_shorthand_flags_fail_like_go_cobra` | low for unknown shorthand rejection; help/Cobra text remains deferred |
| `internal/cli/root.go` | Persistent global flags may appear at parent/child command boundaries | existing `crates/awiki-cli/src/cli/mod.rs` global scan | already matched the representative Go probe | existing group dry-run contracts | no new selector needed for this no-gap path | low for probed case; full argument-order matrix remains deferred |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli group --dry-run get --group did:wba:awiki.ai:groups:demo:e1_group
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli status -v
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli group get -g did:wba:awiki.ai:groups:demo:e1_group
cd /home/ecs-user/awiki-space/awiki-cli && AWIKI_CLI_UPDATE_CACHE_ONLY=1 go run ./cmd/awiki-cli --dry-run status -vh
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli ./internal/cmdmeta -run 'Test.*(Root|Command|Flag|Alias|Required|Catalog|Config)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test cli_parser_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/core/test_basic_commands.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/core/test_basic_commands.py::test_unknown_local_flags_fail_like_go_cobra tests_v2/core/test_basic_commands.py::test_unknown_shorthand_flags_fail_like_go_cobra -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/core/test_basic_commands.py tests_v2/core/CLAUDE.md
```

Observed results:

- Go probe for `group --dry-run get --group ...`: succeeded with command
  `awiki-cli group get` and dry-run `group.show`, matching existing Rust
  behavior.
- Go probes for `status -v`, `group get -g ...`, and `status -vh`: returned
  exit 1 with `error.code=internal_error` and the Cobra/pflag
  `unknown shorthand flag` message.
- Focused Go `internal/cli` and `internal/cmdmeta` guard: passed.
- Rust focused parser contract: 2 passed, 0 failed, 0 ignored.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- System-test wrapper syntax and scoped whitespace checks: passed.
- Focused non-mail system-test parser selectors: 2 passed, 0 failed, 0 skipped
  in 5.95s.
- File sizes stayed below the default review-size limit:
  `src/cli/mod.rs` 582 lines, `tests/cli_parser_contract.rs` 148 lines,
  `tests_v2/core/test_basic_commands.py` 751 lines, and
  `tests_v2/core/CLAUDE.md` 14 lines.
- Disk remained stable after verification: `/home/ecs-user/awiki-space` had
  26G available and 73% used, so no cleanup was needed in this batch.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector paths:
  `tests_v2/core/test_basic_commands.py::test_unknown_local_flags_fail_like_go_cobra`
  and
  `tests_v2/core/test_basic_commands.py::test_unknown_shorthand_flags_fail_like_go_cobra`.
- The selectors use isolated local workspaces and do not start live mail
  services. Mail selectors were not run or counted.

Coverage boundary:

- This batch covers unknown shorthand flag rejection only. It intentionally does
  not implement full Cobra help output, `help` command rendering, `-h` help
  behavior, command usage text, short application flag aliases, or every
  argument/flag ordering edge.
- The Rust parser still uses an explicit hand-written command table. A future
  parser batch should continue with focused Cobra parity slices instead of a
  broad parser rewrite unless the command catalog is redesigned in a dedicated
  architecture pass.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The parser change is std-only and does not affect the approved
`rusqlite + bundled` SQLite path, Rustls-first TLS policy, ANP SDK wiring, or
mail selector deferral.
