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
