# Content / Site 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：当前 IM cutover 后的独立 Content / Site 迁移阶段  
**目标**：把 CLI 中的 `page.*`、`site.root.*`、`site.page.*` 业务能力迁入 `crates/im-core`，让 page/site/content 默认路径完全依赖 `im-core`，不再依赖老的 `awiki-cli` content/site 业务代码。

---

## 0. 约束

执行本计划前先阅读并遵守：

```text
docs/sdk-refactor/implementation-playbook.md
docs/sdk-refactor/README.md
docs/sdk-refactor/architecture.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/im-core-cli-boundary.md
docs/sdk-refactor/Interface/README.md
docs/sdk-refactor/Interface/01-crate-layout.md
docs/sdk-refactor/Interface/02-core-interface.md
docs/sdk-refactor/Interface/03-identity-auth-interface.md
docs/sdk-refactor/Interface/05-cli-adapter-interface.md
docs/sdk-refactor/Interface/06-implementation-map.md
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
```

迁移原则沿用 SDK refactor 主线：

```text
1. im-core 不依赖 awiki-cli。
2. awiki-cli 可以依赖 im-core 和 im_core_adapter。
3. CLI 只保留命令解析、文件读取、dry-run、envelope/render 和 ExitError 映射。
4. RPC wire、DTO 校验、auth session、service 调用和 response normalize 迁入 im-core。
5. 旧 awiki-cli content/site 测试先迁到 im-core，再 cutover handler。
6. cutover 后默认 dispatch 不允许回到 legacy content/site business path。
```

本计划只迁 page/site/content，不迁：

```text
1. raw RPC / SQL / debug command。
2. OpenClaw / Hermes / service manager。
3. attachment send/download。
4. secure direct / group E2EE。
5. Dart / Flutter facade，除非后续单独计划要求。
```

---

## 1. 迁移前代码基线

迁移前，page/site/content 分散在 CLI：

```text
crates/awiki-cli/src/content/client.rs
crates/awiki-cli/src/content/service.rs
crates/awiki-cli/src/content/types.rs
crates/awiki-cli/src/content/wire.rs

crates/awiki-cli/src/site/client.rs
crates/awiki-cli/src/site/service.rs
crates/awiki-cli/src/site/types.rs
crates/awiki-cli/src/site/wire.rs

crates/awiki-cli/src/app/page_handlers.rs
crates/awiki-cli/src/app/site_handlers.rs
```

现有业务边界：

```text
1. `page.*` 是个人内容页命令族，当前走 `/content/rpc`。
2. `site.root.*` 和 `site.page.*` 是站点命令族，当前走 `/site/rpc`。
3. content wire methods: `create`, `list`, `get`, `update`, `rename`, `delete`。
4. site wire methods: `get_root`, `set_root`, `list_pages`, `get_page`, `create_page`, `update_page`, `rename_page`, `delete_page`。
5. app handlers 直接调用 `crate::content` / `crate::site`。
6. `page_contract.rs`、`site_contract.rs`、`site_live_contract.rs` 把 page/site cutover guard 当成 unsupported 行为测试。
7. `content_wire_contract.rs`、`site_wire_contract.rs` 测 `awiki_cli::content` / `awiki_cli::site` wire helper。
```

`im-core` 当前 public service 风格是：

```rust
client.auth()
client.identity()
client.directory()
client.messages()
client.attachments()
client.groups()
client.realtime()
client.email()
client.secure()
```

因此 Content / Site 迁移应新增同风格 service，而不是把 CLI handler 的旧函数整体搬进 SDK public API。

---

## 2. 目标 public API

推荐新增两个 service：

```rust
impl ImClient {
    pub fn content(&self) -> crate::content::ContentService<'_>;
    pub fn site(&self) -> crate::site::SiteService<'_>;
}
```

不推荐统一成 `client.pages()` / `client.sites()`：

```text
1. content page 和 site page 是两个不同产品域。
2. 两者 endpoint、method、权限模型和 DTO 都不同。
3. 合并成 pages API 会让 domain/root/content visibility 等边界变模糊。
4. 当前 im-core service 命名已按产品域组织，`content()` / `site()` 更符合现有风格。
```

新增模块：

```text
crates/im-core/src/content/
  mod.rs
  dto.rs
  service.rs
  wire.rs

crates/im-core/src/site/
  mod.rs
  dto.rs
  service.rs
  wire.rs
```

`wire.rs` 默认 `pub(crate)` 或只对 crate/test 暴露；public API 只通过 DTO 和 service 暴露。若 contract tests 需要检查 wire，可提供 `#[doc(hidden)]` 的 compat/test helper，但不要把 raw RPC builder 放进 prelude。

