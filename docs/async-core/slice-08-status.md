# Slice 08 Status: Attachments Async Streaming

## Status

Implemented in-place for `im-core` attachment upload/download async paths.

This slice keeps existing attachment DTOs, manifest shape, digest semantics,
message projection semantics, and sync public methods intact as staged
compatibility. New async attachment methods reuse the existing attachment
runtime, wire builders, and service boundaries instead of introducing a parallel
attachment SDK.

## Async Methods Added

Attachment service:

- `AttachmentService::send_async`
- `AttachmentService::download_async`

Attachment runtime:

- `AttachmentUploadRuntime::send_async`
- `AttachmentDownloadRuntime::download_async`

Internal helpers:

- `attachment_input_to_async_blob_source`
- `prepare_attachment_metadata_from_path`
- `sha256_digest_file_b64u`
- `write_stream_atomic`
- `resolve_did_document_async`
- `persist_direct_attachment_outgoing_async`
- `persist_group_attachment_outgoing_async`

## Streaming Transport Design

- `AsyncAttachmentObjectTransport` now exposes streaming-oriented methods:
  - `put_attachment_object_stream`
  - `get_attachment_object_stream`
- `CoreHttpTransport` implements those methods with reqwest async streaming:
  - LocalFile upload uses `tokio::fs::File` plus `tokio_util::io::ReaderStream`
    and `reqwest::Body::wrap_stream`.
  - Download returns an `AsyncAttachmentObjectResponse` that can be consumed by
    chunks instead of forcing an immediate full `Vec<u8>`.
- The existing `put_attachment_object` and `get_attachment_object` methods
  remain for sync compatibility and small in-memory paths.

## Upload Behavior

- Bytes input remains in-memory and preserves the existing public DTO semantics.
- LocalFile input on the async path no longer reads the whole file into
  `PreparedAttachment.payload`.
- LocalFile async upload computes metadata and SHA-256 digest with chunked
  `tokio::fs` reads, then streams the file body during object PUT.
- Slot creation, commit, and manifest send still use the existing wire builders:
  - `attachment.create_slot`
  - `attachment.commit_object`
  - `direct.send` / `group.send`

## Download Behavior

- Memory destination still returns `DownloadedAttachmentDestination::Memory(Vec<u8>)`
  for API compatibility.
- LocalFile destination on the async path writes the response stream to a
  sibling temp file and atomically links/renames it into place.
- Existing overwrite false/true validation and temp cleanup behavior are
  preserved.
- DID document resolution has an async path with the same local identity
  document fallback behavior as the sync runtime.

## Local Projection

- Async attachment send projection uses the DB actor:
  - direct attachment outgoing projection stores message records through
    `LocalStateDb::store_messages`
  - group attachment outgoing projection stores message records and touches the
    group cache through actor commands
- Sync attachment methods still use legacy direct SQLite projection paths as
  staged compatibility until slice 13 cleanup/cfg-gating.

## Tests Added

- `attachments_upload_runtime_local_file_async_streams_explicit_path`
  verifies async LocalFile upload builds the same create-slot metadata while
  sending a file stream body instead of a bytes body.
- `attachments_download_runtime_local_file_async_streams_to_file` verifies async
  LocalFile download consumes the object through the streaming response path and
  writes through atomic temp-file handling.

Existing attachment tests continue to cover Bytes input, public API DTOs,
manifest/digest behavior, direct handle resolution, download ticket body shape,
overwrite validation, and local identity document fallback.

## Temporary Boundaries

- CLI and Dart/FRB callers are not migrated in this slice. They remain scheduled
  for slices 11 and 12.
- Sync attachment service/runtime methods remain for staged compatibility.
- Sync LocalFile upload still uses the legacy full-file `BlobSource.bytes` path.
  The new async LocalFile path is streaming and is the path that CLI/FRB should
  migrate to in slices 11 and 12.
- Memory download necessarily materializes a `Vec<u8>` because the public DTO
  represents memory downloads that way.
- Cancellation is not yet exposed as a public option. Current behavior is that
  dropping/cancelling the async task stops waiting/local stream work; once object
  commit or message submit has happened, the SDK does not claim server rollback
  or message recall.

## Validation

Passed:

```bash
cargo test -p im-core attachments --locked
cargo test -p im-core attachment_streaming --locked
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo check --workspace --locked
```

Notes:

- `cargo test -p im-core attachment_streaming --locked` completed successfully
  but the filter matched zero tests in the current test names.
- The concrete async streaming coverage is included in
  `cargo test -p im-core attachments --locked` through the new upload/download
  runtime tests listed above.

Grep fence run:

```bash
rg -n "std::fs::read|read_to_end|Vec<u8>" crates/im-core/src/internal/blob crates/im-core/src/internal/attachment_runtime crates/im-core/src/attachments
```

Recorded exceptions:

- `attachments/dto.rs`: public Bytes input and Memory download DTOs.
- `attachments/manifest.rs` and `attachment_runtime/digest.rs`: existing
  prepared payload structure for Bytes and sync compatibility paths.
- `internal/blob/source.rs`: legacy sync LocalFile path still reads into bytes;
  async LocalFile uses `AsyncBlobSource::LocalFile`.
- `attachment_runtime/upload.rs`: sync credential reads, legacy sync
  compatibility path, Bytes handling, and test fakes.
- `attachment_runtime/download.rs`: sync local identity document fallback,
  legacy sync compatibility path, and test fakes.

No `read_to_end` usage was found in the checked attachment paths.

## CLI/Dart Sync Status

No CLI, FRB, Dart, or Flutter files were changed in this slice. Because sync
compatibility methods remain available, existing upper layers continue to
compile. Full CLI async host migration remains slice 11. FRB/Dart async bridge
migration remains slice 12.

## Remaining Work

- Slice 09 should move secure direct/group E2EE DB and crypto-heavy paths behind
  the DB actor and worker model.
- Slice 10 should replace the blocking realtime runner with a Tokio
  task/session/stream model.
- Slice 11 should migrate CLI attachment commands to `send_async` and
  `download_async`.
- Slice 12 should migrate FRB/Dart attachment bridge methods to async.
- Slice 13 should remove, cfg-gate, or document remaining sync blocking
  compatibility paths after upper layers are migrated.
