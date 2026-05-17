> **文档状态说明（已过时）**  
> 本文档属于早期初始化方案讨论稿，已被最终方案取代。当前请以 `docs/architecture/awiki-v2-architecture.md`（总体架构）、`docs/architecture/awiki-command-v2.md`（最终命令与初始化方案）和 `docs/architecture/awiki-skill-architecture.md`（当前 skill 架构）为准；若本文与最终方案存在冲突，以最终方案为准。本文中关于 `shared/domain` skill 体系的建议均属于历史方案，当前正式方案已改为 `single entry + lazy-loaded references`。保留本文仅用于回溯设计演进。

下面这份方案，是基于你已经**确定用 Go 重写**这个前提做的，不再纠结“要不要继续 Python”。我把最近几天飞书官方 CLI、飞书官方 OpenClaw 插件、官方 skills、以及社区 issue 里的真实反馈又重新看了一遍；同时把你们当前 awiki 的主 SKILL、Heartbeat、Listener、Schema、Security、E2EE 说明也重新对齐了一次。结论是：

**awiki v2 最好的方向，不是“做一个飞书的缩小版”，而是“用飞书的产品结构，重做一个更聚焦的 agent-native identity + messaging CLI”。**
也就是说：学飞书的统一 CLI、shared skill、三层命令、schema/doctor/dry-run、跨平台分发；保留 awiki 的 DID / Handle / E2EE / heartbeat / local store / 多身份这些真正有壁垒的设计；不要把飞书已经暴露出来的问题也复制过来。([GitHub][1])      

---

## 1. 这次重写的总目标

我建议你把这次重写定义成：

**awiki v2 = 一个独立、可分发、可自省、对 AI 友好的 Go CLI 产品；OpenClaw skill 只是它的一个消费层，不再是唯一形态。**

这和飞书现在的结构非常一致：`larksuite/cli` 是独立 CLI 产品，`larksuite/openclaw-lark` 是独立插件产品；前者提供统一命令、skills、schema、doctor、completion，后者负责宿主平台接入和交互体验。飞书官方 README 也明确把 CLI 定位为“built for humans and AI Agents”，并且把 command surface、skills、auth、security、advanced usage 都放进统一产品面。([GitHub][1])

而你们当前 awiki 仍然是“脚本集合 + 巨型 SKILL.md”的形态：安装靠 zip/git clone + `install_dependencies.py`，使用靠一组 `scripts/*.py`，主 SKILL 同时承担安装说明、身份、消息、E2EE、群组、群发现、heartbeat、listener、SQL 查询等职责。这个结构能工作，但不适合继续扩展，也不适合 AI 稳定路由。

---

## 2. 先给最终产品形态

我建议 awiki v2 的最终产品形态是：

1. 一个 **Go 单二进制 CLI**：`awiki`
2. 一个 **内建文档/自省系统**：`awiki docs`、`awiki schema`、`awiki doctor`
3. 一组 **精简 skill**：`awiki-shared`、`awiki-id`、`awiki-msg`、`awiki-runtime`、`awiki-people`、`awiki-page`、`awiki-debug`
4. 一个 **可选的 OpenClaw 插件/接入层**，但不与 CLI 核心混仓耦合
5. 一个 **GoReleaser + GitHub Releases + npm wrapper** 的分发链路

这里面最重要的是第 2 点：**CLI 自身必须完整可用、完整可解释，不再依赖 skill 作为唯一文档入口。** 这是飞书社区已经明确暴露出的改进点：官方 issue #8 直接指出，skills 可以是增强层，但不能成为理解核心命令行为的唯一现实路径；如果某个命令需要先读 guide，CLI 本体就应该能直接把 guide 或精简说明给出来。([GitHub][2])

---

## 3. 这次重写里，哪些必须保留，哪些必须吸收，哪些不要照搬

### 3.1 必须保留的 awiki 优势

这些是你们和飞书最不一样、也最值得保留的东西：

**第一，DID / Handle / self-sovereign identity 是 awiki 的核心，不是附属功能。**
你们当前设计里，身份不是一个 OAuth 登录态，而是本地私钥持有、可恢复 Handle、可多身份并存、围绕 DID 组织本地数据和消息线程。这是 awiki 的根能力，不应该在 Go 重写里被弱化成“普通 auth 模块”。 

