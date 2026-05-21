# 05. CLI Adapter Interface

P1 的目标是让 CLI handler 变成：

```text
parse flags -> build ImCore/ImClient -> call SDK -> render output
```

## 1. Adapter 文件结构

在 `crates/awiki-cli/src/im_adapter/` 增加：

```text
mod.rs
config.rs
paths.rs
identity.rs
messages.rs
error.rs
render.rs
```

这些文件属于 CLI，不属于 `im-core`。

## 2. Config Adapter

```rust
pub fn build_im_core_config(
    resolved: &crate::config::Resolved,
) -> Result<im_core::ImCoreConfig, crate::error::ExitError>;
```

职责：

- 从 CLI config 读取 service base url、did domain、message/user service endpoint。
- 转成 `ServiceEndpoint`。
- 转换 runtime mode 到 `MessageTransportPolicy`。

不在每个 handler 中重复构造 config。

## 3. Paths Adapter

```rust
pub fn build_im_core_paths(
    resolved: &crate::config::Resolved,
    manager: &crate::identity::Manager,
) -> Result<im_core::ImCorePaths, crate::error::ExitError>;
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
    command: &crate::command::ParsedCommand,
) -> Result<im_core::RegisterHandleRequest, crate::error::ExitError>;
```

规则：

```text
empty identity flag -> IdentitySelector::Default
--identity alice    -> IdentitySelector::LocalAlias("alice")
--identity did:...  -> IdentitySelector::Did(...)
```

CLI 继续负责 OTP 输入、alias 文本校验和危险操作确认。

## 5. Message Adapter

```rust
pub fn send_message_request(
    command: &crate::command::ParsedCommand,
    default_domain: &str,
) -> Result<im_core::SendMessageRequest, crate::error::ExitError>;

pub fn inbox_query(
    command: &crate::command::ParsedCommand,
) -> Result<im_core::InboxQuery, crate::error::ExitError>;

pub fn history_request(
    command: &crate::command::ParsedCommand,
    default_domain: &str,
) -> Result<(im_core::ThreadRef, im_core::HistoryQuery), crate::error::ExitError>;
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
) -> crate::error::ExitError;
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

`ImError` 不携带 exit code；exit code 是 CLI 产品策略。

## 7. Handler 示例：msg send

```rust
pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved)?;
    let core = self.build_im_core(&resolved, &manager)?;
    let client = core
        .client(im_adapter::identity::cli_identity_selector(&self.globals.identity))
        .map_err(|err| im_adapter::error::map_im_error(err, "msg send"))?;

    let request = im_adapter::messages::send_message_request(
        command,
        &resolved.did_domain,
    )?;

    if self.globals.dry_run {
        return self.render_msg_send_plan(&resolved, &request);
    }

    let result = client
        .messages()
        .send(request)
        .map_err(|err| im_adapter::error::map_im_error(err, "msg send"))?;

    self.render_im_result("awiki-cli msg send", &resolved, result)
}
```

## 8. Handler 示例：id refresh-token

```rust
pub fn run_id_refresh_token(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved)?;
    let core = self.build_im_core(&resolved, &manager)?;
    let selector = im_adapter::identity::cli_identity_selector(&self.globals.identity);
    let client = core.client(selector).map_err(|err| im_adapter::error::map_im_error(err, "id refresh-token"))?;

    let update = client
        .auth()
        .refresh_session()
        .map_err(|err| im_adapter::error::map_im_error(err, "id refresh-token"))?;

    self.render_im_result("awiki-cli id refresh-token", &resolved, update)
}
```

## 9. Handler 示例：id register

```rust
pub fn run_id_register(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let manager = self.identity_manager(&resolved)?;
    let core = self.build_im_core(&resolved, &manager)?;

    let request = im_adapter::identity::register_handle_request(command)?;

    if self.globals.dry_run {
        return self.render_id_register_plan(&resolved, &request);
    }

    let result = core
        .identities()
        .register_handle(request)
        .map_err(|err| im_adapter::error::map_im_error(err, "id register"))?;

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
