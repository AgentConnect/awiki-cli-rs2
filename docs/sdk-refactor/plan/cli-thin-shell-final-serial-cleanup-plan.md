# Final Serial Track: Legacy Module Deletion and Gate Burn-down

**串行分支**：`cutover/thin-shell-final-serial-cleanup`  
**依赖**：必须等 Track A/B/C/D 合并后执行。  
**目标**：删除共享 legacy 模块、清理 Cargo 依赖、收紧 command surface、移除静态 allowlist，完成 `awiki-cli` 薄壳化。

---

## 1. 进入条件

开始前确认：

```text
Track A 完成:
  mail/page/site/people 默认路径只走 im-core，旧 mail 可删除。

Track B 完成:
  identity/auth 默认路径只走 im-core，旧 identity business 只剩 migration/diagnostic。

Track C 完成:
  msg/group/attachment/secure 默认路径只走 im-core，旧 message module 已无默认引用。

Track D 完成:
  runtime listener run/service-run 只宿主 im-core runner，普通 IM local_state projection 不在 CLI runtime。
```

检查：

```bash
git status --short
cargo test -p im-core
cargo check -p awiki-cli
```

---

## 2. 删除顺序

### F1. 删除旧 business modules

候选删除：

```text
crates/awiki-cli/src/mail/*
crates/awiki-cli/src/message/*
crates/awiki-cli/src/store/messages.rs
crates/awiki-cli/src/store/groups.rs
crates/awiki-cli/src/store/contacts.rs
crates/awiki-cli/src/store/e2ee_outbox.rs
crates/awiki-cli/src/store/schema.rs
crates/awiki-cli/src/store/types.rs
```

保留或重命名为 migration/diagnostic：

```text
crates/awiki-cli/src/store/import.rs
crates/awiki-cli/src/store/rebind.rs
crates/awiki-cli/src/store/recover_merge*
debug.db.import-v1 support code
id.import-v1 support code
upgrade migration support code
doctor read-only diagnostics if still needed
```

如果保留 `store` root，只能表达 migration/diagnostic，不应再被默认 IM path 引用。

### F2. 删除或收敛旧 identity module

候选删除：

```text
identity/client.rs
identity/wire.rs
identity/service.rs default business functions
identity/recover.rs if im-core recovery covers it
identity/replace_did.rs if im-core replace-did plan/execution covers it
```

允许保留：

```text
identity/types.rs only if migration code still needs legacy record decoding
identity/legacy.rs for migration gate
identity/layout.rs for migration gate
identity/key_compat.rs only if migration/import still needs it
```

Final 不应让普通 `id.*` command depend on these.

### F3. 删除 authsdk/anpsdk/transportcfg IM dependencies

检查：

```bash
rg "crate::authsdk|crate::anpsdk|crate::transportcfg" crates/awiki-cli/src
```

策略：

```text
1. 如果只被 old identity/message/mail 使用，删除。
2. 如果被 update/OpenClaw/Hermes 使用，迁到 CLI-owned http helper 或保留为 runtime/update helper，不作为 IM SDK helper。
3. 不允许 msg/group/id/mail 默认 path 继续引用。
```

### F4. 清 lib.rs module exports

删除不再需要的 exports：

```text
pub mod message;
pub(crate) mod mail;
pub mod authsdk;
pub mod anpsdk;
pub mod transportcfg;  # if no CLI-owned user remains
pub mod store;         # if migration/diagnostic code also removed or renamed
pub mod identity;      # if no migration/diagnostic code remains
```

如果保留 migration/diagnostic module，改名要明确，例如：

```text
legacy_identity_migration
legacy_store_migration
cli_http
```

避免继续以 `identity` / `store` 命名承载默认 IM 业务含义。

---

## 3. Command surface 收口

默认 surface 必须只包含：

```text
CLI-owned:
  status, docs, schema, doctor, version, upgrade, init, completion, config.show/set

im-core-backed:
  id.*, msg.*, mail.*, group.*, people.*, page.*, site.*
  runtime listener run/service-run only as internal service entries

CLI runtime UX:
  runtime.status/apply/setup/mode/listener service management
  runtime.host-notify high-level config/enable/disable/status/setup
```

默认 surface 不得包含：

```text
debug.raw.*
debug.db.query
raw SQL
raw RPC
group.code.*
group.e2ee low-level internals
msg.secure.outbox internals
provider token/secret route internals unless operator/diagnostic gated
runtime heartbeat stubs
stub commands
```

检查：

```bash
cargo run -p awiki-cli -- schema --format json
cargo run -p awiki-cli -- schema --audience diagnostic --format json
cargo run -p awiki-cli -- schema --audience internal --format json
```

---

## 4. 静态 allowlist burn-down

Final 完成时，默认 app/adapter/runtime 不应命中：

```bash
rg "crate::message::|use crate::message\\b|awiki_cli::message" \
  crates/awiki-cli/src crates/awiki-cli/tests

rg "crate::mail|use crate::mail\\b|awiki_cli::mail" \
  crates/awiki-cli/src crates/awiki-cli/tests

rg "crate::identity::service|crate::identity::client|crate::identity::wire|identity::Manager" \
  crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime

rg "crate::store::|use crate::store\\b|store_message|upsert_group|upsert_contact" \
  crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime

rg "crate::authsdk|crate::anpsdk|im_core::compat" \
  crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime
```

允许残留必须满足：

```text
1. 文件位于 migration/diagnostic/internal service manager 路径。
2. direct invocation policy 有 gate 或 stable unsupported。
3. 不在 default schema surface。
4. PR 说明列出残留原因和后续删除条件。
```

---

## 5. Cargo 和依赖清理

执行：

```bash
cargo check -p awiki-cli
cargo test -p awiki-cli
cargo test -p im-core
```

删除未使用依赖时优先使用编译器反馈。重点关注：

```text
rusqlite: 如果 CLI diagnostic/migration 仍需要，可保留；普通 IM store 不应依赖 CLI rusqlite。
sha2 / base64 / pem / crypto helpers: 如果只为 old message/identity wire，删除。
websocket/http helpers: 如果 im-core runtime 接管后仅 CLI host notify/update 需要，收敛命名。
anp-mls provider subprocess dependencies: 不应留在 default CLI path。
```

---

## 6. 全量验证

最终必须运行：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test -p im-core
cargo test -p awiki-cli
cargo check --workspace
```

如果 workspace 中存在需要外部服务的 live/system 测试，不作为默认 blocker，但必须记录：

```text
未运行的测试 target
原因
需要的服务/环境变量
后续系统测试 owner
```

---

## 7. 完成定义

Final 完成后：

```text
1. awiki-cli 默认路径中没有旧 IM business module。
2. 如果 im-core 有实现，CLI 使用 im-core；如果 im-core 没实现，CLI 返回 unsupported 或隐藏命令。
3. CLI 只保留壳、本机宿主、host notify、migration/diagnostic gate。
4. im-core 不依赖 CLI 类型。
5. legacy modules、unused deps、compat allowlist 完成 burn-down。
6. schema/default surface 与最终 CLI role 一致。
```

