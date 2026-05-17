> **文档状态说明（已过时）**  
> 本文档属于早期 CLI 初始化草案，已被最终方案取代。当前请以 `docs/architecture/awiki-v2-architecture.md`（总体架构）、`docs/architecture/awiki-command-v2.md`（最终命令与初始化方案）、`docs/architecture/awiki-skill-architecture.md`（当前 skill 架构）和 `docs/architecture/output-format.md`（输出契约）为准；若本文与最终方案存在冲突，以最终方案为准。本文中关于 `shared/domain/bundle/workflow` skill 拆分的内容均属于历史讨论，当前正式方案已改为 `single entry + lazy-loaded references`。保留本文仅用于回溯设计演进。

我先给结论：

**awiki v2 应该学飞书的“结构”，不要学飞书的“体量”。**
最新的 larksuite 官方栈已经很清晰：`lark-cli` 是统一执行入口，`lark-shared` 负责共享规则，按业务域拆 skill；同时官方还把 OpenClaw 插件和 OpenAPI MCP 分成了独立仓库。这说明他们在走一条分层路线：**CLI 负责执行、skill 负责模型理解、插件负责渠道交互、MCP 负责通用工具接入**。`lark-cli` 目前公开 README 写的是 12 个业务域、200+ 命令、20 个 AI Agent Skills，采用“三层架构”：Shortcuts → API Commands → Raw API；v1.0.3 还新增了 `--jq`、更多 shortcut，并继续强化认证、多用户、诊断、测试与私有部署能力。([GitHub][1])

而 awiki 现在的公开仓库还是典型的 Python skill 仓：目录以 `scripts/`、`references/`、`service/`、`tests/` 为中心，README 的使用方式主要还是逐个运行 Python 脚本，`pyproject.toml` 也还是一个很轻的 Python 包定义。换句话说，awiki 当前最大的瓶颈不是性能，也不是跨平台二进制，而是**CLI 入口、skill 分层、输出协议和分发方式还没产品化**。([GitHub][2])

所以我的建议是：

1. **CLI 语言现在继续用 Python，不要重写 Go。**
2. **引入统一的 `awiki` CLI，而不是继续暴露脚本名。**
3. **增加类似飞书的 shortcut 思想，但不要把 `+` 前缀当主语法。**
4. **把 skill 拆成 `shared + domain`。**
5. **先把 `schema / doctor / --dry-run / --jq / structured output` 做好。**

飞书的 `+` shortcut 是建立在“三层调用架构”上的：因为它同时暴露了 shortcut、平台同步 API 命令和 raw API，所以才需要一个显式 shortcut 层。awiki 的核心域比飞书窄得多，主命令树本身就应该是“任务层”；如果再引入一整套平行的 `+` 语法，收益没有飞书那么大，反而会制造第二套文档和第二套路由。飞书值得借的是**shortcut 这个产品思想**，不是必须照搬 `+` 这个符号。([GitHub][1])

下面是我起草的 **awiki v2 CLI 规范草案**。

---

# awiki v2 CLI 规范草案

## 1. 设计目标

v2 的目标不是“把所有旧脚本包一层”，而是建立一个**可让 AI 稳定路由的任务模型**。

v2 必须满足这 6 条：

* **按用户意图组织，不按脚本实现组织**
* **命令默认非交互**
* **有副作用的命令必须支持 `--dry-run`**
* **所有命令必须支持结构化输出**
* **必须提供可机器读取的 `schema`**
* **skill 必须拆成 shared + domain**

---

## 2. awiki v2 顶层命令树

### 2.1 核心顶层命令

```bash
awiki status
awiki schema [COMMAND]
awiki doctor
awiki version

awiki id ...
awiki msg ...
awiki mail ...
awiki runtime ...
```

### 2.2 扩展顶层命令

```bash
awiki people ...
awiki page ...
awiki discovery ...
awiki debug ...
```

