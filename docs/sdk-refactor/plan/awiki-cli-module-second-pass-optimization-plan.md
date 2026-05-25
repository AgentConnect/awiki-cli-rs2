# aWiki-CLI Module Second-Pass Optimization Plan

日期：2026-05-25

目标：在默认业务逻辑已经切到 M-Core / `im-core` 之后，对 `crates/awiki-cli` 做第二轮系统收口。收口不是为了让旧测试继续通过，而是把 CLI 明确变成薄壳、宿主、迁移和诊断入口；所有已经不存在的旧业务功能、旧合约测试支撑代码、空壳模块和宽泛 legacy API 都必须删除。

本计划覆盖三类结果：

1. 必须留下来的模块：保留，但改成能表达当前职责的名字。
2. 部分文件需要留下的模块：删除无用文件；保留文件移动到合理模块下。
3. 完全没有用的内容：整块删除，测试随最新 CLI 命令面调整，不再反向保留旧实现。

---

## 0. 原始基线和删除确认准则

本节记录制定本计划时的原始基线，用来解释为什么要做第二轮收口；它不是 2026-05-26 的当前代码状态。当前已执行结果以第 8 节为准。

制定计划时，`crates/awiki-cli/src` 下仍有这些关键边界：

```text
app
cli
cmdmeta
config
docs
doctor
im_core_adapter
legacy_identity
legacy_store
runtime
runtime_legacy
transportcfg
update
upgrade
authsdk
anpsdk
```

制定计划时没有实际空目录：

```bash
find . -path './target' -prune -o -path './.git' -prune -o -type d -empty -print
```

制定计划时的 legacy 主要残留点：

```text
默认 app:
  crates/awiki-cli/src/app.rs
    - id.create 仍调用 legacy_identity::create_migration_identity
    - id.import-v1 仍调用 legacy_identity::import_v1_migration
    - legacy IdentityError / StoreError 仍作为 app 错误边界

im_core_adapter:
  crates/awiki-cli/src/im_core_adapter/auth.rs
  crates/awiki-cli/src/im_core_adapter/identity.rs
  crates/awiki-cli/src/im_core_adapter/identity_replace_did_plan.rs
  crates/awiki-cli/src/im_core_adapter/message_result.rs
  crates/awiki-cli/src/im_core_adapter/paths.rs
    - 仍复用 legacy_identity::CommandResult / IdentityError / constants / helper

upgrade / doctor / debug:
  crates/awiki-cli/src/upgrade/*
  crates/awiki-cli/src/doctor/mod.rs
  crates/awiki-cli/src/app/debug_handlers.rs
    - 仍读 legacy identity layout 和 legacy SQLite

runtime:
  crates/awiki-cli/src/runtime/listener_bridge_connection.rs
  crates/awiki-cli/src/runtime/listener_bridge_dispatch.rs
  crates/awiki-cli/src/runtime/listener_session_methods.rs
    - 仍复用 legacy_identity::types::StoredIdentity 作为 DTO

runtime_legacy:
  crates/awiki-cli/src/runtime_legacy/*
    - 主要是旧 listener / secure / store side-effect 合约测试支撑

tests:
  crates/awiki-cli/tests/*legacy*
  crates/awiki-cli/tests/store_*_contract.rs
  crates/awiki-cli/tests/identity_contract.rs
  crates/awiki-cli/tests/identity_wire_contract.rs
  crates/awiki-cli/tests/runtime_listener_*_contract.rs 中部分 runtime_legacy 覆盖
```

关键原则：

```text
不把测试当保留理由。
不把 dry-run 文案、旧 DTO、错误映射当保留 legacy 模块的理由。
不保留任何“默认业务路径可能还用得上”的宽泛 API。
保留项必须能被归类为：CLI 壳、M-Core 适配、宿主/本机服务、迁移、诊断、升级元数据。
删除必须逐文件确认，不能因为模块名字带 legacy 或在本计划删除清单中就直接整块 rm。
```

### 0.1 删除确认准则

本计划中的“删除”表示目标状态，不表示可以跳过审计直接删除。每个文件、函数、测试在删除前必须完成下面确认：

```text
1. 引用确认：
   - `rg` 全仓确认所有引用点。
   - 区分 production path、migration path、diagnostics path、tests、docs。
   - 如果仍被 production path 引用，先迁移调用方，不能直接删。

2. 命令面确认：
   - 检查 `cmdmeta` 和 `cli::dispatch` 是否仍有 handler。
   - 检查 schema audience 和 direct invocation policy。
   - 如果命令仍是当前 CLI 命令面的一部分，必须先改成 M-Core path、迁移 path、诊断 path 或明确 unsupported/removed。

3. M-Core 覆盖确认：
   - 如果删除的是默认业务功能，必须确认等价能力已在 `im-core` 或 `m_core_cli_adapter` 中存在。
   - 如果没有等价能力，但命令也已经不存在，删除旧实现。
   - 如果没有等价能力且命令仍要存在，不能删除；先补 M-Core 或把命令改为 unsupported。

4. 迁移/诊断确认：
   - 如果代码用于老工作区、老身份文件、老 SQLite 数据导入或只读诊断，不能直接删除。
   - 这类代码要先移动到 `workspace_upgrade::legacy_*` 或 `diagnostics::legacy_*`，并收窄到最小 API。

5. 数据格式确认：
   - 对 `types.rs`、schema、import、rebind、recover_merge 这类文件，要确认是否仍负责读取历史磁盘格式。
   - 负责历史格式读取的最小 struct/decoder 可保留；默认业务 writer/CRUD 删除。

6. 测试确认：
   - 如果测试只验证旧 API 仍可用，删除。
   - 如果测试验证旧数据可以升级，改名并迁到 workspace upgrade 语义。
   - 如果测试验证当前 CLI 命令面，按最新命令行重写。

7. 编译和静态门禁：
   - 删除后至少跑 `cargo check -p awiki-cli`。
   - 对命令面变更跑 schema/cmdmeta 相关 tests。
```

每个删除 PR 必须在说明里列出：

```text
删除对象
全仓引用确认命令和结果
是否属于 production / migration / diagnostics / tests
替代路径或删除理由
已运行验证
```

---

## 1. 最终模块形态

### 1.1 必须留下来的模块

这些模块保留，但需要改名或收紧 public surface，使名字表达当前职责。

