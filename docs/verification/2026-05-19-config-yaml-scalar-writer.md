# 2026-05-19 Config YAML Scalar/Writer Batch

Timestamp: 2026-05-19T00:00:00+0800.

Scope: improve the Rust workspace `config.yaml` parser/writer slice against Go
`internal/config/config.go` and `internal/config/write.go` without introducing
a YAML dependency. This is a focused scalar/writer parity batch, not a full
replacement for Go `yaml.v3` behavior.

Pipeline:

- Used the accelerated module-batch pipeline for the config YAML/parser/writer
  cluster.
- Three read-only Native Agents mapped Go config behavior, current Rust
  implementation/tests, and non-mail system-test selector coverage in
  parallel.
- The leader evaluated a pure-Rust YAML dependency lane, then rejected adding a
  new dependency in this batch because the reachable candidates either failed
  the dependency constraints or could not be fetched reliably through the
  configured mirror during the batch.
- The implementation stayed in `crates/awiki-cli/src/config/{mod,write}.rs`
  and the focused Rust contract files.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/config/config.go` | Go `yaml.v3` decodes quoted scalars, common double-quoted escapes, single-quoted scalar quoting, and null-like scalar values before normal config resolution | `crates/awiki-cli/src/config/mod.rs` | improved for common scalar cases in the existing hand-written parser | `workspace_config_policy_contract` passed 6 tests | core quoted-scalar selector passed | medium; full YAML anchors/sequences/tags/block scalars remain outside this focused slice |
| `internal/config/config.go` | Malformed YAML is reported through the resolved `config_error` field while `config show` still returns the default resolved snapshot | `crates/awiki-cli/src/config/mod.rs` | preserved and now regression-tested for malformed quoted scalar input | `workspace_config_policy_contract::config_show_reports_malformed_yaml_in_config_error_like_go` | same config/update selector batch passed | low for covered public shape |
| `internal/config/write.go` | Go YAML marshal quotes scalars that would break round-trip parsing, including inline-comment markers and embedded quotes | `crates/awiki-cli/src/config/write.rs` | writer now quotes/escapes scalar values that contain comment markers, quotes, backslashes, tabs, newlines, or null/boolean-like plain values | `workspace_config_writer_contract` passed 8 tests | config set/system wrapper selectors passed | medium; comments/ordering/arbitrary unknown YAML nodes are still not preserved |
| `internal/config/write.go` | Go writer keeps durable same-directory replacement and schema-version stamping | `crates/awiki-cli/src/config/write.rs` | unchanged; existing durable writer tests remain passing | `workspace_config_writer_contract` | existing config set selector passed | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/config -run 'TestResolveRejectsDeprecatedServiceURLFieldsInConfigYAML|TestResolveHonorsExplicitFalseBoolFromConfigFile|TestUpdateDIDDomainCreatesConfigAndNormalizesValue' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test workspace_config_policy_contract --test workspace_config_writer_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo tree -p awiki-cli --locked | rg -n "openssl|native-tls|unsafe-libyaml|libyaml|serde-saphyr|serde_yaml|serde_yml|serde_yaml_ng|saphyr" || true
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/core/test_basic_commands.py::test_config_show_preserves_hash_inside_quoted_yaml_scalars tests_v2/core/test_basic_commands.py::test_config_set_did_domain_persists_normalized_domain_and_rejects_invalid_inputs tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py::test_awiki_cli_workspace_config_update_rust_contracts -ra -q
```

Observed results:

- Go focused config guard: passed.
- Rust focused config tests: 14 passed, 0 failed.
- `cargo fmt --check`: passed.
- `cargo check -p awiki-cli --locked`: passed.
- `xtask check-structure`: passed with no undocumented Rust files over 1200
  lines.
- `git diff --check`: passed.
- Dependency audit query returned no OpenSSL, `native-tls`, `unsafe-libyaml`,
  libyaml, or YAML crate entries in the locked `awiki-cli` tree.
- Focused non-mail `awiki-system-test` selector batch: 3 passed in 10.06s.

Dependency note:

- No dependency was added. Cargo manifests and lockfile remain unchanged.
- `serde_yaml` was rejected because it is deprecated.
- `serde_yaml_ng` was not selected because it still carries the `unsafe-libyaml`
  dependency path, which violates the pure-Rust preference for this batch.
- `serde_yml` was not selected because of RustSec maintenance/soundness risk.
- `serde-saphyr` was evaluated as the pure-Rust candidate, but the configured
  Cargo mirror failed repeatedly while fetching dependencies during this batch.
  Rather than block the module batch or add a half-verified dependency, the
  implementation used a bounded std-only scalar fix and kept full YAML parity
  as a separate dependency decision.
- TLS remains Rustls-first and SQLite remains on the approved
  `rusqlite + bundled` path.

File-size note:

- `crates/awiki-cli/src/config/mod.rs`: 1031 lines.
- `crates/awiki-cli/src/config/write.rs`: 488 lines.
- `crates/awiki-cli/tests/workspace_config_policy_contract.rs`: 229 lines.
- `crates/awiki-cli/tests/workspace_config_writer_contract.rs`: 306 lines.
- No file-size exception is needed.

Coverage boundary:

- Covered: common quoted scalar decoding, common double-quoted escapes,
  null-like scalar defaults, malformed quoted scalar `config_error` reporting,
  writer quoting for scalar values that would otherwise be truncated or
  misparsed, and existing config set/update public selectors.
- Not covered: full Go `yaml.v3` parity for anchors, aliases, tags, sequences,
  block scalars, duplicate keys, comments/ordering preservation, or arbitrary
  unknown YAML node round-tripping.
- Mail selectors were not run or counted. The workspace/config/update wrapper
  still runs the existing Rust target that contains mail-named subtests; this
  batch does not count those subtests as mail system-test acceptance.
