# Track A: Surface Adapters and Already-Cutover Domains Cleanup

**并行分支**：`cutover/thin-shell-track-a-surface-adapters`  
**依赖**：可直接开始。  
**目标**：清理已经有 `im-core` public service 的轻量命令面和 adapter，让 `mail`、`page/site/content`、`people.contacts/relationships` 默认路径只剩 CLI 参数处理、dry-run、render 和 error mapping。

---

## 1. 范围

本 track 处理：

```text
mail.*
page.*
site.*
people.follow / unfollow / status / followers / following
people.contacts.list / save
这些命令的 cmdmeta/schema/default surface 小清理
```

主要文件：

```text
crates/awiki-cli/src/app/mail_handlers.rs
crates/awiki-cli/src/app/page_handlers.rs
crates/awiki-cli/src/app/site_handlers.rs
crates/awiki-cli/src/app/people_handlers.rs
crates/awiki-cli/src/im_core_adapter/email.rs
crates/awiki-cli/src/im_core_adapter/content.rs
crates/awiki-cli/src/im_core_adapter/site.rs
crates/awiki-cli/src/im_core_adapter/people.rs
crates/awiki-cli/src/cmdmeta/mod.rs
crates/awiki-cli/src/mail/*
crates/awiki-cli/tests/*mail*
crates/awiki-cli/tests/*content*
crates/awiki-cli/tests/*site*
crates/awiki-cli/tests/*people*
crates/im-core/tests/*email*
crates/im-core/tests/*content*
crates/im-core/tests/*site*
crates/im-core/tests/*directory*
```

不处理：

```text
runtime listener projection
message/group/secure legacy modules
identity/auth manager cleanup
store module deletion
Cargo dependency deletion
```

---

## 2. 当前判断

当前 `im-core` 已有：

```text
client.email()
client.content()
client.site()
client.directory().follow/unfollow/relationship_status/followers/following
client.directory().contacts/save_contact
```

因此这些 CLI 命令不应再依赖旧 awiki-cli business 模块。

`awiki-cli` 当前已经没有旧 `content` / `site` module，说明 page/site 的主迁移已完成。剩余工作集中在：

```text
1. dry-run 不再暴露 raw RPC endpoint / method。
2. page/site 错误映射不再构造 crate::identity::IdentityError。
3. mail legacy module 从 lib.rs 和源码中删除。
4. people adapter 不做 local projection 或旧 identity fallback。
5. schema default surface 只保留高层产品命令。
```

---

## 3. 执行步骤

### A1. Mail legacy 删除准备

检查：

```bash
rg "crate::mail|use crate::mail\\b|awiki_cli::mail|mail::" \
  crates/awiki-cli/src crates/awiki-cli/tests
```

要求：

```text
1. app/mail_handlers.rs 只能通过 im_core_adapter::email 和 client.email()。
2. old crates/awiki-cli/src/mail/* 不再被生产代码引用。
3. 若测试仍直接测 awiki_cli::mail wire，迁到 crates/im-core/tests/email_* 或删除旧 legacy parity test。
```

完成后可在本 track 删除：

```text
crates/awiki-cli/src/mail/*
crates/awiki-cli/src/lib.rs 中 pub(crate) mod mail
```

如果删除 `lib.rs` 会和其他分支冲突，可以先在本 track 只迁测试和引用，最终删除留给 Final。

### A2. Page/site dry-run 高层化

当前 dry-run 若包含：

```text
rpc_endpoint
rpc_method
/content/rpc
/site/rpc
```

改为高层计划字段：

```text
service: "im-core.content" / "im-core.site"
operation: "page.create" / "site.root.set" / ...
remote_call: "content.create_page" / "site.set_root" / ...
```

禁止：

```text
raw endpoint
JSON-RPC method name
wire params
auth header details
```

### A3. Page/site error mapping 去旧 identity 类型

目标：

```text
page/site handler 不再调用 identity_exit(crate::identity::IdentityError::...)
page/site adapter 返回 ImError 或 CommandResult
统一通过 im_core_adapter::map_im_error / app ExitError 映射
```

检查：

```bash
rg "crate::identity|identity_exit|IdentityError" \
  crates/awiki-cli/src/app/page_handlers.rs \
  crates/awiki-cli/src/app/site_handlers.rs \
  crates/awiki-cli/src/im_core_adapter/content.rs \
  crates/awiki-cli/src/im_core_adapter/site.rs
```

### A4. People adapter 边界检查

`im_core_adapter/people.rs` 允许：

```text
ParsedCommand flags -> im-core DTO
dry-run plan
result rendering
ImError -> ExitError mapping
```

禁止：

```text
crate::store writes
crate::identity business fallback
remote relationship wire builder
local projection
```

检查：

```bash
rg "crate::store|crate::identity::service|crate::identity::client|crate::authsdk|crate::anpsdk|im_core::compat" \
  crates/awiki-cli/src/im_core_adapter/people.rs \
  crates/awiki-cli/src/app/people_handlers.rs
```

### A5. Command surface 检查

确保：

```text
mail/page/site/people.contacts 是 ImCore owner。
people.search 保持 unsupported。
debug.raw 不因本 track 改动进入 default surface。
default schema 不展示 raw RPC / SQL / stub。
```

检查：

```bash
cargo test -p awiki-cli --test cli_shell_core_contract
cargo run -p awiki-cli -- schema --format json
```

如果本地测试 target 名称不同，以现有 `crates/awiki-cli/tests` 为准。

---

## 4. 验证

最小验证：

```bash
cargo test -p im-core
cargo check -p awiki-cli
rg "crate::mail|use crate::mail\\b|awiki_cli::mail" crates/awiki-cli/src crates/awiki-cli/tests
rg "rpc_endpoint|rpc_method|/content/rpc|/site/rpc" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

推荐验证：

```bash
cargo test -p im-core --test email_wire_contract
cargo test -p im-core --test content_wire_contract
cargo test -p im-core --test site_wire_contract
cargo test -p awiki-cli --test mail_contract
cargo test -p awiki-cli --test page_contract
cargo test -p awiki-cli --test site_contract
```

不存在的 test target 不要强行新增到本 track 的完成条件；如果需要新增，保持小范围。

---

## 5. 完成定义

本 track 完成后：

```text
1. mail/page/site/people 默认命令全部通过 im-core public service。
2. awiki-cli 旧 mail module 已删除，或已无引用并交给 Final 删除。
3. page/site dry-run 不再暴露 raw RPC。
4. page/site/people adapter 不依赖旧 identity/store/authsdk/anpsdk 业务路径。
5. default schema 仍只展示高层能力。
```

