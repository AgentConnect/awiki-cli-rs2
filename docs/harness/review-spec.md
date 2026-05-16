# awiki-cli Harness Review 规范

## 1. 文档定位

本文是 `awiki-cli` 的 **二级 review 规范文档**，用于给人类 reviewer、AI reviewer、自动化 harness 提供一份可快速执行的审查索引。

它的目标不是替代源文档，而是解决两个问题：

1. **先看什么**：当 PR 涉及命令面、配置、输出、identity、runtime、storage、API 对接时，应该先看哪些文档。
2. **先查什么**：每类改动的 review 关键项、不可违反的约束、以及进一步确认时要回读的源文档路径。

使用原则：

- 先按本文做 **一级筛查**。
- 某条规则不清晰时，再按本文给出的路径去读 **一级源文档**。
- 若本文与冻结文档冲突，以冻结文档为准。

---

## 2. Review 裁决优先级

实现与 review 的冲突裁决顺序，按 `docs/plan/phase-0/implementation-constraints.md` 冻结结果执行：

1. `docs/plan/phase-0/implementation-constraints.md`
2. `docs/plan/phase-0/audit-findings.md`
3. `docs/plan/awiki-v2-implementation-plan.md`
4. `docs/architecture/awiki-command-v2.md`
5. `docs/architecture/awiki-v2-architecture.md`
6. `docs/architecture/output-format.md`
7. `../user-service/docs/api/`
8. `../message-service/docs/api/`
9. `../awiki-agent-id-message/`
10. `../cli/`

补充说明：

- `docs/architecture/cli-init.md`
- `docs/architecture/overall-init.md`

这两份文档都已明确标注 **已过时**，只能用于回溯设计演进，**不能作为当前 review 的判定真相**。

---

## 3. 依赖与参考地图

本节按“冻结约束 / 架构设计 / API 契约 / 协议与 legacy 基线 / 外部产品参考 / 当前仓库实现锚点”分门别类整理。

### 3.1 冻结约束与架构真相

| 路径 | 类型 | review 关注点 | 核心约束摘要 |
|---|---|---|---|
| `docs/plan/phase-0/implementation-constraints.md` | 冻结约束 | **最高优先级** | canonical 顶级命令、`group` 顶级归属、全局 flags、输出 envelope、错误码/退出码、`AWIKI_CLI_WORKSPACE_HOME_DIR`、`config.yaml`、单根目录工作区路径、`owner_did`、Phase 边界全部以此为准。 |
| `docs/plan/phase-0/audit-findings.md` | 冻结裁决 | 冲突消歧 | 解决了 `group` vs `msg group`、配置入口收口、顶级 `api` 是否暴露、`e2ee_outbox` 是否必保留、工作区路径与 `.openclaw` 兼容、pure Go / no CGO 等冲突。 |
| `docs/plan/phase-0/adr-index.md` | ADR 索引 | 规则引用 | 用于快速确认哪类问题已经被冻结，不必每次重新讨论。 |
| `docs/plan/phase-0/capability-mapping.md` | 能力映射 | 改动溯源 | v2 命令与 v1 脚本、user-service / message-service API 的映射关系，适合 review“改动有没有脱离既定能力映射”。 |
| `docs/plan/awiki-v2-implementation-plan.md` | 实施基线 | 阶段与模块边界 | 规定 Phase 1~5/7 的实现次序、目录结构、参考基线、优先级与迁移目标。 |
| `docs/architecture/awiki-v2-architecture.md` | 总体架构 | 设计原则 | 意图优先、小核心强边界、CLI 独立于 skill、结构化优先、安全前置、多 identity 一等公民、显式 runtime、文档即产品。 |
| `docs/architecture/awiki-command-v2.md` | 命令设计 | 命令与参数层 | 统一命令树、消息三维模型、shortcut 原则、命令/参数命名、schema/self-describing CLI 设计。 |
| `docs/architecture/output-format.md` | 输出协议 | 输出与机器契约 | canonical return 是 JSON；`pretty/table/ndjson` 是渲染层；统一 `ok/data/error/_notice/meta`，并要求 `--jq`、`--dry-run`。 |

