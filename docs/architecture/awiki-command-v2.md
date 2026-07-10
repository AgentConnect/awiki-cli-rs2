# awiki-cli 命令契约

## 1. 目标

`awiki-cli` 是 awiki 的统一命令行产品面。命令按用户意图和产品资源组织，而不是按底层脚本、RPC 方法或 SDK 内部模块组织。

核心要求：

- 默认非交互，适合人类和 Agent 调用。
- 所有有副作用命令支持 `--dry-run`。
- 所有命令返回结构化输出，详见 `docs/architecture/output-format.md`。
- `schema`、help、completion、docs 和 skill reference 应从同一命令事实源保持一致。
- 业务流程通过 `im-core` public services 执行；CLI 保留 parse/build/call/render。

当前收口边界：

- 本文档描述当前分支已经纳入命令契约的 CLI 产品面，不承诺未完成能力。
- `awiki-cli schema` 是命令事实源；本文档只解释产品意图、常用路径和边界。
- 默认用户面只包含 `cli_owned` 和 `im_core` 命令；diagnostic、migration、internal、unsupported 和 removed 命令不得进入默认 Agent 工作流。
- `awiki-cli runtime listener` 是 CLI 自己的本机 realtime listener/service 能力；`crates/awiki-deamon` 是 awiki-me 客户端安装到宿主机的 daemon 包。两者同在本仓库，但不是同一个产品入口，也不共享用户操作路径。

## 2. 顶层命令

当前默认用户顶层命令：

```text
awiki-cli status
awiki-cli docs
awiki-cli schema
awiki-cli doctor
awiki-cli version
awiki-cli completion
awiki-cli config
awiki-cli tenant
awiki-cli id
awiki-cli msg
awiki-cli mail
awiki-cli group
awiki-cli people
awiki-cli page
awiki-cli site
awiki-cli runtime
```

命令归属：

- `id`：身份、Handle、profile、recover、replace DID。
- `msg`：direct/group 消息、inbox/history、mark-read、附件发送/下载、消息 secure 操作。
- `tenant`：管理后端地址、DID host 和租户隔离工作区。
- `mail`：邮件 inbox/read/send/mark-read/attachment。
- `group`：群生命周期、成员、群消息读取、group secure 状态。
- `people`：联系人、关系、profile/directory 相关能力。
- `page`：当前身份 handle 级内容页。
- `site`：tenant bare-domain site pages，必须显式 `--domain`。
- `runtime`：CLI runtime 状态、listener 状态/开关、host notification 开关。listener 安装、启动、停止、Hermes/OpenClaw 配置等属于 operator/diagnostic 面，不进入默认用户面。

非默认命令域：

- `debug`：专家排障和本地 inspection，需要 diagnostic gate；raw SQL / raw RPC 不属于当前支持能力。
- `id.import-v1`、`id.vault.migrate`、`id.vault.cleanup-plaintext`：历史迁移入口，需要 migration gate。
- `runtime.listener.run`、`runtime.listener.service-run`、`runtime.host-notify.hermes.bridge.service-run`：内部服务入口，需要 internal service gate。
- unsupported / removed 命令只保留稳定错误和兼容提示，不代表可用功能。

## 3. 全局参数

| 参数 | 说明 |
| --- | --- |
| `--identity` | 选择本地身份；未传时使用默认身份。 |
| `--tenant` | 临时选择本次命令使用的租户；不改写全局 active tenant。 |
| `--format` | 输出格式：`json`、`pretty`、`table`、`ndjson`。 |
| `--jq` | 对 JSON 输出执行过滤。 |
| `--dry-run` | 返回计划，不执行副作用。 |
| `--verbose` | 增加诊断上下文。 |

规则：

- `--identity` 是用户层身份选择参数；legacy credential 只能作为兼容概念。
- `--tenant` 只接受已存在租户名。创建租户必须使用 `tenant create`，切换全局默认租户使用 `tenant use`。
- `--format json` 是 canonical 输出。
- `--jq` 不应改变命令语义，只过滤 JSON 结果。
- `--dry-run` 不能写远端状态或本地业务状态。

## 4. Tenant

租户是 `backend_base_url + did_host` 的原子组合，并拥有独立的本地身份、SQLite、runtime、cache 和 logs 目录。CLI 二进制版本和更新元数据是产品级全局状态，不随租户切换。

