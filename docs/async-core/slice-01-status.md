# Slice 01 Status: Runtime Foundation

## Summary

Slice 01 introduced the internal runtime foundation without changing public DTOs or business service behavior.

## Code Changes

```text
Cargo.toml
  - Added workspace Tokio dependency with explicit runtime/time/sync/fs/macros features.

crates/im-core/Cargo.toml
  - Added direct tokio workspace dependency.

crates/im-core/src/internal/mod.rs
  - Added internal runtime module.

crates/im-core/src/internal/runtime/**
  - Added OperationId.
  - Added TraceContext.
  - Added cloneable CancellationToken.
  - Added OperationContext with request_id/deadline/trace/cancellation.
  - Added RuntimeLimits defaults.
  - Added RuntimeTimeouts defaults.
  - Added run_blocking worker helper.
```

All new runtime types are `pub(crate)` and do not expand the public SDK API.

## Validation

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all` | Passed | Formatting applied. |
| `cargo check -p im-core` | Passed | Used once to align direct dependency state. |
| `cargo test -p im-core runtime --locked` | Passed | 60 matching/filtered tests ran; runtime unit tests passed. |
| `cargo check -p im-core --locked` | Passed | im-core compiles with locked dependencies. |
| `cargo check --workspace --locked` | Passed | Workspace still checks after runtime foundation. |
| `cargo tree -p im-core --locked \| rg -i "openssl\|openssl-sys\|native-tls"` | Passed | No matches; command exits 1 because no forbidden dependency was found. |

## Known Baseline Failures Not Addressed Here

The following baseline test failures were recorded before slice 01 and were not changed by this slice:

```text
cargo test -p im-core --locked
  - secure_service_api_shape_is_available_from_client expected Unavailable, got Unknown.

cargo test -p awiki-cli --locked
  - group_e2ee_internal_live_commands_stay_unsupported expected unsupported_capability, got identity_required.
```

## Acceptance

```text
1. Tokio runtime foundation is available to later slices.
2. Operation context, cancellation, limits, timeout defaults and blocking worker helper exist.
3. Public DTOs and public service behavior were not changed.
4. No OpenSSL/native-tls dependency was introduced.
5. Workspace check still passes.
```
