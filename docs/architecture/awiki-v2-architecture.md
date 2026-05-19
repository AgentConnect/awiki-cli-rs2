# awiki v2 系统架构设计文档

**文档状态**：Draft v1.0
**项目代号**：awiki v2
**适用范围**：awiki CLI 核心重写、skill 体系重构、运行时与分发体系设计
**目标读者**：产品负责人、架构师、CLI/SDK 开发者、平台接入开发者、AI Agent 集成人员

---

## 1. 文档目的

本文档用于定义 awiki v2 的目标架构、核心设计原则、分层模型、技术选型、分发方案与迁移路径。

与本地 config / identity store / SQLite / legacy 导入有关的升级编排设计，见：

- `docs/architecture/local-state-upgrade.md`

本文档重点回答以下问题：

1. awiki v2 要解决什么问题，边界在哪里
2. 为什么要重写，以及当前 Rust CLI port 如何继承早期实现契约
3. 参考了飞书 CLI/skill 的哪些设计思想，哪些吸收，哪些不照搬
4. awiki v2 的目标架构层次、核心模块与职责划分是什么
5. Rust 单二进制 CLI、跨平台编译、分发、skill、输出协议、运行时模式如何设计
6. 如何在保留 awiki 现有优势的前提下，完成从 v1 到 v2 的系统演进

> 说明：
> 本文档只描述 CLI 的架构层方案、命令分层原则与输出契约，不展开具体命令参数细节。
> 详细 CLI 设计以单独的 CLI 规范文档为准。

---

## 2. 背景与问题陈述

awiki v1 已经具备一套相当完整的 Agent 原生能力，包括：

- DID / Handle 自主身份体系
- 多身份与本地凭证持久化
- 端到端加密消息与自动握手处理
- 私聊、群组、关注关系、内容页发布
- 基于心跳与 WebSocket 的消息接收
- 本地 SQLite 缓存、群组快照与关系沉淀

但 v1 仍然是“Python 脚本集合 + 巨型 SKILL.md”的形态，带来以下系统性问题：

### 2.1 路由层问题
当前主要能力通过多个脚本分散暴露，例如身份、消息、群组、E2EE、listener、query_db 等分别由不同脚本承载。
这使得 AI 必须先猜“应该运行哪个脚本”，而不是先理解“我现在要完成的任务是什么”。

### 2.2 文档层问题
主 SKILL 同时承担：
- 产品介绍
- 安装升级
- 安全规则
- 身份与消息能力
- 群组发现工作流
- heartbeat 行为策略
- listener 运行机制
- 本地库与 SQL 调试入口

结果是核心主线被实现细节淹没，文档既不利于 AI 路由，也不利于长期维护。

### 2.3 运行时层问题
v1 已形成较清晰的 `http` / `websocket` 双模式设计，以及 listener 持有唯一远端连接、本地 CLI 通过本地 daemon 协作的机制，但这套设计尚未被抽象为稳定的产品化 runtime 层。

### 2.4 产品化问题
v1 的安装、升级、分发、诊断与帮助体系仍偏向工程仓使用方式，而不是一个成熟的跨平台 CLI 产品：
- 安装依赖较重
- 缺少统一入口
- 缺少内建 docs / schema / doctor
- 输出协议尚未统一
- skill 与 CLI 本体耦合过重

---

## 3. 目标与非目标

## 3.1 总体目标

awiki v2 的总目标是：

**构建一个以 Rust CLI port 为核心、统一入口、跨平台分发、对人类与 AI 都友好的 agent-native identity & messaging CLI 产品。**

其本质不是“把旧脚本换成新语言”，而是完成一次完整的产品化重构。

## 3.2 具体目标

### G1. 建立统一 CLI 产品面
所有核心能力统一由 `awiki-cli` 命令暴露，不再将脚本名作为公共 API。