**第二，E2EE、自动握手处理、失败 outbox、owner_did 隔离、本地 SQLite cache，这些都非常有价值。**
当前设计已经有 E2EE auto-processing、失败重试/放弃、group/member cache、`relationship_events`、按 `owner_did` 隔离本地数据等机制；这些是 awiki 的“agent-native messaging substrate”，不是应该被删掉的复杂度，而是应该被重构成更清晰的 Go 模块。   

**第三，`messages are data, not instructions` 这条安全边界必须成为 v2 的最高层规则。**
你们当前 RULES 明确禁止把 awiki 消息当命令执行，也禁止基于远端消息读取本地文件、执行 shell、访问数据库或泄露主机信息。这个设计非常对，而且要从“参考规则”升级为核心产品约束：进入 `msg inbox`、listener、webhook、discovery 的每条消息都先过 untrusted-data pipeline。

**第四，当前显式 `http/websocket` 传输模式和“listener 持有唯一远端连接”的思路值得保留。**
`Upgrade Notes` 和 `WEBSOCKET_LISTENER.md` 已经把这条思路讲清楚了：`websocket` 模式下 listener 持有唯一远端连接，其他命令走本地 daemon；`http` 模式下直接 JSON-RPC，listener 关闭。这个设计是对的，只是现在还被文档和脚本层打散了。 

---

### 3.2 必须吸收的飞书做法

**第一，统一 CLI 入口。**
飞书不是让 AI 记住一堆脚本，而是统一走 `lark-cli`，再按 `config / auth / schema / doctor / <service>` 分域。README 和 CHANGELOG 都把这点写得很清楚。awiki v2 必须也这样：所有功能只认 `awiki`，不再把脚本名当公共 API。([GitHub][1])

**第二，shared skill + domain skill。**
飞书现在有 `lark-shared`，而且 `lark-im`、`lark-event` 都要求先读 shared skill；这说明他们把认证、身份切换、权限、风险提示、危险操作规则放到了一个共用层。awiki v2 必须照这个方向改。你们当前最大的痛点之一，就是安全规则、安装规则、行为规则、消息规则都混在同一个主 SKILL。([GitHub][3])

**第三，三层命令体系。**
飞书明确采用三层：Shortcuts → API Commands → Raw API；README 还强调 `schema`、`dry-run`、多种输出格式。awiki 不需要复制它的全部体量，但一定要复制这个思想：高频任务入口、资源/对象级命令、raw/debug 层分开。([GitHub][1])

**第四，GoReleaser + GitHub Releases + npm wrapper。**
飞书的 npm 包 `@larksuite/cli` 并不是 JS CLI，本质是一个包装层；`package.json` 里 `bin` 指向 `scripts/run.js`，`postinstall` 运行 `install.js`，而 `install.js` 会按平台/架构去 GitHub Releases 或镜像地址下载对应的 Go 二进制。这个分发模型非常适合你现在已经决定采用 Go 的 awiki。([GitHub][4])

**第五，CLI 本体要有 `schema / doctor / completion / dry-run`。**
飞书官方已经把这些做成一级产品能力；而第三方指南和社区讨论之所以频繁把它拿出来说，本质上就是因为这些东西对 AI 和自动化特别关键。([GitHub][1])

---

### 3.3 不要照搬的飞书问题

**第一，不要复制“技能必须安装才能理解 CLI”的依赖关系。**
飞书官方 README 里把 skills 安装写成 AI agent 的必需步骤，但 issue #8 已经明确指出：这会让 CLI 文档 discoverability 变差。awiki v2 应该反过来：skill 是增强层，CLI 必须自带核心文档、自省和向导。([GitHub][5])

**第二，不要复制“单 active app / profile 不一等”的设计。**
飞书 issue #29 说明当前 `lark-cli` 仍存在多账号/多 profile 不原生的问题，用户要靠隔离 config dir 绕过去。awiki 恰恰已经在当前系统里有多 identity / 多 credential 的好设计，所以 v2 应该把这件事做成一等能力，而不是退化。([GitHub][6]) 

