# Slice 12 Status: FRB / Dart Async Bridge

## Summary

Slice 12 converts the `im-core-dart` Rust bridge to call the existing
async-first `im-core` APIs in place. It does not introduce a parallel SDK,
does not change domain DTO semantics, and keeps the Dart public facade on
Future/Stream semantics.

## Async Bridge Coverage

The bridge now calls async `im-core` APIs for:

- core open/path validation and client creation
- auth status/login/session refresh/session ensure
- identity list/default/resolve/register/recover
- messages send/inbox/history/mark-read/conversations
- attachments send/download
- directory resolve/lookup/relation/follow/unfollow/followers/following
- profile load/update/public load
- groups create/join/get/list/members/messages/leave
- email account/inbox/read/mark-read/send/download/notifications
- secure direct status/prepare/repair
- secure group status/prepare/repair
- secure outbox list-failed/retry/drop
- realtime status/start/connect/stop/event stream

Unsupported placeholders that do not have an `im-core` capability remain
explicit stubs, for example message retry and group join-code helpers.

## Handle Clone And Locking Strategy

`DartImCore` and `DartImClient` now expose internal clone helpers that copy the
underlying `im_core::ImCore` / `im_core::ImClient` handles while holding the
bridge `RwLock`, then release the guard before any `.await`.

This works because both `im_core::ImCore` and `im_core::ImClient` are cloneable
handle types backed by shared internal ownership. The bridge keeps the existing
object-close semantics: once the bridge object has been closed, new operations
return `object_closed`.

Manual lock review covered:

```bash
rg "RwLock|Mutex|read\\(|write\\(|lock\\(" crates/im-core-dart/src
rg "\\.await" crates/im-core-dart/src
```

The source API files clone handles before awaiting. The only bridge-owned
`Mutex` that remains on async paths is `DartRealtimeSession`; its `stop` method
takes the `RealtimeSession` out of the mutex before awaiting `stop()`, so no
mutex guard is held across `.await`.

Generated FRB code uses FRB's opaque lockable wrappers and was not hand edited.

## Realtime Strategy

`DartRealtimeSession` now wraps `im_core::realtime::RealtimeSession` instead of
the legacy sync `RealtimeHandle`.

The event stream path:

- takes the single `RealtimeEventStream` via `RealtimeSession::subscribe`;
- forwards events to the FRB `StreamSink<DartRealtimeEvent>` from a Tokio task;
- requests session stop if Dart closes the stream sink;
- uses `RealtimeSession::stop().await` for explicit stop;
- relies on `RealtimeSession` drop to request shutdown as best-effort cleanup.

Bridge grep confirmed no `std::thread`, `std::sync::mpsc`, `RealtimeHandle`, or
sync `connect()` call remains in the realtime bridge API/generated surface.

## Generated Bindings

Generated bindings were refreshed through the project scripts:

```bash
CARGO_NET_OFFLINE=true scripts/flutter/codegen.sh
CARGO_NET_OFFLINE=true scripts/flutter/codegen-check.sh
```

The first online attempt stalled inside `cargo metadata` while Cargo was
connected to crates.io. Running the same script with `CARGO_NET_OFFLINE=true`
used already-available dependencies and completed.

`scripts/flutter/codegen.sh` now calls:

```bash
rustfmt --edition 2021 crates/im-core-dart/src/frb_generated.rs
```

This is required because FRB emits `async move` wrappers for async bridge
functions; default `rustfmt` parsing treated the generated file as pre-2018
Rust and failed.

## Dart Test Harness

`packages/awiki_im_core/test/awiki_im_core_stub_test.dart` now imports
`package:test/test.dart` and `packages/awiki_im_core/pubspec.yaml` declares the
`test` dev dependency. The existing test content is pure Dart model/facade
shape coverage, so this lets the documented gate run with `dart test` instead
of requiring `flutter test`.

## Validation

Passed:

```bash
cargo fmt --all -- --check
cargo check -p im-core-dart --locked
cargo test -p im-core-dart --locked
CARGO_NET_OFFLINE=true scripts/flutter/codegen.sh
CARGO_NET_OFFLINE=true scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze
cd packages/awiki_im_core && dart test
git diff --check
```

`dart test` result:

```text
All tests passed: 7.
```

`cargo test -p im-core-dart --locked` result:

```text
14 tests passed across lib tests and facade_contract; doc tests had 0 tests.
```

## Remaining Follow-Up

Slice 13 still needs to remove or cfg-gate the remaining legacy blocking
compatibility paths after both upper layers have moved to async.