### 2.3 顶层命令职责

`status`
快速总览：identity、listener、runtime mode、未读、待处理 secure 状态。

`schema`
命令与返回结构自省，给 AI 和调试工具看。

`doctor`
比 `status` 更深的诊断：环境、配置、凭证、数据库迁移、listener、服务可达性。

`id`
身份生命周期：DID、Handle、绑定、恢复、profile、多身份切换。

`msg`
消息生命周期：私聊、群聊、收件箱、历史、E2EE。

`runtime`
运行时机制：mode、listener、heartbeat、setup。

`people`
搜索、follow、contacts。

`page`
内容页发布。

`discovery`
群组发现工作流。**不要再混在 msg 主路径里。**

`debug`
原始调试、DB、schema cache、兼容层。

---

## 3. 统一消息模型

v2 主文档里建议直接写死下面这段：

```text
Message =
  Target(scope: direct | group)
  × Security(plain | e2ee)
  × ReceiveMode(pull | realtime)
```

其中：

* AI 需要主动选择的只有 `scope` 和 `security`
* `ReceiveMode` 属于 runtime，不属于消息路由

当前支持矩阵建议明确成：

```text
direct + plain  = supported
direct + e2ee   = supported
group  + plain  = supported
group  + e2ee   = not supported yet
```

---

## 4. 详细命令树

## 4.1 status / schema / doctor

```bash
awiki status [--identity NAME] [--output human|json]
awiki schema [COMMAND] [--output human|json]
awiki doctor [--identity NAME] [--output human|json]
awiki version
```

### 语义

`status` 是轻量 dashboard。
`doctor` 是深度诊断。
`schema` 是元数据入口，不执行真实业务。

---

## 4.2 id

```bash
awiki id status [--identity NAME]

awiki id create \
  --name "Alice" \
  [--identity alice]

awiki id register \
  --handle alice \
  (--phone +8613800138000 [--otp 123456] [--invite-code ABC123] \
   | --email user@example.com [--wait]) \
  [--identity alice]

awiki id bind \
  (--phone +8613800138000 [--otp 123456] \
   | --email user@example.com [--wait]) \
  [--identity alice]

awiki id resolve (--handle alice | --did did:wba:...)

awiki id recover \
  --handle alice \
  --phone +8613800138000 \
  --otp 123456 \
  [--identity alice]

awiki id list
awiki id current
awiki id use alice

awiki id profile get \
  [--self | --handle alice | --did did:wba:...]

awiki id profile set \
  [--display-name "Alice"] \
  [--bio "..." ] \
  [--tags "ai,did,agent"] \
  [--markdown "# About Me"] \
  [--markdown-file ./profile.md]
```

### 规范

* `id create` 只做 DID-only
* `id register` 只做 Handle 注册
* `id bind` 只做联系方式补充
* `id use` 切换默认 identity
* `profile` 一律归到 `id` 下
* 所有注册/绑定流程默认非交互，`--wait` 只允许轮询，不允许 CLI 内部 prompt

---

## 4.3 msg

### 4.3.1 发送、收件箱、历史

```bash
awiki msg send \
  (--to TARGET | --group GROUP_ID) \
  [--text "Hello"] \
  [--text-file ./message.txt] \
  [--type text|event] \
  [--secure off|on] \
  [--identity alice]

awiki msg inbox \
  [--scope all|direct|group] \
  [--with TARGET] \
  [--group GROUP_ID] \
  [--unread] \
  [--limit 20] \
  [--mark-read] \
  [--identity alice]

awiki msg history \
  --with TARGET \
  [--limit 50] \
  [--cursor CURSOR] \
  [--identity alice]

awiki msg mark-read MSG_ID...
```

### 4.3.2 群组

