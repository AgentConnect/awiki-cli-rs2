可以。下面这份我按“**最终定版**”来写：它保留了 `cli-init.md` 和 `output-format.md` 里的核心内容——统一 CLI、core/extension 分层、消息三维模型、`schema`、`--dry-run`、`--jq`、shortcut、结构化输出、skill 拆分、legacy 映射——但当前仓库已经正式切换成 **Rust CLI port**。早期 Go 方案只作为命令契约和发布命名的历史来源。

我下面统一使用 **`awiki-cli`** 作为最终命令名和主二进制名；项目名仍保持 awiki 体系，skill 命名继续保留 `awiki-*`。

---

# awiki-cli v2 最终可执行方案（Rust CLI port）

## 1. 定版目标

awiki-cli v2 的目标不是“把旧 Python 脚本逐个翻译成 Rust”，而是建立一个**对 AI 和人都稳定可路由的任务模型**：

* 按**用户意图**组织，而不是按脚本实现组织。
* 命令默认**非交互**。
* 所有有副作用的命令都支持 `--dry-run`。
* 所有命令都支持**结构化输出**。
* 提供可机器读取的 `schema`。
* skill 采用 `single entry + lazy-loaded references`，不再把所有内容塞进一个巨型 `SKILL.md`，也不再默认装载多层 skill。 

这次语言切换到 Rust CLI port 后，**命令契约不变**，变的是实现和发布：

* 命令树、参数语义、shortcut 规则、输出协议，保持 v2 设计；
* 实现层改成 Rust；
* 发布改成多平台原生二进制；
* 文档、schema、shell completion、man page 从代码自动生成。

---

## 2. 产品边界与核心模型

### 2.1 核心能力

CLI 的主线只有三块：

* `id`：身份生命周期
* `msg`：消息生命周期
* `runtime`：运行时与接收机制

辅助命令：

* `status`
* `schema`
* `doctor`
* `docs`
* `version`
* `completion`

扩展能力单独分组：

* `people`
* `page`
* `discovery`
* `debug` 

### 2.2 统一消息模型

主文档里建议直接写死：

```text
Message =
  Target(scope: direct | group)
  × Security(plain | e2ee)
  × ReceiveMode(pull | realtime)
```

其中 AI 需要主动选择的只有：

* `scope`：私聊 / 群聊
* `security`：明文 / E2EE

`ReceiveMode` 属于 `runtime`，不属于 `msg` 路由。当前支持矩阵建议明确为：

```text
direct + plain  = supported
direct + e2ee   = supported
group  + plain  = supported
group  + e2ee   = hidden/test-only supported; public discovery still disabled
```

 

---

## 3. 最终命令树

## 3.1 canonical 命令树

这是最终冻结的主命令树：