| 当前模块 | 最终建议名 | 保留理由 | 收口要求 |
| --- | --- | --- | --- |
| `app` | `cli_shell` | 命令执行、渲染、错误转换、命令 handler 聚合 | 移除直接 legacy 引用；迁移和诊断 handler 拆到独立子模块；`app.rs` 不再承载旧 identity/store API |
| `cli` | `cli_parser` | 参数解析、direct invocation gate | 继续保留；只引用 `command_catalog`，不引用业务模块 |
| `cmdmeta` | `command_catalog` | schema、audience、命令策略 | 继续保留；命令是否实现必须与真实 handler 一致；不再把已删除命令标成 implemented |
| `config` | `workspace_config` | 工作区路径、配置解析、配置写入 | 继续保留；identity index 常量从 `legacy_identity` 移出 |
| `output` | `cli_output` | JSON/pretty/table envelope 和 exit error | 继续保留；不要依赖 legacy error enum |
| `docs` | `cli_docs` | CLI 内置说明主题 | 继续保留；删除旧命令文档入口 |
| `traceutil` | `cli_trace` | CLI 运行 trace | 继续保留 |
| `buildinfo` | `build_info` | version/status 输出 | 继续保留 |
| `durablefs` | `durable_fs` | 原子写、文件安全写入 | 继续保留，供 config/update/runtime/migration 使用 |
| `im_core_adapter` | `m_core_cli_adapter` | CLI 到 M-Core 的薄适配层 | 保留但收紧：只做参数映射、DTO 渲染、错误转换；不得引用 `legacy_identity` / `legacy_store` |
| `runtime` | `host_runtime` | 本机 listener service、bridge、host notify、OpenClaw/Hermes 宿主 | 保留宿主能力；删除旧 IM local state side effects；DTO 改为 M-Core 或本地 runtime DTO |
| `runtime/hermes_bridge` | `host_runtime/hermes_bridge` | Hermes webhook/bridge 服务 | 保留；使用 CLI-owned HTTP helper |
| `update` | `self_update` | CLI 自更新、版本检查、缓存 | 保留；使用 CLI-owned HTTP helper |
| `upgrade` | `workspace_upgrade` | 工作区 schema 升级、锁、journal、backup | 保留；legacy 读取能力内聚到该模块下 |
| `doctor` | `diagnostics` | 只读诊断 | 保留但改名；只读检查可以存在，不能暴露旧 store/identity 业务 API |
| `transportcfg` 的 HTTP 子集 | `cli_http` | update、Hermes、OpenClaw webhook 仍需要 HTTP client | 只保留 CLI-owned HTTP client、proxy、CA bundle、timeout；删除 IM SDK profile/auth 语义 |

### 1.2 建议的最终 `lib.rs`

最终 public module 应趋近于：

```rust
pub mod build_info;
pub mod cli_output;
pub mod cli_parser;
pub mod cli_shell;
pub mod cli_trace;
pub mod command_catalog;
pub mod diagnostics;
pub mod durable_fs;
pub mod m_core_cli_adapter;
pub mod self_update;
pub mod workspace_config;
pub mod workspace_upgrade;

#[doc(hidden)]
pub mod cli_http;

#[doc(hidden)]
pub mod host_runtime;

pub use cli_shell::execute;
```

迁移专用 legacy 文件应在 `workspace_upgrade::legacy_*` 或 `diagnostics::legacy_*` 下私有化，不应继续作为 `awiki_cli::legacy_identity` / `awiki_cli::legacy_store` 暴露。

---

## 2. 部分保留模块的拆分计划

### 2.1 `legacy_identity`

制定计划时的问题：

```text
legacy_identity/mod.rs 仍公开 client、wire、service、recover、replace_did、types 等大量旧身份业务能力。
默认 app 和 im_core_adapter 仍复用 CommandResult、IdentityError、INDEX_FILE_NAME、warning 文案和 helper。
旧 identity_contract / identity_wire_contract 仍把它当默认 identity API 测。
```

最终处理：

```text
删除 root module:
  crates/awiki-cli/src/legacy_identity/mod.rs

保留并移动到 workspace_upgrade::legacy_identity:
  layout.rs
  legacy.rs
  key_compat.rs
  types.rs 的最小迁移记录类型

按需保留到 workspace_upgrade::legacy_identity_rebind:
  replace_did.rs 中 v2->v3 schema upgrade 实际需要的旧 DID 重绑函数
  只保留 migration_v2_to_v3 调用链需要的最小代码

按需保留到 diagnostics::legacy_identity:
  只读 scan / summary / current identity 检查
  不提供 create/register/bind/recover/refresh/replace-did 默认业务函数

移动到 m_core_cli_adapter 或 cli_shell:
  CommandResult -> 新建 CLI-owned `CliCommandResult`
  IdentityError -> 用 `ExitError` 或 `CliIdentityBoundaryError`
  replace_did_danger_warning -> 放到 id_replace_did handler 或 M-Core adapter
  recover_identity_ignored_warning -> 放到 id_recover handler 或 M-Core adapter
  INDEX_FILE_NAME -> `workspace_config::IDENTITY_INDEX_FILE_NAME` 或 `m_core_cli_adapter::paths`
  choose_default_identity_name / choose_named_identity -> 若仍需要，只作为 migration helper 私有存在
```

删除候选：

```text
client.rs
wire.rs
service.rs 中 register/bind/recover/refresh/profile/resolve/status/list/current/use 默认业务函数
did.rs 中已由 im-core identity/auth/wire 覆盖的默认 DID 生成和 ANP service builder
recover.rs 中已由 im-core recover flow 覆盖的执行路径
replace_did.rs 中不再用于 workspace upgrade 的执行路径
```

例外：

```text
如果 `id create` 仍作为 migration-only bootstrap 命令保留，它必须改名或迁入 migration handler：
  command: id.create 继续 hidden + --migration gate
  implementation: cli_shell::migration_handlers + workspace_upgrade::legacy_identity
  不允许默认 id.* 间接调用 legacy_identity
```

完成门禁：

```bash
rg "legacy_identity" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime
rg "awiki_cli::legacy_identity" crates/awiki-cli/tests
```

期望：

```text
默认 app / adapter / runtime 零命中。
测试中只允许 workspace upgrade / migration fixture 命名的文件命中，且路径不再叫 identity_contract。
```

### 2.2 `legacy_store`

当前问题：

```text
legacy_store/mod.rs 重新导出了 contacts、messages、groups、e2ee_outbox、query、rebind、recover_merge、schema、import 等旧本地状态 API。
默认 app/debug、doctor、upgrade 和 runtime_legacy 仍可直接调用这些 CRUD。
store_*_contract 把旧 SQLite local state 当成仍需维护的默认 store contract。
```

当前状态见第 8 节：runtime_legacy、旧 store CRUD、raw SQL debug.db.query handler、awiki-cli recover_merge 旧副本已经删除或收口。

最终处理：

```text
删除 root module:
  crates/awiki-cli/src/legacy_store/mod.rs

保留并移动到 workspace_upgrade::legacy_sqlite:
  open.rs
  schema.rs
  types.rs 的 StoreError / StoreResult / SCHEMA_VERSION
  import.rs 中 scan_legacy_database / import_legacy_database / LegacyOwnerLookup
  rebind.rs 中 migration_v2_to_v3 实际需要的 rebind_local_identity_state
  helpers.rs 中 import/rebind 实际用到的 helper

不再保留在 awiki-cli:
  recover_merge.rs + recover_merge/*
  旧 recover merge 行为已迁到 im-core 的 identity_recover_local_state，并以 im-core 测试覆盖。

保留并移动到 diagnostics::legacy_sqlite:
  open_read_only
  current_schema_version
  list_contact_handle_history 或等价只读检查
  明确禁止 raw SQL 执行能力

删除或内联到 import/rebind:
  contacts.rs
  messages.rs
  groups.rs
  e2ee_outbox.rs
  recover_merge.rs 的 awiki-cli 旧副本
```

关于 `debug db query`：