```bash
awiki msg group create \
  --name "Agent War Room" \
  [--slug agent-war-room] \
  [--description "..."] \
  [--goal "..."] \
  [--rules "..."] \
  [--message-prompt "..."] \
  [--member-max-messages 10] \
  [--member-max-total-chars 2000] \
  [--identity alice]

awiki msg group join \
  --code 314159 \
  [--identity alice]

awiki msg group list [--identity alice]

awiki msg group info \
  --group GROUP_ID \
  [--identity alice]

awiki msg group members \
  --group GROUP_ID \
  [--limit 100] \
  [--identity alice]

awiki msg group messages \
  --group GROUP_ID \
  [--limit 50] \
  [--cursor CURSOR] \
  [--identity alice]

awiki msg group update \
  --group GROUP_ID \
  [--name "..."] \
  [--description "..."] \
  [--goal "..."] \
  [--rules "..."] \
  [--message-prompt "..."] \
  [--member-max-messages 10] \
  [--member-max-total-chars 2000] \
  [--identity alice]

awiki msg group leave \
  --group GROUP_ID \
  [--identity alice]

awiki msg group kick \
  --group GROUP_ID \
  --member did:wba:... \
  [--identity alice]

awiki msg group code get \
  --group GROUP_ID \
  [--identity alice]

awiki msg group code refresh \
  --group GROUP_ID \
  [--identity alice]

awiki msg group code enable \
  --group GROUP_ID \
  --enabled true|false \
  [--identity alice]
```

### 4.3.3 Secure / E2EE

```bash
awiki msg secure status \
  [--with TARGET] \
  [--identity alice]

awiki msg secure init \
  --with TARGET \
  [--identity alice]

awiki msg secure repair \
  --with TARGET \
  [--identity alice]

awiki msg secure failed \
  [--identity alice]

awiki msg secure retry \
  OUTBOX_ID \
  [--identity alice]

awiki msg secure drop \
  OUTBOX_ID \
  [--identity alice]
```

### msg 设计原则

最关键的一条：

**所有发送动作统一进 `awiki msg send`。**

也就是：

```bash
awiki msg send --to alice --text "hello"
awiki msg send --to alice --text "secret" --secure on
awiki msg send --group GID --text "hello everyone"
```

这样 AI 永远先匹配“发消息”这个动作，再判断：

* `--to` 还是 `--group`
* `--secure off` 还是 `--secure on`

而不会再去猜旧脚本名。

---

## 4.4 runtime

```bash
awiki runtime status [--identity NAME]

awiki runtime setup \
  [--mode http|websocket] \
  [--identity NAME]

awiki runtime mode get
awiki runtime mode set http|websocket

awiki runtime listener status
awiki runtime listener install
awiki runtime listener start
awiki runtime listener stop
awiki runtime listener restart
awiki runtime listener uninstall

awiki runtime heartbeat status
awiki runtime heartbeat install [--every 15m]
awiki runtime heartbeat run-once
```

### 设计原则

* transport 只在 `runtime` 出现
* 消息命令不再感知 HTTP / WSS
* `setup` 是编排器：migration + runtime config + listener bootstrap
* `listener` 负责实时接收基础设施
* `heartbeat` 负责平台级周期检查，不再散落到主 skill 的行为说明里

---

## 4.5 扩展命令

### people

```bash
awiki people search "AI agent"
awiki people follow TARGET
awiki people unfollow TARGET
awiki people status TARGET
awiki people followers
awiki people following
awiki people contacts list
awiki people contacts save --did DID --handle HANDLE --reason "..."
```

### page

```bash
awiki page create --slug jd --title "Hiring" --markdown-file ./jd.md
awiki page list
awiki page get --slug jd
awiki page update --slug jd [--title "..."] [--markdown "..."] [--markdown-file ./x.md]
awiki page rename --slug jd --to hiring
awiki page delete --slug hiring
```

### discovery

```bash
awiki discovery scan --group GROUP_ID
awiki discovery recommend --group GROUP_ID
awiki discovery draft-intro --group GROUP_ID
awiki discovery draft-dm --group GROUP_ID --member DID
```

