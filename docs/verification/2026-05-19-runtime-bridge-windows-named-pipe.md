# 2026-05-19 Runtime Bridge Windows Named-Pipe I/O Batch

## Scope

This accelerated module batch closes the Rust `runtime::bridge` Windows
named-pipe implementation gap against the Go reference files:

- `awiki-cli/internal/runtime/bridge_windows.go`
- `awiki-cli/internal/runtime/bridge_unix.go`
- `awiki-cli/internal/runtime/config.go`
- `awiki-cli/internal/runtime/listener/server.go`

The Rust implementation adds target-gated Windows bridge I/O in
`crates/awiki-cli/src/runtime/bridge.rs` and a target-specific
`windows-sys = 0.52` dependency in `crates/awiki-cli/Cargo.toml`.

## Parity Notes

- `listen_bridge` validates the `\\.\pipe\` endpoint and creates the first
  pending pipe instance immediately, matching Go's real `winio.ListenPipe`
  listener exposure more closely than a fully lazy listener.
- `accept_bridge` uses `ConnectNamedPipe`, maps nonblocking no-client state to
  `WouldBlock`, closes stale pending pipe instances left by zero-timeout
  availability probes, and restores accepted server streams to blocking
  byte-mode reads/writes.
- `bridge_endpoint_available` and `bridge_health_probe` use the same dial path
  as client calls, preserving Go's zero-timeout availability shape.
- `windows_dial_named_pipe` mirrors go-winio's retry strategy more closely than
  `WaitNamedPipeW`: it repeatedly attempts `CreateFileW` until the configured
  deadline only when Windows returns `ERROR_PIPE_BUSY`.
- Windows client-side bridge calls use overlapped `ReadFile`/`WriteFile`,
  `WaitForSingleObject`, `GetOverlappedResult`, and `CancelIoEx` so
  `AWIKI_CLI_TIMEOUT_BRIDGE_WRITE` and `AWIKI_CLI_TIMEOUT_BRIDGE_READ` retain
  Go-style deadline behavior instead of hanging indefinitely.
- Server-side accepted streams stay synchronous/blocking because foreground
  connection handling runs on a per-connection thread, matching the current
  Rust listener owner model and Go's blocking `net.Conn` behavior.

## Commands

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --test runtime_listener_bridge_connection_contract --test runtime_listener_bridge_dispatch_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_local_bridge_deterministic_contracts \
  -ra -q
cargo +1.79.0 check -p awiki-cli --target x86_64-pc-windows-gnu --locked
cargo +1.79.0 tree -p awiki-cli --locked | rg -n "openssl|native-tls|openssl-sys|openssl-probe|openssl-src|unsafe-libyaml|libyaml|serde_yaml|yaml|windows-sys|windows-targets" || true
```

## Observed Results

- `cargo +1.79.0 fmt --check` passed.
- `cargo +1.79.0 check -p awiki-cli --locked` passed without warnings after
  the Windows-only `Instant` import was cfg-gated.
- Focused Rust bridge tests passed 37 tests:
  - `runtime_bridge_contract`: 17 passed.
  - `runtime_listener_bridge_connection_contract`: 10 passed.
  - `runtime_listener_bridge_dispatch_contract`: 10 passed.
- `cargo +1.79.0 run --bin xtask --locked -- check-structure` passed with
  `structure ok: no undocumented Rust files over 1200 lines`.
- `git diff --check` passed.
- Focused non-mail `awiki-system-test` selector passed: 1 passed in 4.31s.
- Linux-target dependency audit returned no matches for OpenSSL, `native-tls`,
  unsafe/libyaml, or YAML crates. The Windows `windows-sys` dependency is
  target-gated and does not appear in the Linux target tree.

## Not Tested

- Windows target compilation did not reach Rust compilation. The attempted
  command failed while Cargo downloaded Windows target crates from the
  configured USTC mirror:

  ```text
  SSL connect error (OpenSSL SSL_connect: SSL_ERROR_SYSCALL in connection to crates-io.proxy.ustclug.org:443)
  ```

- Live Windows named-pipe behavior remains pending until a Windows host or CI
  can run listen/availability/health/round-trip/read-timeout/write-timeout
  tests.
- Mail selectors remain deferred and are not part of this acceptance.

## Dependency Boundary

This batch adds only target-gated `windows-sys = 0.52` features for direct
Win32 APIs:

- `Win32_Foundation`
- `Win32_Storage_FileSystem`
- `Win32_System_IO`
- `Win32_System_Pipes`
- `Win32_System_Threading`

No named-pipe abstraction crate, async runtime, WebSocket crate, HTTP client,
OpenSSL, `native-tls`, bundled OpenSSL, YAML crate, platform service-manager
crate, ANP SDK feature, or new SQLite backend was added. SQLite remains on the
approved `rusqlite + bundled` path, and TLS remains Rustls-first.