```text
制定计划时 direct_invocation_policy 已把 debug.db.query 标成 StableUnsupported，但 handler 仍存在。
2026-05-26 当前状态：run_debug_db_query、dispatch handler、legacy_store::execute_sql 和 query.rs raw SQL 执行已经删除。
cmdmeta 中 debug.db.query 保留为 unsupported/stub，implemented=false。
```

关于 `debug db import-v1`：

```text
如果仍需要旧 v1 数据导入，保留为 migration-only:
  debug.db.import-v1 -> workspace_upgrade::legacy_sqlite::import
  必须 --migration gate
  不允许复用 legacy_store public CRUD
```

完成门禁：

```bash
rg "legacy_store" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter crates/awiki-cli/src/runtime
rg "store_message|upsert_group|upsert_contact|queue_e2ee_outbox|mark_e2ee_outbox" crates/awiki-cli/src
rg "awiki_cli::legacy_store" crates/awiki-cli/tests
```

期望：

```text
默认 app / adapter / runtime 零命中。
旧 CRUD helper 零命中。
测试中只允许 workspace upgrade / migration / diagnostics fixture 命名的文件命中。
```

### 2.3 `runtime_legacy`

当前问题：

```text
runtime_legacy/mod.rs 明确说明只为历史 contract coverage 保留。
它仍导出旧 listener side effects、secure local ack/outbox/inbox、wsclient/ws_transport、notification store write 等模块。
这些代码已经不属于 production runtime listener run/service-run。
```

最终处理：

```text
整块删除:
  crates/awiki-cli/src/runtime_legacy/*
  lib.rs 中 pub mod runtime_legacy

迁移到 im-core tests:
  listener_session_loop.rs 中对 im_core::realtime 的 re-export 测试
  listener_wsclient.rs 中已由 im-core realtime runner 覆盖的逻辑
  listener_secure_* 中已由 im-core secure/realtime/local_state 覆盖的状态机

迁移到 host_runtime:
  如果某个函数是真正的本机宿主能力，而不是旧 IM side effect，则移动到 host_runtime 对应模块。
  当前候选需要逐个审计，默认假设 runtime_legacy 全部删除。
```

默认 runtime 中的残留：

```text
crates/awiki-cli/src/runtime/listener_bridge_connection.rs
crates/awiki-cli/src/runtime/listener_bridge_dispatch.rs
crates/awiki-cli/src/runtime/listener_session_methods.rs
```

处理方式：

```text
把 `legacy_identity::types::StoredIdentity` 替换为新的 host runtime DTO：
  host_runtime::identity_session::RuntimeIdentitySession

或直接使用 im-core public identity summary/session DTO：
  im_core::IdentitySummary
  im_core::realtime::* session DTO

不允许 runtime 为了 DTO 继续依赖 legacy_identity。
```

完成门禁：

```bash
rg "runtime_legacy" crates/awiki-cli/src crates/awiki-cli/tests
rg "legacy_identity::types::StoredIdentity" crates/awiki-cli/src/runtime crates/awiki-cli/tests
```

期望：

```text
源码零命中。
测试零命中，或只剩明确迁到 im-core 的测试。
```

### 2.4 `im_core_adapter`

当前问题：

```text
模块职责正确，但仍借旧 identity 类型做 CLI result/error/dry-run 输出。
paths.rs 用 legacy_identity::types::INDEX_FILE_NAME。
auth.rs 和 identity_replace_did_plan.rs 用 legacy_identity::CommandResult/warning。
message_result.rs 还实现 From<legacy_identity::IdentityError>。
```

最终处理：

```text
保留并改名为 `m_core_cli_adapter`。
新增 adapter 自有 DTO:
  CliCommandResult { data, summary, warnings }
  CliIdentityBoundaryError 或直接返回 ExitError

删除 legacy 依赖:
  auth.rs 不再 use crate::legacy_identity
  identity.rs 不再 use crate::legacy_identity
  identity_replace_did_plan.rs 不再 use crate::legacy_identity
  message_result.rs 删除 From<legacy_identity::IdentityError>
  paths.rs 使用 workspace_config 或本地常量

保持边界:
  只做 CLI 参数 -> im-core request
  im-core result -> CLI output data
  im-core error -> ExitError
```

完成门禁：

```bash
rg "legacy_identity|legacy_store" crates/awiki-cli/src/im_core_adapter
```

期望零命中。

### 2.5 `app`

当前问题：

```text
app.rs 同时承担 command host、identity migration、store error mapping、legacy owner lookup、workspace init 等职责。
```

最终处理：

```text
保留并改名为 `cli_shell`。
拆出子模块：
  cli_shell::core_handlers        status/version/init/config/docs/schema
  cli_shell::identity_handlers    id.* 默认 M-Core path
  cli_shell::migration_handlers   id.create/id.import-v1/debug.db.import-v1
  cli_shell::diagnostic_handlers  doctor/debug read-only helpers
  cli_shell::runtime_handlers     runtime high-level UX
  cli_shell::render               render_success/render_identity_result
  cli_shell::errors               ExitError mapping

删除 app root 的 legacy imports:
  use crate::legacy_identity::{...}
  use crate::legacy_store::{...}

把 migration-only 函数移动:
  run_id_create
  run_id_import_v1
  legacy_owner_lookup

把 store_exit/identity_exit 收紧:
  store_exit -> workspace_upgrade/diagnostics 私有错误转换
  identity_exit -> migration handler 私有错误转换，默认 id.* 使用 im-core error mapper
```

### 2.6 `doctor`

当前问题：

```text
doctor/mod.rs 既做现代工作区诊断，也读旧 identity/store。
```

最终处理：

```text
改名为 `diagnostics`。
保留：
  工作区路径检查
  M-Core local state schema 检查
  runtime/host notify 检查
  legacy workspace 只读 scan

删除：
  任何会修复、导入、重绑、写入旧 store/identity 的诊断路径

移动：
  旧 identity scan -> diagnostics::legacy_identity
  旧 SQLite scan -> diagnostics::legacy_sqlite
```

### 2.7 `upgrade`

当前问题：

```text
upgrade 是必须保留模块，但直接依赖 legacy_identity / legacy_store root。
```

最终处理：

```text
改名为 `workspace_upgrade`。
保留：
  backup.rs
  detect.rs
  fsutil.rs
  journal.rs
  lock.rs
  meta.rs
  migration_v0_to_v1.rs
  migration_v1_to_v2.rs
  migration_v2_to_v3.rs
  settings.rs
  types.rs
  upgrader.rs

新增私有子模块：
  legacy_identity.rs
  legacy_sqlite.rs
  legacy_rebind.rs

目标：
  upgrade 内部可以读旧格式。
  upgrade 之外不能 import legacy identity/store。
  recover merge 行为由 im-core 承担，不在 workspace_upgrade 中恢复 awiki-cli 旧副本。
```

完成门禁：

```bash
rg "legacy_identity|legacy_store" crates/awiki-cli/src --glob '!workspace_upgrade/**' --glob '!diagnostics/**'
```

### 2.8 `transportcfg`, `authsdk`, `anpsdk`

当前问题：

```text
transportcfg 仍被 update/runtime/Hermes/OpenClaw 使用，也被 old authsdk/legacy identity 使用。
authsdk/anpsdk 主要服务旧 identity/wire/direct-e2ee facade 和旧合约测试。
```

