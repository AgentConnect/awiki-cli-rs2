# Track B: Identity and Auth Boundary Cleanup

**并行分支**：`cutover/thin-shell-track-b-identity-auth`  
**依赖**：可直接开始。  
**目标**：让默认 identity/auth 命令只通过 `im-core` identity/auth/directory public API 执行；旧 `awiki-cli::identity` 只保留 gated migration helper，不能再作为默认业务流程或 adapter fallback。

---

## 1. 范围

本 track 处理：

```text
id.status
id.list
id.current
id.use
id.register
id.bind
id.refresh-token
id.resolve
id.recover
id.profile.get
id.profile.set
id.replace-did dry-run / diagnostic path
im_core_adapter identity/auth/core/paths boundary
```

主要文件：

```text
crates/awiki-cli/src/app.rs
crates/awiki-cli/src/app/id_recover_handlers.rs
crates/awiki-cli/src/app/id_replace_did_handlers.rs
crates/awiki-cli/src/im_core_adapter/identity.rs
crates/awiki-cli/src/im_core_adapter/auth.rs
crates/awiki-cli/src/im_core_adapter/core.rs
crates/awiki-cli/src/im_core_adapter/paths.rs
crates/awiki-cli/src/im_core_adapter/active_identity.rs
crates/awiki-cli/src/im_core_adapter/message_result.rs
crates/awiki-cli/src/identity/*
crates/im-core/src/identity/*
crates/im-core/src/auth/*
crates/im-core/src/directory/*
crates/im-core/tests/*identity*
```

不处理：

```text
message/group command behavior
runtime listener projection
store module deletion
mail/page/site cleanup
```

---

## 2. 边界目标

允许 `awiki-cli` 做：

```text
--identity flag -> IdentitySelector
Resolved paths -> ImCorePaths
config endpoints -> ImCoreConfig
id.use 的 default identity file write, if im-core exposes a plan/commit boundary
dry-run and render
migration-only legacy import/create behind gate
```

不允许默认路径做：

```text
identity::service::load_identity_for_mutation as business selector
identity::client / identity::wire remote RPC
authsdk Session as default auth/session path
manual JWT refresh fallback outside im-core auth
legacy layout scan as required success path
```

---

## 3. 执行步骤

### B1. Build boundary 去旧 Manager 业务依赖

目标 shape：

```text
build_im_core(resolved) -> ImCore
build_im_client(resolved, selector) -> ImClient
```

`awiki-cli` 可以继续从 `Resolved.paths` 组装 `ImCorePaths`，但不应为了 build client 调用旧 identity business flow。

检查：

```bash
rg "identity::Manager|crate::identity::Manager|identity::service|identity::client|identity::wire" \
  crates/awiki-cli/src/im_core_adapter/core.rs \
  crates/awiki-cli/src/im_core_adapter/paths.rs \
  crates/awiki-cli/src/im_core_adapter/auth.rs \
  crates/awiki-cli/src/im_core_adapter/identity.rs
```

允许短期残留：

```text
id.create
id.import-v1
replace-did migration/diagnostic plan
legacy warning scan, if not required for command success
```

### B2. id.refresh-token 只走 client.auth()

目标：

```text
CLI resolves identity selector
build ImClient
client.auth().refresh_session()
render result
```

禁止：

```text
identity::service::load_identity_for_mutation
manual auth file read/write outside im-core
identity::wire service error conversion as default path
```

### B3. id.register / recover / bind / profile 只走 im-core

要求：

```text
id.register -> core.identities().register_handle(...)
id.recover -> core.identities().recover_handle(...) or im-core recovery command API
id.bind -> client.identity().bind_contact(...)
id.profile.get self -> client.identity().profile()
id.profile.get public -> client.directory().public_profile(...)
id.profile.set -> client.identity().update_profile(...)
id.resolve -> client.directory().resolve_peer(...) / lookup_handle(...)
```

CLI 保留：

```text
flag validation
markdown-file read
dry-run
render
ExitError mapping
```

### B4. id.use 默认写入边界

文档允许 `id.use` 由 CLI 写 default file，但 plan/validation 应通过 `im-core`：

```text
core.identities().plan_default_identity_change(...)
CLI writes selected default identity file, or calls im-core commit API if available
render result
```

不要通过旧 `identity::store` 重新实现 registry selection。

### B5. id.create / id.import-v1 / id.replace-did 降级为 gated

策略：

```text
id.create: hidden migration helper; not default product identity creation.
id.import-v1: migration-only gate.
id.replace-did: diagnostic/advanced hidden; dry-run allowed; execution only if stable im-core API exists.
```

如果某能力还没有 `im-core` API：

```text
返回 unsupported_cutover_command
不回退旧 awiki-cli identity execution
```

### B6. message adapter active identity 依赖移除准备

`im_core_adapter/active_identity.rs` 和 `message_result.rs` 里把旧 identity error 类型转换成 message adapter error。Track B 需要提供替代：

```text
ImClient current_identity / identity summary as source of truth
ImError -> MessageAdapterError / ExitError mapping
```

Track C 合并后再彻底删除 message adapter 里的旧 identity 参数。

---

## 4. 验证

最小验证：

```bash
cargo test -p im-core
cargo check -p awiki-cli
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" \
  crates/im-core/src crates/im-core/tests
```

Track B 静态检查：

```bash
rg "identity::service|identity::client|identity::wire|crate::authsdk|crate::anpsdk" \
  crates/awiki-cli/src/app.rs \
  crates/awiki-cli/src/app/id_recover_handlers.rs \
  crates/awiki-cli/src/app/id_replace_did_handlers.rs \
  crates/awiki-cli/src/im_core_adapter
```

推荐测试：

```bash
cargo test -p awiki-cli --test identity_im_core_mvp_contract
cargo test -p awiki-cli --test identity_live_contract
cargo test -p im-core --test phase2_identity_directory
```

如果 live/system 环境不可用，不阻塞本 track；记录未运行原因。

---

## 5. 完成定义

本 track 完成后：

```text
1. 默认 id.* / auth 命令不再调用旧 identity service/client/wire。
2. build ImCore/ImClient 不需要旧 identity Manager 业务流程。
3. 旧 identity 模块只剩 migration/diagnostic 或待 Final 删除的兼容 wrapper。
4. im-core 仍不引用 CLI 类型。
5. Track C/D 可以基于新的 identity/auth boundary 删除 message/runtime 里的旧 identity 参数。
```

