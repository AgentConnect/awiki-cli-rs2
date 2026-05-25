# 2026-05-18 Release/Test/Packaging Static Asset Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for release, unit-test,
  coverage, CI, and package-distribution assets.
- Three read-only Native Agents mapped the Go release/test asset surface,
  current Rust packaging/test coverage, and non-mail `awiki-system-test`
  selectors in parallel. The leader then integrated the Rust workflow/scripts,
  fixed a runtime parity bug exposed by the new unit-test entrypoint, ran
  layered verification, and updated docs once at batch close.
- `.goreleaser.yml` was not copied byte-for-byte because it references Go
  `cmd/awiki-cli` and Go linker flags. The Rust replacement preserves the
  externally visible npm installer artifact contract instead.
- No Cargo dependency was added. `cargo-llvm-cov` is optional external tooling
  used by `scripts/test-unit-cover.sh`; without it, non-check coverage mode
  falls back to ordinary Rust unit tests.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `.goreleaser.yml`, `.github/workflows/release.yml` | Build and publish linux/darwin/windows amd64/arm64 archives named `awiki-cli-<version>-<os>-<arch>.tar.gz|zip`, generate `awiki-cli-<version>-checksums.txt`, embed version/commit/date/CGO metadata, publish stable npm | `.github/workflows/release.yml`, `scripts/release/build-release-artifact.sh` | translated to Rust-native Cargo workflow; artifact/checksum names and archive root binary preserved | release dry-runs for host, Windows, and Linux arm64 plans; YAML parse | installer selectors and Node probes validate public artifact names | medium until a real tag release runs |
| `.github/workflows/ci.yml`, `scripts/test-unit.sh`, `scripts/test-unit-cover.sh`, `scripts/check_go_coverage.py` | CI and local scripts run unit tests, coverage checks, script syntax, helper tests, release plan smoke, installer probes | `.github/workflows/ci.yml`, `scripts/test-unit.sh`, `scripts/test-unit-cover.sh`, `scripts/check_rust_coverage.py` | implemented for Cargo/Rust layout | syntax checks, Python compile, Python helper tests, `cargo fmt --check`, `cargo check -p awiki-cli --locked` | focused non-mail update/core selectors passed | low; coverage threshold mode requires optional `cargo-llvm-cov` |
| `scripts/release/*.sh`, `scripts/release/release.env.example` | release tag, one-click publish, Gitee mirror, delete/withdraw, local env, ANP helper staging | `scripts/release/*.sh`, `.gitignore` | copied/translated with Rust unit-test commands and ANP SDK default path `../anp/anp/rust` | `bash -n`, dry-run release helper checks | public publish paths not run because they require credentials and remote releases | medium external-publication risk |
| `scripts/install.js`, `scripts/run.js`, `package.json` | npm package downloads expected archive names and launches `awiki-cli` or `awiki-cli.exe` | unchanged shared assets | already system-verified; release workflow now preserves their expected artifact names | Node installer contract probe | `tests_v2/update/test_install_script.py` passed | low |
| `internal/runtime/listener/host_notify.go` | Hermes host-notify listener status includes configured `notify_url` | `crates/awiki-cli/src/runtime/listener.rs` | fixed after `scripts/test-unit.sh` exposed the gap | focused host-notify status tests and adjacent runtime contracts | live host-notify selectors deferred to runtime listener batch | low after focused verification |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && for f in scripts/test-unit.sh scripts/test-unit-cover.sh scripts/release/*.sh; do bash -n "$f"; done
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && python3 -m py_compile scripts/check_rust_coverage.py scripts/hermes_notify_adapter.py scripts/host_notify_webhook_server.py
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && python3 -m unittest discover -s scripts -p 'test_hermes_notify_adapter.py'
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && python3 <YAML parse smoke for .github/workflows/ci.yml and release.yml>
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && scripts/release/build-release-artifact.sh --dry-run
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && scripts/release/build-release-artifact.sh --dry-run --os windows --arch amd64 --target x86_64-pc-windows-msvc
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && scripts/release/build-release-artifact.sh --dry-run --os linux --arch arm64 --target aarch64-unknown-linux-gnu
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && scripts/release/build-anp-mls.sh --dry-run
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && node <installer artifact-name and run.js binary path contract probe>
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/update/test_install_script.py -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_BINARY=/home/ecs-user/awiki-space/awiki-cli-rs2/target/debug/awiki-cli PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/update/test_install_script.py tests_v2/update/test_update_policy.py::test_root_preflight_soft_fails_non_exempt_update_checks_and_exempts_local_commands tests_v2/core/test_basic_commands.py::test_core_query_commands_return_structured_success tests_v2/core/test_basic_commands.py::test_completion_commands_generate_shell_scripts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_notify_sink_contract new_host_notify_sink_hermes_status_and_constructor_errors_match_go --locked -- --exact --nocapture
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli runtime::listener::tests::status_for_hermes_host_notify_includes_notify_url_like_go_manager --locked -- --exact
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener ./internal/runtime -run 'Test.*HostNotify|Test.*Hermes|TestResolve' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_notify_sink_contract --test host_runtime_contract --test host_runtime_hermes_host_notify_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
```

Observed results:

- Script syntax, Python compile, Python helper tests, workflow YAML parse,
  release dry-runs, ANP helper dry-run, and Node installer probes passed.
- `tests_v2/update/test_install_script.py` passed with 2 tests.
- Focused non-mail update/core system-test batch passed with 8 tests.
- `scripts/test-unit.sh` initially reached the Rust crate tests and exposed an
  existing Hermes host-notify status gap. After the fix, focused Rust
  host-notify status tests passed, the focused Go host-notify/Hermes runtime
  guard passed, and adjacent Rust runtime tests passed.
- `cargo +1.79.0 fmt --check` and `cargo +1.79.0 check -p awiki-cli --locked`
  passed before docs were updated.

Boundary note: this batch does not claim a real GitHub/Gitee/npm publication,
does not run a full release matrix build, and does not run or count mail
selectors. The next runtime/listener batch should use the precomputed
service/foreground/status-init gap table and keep mail selectors deferred.