最终处理：

```text
transportcfg:
  改名为 cli_http。
  保留 new_http_client / new_http_client_with_proxy_env / HttpRequest / HttpResponse / CA bundle / proxy / timeout。
  删除或迁走 Profile 中只服务旧 IM SDK 的语义。

authsdk:
  删除。
  如果仍有 JSON-RPC payload helper 被 CLI-owned HTTP 使用，移动到 cli_http::json_rpc 或 im-core。

anpsdk:
  删除 CLI facade。
  如果 key_compat 还需要 PrivateKeyMaterial，优先让 im-core 或 anp crate 直接承担。
  不再维护 Go PascalCase facade 测试。
```

完成门禁：

```bash
rg "crate::authsdk|awiki_cli::authsdk" crates/awiki-cli/src crates/awiki-cli/tests
rg "crate::anpsdk|awiki_cli::anpsdk" crates/awiki-cli/src crates/awiki-cli/tests
rg "transportcfg" crates/awiki-cli/src crates/awiki-cli/tests
```

期望：

```text
authsdk/anpsdk 零命中。
transportcfg 只剩改名前的过渡命中，最终替换为 cli_http。
```

---

## 3. 完全删除清单

### 3.1 通过确认后删除的源码模块

这些是目标上要删除的内容，但执行时必须先按 `0.1 删除确认准则` 逐文件确认。不能用一次 `rm -rf` 直接删除整个目录，除非已经证明目录内所有文件都只服务已废弃功能或旧测试。

第一批删除目标：

```text
crates/awiki-cli/src/runtime_legacy/*
crates/awiki-cli/src/authsdk/*
crates/awiki-cli/src/anpsdk.rs
```

执行确认重点：

```text
runtime_legacy:
  - 先列出每个 runtime_listener_*_contract 的引用。
  - 确认 host runtime service manager 逻辑没有放在 runtime_legacy 内。
  - 如果某个 helper 实际仍属于 host runtime，先移动到 host_runtime，再删除 runtime_legacy。

authsdk:
  - 确认 update/Hermes/OpenClaw 没有依赖其中 HTTP/JSON-RPC helper。
  - 如果有通用 HTTP helper，先移动到 cli_http。
  - 旧 DID auth/session 逻辑不保留在 CLI。

anpsdk.rs:
  - 确认只有 CLI facade 或旧测试引用。
  - key_compat 如仍需要底层 key material，改为直接依赖 im-core/anp 的正式 API。
```

第二批删除目标：

```text
crates/awiki-cli/src/legacy_identity/client.rs
crates/awiki-cli/src/legacy_identity/wire.rs
crates/awiki-cli/src/legacy_identity/service.rs 的默认业务函数
crates/awiki-cli/src/legacy_identity/did.rs 中已由 im-core 覆盖的默认 DID/service builder
crates/awiki-cli/src/legacy_identity/recover.rs 中已由 im-core recover 覆盖的执行路径
crates/awiki-cli/src/legacy_identity/replace_did.rs 中不被 workspace upgrade 调用的执行路径
```

执行确认重点：

```text
client.rs / wire.rs:
  - 如果只是旧 user-service client 和 wire builder，删除。
  - 如果某个 wire DTO 仍是历史文件格式，不放这里，移动到 workspace_upgrade 私有 legacy type。

service.rs:
  - create/register/bind/recover/refresh/profile/resolve/status/list/current/use 这些默认业务函数删除或由 M-Core adapter 替换。
  - import_v1_migration / create_migration_identity 若仍保留，移动到 migration handler + workspace_upgrade legacy module。
  - CommandResult / warning 文案移动到 CLI-owned adapter/handler。

did.rs:
  - 默认身份生成、ANP service builder 如 im-core 已覆盖，删除。
  - 仅历史磁盘格式解析需要的 helper 才可私有保留。

recover.rs / replace_did.rs:
  - 当前 id recover / id replace-did 默认路径应走 im-core。
  - workspace migration v2->v3 实际需要的重绑逻辑先抽到 workspace_upgrade，再删除剩余旧执行路径。
```

第三批删除目标：

```text
crates/awiki-cli/src/legacy_store/contacts.rs
crates/awiki-cli/src/legacy_store/messages.rs
crates/awiki-cli/src/legacy_store/groups.rs
crates/awiki-cli/src/legacy_store/e2ee_outbox.rs
crates/awiki-cli/src/legacy_store/query.rs 的 raw SQL execute_sql
```

执行确认重点：

```text
contacts/messages/groups/e2ee_outbox:
  - 如果函数是旧 local state CRUD，删除。
  - 如果 import/rebind 还需要读写某些表，先把最小 SQL 移到 workspace_upgrade::legacy_sqlite 私有模块。
  - recover merge 行为不再放在 awiki-cli legacy_store；当前保留位置是 im-core 的 identity_recover_local_state。
  - 不保留旧 store public API。

query.rs / execute_sql:
  - raw SQL 执行能力删除。
  - debug.db.query handler 删除或改为 removed/unsupported，cmdmeta 不再 implemented=true。
  - 诊断只保留明确白名单的只读检查函数。
```

### 3.2 删除或重写的测试

不再维护这些旧合约测试：

```text
crates/awiki-cli/tests/store_contact_contract.rs
crates/awiki-cli/tests/store_e2ee_outbox_contract.rs
crates/awiki-cli/tests/store_groups_contract.rs
crates/awiki-cli/tests/store_helpers_contract.rs
crates/awiki-cli/tests/store_messages_contract.rs
crates/awiki-cli/tests/identity_contract.rs
crates/awiki-cli/tests/identity_wire_contract.rs
crates/awiki-cli/tests/authsdk_contract.rs
crates/awiki-cli/tests/anpsdk_contract.rs
crates/awiki-cli/tests/cli_http_profile_contract.rs
crates/awiki-cli/tests/cli_http_contract.rs  # 改名为 cli_http_contract 后只保留 CLI-owned HTTP 覆盖
```

需要迁移或改名的测试：

```text
identity_legacy_import_contract.rs
workspace_migration_v0_to_v1_contract.rs
workspace_upgrade_contract.rs
workspace_upgrade_if_needed_contract.rs
  -> 保留为 workspace_upgrade legacy fixture 测试

store_import_contract.rs
store_rebind_contract.rs
store_recover_merge_contract.rs
  -> 改名为 workspace_upgrade_legacy_sqlite_import_contract.rs
     workspace_upgrade_legacy_sqlite_rebind_contract.rs
     im_core_identity_recover_local_state tests

diagnostics_contract.rs
diagnostic_debug_contract.rs
  -> 改成 diagnostics_contract.rs / migration_diagnostic_debug_contract.rs
  -> 只覆盖当前 schema 和 gate，不覆盖旧 CRUD

runtime_listener_*_contract.rs 中引用 runtime_legacy 的测试
  -> 删除或迁到 im-core realtime tests
  -> host runtime service manager 相关测试保留并改名为 host_runtime_*_contract.rs
```

原则：