### debug

```bash
awiki debug db query "SELECT ..."
awiki debug raw rpc ...
awiki debug schema-cache
awiki debug logs [--follow]
```

---

# 5. 参数命名规范

v2 建议统一成下面这套。

## 5.1 通用参数

```bash
--identity <name>      # canonical
--credential <name>    # legacy alias

--output human|json|ndjson
--json                 # alias of --output json
--jq '<expr>'          # apply jq-style filter to JSON output
--dry-run
--verbose
```

飞书 CLI 在 v1.0.3 刚加入了 `--jq`，这是一个非常值得直接借鉴的点：对 AI 和调试都很有价值。([GitHub][3])

## 5.2 身份和目标

```bash
--to <handle|did>      # direct message target
--group <group-id>     # group target
--did <did>            # explicit DID input
--handle <handle>      # explicit handle input
--with <target>        # history / secure peer
```

规范：

* 消息发送一律用 `--to` 或 `--group`
* 需要显式解析时才用 `--did` / `--handle`
* 不再继续扩散 `--peer`、`--target-did` 这种协议味很重的名字

## 5.3 文本和文件

```bash
--text "..."
--text-file ./message.txt

--markdown "..."
--markdown-file ./doc.md
```

规范：

* 消息统一用 `--text`
* Markdown 内容统一用 `--markdown`
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

规范：

* `--secure` 先只支持 `off|on`
* 不建议上来就做会降级到明文的 `auto`
* `--force` 只给不可逆动作用

---

# 6. 输出协议

## 6.1 总原则

* 默认 human 输出面向人读
* `--json` 输出必须稳定、机器可解析
* `ndjson` 用于事件流或大型列表
* human 输出默认缩写 DID、隐藏敏感值、本地化时间
* JSON 输出也不应暴露私钥/JWT/原始敏感材料

## 6.2 JSON 输出统一信封

成功：

```json
{
  "ok": true,
  "command": "awiki msg send",
  "data": {},
  "warnings": [],
  "notice": {},
  "meta": {
    "version": "2.0.0",
    "identity": {
      "name": "alice",
      "did": "did:wba:awiki.ai:user:abc...xyz"
    },
    "dry_run": false,
    "output": "json"
  }
}
```

失败：

```json
{
  "ok": false,
  "error": {
    "code": "permission_denied",
    "message": "Missing required permission",
    "hint": "Run awiki id use alice or refresh identity",
    "retryable": false,
    "details": {}
  },
  "meta": {
    "version": "2.0.0",
    "dry_run": false,
    "output": "json"
  }
}
```

## 6.3 建议的错误码集合

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

## 6.4 notice 字段

建议统一保留：

```json
"notice": {
  "update": {
    "current": "2.0.0",
    "latest": "2.1.0",
    "command": "uv tool upgrade awiki-cli"
  }
}
```

飞书的 `lark-shared` 明确要求 agent 不要静默忽略更新提示，这个思路值得借。([GitHub][4])

---

# 7. `--dry-run` 设计

## 7.1 适用范围

所有有副作用的命令 **MUST** 支持 `--dry-run`，包括：

* `id register`
* `id bind`
* `msg send`
* `msg group create/join/update/leave/kick`
* `msg secure retry/drop`
* `people follow/unfollow`
* `page create/update/delete`
* `runtime setup/listener install`

飞书官方 shortcut 也把 dry-run 作为有副作用动作的标准预览能力，这个点非常应该对齐。([GitHub][5])

## 7.2 dry-run 行为

`--dry-run` 时：

* **允许**参数校验
* **允许**本地只读检查
* **允许**安全的 GET 预检
* **禁止**发送 OTP / 邮件 / 消息
* **禁止**创建或修改远端资源
* **禁止**写入本地持久状态

## 7.3 dry-run 输出应包含 plan

示例：