```bash
awiki-cli status
awiki-cli docs [TOPIC]
awiki-cli schema [COMMAND]
awiki-cli doctor
awiki-cli version
awiki-cli init
awiki-cli completion <bash|zsh|fish|powershell>

awiki-cli id status
awiki-cli id create --name "Alice" [--identity alice]
awiki-cli id register --handle alice (--phone +8613800138000 [--otp 123456] [--invite-code ABC123] | --email user@example.com [--wait]) [--identity alice]
awiki-cli id bind (--phone +8613800138000 [--otp 123456] | --email user@example.com [--wait]) [--identity alice]
awiki-cli id resolve (--handle alice | --did did:wba:...)
awiki-cli id recover --handle alice --phone +8613800138000 --otp 123456 [--identity alice]
awiki-cli id list
awiki-cli id current
awiki-cli id use alice
awiki-cli --identity alice id replace-did [--is-public] [--is-agent] [--role <role>] [--endpoint-url <url>] # dangerous DID replacement
awiki-cli id profile get [--self | --handle alice | --did did:wba:...]
awiki-cli id profile set [--display-name "Alice"] [--bio "..."] [--tags "ai,did,agent"] [--markdown "# About Me"] [--markdown-file ./profile.md]

awiki-cli msg send (--to TARGET | --group GROUP_DID) [--text "Hello"] [--text-file ./message.txt] [--file ./hello.txt] [--mime-type text/plain] [--type text|event] [--secure off|on] [--identity alice]
awiki-cli msg attachment download (--with TARGET | --group GROUP_DID) --message-id MSG_ID [--attachment-id ATTACHMENT_ID] --output ./downloads/file.bin [--identity alice]
awiki-cli msg inbox [--scope all|direct|group] [--with TARGET] [--group GROUP_DID] [--unread] [--limit 20] [--mark-read] [--identity alice]
awiki-cli msg history --with TARGET [--limit 50] [--cursor CURSOR] [--identity alice]
awiki-cli msg mark-read MSG_ID...

`msg attachment download` 会按 `message_id` 分页扫描 direct history 或 group messages，直到命中目标附件消息，而不是只检查最新一页结果。

awiki-cli group create --name "Agent War Room" [--description "..."] [--discoverability private|listed|public] [--admission-mode admin-add|open-join] [--message-security-profile transport-protected|group-e2ee] [--e2ee] [--slug agent-war-room] [--goal "..."] [--rules "..."] [--message-prompt "..."] [--doc-url "https://..."] [--attachments-allowed] [--max-members 500] [--member-max-messages 10] [--member-max-total-chars 2000] [--identity alice]
awiki-cli group get --group GROUP_DID [--identity alice]
awiki-cli group join --group GROUP_DID [--reason "..."] [--identity alice]
awiki-cli group add --group GROUP_DID --member did:wba:... [--role member|admin] [--reason "..."] [--e2ee] [--identity alice]
awiki-cli group remove --group GROUP_DID --member did:wba:... [--reason "..."] [--e2ee] [--identity alice]
awiki-cli group members --group GROUP_DID [--limit 100] [--identity alice]
awiki-cli group messages --group GROUP_DID [--limit 50] [--cursor CURSOR] [--identity alice]
awiki-cli group update --group GROUP_DID [--name "..."] [--description "..."] [--discoverability private|listed|public] [--admission-mode admin-add|open-join] [--slug "..."] [--goal "..."] [--rules "..."] [--message-prompt "..."] [--doc-url "https://..."] [--attachments-allowed=true|false] [--max-members 500] [--member-max-messages 10] [--member-max-total-chars 2000] [--identity alice]
awiki-cli group leave --group GROUP_DID [--reason "..."] [--e2ee] [--identity alice]
awiki-cli group e2ee publish-key-package [--purpose normal|recovery|update] [--group GROUP_DID] [--device default] [--identity alice]
awiki-cli group e2ee update-key --group GROUP_DID --member did:wba:... [--device default] [--identity alice]
awiki-cli group e2ee rejoin --group GROUP_DID --member did:wba:... [--role member] [--identity alice]
awiki-cli group e2ee process-leave-request --group GROUP_DID --member did:wba:... [--leave-request-id LR_ID] [--reason "..."] [--identity alice]

测试与示例约定：

- DID / Group DID 的 profile 段默认使用 `e1_...` 形式，例如 `did:wba:example.com:user:alice:e1_alice`、`did:wba:example.com:groups:demo:e1_group`。
- 不再新增裸 `:e1` 的测试 fixture 或命令示例。

awiki-cli msg secure status [--with TARGET] [--identity alice]
awiki-cli msg secure init --with TARGET [--identity alice]
awiki-cli msg secure repair --with TARGET [--identity alice]
awiki-cli msg secure failed [--identity alice]
awiki-cli msg secure retry OUTBOX_ID [--identity alice]
awiki-cli msg secure drop OUTBOX_ID [--identity alice]

awiki-cli runtime status [--identity NAME]
awiki-cli runtime apply
awiki-cli runtime setup [--mode http|websocket] [--identity NAME]
awiki-cli runtime mode get
awiki-cli runtime mode set http|websocket
awiki-cli runtime listener status
awiki-cli runtime listener install
awiki-cli runtime listener start
awiki-cli runtime listener stop
awiki-cli runtime listener restart
awiki-cli runtime listener uninstall
awiki-cli runtime listener config show
awiki-cli runtime listener config set [--enabled true|false] [--auto-install true|false] [--auto-start true|false]
awiki-cli runtime listener enable
awiki-cli runtime listener disable
awiki-cli runtime host-notify config show
awiki-cli runtime host-notify enable
awiki-cli runtime host-notify disable
awiki-cli runtime host-notify config set --sink noop|log|file|openclaw
awiki-cli runtime host-notify openclaw set [--hook-url ...]
awiki-cli runtime host-notify openclaw set-token --value <token>
awiki-cli runtime host-notify openclaw clear-token
awiki-cli runtime heartbeat status
awiki-cli runtime heartbeat install [--every 15m]
awiki-cli runtime heartbeat run-once

awiki-cli people search "AI agent"
awiki-cli people follow TARGET
awiki-cli people unfollow TARGET
awiki-cli people status TARGET
awiki-cli people followers
awiki-cli people following
awiki-cli people contacts list
awiki-cli people contacts save --did DID --handle HANDLE --reason "..."

awiki-cli page create --slug jd --title "Hiring" --markdown-file ./jd.md [--visibility public|draft|unlisted]
awiki-cli page list
awiki-cli page get --slug jd
awiki-cli page update --slug jd [--title "..."] [--markdown "..."] [--markdown-file ./x.md] [--visibility public|draft|unlisted]
awiki-cli page rename --slug jd --to hiring
awiki-cli page delete --slug hiring

awiki-cli site root get --domain xianglianggongshi.cn
awiki-cli site root set --domain xianglianggongshi.cn --markdown-file ./root.md
awiki-cli site page list --domain xianglianggongshi.cn
awiki-cli site page get --domain xianglianggongshi.cn --slug about
awiki-cli site page create --domain xianglianggongshi.cn --slug about --markdown-file ./about.md
awiki-cli site page update --domain xianglianggongshi.cn --slug about --markdown-file ./about-v2.md
awiki-cli site page rename --domain xianglianggongshi.cn --slug about --to intro
awiki-cli site page delete --domain xianglianggongshi.cn --slug intro

awiki-cli discovery scan --group GROUP_ID
awiki-cli discovery recommend --group GROUP_ID
awiki-cli discovery draft-intro --group GROUP_ID
awiki-cli discovery draft-dm --group GROUP_ID --member DID

awiki-cli debug db handle-history alice
awiki-cli debug db query "SELECT ..."
awiki-cli debug raw rpc ...
awiki-cli debug schema-cache
awiki-cli debug logs [--follow]
```

