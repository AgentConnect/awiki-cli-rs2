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

## Directory profile metadata

`client.directory.resolvePeer(handle)` and `client.directory.lookupHandle(handle)` can return a `DirectoryResolution.profile` populated from the WNS Handle Resolution Document `profile` object. This profile is a DID Subject Profile projection, not routing or security metadata.

The Dart `UserProfile` model uses these standard display fields:

- `displayName`
- `avatarUri`
- `profileUri`
- `description`
- `subjectType`
- `versionId`
- `ttl`

Legacy compatibility fields remain available:

- `bio` maps to / from `description` where needed.
- `avatarUrl` maps to / from `avatarUri` where needed.
- Older service inputs such as `nick_name`, `name`, `avatar_url`, and `avatar` are normalized by `im-core`.

Display fields must not be used for routing, authentication, authorization, service endpoint selection, E2EE binding, or security-profile negotiation. Apps should keep Handle or DID visible on profile and recipient-confirmation surfaces, especially for high-risk operations.

`client.directory.hydrateDisplayProfiles(peers)` reads only the local `im-core` contact/profile cache. It does not call WNS or User Service, and is intended for hot UI paths such as conversation lists, contact lists, and member lists. A returned `DisplayProfile` has `cacheHit = false` when the peer is absent locally; the app should fall back to `displayName -> handle -> did` without blocking list rendering. Remote refresh must be explicit through `resolvePeer`, `lookupHandle`, `loadPublicProfile`, or the send-time security verification path.

## Group display metadata

`CreateGroupRequest.avatarUri` maps to `group_profile.avatar_uri`; `CreateGroupRequest.name` remains the Flutter convenience input for `group_profile.display_name`. `GroupSummary` and `GroupSnapshot` expose `displayName` and `avatarUri`; the old `name` field is retained as a compatibility projection of `displayName`.

Group creation service DID is resolved from `AwikiImCoreConfig.anpServiceDid`. If it is absent, group create returns `invalid_input(field = anp_service_did)`.

These display fields are UI metadata only. They must not be used for routing, authorization, membership checks, E2EE binding, or service endpoint selection.

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
scripts/flutter/package.sh --dry-run
```

Full Android builds require `cargo-ndk`. Full Apple builds must run on macOS with Xcode and Rust Apple targets installed.

## Common local errors

- Missing `../anp/anp/rust` sibling checkout: this workspace depends on a sibling ANP Rust crate.
- Missing `cargo-ndk`: required for full Android native library builds.
- iOS symbols not found: verify the podspec vendored XCFramework path and `-force_load` slice path.
- FRB generated files stale: run `scripts/flutter/codegen-check.sh`.