### 3.2 user-service API 依赖

| 路径 | 覆盖域 | review 关注点 | 核心约束摘要 |
|---|---|---|---|
| `../user-service/docs/api/README.md` | 总入口 | API 入口与路径 | 区分 `/user-service/...`、`/content/rpc`、`/group/rpc` 等顶层例外路径；明确 DID / JWT 两套认证面。 |
| `../user-service/docs/api/authentication.md` | auth / bind | 注册与绑定 | 手机号统一 E.164；短信/邮箱验证流分离；绑定接口要求 Bearer JWT；手机号冲突与限流是显式错误。 |
| `../user-service/docs/api/did-auth.md` | DID 注册 / 验证 | DID 认证流 | `register` 只用于首次注册；已有 DID 更新必须走 `update_document`；DID domain 必须匹配服务域；proof/challenge/timestamp 要校验；主公钥不可漂移。 |
| `../user-service/docs/api/handle.md` | handle | handle 生命周期 | `lookup` 支持 handle/did 二选一；`get_quota` 只能查当前 DID 已绑定手机号；默认每手机号 3 个 handle；邮箱注册受白名单限制。 |
| `../user-service/docs/api/did-profile.md` | DID Profile | DID 资料面 | DID JWT 认证下的 `get_me/update_me/get_public_profile/resolve`；`profile_url` 优先走 handle 子域；`profile_md` 读取时可能注入邀请码块。 |
| `../user-service/docs/api/profile.md` | User Profile | 传统资料面 | 字段限制：`tags <= 10`、`bio <= 200`、`profile_md <= 50000`；公开资料与私有字段边界必须清晰。 |
| `../user-service/docs/api/relationships.md` | relationships | 社交关系 | 不能 follow/block 自己；block 会打断双向关系；`get_followers/get_following` 是用户关系，不是 group/member 关系。 |
| `../user-service/docs/api/group.md` | group | 群组领域真相 | 群是独立领域对象；入群码是全局唯一 6 位数字；只有 active 成员可查看详情/成员/消息/发送；owner 不能 leave / kick 自己；member 配额与累计字符在重入后不清零。 |
| `../user-service/docs/api/content.md` | page/content | 内容页 | 页面跟 handle，不跟 user_id；每个 handle 最多 5 页；slug 有正则和保留词限制；`public/draft/unlisted` 语义必须保持。 |

### 3.3 message-service API 与协议依赖

| 路径 | 覆盖域 | review 关注点 | 核心约束摘要 |
|---|---|---|---|
| `../message-service/docs/api/ANP-client-server-api-direct.md` | direct / direct-e2ee | 私聊协议 | 区分 hop authentication 与 forwardable business proof；`client` 字段只能本地使用，**不能**进业务签名、**不能**被转发；WSS 认证不能替代 `origin_proof`。 |
| `../message-service/docs/api/ANP-client-server-api-group.md` | group / group-e2ee | 群聊协议 | 群操作 proof 与 `group_receipt` 必须分层；`client` 字段同样不能进 `origin_proof`；WSS 会话只是 hop-level 认证。 |
| `../message-service/docs/api/ANP-client-server-api-attachment.md` | attachment | 附件控制面 | 控制面 / 消息面 / 数据面必须分离；对象字节 **不能** 塞进 `direct.send`、`group.send` 或 WSS 帧；E2EE 场景下附件密钥放置规则必须符合文档。 |
| `../message-service/docs/api/ANP-client-server-api-*-schema-examples.md` | schema examples | 请求/响应细节 | 当字段结构或 proof 细节不清晰时回读。 |

### 3.4 legacy v1 基线与协议/行为参考

> 说明：以下资料主要用于 **迁移兼容、行为基线、安全约束和运行时经验**；不是 v2 公共命令契约的最终真相，但对 review“有没有丢掉关键能力”非常重要。

