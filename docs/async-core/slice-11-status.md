# Slice 11 Status: CLI Async Host

## Status

Implemented in-place for `awiki-cli`.

This slice does not rewrite the SDK or introduce a parallel CLI/SDK stack. The
existing CLI parser, command catalog, JSON renderers, error mapping, adapter
modules, and host-runtime modules remain in place. The migration changes the
CLI entrypoint and the im-core-backed command paths to call the async SDK APIs
directly.

## Entrypoint And Compatibility

- `crates/awiki-cli/src/main.rs` now uses a Tokio host:

  ```rust
  #[tokio::main]
  async fn main() {
      std::process::exit(awiki_cli::execute_async().await);
  }
  ```

- `awiki_cli::execute_async()` is the primary CLI entrypoint.
- `awiki_cli::execute()` remains as a compatibility wrapper and creates a
  current-thread Tokio runtime. It is only for existing sync callers and must
  not be used from inside an already-running async runtime.
- Root `--help` / `-h` continues to print the default command-surface JSON
  schema through the existing CLI renderer.

## Async CLI Build Path

- Added async CLI adapter builders:
  - `build_im_core_async`
  - `build_im_client_async`
- Async command dispatch goes through `dispatch_async`.
- Handler code awaits im-core async APIs instead of using sync SDK calls on the
  migrated command paths.
- JSON output envelopes and error envelope mapping are still produced by the
  existing CLI rendering/error helpers.

## Migrated Command Domains

The following command families now route through async CLI handler/adapter paths
where they depend on im-core:

- identity:
  - `id register`
  - `id recover`
  - `id bind`
  - `id list`
  - `id current`
  - `id use`
  - `id status`
  - `id refresh-token`
  - `id resolve`
  - `id profile get`
  - `id profile set`
- messages and attachments:
  - `msg send`
  - `msg inbox`
  - `msg history`
  - `msg mark-read`
  - `msg attachment send`
  - `msg attachment download`
  - direct secure status/repair paths that are supported in the current
    cutover surface
- groups:
  - group reads
  - group lifecycle/mutation paths
  - group secure aliases
  - supported `group.e2ee` aliases such as status/publish-key-package and
    repair/update/rejoin/recover dry-run or supported paths
- people:
  - contact/read/write/follow command paths that use im-core plans/APIs
- content/page/site:
  - `page`
  - `page create`
  - `site`
  - `site page`
- mail:
  - inbox/read and attachment download paths
- runtime listener:
  - `runtime listener run`
  - `runtime listener service-run`
  - foreground/service im-core session startup now uses async core/client build
    and `RealtimeService::start_async`

## Preserved CLI-Owned Or Local Boundaries

The following remain intentionally sync/local boundaries in this slice:

- parser, command catalog, schema rendering, completion/version/init/docs/config
  surfaces
- CLI JSON envelope rendering and error/hint shaping
- workspace config and upgrade preflight orchestration
- local file reads for CLI input such as `--markdown-file`
- attachment/mail output file writes performed by the CLI after async SDK
  download/read results
- runtime service-manager operations for systemd, launchd, and Windows service
  installation/status/lifecycle
- local bridge socket accept loop and local process I/O boundaries
- legacy `id create`, `id import-v1`, and live `replace-did` execution
  boundaries that are still dry-run or explicitly unsupported on the cutover
  surface
- compatibility `execute()` wrapper, which must not be called from an existing
  runtime

## Unsupported Or Catalog Notes

- `auth status` is not a separate command in the current CLI catalog. The
  current equivalent coverage is through `id status` and `id refresh-token`.
- Hidden low-level `group.e2ee` live commands that are not part of the supported
  cutover surface still return `unsupported_capability` before identity lookup
  unless the command is a dry run. The async dispatch preserves that policy.

## Test Updates

- Updated CLI live-contract HTTP test helpers to treat HTTP header names as
  case-insensitive. Async reqwest/hyper emits lowercase names such as
  `authorization`, `signature-input`, and `content-length`, while the previous
  helpers assumed Go-style title casing. Production code was not changed to
  force header casing.
- Updated `msg_im_core_mvp_contract` group-history expectation to match the
  established im-core group message identity rule: group history IDs use
  `group_did:group_event_seq`, while the original service ID is preserved as
  `raw_message_id`.
- Tightened one `self_update` unit-test ordering issue so shared test registry
  URL and proxy environment mutations are guarded consistently.

## Validation

Passed:

```bash
cargo test -p awiki-cli --locked
cargo fmt --all -- --check
cargo check -p awiki-cli --locked
cargo run -p awiki-cli -- --help
git diff --check
```

Additional focused checks run while stabilizing the CLI async host:

```bash
cargo test -p awiki-cli --lib --locked
cargo test -p awiki-cli --test identity_live_contract --locked
cargo test -p awiki-cli --test identity_recover_live_contract --locked
cargo test -p awiki-cli --test identity_register_email_live_contract --locked
cargo test -p awiki-cli --test msg_im_core_mvp_contract --locked
cargo test -p awiki-cli --test msg_live_contract --locked
cargo test -p awiki-cli --test msg_jwt_fallback_trace_contract --locked
cargo test -p awiki-cli --test mail_live_contract --locked
cargo test -p awiki-cli --test page_live_contract --locked
cargo test -p awiki-cli --test site_live_contract --locked
cargo test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo test -p awiki-cli --test msg_ws_group_live_contract --locked
cargo test -p awiki-cli --test msg_ws_history_live_contract --locked
cargo test -p awiki-cli --test msg_ws_inbox_live_contract --locked
cargo test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo test -p awiki-cli --test msg_secure_prekey_read_live_contract --locked
```

## Remaining Work

- Slice 12 still needs to migrate FRB/Dart/Flutter callers to async SDK APIs.
- Slice 13 still needs to remove, cfg-gate, or explicitly document the staged
  sync compatibility APIs and blocking legacy boundaries after all upper layers
  have migrated.