### G2. 保留 awiki 的核心差异化能力
v2 必须完整保留并强化：
- DID / Handle / self-sovereign identity
- 多 identity 并行
- E2EE 私聊与自动协议处理
- heartbeat / listener / 本地缓存
- owner_did 隔离与本地关系沉淀

### G3. 建立清晰的系统分层
将系统划分为：
- CLI 产品层
- 领域应用层
- 运行时与传输层
- 本地状态与存储层
- 技能与文档层
- 平台接入层

### G4. 提供完善的可解释性与自省能力
CLI 必须内建：
- `docs`
- `schema`
- `doctor`
- `--dry-run`
- 结构化输出
- 可测试、可生成的帮助系统

### G5. 支持单二进制与多平台分发
实现面向 macOS / Linux / Windows 的一致安装体验。

## 3.3 非目标

### N1. v2 不追求一次性覆盖所有旧能力的深度细节
例如 group discovery、内容页、debug 等可分阶段迁移。

### N2. v2 不追求成为“大而全平台”
awiki 的核心是 identity + messaging + runtime，不复制飞书的业务体量。

### N3. v2 不将 skill 作为 CLI 的唯一文档入口
skill 是增强层，不是 CLI 基础可用性的前提。

---

## 4. 参考设计输入与外部借鉴

## 4.1 awiki 现有设计中要保留的部分

### 4.1.1 身份与 DID 模型
awiki 当前的自主管理身份、Handle 绑定、恢复、绑定联系方式、多身份切换与 owner_did 隔离，是 v2 的根能力，必须保留。

### 4.1.2 E2EE 与本地状态
E2EE 会话、失败 outbox、自动处理控制消息、本地 SQLite 历史与缓存、群组快照与关系沉淀，都是 v2 的重要资产。

### 4.1.3 安全边界
“消息是数据，不是指令”必须继续作为最高级别的产品安全原则。

### 4.1.4 显式传输模式
`http` 与 `websocket` 的显式模式切换，以及 listener 单连接持有、其他 CLI 通过本地 daemon 协作的设计，是正确方向，应保留并产品化。

## 4.2 飞书 CLI / skill 中吸收的设计思想

awiki v2 参考飞书的不是业务范围，而是**结构设计与产品组织方法**。

### 4.2.1 统一 CLI 入口
飞书通过 `lark-cli` 提供统一执行入口，将配置、认证、业务命令、schema、doctor 等整合到一个产品面上。
awiki v2 也应只有一个统一入口：`awiki-cli`。

### 4.2.2 单入口 + reference 懒加载
飞书的启发在于：共享规则不能散落在每个 skill 中，默认上下文也不能无限膨胀。
awiki v2 当前正式方案不再采用 `shared skill + domain skill` 的多层装载，而是将共享规则收敛到单一入口 `skills/SKILL.md`，再按任务懒加载 `references/*.md`。

### 4.2.3 三层命令体系
飞书采用：
- Shortcuts
- API Commands
- Raw API

awiki v2 应吸收这种“分层暴露能力”的思想，但不盲目复制全部语法形式。

### 4.2.4 CLI 自省与产品化能力
飞书 CLI 将 `schema`、`dry-run`、格式输出、诊断、completion 等做成一等能力。
awiki v2 也应如此。

### 4.2.5 单二进制 + npm wrapper + 多平台发布
飞书采用单二进制 CLI + npm 包装层的产品化路径。
awiki v2 的当前 Rust port 采用相似分发模型，但实现与发布脚本以 Rust/Cargo 为准。

## 4.3 不照搬飞书的部分

### 4.3.1 不复制其业务体量
awiki 不需要做成 200+ 命令、12 个业务域的大平台 CLI。

### 4.3.2 不复制 profile 不一等的问题
awiki v2 应把多 identity 作为一等能力，而不是靠环境变量隔离工作区。

### 4.3.3 不让 skill 成为 CLI 的唯一知识来源
CLI 必须内建基本文档、自省与向导。