```text
系统测试会按最新命令行调整。
Rust contract tests 也应按最新命令面调整。
如果一个测试只证明旧模块还能用，它应该删除。
如果一个测试证明旧数据能升级，它可以保留，但文件名和 module path 必须写明 migration/upgrade。
```

---

## 4. 执行顺序

### Phase A：先切断默认路径对 legacy DTO 的依赖

目标：默认 `app` / `im_core_adapter` / `runtime` 不再为了类型、文案、错误转换引用 legacy。

步骤：

```text
1. 在 m_core_cli_adapter 增加 `CliCommandResult`。
2. 替换 identity/auth/replace_did adapter 中的 legacy_identity::CommandResult。
3. 把 INDEX_FILE_NAME 移到 workspace_config 或 adapter paths 本地常量。
4. 删除 message_result.rs 中 From<legacy_identity::IdentityError>。
5. runtime bridge/session 把 StoredIdentity 替换成 host runtime DTO 或 im-core DTO。
```

门禁：

```bash
rg "legacy_identity|legacy_store" crates/awiki-cli/src/im_core_adapter
rg "legacy_identity|legacy_store" crates/awiki-cli/src/runtime
cargo check -p awiki-cli
```

### Phase B：拆出 migration / diagnostics legacy 私有模块

目标：`legacy_identity` / `legacy_store` root 不再是 crate public module。

步骤：

```text
1. 在 workspace_upgrade 下建立 legacy_identity / legacy_sqlite 私有模块。
2. 移动 upgrade 实际需要的旧格式读取、导入、重绑。
   recover merge 行为已经迁到 im-core，不在 awiki-cli 中恢复旧副本。
3. 在 diagnostics 下建立 legacy_identity / legacy_sqlite 只读模块。
4. app migration/debug/doctor 调用新路径。
5. lib.rs 删除 pub mod legacy_identity / pub mod legacy_store。
```

门禁：

```bash
rg "pub mod legacy_identity|pub mod legacy_store" crates/awiki-cli/src/lib.rs
rg "crate::legacy_identity|crate::legacy_store" crates/awiki-cli/src
cargo check -p awiki-cli
```

期望：

```text
lib.rs 零命中。
源码只允许 workspace_upgrade/diagnostics 内部出现 legacy_* 文件名。
```

### Phase C：删除 runtime_legacy

目标：删除旧 listener internals。

步骤：

```text
1. 标出 runtime_listener_*_contract 中哪些是 host runtime service manager 测试，哪些只是旧 listener side effect 测试。
2. host runtime 测试改名并继续覆盖 `host_runtime`。
3. 已由 im-core realtime 覆盖的测试迁到 im-core 或删除。
4. 删除 runtime_legacy module 和文件。
5. lib.rs 删除 pub mod runtime_legacy。
```

门禁：

```bash
rg "runtime_legacy" crates/awiki-cli/src crates/awiki-cli/tests
cargo check -p awiki-cli
```

### Phase D：删除 authsdk/anpsdk，收敛 transportcfg 为 cli_http

目标：CLI 不再承载 ANP/auth SDK façade。

步骤：

```text
1. update/runtime/Hermes/OpenClaw 改用 cli_http。
2. legacy migration 如需 key parsing，直接用 im-core/anp crate 或私有 helper。
3. 删除 authsdk module 和测试。
4. 删除 anpsdk.rs 和测试。
5. Cargo.toml 删除只因这些 facade 存在而保留的依赖。
```

门禁：

```bash
rg "authsdk|anpsdk|transportcfg" crates/awiki-cli/src crates/awiki-cli/tests
cargo check -p awiki-cli
```

### Phase E：命令面和测试面最终收口

目标：schema、direct invocation policy、handler、测试与最新 CLI 命令行一致。

步骤：

```text
1. 删除 debug.db.query handler 和 raw SQL helper。
2. 对已不存在的命令改为 removed/unsupported，并保证 cmdmeta 不再 implemented=true。
3. 确认 migration-only 命令必须 --migration gate。
4. 确认 diagnostic-only 命令必须 --diagnostic gate。
5. 删除所有只覆盖旧业务 API 的 contract tests。
6. 把保留的 legacy 测试重命名为 workspace_upgrade/diagnostics 语义。
```

门禁：

```bash
cargo run -p awiki-cli -- schema --format json
cargo run -p awiki-cli -- schema --audience migration --format json
cargo run -p awiki-cli -- schema --audience diagnostic --format json
rg "implemented: true|handler:" crates/awiki-cli/src/cmdmeta/mod.rs
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
cargo test -p awiki-cli --test command_catalog_schema_contract
```

---

## 5. 三类模块矩阵

### 5.1 必须留下来的模块

```text
buildinfo        -> build_info
cli              -> cli_parser
cmdmeta          -> command_catalog
config           -> workspace_config
docs             -> cli_docs
durablefs        -> durable_fs
output           -> cli_output
traceutil        -> cli_trace
app              -> cli_shell
im_core_adapter  -> m_core_cli_adapter
runtime          -> host_runtime
update           -> self_update
upgrade          -> workspace_upgrade
doctor           -> diagnostics
transportcfg/http subset -> cli_http
```

保留条件：

```text
1. 不直接依赖 legacy_identity / legacy_store / runtime_legacy。
2. 不承担旧 IM business logic。
3. public API 名字表达 CLI 当前职责。
4. 测试覆盖的是当前命令面或宿主行为。
```

### 5.2 部分文件需要留下来的模块

```text
legacy_identity:
  保留 layout/legacy/key_compat/minimal types 到 workspace_upgrade::legacy_identity。
  按需保留只读 scan 到 diagnostics::legacy_identity。
  其余默认业务、wire、client、service 删除。

legacy_store:
  保留 import/rebind/open/schema/minimal types/helpers 到 workspace_upgrade::legacy_sqlite。
  recover merge 行为保留在 im-core identity_recover_local_state，不再在 awiki-cli 保留旧副本。
  按需保留只读 schema/handle history 到 diagnostics::legacy_sqlite。
  messages/groups/contacts/e2ee_outbox CRUD 和 raw SQL 删除。

runtime:
  保留 service manager、platform integration、host notify、Hermes/OpenClaw、bridge 宿主。
  删除或替换 StoredIdentity legacy DTO。
  不再执行旧 secure/outbox/inbox local side effects。

transportcfg:
  保留 HTTP client 子集为 cli_http。
  删除 IM SDK profile/auth 语义。

tests:
  workspace upgrade / migration fixture 测试保留并改名。
  diagnostics gate 测试保留并改名。
  默认旧 store/identity/runtime legacy contract 删除。
```

### 5.3 完全没有用的内容

```text
runtime_legacy 整个模块。
authsdk 整个模块。
anpsdk CLI facade。
legacy identity client/wire/default service 旧业务路径。
legacy store messages/groups/contacts/e2ee_outbox CRUD。
debug.db.query raw SQL 执行。
只为旧 contract tests 存在的 adapters、aliases、re-export。
旧 Go PascalCase facade 合约测试。
旧 store_*_contract 默认本地状态测试。
旧 identity_contract / identity_wire_contract 默认身份协议测试。
```

---

## 6. 静态审计命令

每个 PR 合并前至少跑：

