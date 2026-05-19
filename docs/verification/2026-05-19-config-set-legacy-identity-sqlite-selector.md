# 2026-05-19 Config Set Legacy Identity SQLite Selector

Timestamp: 2026-05-19T22:10:00+0800.

Scope: add `awiki-system-test/tests_v2` subprocess acceptance for the existing
Go-shaped `config set --did-domain` preflight path. The selector proves the
normal Rust public command auto-runs workspace upgrade for a legacy OpenClaw
identity plus legacy SQLite fixture before persisting the requested DID domain.
This batch changes system-test coverage and Rust repository evidence only; it
does not change production Rust code, manifests, dependencies, ANP SDK source,
or mail selector coverage.

Pipeline:

- Followed the system-test-first lane after a broad non-mail run was already
  green.
- Used read-only mapping to compare Go `runConfigSet` /
  `resolveConfigForWorkspace`, Rust `run_config_set` /
  `resolve_config_for_workspace`, and current Rust migration contracts.
- Found no Rust production behavior gap in the scoped path. The remaining gap
  was public `tests_v2` acceptance for the combined legacy settings, flat
  OpenClaw identity, legacy SQLite import, imported k1 replacement, SQLite owner
  rebind, and final config write path.
- Added one Rust-only selector in `tests_v2/core/test_basic_commands.py` and
  updated the nearest `tests_v2/core/CLAUDE.md` coverage note.
- Existing dirty helper files in `awiki-system-test` were not touched or staged.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/config.go`, `internal/cli/app.go`, `internal/upgrade/migration_v0_to_v1.go`, `internal/identity/legacy.go`, `internal/store/import.go` | normal non-dry-run `config set --did-domain` runs workspace upgrade first, imports legacy settings, legacy flat OpenClaw identity, and legacy SQLite, replaces imported handle-shaped k1 DID via `did-auth.replace_did`, rebinds SQLite owner state, stamps schema version 3, clears the journal, then writes the requested DID domain | `crates/awiki-cli/src/app.rs`, `src/upgrade/{upgrader,migration_v0_to_v1,migration_v2_to_v3}.rs`, `src/identity/legacy.rs`, `src/store/{import,rebind}.rs` | implemented; subprocess acceptance added | `workspace_migration_v0_to_v1_contract` passed 25 tests; `workspace_upgrade_if_needed_contract` passed 13 tests | `tests_v2/core/test_basic_commands.py::test_config_set_auto_upgrades_legacy_openclaw_identity_and_sqlite` passed | low; loopback fake user-service only, no live external service |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/core/test_basic_commands.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/core/test_basic_commands.py::test_config_set_auto_upgrades_legacy_openclaw_identity_and_sqlite
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q tests_v2/core/test_basic_commands.py::test_config_set_did_domain_persists_normalized_domain_and_rejects_invalid_inputs tests_v2/core/test_basic_commands.py::test_config_set_migrates_legacy_config_json_before_persisting_did_domain tests_v2/core/test_basic_commands.py::test_config_set_auto_upgrades_legacy_openclaw_identity_and_sqlite tests_v2/core/test_basic_commands.py::test_config_set_runs_v1_to_v2_cleanup_for_legacy_openclaw_artifacts tests_v2/core/test_basic_commands.py::test_config_set_workspace_upgrade_backup_includes_sqlite_and_go_named_files
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q tests_v2/core/test_basic_commands.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/core/test_basic_commands.py tests_v2/core/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
```

Observed results:

- Python syntax check passed.
- Focused selector passed with 1 passed, 0 failed, and 0 skipped in 0.88s.
- Config-set focused batch passed with 5 passed, 0 failed, and 0 skipped in
  2.05s.
- Full core basic command file passed with 22 passed, 0 failed, and 0 skipped
  in 4.85s.
- System-test whitespace check for the touched core files passed.
- `workspace_migration_v0_to_v1_contract` passed 25 tests.
- `workspace_upgrade_if_needed_contract` passed 13 tests.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/core/test_basic_commands.py::test_config_set_auto_upgrades_legacy_openclaw_identity_and_sqlite`.
- The selector uses an isolated awiki-cli runtime, a local
  `127.0.0.1:0` fake `did-auth.replace_did` JSON-RPC server, and Python's
  standard `sqlite3` fixture writer.
- It does not use the external user-service URL, message-service URL,
  WebSocket URL, platform service manager, or mail service endpoints.
- Failures: 0. Skips: 0 for the focused selector, focused config-set batch, and
  full core basic command file.

Behavior proven:

- Removes the helper-created `config.yaml`, seeds legacy OpenClaw
  `settings.json`, a flat legacy identity, and a v6 legacy SQLite
  `database/awiki.db` message row owned by the legacy k1 DID.
- Runs the real Rust CLI subprocess through `config set --did-domain
  " Tenant.Example. "`.
- Verifies the command returns the Go-shaped success envelope with normalized
  `tenant.example` after workspace upgrade succeeds.
- Verifies one authenticated `POST /user-service/did-auth/rpc` request reaches
  the loopback fake server with `Authorization: Bearer jwt-legacy`, method
  `replace_did`, and a generated handle-path `e1` DID.
- Verifies the imported identity index, identity record, auth record, and DID
  document are rewritten to the new DID and returned `jwt-replaced` token, and
  the old k1 identity directory is removed.
- Verifies the replace-did backup manifest records the legacy DID and planned
  new DID.
- Verifies final `config.yaml` preserves the imported service base URL and
  websocket runtime mode while persisting `services.did_domain: tenant.example`.
- Verifies `upgrade/meta.json` reaches schema version 3 with no warnings,
  records a backup directory under `upgrade/backups`, and clears the journal.
- Verifies the target `data/awiki-cli.db` contains the legacy message row with
  `owner_did` rebound to the replacement DID and a positive SQLite user version.

Coverage boundary:

- This is public CLI subprocess evidence for the normal `config set` preflight
  path and the combined legacy settings, identity, SQLite, k1 replacement, and
  final config-write flow.
- It does not add live external identity-service acceptance; the fake loopback
  server verifies request and persistence behavior deterministically.
- It does not claim rollback, every YAML parser/serializer edge, platform
  listener cleanup, platform service-manager behavior, broad non-mail
  repository acceptance for this specific batch, or mail selectors.
- Mail selectors remain deferred and were not run or counted.

Dependency note: no Rust dependency was added. Cargo manifests and lockfile
remain unchanged. SQLite remains on the approved `rusqlite + bundled` path for
runtime portability. TLS remains Rustls-first; no OpenSSL, `native-tls`,
bundled OpenSSL, ANP SDK, platform service, or SQLite backend change was
introduced.
