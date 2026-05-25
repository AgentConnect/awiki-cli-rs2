# awiki-cli-rs2 Port Closeout Report

Date: 2026-05-19

This report closes the current Go `awiki-cli` to Rust `awiki-cli-rs2` porting
goal at the user's request. It records the verified baseline, completed areas,
unfinished areas, known fake/loopback/contract-only coverage, system-test
results, and the intermediate evidence documents produced during development.

## Repositories And Final State

| Repository | Path | Branch | Final commit | State |
| --- | --- | --- | --- | --- |
| Rust target | `/home/ecs-user/awiki-space/awiki-cli-rs2` | `main` | `74e835a Record site write live boundary acceptance` | Clean and pushed to `origin/main` |
| System tests | `/home/ecs-user/awiki-space/awiki-system-test` | `feature/changshan/group-e2ee` | `5fa1245 Prove site write non-admin RPC boundary` | Pushed; two pre-existing helper files remain dirty |
| Go reference | `/home/ecs-user/awiki-space/awiki-cli` | reference only | not changed by the final batch | Reference source |
| ANP Rust SDK | `/home/ecs-user/awiki-space/anp/anp/rust` | local SDK dependency | not changed by the final batch | Used by Rust port |

Remaining dirty files in `awiki-system-test` after final push:

- `/home/ecs-user/awiki-space/awiki-system-test/tests_v2/helpers/CLAUDE.md`
- `/home/ecs-user/awiki-space/awiki-system-test/tests_v2/helpers/awiki_cli_build.py`

Those files were already dirty and were intentionally not staged or committed
by the final site selector batch.

## Final Verified Baseline

Latest broad non-mail `tests_v2` run against the Rust CLI:

```text
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_GROUP_E2EE_CONTRACT_TEST=0 \
AWIKI_ENABLE_MAIL_TESTS=0 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider -ra -q --ignore=tests_v2/mail tests_v2
```

Result:

- Passed: 175
- Failed: 0
- Skipped: 16
- Duration: 778.04s
- Mail selectors: excluded by `--ignore=tests_v2/mail`, deferred, and not
  counted as passed.

Environment and service context:

- `AWIKI_SYSTEM_TEST_MODE=remote`
- User-service URL: `https://awiki.info`
- Message-service RPC URL: `https://awiki.info/im/rpc`
- Message-service WebSocket URL: `wss://awiki.info/im/ws`
- ANP message-service RPC URL: `https://awiki.info/anp-im/rpc`
- `AWIKI_GROUP_E2EE_CONTRACT_TEST=0` for the broad run
- `AWIKI_ENABLE_MAIL_TESTS=0` for the broad run
- `https://awiki.info/user-service/health` returned HTTP 200
- `https://awiki.info/im/rpc` GET returned HTTP 405, the expected method guard
- `https://awiki.info/anp-im/rpc` GET returned HTTP 405, the expected method guard
- Local `message-service` systemd status was active during the broad pass

Skipped broad-run selectors:

- 12 hidden Group E2EE enabled-mode selectors skipped because
  `AWIKI_GROUP_E2EE_CONTRACT_TEST=0`
- 1 local-topology direct-message selector skipped in remote mode
- 3 multi-tenant admission selectors skipped because DID-only/message-only
  tenant env/data were not configured
- Mail selectors were not part of the broad run; they are a separate deferred
  category, not part of the 16 skips.

## Final Batch: Site Write Live Boundary

The final batch added live system-test coverage for implemented Rust tenant
site write commands:

- `site root set`
- `site page create`

New selector:

```text
tests_v2/site/test_site_cli.py::test_site_write_commands_reach_site_rpc_and_map_non_admin_forbidden
```

The selector runs real Rust CLI subprocesses against the configured `/site/rpc`
service boundary and verifies:

- authenticated command execution reaches live `/site/rpc`
- non-admin denial maps to a Go-shaped `forbidden` envelope
- `meta.dry_run=false` is preserved
- the selector does not require an existing page state or tenant-admin fixture

Final batch verification:

- `python3 -m py_compile tests_v2/site/test_site_cli.py`: passed
- `git diff --check` for touched system-test files: passed
- `cargo +1.79.0 test -p awiki-cli --test site_contract --test site_live_contract --locked`: 10 passed
- focused site write selector: 1 passed, 0 failed, 0 skipped
- full `tests_v2/site/test_site_cli.py`: 3 passed, 0 failed, 0 skipped