```bash
git status --short
cargo fmt --check
cargo check -p awiki-cli
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
cargo test -p awiki-cli --test command_catalog_schema_contract
```

关键收口 grep：

```bash
rg "legacy_identity|legacy_store|runtime_legacy" crates/awiki-cli/src
rg "awiki_cli::legacy_identity|awiki_cli::legacy_store|awiki_cli::runtime_legacy" crates/awiki-cli/tests
rg "crate::authsdk|crate::anpsdk|awiki_cli::authsdk|awiki_cli::anpsdk" crates/awiki-cli/src crates/awiki-cli/tests
rg "store_message|upsert_group|upsert_contact|queue_e2ee_outbox|execute_sql" crates/awiki-cli/src crates/awiki-cli/tests
rg "StoredIdentity" crates/awiki-cli/src/runtime crates/awiki-cli/tests
```

最终期望：

```text
legacy_identity / legacy_store:
  只允许在 workspace_upgrade::legacy_* 或 diagnostics::legacy_* 私有模块中出现。

runtime_legacy:
  零命中。

authsdk/anpsdk:
  零命中。

legacy store CRUD:
  零命中。

debug.db.query:
  不再有执行 handler；cmdmeta 不再 implemented=true。
```

---

## 7. 完成定义

这轮优化完成时必须满足：

```text
1. awiki-cli 默认命令路径只走 M-Core adapter，不再碰 legacy_identity / legacy_store。
2. awiki-cli public module 不再暴露 legacy_identity / legacy_store / runtime_legacy / authsdk / anpsdk。
3. 旧数据升级能力只存在于 workspace_upgrade 私有 legacy 子模块。
4. 旧数据只读诊断只存在于 diagnostics 私有 legacy 子模块。
5. runtime 只负责本机宿主、service manager、host notify 和 bridge，不负责旧 IM local state side effects。
6. 不存在为了旧测试通过而保留的旧业务实现。
7. Rust contract tests 的命名与当前职责一致：cli_shell / m_core_adapter / host_runtime / workspace_upgrade / diagnostics。
8. 系统测试按最新 CLI 命令行更新，不再要求旧命令、旧 store API、旧 wire API 可用。
9. Cargo 依赖根据删除结果收紧；`rusqlite` 只有 migration/diagnostics 仍需要时才保留。
10. 空目录、空 module、只有 re-export 的过渡 module 全部删除。
```

推荐最终验证：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test -p im-core
cargo test -p awiki-cli
cargo check --workspace
```

如果某些 live/system tests 需要外部服务，不作为本轮代码删除 blocker，但必须记录：

```text
未运行的测试 target
未运行原因
需要的服务或环境变量
对应系统测试 owner
```

---

## 8. 本轮已执行收口记录

记录日期：2026-05-25；更新日期：2026-05-26

这一节记录当前执行态。前面章节保留了启动本轮工作时的历史 baseline 和目标拆分；后续继续收口时，以本节的当前模块名、保留边界和验证命令为准，不要把旧 `app` / `doctor` / `upgrade` / `runtime` 路径重新当作现状。

### 8.1 源模块命名已收敛

当前 `crates/awiki-cli/src/lib.rs` 只暴露新模块名：

```text
build_info
cli_docs
cli_output
cli_parser
cli_shell
cli_trace
command_catalog
diagnostics
durable_fs
host_runtime
m_core_cli_adapter
cli_http
self_update
workspace_config
workspace_upgrade
```

已完成的源码模块重命名：

```text
app -> cli_shell
cli -> cli_parser
cmdmeta -> command_catalog
config -> workspace_config
docs -> cli_docs
doctor -> diagnostics
durablefs.rs -> durable_fs.rs
im_core_adapter -> m_core_cli_adapter
im_core_adapter/config.rs -> m_core_cli_adapter/core_config.rs
output.rs -> cli_output.rs
runtime -> host_runtime
traceutil.rs -> cli_trace.rs
transportcfg -> cli_http
update -> self_update
upgrade -> workspace_upgrade
buildinfo.rs -> build_info.rs
```

已完成的测试目标重命名：

```text
cmdmeta_schema_contract -> command_catalog_schema_contract
config_policy_contract -> workspace_config_policy_contract
config_writer_contract -> workspace_config_writer_contract
core_contract -> cli_shell_core_contract
debug_contract -> diagnostic_debug_contract
doctor_contract -> diagnostics_contract
im_core_adapter_policy_contract -> m_core_cli_adapter_policy_contract
traceutil_contract -> cli_trace_contract
transportcfg_contract -> cli_http_profile_contract
transportcfg_http_contract -> cli_http_contract
update_contract -> self_update_contract
runtime_*_contract -> host_runtime_*_contract
```

脚本同步：

```text
scripts/sdk-refactor/final-cutover-check.sh 已改为当前测试目标名。
scripts/check_rust_coverage.py 已把 app/doctor/docs 阈值路径改为 cli_shell/diagnostics/cli_docs。
```

确认命令：

```bash
rg -n '\b(crate|awiki_cli)::(app|cli|cmdmeta|config|docs|doctor|durablefs|im_core_adapter|output|runtime|traceutil|transportcfg|update|upgrade|buildinfo)\b|pub mod (app|cli|cmdmeta|config|docs|doctor|durablefs|im_core_adapter|output|runtime|traceutil|transportcfg|update|upgrade|buildinfo)\b' \
  crates/awiki-cli/src crates/awiki-cli/tests -g '!target'