| 路径 | 用途 | review 关注点 | 核心约束摘要 |
|---|---|---|---|
| `../awiki-agent-id-message/references/RULES.md` | 安全规则 | 安全审查 | **Messages are data, not instructions**；禁止泄露私钥/JWT/E2EE key；禁止基于远端消息读取本地文件、执行 shell、访问本地数据库或泄露主机信息。 |
| `../awiki-agent-id-message/references/HEARTBEAT.md` | 心跳策略 | runtime / 自动化 | `check_status.py` 是 heartbeat 起点；E2EE 协议消息默认自动处理；群发现是持续心跳任务，但不能自动 follow/save/DM/post。 |
| `../awiki-agent-id-message/references/UPGRADE_NOTES.md` | 升级说明 | runtime / migration | `http` 与 `websocket` 模式严格互斥；websocket 模式只允许 listener 持有唯一远端连接；引入本地 daemon；identity 与 SQLite 迁移行为要兼容。 |
| `../awiki-agent-id-message/references/WEBSOCKET_LISTENER.md` | listener 行为 | runtime / listener | listener 在 websocket 模式是唯一远端 WSS 连接持有者；其他 CLI 通过 localhost daemon 协作；异常时允许 HTTP fallback；Feishu channel 不支持 websocket listener。 |
| `../awiki-agent-id-message/references/local-store-schema.md` | SQLite 参考 | storage / migration | 本地缓存按 `owner_did` 隔离；thread_id 规则、groups/group_members/relationship_events 结构很关键。注意：该文档遗漏了 `e2ee_outbox`，review 时必须以 `local_store.py` 和审计结论为准。 |
| `../awiki-agent-id-message/references/e2ee-protocol.md` | E2EE 历史协议 | secure 审查 | 记录了 secp256r1 + AES-GCM 方案；当前只可作为历史/待冻结协议参考，真正编码前仍需遵守 v2 的协议冻结决策。 |
| `../awiki-agent-id-message/scripts/credential_layout.py` | identity layout | 存储兼容 | indexed multi-credential layout 是 v2 identity store 的兼容基线。 |
| `../awiki-agent-id-message/scripts/credential_store.py` | identity store | 存储兼容 | 文件布局、字段桥接、导入兼容。 |
| `../awiki-agent-id-message/scripts/local_store.py` | SQLite source of truth | 数据基线 | 是本地 SQLite 的真正 source of truth；`e2ee_outbox`、owner_did 隔离、group cache、relationship_events 都要以它为准。 |
| `../awiki-agent-id-message/scripts/setup_realtime.py` | runtime setup | runtime | receive mode、listener/install、HTTP/WSS 模式切换的基线参考。 |
| `../awiki-agent-id-message/scripts/ws_listener.py` | listener | realtime | listener 生命周期、后台服务、webhook 路由与 fallback 行为的参考实现。 |
| `../awiki-agent-id-message/scripts/e2ee_messaging.py` | secure messaging | Phase 5 | direct E2EE 首发功能、outbox/retry/drop 的参考入口。 |
| `../awiki-agent-id-message/scripts/manage_group.py` | group | group 生命周期 | create/show/update/join/leave/kick/members/messages/code 的 v1 能力基线。 |

### 3.5 外部产品与工程参考（`../cli/`）

> 说明：这部分是 **产品化组织方式参考**，不是 awiki 的业务真相。

