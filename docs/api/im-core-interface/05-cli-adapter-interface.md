# 05. CLI Adapter Interface

P1 的目标是让 CLI handler 变成：

```text
parse flags -> build ImCore/ImClient -> call SDK -> render output
```

## 1. Adapter 文件结构

在 `crates/awiki-cli/src/im_core_adapter/` 增加：

```text
mod.rs
config.rs
paths.rs
identity.rs
messages.rs
error.rs
render.rs
```

这些文件属于 CLI，不属于 `im-core`。目录名统一使用 `im_core_adapter`，不要使用 `im_adapter`，避免和其他 adapter 混淆。

## 2. Config Adapter

```rust
pub fn build_im_core_config(
    resolved: &crate::config::Resolved,
) -> Result<im_core::ImCoreConfig, crate::output::ExitError>;
```

职责：

- 从 CLI config 读取 service base url、did domain、message/user service endpoint。
- 转成 `ServiceEndpoint`。
- 转换 runtime mode 到 `MessageTransportPolicy`。

不在每个 handler 中重复构造 config。

### 2.1 Secret Storage Config

CLI identity SecretVault settings live under workspace `secret_storage`.
Supported fields:

```yaml
secret_storage:
  mode: file_compat        # file_compat | vault_preferred | vault_required
  vault_dir: .awiki/identity-vault
  workspace_id: cli-workspace-id
  device_id: cli-device-id
```

The vault root key is never written to `config.yaml`. It is read first from
`AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`, which must contain a base64/base64url
encoded 32-byte root key. If the env var is absent, CLI reads
`vault_dir/root-key.b64u`; normal live SDK opens and registration/recovery paths
may create that local private file, while status and dry-run mutation surfaces
only report the redacted plan. `config show`, diagnostics, JSON output, human
output, errors, and dry-run plans must report only whether the root key is
available and which source would be used.

`build_im_core(_async)` resolves `secret_storage` into `ImCoreOpenOptions`:

- `file_compat` opens the SDK without vault options.
- `vault_preferred` passes vault options when the root key is available and is a
  migration-period mode.
- `vault_required` passes `IdentitySecretStoragePolicy::VaultRequired`; normal
  SDK opens and mutation paths fail closed when the root key is invalid or
  cannot be created/read. `id vault status` may use a redacted
  `checked_without_vault_context` mode for diagnostics.

The adapter also maps the local rollout environment
`AWIKI_MULTI_DEVICE_JOIN_ENABLED` to
`ImCoreOpenOptions.multi_device_join_enabled`. Unset/`0` is disabled; only `1`
enables the path, and malformed values fail closed. This is a host rollout
switch, not an ANP field.

## 3. Paths Adapter

```rust
pub fn build_im_core_paths(
    resolved: &crate::config::Resolved,
    manager: &crate::identity::Manager,
) -> Result<im_core::ImCorePaths, crate::output::ExitError>;
```

职责：

- 使用现有 CLI workspace 和 identity manager 规则确定路径。
- 填充 `IdentityRegistryPaths`、`LocalStatePaths`、`RuntimePaths`。
- 处理目录创建、权限检查、可写性检查。

SDK 不直接读取 CLI workspace，不直接依赖 `Manager`。

## 4. Identity Adapter

```rust
pub fn cli_identity_selector(identity_flag: &str) -> im_core::IdentitySelector;

pub fn register_handle_request(
    command: &crate::cli::ParsedCommand,
) -> Result<im_core::RegisterHandleRequest, crate::output::ExitError>;
```

规则：

```text
empty identity flag -> IdentitySelector::Default
--identity alice    -> IdentitySelector::LocalAlias("alice")
--identity did:...  -> IdentitySelector::Did(...)
```

CLI 继续负责 OTP 输入、alias 文本校验和危险操作确认。

### 4.1 Device Join adapter

The advanced `id device ...` commands are gated before workspace/Core open.
`id device join start` reads the short-lived account verification grant only
from `AWIKI_ACCOUNT_VERIFICATION_TOKEN`; it is never accepted in argv or
rendered in output. Session list/start/poll/cancel call the corresponding Core
facade, while `id device list` and admin claim/poll use the selected identity.

`id device join approve` requires a foreground TTY. The user types the locally
derived SAS and an explicit approval word; the adapter then prepares and
consumes the one-time approval handle in the same process. The handle is never
returned by the CLI. JSON/pretty output uses only the safe host projections and
must not contain tokens, pairing/private/root material, challenge ciphertext,
or internal Document/Registry/auth versions and hashes.