```

当前结果：零命中。

### 8.2 已删除或重写的对象

`runtime_legacy`：

```text
已删除：
  crates/awiki-cli/src/runtime_legacy/*
  lib.rs 中的 pub mod runtime_legacy

已删除旧 runtime_legacy contract tests：
  runtime_listener_notification_execute_contract.rs
  runtime_listener_notification_handler_contract.rs
  runtime_listener_notification_plan_contract.rs
  runtime_listener_secure_ack_delivery_contract.rs
  runtime_listener_secure_ack_in_process_contract.rs
  runtime_listener_secure_inbox_poll_contract.rs
  runtime_listener_secure_normalize_contract.rs
  runtime_listener_secure_notifications_contract.rs
  runtime_listener_secure_outbox_flush_contract.rs
  runtime_listener_secure_replay_contract.rs
  runtime_listener_secure_sessions_contract.rs
  runtime_listener_secure_sync_contract.rs
  runtime_listener_session_loop_contract.rs
  runtime_listener_wsclient_contract.rs

保留并改名：
  当前 host runtime 覆盖位于 host_runtime_*_contract.rs。
  bridge/session/listener 当前能力位于 crates/awiki-cli/src/host_runtime/*。
```

确认命令：

```bash
rg -n "runtime_legacy|awiki_cli::runtime_legacy" crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'
```

当前结果：零命中。

`authsdk` / `anpsdk`：

```text
已删除：
  crates/awiki-cli/src/authsdk/*
  crates/awiki-cli/src/anpsdk.rs
  lib.rs 中的 pub mod authsdk
  lib.rs 中的 pub mod anpsdk

已删除旧 contract tests：
  authsdk_contract.rs
  anpsdk_contract.rs
  identity_wire_contract.rs
  identity_key_compat_contract.rs
  identity_legacy_import_contract.rs

保留：
  workspace_upgrade::legacy_identity 内部仍需要的 legacy auth wire helper。
  key compatibility 直接使用 anp / im-core 当前能力，不再经过 CLI facade。
```

确认命令：

```bash
rg -n "authsdk|anpsdk|awiki_cli::authsdk|awiki_cli::anpsdk|crate::authsdk|crate::anpsdk" \
  crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'
```

当前结果：零命中。

`debug.db.query` raw SQL：

```text
已删除：
  run_debug_db_query
  debug.db.query dispatch handler
  legacy_store::execute_sql public API
  旧 query.rs raw SQL validate/execute 路径

保留：
  command_catalog 中 debug.db.query 的 stable unsupported/stub metadata，避免 direct invocation 从“受控不支持”退化成“未知命令”。
  diagnostic_debug_contract / cli_shell_core_contract / cli_cutover_command_surface_contract 中对 unsupported 行为的断言。
  workspace_upgrade::legacy_sqlite::query_rows 作为 legacy SQLite import 的 crate-private 读取 helper。
```

确认命令：

```bash
rg -n "run_debug_db_query|legacy_store::execute_sql|debug\\.db\\.query.*implemented.*true" \
  crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'
```

当前结果：零命中。

注意：当前 `execute_sql` 仍在少数 live contract tests 中作为本地 fixture helper 函数名出现，不是 `legacy_store::execute_sql` public API。

`legacy_store` 旧 CRUD 和 recover merge：

```text
已删除 root module：
  crates/awiki-cli/src/legacy_store/*
  lib.rs 中的 pub mod legacy_store

已删除旧 CRUD 文件：
  legacy_store/messages.rs
  legacy_store/groups.rs
  legacy_store/e2ee_outbox.rs
  legacy_store/recover_merge.rs
  legacy_store/recover_merge/*

已删除旧 contract tests：
  store_contact_contract.rs
  store_e2ee_outbox_contract.rs
  store_groups_contract.rs
  store_helpers_contract.rs
  store_import_contract.rs
  store_messages_contract.rs
  store_rebind_contract.rs
  store_recover_merge_contract.rs

保留并移动：
  crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/*

保留原因：
  legacy SQLite import
  workspace migration v0->v1
  DID rebind
  diagnostics / debug handle history 只读查询

不再保留：
  messages/groups/e2ee_outbox CRUD
  recover_merge awiki-cli 旧副本
  make_thread_id / now_utc 等只服务旧 store contract 的 helper
```

确认命令：

```bash
rg -n "crate::legacy_store|awiki_cli::legacy_store|legacy_store::recover_merge|store_recover_merge_contract|store::(store_message|store_messages_batch|upsert_group|upsert_contact|queue_e2ee_outbox)" \
  crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'
```

当前结果：零命中。

recover merge 当前边界：

```text
awiki-cli 旧副本已删除。
当前 id recover 走 M-Core / im-core。
本地状态恢复合并能力由 crates/im-core/src/internal/identity_recover_local_state.rs 承担。
```

`legacy_identity` 默认业务路径：

```text
已删除 root module：
  crates/awiki-cli/src/legacy_identity/*
  lib.rs 中的 pub mod legacy_identity

已删除旧默认业务文件或能力：
  legacy_identity/recover.rs
  RegisterParams / BindParams / RecoverParams
  旧 public identity wire contract
  旧 authsdk facade wrapper
  只服务旧 contract 的 key compatibility wrapper
  update_display_name / set_default 等旧默认业务 mutator

已删除旧 contract tests：
  identity_contract.rs
  identity_wire_contract.rs
  identity_key_compat_contract.rs
  identity_legacy_import_contract.rs

保留并移动：
  crates/awiki-cli/src/workspace_upgrade/legacy_identity/*

保留原因：
  老身份 layout scan/import
  workspace migration v0->v1
  workspace migration v2->v3 replace-did/rebind
  debug id import-v1 migration path
  diagnostics 旧身份只读检查

不再保留：
  默认 id recover 路径；当前 id recover 走 M-Core / im-core。
  默认 register/bind/recover/profile/list/current/status 旧 helper。
```

确认命令：

```bash
rg -n "crate::legacy_identity|awiki_cli::legacy_identity|legacy_identity::recover|RegisterParams|BindParams|RecoverParams" \
  crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'
```

当前结果：零命中。

### 8.3 默认路径已完成的解耦

`m_core_cli_adapter`：

```text
当前路径：
  crates/awiki-cli/src/m_core_cli_adapter/*

已完成：
  不再直接引用 legacy_identity / legacy_store / authsdk / anpsdk。
  config adapter 文件改名为 core_config.rs，避免和 workspace_config 混淆。
  adapter 只保留 CLI flag/path/config/output 到 M-Core DTO 的薄转换、错误映射和渲染。
```

确认命令：

```bash
rg -n "legacy_identity|legacy_store|authsdk|anpsdk" crates/awiki-cli/src/m_core_cli_adapter -g '!target'
```

当前结果：零命中。

`host_runtime` bridge/session：

```text
当前路径：
  crates/awiki-cli/src/host_runtime/*

已完成：
  不再使用 legacy_identity::types::StoredIdentity 作为 runtime DTO。
  host runtime 自有 DTO 位于 host_runtime/listener_identity_record.rs。
  listener_bridge_connection / listener_bridge_dispatch / listener_session_methods 改用 RuntimeIdentityRecord。
```

确认命令：

```bash
rg -n "legacy_identity::types::StoredIdentity|StoredIdentity" \
  crates/awiki-cli/src/host_runtime \
  crates/awiki-cli/tests/host_runtime_listener_bridge_connection_contract.rs \
  crates/awiki-cli/tests/host_runtime_listener_bridge_dispatch_contract.rs \
  crates/awiki-cli/tests/host_runtime_listener_session_methods_contract.rs \
  -g '!target'
```

当前结果：零命中。

`cli_shell` / `diagnostics` / `workspace_upgrade` legacy 边界：

```text
当前路径：
  crates/awiki-cli/src/cli_shell.rs
  crates/awiki-cli/src/diagnostics/mod.rs
  crates/awiki-cli/src/diagnostics/legacy_identity.rs
  crates/awiki-cli/src/diagnostics/legacy_sqlite.rs
  crates/awiki-cli/src/workspace_upgrade/*

已完成：
  workspace_upgrade 内建立 legacy_identity / legacy_sqlite 私有模块。
  root legacy_identity / legacy_store 不再是 crate public module。
  cli_shell 中只通过局部 facade 调用 workspace_upgrade 私有 legacy module，用于 id create/import-v1/debug legacy 边界。
  diagnostics 中通过私有 legacy_identity / legacy_sqlite facade 调用 workspace_upgrade 私有 legacy module，用于旧身份/旧 SQLite 只读诊断。
  debug db handle-history 的只读旧 SQLite 查询已走 diagnostics::legacy_sqlite；raw SQL debug.db.query 仍是 stable unsupported。
```

确认命令：

```bash
rg -n "crate::legacy_identity|crate::legacy_store|awiki_cli::legacy_identity|awiki_cli::legacy_store" \
  crates/awiki-cli/src crates/awiki-cli/tests -g '!target'
```

当前结果：零命中。

### 8.4 当前明确保留边界

这些文件仍然属于迁移、诊断或旧数据导入边界，不应仅因为名字包含 `legacy` 就删除：

```text
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/import.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/rebind.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/schema.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/open.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/types.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/helpers.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/contacts.rs
crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/query.rs

crates/awiki-cli/src/diagnostics/legacy_identity.rs
crates/awiki-cli/src/diagnostics/legacy_sqlite.rs

crates/awiki-cli/src/workspace_upgrade/legacy_identity/layout.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/legacy.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/key_compat.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/types.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/service.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/replace_did.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/client.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/wire.rs
crates/awiki-cli/src/workspace_upgrade/legacy_identity/auth/*
```

保留原因必须保持具体：

```text
workspace_upgrade::legacy_sqlite::import:
  老 SQLite 数据导入。

workspace_upgrade::legacy_sqlite::rebind:
  v2->v3 / replace-did 后旧本地状态 owner_did 重绑和 E2EE 清理。

workspace_upgrade::legacy_sqlite::contacts:
  diagnostics/debug handle history 只读查询。

diagnostics::legacy_sqlite:
  旧 SQLite 只读诊断和 debug handle-history 的私有 facade，不提供 raw SQL 执行。

diagnostics::legacy_identity:
  旧身份只读诊断的私有 facade，不提供默认 identity 业务 API。

workspace_upgrade::legacy_identity::*:
  老身份 layout scan/import、v0->v1 migration、v2->v3 replace-did migration、import-v1/debug migration 边界。
```

明确不在保留边界内，且已经删除：

```text
runtime_legacy/*
authsdk/*
anpsdk.rs
legacy_store root module
legacy_identity root module
legacy_store CRUD
legacy_store recover_merge
legacy_identity recover
raw SQL debug.db.query handler
store_*_contract
identity_contract / identity_wire_contract / identity_key_compat_contract / identity_legacy_import_contract
authsdk_contract / anpsdk_contract
```

当前允许的测试引用：

```text
workspace_migration_v0_to_v1_contract.rs
workspace_upgrade_contract.rs
workspace_upgrade_if_needed_contract.rs
diagnostics_contract.rs
diagnostic_debug_contract.rs
identity_cli_surface_contract.rs
当前 live/contract tests 中用于构造旧数据的 fixture helper
```

当前不允许的测试引用：

```text
awiki_cli::legacy_identity
awiki_cli::legacy_store
awiki_cli::runtime_legacy
awiki_cli::authsdk
awiki_cli::anpsdk
旧 store_*_contract / identity_*_contract 只为旧 public API 存在的目标
```

### 8.5 本轮验证命令

2026-05-26 已运行并通过：

```bash
cargo fmt -p awiki-cli
cargo fmt --check
cargo check -p awiki-cli --tests
cargo check --workspace

cargo test -p awiki-cli --test command_catalog_schema_contract
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
cargo test -p awiki-cli --test m_core_cli_adapter_policy_contract
cargo test -p awiki-cli --test legacy_path_cutover_contract

cargo test -p awiki-cli --test diagnostics_contract
cargo test -p awiki-cli --test diagnostic_debug_contract
cargo test -p awiki-cli --test cli_shell_core_contract
cargo test -p awiki-cli --test workspace_migration_v0_to_v1_contract
cargo test -p awiki-cli --test workspace_upgrade_if_needed_contract

cargo test -p awiki-cli --lib workspace_upgrade::legacy_identity::legacy_import_tests
cargo test -p awiki-cli --lib workspace_upgrade::legacy_sqlite::import_tests
cargo test -p awiki-cli --lib workspace_upgrade::legacy_sqlite::rebind_tests

cargo test -p awiki-cli --test msg_all_inbox_live_contract
cargo test -p awiki-cli --test msg_ws_mark_read_live_contract
cargo test -p awiki-cli
cargo test -p im-core
cargo clippy --workspace --all-targets
```

测试夹具修复：

```text
msg_all_inbox_live_contract.rs 和 msg_ws_mark_read_live_contract.rs 内部的 SQLite fixture helper 已改用 tests/support::open_local_state。
这只是创建 data 目录并初始化当前 im-core local_state schema；不会恢复旧 store public API，也不会让命令回退到 legacy cache/bridge。
```

当前已知警告：

```text
im-core 仍有若干 dead_code warnings，非本轮 awiki-cli 删除目标。
awiki-cli 仍有两个 legacy migration/rebind 结果字段 dead_code warnings：
  workspace_upgrade/legacy_identity/replace_did.rs: ReplaceDidBackupResult.manifest
  workspace_upgrade/legacy_sqlite/rebind.rs: RebindLocalIdentityStateError.store_rebind / e2ee_cleanup

这些字段属于备份/错误语义结构，不在本轮为了清警告而删除。

clippy 已完整运行并退出 0；仍输出既有 style/dead_code warnings，主要分布在 im-core、host_runtime、workspace_upgrade 和测试断言风格中，未发现阻塞错误。
```

最后一次静态确认：

```bash
rg -n '\b(crate|awiki_cli)::(app|cli|cmdmeta|config|docs|doctor|durablefs|im_core_adapter|output|runtime|traceutil|transportcfg|update|upgrade|buildinfo)\b|pub mod (app|cli|cmdmeta|config|docs|doctor|durablefs|im_core_adapter|output|runtime|traceutil|transportcfg|update|upgrade|buildinfo)\b' \
  crates/awiki-cli/src crates/awiki-cli/tests -g '!target'

rg -n "runtime_legacy|authsdk|anpsdk|awiki_cli::legacy_identity|awiki_cli::legacy_store|awiki_cli::runtime_legacy|crate::legacy_identity|crate::legacy_store|crate::authsdk|crate::anpsdk|legacy_identity::recover|legacy_store::recover_merge|RegisterParams|BindParams|RecoverParams|run_debug_db_query|store_recover_merge_contract|identity_contract|identity_wire_contract|store_.*_contract" \
  crates/awiki-cli/src crates/awiki-cli/tests scripts -g '!target'

find crates/awiki-cli/src crates/awiki-cli/tests -type d -empty | sort
```

当前结果：

```text
旧 public module 名扫描：零命中。
确认删除目标扫描：零命中。
空目录扫描：零命中。
```

补充确认：

```text
rg execute_sql crates/awiki-cli/src crates/awiki-cli/tests -g '!target'
```

当前只命中少数 live contract tests 内部的本地 fixture helper，不存在 `legacy_store::execute_sql` 或 raw SQL debug handler。

后续继续收口前推荐先跑：

```bash
rg -n "legacy_identity|legacy_store" \
  crates/awiki-cli/src/cli_shell.rs \
  crates/awiki-cli/src/diagnostics \
  crates/awiki-cli/src/workspace_upgrade \
  crates/awiki-cli/src/m_core_cli_adapter \
  crates/awiki-cli/src/host_runtime \
  -g '!target'

rg -n "awiki_cli::legacy_identity|awiki_cli::legacy_store|awiki_cli::runtime_legacy|awiki_cli::authsdk|awiki_cli::anpsdk" \
  crates/awiki-cli/tests -g '!target'

cargo check -p awiki-cli --tests
```