这棵树延续了两份文档的核心设计：`status / docs / schema / doctor / id / msg / runtime` 作为核心，`people / page / discovery / debug` 作为扩展；其中**所有发送动作都统一收敛到 `msg send`**，不再按“私聊脚本 / 群发脚本 / E2EE 脚本”分裂。 

### page 与 site 的边界

- `page` 表示 **handle 级 content page**，绑定当前身份关联的 handle，用于原有内容发布能力。
- `site` 表示 **tenant bare-domain site page**，绑定租户域名根路径和 `/pages/{slug}.md`，必须显式传 `--domain`。
- 两者是两个独立产品面，不共享 slug 空间、不共享存储，也不应在文档或帮助文本中混用。

## 3.2 新增的 Rust CLI 标准命令

相比原草案，我建议在 Rust CLI port 里正式加入：

```bash
awiki-cli docs [TOPIC]
awiki-cli init
awiki-cli completion <bash|zsh|fish|powershell>
```

`docs` 作为一级命令，用于承载 onboarding、identity、secure-messaging、transport-modes 等产品内建文档。

`init` 作为显式初始化命令，用于：

* 帮用户创建工作目录（默认是 `~/.awiki-cli`，仅支持 `AWIKI_CLI_WORKSPACE_HOME_DIR` 作为工作区根目录覆盖）及其子目录；
* 在首次需要时生成一份最小的 `config.yaml` 骨架；

当前 Rust CLI port 以 `cmdmeta` 作为命令树元数据源，并由 CLI parser、`schema`、docs/skill 校验共同消费；这能把命令、帮助、completion、文档和 LLM 索引统一起来。

---

## 4. shortcut 设计

shortcut 要加，但**只能是 canonical command 的别名**，不能形成第二套语义。这个原则保留。

建议保留 8 个以内：

```bash
awiki-cli setup                 # = awiki-cli runtime setup
awiki-cli register ...          # = awiki-cli id register ...
awiki-cli whoami                # = awiki-cli id current
awiki-cli inbox                 # = awiki-cli msg inbox
awiki-cli dm alice "hello"      # = awiki-cli msg send --to alice --text "hello"
awiki-cli secure alice "secret" # = awiki-cli msg send --to alice --text "secret" --secure on
awiki-cli group get --group did:wba:... # top-level canonical group lifecycle entry
awiki-cli history alice         # = awiki-cli msg history --with alice
```

最终规则：