### 4.2 Root-key transfer adapter

`id device root-key send --device <id> --message-id <id>` is guarded by the
independent `AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED=1` rollout flag before
workspace/Core open. It rejects dry-run and non-interactive input. In a
foreground TTY the user must re-enter the exact recipient device ID and type
`TRANSFER`; there is no argv user-presence bypass, root material flag, inner
JSON flag, or separate `transfer_id`.

The command calls the Core facade and renders delivery acceptance metadata
only. It must state that mailbox acceptance is not import completion and must
never output root bytes, encrypted control payloads, private sidecars,
completion proofs, or internal checkpoints.

For a device pair without an established P5 v2 session, the first `send`
delivers only the fixed session Init and returns
`p5-v2-session-establishment-pending`; it has not opened or persisted a root
Envelope. After both devices sync the Init/reply, the operator repeats `send`
with the same `--device` and `--message-id` and confirms user presence again.
The pre-Envelope handshake is intentionally absent from `root-key list`, and
`root-key retry` becomes applicable only after a root-control sidecar exists.

`id device root-key list [--include-completed]` reads Core's owner-scoped,
restart-safe status projection. Its output is limited to DID, standard
`message_id`, sender/recipient device IDs, status, timestamps, and `retryable`.
It never opens the persisted root Envelope or exposes internal versions/hashes.

`id device root-key retry --message-id <id>` requires a foreground TTY and an
explicit `RETRY` confirmation. It accepts no recipient, secret, sidecar, inner
JSON, or user-presence override flags. Core derives the route from the exact
persisted operation selected by the original `message_id` and rejects expired,
completed, unknown, or otherwise non-retryable entries.

## 5. Message Adapter

```rust
pub fn send_message_request(
    command: &crate::cli::ParsedCommand,
    default_domain: &str,
) -> Result<im_core::SendMessageRequest, crate::output::ExitError>;

pub fn inbox_query(
    command: &crate::cli::ParsedCommand,
) -> Result<im_core::InboxQuery, crate::output::ExitError>;

pub fn history_request(
    command: &crate::cli::ParsedCommand,
    default_domain: &str,
) -> Result<(im_core::ThreadRef, im_core::HistoryQuery), crate::output::ExitError>;
```

CLI 负责：

```text
--to / --group 互斥校验
--text / --text-file 读取和互斥校验
--limit / --cursor 解析
--secure / --file 的 P1 unsupported 提示
```

SDK 负责：

```text
target resolve
auth ensure / refresh retry
RPC/wire params
远端结果 normalize
必要本地状态
```

## 6. Error Adapter

```rust
pub fn map_im_error(
    err: im_core::ImError,
    context: &'static str,
) -> crate::output::ExitError;
```

建议映射：