Notes:

- This is live negative-boundary coverage, not tenant-admin successful CRUD.
- `site page update`, `site page rename`, and `site page delete` live
  acceptance remain future fixture-dependent work because stable live coverage
  needs an existing page state or tenant-admin fixture.
- A concurrent exploratory run saw one `auth_required DID not found or revoked`
  response while another site selector was using the same registration fixture.
  Sequential focused and full-file reruns passed, so site selectors should be
  run sequentially unless identity fixtures are isolated.

## Completed Functional Areas

The Rust port has broad non-mail system-test coverage plus many focused Rust
contracts. The following areas are implemented and verified to the level
recorded in `docs/parity-matrix.md` and `docs/verification/README.md`:

| Area | Current status | Verification shape |
| --- | --- | --- |
| Core CLI, version, status, docs, schema, completion, init | System verified | Rust contracts plus `tests_v2/core` |
| Output envelopes, JSON/pretty/NDJSON/table, jq subset | System verified | Rust contracts plus output system tests |
| Parser/Cobra-like boundaries | System verified | Unknown long flags, shorthand flags, required-empty flag behavior, command schema selectors |
| Command metadata and planned stubs | System verified for stub boundary | Schema and real subprocess selectors verify Go-shaped planned-stub errors |
| Config read/write/init/update/root preflight | System verified | Core/update selectors and Rust writer contracts |
| Workspace upgrade/migration | System verified | v0/v1/v2/v3 migration contracts plus config-set legacy selector |
| Legacy OpenClaw identity import | System verified through config-set path | Loopback replace-did selector |
| Legacy flat identity import | System verified through config-set and identity paths | Rust migration contracts and system selectors |
| Legacy SQLite import/rebind | System verified | Debug DB import selector and config-set auto-upgrade selector |
| Identity local store/DID/key compatibility | System verified | Identity contracts and `tests_v2/id` |
| Identity register/bind/profile/resolve/recover/replace-did | System verified or fake-service contract verified, depending on subflow | Focused Rust contracts, loopback fake-service selectors, and system-test wrappers |
| Identity k1 to e1 upgrade | System verified through subprocess | Loopback fake `did-auth.replace_did` selector |
| Direct message send/inbox/history/mark-read/contact sync | System verified in non-mail broad run | Direct message selectors and focused contracts |
| Direct secure/E2EE message flows | System/focused-contract verified | secure init/retry/repair/status/outbox/incoming contracts and selectors |
| Group ordinary lifecycle and messages | System verified | Group local selectors and Rust contracts |
| Group E2EE dry-run/schema/status/pending/publish | Focused system verified | Enabled-mode focused selectors require explicit gate and `anp-mls` |
| Attachment upload/download | System verified | Focused remote group attachment selector and Rust attachment contracts |
| Page/content handle-level CRUD | System verified | `tests_v2/page` and Rust content/page live contracts |
| Tenant site read/write non-admin boundary | System verified for selected commands | Live `/site/rpc` non-admin selectors |
| Runtime mode/setup/apply/local listener config | System verified | Runtime system tests and Rust contracts |
| Linux user-systemd listener lifecycle | System verified on Linux host | Gated runtime listener service tests |
| Runtime local bridge and Windows named-pipe contract | Contract verified; Windows live acceptance pending | Rust target-gated/dry contracts |
| Runtime host-notify OpenClaw/Hermes local behavior | System/focused-contract verified | Local capture/fake-service selectors |
| Store schema/import/merge/contact/group/message/e2ee outbox | System/focused-contract verified | Store Rust selector and debug DB selectors |
| Docs, skills, README, onboarding, installation, config template | Asset parity verified | Byte-compare or structure checks |
| Mail CLI/RPC/cache translation | Unit/fake-service verified only | Mail system tests deferred |

## Unfinished Or Deferred Work

The current state is not a full 100% Go parity claim. The following items are
explicitly unfinished, deferred, or gated:

| Item | Status | Reason / next requirement |
| --- | --- | --- |
| Mail `tests_v2/mail/**` | Deferred | User allowed mail system tests to be skipped for now |
| Live mail-service-backed acceptance | Deferred | Needs explicit mail-focused system-test pass |
| Group E2EE enabled-mode in normal broad run | Gated | Requires `AWIKI_GROUP_E2EE_CONTRACT_TEST=1` and local `anp-mls` prerequisite |
| Local-topology direct-message selector | Skipped in remote broad run | Requires local topology mode rather than remote `awiki.info` mode |
| Multi-tenant admission selectors | Skipped | DID-only/message-only tenant env/data missing |
| Tenant-admin site CRUD success | Not system-covered | Current test domain does not provide tenant-admin credentials for fresh test identities |
| Live site update/rename/delete write acceptance | Deferred | Needs existing page state or tenant-admin fixture |
| macOS launchd live service-manager acceptance | Not live-tested | Linux host cannot prove launchd execution |
| Windows Service Manager live acceptance | Not live-tested | Linux host cannot prove Windows SCM execution |
| Windows named-pipe live OS acceptance | Not live-tested | Contract/target-gated coverage exists; Windows host validation remains |
| Full Cobra/help/short alias parity for every edge case | Partial | Parser parity was expanded for tested slices, not every Cobra nuance |
| Full `yaml.v3` parser/serializer parity | Partial | Rust config writer/scalar paths are verified for targeted cases |
| Full tenant site admin workflow | Deferred | Needs admin fixture and stable backend state |
| Mail notification end-to-end delivery | Deferred | Mail selectors and mail service acceptance are not yet run |
| Deep production trace-phase parity for every service call | Partial | Many trace labels are covered; remaining call-site depth is recorded per module |
| File-size cleanup for large test/aggregation files | Deferred | Some files were accepted temporarily to prioritize system-test blockers |
| Optimization/refactor pass after 1:1 translation | Deferred | User requested translation first; optimization ideas should be recorded separately |

## Planned Stub Families

Some Go command families are planned stubs in the reference implementation or
are currently preserved as Go-shaped stub boundaries in Rust. They are not
business-feature implementations:

- `group.code.*`
- `runtime.heartbeat.*`
- `people.*`
- `people.contacts.*`
- `debug.raw.rpc`
- `debug.schema-cache`
- `debug.logs`

The Rust port verifies these through parser/schema/stub-boundary selectors so
they return stable Go-shaped `planned for PHASE` hints instead of accidental
Rust `not implemented` errors.

## Fake, Loopback, Or Contract-Only Coverage

The following coverage should not be misread as production external service
acceptance:

| Coverage | Type | Boundary |
| --- | --- | --- |
| Config-set legacy OpenClaw/SQLite auto-upgrade | Loopback fake service | Uses fake `did-auth.replace_did`, not live user-service replacement |
| Identity k1 to e1 upgrade | Loopback fake service | Uses fake `did-auth.replace_did` |
| Identity register email live contracts | Local subprocess plus fake HTTP | Proves payload/session behavior, not production email delivery |
| Identity recover live contracts | Local subprocess plus fake HTTP | Proves local staging/finalization and RPC shape |
| Identity replace-did live contracts | Local subprocess plus fake HTTP | Proves store rewrite and RPC shape |
| Many `*_live_contract.rs` targets | Loopback fake HTTP | Rust contract targets bind `127.0.0.1:0`, not external services |
| Page/content live contracts | Loopback fake HTTP for contract targets | Real page system tests cover external content paths separately |
| Site live contracts | Loopback fake HTTP for contract targets | Real site system tests cover non-admin live boundary only |
| Attachment timeout fix | Local fake-server stability | Fixes test fake-server lifetime, not production logic |
| Runtime host-notify OpenClaw/Hermes selectors | Local capture/fake service | Proves local config/route/payload behavior, not real OpenClaw/Hermes delivery |
| `tests_v2/cli/test_awiki_cli_*_rust_contracts.py` wrappers | Pytest wrapper over Cargo tests | Exposes Rust contracts through system-test harness but does not always call external services |
| Mail contracts | Rust unit/fake-service contracts | Mail system-service acceptance remains deferred |
| Windows/macOS service-manager dry contracts | Target-gated or dry contract | Live OS behavior requires respective hosts |

## System-Test Status By Category