**第三，不要复制“工具/能力一次性全注册给模型”的 prompt-heavy 模式。**
`openclaw-lark` 的 issue #17 已经把这个问题说得很清楚：能力面太大、schema 太多，会抬高默认上下文成本、拉长启动上下文、增加 token 压力。awiki v2 必须默认小核心、按需加载，把 `people/page/debug` 从主 skill 和主上下文里拿掉。([GitHub][7])

**第四，不要允许 help/README/example 与真实 CLI surface 漂移。**
飞书 issue #48 说明这类问题哪怕是小问题，也会直接伤害用户复制命令的第一体验。awiki v2 应该把 README、`--help`、schema、skills、在线文档都建立在同一个 metadata source 上生成。([GitHub][8])

---

## 4. 重写前必须先做的“语义冻结”

在你开始写 Go 代码之前，我建议先做 4 个 ADR，不然你会在实现过程中一直被旧文档互相冲突的表述拖住。

**ADR-0001：E2EE 规范冻结。**
你们当前文档有明显冲突：主 SKILL 和 `WHY_AWIKI` 写的是 HPKE / X25519 / chain ratchet，而 `e2ee-protocol.md` 写的是 secp256r1 ECDHE + AES-GCM。Go 重写前必须只保留一份规范为准；如果协议现实是 secp256r1，就不要在 CLI/README/skill 里继续写 HPKE/X25519；如果未来要迁到 HPKE/X25519，就显式做成新版本协议和迁移计划。  

**ADR-0002：凭证/身份存储冻结。**
主 SKILL 说现在是 indexed multi-credential layout（`index.json + 每个 credential 一个目录`），但 RULES 还在写“一身份一个 JSON 文件”。这在 Go 重写前必须先统一。我的建议是：继续采用“索引 + 每身份目录”的结构，但把用户层术语从 `credential` 改成 `identity`。 

**ADR-0003：transport / listener / daemon 的责任边界冻结。**
`Upgrade Notes` 和 listener 文档已经把 `http` / `websocket` 模式边界讲清楚了，所以 v2 不要再把 transport 暴露到业务命令层；它应该只出现在 `runtime`。 

**ADR-0004：group discovery 从“隐式自动工作流”改为“显式 workflow”。**
当前 SKILL 把发现型群组的 post-join 行为设计得非常自动化，但 Heartbeat 文档又强调不能在没有用户确认的情况下自动 follow/save/DM。v2 最好把它显式化成 `people discover ...` 工作流，而不是“只要 join 就自动跑完整分析”。这保留了你们的关系发现设计，又降低了副作用。  

---

## 5. 我建议的 v2 仓库结构

如果你就用当前仓库重写，我建议直接把它变成一个 **Go monorepo**，但从目录层就把“核心 CLI / skills / docs / legacy import”切开：

```text
/
├── cmd/
│   └── awiki/
├── internal/
│   ├── app/
│   ├── cli/
│   ├── config/
│   ├── docs/
│   ├── schema/
│   ├── doctor/
│   ├── output/
│   ├── identity/
│   ├── messaging/
│   ├── group/
│   ├── secure/
│   ├── runtime/
│   ├── transport/
│   │   ├── http/
│   │   ├── websocket/
│   │   └── ipc/
│   ├── store/
│   ├── people/
│   ├── page/
│   └── migrate/
├── pkg/
│   └── awikiapi/        # 只有你确定要对外复用时才保留
├── skills/
│   ├── awiki-shared/
│   ├── awiki-id/
│   ├── awiki-msg/
│   ├── awiki-runtime/
│   ├── awiki-people/
│   ├── awiki-page/
│   └── awiki-debug/
├── docs/
├── npm/
│   ├── package.json
│   ├── scripts/install.js
│   └── scripts/run.js
├── migrations/
├── testdata/
└── legacy/
    └── python-v1-import/
```

这个结构一方面保留单仓开发和 release 简单度，另一方面已经为未来“单独 OpenClaw 插件仓库”预留了边界。它本质上是学习飞书的 `cli` 与 `openclaw-lark` 分离方式，但不要求你第一天就拆成两个 repo。([GitHub][9])

---

## 6. Go 技术栈建议

既然你已经确定 Go，我建议栈尽量“稳、少、可交叉编译”：

* **CLI 框架：Cobra**
  飞书官方现在就是 Cobra 风格的命令树；公开 pkg 文档里 `schema` 命令也是 `*cobra.Command`。这条线成熟、completion 现成、help 体系完整。([Go Packages][10])