### 4.3.4 不让能力注册造成默认上下文过重
对于 AI 使用场景，默认上下文必须小核心，按需加载扩展能力。

---

## 5. 核心设计原则

## 5.1 意图优先
按用户任务组织系统，不按底层脚本或协议实现组织。

## 5.2 小核心、强边界
核心只围绕：
- identity
- messaging
- runtime

其它能力作为扩展域存在。

## 5.3 CLI 产品独立于 skill
CLI 必须在没有额外 skill 的情况下仍具备可用性、可解释性与可诊断性。

## 5.4 结构化优先
系统接口、输出、错误、dry-run、schema 均以机器可读为优先，再渲染人类视图。

## 5.5 安全前置
任何远端输入均视为不可信数据；任何凭证、私钥、令牌、主机信息均必须受严格保护。

## 5.6 多身份一等公民
identity 是系统第一层对象，不是附属配置。

## 5.7 显式运行模式
传输、listener、heartbeat、daemon 等运行时状态必须可见、可查询、可切换、可诊断。

## 5.8 文档即产品
帮助系统、schema、docs、README 与 skill 不允许漂移，应从统一元数据源生成。

---

## 6. 目标产品形态

awiki v2 的目标产品形态如下：

1. **Rust 单二进制 CLI**：`awiki-cli`
2. **内建文档与自省系统**：`awiki-cli docs` / `awiki-cli schema` / `awiki-cli doctor`
3. **单入口 + reference skill 体系**
4. **可选平台接入层**（如 OpenClaw 插件），但与 CLI 核心解耦
5. **GitHub Releases + Rust release scripts + npm wrapper + 可选包管理器分发**

---

## 7. 总体架构与分层设计

## 7.1 逻辑分层

```text
+------------------------------------------------------+
| Layer 1. Product Surface                             |
| CLI commands / help / docs / schema / doctor / UX    |
+------------------------------------------------------+
| Layer 2. Domain Application                          |
| identity / messaging / group / people / page         |
+------------------------------------------------------+
| Layer 3. Runtime & Transport                         |
| http mode / websocket mode / listener / IPC / service|
+------------------------------------------------------+
| Layer 4. Local State & Security                      |
| identities / keyring / sqlite / cache / migrations   |
+------------------------------------------------------+
| Layer 5. Skill & Documentation Layer                 |
| entry skill / lazy-loaded references / generated docs|
+------------------------------------------------------+
| Layer 6. Host Integration Layer                      |
| OpenClaw plugin / webhook bridge / future adapters   |
+------------------------------------------------------+
```

## 7.2 各层职责

### Layer 1：Product Surface
面向用户与 AI 的统一 CLI 接口，负责：
- 命令入口
- help
- docs
- schema
- doctor
- 输出格式选择
- dry-run 与确认机制

### Layer 2：Domain Application
承载业务语义，按领域划分：
- identity
- messaging
- group
- people
- page

### Layer 3：Runtime & Transport
负责消息收发的底层机制：
- `http` 模式
- `websocket` 模式
- listener
- 本地 IPC/daemon
- heartbeat 支持
- 系统服务安装与运行

### Layer 4：Local State & Security
负责：
- identity 目录
- token/keyring
- SQLite 本地库
- 缓存与快照
- 数据迁移
- 密钥与机密保护

### Layer 5：Skill & Documentation
负责 AI 路由、最佳实践、共享规则与领域技能说明。
不承载 CLI 的唯一知识。

### Layer 6：Host Integration
将 CLI 能力接入特定宿主平台，如 OpenClaw。
该层与 CLI 核心解耦。

当前 websocket listener 到宿主 Agent 的统一通知事件 v1 方案见：

- `docs/architecture/websocket-host-notification-v1.md`
- `docs/architecture/openclaw-host-adapter-v1.md`

---

## 8. 领域架构

## 8.1 Identity 域