常用命令：

```bash
awiki-cli tenant list
awiki-cli tenant current
awiki-cli tenant create acme --backend-base-url https://api.acme.example --did-host acme.example
awiki-cli tenant use acme
awiki-cli tenant reconfigure acme --backend-base-url https://api2.acme.example --did-host acme.example
```

规则：

- 租户名会 trim 并规范化为小写；只允许 ASCII 字母、数字和单个 `-` 分隔符，最长 64 个字符，不能以 `-` 开头或结尾，也不能包含 `--`。需要中文、空格或展示用大小写时，使用 `--display-name`。
- `tenant create` 只创建租户，不自动切换 active tenant。
- `tenant use <name>` 只能按已有租户名切换，不能携带 backend 或 DID host 字段。
- `tenant reconfigure` 只允许修改还没有身份或本地数据库数据的空租户；已有数据时应创建新租户。
- `backend_base_url` 和 `did_host` 只保存在租户注册表中，不写入 `tenants/<name>/config.yaml`。
- `--tenant <name>` 是本次命令的临时覆盖，不会改写 `global.json` 中的 active tenant。
- 租户名、`backend_base_url`、`did_host` 等输入不合法时返回 `invalid_argument`；租户不存在返回 `not_found`；重复租户或已有数据阻止重配置返回 `conflict`。这些都不是 `internal_error`。

## 5. Identity

常用命令：

```bash
awiki-cli id list
awiki-cli id current
awiki-cli id use alice
awiki-cli id status
awiki-cli id register --handle alice
awiki-cli id bind --email alice@example.com
awiki-cli id recover --handle alice --phone +12025550123
awiki-cli id profile get
awiki-cli id profile set --markdown-file ./profile.md
```

边界：

- `id` 命令的业务能力归 `im-core::identity` 和 `im-core::auth`。
- CLI 负责参数解析、OTP 输入、文件读取、default identity 文件写入、dry-run 和输出。
- `--identity` 是全局选择参数，用于选择本次命令读取/操作哪个本地身份；切换默认身份使用 `awiki-cli id use <identity>`。
- 私钥、JWT、DID document 写入细节不进入普通输出。

## 6. Messaging

常用命令：

```bash
awiki-cli msg send --to alice --text "hello"
awiki-cli msg send --group GROUP_DID --text "hello"
awiki-cli msg send --to alice --file ./hello.txt --text "caption"
awiki-cli msg inbox --limit 20
awiki-cli msg history --with alice --limit 20
awiki-cli msg mark-read MESSAGE_ID
awiki-cli msg attachment download --with alice --message-id MESSAGE_ID --output ./file.bin
```

消息模型：

```text
Target(direct | group)
x Body(text | attachment)
x Security(default | plaintext | e2ee-required)
x ReceiveMode(pull | realtime)
```

规则：

- direct 消息使用 `--to`。
- 群发消息的 canonical 入口是 `msg send --group`。
- `ReceiveMode` 属于 runtime，不作为普通 `msg send` 语义暴露。
- 附件字节走附件能力，不塞进普通消息 JSON。
- `msg.secure.status` 和 `msg.secure.repair` 是当前默认 secure direct 入口；`msg.secure.init`、failed/retry/drop 等低层 direct secure 命令当前是 stable unsupported，不进入默认产品面。

## 7. Group

常用命令：

```bash
awiki-cli group create --name "Team"
awiki-cli group list
awiki-cli group get --group GROUP_DID
awiki-cli group join --code 123456
awiki-cli group leave --group GROUP_DID
awiki-cli group add --group GROUP_DID --member DID
awiki-cli group remove --group GROUP_DID --member DID
awiki-cli group members --group GROUP_DID
awiki-cli group messages --group GROUP_DID
awiki-cli group secure status --group GROUP_DID
awiki-cli group secure repair --group GROUP_DID
```

边界：

- 群对象是独立资源，归 `group` 顶级域。
- 向群发消息仍通过 `msg send --group`。
- `group secure status` 和 `group secure repair` 是默认用户入口。
- 低层 `group e2ee *` 不属于默认产品契约；部分命令仅在 diagnostic/internal 场景下保留，不能作为普通用户流程或 Agent 默认技能入口。

## 8. Mail