| `ImError` | CLI exit/hint |
| --- | --- |
| `IdentityRequired` | 提示 `--identity` 或 `id use`。 |
| `DefaultIdentityMissing` | 提示先注册或选择默认身份。 |
| `AuthRequired` / `SessionExpired` | 提示 `id refresh-token`。 |
| `PeerNotFound` | 提示检查 handle/DID。 |
| `GroupNotFound` | 提示检查 group DID/id。 |
| `UnsupportedCapability` | 提示该能力不在 Phase 1。 |
| `TransportUnavailable` | 提示检查 endpoint/network。 |
| `PathUnavailable` | 提示检查 workspace/path/permission。 |
| secret vault local-state errors | 映射到 `vault_root_key_required`、`vault_root_key_invalid` 或 `vault_not_ready`，并提示 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` 或本地私有 root-key 文件。 |

`ImError` 不携带 exit code；exit code 是 CLI 产品策略。

## 7. Handler 示例：msg send

```rust
pub fn run_msg_send(&self, command: &crate::cli::ParsedCommand) -> Result<(), crate::output::ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved);
    let core = self.build_im_core(&resolved, &manager)?;
    let client = core
        .client(im_core_adapter::identity::cli_identity_selector(&self.globals.identity))
        .map_err(|err| im_core_adapter::error::map_im_error(err, "msg send"))?;

    let request = im_core_adapter::messages::send_message_request(
        command,
        &resolved.did_domain,
    )?;

    if self.globals.dry_run {
        return self.render_msg_send_plan(&resolved, &request);
    }

    let result = client
        .messages()
        .send(request)
        .map_err(|err| im_core_adapter::error::map_im_error(err, "msg send"))?;

    self.render_im_result("awiki-cli msg send", &resolved, result)
}
```

## 8. Handler 示例：id refresh-token

```rust
pub fn run_id_refresh_token(&self) -> Result<(), crate::output::ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved);
    let core = self.build_im_core(&resolved, &manager)?;
    let selector = im_core_adapter::identity::cli_identity_selector(&self.globals.identity);
    let client = core
        .client(selector)
        .map_err(|err| im_core_adapter::error::map_im_error(err, "id refresh-token"))?;

    let update = client
        .auth()
        .refresh_session()
        .map_err(|err| im_core_adapter::error::map_im_error(err, "id refresh-token"))?;

    self.render_im_result("awiki-cli id refresh-token", &resolved, update)
}
```

## 9. Handler 示例：id register

```rust
pub fn run_id_register(&self, command: &crate::cli::ParsedCommand) -> Result<(), crate::output::ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved);
    let core = self.build_im_core(&resolved, &manager)?;

    let request = im_core_adapter::identity::register_handle_request(command)?;

    if self.globals.dry_run {
        return self.render_id_register_plan(&resolved, &request);
    }

    let result = core
        .identities()
        .register_handle(request)
        .map_err(|err| im_core_adapter::error::map_im_error(err, "id register"))?;

    self.render_im_result("awiki-cli id register", &resolved, result)
}
```

## 10. P1 CLI Migration Order

建议顺序：

```text
1. build_im_core_config / build_im_core_paths / cli_identity_selector / map_im_error
2. id list / id current / id status
3. id refresh-token
4. id register
5. msg send --to
6. msg send --group
7. msg inbox / msg history P1 subset
```

不要在 P1 改：

```text
msg secure *
group e2ee *
msg attachment *
group lifecycle commands
runtime listener service commands
debug.db.*
```

## 10.1 Identity Vault Commands

The CLI exposes identity vault inspection and migration surfaces:

- `id vault status`: shows resolved open options, root-key availability, selected
  backend, migration metadata state, plaintext compatibility retention,
  warnings, and missing items. If root key is absent, the status can still run in
  a redacted `checked_without_vault_context` mode.
- `id vault migrate`: requires `--migration` and a root key. Dry-run reports the
  planned mutation. Live migration uses the SDK status/migrate API and must fail
  before rewriting metadata when the root key is missing.
- `id vault cleanup-plaintext`: migration-gated/preflight surface. In this build
  the CLI does not have a CLI-safe live plaintext cleanup API, so it must not be
  documented or rendered as deleting legacy compatibility files.

Vault command output must never include root key bytes, JWTs, private PEM,
complete `SecretRef` JSON, ciphertext internals, or bearer tokens. Human output
can show mode/backend/warnings; JSON output should expose redacted status
objects only.

## 11. Name / Avatar 展示字段边界

CLI adapter 的 JSON 输出使用标准展示字段：

- 联系人、关系列表和目录结果应稳定包含 `did`、`handle`、`display_name`、`avatar_uri`、`profile_uri`、`subject_type`。
- 群组结果应稳定包含 `group_did` / `did`、`display_name`、`avatar_uri`，并保留 `group_profile.display_name` / `group_profile.avatar_uri` 作为群组资料权威投影。
- 旧字段 `name`、`avatar`、`avatar_url` 只能作为兼容输出或输入 alias；新逻辑优先读取 `display_name` / `avatar_uri`。
- human summary 可以显示 `display_name`，但必须保留 Handle 或 DID，例如 `Alice (alice.awiki.ai / did:...)`。

CLI 输入兼容规则：

- `people contacts save --display-name` 是标准联系人展示名输入；`--name` 保留为 deprecated alias。
- `id profile set --avatar-uri` 是标准头像输入；`--avatar-url` 保留为 deprecated alias。
- `group create --name` 和 `group update --name` 保留为 CLI 便利输入，内部映射到 `group_profile.display_name`；`--avatar-uri` 映射到 `group_profile.avatar_uri`。

安全边界：

- `display_name`、`avatar_uri`、`profile_uri`、`subject_type`、`name`、`avatar` 不得用于路由、身份认证、授权、服务发现、E2EE 绑定或安全 profile 协商。
- Daemon runtime inbox 返回的 `title` 和 `display` 对象只是 UI fallback metadata；响应必须同时保留 `peer_did` 或 `group_did`，App 可以用这些 DID 从 SDK profile cache 进行后续水化。
- Daemon runtime agent 的 `display_name` 是本机 runtime 管理名，不是公开 DID Subject Profile；公开联系人或 Agent 展示资料仍应来自 WNS / User Service profile。