* shortcut 只能是 alias
* shortcut 不能有独占语义
* shortcut 数量控制在 6–8 个
* 文档先写 canonical，再写 shortcut
* 不引入飞书那种 `+xxx` 作为主语法

另外，**canonical 命令默认 JSON，shortcut 默认 pretty/table**。这点保留 output-format 的结论：协议层以 JSON 为主，展示层可以更友好。

---

## 5. 参数命名与冲突收敛

这里我把两份文档里不一致的地方做了统一。

## 5.1 全局参数

最终定为：

```bash
--identity <name>          # canonical
--credential <name>        # legacy alias

--format json|pretty|table|ndjson
--output ...               # legacy alias of --format
--json                     # alias of --format json
--jq '<expr>'
--dry-run
--yes
--verbose
--no-color
```

这样做有两个好处：

第一，把 `cli-init.md` 里的 `--output` 和 `output-format.md` 里的 `--format` 收敛到一个最终名字：**`--format`**。
第二，把 `human` 收敛成 `pretty`；如果你还想兼容旧写法，可以把 `human` 当成 `pretty` 的 deprecated alias。 

## 5.2 目标与身份参数

```bash
--to <handle|did>
--group <group-id>
--did <did>
--handle <handle>
--with <target>
```

规则：

* 发消息只用 `--to` 或 `--group`
* 历史、secure peer 用 `--with`
* 只有显式解析场景才用 `--did` / `--handle`
* `--peer`、`--target-did` 不再扩散

## 5.3 内容参数

```bash
--text "..."
--text-file ./message.txt

--markdown "..."
--markdown-file ./doc.md
```

规则：

* 消息一律 `--text`
* Markdown 一律 `--markdown`
* 不再混用 `--content` / `--body` / `--profile-md`

## 5.4 控制参数

```bash
--secure off|on
--mode http|websocket
--wait
--limit 50
--cursor CURSOR
--force
```

`--wait` 只允许轮询，不允许 CLI 内部 prompt；CLI 仍然保持非交互默认。

---

## 6. 返回协议：JSON 为契约，pretty/table/ndjson 为视图

这里按 `output-format.md` 直接定版：**CLI 的标准返回是 JSON，自然语言只是 JSON 里的字段，不是命令契约本身。** `pretty`、`table`、`ndjson` 都是 JSON 的渲染形式。

## 6.1 默认输出规则

* canonical command：默认 `--format json`
* shortcut：默认 `--format pretty` 或 `table`
* 流式命令：只允许 `--format ndjson`

## 6.2 成功/失败统一信封

我建议最终 Rust 代码里统一成这一套：

```json
{
  "ok": true,
  "command": "awiki-cli msg send",
  "data": {},
  "warnings": [],
  "summary": "",
  "_notice": {},
  "meta": {
    "version": "2.0.0",
    "identity": {
      "name": "alice",
      "did": "did:wba:awiki.ai:user:abc...xyz"
    },
    "dry_run": false,
    "format": "json"
  }
}
```

```json
{
  "ok": false,
  "error": {
    "code": "permission_denied",
    "message": "Missing required permission",
    "hint": "Run awiki-cli id use alice or refresh identity",
    "retryable": false,
    "details": {}
  },
  "_notice": {},
  "meta": {
    "version": "2.0.0",
    "dry_run": false,
    "format": "json"
  }
}
```

这里我也做了一个冲突收敛：
`cli-init.md` 里有 `notice`，`output-format.md` 里用的是 `_notice`。最终建议统一为 **`_notice`**；`notice` 只在 legacy 模式兼容，不再作为新输出字段。 

## 6.3 命令类别与返回内容

查询类命令返回**事实状态**：

* `status`
* `id status`
* `msg inbox`
* `msg history`
* `people search`

写操作返回**发生了什么变更**：

* `id register`
* `id replace-did`（危险维护命令；必须先确认 `--identity` 目标并优先 `--dry-run`）
* `msg send`
* `msg group join`
* `people follow`
* `page create`

异步命令返回**任务状态**：

* `runtime setup`
* `listener install`
* `discovery scan`（如做异步）
* 大批量同步/发布

流式命令一律 `ndjson`：

* `runtime listener logs --follow`
* 未来的 `msg watch`
* 未来的 `heartbeat watch`

这部分保持 output-format 文档的原结论。 

## 6.4 `--dry-run`

所有有副作用的命令都必须支持 `--dry-run`，并返回 `plan`。
`--dry-run` 时允许参数校验、本地只读检查、安全的 GET 预检；禁止发送 OTP、邮件、消息，禁止远端写入，禁止本地持久化。 