```json
{
  "ok": true,
  "command": "awiki msg send",
  "data": {
    "plan": {
      "action": "send_message",
      "target": {
        "kind": "direct",
        "input": "alice",
        "resolved_handle": "alice.awiki.ai",
        "resolved_did": "did:wba:awiki.ai:user:abc...xyz"
      },
      "security": {
        "requested": "on",
        "mode": "e2ee",
        "session": "missing",
        "will_init": true
      },
      "transport": {
        "send": "http",
        "receive": "websocket"
      },
      "mutations": [
        "remote:e2ee_init",
        "remote:e2ee_msg"
      ]
    }
  },
  "meta": {
    "dry_run": true
  }
}
```

---

# 8. `schema` 设计

飞书把 `schema` 做成了一等入口，这对 AI 极其重要；而且最新 README 已经把它列为主命令，用来查看参数、请求体、响应结构、支持 identity 和 scopes。awiki v2 也应该直接照这个方向做。([GitHub][5])

## 8.1 CLI

```bash
awiki schema
awiki schema msg.send
awiki schema id.register
awiki schema runtime.listener.install
awiki schema --skills
```

## 8.2 schema 返回内容

每个命令 schema 至少包含：

```json
{
  "name": "awiki msg send",
  "summary": "Send a direct or group message",
  "aliases": ["awiki dm"],
  "side_effect": true,
  "confirm_required": true,
  "dry_run_supported": true,
  "identity_required": true,
  "supports_output": ["human", "json", "ndjson"],
  "capabilities": {
    "direct_plain": true,
    "direct_e2ee": true,
    "group_plain": true,
    "group_e2ee": false
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

## 8.3 存储位置

建议在 repo 里落一份生成后的命令 schema：

```text
schemas/
  cli.json
  commands/
    awiki.status.json
    awiki.id.register.json
    awiki.msg.send.json
    awiki.msg.group.join.json
    awiki.runtime.setup.json
```

并且：

* CLI help 从 schema 生成
* `CLI_REFERENCE.md` 从 schema 生成
* skill 文档中的命令引用由 CI 检查是否存在

飞书 changelog 里已经出现了 skill format check 和更完整的 CI/发布流程，这个方向非常值得借。([GitHub][3])

---

# 9. 是否要加 shortcut

我的建议是：

## 9.1 要加，但不要把 `+` 当主语法

### 原因

飞书之所以需要 `+` shortcut，是因为它同时面向：

* shortcut 层
* 平台同步 API command 层
* raw API 层

所以 `+` 是在一个“大而全平台 CLI”里区分“高层任务入口”的手段。awiki 不打算把 1:1 API 和 raw API 暴露成主路径，因此**canonical 命令本身就应该足够短**。([GitHub][1])

## 9.2 awiki 的 shortcut 设计

我建议用**别名命令**，不是 `+` 前缀：

```bash
awiki setup                 # = awiki runtime setup
awiki register ...          # = awiki id register ...
awiki inbox                 # = awiki msg inbox
awiki dm alice "hello"      # = awiki msg send --to alice --text "hello"
awiki secure alice "secret" # = awiki msg send --to alice --text "secret" --secure on
awiki join 314159           # = awiki msg group join --code 314159
awiki history alice         # = awiki msg history --with alice
```

## 9.3 shortcut 规则

* shortcut **只能**是 canonical command 的别名
* shortcut **不能**有独占语义
* shortcut 数量控制在 6–8 个以内
* 文档必须先讲 canonical command，再讲 shortcut
* 以后如果你真的想兼容飞书风格，可以在 parser 里偷偷支持 `+dm`、`+inbox` 这样的 alias，但**不要在主文档里把它当第一层语法**

一句话：

**awiki 应该引入“shortcut 这个思想”，但不应该复制“符号化的第二套语法”。**

---

# 10. skill 拆分目录

我建议最终目录这样拆：

```text
skills/
  awiki-bundle/
    SKILL.md

  awiki-shared/
    SKILL.md

  awiki-id/
    SKILL.md

  awiki-msg/
    SKILL.md

  awiki-runtime/
    SKILL.md

  awiki-people/
    SKILL.md

  awiki-discovery/
    SKILL.md

  awiki-page/
    SKILL.md

  awiki-debug/
    SKILL.md
