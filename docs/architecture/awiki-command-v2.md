# awiki-cli 命令契约

## 1. 目标

`awiki-cli` 是 awiki 的统一命令行产品面。命令按用户意图和产品资源组织，而不是按底层脚本、RPC 方法或 SDK 内部模块组织。

核心要求：

- 默认非交互，适合人类和 Agent 调用。
- 所有有副作用命令支持 `--dry-run`。
- 所有命令返回结构化输出，详见 `docs/architecture/output-format.md`。
- `schema`、help、completion、docs 和 skill reference 应从同一命令事实源保持一致。
- 业务流程通过 `im-core` public services 执行；CLI 保留 parse/build/call/render。

## 2. 顶层命令

当前公共顶层命令：

```text
awiki-cli status
awiki-cli docs
awiki-cli schema
awiki-cli doctor
awiki-cli version
awiki-cli completion
awiki-cli config
awiki-cli id
awiki-cli msg
awiki-cli mail
awiki-cli group
awiki-cli people
awiki-cli page
awiki-cli site
awiki-cli runtime
awiki-cli debug
```

命令归属：

- `id`：身份、Handle、profile、recover、replace DID。
- `msg`：direct/group 消息、inbox/history、mark-read、附件发送/下载、消息 secure 操作。
- `mail`：邮件 inbox/read/send/mark-read/attachment。
- `group`：群生命周期、成员、群消息读取、group secure 状态。
- `people`：联系人、关系、profile/directory 相关能力。
- `page`：当前身份 handle 级内容页。
- `site`：tenant bare-domain site pages，必须显式 `--domain`。
- `runtime`：运行模式、listener、service、host notification。
- `debug`：本地诊断、数据库 inspection 和专家排障。

## 3. 全局参数

| 参数 | 说明 |
| --- | --- |
| `--identity` | 选择本地身份；未传时使用默认身份。 |
| `--format` | 输出格式：`json`、`pretty`、`table`、`ndjson`。 |
| `--jq` | 对 JSON 输出执行过滤。 |
| `--dry-run` | 返回计划，不执行副作用。 |
| `--verbose` | 增加诊断上下文。 |

规则：

- `--identity` 是用户层身份选择参数；legacy credential 只能作为兼容概念。
- `--format json` 是 canonical 输出。
- `--jq` 不应改变命令语义，只过滤 JSON 结果。
- `--dry-run` 不能写远端状态或本地业务状态。

## 4. Identity

常用命令：

```bash
awiki-cli id list
awiki-cli id current
awiki-cli id use --identity alice
awiki-cli id status
awiki-cli id register --handle alice
awiki-cli id bind --email alice@example.com
awiki-cli id recover --handle alice --phone +12025550123
awiki-cli id profile get
awiki-cli id profile set --markdown-file ./profile.md
```

边界：

- `id` 命令的业务能力归 `im-core::identity` 和 `im-core::auth`。
- CLI 负责 OTP 输入、文件读取、default identity 文件写入、dry-run 和输出。
- 私钥、JWT、DID document 写入细节不进入普通输出。

## 5. Messaging

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

## 6. Group

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
- 低层 `group e2ee *` 不属于默认产品契约；secure 用户入口是高层 status/repair 和 `--secure required`。

## 7. Mail

```bash
awiki-cli mail inbox --folder inbox --limit 20
awiki-cli mail read --id MESSAGE_ID
awiki-cli mail mark-read MESSAGE_ID
awiki-cli mail send --to a@example.com --subject "Hello" --body "Hi"
awiki-cli mail attachment download --message-id MESSAGE_ID --attachment-index 0 --output ./file
```

`mail.*` 默认通过 `im-core::email` 执行。CLI 负责 flags、dry-run、输出 envelope 和附件文件写入。

## 8. Page 与 Site

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

## 9. Runtime

```bash
awiki-cli runtime setup --mode websocket
awiki-cli runtime apply
awiki-cli runtime listener status
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

## 10. Debug

`debug` 域用于专家排障和本地 inspection：

```bash
awiki-cli debug db query --sql "select ..."
awiki-cli debug db handle-history alice
```

规则：

- Debug 命令不进入默认 Agent 工作流。
- Debug 输出也必须遵守脱敏规则。
- 不要把 debug/raw 能力提升为普通产品命令。

## 11. Schema 与 Docs

`schema` 用于机器读取命令、参数、输出、能力状态和默认 surface。`docs` 用于内置文档主题索引。

```bash
awiki-cli schema
awiki-cli docs list
awiki-cli docs topic architecture
```

命令树、schema、help、docs 和 skill reference 必须保持一致。新增或改名命令时，需要同步相关文档和测试。

## 12. 发布与版本

公开版本以 `package.json.version` 为事实源，并通过 `xtask check-version` 约束 npm package、`crates/awiki-cli/Cargo.toml` 和 release tag。

本地开发可使用：

```bash
cargo run -p awiki-cli -- --help
cargo install --path crates/awiki-cli --locked
```

正式发布流程见 `docs/publish.md`。
