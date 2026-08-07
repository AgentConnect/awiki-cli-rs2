# File Size Exceptions

Current file-size policy:

- Source files may contain at most 2500 lines by default. Rust files under a
  test path may contain at most 3000 lines because focused fixtures and local
  mock servers can remain easier to review together.
- An exception is a bounded, temporary maintenance decision, not a larger
  default. Every exception must record an approved line ceiling, owning domain,
  review date, reason, and concrete exit condition.
- `cargo run -p xtask -- check-structure` rejects undocumented oversized files,
  growth beyond an approved ceiling, expired or incomplete exceptions, kind
  mismatches, missing files, and stale exceptions for files that are back under
  the normal limit.
- Any change that grows an excepted file must either split it or deliberately
  update this table with a new review. A smaller file does not require changing
  its ceiling until it returns below the normal limit, at which point the row
  must be removed.
- Generated files still use a bounded exception. Their source of truth must be
  the generator, and generated output must never be split by hand.

Active exceptions are reviewed no later than the date shown below.

| Rust path | Kind | Approved lines | Owner | Review by | Reason | Exit condition |
| --- | --- | ---: | --- | --- | --- | --- |
| `crates/awiki-cli/src/system_test_probe_main.rs` | Source | 7982 | CLI test infrastructure | 2026-11-30 | Secret-contained probe keeps one closed JSONL boundary across multi-device system-test operations. | Split request families into private probe modules behind the unchanged dispatcher and output schema. |
| `crates/awiki-deamon/src/agent_status.rs` | Source | 4060 | Daemon runtime | 2026-11-30 | Status collection currently coordinates heartbeat, runtime probes, repair, release status, and inventory publication. | Extract probe, repair, and publication services while retaining one scheduler owner. |
| `crates/awiki-deamon/src/commands/mod.rs` | Source | 3255 | Daemon command layer | 2026-11-30 | Agent lifecycle and runtime command dispatch still share one protocol entry boundary. | Split lifecycle and runtime-session handlers behind the existing command dispatcher. |
| `crates/awiki-deamon/src/foreground.rs` | Source | 3611 | Daemon runtime | 2026-11-30 | Foreground ownership combines listener lifecycle, controller routing, and outbox coordination during runtime stabilization. | Extract listener, controller-message, and outbox loops with a single foreground coordinator. |
| `crates/awiki-deamon/src/foreground/tests.rs` | Test | 4474 | Daemon runtime tests | 2026-11-30 | Foreground scenarios share stateful fixtures and cross-loop assertions. | Split tests by listener, control-message, and outbox behavior once shared fixtures have a stable test module. |
| `crates/awiki-deamon/src/runtime/host.rs` | Source | 2586 | Daemon runtime | 2026-11-30 | Host execution keeps authorization, workspace preparation, invocation, and final reply in one audited path. | Extract preparation and finalization phases without duplicating authorization checks. |
| `crates/awiki-deamon/src/state/runtime_tasks.rs` | Source | 3324 | Daemon state | 2026-11-30 | Runtime task persistence and transition queries share transaction-sensitive row mappings. | Split read models from transition commands behind the existing DaemonState API. |
| `crates/awiki-deamon/src/state/tests.rs` | Test | 4666 | Daemon state tests | 2026-11-30 | Schema, migration, and state-contract tests reuse one database fixture surface. | Split schema, runtime-profile, and task-state suites after extracting common fixtures. |
| `crates/awiki-deamon/tests/agent_registration_management.rs` | Test | 3339 | Daemon integration tests | 2026-11-30 | Registration and management scenarios share a closed mock registration service. | Split registration, inventory, and lifecycle scenarios around a shared mock module. |
| `crates/awiki-deamon/tests/generic_cli_runtime_mvp.rs` | Test | 5633 | Daemon integration tests | 2026-11-30 | Generic CLI driver compatibility scenarios share process, route, and outbox fixtures. | Split driver, routing, and resume suites after moving common fixtures into test support. |
| `crates/im-core-dart/src/frb_generated.rs` | Generated | 22005 | IM Core Dart adapter | 2026-11-30 | flutter_rust_bridge owns this generated glue and codegen-check verifies staleness. | Regenerate from the adapter API; remove the exception only when upstream generation emits bounded modules. |
| `crates/im-core-dart/src/mapping/from_core.rs` | Source | 2532 | IM Core Dart adapter | 2026-11-30 | Core-to-Dart mappings remain centralized so DTO exhaustiveness is reviewable at the adapter boundary. | Split mappings by DTO domain while keeping all conversions private to the adapter. |
| `crates/im-core/src/groups/service.rs` | Source | 3420 | IM Core groups | 2026-11-30 | Public group operations and feature-gated E2EE delegation currently share one service facade. | Move protocol-specific implementations into private modules behind the unchanged GroupService API. |
| `crates/im-core/src/identity/registry.rs` | Source | 3394 | IM Core identity | 2026-11-30 | Registry mutation, migration, and projection logic share lock-sensitive identity invariants. | Extract migration and projection modules while preserving one serialized registry writer. |
| `crates/im-core/src/internal/identity_device_join.rs` | Source | 3533 | IM Core identity | 2026-11-30 | Restart-safe join cryptography and state transitions are kept together for auditability. | Split crypto encoding from the persisted transition machine after join fixtures cover both boundaries. |
| `crates/im-core/src/internal/identity_store.rs` | Source | 4732 | IM Core identity | 2026-11-30 | Identity files, vault references, migration, and locking share one consistency boundary. | Extract codecs and migrations while retaining a single lock-owning store. |
| `crates/im-core/src/internal/local_state/actor.rs` | Source | 2971 | IM Core local state | 2026-11-30 | Actor commands, owner-scope invariants, and async serialization share one database owner. | Split command families behind the same actor mailbox after schema migration behavior stabilizes. |
| `crates/im-core/src/internal/local_state/messages.rs` | Source | 9050 | IM Core local state | 2026-11-30 | Message schema, row mapping, projection, recovery, and read-state transactions accumulated in one SQLite boundary. | Split schema, projection, recovery, and read-state repositories with transaction ownership kept explicit. |
| `crates/im-core/src/internal/local_state/sync_v2.rs` | Source | 5752 | IM Core local state | 2026-11-30 | Sync-v2 schema and transaction primitives share ordering and cleanup invariants. | Split schema, receipt, cursor, and cleanup repositories without crossing transaction boundaries. |
| `crates/im-core/src/internal/message_runtime/local_projection.rs` | Source | 2749 | IM Core messaging | 2026-11-30 | Local conversation projection keeps canonicalization and persistence ordering together. | Extract pure canonicalization from persistence orchestration behind the existing projection API. |
| `crates/im-core/src/internal/message_runtime/mark_read.rs` | Source | 2826 | IM Core messaging | 2026-11-30 | Local and remote read-watermark transitions share retry and projection invariants. | Separate transition planning from transport execution while preserving idempotency tests. |
| `crates/im-core/src/internal/message_runtime/read.rs` | Source | 5367 | IM Core messaging | 2026-11-30 | Inbox, history, hydration, and recovery reads share transport and local projection decisions. | Split remote fetch, hydration, and recovery modules behind one read-runtime facade. |
| `crates/im-core/src/internal/message_runtime/read/tests.rs` | Test | 7116 | IM Core messaging tests | 2026-11-30 | Read-runtime scenarios share extensive transport, session, and vault fixtures. | Extract common fixtures and split inbox, history, hydration, and recovery suites. |
| `crates/im-core/src/internal/message_runtime/sync.rs` | Source | 2794 | IM Core messaging | 2026-11-30 | Legacy sync orchestration retains a cohesive compatibility state machine. | Isolate legacy transport and projection phases, then remove with the documented compatibility sunset. |
| `crates/im-core/src/internal/message_runtime/sync_v2.rs` | Source | 5267 | IM Core messaging | 2026-11-30 | Sync-v2 fetch, reconciliation, receipts, and cleanup currently share one ordered runtime. | Extract phase modules around an explicit reconciliation plan and single commit coordinator. |
| `crates/im-core/src/internal/secure_direct/v2_product.rs` | Source | 2887 | IM Core Direct E2EE | 2026-11-30 | Exact-device fan-out, attachment resume, and delivery aggregation share one product transaction. | Extract attachment and aggregation helpers behind one authorized fan-out coordinator. |
| `crates/im-core/src/internal/secure_direct/v2_store.rs` | Source | 3553 | IM Core Direct E2EE | 2026-11-30 | V2 session, prekey, ledger, and vault-reference persistence share owner/device scoping. | Split repositories by record family while retaining one owner-scope transaction boundary. |
| `crates/im-core/src/messages/service.rs` | Source | 6174 | IM Core messaging | 2026-11-30 | Public message facade, compatibility routing, normalization, and inline contract tests remain co-located. | Move inline tests to dedicated modules and split send, read, and sync implementations behind MessageService. |