## 6.5 `--jq`

`--jq` 必须做，但在 Rust port 中应使用 Rust 侧实现或既有表达式求值模块，不再引入 Go 专用 `gojq` 方案。对 awiki-cli 来说，关键是 `--jq` 的输出契约稳定，而不是绑定某个 Go 依赖。

## 6.6 错误码与退出码

错误码集合定为：

```text
invalid_argument
identity_required
auth_required
permission_denied
not_found
conflict
network_error
transport_unavailable
secure_session_required
unsupported_mode
partial_failure
internal_error
```

退出码定为：

```text
0 success
1 generic error
2 invalid argument
3 identity/auth missing
4 permission denied
5 not found
6 partial failure
7 confirmation required but not provided
```

---

## 7. `schema`：从“命令说明”升级成“机器契约”

`schema` 不是文档别名，而是**命令元数据接口**。这是两份文档里最值得保留的设计之一。 

最终形态：

```bash
awiki-cli schema
awiki-cli schema msg.send
awiki-cli schema id.register
awiki-cli schema runtime.listener.install
awiki-cli schema --skills
```

返回结构至少包括：

```json
{
  "name": "awiki-cli msg send",
  "summary": "Send a direct or group message",
  "aliases": ["awiki-cli dm"],
  "side_effect": true,
  "confirm_required": false,
  "dry_run_supported": true,
  "identity_required": true,
  "output_formats": ["json", "pretty", "table", "ndjson"],
  "capabilities": {
    "direct_plain": true,
    "direct_e2ee": true,
    "group_plain": true,
    "group_e2ee": false,
    "group_e2ee_hidden_test_only": true
  },
  "args": [],
  "returns": {},
  "errors": [],
  "examples": [],
  "legacy_maps_to": [
    "scripts/send_message.py",
    "scripts/e2ee_messaging.py --send",
    "scripts/manage_group.py --post-message"
  ]
}
```

最终文件布局：

```text
schemas/
  cli.json
  commands/
    awiki-cli.status.json
    awiki-cli.id.register.json
    awiki-cli.msg.send.json
    awiki-cli.msg.group.join.json
    awiki-cli.runtime.setup.json
```

并且要求：

* `awiki-cli schema` 从代码内的 command registry 直接生成
* `CLI_REFERENCE.md` 从 schema 生成
* `skills/SKILL.md` 与 `skills/references/*.md` 中的命令引用由 CI 校验
* shell help、Markdown docs、man pages 一起自动生成

`cmdmeta` 应继续作为 Rust port 的命令树事实源，用于保持命令解析、schema、skill 文档和 CI 校验同步。

---

## 8. Rust 实现架构

## 8.1 技术选型

当前 Rust CLI port 固定为这套：

* 命令树事实源：**`cmdmeta`**
* JSON 编码：**serde / serde_json**
* 发布：**Cargo release build scripts + GitHub Releases + npm wrapper**
* 后台 listener/service：**Rust runtime/service modules**
* 本地 SQLite：**rusqlite bundled SQLite**

命令树、schema、parser、stub 元数据应从 `cmdmeta` 派生；发布版本以 `package.json.version` 为公开事实源，并通过 `xtask check-version` 约束 Cargo crate、npm package、release tag 与 buildinfo 注入。未注入 `AWIKI_CLI_VERSION` 的本地测试/开发构建仍显示为 `dev`，以保留 dev-build update 策略。

## 8.2 工程目录

我建议 repo 最终长这样：

```text
.
├── crates/
│   └── awiki-cli/
│       ├── src/
│       │   ├── app/
│       │   ├── cli/
│       │   ├── cmdmeta/
│       │   ├── config/
│       │   ├── output/
│       │   └── runtime/
│       └── tests/
├── xtask/
│   └── src/
├── scripts/
│   └── release/
├── skills/
│   ├── SKILL.md
│   ├── README.md
│   └── references/
│       ├── 00-installation.md
│       ├── 01-onboarding.md
│       ├── 02-identity.md
│       ├── 03-messaging.md
│       ├── 04-groups.md
│       ├── 05-runtime.md
│       ├── 06-pages.md
│       ├── 07-discovery.md
│       ├── 08-debug.md
│       └── 09-people-planned.md
├── schemas/
├── docs/
│   └── cli/
├── Cargo.toml
├── Cargo.lock
└── package.json
```

