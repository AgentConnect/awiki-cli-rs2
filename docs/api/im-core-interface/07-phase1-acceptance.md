# 07. Phase 1 Acceptance

## 1. 编译验收

```bash
cargo test -p awiki-im-core --locked
cargo test -p awiki-cli --locked
cargo fmt --check
cargo run --bin xtask --locked -- check-structure
```

P1A 完成后，`crates/im-core` 必须可独立编译。

## 2. Import Fence 验收

`crates/im-core/src` 中不能出现：

```text
ParsedCommand
GlobalOptions
ExitError
config::Resolved
identity::Manager
crate::app
crate::cli
crate::output
awiki_cli
```

也不能在默认 public API 中出现：

```text
ActorContext
StoredIdentity
ClientIdentityRuntime
IdentityRuntimePaths
build_*_rpc_params
SQLite connection
owner_did as method parameter
identity_name as request field
raw serde_json::Value as Message public field
```

## 3. API Shape 验收

这些代码应能编译：

```rust
use im_core::prelude::*;

fn send_direct(core: &ImCore) -> ImResult<()> {
    let client = core.client(IdentitySelector::LocalAlias("alice".to_string()))?;
    client.auth().ensure_session(AuthScope::Messaging)?;

    let peer = PeerRef::parse("bob", "awiki.info")?;
    client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(peer),
        body: MessageBody::Text { text: "hello".to_string(), kind: MessageKind::Text },
        security: MessageSecurityMode::DefaultPlain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    })?;
    Ok(())
}
```

群聊文本：

```rust
fn send_group(core: &ImCore) -> ImResult<()> {
    let client = core.client(IdentitySelector::Default)?;
    let group = GroupRef::parse("did:example:group")?;
    client.messages().send(SendMessageRequest {
        target: MessageTarget::Group(group),
        body: MessageBody::Text { text: "hello group".to_string(), kind: MessageKind::Text },
        security: MessageSecurityMode::Plain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    })?;
    Ok(())
}
```

Unsupported：

```rust
let result = client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Text { text: "secret".to_string(), kind: MessageKind::Text },
    security: MessageSecurityMode::SecureDirect,
    client_message_id: None,
    delivery: MessageDeliveryOptions::default(),
});
assert!(matches!(result, Err(ImError::UnsupportedCapability { .. })));
```

P1 默认 public API 不应要求这些代码可编译：

```rust
client.groups()
client.attachments()
client.realtime()
client.secure()
```

如果这些 placeholder service 被提前加入，必须在 non-default feature / experimental API 下，并返回 `UnsupportedCapability`。

## 4. CLI 行为验收

P1 迁移后：

```text
id list            -> core.identities().list()
id current         -> core.identities().default_identity()
id status          -> identity readiness via SDK summary
id use             -> plan_default_identity_change + CLI writes default file
id refresh-token   -> client.auth().refresh_session()
id register        -> core.identities().register_handle()
msg send --to      -> client.messages().send(Direct + Text)
msg send --group   -> client.messages().send(Group + Text)
msg inbox          -> client.messages().inbox()
msg history        -> client.messages().history()
```

仍旧留在 CLI 或旧实现：

```text
msg secure *
group e2ee *
msg attachment *
group create/get/list/join/leave/add/remove/update/members/messages
runtime listener service management
debug.db.*
```

## 5. 多身份验收

至少有测试覆盖：

```text
alice 与 bob 使用不同 IdentitySelector::LocalAlias
alice/bob auth state path 不同
alice/bob message send 注入不同 current_did
Default selector 不改变 SDK 全局状态
```

建议 test 名：

```text
identity_selector_local_alias_resolves_runtime
client_binding_keeps_identity_isolated
default_identity_selector_is_not_global_mutation
```

## 6. Path Fixture 验收

建立 tempdir fixture：

```text
temp/identities/
temp/identities/registry.json
temp/identities/default
temp/local/im.sqlite
temp/cache/
temp/tmp/
```

测试：

```text
ImCore::new(config, paths) 不需要 CLI Resolved
core.bootstrap().validate_paths() 返回 PathValidationReport
core.bootstrap().initialize_local_state() 可创建/初始化 sqlite
core.client(selector) 不需要 CLI Manager
```

## 7. Message 验收

Direct：

```text
Text 为空 -> InvalidInput
Attachment -> UnsupportedCapability("attachments")
SecureDirect -> UnsupportedCapability("secure-direct")
GroupE2ee -> UnsupportedCapability("group-e2ee")
Plain direct text -> 调用 internal sender
Session expired -> refresh once and retry
```

Group：

```text
Plain group text -> 调用 internal group sender
GroupRef 为空/非法 -> InvalidInput
P1 不要求 create/join/list group
```

Inbox/history：

```text
limit 归一化
cursor 透传
remote response normalize 成 Page<Message>
不强制 conversation projection
不返回默认 raw JSON payload
```

## 8. 第一阶段完成标准

Phase 1 完成后必须满足：

- `crates/im-core` 存在且可独立测试。
- CLI P1 命令通过 SDK façade 调用。
- SDK public API 不含 CLI 类型、不含底层 wire/store/crypto 类型。
- 身份绑定通过 `ImClient` 完成，业务 request 不含 `identity_name`。
- paths 只出现在 `ImCore::new` / `bootstrap` / internal runtime，不出现在业务方法参数。
- secure/attachment/group lifecycle/realtime 未进入 default SDK public API。
- App 可通过显式 paths 构造 `ImCore` 并构造 P1 message request。