* **配置层：Koanf**
  不建议上来就用 Viper 的全局魔法。Koanf 更适合做“flag > env > file > default”的显式优先级合并，也更方便测试。

* **日志：`log/slog`**
  标准库足够，结合 JSON log 即可。

* **本地存储：`database/sql` + `modernc.org/sqlite` + `goose` + `sqlc`**
  这样可以保持**无 CGO 跨平台编译**，同时继续享受你们当前 SQLite schema 的表达力。现有 schema 和 query 复杂度已经足够高，`sqlc` 比 ORM 更适合。你们当前 schema 里 `owner_did`、`groups`、`group_members`、`relationship_events` 这些设计都值得继续保留。

* **WebSocket：`github.com/coder/websocket`**
  比继续走 Python service/daemon 链路更干净，适合单二进制实现 long-lived listener。

* **系统服务：`kardianos/service`**
  方便做 macOS LaunchAgent / systemd / Windows Service。

* **表格输出：`go-pretty/table`**
  只在 `--format table` 时启用。

* **凭证/令牌：`99designs/keyring`**
  但我建议**只把 bearer-like token 和 daemon token 放 keychain**，不要把 DID 私钥完全塞进 OS keychain。

这里最关键的不是“Go 化”，而是**为单二进制跨平台发布让路**。飞书能用 GoReleaser 稳定覆盖 macOS / Linux / Windows，本质上也是因为底层尽量避免重依赖和平台特例。([GitHub][11])

---

## 7. 凭证与本地状态：保留 awiki 的好设计，但做得更像产品

我建议 v2 把秘密分成两类存储：

**A. 身份类私钥材料：文件存储，受权限保护，可导出备份。**
包括 DID 私钥、E2EE 私钥、identity metadata。因为 awiki 的 identity 是可迁移、可恢复、可跨环境导入的，完全锁进系统 keychain 反而会削弱它的可移植性。

**B. 会话类密钥/令牌：优先 keychain，失败时加密文件回退。**
包括 JWT/refresh token、local daemon token、webhook token 等。

目录可以这样设计：

```text
~/.awiki/
  config/config.yaml
  identities/index.json
  identities/alice/
    identity.json
    did.json
    keys/
    e2ee/
  data/alice/awiki.db
  state/
  logs/
```

如果检测到现有 `.openclaw` 目录，就支持兼容导入：

```bash
awiki migrate from-v1
```

这样既保留 awiki 的多身份与 owner_did 隔离，又吸收飞书的“OS-native keychain credential storage”优点。飞书官方 README 明确把 keychain 放进了其安全卖点之一；而你们当前 RULES 对私钥、JWT、E2EE key 的保密要求也已经非常清楚。([GitHub][1]) 

---

## 8. 命令体系：学飞书的三层，但不要照搬 `+` 语法

飞书的三层命令思想非常好，但 awiki 的业务域没那么多，所以不必机械复制 `+shortcut` 语法。我的建议是：

### 第一层：Task Layer（AI 默认层）

这是 AI 默认应使用的层：

```bash
awiki init
awiki status
awiki id register ...
awiki id bind ...
awiki msg send ...
awiki msg inbox
awiki msg history ...
awiki group join ...
awiki runtime setup ...
```

### 第二层：Resource Layer（对象级）

这是资源/对象级命令：

```bash
awiki id resolve ...
awiki id recover ...
awiki msg secure init ...
awiki msg secure repair ...
awiki group members ...
awiki group code refresh ...
awiki people discover start ...
```

### 第三层：Raw / Debug Layer

这是调试和兜底层：

```bash
awiki api ...
awiki debug db query ...
awiki debug state dump ...
```

这样你吸收了飞书的“三层架构”，但不引入对 awiki 来说没必要的额外语法负担。飞书的 `Shortcuts → API Commands → Raw API` 之所以成立，是因为它有 200+ 命令和 10+ 业务域；awiki 的核心域更窄，所以更适合直接用清晰的动词树来承载 task layer。([GitHub][1])

---

## 9. 我建议的 awiki v2 顶级命令树

这是我建议直接作为 v2 目标的命令树：