这里推荐的 skill 文档结构以当前仓库的 `skills/` 为准：默认只加载 `SKILL.md`，其余领域与 workflow 内容全部下沉到 `references/`，按任务懒加载。

## 8.3 代码分层

强制遵守这条规则：

* CLI parser 层只负责参数解析、校验、调用 handler
* handler 只返回**typed result**
* 所有 stdout/stderr 输出统一走 renderer
* 业务层不直接打印
* `schema`、help、docs、skill 引用全部来自统一 registry

这能避免最常见的 CLI 漂移：代码、help、docs、skill 四套说法各自为政。

## 8.4 推荐的核心 Rust 类型

建议从一开始就把“输出契约”和“schema 契约”写成显式类型：

```go
type Envelope[T any] struct {
	OK       bool        `json:"ok"`
	Command  string      `json:"command"`
	Data      *T         `json:"data,omitempty"`
	Error     *ErrBody   `json:"error,omitempty"`
	Warnings []Warning   `json:"warnings,omitempty"`
	Summary  string      `json:"summary,omitempty"`
	Notice   *Notice     `json:"_notice,omitempty"`
	Meta     Meta        `json:"meta"`
}

type ErrBody struct {
	Code      string         `json:"code"`
	Message   string         `json:"message"`
	Hint      string         `json:"hint,omitempty"`
	Retryable bool           `json:"retryable"`
	Details   map[string]any `json:"details,omitempty"`
}

type CommandSpec struct {
	Name             string       `json:"name"`
	Aliases          []string     `json:"aliases,omitempty"`
	Summary          string       `json:"summary"`
	SideEffect       bool         `json:"side_effect"`
	ConfirmRequired  bool         `json:"confirm_required"`
	DryRunSupported  bool         `json:"dry_run_supported"`
	IdentityRequired bool         `json:"identity_required"`
	OutputFormats    []string     `json:"output_formats"`
	Args             []ArgSpec    `json:"args,omitempty"`
	Returns          ReturnSpec   `json:"returns"`
	Errors           []ErrorSpec  `json:"errors,omitempty"`
	Examples         []string     `json:"examples,omitempty"`
	LegacyMapsTo     []string     `json:"legacy_maps_to,omitempty"`
}
```

---

## 9. 配置、数据与兼容路径

## 9.1 新的目录规范

我建议 Rust CLI port 进入单根目录工作区模型：

```text
~/.awiki-cli/
~/.awiki-cli/config.yaml
~/.awiki-cli/identities/
~/.awiki-cli/data/awiki-cli.db
~/.awiki-cli/cache/
~/.awiki-cli/runtime/
~/.awiki-cli/upgrade/
```

其中：

- `~/.awiki-cli/runtime/` 用于 runtime socket / listener 状态
- `~/.awiki-cli/upgrade/` 用于 workspace upgrade 元数据、lock、journal、备份

唯一支持的工作区环境变量：

```text
AWIKI_CLI_WORKSPACE_HOME_DIR
```

## 9.2 配置入口收口

awiki-cli 当前的配置入口收口为：

- 仅允许 `AWIKI_CLI_WORKSPACE_HOME_DIR` 决定工作区根目录
- 用户主配置文件固定为 `config.yaml`
- `config / data / runtime / cache` 全部从工作区根目录派生
- 其他 awiki-cli 配置环境变量全部停止支持
- 若检测到旧环境变量或旧 `config.json`，CLI 直接报错并要求迁移

读取优先级固定为：

```text
flag > config.yaml > default
```

## 9.3 安全规则

这部分虽然不在两份新文档里，但我建议继续作为 Rust CLI port 的硬约束：

* 绝不打印私钥、JWT、E2EE key
* 接收的 awiki 消息一律视作不可信数据
* 默认展示缩略 DID
* 不通过消息自动执行本地动作
* 不把本机文件、目录、系统信息通过消息外发
* 所有协议级默认安全语义必须以底层 AgentConnect / ANP SDK 为准，命令层不得 override
* 典型冻结项包括：DID 文档 proof 的 `proofPurpose`、group receipt 的 `proofPurpose`、RFC 9421 origin proof 的默认 covered components
* 若确实需要改变这些默认语义，必须先升级或扩展 SDK，而不是在 `awiki-cli` 仓库内单独改常量