`crates/im-core/src/lib.rs` re-export：

```rust
pub mod content;
pub mod site;

pub use crate::content::ContentService;
pub use crate::site::SiteService;
```

如需 re-export DTO，按现有模块风格从 `content::mod.rs` / `site::mod.rs` 内统一 `pub use`。

---

## 3. DTO 边界

### 3.1 共享 DTO

新增或复用：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageSlug(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRef {
    pub slug: PageSlug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Draft,
    Unlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDeleteResult {
    pub deleted: bool,
    pub raw: serde_json::Value,
}
```

规则：

```text
1. `PageSlug::parse` trim 后不能为空。
2. `Visibility::parse` 接受 public/draft/unlisted，大小写不敏感。
3. create draft 的空 visibility 默认为 public。
4. update patch 的空 visibility 表示不更新。
5. 分页统一使用 `crate::ids::Page<T>`、`PageLimit`、`Cursor`。
6. 错误统一使用 `ImError`，不新增 public `ContentError` / `SiteError`。
```

### 3.2 content DTO

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDraft {
    pub slug: PageSlug,
    pub title: String,
    pub body: String,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPageQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDocument {
    pub slug: PageSlug,
    pub title: Option<String>,
    pub body: Option<String>,
    pub visibility: Option<Visibility>,
    pub raw: serde_json::Value,
}
```

`ContentService` public API：

```rust
impl<'a> ContentService<'a> {
    pub fn create_page(&self, draft: PageDraft) -> crate::ImResult<PageDocument>;
    pub fn list_pages(
        &self,
        query: ContentPageQuery,
    ) -> crate::ImResult<crate::ids::Page<PageDocument>>;
    pub fn get_page(&self, page: PageRef) -> crate::ImResult<PageDocument>;
    pub fn update_page(
        &self,
        page: PageRef,
        patch: PageUpdate,
    ) -> crate::ImResult<PageDocument>;
    pub fn rename_page(
        &self,
        page: PageRef,
        target: PageSlug,
    ) -> crate::ImResult<PageDocument>;
    pub fn delete_page(&self, page: PageRef) -> crate::ImResult<PageDeleteResult>;
}
```

content wire contract：

```text
create_page -> endpoint `/content/rpc`, method `create`, profile RpcDefault
list_pages  -> endpoint `/content/rpc`, method `list`, profile RpcReadHeavy
get_page    -> endpoint `/content/rpc`, method `get`, profile RpcReadHeavy
update_page -> endpoint `/content/rpc`, method `update`, profile RpcDefault
rename_page -> endpoint `/content/rpc`, method `rename`, profile RpcDefault
delete_page -> endpoint `/content/rpc`, method `delete`, profile RpcDefault
```

payload 兼容旧 Go/CLI contract：

```text
create: `{ "slug", "title", "body", "visibility" }`
list:   `{}`
get:    `{ "slug" }`
update: `{ "slug", optional "title", optional "body", optional "visibility" }`
rename: `{ "old_slug", "new_slug" }`
delete: `{ "slug" }`
```

validation：

```text
1. create: slug required，title required。
2. update: slug required，title/body/visibility 至少一个字段存在。
3. rename: source slug 和 target slug 都 required。
4. delete/get: slug required。
5. invalid visibility -> `ImError::InvalidInput { field: Some("visibility"), ... }`。
```

### 3.3 site DTO

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiteDomain(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRootDraft {
    pub domain: SiteDomain,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRootDocument {
    pub domain: SiteDomain,
    pub body: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageRef {
    pub domain: SiteDomain,
    pub slug: PageSlug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageQuery {
    pub domain: SiteDomain,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageDraft {
    pub domain: SiteDomain,
    pub slug: PageSlug,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageUpdate {
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageDocument {
    pub domain: SiteDomain,
    pub slug: PageSlug,
    pub body: Option<String>,
    pub raw: serde_json::Value,
}
```

`SiteDomain::parse` 必须复用当前 `normalize_did_domain` 的语义：

```text
1. trim。
2. lower-case。
3. 去掉末尾单个 dot。
4. 拒绝空 domain。
5. 拒绝 URL-like domain，例如 `https://tenant.example`。
6. `Tenant.Example.` 规范化为 `tenant.example`。
```

`SiteService` public API：

```rust
impl<'a> SiteService<'a> {
    pub fn get_root(&self, domain: SiteDomain) -> crate::ImResult<SiteRootDocument>;
    pub fn set_root(&self, draft: SiteRootDraft) -> crate::ImResult<SiteRootDocument>;
    pub fn list_pages(
        &self,
        query: SitePageQuery,
    ) -> crate::ImResult<crate::ids::Page<SitePageDocument>>;
    pub fn get_page(&self, page: SitePageRef) -> crate::ImResult<SitePageDocument>;
    pub fn create_page(&self, draft: SitePageDraft) -> crate::ImResult<SitePageDocument>;
    pub fn update_page(
        &self,
        page: SitePageRef,
        patch: SitePageUpdate,
    ) -> crate::ImResult<SitePageDocument>;
    pub fn rename_page(
        &self,
        page: SitePageRef,
        target: PageSlug,
    ) -> crate::ImResult<SitePageDocument>;
    pub fn delete_page(&self, page: SitePageRef) -> crate::ImResult<PageDeleteResult>;
}
```

site wire contract：

```text
get_root    -> endpoint `/site/rpc`, method `get_root`, profile RpcReadHeavy
set_root    -> endpoint `/site/rpc`, method `set_root`, profile RpcDefault
list_pages  -> endpoint `/site/rpc`, method `list_pages`, profile RpcReadHeavy
get_page    -> endpoint `/site/rpc`, method `get_page`, profile RpcReadHeavy
create_page -> endpoint `/site/rpc`, method `create_page`, profile RpcDefault
update_page -> endpoint `/site/rpc`, method `update_page`, profile RpcDefault
rename_page -> endpoint `/site/rpc`, method `rename_page`, profile RpcDefault
delete_page -> endpoint `/site/rpc`, method `delete_page`, profile RpcDefault
```

payload 兼容旧 Go/CLI contract：

```text
get_root:    `{ "domain" }`
set_root:    `{ "domain", "body" }`
list_pages:  `{ "domain" }`
get_page:    `{ "domain", "slug" }`
create_page: `{ "domain", "slug", "body" }`
update_page: `{ "domain", "slug", "body" }`
rename_page: `{ "domain", "old_slug", "new_slug" }`
delete_page: `{ "domain", "slug" }`
```

validation：

```text
1. domain required / invalid -> `ImError::InvalidInput { field: Some("domain"), ... }`。
2. slug required -> `ImError::InvalidInput { field: Some("slug"), ... }`。
3. set_root/create_page/update_page 允许空 body，因为旧 contract 允许显式 empty body。
```

---

## 4. 错误和 response 边界

public error 使用 `ImError`：

```text
InvalidInput       -> DTO parse / field validation
AuthRequired       -> missing JWT / auth material / 401 without finer signal
SessionExpired     -> refresh/session expired if transport can distinguish
PermissionDenied   -> 403
Service            -> remote JSON-RPC / HTTP service error
TransportUnavailable -> endpoint/network unavailable
Serialization      -> malformed JSON response
Internal           -> invariant broken
```

不要把 CLI 的 exit code、hint、`ContentError`、`SiteError` 放进 `im-core`。

response DTO 规则：

```text
1. 稳定字段尽量提取到 typed fields。
2. 服务端未稳定字段保留在 `raw: serde_json::Value`。
3. list response 转成 `Page<T>`；如果服务端只返回 `pages` 和 `count`，则 `has_more=false`、`next_cursor=None`。
4. delete response 转成 `PageDeleteResult`；`deleted` 优先从 response.deleted 读取，缺省按 RPC 成功视为 true。
```

CLI adapter 可以继续渲染旧 envelope shape，但这属于 CLI 层：

```text
content:
  action: create_page/list_pages/get_page/update_page/rename_page/delete_page
  identity
  page/pages/count/changed_fields/from/to

site:
  action: site_root_get/site_root_set/site_page_*
  identity
  domain/root/page/pages/count/from/to
```

---

## 5. 迁移切片

### C1：API 骨架和 DTO

新增：

```text
crates/im-core/src/content/{mod.rs,dto.rs,service.rs,wire.rs}
crates/im-core/src/site/{mod.rs,dto.rs,service.rs,wire.rs}
```

修改：

```text
crates/im-core/src/core/client.rs
crates/im-core/src/lib.rs
```

完成标准：

```text
1. `client.content()` 和 `client.site()` 可编译。
2. DTO parse/default/serde 单元测试通过。
3. im-core boundary test 确认没有 CLI 类型引用。
```

### C2：content wire 迁移

迁移源：

```text
crates/awiki-cli/src/content/types.rs
crates/awiki-cli/src/content/wire.rs
```

迁移内容：

```text
1. `/content/rpc` endpoint。
2. create/list/get/update/rename/delete method。
3. RpcDefault / RpcReadHeavy profile 选择。
4. slug/title/visibility/no-update-fields validation。
5. response normalize helper。
```

不迁：

```text
1. CommandResult。
2. CLI summary 文案。
3. CLI JSON envelope 构造。
```

### C3：content service 迁移

迁移源：

```text
crates/awiki-cli/src/content/client.rs
crates/awiki-cli/src/content/service.rs
```

替换点：

```text
1. 旧 `content::Client` 替换为 im-core internal transport。
2. 旧 `authsdk::Session` bootstrap 替换为 im-core session provider。
3. 旧 `config::Resolved` / `identity::Manager` 替换为 `ImClient` 当前 identity runtime。
```

完成标准：

```text
1. ContentService 可对 fake RPC server 发出旧 wire contract 兼容请求。
2. JWT/session 持久化走 im-core 已有身份 runtime。
3. service 层返回 typed DTO，不返回 CLI CommandResult。
```

### C4：site wire 迁移

迁移源：

```text
crates/awiki-cli/src/site/types.rs
crates/awiki-cli/src/site/wire.rs
```

迁移内容：

```text
1. `/site/rpc` endpoint。
2. root get/set wire。
3. page list/get/create/update/rename/delete wire。
4. domain normalize / slug validation。
5. response normalize helper。
```

不迁：

```text
1. CommandResult。
2. CLI summary 文案。
3. CLI JSON envelope 构造。
```

### C5：site service 迁移

迁移源：

```text
crates/awiki-cli/src/site/client.rs
crates/awiki-cli/src/site/service.rs
```

替换点：

```text
1. 旧 `site::Client` 替换为 im-core internal transport。
2. 旧 `authsdk::Session` bootstrap 替换为 im-core session provider。
3. 旧 `config::Resolved` / `identity::Manager` 替换为 `ImClient` 当前 identity runtime。
```

完成标准：

```text
1. SiteService 可对 fake RPC server 发出旧 wire contract 兼容请求。
2. root/page response 返回 typed DTO。
3. service 层不引用 CLI 类型。
```

### C6：CLI adapter cutover

优先新增或扩展：

```text
crates/awiki-cli/src/im_core_adapter/content.rs
crates/awiki-cli/src/im_core_adapter/site.rs
```

职责：

```text
1. 从 app handler 输入构造 im-core DTO。
2. 调用 `client.content()` / `client.site()`。
3. 把 `ImError` 映射成 CLI `ExitError`。
4. 把 typed DTO 渲染回旧 CLI envelope data shape。
5. 保留旧 summary 文案。
```

`app/page_handlers.rs` cutover：

```text
1. `crate::content::{...}` 调用改为 `im_core_adapter::content` 或直接 `client.content()`。
2. handler 继续负责 `--markdown` / `--markdown-file` 互斥和文件读取。
3. dry-run 不调用 im-core service，继续输出当前 plan shape。
```

`app/site_handlers.rs` cutover：

```text
1. `crate::site::{...}` 调用改为 `im_core_adapter::site` 或直接 `client.site()`。
2. handler 继续负责 required flags、markdown body source 和 dry-run。
3. dry-run 不调用 im-core service，继续输出当前 plan shape。
```

### C7：cutover classifier / schema 打开

修改 cutover metadata：

```text
page.*      -> im-core / supported / default_surface=true
site.root.* -> im-core / supported / default_surface=true
site.page.* -> im-core / supported / default_surface=true
```

迁移后不得再使用下列 page/site 当前状态：

```text
capability = page-site
required_phase = outside-current-im-core-cutover
cutover_status = unsupported
```

除非是在历史迁移说明中明确标注“迁移前状态”。生产代码、测试断言和当前状态文档均不得继续使用上述结论。

### C8：legacy cleanup

cutover 后处理：

```text
1. `crates/awiki-cli/src/content/*` 和 `crates/awiki-cli/src/site/*` 不再作为默认业务实现入口。
2. 若短期保留，只能作为 deprecated thin wrapper，内部调用 im-core/adapter。
3. 最终删除旧 client/service/wire/types，或将它们移出默认 crate API。
4. 用 rg 确认 app handler 不再引用 `crate::content` / `crate::site`。
```

验收检查：

```bash
rg "crate::content|crate::site" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

---

## 6. 测试迁移

### 6.1 先迁 wire contract

从 CLI 迁到 im-core：

```text
crates/awiki-cli/tests/content_wire_contract.rs
  -> crates/im-core/tests/content_wire_contract.rs

crates/awiki-cli/tests/site_wire_contract.rs
  -> crates/im-core/tests/site_wire_contract.rs
```

迁移后断言对象从 `awiki_cli::content/site` 改为 `im_core::content/site`。

覆盖：

```text
1. endpoint。
2. method。
3. transport profile。
4. payload shape。
5. slug/domain/visibility validation。
6. list/get read-heavy profile。
7. create/update/rename/delete default profile。
```

### 6.2 新增 im-core service tests

新增：

```text
crates/im-core/tests/content_api.rs
crates/im-core/tests/site_api.rs
```

覆盖：

```text
1. `client.content().create_page(...)` 发出 `/content/rpc create`。
2. `client.content().list_pages(...)` normalize `pages/count` 到 `Page<PageDocument>`。
3. content get/update/rename/delete request 和 response DTO。
4. `client.site().get_root(...)` / `set_root(...)` request 和 response DTO。
5. site page list/get/create/update/rename/delete request 和 response DTO。
6. service error mapping：400/401/403/404/409。
```

### 6.3 CLI tests 保留边界，不再测业务 wire

保留并更新：

```text
crates/awiki-cli/tests/page_contract.rs
crates/awiki-cli/tests/site_contract.rs
crates/awiki-cli/tests/site_live_contract.rs
crates/awiki-cli/tests/cli_cutover_command_surface_contract.rs
```

调整：

```text
1. schema cutover status 从 unsupported 改为 im-core/supported。
2. 删除“commands return cutover unsupported”断言。
3. 增加 dry-run envelope contract。
4. 增加 argument contract：required flags、body source conflict、markdown-file unreadable。
5. 增加 SDK dispatch contract：fake RPC server 收到 im-core 发出的请求。
6. 保留旧 envelope data shape 和 summary 文案断言。
```

CLI 不再重复测试：

```text
1. content/site raw wire builder 全量矩阵。
2. im-core DTO parse 细节。
3. im-core service response normalize 细节。
```

这些由 im-core tests 负责。

### 6.4 最小验证命令

```bash
cargo test -p im-core content
cargo test -p im-core site
cargo test -p awiki-cli --test page_contract
cargo test -p awiki-cli --test site_contract
cargo test -p awiki-cli --test site_live_contract
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
rg "crate::content|crate::site" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
```

---

## 7. 文档冲突修正

本计划落地时同步修正当前明确写着 page/site 非默认支持状态的文档。

必须更新：

```text
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
```

修正点：

```text
1. “不迁 mail/page/site 等不属于当前 im-core public API 的产品域”
   改为 page/site 由本计划纳入 im-core public API；mail 已由 Email plan 单独处理。

2. `page.* | hidden/unsupported`
   改为 `page.* | im-core`，原因指向 `ContentService`。

3. `site.* | hidden/unsupported`
   改为 `site.* | im-core`，原因指向 `SiteService`。

4. 默认命令面裁剪中的 `page.* / site.* -> hidden/unsupported`
   改为 `page.* / site.* -> im-core Content/Site service`。

5. “不建议默认展示”列表中的 `page/site/mail`
   改为只保留仍不应默认展示的 debug/raw/advanced/internal 项，不再包含 page/site。

6. 完成定义中的 “mail/page/site/people/... 等非当前 im-core 能力不进入默认命令面”
   改为 page/site 完成迁移后进入默认命令面；非当前能力只包括 people.search、heartbeat、debug raw 等。
```

如果其他分支存在 `docs/sdk-refactor/plan/cli-shell-final-cutover-execution-plan2.md`，同样按上述语义修正；当前仓库不要求处理该文件。

---

## 8. 完成定义

本迁移完成时，应满足：

```text
1. `im_core::content` 和 `im_core::site` 是 page/site/content 唯一默认业务实现入口。
2. `ImClient` 暴露 `content()` 和 `site()`。
3. CLI page/site handler 不再调用旧 `crate::content` / `crate::site` business path。
4. CLI 仍保留 dry-run、argument validation、markdown file 读取和 envelope 输出。
5. content/site wire contract 在 im-core tests 中覆盖。
6. CLI page/site tests 只覆盖 CLI boundary 和 adapter dispatch。
7. cutover metadata 不再把 page/site 标为 unsupported。
8. docs 中不再出现把 page/site 当前状态写成 hidden/unsupported 的冲突结论。
9. im-core boundary check 确认没有 awiki-cli 依赖。
```

建议最终检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test page_contract
cargo test -p awiki-cli --test site_contract
cargo test -p awiki-cli --test cli_cutover_command_surface_contract
! rg "page\\.\\* / site\\.\\*.*hidden|page\\.\\*.*unsupported|site\\.\\*.*unsupported|page-site" docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
! rg "crate::content|crate::site" crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
! rg "awiki_cli|ParsedCommand|ExitError|config::Resolved|identity::Manager" crates/im-core/src crates/im-core/tests
```