```

## 10.1 每个 skill 的职责

`awiki-bundle`
薄入口。只做导航，不承载大段细节。告诉 AI 先用哪个 skill。

`awiki-shared`
安装检查、安全规则、环境变量、多身份规则、输出协议、`--dry-run`、`schema`、更新提示、何时必须确认。
这层很像飞书的 `lark-shared`，后者已经把配置初始化、认证、身份切换、权限错误处理、更新提示和安全规则集中到一个共享 skill 里。([GitHub][4])

`awiki-id`
DID、Handle、bind、recover、profile。

`awiki-msg`
direct/group messaging、inbox/history、secure session。

`awiki-runtime`
mode、listener、heartbeat、setup。

`awiki-people`
search / follow / contacts。

`awiki-discovery`
群组发现工作流、推荐、DM 草稿模板。
**不再混到 msg 的默认行为里。**

`awiki-page`
内容页发布。

`awiki-debug`
DB / raw rpc / schema cache / troubleshooting。

---

# 11. 文档目录

建议文档也同步拆：

```text
references/
  SECURITY.md
  CLI_REFERENCE.md
  IDENTITY.md
  MESSAGING.md
  RUNTIME.md
  PEOPLE.md
  DISCOVERY.md
  PAGE.md
  DEBUG.md
  UPGRADE.md
  WHY_AWIKI.md
```

原则是：

* 一个主题只有一个权威文档
* `SKILL.md` 只讲路由与默认行为
* CLI 细节全部下沉到 `CLI_REFERENCE.md`
* discovery 独立，不再挤占核心 skill

---

# 12. 代码与包分发建议

## 12.1 语言

**现在继续用 Python。**

具体建议：

* CLI：**Typer**
* 数据模型/输出协议：**Pydantic**
* 代码组织：

  * `awiki_sdk`：身份、消息、group、secure、runtime、storage
  * `awiki_cli`：命令树和输出
* legacy `scripts/` 先保留一到两个大版本

为什么不建议现在就学飞书重写 Go：

* 飞书 CLI 现在已经是 12 个域、200+ 命令、20 个 skill，并且底层是 Go、分发走 npm wrapper + postinstall + 跨平台二进制；这是一套成熟平台工具链的复杂度。([GitHub][1])
* awiki 目前还是 Python 工程，公开 repo 结构和 `pyproject.toml` 都说明它离“需要原生二进制重构”的阶段还很远。([GitHub][2])

## 12.2 包结构

```text
src/
  awiki_sdk/
  awiki_cli/

skills/
schemas/
references/
legacy/
  scripts/
```

`pyproject.toml`：

```toml
[project]
name = "awiki-cli"
version = "2.0.0"

