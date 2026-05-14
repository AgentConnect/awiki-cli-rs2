# Known Go Issues And Deferred Fixes

This file records Go behavior or design debt found during the Rust parity port.
Parity work should reproduce observable Go behavior unless a deviation is
explicitly approved and documented here.

Translation rule: implement the Rust port one-to-one with the Go implementation
first. Do not mix optimization/refactoring goals into parity translation work.
When an optimization, cleanup, or Rust-native redesign looks useful, record it
below as a deferred optimization and keep the current translation aligned with
Go behavior. Deferred optimizations require a later, separate goal after parity
is proven.

| Area | Go reference | Issue / debt or optimization opportunity | Rust parity decision | Status |
| --- | --- | --- | --- | --- |
| Translation process | all Go files/modules | Potential Rust-native optimizations may be discovered while translating. | Record only; do not implement during parity translation unless needed to reproduce Go behavior or meet hard Rust safety/build constraints. | standing_rule |
| Mail RPC dependency boundary | `internal/mail/client.go`, `internal/mail/service.go`, `internal/authsdk/*` | Non-dry-run mail RPC requires auth session/JWT refresh plus HTTP/TLS dependency selection. Implementing it inside the first mail command slice would mix translation with dependency architecture and SSL policy decisions. | First Rust mail slice preserves CLI validation, dry-run plans, and local `mail notify`; remote RPC is deferred to a dedicated authsdk/mail client slice using a Rustls-reviewed stack. | deferred_translation |
| Config writer durability | `internal/config/write.go` | Go writes config through a temp file, fsync, chmod `0600`, rename, and directory sync. Current Rust config writer already existed as a simpler parser/renderer path before the `config set` slice. | `config set --did-domain` preserves black-box command output and file content for the tested slice; full durable write mechanics remain a later `write.go` parity task, not an optimization. | deferred_translation |
| Update network fetch boundary | `internal/update/update.go` | Go `CheckFresh` fetches npm metadata from npmjs and npmmirror over HTTPS with a 3-second timeout and writes cache metadata with restricted permissions. Implementing this now would force an HTTP/TLS dependency decision outside the shared service-client slice. | First Rust update slice preserves cache-only decision behavior used by system tests and records registry fetch/writeback for the shared Rustls HTTP dependency slice. | deferred_translation |
| Root update preflight guard | `internal/cli/root.go` | Go runs a global update-policy guard before most commands, can block unsupported non-dev builds, and injects newer-version warnings. | Current Rust slice implements explicit `upgrade` only. The preflight guard remains a later CLI-root parity task after update metadata fetch behavior is complete. | deferred_translation |