```bash
awiki init
awiki status
awiki config ...
awiki id ...
awiki msg ...
awiki mail ...
awiki group ...
awiki people ...
awiki page ...
awiki runtime ...
awiki docs ...
awiki schema ...
awiki doctor
awiki completion
awiki api ...
awiki debug ...
```

其中最核心的是这六个业务域：

```bash
awiki id
awiki msg
awiki group
awiki runtime
awiki people
awiki page
```

再补五个产品控制命令：

```bash
awiki init
awiki config
awiki docs
awiki schema
awiki doctor
```

这就是“参考飞书，但更克制”的版本。飞书的官方命令里 `auth / config / schema / doctor / completion` 是一级产品能力；awiki v2 也应该这么做，只是业务域更少。([GitHub][11])

---

## 10. 业务命令怎么设计

### 10.1 `awiki id`

```bash
awiki id status
awiki id create --name "Alice"
awiki id register --handle alice --phone +8613800138000 [--otp 123456] [--invite-code ABC123]
awiki id register --handle alice --email user@example.com [--wait]
awiki id bind --phone +8613800138000 [--otp 123456]
awiki id bind --email user@example.com [--wait]
awiki id resolve --handle alice
awiki id resolve --did did:wba:...
awiki id recover --handle alice --phone +8613800138000 --otp 123456
awiki id list
awiki id use alice
awiki id current
awiki id profile get [--self | --handle alice | --did did:wba:...]
awiki id profile set --name "Alice" --bio "..." --tags "did,e2ee"
```

这里有一个我建议你**明确不要学飞书**的点：
飞书社区对 `--profile` 的诉求，是因为它现在还没有把多账户做成一等能力；而 awiki 已经有“身份就是产品核心”的基础，所以不要把用户层概念叫 `profile`，否则会和 social profile 冲突。v2 应该统一叫 `identity`。([GitHub][6]) 

---

### 10.2 `awiki msg`

```bash
awiki msg send (--to TARGET | --group GID) --text "Hello" [--type text|event] [--secure off|on]
awiki msg inbox [--scope all|direct|group] [--group GID] [--mark-read]
awiki msg history --with TARGET
awiki msg mark-read MSG_ID...
awiki msg secure status [--with TARGET]
awiki msg secure init --with TARGET
awiki msg secure repair --with TARGET
awiki msg secure failed
awiki msg secure retry OUTBOX_ID
awiki msg secure drop OUTBOX_ID
```

这里坚持一条原则：

**所有“发送消息”都统一进 `awiki msg send`。**

也就是说：

```bash
awiki msg send --to alice --text "Hello"
awiki msg send --to alice --text "Secret" --secure on
awiki msg send --group grp_xxx --text "Hello everyone"
```

这样你把当前 `send_message.py`、`e2ee_messaging.py --send`、`manage_group.py --post-message` 三个入口，收敛成了一个统一意图入口。你保留了 awiki 的 direct/group + plain/e2ee 模型，但把它变成 AI 能稳定理解的命令语义。当前主 SKILL 正是这三件事分散在不同脚本和章节里。

---

### 10.3 `awiki group`

```bash
awiki group create ...
awiki group get --group GROUP_DID
awiki group join --group GROUP_DID
awiki group add --group GROUP_DID --member DID
awiki group remove --group GROUP_DID --member DID
awiki group members --group GROUP_DID
awiki group messages --group GROUP_DID [--cursor 120]
awiki group update --group GROUP_DID ...
awiki group leave --group GROUP_DID
```

这里我建议把 **group 生命周期** 做成 top-level `group`，但把 **群消息发送** 仍然统一在 `msg send --group`。
这样一方面继承了你“消息是核心”的设计，另一方面又让 group lifecycle 更可发现。当前 awiki 文档里 group 的概念已经明显超过“只是消息目标”，它有 members、doc_url、policy、profile 等完整对象语义。 

---

### 10.4 `awiki runtime`

```bash
awiki runtime status
awiki runtime setup [--mode http|websocket]
awiki runtime mode get
awiki runtime mode set http|websocket
awiki runtime listener status
awiki runtime listener install
awiki runtime listener uninstall
awiki runtime listener start
awiki runtime listener stop
awiki runtime daemon run
awiki runtime heartbeat status
```

这个命令组对应你当前的 `setup_realtime.py`、`ws_listener.py`、heartbeat 规则，但有两个重要改进：

