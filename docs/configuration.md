# Configuration

[English](configuration.md) | [简体中文](configuration.zh-CN.md)

Authoritative configuration for **awiki-cli-rs2** (CLI, daemon, im-core). Source of truth: typed loaders, `std::env::var` / `option_env!`, and Cargo `[features]`. Defaults below are **code** defaults when unset.

## Compile flags

| Key | Source | Purpose | Default |
| --- | --- | --- | --- |
| `awiki-cli:default` | `crates/awiki-cli/Cargo.toml` | Empty default feature set | on (empty) |
| `awiki-cli:system-test-probe` | same | System-test probe binary | **off** |
| CLI → `im-core` features | same `dependencies` | **`secure-direct` + `group-e2ee` + `blocking`** | CLI enables them |
| `im-core:default` | `crates/im-core/Cargo.toml` | `sqlite` + `http` + `identity-native-anp` | **on** |
| Flutter `im-core-dart` | `scripts/flutter/build-*.sh` | Native App SDK | `group-e2ee` + `secure-direct` |
| `AWIKI_CLI_RELEASE` / `AWIKI_DAEMON_RELEASE` | `option_env!` | Release line | **`0815`** if unset |
| `AWIKI_CLI_VERSION` | `option_env!` | Embedded version | `dev` if unset |

## Workspace / tenant

| Key | Source | Purpose | Default |
| --- | --- | --- | --- |
| `global.active_tenant` | `global.json`; `--tenant` | Active tenant | fresh workspace → `china` |
| `registry.schema_version` | `tenants/registry.json` | Tenant registry format | `2` |
| `registry.official_catalog_version` | same | Reconciled official catalog | `1` |
| `registry.aliases.default` | same | Compatibility alias | fresh workspace → `china` |
| `registry.tenants[].kind` | same | `built_in` or `custom` | official entries → `built_in` |
| official `china` endpoint | same | Shanghai tenant | `https://awiki.me` / `awiki.me` |
| official `global` endpoint | same | Silicon Valley tenant | `https://awiki.ai` / `awiki.ai` |
| `AWIKI_CLI_DEFAULT_BACKEND_BASE_URL` | env | Private-release initial backend (paired override) | empty → official China |
| `AWIKI_CLI_DEFAULT_DID_HOST` | env | Private-release initial DID host (paired override) | empty → official China |
| `AWIKI_CLI_WORKSPACE_HOME_DIR` | env | Product home | unset → `~/.awiki-cli` |

Official tenants cannot be reconfigured. On first read, a v1 `default` profile that
targets `awiki.ai` becomes the canonical `global` profile without changing its
`dir_name=default`; the `default` alias continues to resolve to `global`. Before the
v2 registry and active-tenant control files are atomically replaced, `.v1.bak`
copies are created. A failed migration keeps the v1 registry usable and retries on
the next run.

## Runtime / secrets / multi-device

| Key | Source | Purpose | Default |
| --- | --- | --- | --- |
| `runtime.listener.enabled` | `config.yaml` | Message listener | `true` |
| `runtime.host_notify.enabled` | `config.yaml` | Host notify | `true` |
| `secret_storage.mode` | `config.yaml` | Secret storage | empty → `vault_required` |
| `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` | env | Vault root key | required in vault mode |
| `AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED` | env | Device revoke | unset = **on**; `0` off |
| `AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED` | env | Direct E2EE capability | unset = **on**; `0` off |
| `AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED` | env | Group E2EE capability | unset = **on**; `0` off |
| `AWIKI_DID_TRANSITION_VNEXT_HIDDEN_ROLLOUT_ENABLED` | env | DID transition client path | unset = **on**; `0` off |
| im-core `did_transition_vnext_hidden_rollout_enabled` | `ImCoreOpenOptions` | SDK gate | **`true`** |

## Daemon

| Key | Source | Purpose | Default |
| --- | --- | --- | --- |
| `AWIKI_DAEMON_BASE_URL` | env | Override base URL | persisted file or fresh install `https://awiki.me` |
| `AWIKI_DAEMON_VAULT_ROOT_KEY_B64` | env | Daemon vault root | required in vault mode |
| `AWIKI_HERMES_TUI_TOOLSETS` | env | TUI toolsets | `terminal,skills` |
| `AWIKI_CLI_ENABLE_DIAGNOSTIC` | env | Diagnostic commands (`=1`) | off |
| `AWIKI_CLI_ENABLE_MIGRATION` | env | Migration commands (`=1`) | off |

## Tests / probes

| Key | Source | Default |
| --- | --- | --- |
| `awiki-cli:system-test-probe` | Cargo feature | **off** |
| `AWIKI_SYSTEM_TEST_PROBE_DAEMON_STATE_ROOT` | env | required for probe |
| `AWIKI_DAEMON_TEST_RUNTIME_PRE_FINISH_DELAY_MS` | env | unset = no delay |
