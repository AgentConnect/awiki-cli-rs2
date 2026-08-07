# Changelog

## 0.1.0

- Initial SDK scaffold for Rust-backed Awiki IM core bindings.
- Adds Windows x64 native loading and Flutter FFI plugin packaging.
- Exposes reliable message sync helpers `syncDelta`, `syncThreadAfter`, and
  readonly realtime sync hints while keeping global checkpoint ownership inside
  Rust `im-core`.