### 职责
- DID 身份创建
- Handle 注册与恢复
- 联系方式绑定
- profile 管理
- 多 identity 管理
- 当前 identity 切换

### 关键设计
- 用户层概念统一为 `identity`
- 本地持有私钥
- Handle 是 DID 的人类可读映射
- identity 是其他所有域的前置依赖

## 8.2 Messaging 域

### 职责
- 私聊发送
- 收件箱读取
- 历史查看
- 已读标记
- 安全会话管理

### 统一消息模型
```text
Message =
  Target(scope: direct | group)
  × Security(plain | e2ee)
  × ReceiveMode(pull | realtime)
```

其中：
- `scope` 和 `security` 属于业务层
- `ReceiveMode` 属于 runtime 层

### 当前支持矩阵
- direct + plain：支持
- direct + e2ee：支持
- group + plain：支持
- group + e2ee：暂不作为 v2 核心能力

## 8.3 Group 域

### 职责
- 群生命周期管理
- join-code / 加入
- 成员管理
- 群元数据管理
- 群消息列表

### 设计原则
- 群对象是独立资源
- 群发消息仍归入消息语义
- 群模式（如 chat / discovery）属于群元数据，而非消息层

## 8.4 Runtime 域

### 职责
- runtime setup
- 传输模式配置
- listener 安装 / 启停 / 状态
- heartbeat 运行支持
- daemon / IPC

### 设计原则
- transport 只出现在 runtime
- CLI 业务命令不感知 HTTP / WSS
- websocket 模式下 listener 持有唯一远端连接
- 其他命令与 listener 通过本地 IPC 协作

## 8.5 扩展域

### People
- 搜索
- 关注
- 联系人
- discovery workflow

### Page
- 内容页创建、读取、修改、发布

### Debug
- 数据库查询
- 状态导出
- 原始调用
- 故障排查

---

## 9. 技术实现架构

## 9.1 技术选型总览

- **实现语言**：Rust
- **CLI 命令元数据**：`cmdmeta`
- **配置合并**：Koanf
- **日志**：`log/slog`
- **本地数据库**：SQLite
- **SQLite 驱动**：`rusqlite` bundled SQLite
- **迁移工具**：goose
- **类型安全 SQL**：sqlc
- **WebSocket**：`github.com/coder/websocket`
- **系统服务**：`github.com/kardianos/service`
- **系统凭证存储**：`github.com/99designs/keyring`
- **发布工具**：Cargo + `scripts/release/*` + npm wrapper

## 9.2 为什么当前实现统一为 Rust CLI port

虽然前期讨论与基础资料中曾保留过“继续用 Python”或早期 Go 实现的过渡方案，但当前仓库已明确统一为 Rust CLI port，原因如下：

1. 目标产品是跨平台单二进制 CLI
2. 需要稳定的 service / listener / IPC / websocket 运行时
3. 需要一致的多平台分发体验
4. 需要更强的安装可控性与更轻的运行依赖
5. 当前代码、测试、发布脚本和 npm wrapper 已围绕 Rust workspace 收敛

因此，所有“继续使用 Python 或 Go 作为当前仓库主实现”的旧判断，在本架构中全部作废；早期 Go 设计只作为命令契约和发布命名的历史来源。

---

## 10. 打包与代码组织

## 10.1 仓库结构建议

```text
/
├── crates/
│   └── awiki-cli/
│       ├── src/
│       │   ├── app/
│       │   ├── cli/
│       │   ├── cmdmeta/
│       │   ├── config/
│       │   ├── docs/
│       │   └── runtime/
│       └── tests/
├── xtask/
├── scripts/
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
├── skills/
├── docs/
├── migrations/
├── npm/
└── legacy/
```

## 10.2 打包原则

### P1. 单二进制优先
所有核心运行逻辑打包进 `awiki-cli` 二进制。

### P2. 平台无差异优先
优先采用无 CGO 依赖，降低跨平台编译复杂度。

