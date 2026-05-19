# 2026-05-19 Site Write Live Selector

Timestamp: 2026-05-19T23:20:00+0800.

Scope: add focused live `awiki-system-test` evidence that implemented Rust
tenant site write commands reach the configured `/site/rpc` boundary and map
the current backend's non-admin denial through the public CLI envelope. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for a small site acceptance
  gap rather than doing production-code translation.
- A read-only Native Agent mapped the existing live site selector shape and
  confirmed the stable write commands to exercise in this batch.
- The leader integrated the selector, ran focused Python/Rust/site validation,
  updated docs, and kept existing dirty helper files out of scope.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/site.go`, `internal/site/service.go` | `site root set` authenticates, reaches `/site/rpc`, and maps non-admin `forbidden` service errors without dry-run metadata drift | `crates/awiki-cli/src/app/site_handlers.rs`, `src/site/{client,service,wire}.rs` | implemented and fake-service tested | `site_live_contract` passed 4 tests | `tests_v2/site/test_site_cli.py::test_site_write_commands_reach_site_rpc_and_map_non_admin_forbidden` passed | low; stable live negative boundary, not tenant-admin success |
| `internal/cli/site.go`, `internal/site/service.go` | `site page create` authenticates, reaches `/site/rpc`, and maps non-admin `forbidden` service errors without dry-run metadata drift | same | implemented and fake-service tested | same | same | low; generated random slug, no existing page state required |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/site/test_site_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/site/test_site_cli.py tests_v2/site/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test site_contract --test site_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/site/test_site_cli.py::test_site_write_commands_reach_site_rpc_and_map_non_admin_forbidden
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q tests_v2/site/test_site_cli.py
```

Observed results:

- Python syntax check for `tests_v2/site/test_site_cli.py` passed.
- Whitespace check for the touched site system-test files passed.
- Rust site contracts passed 10 tests total:
  `site_contract` 6 and `site_live_contract` 4.
- Focused live site write selector passed with 1 passed, 0 failed, and 0
  skipped in 1.69s.
- Full live site system-test file passed with 3 passed, 0 failed, and 0 skipped
  in 3.13s.
- A concurrent exploratory focused run saw an `auth_required DID not found or
  revoked` response while another site selector was using the same
  registration fixture. The sequential focused and full-file reruns passed, so
  site selectors should be kept sequential unless their identity fixtures are
  isolated.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_ENABLE_MAIL_TESTS=0`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/site/test_site_cli.py::test_site_write_commands_reach_site_rpc_and_map_non_admin_forbidden`.
- Live endpoint context came from the normal `awiki-system-test` environment
  and the configured DID domain fixture. The command uses real Rust CLI
  subprocesses and the configured remote service stack, not the local
  fake-service Rust contract servers.
- Failures: 0 in final sequential verification. Skips: 0.

Coverage boundary:

- This batch adds live negative-boundary evidence for `site root set` and
  `site page create`, complementing the existing live negative-boundary
  coverage for `site root get` and `site page list`.
- It does not claim tenant-admin successful CRUD because the current configured
  test domain does not provide tenant-admin credentials for freshly registered
  test identities.
- Live `site page update`, `site page rename`, and `site page delete`
  acceptance remain future fixture-dependent work because stable coverage needs
  existing page state or a tenant-admin fixture. They remain covered by Rust
  contract/fake-service tests.
- No Rust production code, Cargo manifest, lockfile, ANP SDK source, system
  dependency, or mail selector changed.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