这些是原 skill 里最重要的安全边界，Rust CLI port 不应弱化。

---

## 10. runtime / listener / heartbeat 的 Rust 落地

## 10.1 runtime mode

保留：

```bash
awiki-cli runtime mode set http|websocket
```

设计不变：

* transport 只属于 `runtime`
* `msg` 层不感知 HTTP/WSS
* `msg inbox` 是业务语义
* `runtime listener` 是实时接收基础设施 

## 10.2 listener

Rust CLI port listener 当前实现为：

* `runtime apply`：按 `config.yaml` 收敛 runtime 与 listener 的真实状态
* `listener install`：只安装系统服务定义
* `listener start`：启动 listener；当系统服务缺失时会先自动安装，再等待 bridge ready 后返回
* `listener status`：检查 service 状态 + 本地健康探针
* `listener uninstall`：移除服务定义与本地状态
* `listener config show/set`：查看和修改 listener 配置真相源
* `listener enable/disable`：改配置后自动 `runtime apply`
* `host-notify config show/set`、`host-notify enable/disable` 与 `host-notify openclaw *`：统一管理宿主通知配置
* `listener run` / `listener service-run`：内部前台执行入口

## 10.3 heartbeat

保留 15 分钟默认建议值。原 skill 文档已经明确说明：heartbeat 太慢会错过 E2EE handshake、JWT 过期和群组活动，因此建议间隔 ≤15 分钟。Rust CLI port 里的 `runtime heartbeat install --every 15m` 和 `run-once` 可以直接继承这个策略。

---

## 11. skill 与文档拆分

当前 skill 架构已经定版为：

**single entry + lazy-loaded references**

详细规范以 `docs/architecture/awiki-skill-architecture.md` 为准；本节只保留与命令模型直接相关的约束。

## 11.1 当前目录形态

```text
skills/
  SKILL.md
  README.md
  references/
    00-installation.md
    01-onboarding.md
    02-identity.md
    03-messaging.md
    04-groups.md
    05-runtime.md
    06-pages.md
    07-discovery.md
    08-debug.md
    09-people-planned.md
```

## 11.2 命令模型对 skill 的要求

必须满足：

* 默认只加载 `skills/SKILL.md`
* 共享规则、输出协议、确认规则、安全边界统一收敛在入口层
* 单领域任务只补读一个 matching reference
* workflow 任务只补读一个 matching workflow reference
* `08-debug.md` 只作为最后兜底入口
* `09-people-planned.md` 与 `00-installation.md` 不进入默认上下文

## 11.3 路由约束

* identity 任务 -> `02-identity.md`
* messaging 与 `msg send --group` -> `03-messaging.md`
* group lifecycle / members / policy -> `04-groups.md`
* runtime / listener / host notify -> `05-runtime.md`
* pages -> `06-pages.md`
* onboarding -> `01-onboarding.md`
* discovery workflow -> `07-discovery.md`
* low-level debug -> `08-debug.md`

## 11.4 文档生成与校验

原则不变：

* 一个主题只有一个权威入口
* `SKILL.md` 只负责路由与默认行为
* CLI 细节和命令契约以 CLI 自身的 `docs / schema / help` 为准
* skill/reference 中的命令引用应由统一 registry 和 CI 校验，避免与真实命令面漂移

---

## 12. 旧脚本迁移映射

这一部分直接保留，但目标从 Python wrapper 改成 Rust CLI port：

```text
scripts/check_status.py                  -> awiki-cli status
scripts/setup_identity.py                -> awiki-cli id create / id list / id use
scripts/send_verification_code.py        -> awiki-cli id register / id bind
scripts/register_handle.py               -> awiki-cli id register
scripts/bind_contact.py                  -> awiki-cli id bind
scripts/resolve_handle.py                -> awiki-cli id resolve
scripts/recover_handle.py                -> awiki-cli id recover
scripts/get_profile.py                   -> awiki-cli id profile get
scripts/update_profile.py                -> awiki-cli id profile set
scripts/send_message.py                  -> awiki-cli msg send --secure off
scripts/e2ee_messaging.py --send         -> awiki-cli msg send --secure on
scripts/check_inbox.py                   -> awiki-cli msg inbox / msg history / msg mark-read
scripts/manage_group.py                  -> awiki-cli msg group ...
scripts/manage_group.py --post-message   -> awiki-cli msg send --group ...
scripts/setup_realtime.py                -> awiki-cli runtime setup
scripts/ws_listener.py                   -> awiki-cli runtime listener ...
scripts/query_db.py                      -> awiki-cli debug db query
```

