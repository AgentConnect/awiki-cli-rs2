# SDK Refactor 2：CLI 与 im-core 边界

## 1. 目标

CLI 的目标不是消失，而是变瘦：

```text
CLI handler = parse flags -> build ImCore/ImClient -> call SDK -> render output
```

业务规则、目标解析、auth retry、本地状态合并、message/group 投影应该迁到 `im-core`。CLI 保留命令行 UX、本机路径和输出。

## 2. CLI 保留职责

| 职责 | 是否留在 CLI | 说明 |
| --- | --- | --- |
| 命令解析 | 是 | `ParsedCommand`、flag、args、alias、completion。 |
| 全局参数 | 是 | `--identity`、`--format`、`--dry-run`、`--verbose`。 |
| config/workspace 解析 | 是 | SDK 不读取 CLI config，不自动发现 workspace。 |
| 路径布局 | 是 | identity root、default identity 文件、SQLite path、auth path、key path、runtime path。 |
| 文件权限/备份 | 是 | chmod、backup、atomic write strategy。 |
| 输出渲染 | 是 | pretty/table/json、jq、trace、warning。 |
| exit code | 是 | `ImError` -> `ExitError` 映射。 |
| dry-run 展示 | 第一阶段留在 CLI | 避免一开始把 plan system 也搬进 SDK。 |
| daemon/service | 是 | systemd/launchd/Windows service、pid/log/socket。 |
| OpenClaw/Hermes | 是 | 属于 CLI runtime/host notify UX。 |
| Debug SQL | 是 | `debug.db.*` 不属于 SDK default API。 |

## 3. im-core 承担职责

| 职责 | 说明 |
| --- | --- |
| 多身份 registry | list/default/resolve/load identity summary。 |
| 绑定身份 | `core.client(selector)` 自动绑定 actor、auth、local owner。 |
| auth/session | login、ensure、refresh、logout、401 retry。 |
| profile/directory | profile、handle/DID resolve、contacts、relation status。 |
| 私聊 | direct send、inbox、history、mark-read、conversation projection。 |
| 群聊 | create/get/list/join/leave/member/update/messages。 |
| 本地状态 | 初始化/迁移、cache merge、owner isolation。 |
| 底层实现 | HTTP/RPC、wire params、DID proof、SQLite helper 作为内部实现。 |

## 4. 第一阶段命令映射

| 当前 CLI 命令 | 目标 SDK API | CLI 保留 |
| --- | --- | --- |
| `id list` | `core.identities().list()` | table/pretty/json 渲染。 |
| `id current` | `core.identities().default_identity()` | 输出默认身份。 |
| `id use` | `core.identities().plan_default_identity_change()` | 写 default 文件、提示。 |
| `id status` | `core.identities().list()` + readiness | 输出状态和缺失项。 |
| `id register` | `core.identities().register_handle()` | OTP 输入、identity alias、路径和权限。 |
| `id recover` | `core.identities().recover_handle()` | 本地目标路径、旧状态合并策略。 |
| `id refresh-token` | `client.auth().refresh_session()` | 输出和错误 hint。 |
| `id resolve` | `client.directory().resolve_peer()` 或 `core.identities().resolve()` | 参数解析、输出。 |
| `id profile get` | `client.identity().profile()` | 输出。 |
| `id profile set` | `client.identity().update_profile()` | markdown-file 读取、patch 构造。 |
| `msg send` | `client.messages().send()` | `--to/--group/--text/--text-file` 解析、dry-run。 |
| `msg inbox` | `client.messages().inbox()` | 输出、limit/scope 参数。 |
| `msg history` | `client.messages().history()` | thread/peer/group 参数解析。 |
| `msg mark-read` | `client.messages().mark_read()` | message id 参数解析。 |
| `group create` | `client.groups().create()` | profile/policy flag 转 DTO。 |
| `group get` | `client.groups().get()` | 输出。 |
| `group list` | `client.groups().list()` | 输出。 |
| `group join` | `client.groups().join()` | reason flag。 |
| `group leave` | `client.groups().leave()` | reason/确认 UX。 |
| `group add` | `client.groups().add_member()` | member/role 参数。 |
| `group remove` | `client.groups().remove_member()` | member 参数。 |
| `group update` | `client.groups().update_profile()` / `update_policy()` | patch 构造。 |
| `group members` | `client.groups().members()` | 输出。 |
| `group messages` | `client.groups().messages()` | 输出。 |