| 路径 | 用途 | review 关注点 | 核心借鉴点 |
|---|---|---|---|
| `../cli/cmd/root.go` | 统一 CLI 入口 | 产品面组织 | 统一入口、schema/doctor/completion、更新 notice 注入、shortcuts 与 service 命令注册方式。 |
| `../cli/cmd/schema/schema.go` | schema 设计 | 自省能力 | schema 是一等能力，帮助系统与命令元数据应结构化、可被机器读取。 |
| `../cli/cmd/doctor/doctor.go` | doctor 设计 | 诊断产品化 | doctor 不只是报错，而是有 check 列表、hint、offline 模式与最终 summary。 |
| `../cli/internal/output/envelope.go` | 输出 envelope | 输出统一 | `ok/data/error/_notice/meta` 风格的统一封装。 |
| `../cli/internal/output/format.go` | 多格式输出 | 输出渲染 | JSON 为协议层，table/csv/ndjson 是渲染层。 |
| `../cli/internal/output/jq.go` | jq 处理 | jq 契约 | `--jq` 只能作用于 JSON 输出，必须有明确校验与错误处理。 |
| `../cli/skills/lark-shared/SKILL.md` | shared skill 设计 | 文档分层 | 共享规则、权限、安全、配置初始化放在 shared 层。 |
| `../cli/skills/lark-im/SKILL.md` | domain skill 设计 | 技能拆分 | shared + domain 技能分层，而不是一个巨型技能文档。 |

### 3.6 当前仓库实现锚点（review 当前代码时必看）

| 路径 | 当前作用 | review 关注点 |
|---|---|---|
| `internal/cmdmeta/catalog.go` | 当前 Phase 1 静态命令元数据目录 | 是否与冻结命令面一致；是否错误地引入新公共命令或错误归属。 |
| `internal/cli/root.go` | Cobra 根命令与 handler 装配 | 全局 flags、help/handler 装配、当前 Phase 1 已实现能力是否符合 docs。 |
| `internal/output/output.go` | 统一 success/error envelope 与渲染 | `--format`、`--jq`、`_notice`、table/ndjson 行为是否保持统一。 |
| `internal/config/config.go` | workspace / env / config 解析 | 仅支持 `AWIKI_CLI_WORKSPACE_HOME_DIR`，统一读取 `config.yaml`，并在发现旧环境变量或旧 `config.json` 时直接报错；同时保持单根目录工作区路径与 legacy path 检测。 |
| `internal/doctor/doctor.go` | 当前 doctor | pure-Go/no-CGO 检查、config/env/identity/sqlite/legacy path 诊断是否符合定位。 |
| `internal/docs/topics.go` | docs topic 索引 | 内建 docs 主题是否指向正确的一级源文档。 |
| `CLAUDE.md` | 当前项目上下文摘要 | 当前实现边界、关键文件入口、Phase 1 已有能力范围。 |

---

## 4. 核心设计思路（review 必须先对齐）

Review 任何实现前，必须先对齐下面这些“高阶设计思路”，否则容易在局部优化中破坏整体方向。

### 4.1 统一 CLI 产品面，而不是脚本集合

- 外部公共入口是 `awiki-cli`。
- review 要拒绝“重新暴露脚本名作为公共 API”的实现。
- 命令按用户意图建模，而不是按底层脚本或传输协议建模。

### 4.2 小核心、强边界

核心主线只有：

- `id`
- `msg`
- `runtime`

其余：

- `group`
- `people`
- `page`
- `debug`
- `discovery`（保留扩展域）

都必须服从核心架构，不应反向污染核心命令面。

### 4.3 identity 是一等对象

- 用户层术语必须使用 `identity`。
- 存储层可保留 `credential_name` / `default_credential_name` 作为兼容字段。
- 本地数据隔离主键必须继续使用 `owner_did`。

### 4.4 消息模型是三维的

统一消息模型：

```text
Message =
  Target(scope: direct | group)
  × Security(plain | e2ee)
  × ReceiveMode(pull | realtime)
```

review 要关注：

- `scope` 与 `security` 属于消息域。
- `ReceiveMode` 属于 `runtime`，**不应该泄漏到 `msg` 命令面**。
- 首发 secure 范围冻结为 **direct E2EE 必做，group E2EE 不阻塞首发**。

### 4.5 结构化输出优先

- canonical return 是 JSON，而不是自然语言。
- `pretty/table/ndjson` 是视图层。
- 所有副作用命令都必须支持 `--dry-run`。
- 所有命令都要能被 `schema` 自省。