| Category | Status | Evidence |
| --- | --- | --- |
| Broad non-mail `tests_v2` | Passed | 175 passed, 0 failed, 16 skipped |
| `tests_v2/core` | Passed after config-set selector | Full core basic command file: 22 passed |
| Config-set legacy identity/SQLite selector | Passed | 1 focused passed; focused config batch 5 passed |
| Site live file | Passed after final batch | 3 passed |
| Site write focused selector | Passed | 1 passed |
| Group E2EE enabled-mode focused selectors | Passed in dedicated gated run | Not part of normal broad run |
| Attachment contract wrapper under broad load | Fixed and passed | Broad rerun passed after fake-server timeout fix |
| Mail selectors | Deferred | Not run and not counted |
| Local topology direct selector | Skipped | Remote mode |
| Multi-tenant tenant-gated selectors | Skipped | Missing env/data |
| macOS/Windows live platform selectors | Not run | Host limitation |

## Dependency And Platform Constraints

Current dependency policy recorded in `docs/parity-matrix.md` and
`docs/dependency-decisions.md` remains:

- Prefer pure Rust / cross-platform dependencies.
- SQLite exception is approved as `rusqlite + bundled`, compiling SQLite into
  the CLI binary for runtime portability.
- TLS is Rustls-first.
- Do not introduce OpenSSL, `native-tls`, or bundled OpenSSL unless Rustls
  options fail a documented parity requirement.
- Avoid system libraries where practical.
- New dependency additions require dependency-decision documentation and
  verification evidence.

No new Rust dependency, manifest change, lockfile change, ANP SDK source change,
or platform-service dependency was introduced by the final site selector batch.

## File-Size And Structure Notes

The active guideline evolved during the task. The current repository policy is:

- Rust source files should target at most 2500 non-generated lines by default.
- Rust test files should target at most 3000 non-generated lines by default.
- Files above the applicable source/test limit are allowed as exceptions when
  documented in `docs/file-size-exceptions.md` with a concrete reason.
- During late system-test-fix work, file-size cleanup was intentionally
  deprioritized so system-test blockers could be closed first.

Known notes:

- `msg_live_contract.rs` reached 1200 lines under the older default. Under the
  current 3000-line test-file target, it does not require an exception, but
  substantial additions should still consider focused helper extraction when
  reviewability suffers.
- `message_secure_client_contract.rs` and
  `message_secure_commands_contract.rs` were near the older default limit; under
  the current 3000-line test-file target, neither requires an exception.
- `tests_v2/runtime/test_runtime_cli.py` is a pre-existing aggregation file
  above the older review-size target but below the current 3000-line test-file
  target; the runtime-small batch documented it as a system-test aggregation
  exception.
- `listener_supervisor_run.rs` was reduced by extracting
  `listener_session_rpc.rs`. It is below the current 2500-line source target,
  but remains close enough to the limit that future listener work should keep
  extracting focused helpers instead of growing the foreground owner.

## Development Pipeline Used

The later phase used the accelerated module-batch pipeline requested by the
user:

1. Pre-scan Go/Rust/system-test coverage for a module or feature cluster.
2. Use Native Agents in parallel for independent mapping or bounded
   implementation/test lanes when useful.
3. Keep write scopes non-overlapping.
4. Keep mail selectors deferred unless the batch is mail-focused.
5. Run layered validation:
   - Rust focused tests and `cargo check`
   - structure and whitespace checks
   - focused system-test selectors
   - broad non-mail system-test batch only at module/checkpoint boundaries
6. Batch documentation updates at the end of a module batch.
7. Leader owns integration, docs, commit, and push.

This pipeline should remain the preferred approach if future work continues.

## Intermediate Reports And Evidence Documents

Main index and cross-module records:

- `docs/parity-matrix.md`
- `docs/verification/README.md`
- `docs/dependency-decisions.md`
- `docs/file-size-exceptions.md`
- `docs/known-go-issues.md`

System-test audit and broad-run reports:

- `docs/verification/2026-05-19-tests-v2-remote-system-audit.md`
- `docs/verification/2026-05-19-tests-v2-non-mail-broad-pass.md`
- `docs/verification/2026-05-19-tests-v2-group-e2ee-enabled-and-broad-rerun.md`
- `docs/verification/2026-05-19-tests-v2-non-mail-broad-pass-after-config-selector.md`

Final and late system-test-fix reports:

- `docs/verification/2026-05-19-site-write-live-selector.md`
- `docs/verification/2026-05-19-config-set-legacy-identity-sqlite-selector.md`
- `docs/verification/2026-05-19-attachment-contract-timeout-system-test-fix.md`
- `docs/verification/2026-05-19-identity-k1-upgrade-subprocess-selector.md`

Accelerated selector-batch reports:

- `docs/verification/2026-05-19-foundation-rust-selector.md`
- `docs/verification/2026-05-19-store-rust-selector.md`
- `docs/verification/2026-05-19-identity-rust-selector.md`
- `docs/verification/2026-05-19-identity-live-rust-selector.md`
- `docs/verification/2026-05-19-message-group-rust-selector.md`
- `docs/verification/2026-05-19-message-secure-rust-selector.md`
- `docs/verification/2026-05-19-page-site-rust-selector.md`
- `docs/verification/2026-05-19-runtime-small-rust-selector.md`
- `docs/verification/2026-05-19-mail-local-coverage-audit.md`

Runtime/listener/host-notify reports:

- `docs/verification/2026-05-19-runtime-listener-session-rpc-structure.md`
- `docs/verification/2026-05-19-runtime-listener-service-manager-dry-contract.md`
- `docs/verification/2026-05-19-runtime-listener-service-managed-restart.md`
- `docs/verification/2026-05-19-runtime-listener-secure-session-local-queue-selector.md`
- `docs/verification/2026-05-19-runtime-listener-secure-outbox-local-queue-selector.md`
- `docs/verification/2026-05-19-runtime-bridge-windows-named-pipe.md`
- `docs/verification/2026-05-19-runtime-hermes-bridge-linux-service.md`
- `docs/verification/2026-05-18-runtime-host-notify-local-contracts.md`
- `docs/verification/2026-05-18-runtime-listener-contact-notification-lookup.md`
- `docs/verification/2026-05-18-runtime-listener-foreground-signal-selector.md`
- `docs/verification/2026-05-18-runtime-listener-service-foreground-status.md`
- `docs/verification/2026-05-18-runtime-listener-session-secure-host-notify-selector.md`
- `docs/verification/2026-05-18-runtime-local-bridge-contracts.md`

Message/group/E2EE/attachment reports:

- `docs/verification/2026-05-18-message-attachment-rust-selector.md`
- `docs/verification/2026-05-18-message-group-ws-selector.md`
- `docs/verification/2026-05-18-message-group-e2ee-status-pending-publish-selector.md`
- `docs/verification/2026-05-18-message-group-e2ee-create-add-repair-decrypt-selector.md`
- `docs/verification/2026-05-18-message-group-e2ee-recover-update-negative.md`
- `docs/verification/2026-05-18-message-group-e2ee-recover-update-retryable.md`
- `docs/verification/2026-05-18-message-group-e2ee-recover-update-rpc-deterministic.md`
- `docs/verification/2026-05-18-message-group-e2ee-stale-negative.md`
- `docs/verification/2026-05-19-group-e2ee-stale-docs-reconciliation.md`

Identity/config/parser/update reports:

- `docs/verification/2026-05-18-identity-profile-resolve-bind-refresh.md`
- `docs/verification/2026-05-18-workspace-config-update-selector.md`
- `docs/verification/2026-05-18-cli-parser-required-empty.md`
- `docs/verification/2026-05-18-cli-parser-shorthand-flags.md`
- `docs/verification/2026-05-19-cli-parser-unknown-global-flags.md`
- `docs/verification/2026-05-19-config-yaml-scalar-writer.md`

Release/packaging and other support reports:

- `docs/verification/2026-05-18-release-test-packaging.md`

## Recommended Next Work

Recommended order for future completion:

1. Run and fix mail system tests under `tests_v2/mail/**`.
2. Decide whether Group E2EE enabled-mode should become part of the normal
   broad batch or remain a gated focused batch.
3. Add tenant-admin fixtures for site CRUD success and live update/rename/delete
   acceptance.
4. Run Windows and macOS platform acceptance for named pipes, Windows Service
   Manager, and launchd.
5. Resolve the remaining `awiki-system-test` helper dirty files.
6. Split or document any large Rust/test files that are still above the desired
   review-size threshold.
7. Do a cleanup/optimization pass only after preserving the current 1:1 parity
   behavior with focused regression tests.

## Closeout Conclusion

The current handoff baseline is:

- Rust `awiki-cli-rs2` is clean and pushed at `74e835a`.
- `awiki-system-test` site selector work is pushed at `5fa1245`.
- Broad non-mail `tests_v2` is green: 175 passed, 0 failed, 16 skipped.
- Mail remains deferred.
- Several focused areas are contract-only or fake-service verified and are
  explicitly recorded above.
- This is a strong non-mail system-test baseline, not a full 100% Go parity
  completion claim.