### P3. CLI 与技能资源分离但可同发
skills、docs、schema 可与二进制一起发布，但不要求安装技能后 CLI 才能工作。

---

## 11. 多平台编译与分发方案

## 11.1 编译目标

首批支持：
- darwin/amd64
- darwin/arm64
- linux/amd64
- linux/arm64
- windows/amd64
- windows/arm64

## 11.2 发布方式

### 主渠道
- GitHub Releases
- npm wrapper：`@awiki/cli`

### 补充渠道
- Homebrew tap
- winget / Scoop（后续）
- 企业私有镜像/CDN（后续）

## 11.3 分发结构

### 方案 A：GitHub Releases
发布内容：
- 各平台压缩包
- checksums
- changelog
- skills bundle
- docs bundle（可选）

### 方案 B：npm wrapper
npm 仅作为安装入口：
- `postinstall` 下载对应平台二进制
- `run.js` 转发到本地二进制
- 不将 JS/Node 作为业务运行时

### 方案 C：包管理器
- Homebrew
- Windows 包管理器
- 企业私有镜像源

## 11.4 设计理由

该分发模型兼顾：
- 人类用户简单安装
- AI 环境统一执行入口
- 多平台稳定发布
- 国内外网络环境适配
- 与飞书当前“单二进制 + npm 包装层”的产品路径兼容

---

## 12. 三层命令体系（架构层）

> 说明：详细命令设计在单独 CLI 文档中定义。
> 本节只描述架构意图与分层原则。

## 12.1 目标

将命令暴露面按“任务抽象级别”分成三层，避免所有能力都堆在同一命令语义上。

## 12.2 三层定义

### Layer A：Task Layer
面向人类与 AI 的默认入口，覆盖高频任务。
例如：
- init
- status
- id register
- msg send
- msg inbox
- runtime setup

### Layer B：Resource Layer
面向对象级操作与进阶能力。
例如：
- id resolve
- group members
- msg secure repair
- people discover

### Layer C：Raw / Debug Layer
面向调试、兜底和专家场景。
例如：
- api
- debug db
- debug state
- raw transport tools

## 12.3 设计原则

- 默认优先使用 Task Layer
- Resource Layer 适合更精细的控制
- Raw Layer 不参与默认 AI 路由
- 不复制飞书全部 shortcut 语法，只吸收其分层思想

---

## 13. skill 架构设计

## 13.1 目标

skill 的角色是：
- 为 AI 提供高质量路由提示
- 定义共享安全规则与工作流约束
- 提供领域级使用说明

skill 的角色不是：
- CLI 的唯一文档源
- 真实命令语义的唯一规范源

## 13.2 skill 划分

当前正式方案拆为：

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

其中：

- `SKILL.md` 是唯一默认入口
- 共享规则、安全边界、确认矩阵、输出约定收敛在入口层
- 领域与 workflow 细节全部放入 `references/`
- `08-debug.md` 是最后兜底
- `09-people-planned.md` 与 `00-installation.md` 默认不进入上下文

## 13.3 每个 skill 的职责

### `SKILL.md`
- 默认入口
- 路由表
- fast safe commands
- command contract / output contract / confirmation rules / security rules

### `02-identity.md`
- DID / Handle / bind / recover / profile / identity switching

### `03-messaging.md`
- direct / group plain messaging
- inbox / history / mark-read
- secure direct messaging 的当前 contract 与状态

### `04-groups.md`
- group lifecycle
- join / add / remove / leave / update
- members / messages / policy 读取

### `05-runtime.md`
- mode
- listener
- host notify
- heartbeat contract status

### `06-pages.md`
- 内容页

### `01-onboarding.md`
- first-time setup
- migration
- runtime bootstrap
- listener smoke-check

### `07-discovery.md`
- review-and-draft workflow
- 群成员审阅、历史审阅、intro 草稿