```bash
awiki-cli mail inbox --folder inbox --limit 20
awiki-cli mail read --id MESSAGE_ID
awiki-cli mail mark-read MESSAGE_ID
awiki-cli mail send --to a@example.com --subject "Hello" --body "Hi"
awiki-cli mail attachment download --message-id MESSAGE_ID --attachment-index 0 --output ./file
```

`mail.*` 默认通过 `im-core::email` 执行。CLI 负责 flags、dry-run、输出 envelope 和附件文件写入。

## 9. Page 与 Site

Handle page：

```bash
awiki-cli page list
awiki-cli page get --slug about
awiki-cli page create --slug about --markdown-file ./about.md
awiki-cli page update --slug about --markdown-file ./about-v2.md
awiki-cli page delete --slug about
```

Tenant site page：

```bash
awiki-cli site root get --domain example.com
awiki-cli site root set --domain example.com --markdown-file ./root.md
awiki-cli site page list --domain example.com
awiki-cli site page create --domain example.com --slug about --markdown-file ./about.md
```

`page` 绑定当前身份的 handle；`site` 绑定显式 tenant domain，不从当前 DID/handle 反推。

## 10. Runtime

```bash
awiki-cli runtime setup --mode websocket
awiki-cli runtime status
awiki-cli runtime listener status
awiki-cli runtime listener enable
awiki-cli runtime listener disable
awiki-cli runtime host-notify enable
awiki-cli runtime host-notify disable
```

Operator / diagnostic 常用命令：

```bash
awiki-cli runtime setup --mode http
awiki-cli runtime apply
awiki-cli runtime listener start
awiki-cli runtime listener stop
awiki-cli runtime host-notify config show
awiki-cli runtime host-notify config set --sink hermes
awiki-cli runtime host-notify hermes setup
awiki-cli runtime host-notify hermes status
```

边界：

- `im-core` 提供 realtime runner/session/event。
- CLI 负责 systemd/launchd/Windows service、socket/pipe、pid/log/status 和 host notification 配置。
- host notification sink 属于 runtime UX，不进入普通 message API。
- CLI runtime listener 是 `awiki-cli` 的本机消息接收辅助能力；awiki-me 使用的 `awiki-deamon` release / install / upgrade 流程见 `docs/publish.md`，不通过 `awiki-cli runtime listener` 管理。
- `runtime.heartbeat *` 是 stable unsupported，不属于当前 CLI runtime 产品面。

## 11. Debug

`debug` 域用于专家排障和本地 inspection，需要 `--diagnostic` gate：

```bash
awiki-cli --diagnostic debug db handle-history alice
```

规则：

- Debug 命令不进入默认 Agent 工作流。
- Debug 输出也必须遵守脱敏规则。
- 不要把 debug/raw 能力提升为普通产品命令。
- `debug db query` 是 stable unsupported；raw SQL 不属于当前支持能力。
- `debug raw *` 已移除；如果需要新增专家诊断能力，应定义受控命令和脱敏输出，而不是恢复 raw RPC。

## 12. Schema 与 Docs

`--help` / `-h` 用于人类可读的简洁命令说明。`schema` 用于机器读取命令、参数、输出、能力状态和默认 surface。`docs` 用于内置文档主题索引。

```bash
awiki-cli schema
awiki-cli schema --audience operator
awiki-cli schema --all
awiki-cli docs
awiki-cli docs architecture
awiki-cli tenant --help
awiki-cli tenant create --help
```

命令树、schema、help、docs 和 skill reference 必须保持一致，并共用 command catalog 作为命令事实源。`--help` 默认输出纯文本，不包 JSON envelope；完整机器可读数据只通过 `schema` 输出。新增或改名命令时，需要同步相关文档和测试。默认用户文档不得推荐 `schema` 中标记为 unsupported、removed、hidden、diagnostic-only、migration-only 或 internal-only 的能力，除非文案明确说明它是非默认面。

## 13. 发布与版本

公开版本以 `package.json.version` 为事实源，并通过 `xtask check-version` 约束 npm package、`crates/awiki-cli/Cargo.toml` 和 release tag。

本地开发可使用：

```bash
cargo run -p awiki-cli -- --help
cargo install --path crates/awiki-cli --locked
```

正式发布流程见 `docs/publish.md`。