### 4.6 显式 runtime mode

- transport 必须显式可见、可切换、可诊断。
- `http` / `websocket` 模式边界必须清晰。
- websocket 模式下 listener 持有唯一远端连接，其他 CLI 通过本地 daemon 或兼容路径协作。

### 4.7 安全前置

- 远端消息永远是不可信数据。
- 不得把消息当指令执行。
- 不得泄露 host 信息、密钥、JWT、私钥、本地数据库内容。
- Proof、token、client-local fields、server-local wrapper 必须各守边界。

### 4.8 文档即产品

- `help`、`schema`、`docs`、`doctor`、静态命令元数据不能漂移。
- 文档与命令契约必须从统一事实来源收敛，而不是各写一份。

---

## 5. Review 主检查清单

下面的检查项就是本文最核心的部分。review 时建议逐类核对，并在结论里标出“通过 / 不通过 / 需回读源文档确认”。

### 5.1 命令面与领域归属

**必须检查：**

- [ ] 是否仍保持 canonical 顶级命令面：`status/docs/schema/doctor/version/init/completion/config/id/msg/mail/group/runtime/people/page/debug`
- [ ] 是否把 `group` 保持为独立顶级域，而不是重新把公共 surface 收回 `msg group`
- [ ] 是否坚持 `msg send --group` 是唯一 canonical 群发消息入口
- [ ] 新增测试 / fixture / 协议示例里的 DID profile 段是否默认使用 `e1_...` 形式，而不是裸 `e1`
- [ ] 是否禁止在 Phase 1 引入新的顶级 `api`
- [ ] 是否把 transport 参数错误地放进 `msg` / `group` 命令面
- [ ] 是否引入 shortcut 独占语义（不允许）

**不可违反的约束：**

- `group` 是独立领域对象，不只是消息目标。
- `msg group ...` 最多只能做兼容 alias，不能成为主 surface。
- shortcut 只能是 alias，不能形成第二套命令语义。
- 新增本地测试数据和 DID 示例默认使用 `e1_alice` / `e1_group` 这类 `e1_...` profile 后缀。

**回读路径：**

- `docs/plan/phase-0/implementation-constraints.md`
- `docs/plan/phase-0/audit-findings.md`
- `docs/architecture/awiki-command-v2.md`
- `internal/cmdmeta/catalog.go`

### 5.2 输出协议、全局 flags、schema / dry-run

**必须检查：**

- [ ] 全局格式参数是否仍然是 `--format`
- [ ] identity 选择参数是否仍然是 `--identity`
- [ ] 是否保留 `--jq` 与 `--dry-run`
- [ ] 输出 envelope 是否仍然使用 `ok`, `data` / `error`, `_notice`, `meta`
- [ ] 是否有命令绕过统一 envelope 直接输出自由文本
- [ ] `--jq` 是否只作用在 JSON 契约之上
- [ ] 表格、ndjson 是否只是 JSON 的渲染视图，而不是第二套语义

**不可违反的约束：**

- `notice` 不是 canonical 字段，必须使用 `_notice`
- 所有副作用命令必须支持 `--dry-run`
- schema 是一等能力，不是文档附件

**回读路径：**

- `docs/architecture/output-format.md`
- `docs/plan/phase-0/implementation-constraints.md`
- `internal/output/output.go`
- `../cli/internal/output/envelope.go`
- `../cli/internal/output/jq.go`

### 5.3 identity / config / env / path / migration

**必须检查：**

- [ ] 用户接口是否始终使用 `identity` 术语
- [ ] 存储兼容字段是否仍能桥接 v1 的 `credential_*`
- [ ] 单根目录工作区路径是否保持：
  - `~/.awiki-cli/`
  - `~/.awiki-cli/identities/`
  - `~/.awiki-cli/data/`
  - `~/.awiki-cli/cache/`
  - `~/.awiki-cli/logs/`