1. **transport 只出现在 runtime 层**
2. **websocket 模式下本地 CLI 与 listener 的通信优先走 IPC，而不是 localhost TCP**

也就是说，v2 不再默认用 `127.0.0.1 + token` 作为唯一 daemon 通道，而是优先使用：

* Unix Domain Socket（macOS / Linux）
* Named Pipe（Windows）
* 仅在必要时 fallback 到 localhost

这条不是飞书直接给你的答案，而是你基于 awiki 当前 daemon/listener 设计向前迈一步。你当前 listener 文档和 upgrade notes 已经证明“单连接 + 本地代理”是对的，Go 版只需要把这层做得更安全。 

---

### 10.5 `awiki people`

```bash
awiki people search "alice"
awiki people follow --did ...
awiki people unfollow --did ...
awiki people following
awiki people followers
awiki people contacts list
awiki people contacts save ...
awiki people discover start --group GID
awiki people discover status --group GID
awiki people discover stop --group GID
awiki people discover run-once --group GID
```

这是我最建议你从主 skill 里“降级”的一块。
也就是：保留你们当前的群发现、推荐、DM 草稿、local relationship sedimentation 这些好设计，但把它从“join 后默认自动跑”的主路径，改成显式 workflow。这样既能保留 `GROUP_DISCOVERY_GUIDE` 和 `relationship_events` 的价值，又能降低副作用、上下文压力和错误自动化。  

---

## 11. docs / schema / doctor 是 v2 的产品核心，不是附加件

这是你重写里一定要抄飞书、而且要比它做得更彻底的地方。

### `awiki docs`

```bash
awiki docs onboarding
awiki docs identity
awiki docs secure-messaging
awiki docs transport-modes
awiki docs discovery-groups
```

### `awiki schema`

```bash
awiki schema
awiki schema msg.send
awiki schema id.register
awiki schema group.create
```

输出里建议包含：

* 参数定义
* 是否有副作用
* 是否必须用户确认
* 是否支持 `--dry-run`
* 支持的 target 类型
* transport 兼容性
* 需要的本地前置状态
* 返回 JSON 结构
* 常见错误码

### `awiki doctor`

```bash
awiki doctor
awiki doctor identity
awiki doctor transport
awiki doctor e2ee
awiki doctor store
```

飞书官方把 `schema / doctor / completion` 都列成一级能力；社区 issue #8 又明确抱怨“CLI 本体缺少 first-class in-product docs”；issue #48 又说明 help/example drift 是实打实的 UX 伤害。对 awiki 来说，这三件事应该一次性解决：
**CLI 自带 docs，自带 schema，自带 doctor；skills 只做 AI routing 和策略，而不是承载全部说明。** ([GitHub][11])

---

## 12. skill 的新结构

我建议 skill 最终拆成这 7 份：

```text
awiki-shared
awiki-id
awiki-msg
awiki-runtime
awiki-people
awiki-page
awiki-debug
```

### 设计原则

* `awiki-shared` 自动加载或由其他 skill 强依赖
* 每份 skill 控制在 50–120 行
* skill 只讲：何时用、先做什么、默认安全规则、常见工作流
* 复杂参考说明一律交给 `awiki docs` / `awiki schema`
* 所有 skill 都不要再直接写大量 bash 示例路径

这就是从飞书 `lark-shared + lark-im + lark-event` 学来的最有价值的部分。你甚至可以让 `awiki-msg` 在开头直接写：

> CRITICAL — 开始前先读取 `awiki-shared`

飞书就是这么做的。([GitHub][3])

---

## 13. 文档生成策略：一定要“一处定义，多处生成”

为了避免飞书 issue #48 这种 help / README / 实际命令漂移，你的 Go 重写最好从第一天就采用 metadata-driven 生成：

```go
type CommandMeta struct {
    Name                string
    Domain              string
    ReadOnly            bool
    RequiresConfirm     bool
    SupportsDryRun      bool
    SupportsFormats     []string
    Examples            []Example
    Schema              CommandSchema
    GuideTopics         []string
}
```

然后自动生成：

* Cobra `--help`
* `awiki schema`
* `docs/cli/*.md`
* `skills/*/SKILL.md` 中的引用片段
* README 命令示例
* Golden tests