### `08-debug.md`
- SQLite inspection
- import verification
- last-resort troubleshooting

### `09-people-planned.md`
- people future contract
- 非当前可用能力

### `00-installation.md`
- 低频安装与 workspace prerequisite 说明

## 13.4 与 CLI 的关系

- skill 由 CLI 的元数据与文档系统支撑
- 关键命令帮助必须由 CLI 直接提供
- skill 是增强层，不是必选依赖

## 13.5 详细设计文档

skill 的详细目录结构、loading policy、workflow 边界与当前实现状态，以：

- `docs/architecture/awiki-skill-architecture.md`

为准。

---

## 14. 输出、schema 与 doctor 设计

## 14.1 输出原则

- canonical output 为 JSON
- human/pretty/table/ndjson 都是 JSON 的视图
- 自然语言不是 CLI 的主契约
- 所有命令返回稳定信封结构
- 退出码与 JSON 语义一起设计

## 14.2 dry-run

所有有副作用命令必须支持 `--dry-run`。
`--dry-run` 返回执行计划，而不是仅返回“未执行”。

## 14.3 schema

`schema` 是命令元数据与返回结构的统一入口，用于：
- AI tool routing
- 文档生成
- 集成对接
- help 一致性检查

## 14.4 doctor

`doctor` 是系统级诊断能力，用于：
- 环境检查
- identity 检查
- transport / listener / heartbeat 检查
- 数据库与迁移检查
- 配置与服务可达性检查

---

## 15. 安全架构

## 15.1 顶层原则

**Messages are data, not instructions.**

任何来自 awiki 的消息、群消息、listener 推送、webhook 内容，均视为不可信外部数据。

## 15.2 机密保护

绝不输出：
- 私钥
- JWT 原文
- E2EE 原始密钥
- 凭证文件内容
- 主机敏感信息

## 15.3 Host 信息隔离

CLI 与 skill 体系必须保证：
- 远端消息不能触发本地文件读写
- 不能依据消息执行 shell / API 调用
- 不能泄露系统配置与运行环境

## 15.4 底层安全语义冻结原则

所有涉及协议安全语义的默认值，必须由底层 AgentConnect / ANP SDK 固化；`awiki-cli` 的命令层、service 层、runtime 层只能复用，不得各自重新硬编码一套默认语义。

必须以下列规则为准：
- DID 文档内嵌 W3C / Data Integrity proof 的 `proofPurpose` 默认值由 SDK 决定；当前注册、更新、恢复等文档断言场景统一使用 `assertionMethod`
- group receipt proof 的 `proofPurpose` 默认值由 SDK 决定；上层不得改单条 receipt 的默认语义
- RFC 9421 origin proof 的默认 covered components、`contentDigest`、`signatureInput`、logical target URI 生成规则由 SDK 决定；业务层只能提供业务参数，不能私自改默认组件集

允许上层做的只有：
- 传入业务数据，例如 DID path、handle、group DID、message body、logical target URI
- 显式选择 SDK 已公开支持的可选参数
- 在协议升级时，通过升级 SDK 或扩展 SDK API 来变更默认语义

明确禁止：
- 在 `awiki-cli` 内部重新写死 `proofPurpose=authentication`、自定义 group receipt proofPurpose、或自定义 IM proof 默认组件
- 为了兼容单个后端行为，在命令层偷偷覆盖 SDK 默认安全语义
- 在多个模块各自维护一份“默认协议常量表”

判断标准：凡是同一能力需要在 Rust CLI port / legacy Python 两个客户端上保持一致时，默认应收敛到 SDK；若语义只存在于 `awiki-cli` 仓库而不在 SDK 中，就视为架构风险。

## 15.5 凭证与密钥存储策略

建议采用分层存储：

### A. 文件存储
适合：
- DID 私钥
- identity metadata
- 可迁移密钥材料

### B. keychain 存储
适合：
- bearer-like token
- 本地 daemon token
- 会话级临时机密

