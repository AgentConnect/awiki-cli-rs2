# File Size Exceptions

Current file-size policy:

- Source files should target at most 2500 non-generated lines by default.
- Test files should target at most 3000 non-generated lines by default because
  CLI contract fixtures and local mock servers often stay more reviewable when
  kept with their focused scenario.
- The automated structure check currently enforces these limits for counted
  Rust files: non-test Rust files use the 2500-line source limit, and Rust test
  files use the 3000-line test limit.
- These limits are review-size defaults, not hard compiler constraints.
- Files may exceed the applicable source/test limit as documented exceptions.
  Record each exception in this file with the kind, current line count, and a
  concrete reason. An exception records a deliberate review/maintenance
  tradeoff; it is not a new default for nearby files.
- Prefer splitting oversized source files first. Keep test files focused, but
  document intentional aggregation when it remains clearer.
- Historical verification notes may mention the older 1200-line review target;
  those notes are historical evidence, not the active policy. A source file
  above 1200 lines does not need an exception unless it exceeds the active
  2500-line source limit. A test file above 1200 lines does not need an
  exception unless it exceeds the active 3000-line test limit.

Record active exceptions below.

| Rust path | Kind | Rust lines | Reference path | Reference lines | Reason |
| --- | --- | ---: | --- | ---: | --- |
| `crates/im-core-dart/src/frb_generated.rs` | Generated flutter_rust_bridge glue; stale checked by scripts/flutter/codegen-check.sh. | Flutter SDK |
| `crates/im-core/src/internal/group_e2ee/lifecycle.rs` | Source | 3858 | `docs/architecture/group-e2ee-operations.md` |  | Group E2EE lifecycle currently keeps create/join/member/policy orchestration and repair-sensitive MLS state transitions together so the release cutover can preserve reviewable protocol ordering. Split after the release stabilization window. |
| `crates/im-core/src/internal/local_state/actor.rs` | Source | 2676 | `docs/architecture/local-state-owner-scope.md` |  | Local state actor centralizes SQLite actor commands, owner-scope invariants, and async call serialization during the owner-identity migration. Split command families after schema 17 migration behavior is stable. |
| `crates/im-core/src/internal/secure_direct/incoming.rs` | Source | 2776 | `docs/architecture/direct-e2ee-operations.md` |  | Direct secure inbound processing keeps init/cipher decrypt, replay projection, and pending outbox reconciliation in one state-machine module for the async cutover. Split once direct E2EE regression coverage is settled on release. |
| `crates/im-core/src/realtime/runner.rs` | Source | 2843 | `docs/async-core/slice-10-realtime-runner-async.md` |  | Realtime runner currently combines session lifecycle, transport retries, and event projection during the async runtime cutover. Split transport/session/projection phases after release validation. |