也就是说，**不要再手写“主文档示例”和“命令帮助”两套源。**

---

## 14. 分发方案：我建议你几乎完整复用飞书这套

既然你已经选 Go，我建议分发就直接走飞书这条成熟路线。

### 推荐分发链路

**主渠道**

* GitHub Releases：发布原生二进制
* npm wrapper：`@awiki/cli`

**二级渠道**

* Homebrew tap（macOS / Linux）
* Scoop 或 winget（Windows）

**国内镜像**

* npm wrapper 的 `install.js` 增加 GitHub Releases 失败后的镜像回退
* 可用你自己的 CDN 或 npmmirror 风格镜像

飞书官方现在正是这个模型：Go 编译出的原生二进制通过 release 分发，npm 包只负责安装和转发。`package.json` 里 `postinstall` 触发安装脚本，安装脚本根据平台/架构计算 release asset 名称，然后从 GitHub Releases 或镜像拉取。([GitHub][4])

### 我建议你的包名

* 二进制：`awiki`
* npm：`@awiki/cli`
* GitHub Releases asset：

  * `awiki_2.0.0_darwin_amd64.tar.gz`
  * `awiki_2.0.0_darwin_arm64.tar.gz`
  * `awiki_2.0.0_linux_amd64.tar.gz`
  * `awiki_2.0.0_linux_arm64.tar.gz`
  * `awiki_2.0.0_windows_amd64.zip`
  * `awiki_2.0.0_windows_arm64.zip`

### 安装路径

我建议对外只保留两条主路径：

```bash
npm install -g @awiki/cli
# AI agent 如需 skills
npx skills add agentconnect/awiki-cli -y -g
```

和：

```bash
brew install agentconnect/tap/awiki
```

不要再把 zip + git clone + `install_dependencies.py` 当主安装路径。你们当前 SKILL 的安装方式在 Go 重写后应该彻底退出主路径。

---

## 15. 对 OpenClaw 的关系：参考飞书，但不要现在就拆仓

飞书现在已经把 CLI 和 OpenClaw 插件拆开了：`larksuite/cli` 做命令和 skills，`larksuite/openclaw-lark` 做宿主侧集成、卡片、交互、群策略等。这个方向是对的。([GitHub][9])

对你来说，最稳妥的做法不是马上拆成两个 repo，而是：

* v2 第一阶段先保持一个仓库
* 代码层把 `core CLI` 与 `host/plugin adapter` 解耦
* 等命令树、存储布局、技能层稳定之后，再单独抽 `openclaw-awiki`

这样你不会在“协议重写 + CLI 重写 + 插件拆分”三个方向同时冒风险。

---

## 16. 互联网和社区反馈里，最值得吸收到 awiki 的 4 个点

除了官方结构，最近外部评价里我觉得最值得你吸收的有四条：

**1. CLI 应该是“执行桥”，不是“编排器”。**
第三方指南对飞书 CLI 的一个很稳定的定位是：它的价值在于把 agent 输出和团队系统连接起来，但它本身不是 orchestration layer。这个观点我非常赞同，也很适合 awiki：heartbeat / discovery / listener / retry 这些可以是 runtime/workflow，但不要把整个 CLI 设计成“自动化总控平台”。([Verdent AI][12])

**2. `--dry-run` 是 AI 时代的安全网。**
不管是第三方教程还是社区讨论，大家反复强调的都是：AI 会自己做决策，所以 preview-before-write 非常重要。awiki v2 应该把 `--dry-run` 作为所有有副作用命令的统一能力。([GitHub][1])

**3. `schema` 是 AI 的“自描述接口”。**
外部讨论普遍认为，schema/introspection 解决的是“AI 看不到 GUI，只能靠文本理解工具”的根问题。awiki v2 也应该把 `schema` 当一等公民，而不是只给 README 示例。([GitHub][1])

**4. 默认上下文面要小。**
`openclaw-lark` 的上下文开销 issue 本质上说明：功能越多，越要模块化和按需暴露。awiki v2 的默认 skill 集不应包含 discovery/page/debug 这些非核心域。([GitHub][7])

---

## 17. 重写实施顺序

我建议你按下面顺序推进，而不是一次性全推倒。

### Phase 0：冻结旧版并做审计