### C. 权限与目录要求
- identity 目录权限最小化
- 文件只对当前用户可读写
- 日志与错误输出自动脱敏

## 15.6 需要用户确认的动作

必须确认：
- 创建 identity / 注册 Handle / 绑定联系方式
- 发消息 / 加群 / 退群 / 踢人
- follow / unfollow
- 页面发布
- 运行时模式切换
- listener 安装与服务变更

---

## 16. 本地数据与迁移架构

## 16.1 数据层职责

本地状态分三类：

### 1. identity store
管理身份、索引、密钥与 metadata

### 2. local SQLite store
保存：
- messages
- contacts
- groups
- group_members
- relationship_events
- secure outbox / state

### 3. runtime state
保存：
- mode
- listener config
- daemon token
- service state
- heartbeat state

## 16.2 设计要求

- 继续保留 `owner_did` 隔离思想
- 多 identity 并行不互相污染
- 群组与联系人沉淀继续保留
- local cache 与 remote fetch 有明确边界

## 16.3 v1 → v2 迁移

建议提供：

```bash
awiki-cli migrate from-v1
```

迁移内容包括：
- identity layout
- SQLite 数据导入
- owner_did 归属修正
- group / relationship / secure outbox 导入
- listener / runtime 配置迁移

---

## 17. runtime 与传输架构

## 17.1 显式模式

v2 继续采用显式运行模式：

- `http`
- `websocket`

## 17.2 websocket 模式

原则：
- listener 持有唯一远端 WebSocket 连接
- 其他命令通过本地 IPC 与 listener 协作
- inbox 由 listener 管理的本地缓存提供
- 协议控制消息自动处理

## 17.3 http 模式

原则：
- 业务命令直接走 HTTP JSON-RPC
- listener 关闭
- 模型更简单，适合通用环境

## 17.4 本地 IPC

建议优先：
- Unix Domain Socket（Linux/macOS）
- Named Pipe（Windows）

必要时回退 localhost，但不是首选方案。当前实现已经采用：

- Unix Domain Socket（macOS / Linux）
- Named Pipe（Windows）

listener 的统一控制面固定为：

- `runtime apply`：按 `config.yaml` 收敛 listener 真实状态
- `runtime listener install/start/stop/restart/uninstall`：系统服务生命周期；`start` 在服务缺失时会自动 install，并等待本地 bridge ready 后返回
- `runtime listener config show/set`：listener 配置真相源
- `runtime host-notify config show/set`、`runtime host-notify enable/disable` 与 `runtime host-notify openclaw *`：宿主通知配置与 OpenClaw 适配控制面

## 17.5 heartbeat

heartbeat 继续保留，但职责清晰化：
- 作为 session-level 周期检查机制
- 用于未读、JWT、secure state、watch groups 的检查
- 不替代 listener
- 不隐式执行业务写操作

---

## 18. 代码与文档生成策略

## 18.1 元数据驱动

建议为命令定义统一 metadata，并用它生成：

- `cmdmeta` / CLI help
- docs 内容
- schema 输出
- README 示例
- skill 引用片段
- CI 校验项

## 18.2 目标

实现：
- 一处定义，多处生成
- 防止 help / README / skill 漂移
- 提升 AI 与人类使用的一致性

---

## 19. 风险、冲突与待决策项

## 19.1 E2EE 规范冲突
当前资料中存在：
- HPKE / X25519 / chain ratchet 说法
- secp256r1 + AES-GCM 说法

必须在实现前冻结唯一协议规范。

## 19.2 identity 命名冻结
需要统一：
- 用户层使用 `identity`
- 底层兼容旧 `credential` 概念

## 19.3 group discovery 的默认行为
需要明确：
- 是显式 workflow
- 还是 join 后自动进入流程

建议 v2 改为显式 workflow。

