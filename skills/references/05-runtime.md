# Runtime Reference

## Purpose

Use this reference when you are handling runtime selection and long-connection delivery tasks in `awiki-cli`, including runtime mode inspection, websocket listener control, host-notification configuration, and the current heartbeat limitations.

This file is a **reference**, not an entry skill. Load it only when the task clearly involves runtime mode, listener, websocket transport, host notification, or runtime recovery.

## Current Status

- Status: **partially implemented**
- Currently implemented:
  - `runtime status`
  - `runtime apply`
  - `runtime setup`
  - `runtime mode get`
  - `runtime mode set`
  - `runtime listener status/install/start/stop/restart/uninstall`
  - `runtime listener config show/set`
  - `runtime listener enable/disable`
  - `runtime host-notify config show/set`
  - `runtime host-notify enable/disable`
  - `runtime host-notify openclaw set/set-token/clear-token`
  - `runtime host-notify openclaw route add/list/remove`
- Planned but not yet implemented:
  - `runtime heartbeat status/install/run-once`

## Current Behavior Notes

- When the listener service is missing, `runtime listener start` automatically installs the service
- `runtime setup` and `runtime mode set` apply runtime policy after writing configuration; in websocket mode, if the listener is enabled and auto-install/auto-start are enabled, they may install and start the listener service
- `runtime listener install` still exists as an explicit install-only path
- Do not describe heartbeat as already implemented in the current repository

## When to Use

- Inspect the runtime mode
- Switch between `http` and `websocket`
- Control real-time listener and host-notification settings
- Understand the current heartbeat contract and its limitations

## Core Concepts

- **runtime mode**: the transport selection exposed only by the runtime domain
- **listener**: the long-running process used on the websocket side
- **daemon bridge**: the local process boundary used in websocket mode
- **host notify**: normalized websocket events forwarded to `log`, `file`, or `openclaw`
- **heartbeat**: a reserved but not yet implemented timed reliability path

## Decision Rules

- Need to know the current transport state -> `runtime status` or `runtime mode get`
- Need to converge runtime and listener state based on the current `config.yaml` -> `runtime apply`
- Need to initialize runtime files and the local store -> `runtime setup --mode <http|websocket>`
- Need to change persistent listener policy -> use `runtime listener config show/set`
- Need to enable or disable listener management and apply runtime state -> use `runtime listener enable` or `runtime listener disable`
- Need websocket real-time receiving -> set websocket mode first, then use listener commands
- Need host/webhook notifications -> inspect `runtime host-notify config show` first, then set the sink or use `runtime host-notify enable`
- messaging returns `transport-unavailable` -> inspect listener state, or switch back to `http`
- Need heartbeat automation -> explain that this command family is still in the planning stage in the current repository

## Canonical Commands

Currently implemented:

- `awiki-cli runtime status`
- `awiki-cli runtime apply`
- `awiki-cli runtime setup --mode http|websocket`
- `awiki-cli runtime mode get`
- `awiki-cli runtime mode set <http|websocket>`
- `awiki-cli runtime listener status`
- `awiki-cli runtime listener install`
- `awiki-cli runtime listener start`
- `awiki-cli runtime listener stop`
- `awiki-cli runtime listener restart`
- `awiki-cli runtime listener uninstall`
- `awiki-cli runtime listener config show`
- `awiki-cli runtime listener config set [--enabled true|false] [--auto-install true|false] [--auto-start true|false]`
- `awiki-cli runtime listener enable`
- `awiki-cli runtime listener disable`
- `awiki-cli runtime host-notify config show`
- `awiki-cli runtime host-notify config set --sink noop|log|file|openclaw`
- `awiki-cli runtime host-notify enable`
- `awiki-cli runtime host-notify disable`
- `awiki-cli runtime host-notify openclaw set --hook-url <url>`
- `awiki-cli runtime host-notify openclaw set-token --value <token>`
- `awiki-cli runtime host-notify openclaw clear-token`
- `awiki-cli runtime host-notify openclaw route add --channel <channel> --to <target>`
- `awiki-cli runtime host-notify openclaw route add --session-key <session-key>`
- `awiki-cli runtime host-notify openclaw route list`
- `awiki-cli runtime host-notify openclaw route remove --channel <channel> --to <target>`
- `awiki-cli runtime host-notify openclaw route remove --session-key <session-key>`

## Common Patterns

### Initialize WebSocket Mode

1. `awiki-cli runtime status`
2. `awiki-cli runtime setup --mode websocket --dry-run`
3. `awiki-cli runtime setup --mode websocket`
4. `awiki-cli runtime listener status`

Under the default websocket listener policy, step 3 may already have installed and started the listener service.

### Converge Runtime State from the Current Configuration

1. `awiki-cli runtime status`
2. `awiki-cli runtime apply --dry-run`
3. `awiki-cli runtime apply`

### Persistently Disable Listener Auto-Start

1. `awiki-cli runtime listener config show`
2. `awiki-cli runtime listener config set --auto-install false --auto-start false --dry-run`
3. `awiki-cli runtime listener config set --auto-install false --auto-start false`

### Recover from Transport Problems

1. `awiki-cli runtime listener status`
2. `awiki-cli runtime listener restart`
3. If still blocked, run `awiki-cli runtime mode set http`

### Explicitly Enable Host Notifications

1. `awiki-cli runtime host-notify config show`
2. `awiki-cli runtime host-notify config set --sink openclaw --dry-run`
3. `awiki-cli runtime host-notify config set --sink openclaw`
4. If OpenClaw hooks have token validation enabled, but you do not want to rely on auto-detection from `hooks.token` in `~/.openclaw/openclaw.json`: `awiki-cli runtime host-notify openclaw set-token --value <token>`
5. Performed by the host agent:
   - `awiki-cli runtime host-notify openclaw route add --session-key <session-key>`
   - or `awiki-cli runtime host-notify openclaw route add --channel <channel> --to <target>`

## Side Effects and Confirmation

- Require explicit confirmation:
  - `runtime apply`
  - `runtime setup`
  - `runtime mode set`
  - `runtime listener install/start/stop/restart/uninstall`
  - `runtime listener config set`
  - `runtime listener enable/disable`
  - `runtime host-notify enable/disable`
  - `runtime host-notify config set`
  - `runtime host-notify openclaw set/set-token/clear-token`
- Internal-only:
  - `runtime listener run`
  - `runtime listener service-run`

## Error Handling

- The runtime mode is unclear -> check `awiki-cli schema runtime mode set`
- The listener state is unclear -> `awiki-cli runtime listener status`
- The host-notify configuration is unclear -> `awiki-cli runtime host-notify config show`
- The configuration or path is unclear -> `awiki-cli config show`
- More general runtime failures -> `awiki-cli doctor`

## Implementation Notes

- Business commands should not choose transport directly
- `runtime apply` performs runtime bootstrap based on the current configuration and may trigger service-state changes caused by listener policy
- `runtime listener start` now installs the service automatically when needed
- `runtime listener config show/set` is the persistent control plane for `enabled`, `auto_install`, and `auto_start`
- `runtime host_notify.enabled` is enabled by default, while the default sink remains `log`
- `runtime host-notify config show` displays whether an OpenClaw token is configured, the registered routes, the auto-detected webhook port, and the final effective `hook_url`
- The OpenClaw adapter now keeps only the pure webhook path and performs fan-out to registered routes based on the local route registry
- OpenClaw is the recommended host integration path for `host-notify`; see `00-installation.md`
- `runtime heartbeat` is still in the planning stage in the current repository

## Related References

- `03-messaging.md`
- `01-onboarding.md`
- `08-debug.md`
- `00-installation.md`
