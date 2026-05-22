# Awiki IM Core Flutter SDK

`packages/awiki_im_core` is a general-purpose Flutter/Dart SDK for `crates/im-core`. It is not an `awiki-me` adapter and must not expose app UI/cache DTOs such as `ChatMessage` or `ConversationSummary`.

## Layers

```text
Flutter app
  -> packages/awiki_im_core (Dart API, platform loader, generated FRB Dart glue)
  -> crates/im-core-dart (Rust-Dart facade, DTO mapping, lifecycle)
  -> crates/im-core (pure Rust IM SDK)
```

`crates/im-core` remains pure Rust. Flutter, Dart, FFI, codegen, and platform packaging belong only in the Dart package and `im-core-dart` facade.

## Supported platforms

v0.1 targets native Flutter on Android, iOS, and macOS:

- Android: `arm64-v8a`, `x86_64`, optional `armeabi-v7a`.
- iOS: device and simulator static-library XCFramework slices.
- macOS: `aarch64`/`x86_64` XCFramework slices.

Windows is not declared as a v0.1 Flutter plugin platform. Flutter Web has a stub so dependent apps can analyze; calling `AwikiImCore.open` on Web throws `UnsupportedError` because `dart:ffi` cannot load the native Rust backend there.

## Lifecycle and blocking model

The Rust facade exposes opaque `DartImCore` and `DartImClient` objects. Each object has an explicit close/dispose path. After close, Rust calls return `DartImError` with code `object_closed`; the Dart wrapper mirrors this with `AwikiImCoreException`.

`im-core` is blocking-first. Public Dart APIs are `Future<T>` and must not expose synchronous IO, SQLite, or HTTP calls into widget build paths. FRB generated calls should use the worker-thread model and the wrapper must keep App-facing methods async.

## DTO policy

Facade DTOs follow `im-core` public DTO semantics and use Dart-friendly primitives at the boundary. Time values remain ISO-8601 strings. The Dart wrapper may add convenience getters such as `AuthStatus.authenticated`, but it must not rename Rust DTO semantics such as `has_session` into a different facade meaning.

## Group creation service DID

`CreateGroupRequest.serviceDid` maps to `im_core::groups::GroupCreateRequest.service_did`. Resolution order is:

1. `request.serviceDid`
2. `AwikiImCoreConfig.anpServiceDid`
3. `invalid_input(field = service_did)`

## Message retry

`retryMessage` is explicitly unsupported in v0.1 and returns `unsupported_capability("message-retry")`. The SDK must not rebuild a send request from display message DTOs because those DTOs can lose target, body, security, idempotency, and retry-plan information.

## Realtime ownership

v0.1 exposes capability/status shape only. Realtime connect/session/events are deferred; `connect()` returns `unsupported_capability("realtime-runner")`. Transport details such as raw frames, ping/pong, request IDs, and dispatch queues are internal to `im-core` and must not become Dart public API. Future realtime work should expose high-level sessions/events only.

## Codegen

Generated files are committed so the package can be checked out and analyzed without requiring codegen first:

- `crates/im-core-dart/src/frb_generated.rs`
- `packages/awiki_im_core/lib/src/generated/bridge_generated.dart`

Run:

```bash
scripts/flutter/codegen.sh
scripts/flutter/codegen-check.sh
```

If `flutter_rust_bridge_codegen` CLI flags change, update the script but keep the same input/output paths.

## Build commands

```bash
scripts/flutter/build-host.sh
scripts/flutter/build-android.sh --dry-run
scripts/flutter/build-apple.sh --dry-run
scripts/flutter/build-all.sh
```

Full Android builds require `cargo-ndk`. Full Apple builds must run on macOS with Xcode and Rust Apple targets installed.

## Common local errors

- Missing `../anp/rust` sibling checkout: this workspace depends on a sibling ANP Rust crate.
- Missing `cargo-ndk`: required for full Android native library builds.
- iOS symbols not found: verify the podspec vendored XCFramework path and `-force_load` slice path.
- FRB generated files stale: run `scripts/flutter/codegen-check.sh`.