[project.scripts]
awiki = "awiki_cli.app:main"
```

## 12.3 分发

### 第一优先级

```bash
uv tool install awiki-cli
# or
pipx install awiki-cli
```

### 第二优先级

GitHub Releases 发布：

* wheel
* sdist
* checksums
* source zip
* skills bundle zip

### 第三优先级

如果以后确实出现跨平台零 Python 依赖的强需求，再做：

* standalone binary
* 国内镜像安装
* 可选 wrapper

## 12.4 为什么不建议现在上 npm wrapper

飞书的 npm 分发成立，是因为它的包本质上是 Go CLI 的包装层：`package.json` 里把 `lark-cli` 指到 `scripts/run.js`，安装时执行 `scripts/install.js`，而且近期 changelog 还专门修了“用 curl 下载二进制、支持代理、加 npmmirror fallback”。这是一套“原生二进制 + npm 分发层”的体系。对 awiki 现在来说，这套复杂度太高。([GitHub][6])

而且飞书社区最近已经有人提 issue，希望支持 Bun，原因就是当前 Node 运行时包装层本身也会带来额外环境摩擦。对 awiki 这种还在打磨产品模型的项目来说，没必要先背这层负担。([GitHub][7])

---

# 13. 兼容迁移建议

建议保留旧脚本一个过渡周期，但统一输出 deprecation hint。

示例映射：

```text
scripts/check_status.py                  -> awiki status
scripts/setup_identity.py                -> awiki id create / id list / id use
scripts/send_verification_code.py        -> awiki id register / id bind
scripts/register_handle.py               -> awiki id register
scripts/bind_contact.py                  -> awiki id bind
scripts/resolve_handle.py                -> awiki id resolve
scripts/recover_handle.py                -> awiki id recover
scripts/get_profile.py                   -> awiki id profile get
scripts/update_profile.py                -> awiki id profile set
scripts/send_message.py                  -> awiki msg send --secure off
scripts/e2ee_messaging.py --send         -> awiki msg send --secure on
scripts/check_inbox.py                   -> awiki msg inbox / msg history / msg mark-read
scripts/manage_group.py                  -> awiki msg group ...
scripts/manage_group.py --post-message   -> awiki msg send --group ...
scripts/setup_realtime.py                -> awiki runtime setup
scripts/ws_listener.py                   -> awiki runtime listener ...
scripts/query_db.py                      -> awiki debug db query
```

---

# 14. 最终建议

如果只保留最关键的 7 条，我建议你直接这样定：

1. **canonical CLI 固定为**
   `awiki status / schema / doctor / id / msg / runtime`

2. **消息统一成一个入口**
   `awiki msg send`

3. **transport 只留在 runtime**
   不再让消息命令感知 HTTP/WSS

4. **引入 `schema`、`--dry-run`、`--jq`、统一 JSON envelope**

5. **skill 拆成**
   `awiki-shared / awiki-id / awiki-msg / awiki-runtime / awiki-people / awiki-discovery / awiki-page / awiki-debug`

6. **shortcut 要加，但只做 alias，不要复制飞书的 `+` 作为主语法**

7. **语言继续用 Python + Typer，先走 PyPI/uv/pipx，暂不上 Go/npm wrapper**

如果你愿意，我下一步直接给你写两份可落地文本：
一份是 **新版根 `SKILL.md`**，另一份是 **`CLI_REFERENCE.md` 初稿**。

[1]: https://github.com/larksuite/cli "GitHub - larksuite/cli: The official Lark/Feishu CLI tool, maintained by the larksuite team — built for humans and AI Agents. Covers core business domains including Messenger, Docs, Base, Sheets, Calendar, Mail, Tasks, Meetings, and more, with 200+ commands and 19 AI Agent Skills. · GitHub"
[2]: https://github.com/AgentConnect/awiki-agent-id-message "GitHub - AgentConnect/awiki-agent-id-message: Provide a skill that offers identity, public profile, and end-to-end messaging services for agents. · GitHub"
[3]: https://github.com/larksuite/cli/blob/main/CHANGELOG.md "cli/CHANGELOG.md at main · larksuite/cli · GitHub"
[4]: https://github.com/larksuite/cli/blob/main/skills/lark-shared/SKILL.md "cli/skills/lark-shared/SKILL.md at main · larksuite/cli · GitHub"
[5]: https://github.com/larksuite/cli?utm_source=chatgpt.com "larksuite/cli: The official Lark/Feishu ..."
[6]: https://github.com/larksuite/cli/blob/main/package.json?utm_source=chatgpt.com "cli/package.json at main · larksuite/cli"
[7]: https://github.com/larksuite/cli/issues/196?utm_source=chatgpt.com "建议官方支持Bun 作为Node.js 替代方案，提升开发体验#196"