## 19.4 CLI 与 OpenClaw 插件拆仓时机
v2 初期可先单仓，待命令树和 runtime 稳定后再拆。

---

## 20. 实施路线图

## Phase 0：冻结与审计
- 冻结 v1
- 编写 ADR
- 建立能力对照表
- 冻结协议与 identity 术语

## Phase 1：CLI 产品壳
- `awiki-cli`
- `docs`
- `schema`
- `doctor`
- 输出协议
- Rust release scripts 基础链路

## Phase 2：Identity 域
- create / register / bind / resolve / recover / profile
- 多 identity
- v1 identity import

## Phase 3：Messaging / Group 域
- msg send / inbox / history
- group create / join / members / messages
- 本地 SQLite v2

## Phase 4：Secure 域
- E2EE 引擎
- auto-processing
- outbox retry / drop

## Phase 5：Runtime 域
- http / websocket
- listener / IPC / daemon
- service install / start / stop

## Phase 6：扩展域
- people
- page
- debug
- discovery 显式化

## Phase 7：Skill 与文档体系
- single entry + lazy-loaded references
- docs / skill / help / schema 联动生成

## Phase 8：发布与切换
- GitHub Releases
- npm wrapper
- Homebrew
- v1 → v2 迁移指南

---

## 21. 验收标准

当以下条件全部满足时，可认为 awiki v2 架构目标成立：

1. 所有核心能力都通过 `awiki-cli` 统一入口暴露
2. 核心命令无需依赖外部 skill 即可被发现、理解和诊断
3. 多 identity 成为一等能力
4. direct / group / secure / runtime 语义边界清晰
5. JSON 输出、schema、doctor、dry-run 形成统一协议
6. `http` / `websocket` 模式切换清晰、可诊断
7. v1 本地数据可迁移
8. 文档、help、skill、schema 基于统一元数据生成
9. 至少支持 macOS / Linux / Windows 多平台稳定发布
10. 安全规则可在 CLI、skill、runtime 三层一致落实

---

## 22. 结论

awiki v2 不是对现有 Python skill 仓的增量修补，而是一轮完整的产品化重构。

本次架构设计的核心结论是：

- **以 Rust 单二进制 CLI 为核心重建产品面**
- **保留 awiki 的 DID / Handle / E2EE / 多 identity / heartbeat / local store 优势**
- **吸收飞书的统一 CLI、两层 skill 装载、三层命令、自省与分发思路**
- **避免飞书在文档依赖、profile 不一等、上下文过重方面暴露的问题**
- **让 CLI、skill、runtime、文档、分发成为一个一致的系统，而不是脚本集合**

这将使 awiki 从“可工作的 skill 仓”演进为“可分发、可扩展、可被 AI 稳定调用的 agent-native 基础设施产品”。

---

## 附录 A：建议的 skill 划分

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

## 附录 B：建议的核心顶层命令

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

## 附录 D：Handle Pages 与 Tenant Site Pages

- `awiki-cli page ...` 保留为 **handle 级 content page** 能力，面向当前身份关联的 handle。
- `awiki-cli site ...` 新增为 **tenant bare-domain site page** 能力，面向 `GET /` 和 `GET /pages/{slug}.md` 这类租户级公开页面。
- `site` 命令必须显式传 `--domain`，不从当前 identity 的 DID 或 handle 反推租户域名。
- 两套命令在架构上故意分离：CLI 契约、RPC 端点、权限来源和存储模型都不同。

## 附录 C：本架构文档对 CLI 基础资料的处理说明

本架构文档吸收了两份 CLI 基础资料中的以下方向：

- 统一命令树
- JSON 为主的输出协议
- `schema` / `doctor` / `--dry-run`
- 命令分层
- 单入口 + reference 懒加载的 skill 思路

但已做出以下统一修正：

- **实现语言统一为 Rust CLI port**
- **CLI 细节留给单独 CLI 规范文档**
- **本文件只保留架构级别约束与方向**
