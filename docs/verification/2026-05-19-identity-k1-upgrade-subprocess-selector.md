# 2026-05-19 Identity K1 Upgrade Subprocess Selector

Timestamp: 2026-05-19T20:08:00+0800.

Scope: add `awiki-system-test/tests_v2` subprocess acceptance for the existing
workspace v2->v3 automatic current handle `k1` DID replacement path. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline with system-test failures and
  gaps prioritized first.
- Three read-only Native Agents mapped Go workspace-upgrade behavior, current
  Rust upgrade/identity/store coverage, and `awiki-system-test` selector gaps
  in parallel.
- No Rust production gap was found in the scoped path. Existing Rust contract
  coverage already proves v2->v3 current-k1 replacement and warning behavior.
- The leader added one focused Rust-only subprocess selector in
  `tests_v2/id/test_identity_cli.py` plus the nearest `tests_v2/id/CLAUDE.md`
  coverage note.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/upgrade/*`, `internal/identity/service.go`, `internal/identity/replace_did.go`, `internal/cli/id.go` | command preflight runs workspace v2->v3 migration, detects current handle-shaped `k1` DIDs, calls authenticated `did-auth.replace_did`, stores the generated `e1` DID, rotates JWT, writes backup manifest, stamps schema version 3, and clears the journal | `crates/awiki-cli/src/upgrade/migration_v2_to_v3.rs`, `src/identity/replace_did.rs`, `src/app.rs` | implemented; subprocess acceptance added | `workspace_upgrade_if_needed_contract` covers current-k1 success and warning paths | `tests_v2/id/test_identity_cli.py::test_id_current_rust_auto_replaces_current_k1_did_during_workspace_upgrade` | low; loopback fake HTTP only, not live external user-service |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/id/test_identity_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q -s tests_v2/id/test_identity_cli.py::test_id_current_rust_auto_replaces_current_k1_did_during_workspace_upgrade
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_ENABLE_MAIL_TESTS=0 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider -ra -q tests_v2/id/test_identity_cli.py::test_id_current_rust_auto_replaces_current_k1_did_during_workspace_upgrade tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py::test_awiki_cli_workspace_config_update_rust_contracts
```

Observed results:

- Python syntax check passed.
- Focused Rust-only subprocess selector passed with 1 passed, 0 failed, and 0
  skipped in 0.82s.
- Combined focused selector run passed with 2 passed, 0 failed, and 0 skipped
  in 2.03s.
- The new selector starts a local `127.0.0.1:0` JSON-RPC server, seeds a
  current handle-shaped `did:wba:example.test:legacy:k1_*` identity with
  `jwt-legacy`, marks workspace schema version 2, runs the real Rust
  `awiki-cli id current` subprocess, and verifies:
  - one `POST /user-service/did-auth/rpc` request;
  - `Authorization: Bearer jwt-legacy`;
  - `method == "replace_did"`;
  - generated `new_did_document.id` starts with
    `did:wba:example.test:legacy:e1_`;
  - persisted identity index, record, DID document, and `auth.json` use the
    new `e1` DID and `jwt-replaced`;
  - the old `k1` identity directory is removed;
  - `.legacy-backup/replace-did/*/backup_manifest.json` records old and planned
    new DID values; and
  - upgrade meta is schema version 3 with no warnings and the journal removed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_ENABLE_MAIL_TESTS=0`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/id/test_identity_cli.py::test_id_current_rust_auto_replaces_current_k1_did_during_workspace_upgrade`.
- The selector uses only a loopback fake user-service endpoint. It does not use
  the external user-service URL, message-service URL, WebSocket URL, or mail
  service endpoints.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch adds true CLI subprocess acceptance for current handle-k1
  automatic replacement during workspace upgrade.
- It does not add live external identity service acceptance; the fake
  user-service verifies request and persistence behavior deterministically.
- `tests_v2/id/CLAUDE.md` now records the current-k1 replacement coverage and
  leaves PKCS#8 k1 signing replacement deferred.
- Mail selectors remain deferred and were not run or counted.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
