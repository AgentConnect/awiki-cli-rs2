# File Size Exceptions

Rust source files should stay at or below 1200 non-generated lines by default. Exceptions require a corresponding oversized Go source file and a documented reason. A genuinely special file may be relaxed up to about 5000 lines when justified, but exception use must remain rare rather than becoming the normal module shape.

| Rust path | Rust lines | Go path | Go lines | Reason |
| --- | ---: | --- | ---: | --- |
| `crates/awiki-cli/src/runtime/listener_supervisor_run.rs` | 1536 | `awiki-cli/internal/runtime/listener/server.go` | 1802 | Foreground listener execution currently mirrors the oversized Go supervisor file for traceable 1:1 translation of run/service-run startup, bridge ownership, WebSocket session loops, secure notification normalization, local secure ACK recovery, queued secure outbox flushing, secure backlog replay, and direct/group incoming contact lookup wiring. This is an intentional translation-time exception; splitting this runtime owner into smaller Rust files is a later optimization/refactor task, not mixed into the parity slice. New HTTP lookup behavior should stay in smaller helper modules where practical. |
