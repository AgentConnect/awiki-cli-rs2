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

## Identity registration and recovery

The SDK exposes `registerHandleWithPhone`, `registerHandleWithEmail`, and `recoverHandle` on `AwikiImCore`. These calls are core-level identity registry operations that map to `im-core` public identity DTOs; they do not depend on any `awiki-me` account gateway or UI model.

## Group creation service DID

`CreateGroupRequest.serviceDid` maps to `im_core::groups::GroupCreateRequest.service_did`. Resolution order is:

1. `request.serviceDid`
2. `AwikiImCoreConfig.anpServiceDid`
3. `invalid_input(field = service_did)`

## Message retry

`retryMessage` is explicitly unsupported in v0.1 and returns `unsupported_capability("message-retry")`. The SDK must not rebuild a send request from display message DTOs because those DTOs can lose target, body, security, idempotency, and retry-plan information.

## Realtime ownership

The native SDK exposes realtime as a high-level session and event stream:

```dart
final capability = await client.realtime.capability();
if (capability.runnerExposed) {
  final session = await client.realtime.start();
  final eventsSub = client.events.listen((event) {
    // MessageReceived / GroupUpdated / HostNotification / connection state, etc.
  });
  final stateSub = client.connectionStates.listen((state) {
    // connected / reconnecting / closed, without transport details.
  });

  await session.stop();
  await eventsSub.cancel();
  await stateSub.cancel();
}
```

WebSocket remains an `im-core` internal transport concern. Transport details such as WebSocket URLs, raw frames, ping/pong, request IDs, bearer headers, and dispatch queues are internal to `im-core` and must not become Dart public API. App code should configure only `AwikiImCoreConfig.transportPolicy` and consume `client.events` / `client.connectionStates`.

Flutter Web still receives a stub and does not support native realtime.

## Attachments And E2EE

`client.attachments.send(AttachmentSendRequest(...))` is a high-level facade over `im-core` attachment sending. `AttachmentSendRequest.security` defaults to `MessageSecurityMode.defaultPlain`; callers can set `MessageSecurityMode.e2eeRequired` for direct or group E2EE attachment messages.

Secure attachment sends do not expose P7 control-plane calls, download tickets, object keys, nonces, raw ciphertext, secure session state, or MLS provider paths to Dart. `AttachmentSendResult.manifestJson` is the public redacted manifest projection. For E2EE attachments it may include `encryption_info.mode = object-e2ee`, `object_cipher`, and `plaintext_size`, but must not contain `object_key_b64u` or `nonce_b64u`.

`UploadedAttachment.sizeBytes` / `size` describe the uploaded object bytes. For `object-e2ee` this is ciphertext size. `UploadedAttachment.plaintextSizeBytes` carries the original plaintext size when available.

## Codegen

Generated files are committed so the package can be checked out and analyzed without requiring codegen first:

- `crates/im-core-dart/src/frb_generated.rs`
- `packages/awiki_im_core/lib/src/generated/bridge_generated.dart`

Run:

```bash
scripts/flutter/codegen-check.sh
```

`codegen-check.sh` runs the bridge generator and fails if the committed generated Rust/Dart files are not already in sync. If `flutter_rust_bridge_codegen` CLI flags change, update this script but keep the same input/output paths.

## Build commands

Rebuild all native SDK artifacts after Rust SDK changes:

```bash
scripts/flutter/build-sdk-native.sh
```

The one-step script runs:

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-apple.sh
scripts/flutter/build-android.sh
```

Single-platform builds remain available:

```bash
scripts/flutter/build-sdk-native.sh --macos-only
scripts/flutter/build-sdk-native.sh --ios-only
scripts/flutter/build-sdk-native.sh --android-only
```

Full Android builds require `cargo-ndk`. Full Apple builds must run on macOS with Xcode and Rust Apple targets installed. Use `--dry-run` to print the selected build steps without compiling native artifacts.

## Common local errors

- Missing `../anp/anp/rust` sibling checkout: this workspace depends on a sibling ANP Rust crate.
- Missing `cargo-ndk`: required for full Android native library builds.
- iOS symbols not found: verify the podspec vendored XCFramework path and `-force_load` slice path.
- FRB generated files stale: run `scripts/flutter/codegen-check.sh`.