## 5. 第一阶段暂不迁移或降级的命令

| 命令 | 建议 |
| --- | --- |
| `msg.attachment.download` | Phase 2 迁移。第一阶段可继续走旧实现或标记未进入 SDK。 |
| `msg secure *` | Phase 3。第一阶段不进入 SDK default API。 |
| `group e2ee *` | Phase 3/diagnostic。不要作为 SDK 主接口设计依据。 |
| `runtime listener *` | Phase 2 只迁移 runner；service install/start/stop 永远留 CLI。 |
| `runtime host-notify *` | 留 CLI。SDK 只产出领域事件。 |
| `debug.db.*` | 留 CLI。 |
| `id import-v1` | 可以作为迁移工具留 CLI，或后续 `identity::migration` advanced feature。 |
| `id replace-did` | 危险能力，晚于普通 IM 能力迁移。 |

## 6. Handler 目标形态

以 `msg send` 为例：

```rust
pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
    let resolved = self.resolve_config_for_workspace()?;
    let core = self.build_im_core(&resolved)?;
    let client = core.client(cli_identity_selector(&self.globals.identity))?;

    let request = SendMessageRequest {
        target: cli_message_target(command)?,
        body: cli_message_body(command)?,
        security: cli_message_security(command)?,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    };

    if self.globals.dry_run {
        return self.render_msg_send_plan(&resolved, &request);
    }

    let result = client.messages().send(request).map_err(self.im_error("msg send"))?;
    self.render_im_result("awiki-cli msg send", &resolved, result)
}
```

关键变化：

- CLI 不再构造 `message::SendRequest { identity_name, target, group, file_path, ... }`。
- CLI 不再调用 `message::send(&resolved, &manager, request)`。
- CLI 不再关心 HTTP vs WebSocket fallback、auth refresh、local state owner、store_message。
- CLI 只把 CLI 输入翻译成 SDK DTO。

## 7. dry-run 策略

为了减少第一阶段改动，dry-run 先留在 CLI：

```text
CLI dry-run = 根据 ParsedCommand + resolved config + SDK DTO 输出 plan
```

SDK 可以后续增加：

```rust
client.messages().plan_send(&request) -> ImResult<SendMessagePlan>
```

但这不是第一阶段前置条件。

## 8. 错误映射

CLI 增加统一映射：

```rust
fn map_im_error(err: ImError, context: &'static str) -> ExitError
```

示例：

```text
IdentityRequired      -> exit 2, hint: pass --identity or run id use
AuthRequired          -> exit 3, hint: run id refresh-token or login
PeerNotFound          -> exit 4, hint: check handle or DID
TransportUnavailable  -> exit 5, hint: check service endpoint/network
UnsupportedCapability -> exit 2, hint: feature not supported in this phase
```

`ImError` 不包含 CLI exit code；exit code 是 CLI 产品策略。

## 9. 构造 ImCore 的 adapter

CLI 应新增一个集中 adapter：

```rust
impl App {
    pub(super) fn build_im_core(&self, resolved: &Resolved) -> Result<ImCore, ExitError> {
        let config = build_im_core_config(resolved)?;
        let paths = build_im_core_paths(resolved, &self.identity_manager(resolved))?;
        ImCore::new(config, paths).map_err(map_im_error)
    }
}
```

不要在每个 handler 中重复拼 `ImCorePaths`。

## 10. 命令不等于 SDK API

某些 CLI 命令可以很低层，例如 debug、diagnostic、secure repair、group E2EE testing。但 SDK public API 不应该跟着低层化。

CLI 可以有：

```text
group e2ee publish-key-package
msg secure retry
debug db query
runtime listener service-run
```

SDK 默认 public API 不应该有：

```rust
publish_key_package(...)
build_group_e2ee_add_rpc_params(...)
execute_sql(...)
process_websocket_frame(...)
```

这些只应在 internal、diagnostic feature 或 CLI-only module 中出现。