- [ ] runtime / listener 状态目录是否固定为：
  - `~/.awiki-cli/runtime/`
- [ ] workspace upgrade 目录是否固定为：
  - `~/.awiki-cli/upgrade/`
- [ ] 根目录入口是否仅保留：`AWIKI_CLI_WORKSPACE_HOME_DIR`
- [ ] 配置优先级是否仍遵循：`flag > config.yaml > default`
- [ ] 是否仍然只检测 legacy `.openclaw` 路径而非默认原地写回
- [ ] 正式迁移入口是否仍为 `awiki-cli migrate from-v1`

**不可违反的约束：**

- 唯一允许的工作区环境变量是 `AWIKI_CLI_WORKSPACE_HOME_DIR`
- 旧 `AWIKI_*` / `AVIKI_*` / `E2E_*` 必须直接报错
- `.openclaw` 旧路径只做检测和导入提示，不应被默认原地篡改

**回读路径：**

- `docs/plan/phase-0/implementation-constraints.md`
- `docs/plan/phase-0/audit-findings.md`
- `docs/plan/awiki-v2-implementation-plan.md`
- `internal/config/config.go`
- `../awiki-agent-id-message/scripts/credential_layout.py`

### 5.4 auth 与服务 API 对接

**必须检查：**

- [ ] DID 注册 / update 是否严格区分 `register` 与 `update_document`
- [ ] handle / profile / relationships / group / content 的 API 映射是否仍与 capability mapping 一致
- [ ] 是否混淆了 User JWT 与 DID JWT 的使用边界
- [ ] API 路径是否正确处理了 `/group/rpc` 与 `/content/rpc` 等顶层例外
- [ ] profile / content / group / relationships 字段约束是否与 API 文档保持一致

**不可违反的约束：**

- 不能凭空发明 API 字段或省略关键鉴权前置
- 不能把 user-service 的用户态接口误当 DID 认证接口，反之亦然
- 不能把群组、内容页的权属模型改写掉

**回读路径：**

- `docs/plan/phase-0/capability-mapping.md`
- `../user-service/docs/api/README.md`
- `../user-service/docs/api/authentication.md`
- `../user-service/docs/api/did-auth.md`
- `../user-service/docs/api/handle.md`
- `../user-service/docs/api/did-profile.md`
- `../user-service/docs/api/group.md`
- `../user-service/docs/api/content.md`

### 5.5 消息、群组、runtime 的领域边界

**必须检查：**

- [ ] direct / group / secure / runtime 三者边界是否清晰
- [ ] `ReceiveMode` 是否仍只属于 `runtime`
- [ ] websocket 模式下是否仍坚持 listener 唯一远端连接拥有者
- [ ] 是否错误把群成员配额、join code、owner 规则挪到 CLI 层自行改写
- [ ] direct E2EE 首发范围是否仍优先于 group E2EE

**不可违反的约束：**

- Group join 使用全局唯一 6 位数字码，不应要求额外 `group_id`
- owner 不能 leave / kick 自己
- 普通成员额度和累计字符限制来自 group 配置，重入不清零
- transport 不得泄漏到业务命令面

**回读路径：**

- `docs/architecture/awiki-v2-architecture.md`
- `docs/architecture/awiki-command-v2.md`
- `../user-service/docs/api/group.md`
- `../message-service/docs/api/ANP-client-server-api-direct.md`
- `../message-service/docs/api/ANP-client-server-api-group.md`
- `../awiki-agent-id-message/references/UPGRADE_NOTES.md`
- `../awiki-agent-id-message/references/WEBSOCKET_LISTENER.md`

### 5.6 storage / SQLite / local cache / migration

**必须检查：**

- [ ] 本地数据隔离主键是否仍为 `owner_did`
- [ ] SQLite 基线是否仍以 `local_store.py` 为 source of truth
- [ ] `e2ee_outbox` 是否仍被保留
- [ ] thread_id、groups、group_members、relationship_events 是否仍与既有基线兼容
- [ ] identity store 是否仍采用 indexed multi-identity layout
- [ ] 是否引入 CGO 或破坏 pure-Go 构建