这层映射建议做成：

* `legacy/` 中的兼容说明
* `schema` 里的 `legacy_maps_to`
* `doctor` 的迁移提示
* 旧脚本执行时输出 deprecation hint

这部分正是 `cli-init.md` 里最适合直接继承的内容。

---

## 13. 包发布与安装

既然已经切到 Rust CLI port，我建议发布路径也跟着定：

## 13.1 首发方式

首发只做：

* GitHub Releases
* npm wrapper（`@awiki/cli`）
* 多平台二进制：

  * macOS: amd64 / arm64
  * Linux: amd64 / arm64
  * Windows: amd64 / arm64
* checksum
* shell completions
* man pages
* `docs/cli` Markdown 文档

当前仓库使用 Rust release scripts 触发 Cargo 构建、GitHub Release artifact 生成与 npm wrapper 发布。版本规则固定为 `package.json.version` -> `v${version}` tag -> `AWIKI_CLI_VERSION` buildinfo 注入。

## 13.2 安装方式

建议四种：

```bash
# 1) 直接下载 release 压缩包
# 2) npm install -g @awiki/cli
# 3) curl 安装脚本（官方自建）
# 4) cargo install（仅开发者/高级用户，本地源码）
cargo install --path crates/awiki-cli --locked
```

首发阶段直接支持 **npm wrapper**。
推荐同时提供 GitHub Releases、npm wrapper 和 Cargo 本地安装：人类用户可以走 npm / release，开发者和 CI 可以直接走原生二进制或 `cargo install --path`。

## 13.3 发布文件

建议发布这些：

```text
awiki-cli_Darwin_x86_64.tar.gz
awiki-cli_Darwin_arm64.tar.gz
awiki-cli_Linux_x86_64.tar.gz
awiki-cli_Linux_arm64.tar.gz
awiki-cli_Windows_x86_64.zip
awiki-cli_Windows_arm64.zip
checksums.txt
npm/
awiki-cli.bash
awiki-cli.zsh
awiki-cli.fish
awiki-cli.ps1
manpages/
docs/cli/
```

---

## 14. 开发与测试计划

为了让这个方案真的能落地，我建议按 4 个阶段做。

## Phase 1：冻结契约

先只做：

* root command tree
* `status / docs / schema / doctor / id / msg / runtime`
* 全局 flags
* JSON envelope
* `--format`
* `--dry-run`
* `--jq`
* `schema`
* `completion`

这个阶段重点是**命令契约稳定**，不是功能全做完。

## Phase 2：把 core 跑通

实现：

* identity create/register/bind/resolve/recover/profile
* direct/group send/inbox/history
* secure send/init/repair/retry/drop
* runtime mode/listener/heartbeat
* SQLite store
* legacy import

## Phase 3：扩展命令

实现：

* people
* page
* discovery
* debug

## Phase 4：自动化与文档闭环

补齐：

* docs/cli 自动生成
* skill 引用校验
* shell completion
* man page
* release pipeline
* golden tests / integration tests / migration tests

---

## 15. 最终定版结论

我建议你直接按下面 10 条拍板：

1. **命令名与主二进制名统一定为 `awiki-cli`。**
2. **canonical CLI 定为：**
   `status / docs / schema / doctor / version / completion / id / msg / runtime`
3. **消息发送统一入口：**
   `awiki-cli msg send`
4. **transport 只属于 runtime，msg 层不感知 HTTP/WSS。**
5. **shortcut 要加，但只做 alias，不做 `+` 第二语法。**
6. **标准返回是 JSON；pretty/table/ndjson 都是视图。**
7. **全局格式参数统一为 `--format`；`--output` 仅做兼容别名。**
8. **`schema`、`--dry-run`、`--jq` 必须是第一天就有的一等能力。**
9. **Rust 技术栈固定为：cmdmeta + serde_json + Cargo release scripts + runtime/service modules + rusqlite bundled SQLite。**
10. **skill 体系采用 `single entry + lazy-loaded references`，同时分发支持 GitHub Releases + npm wrapper。**

如果你要，我下一步可以直接继续给你两份可落地内容：
**1）`crates/awiki-cli` 的 Rust 项目目录初始化方案**，以及 **2）`CLI_REFERENCE.md` 的最终文档定稿**。