* 打 `python-v1-final` tag
* 写 4 个 ADR：E2EE、storage、transport、discovery
* 列出现有功能对照表
* 选定 v2 CLI nouns

### Phase 1：先搭空壳

* `awiki`
* `config / docs / schema / doctor / completion`
* 输出协议、错误码、`--format`、`--dry-run`
* GoReleaser 初版

### Phase 2：身份模块

* `id create / register / bind / resolve / recover / profile`
* 多 identity 一等支持
* v1 数据导入器

### Phase 3：消息与群

* `msg send / inbox / history`
* `group create / join / members / messages`
* 统一 send 语义
* SQLite v2 schema & migrations

### Phase 4：E2EE

* secure session engine
* auto-process incoming control frames
* outbox failure / retry / drop
* 只保留一个协议规范源

### Phase 5：runtime

* `http` / `websocket` mode
* single remote connection
* daemon IPC
* service install/start/stop/status

### Phase 6：扩展域

* `people`
* `page`
* `debug`
* discovery workflow 显式化

### Phase 7：skills & docs 生成

* `awiki-shared` 等 modular skills
* `awiki docs` 与 skill 内容对齐
* 生成式 help/schema/readme

### Phase 8：发布切换

* GitHub Releases
* npm wrapper
* optional Homebrew
* skill install path
* migration guide

---

## 18. 我给你的最终定版建议

如果让我把整个方案压缩成一句话，那就是：

**awiki v2 应该是一个 Go 写的、独立分发的、带 `docs/schema/doctor/dry-run` 的统一 CLI；核心只围绕 identity / messaging / runtime 三大层重建；skills 采用 `shared + domain` 模式；保留 awiki 的 DID、E2EE、多身份、heartbeat、本地状态与安全边界；吸收飞书的统一入口、三层命令、跨平台发布与 agent-first 设计；但明确避免飞书已经暴露出的文档依赖、profile 不原生、schema 过重和 help 漂移问题。** ([GitHub][1])      

下一步最适合做的，不是继续讨论理念，而是我直接给你产出一份 **awiki v2 CLI 规范文档**：包含命令树、参数规范、输出协议、错误码、`schema` 结构、repo 目录、Go 包划分，以及首批 7 个 skill 的骨架。

[1]: https://github.com/larksuite/cli/blob/main/README.md "cli/README.md at main · larksuite/cli · GitHub"
[2]: https://github.com/larksuite/cli/issues/8 "CLI needs first-class in-product docs; skills should be optional enhancement, not required path · Issue #8 · larksuite/cli · GitHub"
[3]: https://github.com/larksuite/cli/blob/main/skills/lark-shared/SKILL.md "cli/skills/lark-shared/SKILL.md at main · larksuite/cli · GitHub"
[4]: https://github.com/larksuite/cli/blob/main/package.json "cli/package.json at main · larksuite/cli · GitHub"
[5]: https://github.com/larksuite/cli?utm_source=chatgpt.com "larksuite/cli: The official Lark/Feishu ..."
[6]: https://github.com/larksuite/cli/issues/29 "Feature Request: Multi-account / Profile support · Issue #29 · larksuite/cli · GitHub"
[7]: https://github.com/larksuite/openclaw-lark/issues/17 "Reduce context overhead by exposing Feishu capabilities via CLI/on-demand bridge instead of registering all as tools · Issue #17 · larksuite/openclaw-lark · GitHub"
[8]: https://github.com/larksuite/cli/issues/48 "Help and README examples drift from the current CLI surface · Issue #48 · larksuite/cli · GitHub"
[9]: https://github.com/larksuite/openclaw-lark "GitHub - larksuite/openclaw-lark: 飞书官方出品的 OpenClaw 飞书/Lark Channel 插件 · GitHub"
[10]: https://pkg.go.dev/github.com/larksuite/cli/cmd/schema?utm_source=chatgpt.com "schema package - github.com/larksuite/cli/cmd ..."
[11]: https://github.com/larksuite/cli/blob/main/CHANGELOG.md "cli/CHANGELOG.md at main · larksuite/cli · GitHub"
[12]: https://www.verdent.ai/guides/lark-cli-ai-coding-agent-workflow?utm_source=chatgpt.com "How Lark CLI Fits Into an AI Coding Agent Workflow"