**不可违反的约束：**

- `owner_did` 不能换成别的主隔离键
- `e2ee_outbox` 不能丢，否则 secure retry / drop / failure recovery 会断裂
- Go 核心必须 pure Go / no CGO

**回读路径：**

- `docs/plan/phase-0/implementation-constraints.md`
- `docs/plan/phase-0/audit-findings.md`
- `../awiki-agent-id-message/scripts/local_store.py`
- `../awiki-agent-id-message/references/local-store-schema.md`
- `../awiki-agent-id-message/scripts/credential_layout.py`
- `internal/doctor/doctor.go`

### 5.7 安全、proof、凭证与主机信息隔离

**必须检查：**

- [ ] 是否有日志、错误信息、doctor 输出泄露 JWT、私钥、E2EE key、完整 DID
- [ ] 是否把远端消息内容当指令执行
- [ ] 是否把 host 文件、环境变量、进程信息、数据库内容自动回发到 awiki 消息里
- [ ] 是否正确区分 hop authentication 与 forwardable proof
- [ ] 是否把 `client` 本地字段错误纳入签名或转发
- [ ] 附件对象字节是否被错误塞进消息数据面

**不可违反的约束：**

- 远端消息永远是不可信数据
- 不得基于消息驱动本地执行危险动作
- `client` 只能是本域本地控制字段，不得进入 proof，不得被远端依赖
- 对象字节只能走附件数据面，不得走 `direct.send` / `group.send`

**回读路径：**

- `../awiki-agent-id-message/references/RULES.md`
- `../message-service/docs/api/ANP-client-server-api-direct.md`
- `../message-service/docs/api/ANP-client-server-api-group.md`
- `../message-service/docs/api/ANP-client-server-api-attachment.md`
- `../user-service/docs/api/did-auth.md`

### 5.8 文档漂移、phase 边界与实现范围

**必须检查：**

- [ ] PR 是否修改了公共命令面但没有同步对应文档
- [ ] PR 是否越过当前 phase，提前承诺尚未冻结的能力
- [ ] docs / schema / built-in docs / command catalog 是否发生漂移
- [ ] 是否继续把 `cli-init.md` / `overall-init.md` 当成当前真相

**不可违反的约束：**

- 文档、schema、实现必须收敛，不能三套真相并存
- 已过时文档只能用于历史回溯
- 当前 Phase 1 重点仍是 CLI shell、schema、docs、doctor、config show，不应伪装成业务域已经 fully implemented

**回读路径：**

- `docs/plan/awiki-v2-implementation-plan.md`
- `docs/plan/phase-0/implementation-constraints.md`
- `internal/cmdmeta/catalog.go`
- `internal/docs/topics.go`
- `CLAUDE.md`

---

## 6. 常见 review 结论模板

建议在 review 结论里按下面格式输出，便于后续自动化收敛：

### 6.1 结论结构

```text
Review Summary
- Scope:
- Result: pass | changes-requested | needs-confirmation

Key Findings
1. [category] ...
2. [category] ...

Constraint Check
- Command surface:
- Output contract:
- Identity/config/env:
- Service API mapping:
- Runtime/storage:
- Security:
- Docs drift:

Primary References
- ...
- ...
```

### 6.2 建议的分类标签

- `command-surface`
- `output-contract`
- `identity-config`
- `service-api`
- `runtime`
- `storage-migration`
- `security`
- `docs-drift`
- `phase-scope`

---

## 7. 一句话总纲

如果只记一条 review 原则，请记住这一句：

> **awiki-cli 的 review 目标，不是检查代码“能不能跑”，而是检查它是否继续忠实于冻结命令面、结构化输出、identity-first、显式 runtime、owner_did 隔离、安全前置，以及对 v1 基线和服务契约的兼容演进。**
