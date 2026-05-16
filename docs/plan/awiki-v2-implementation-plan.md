# awiki-cli v2 落地实施规划

**文档状态**：Draft v1.0  
**文档用途**：把架构设计转成可执行的工程实施计划，作为后续拆解里程碑、Issue、子任务和验收的基线。  
**适用范围**：`awiki-cli` Go 重写、命令面收敛、SQLite/凭证迁移、runtime/listener、skills/docs/schema、发布切换。  
**最后更新**：2026-04-07

> **Phase 0 冻结结果说明**：`docs/plan/phase-0/` 下的冻结文档是后续实现的直接约束。当本文与 Phase 0 冻结文档冲突时，以 Phase 0 冻结文档为准。

---

## 1. 目标与输入基线

本文档不再讨论“是否要重写”或“是否继续 Python”。这些决策已经在架构文档中冻结。本文档只回答一个问题：

**如何把 awiki v2 的目标架构按可交付、可验收、可迁移的方式实现出来。**

### 1.1 实施目标

awiki-cli v2 的实施目标是：

1. 把当前脚本集合收敛成统一的 `awiki-cli` 产品面。
2. 把 `id / msg / group / runtime` 做成首批可用的稳定域。
3. 保留 awiki 的差异化能力：DID、多身份、本地状态、E2EE、显式 runtime mode、group/relationship 沉淀。
4. 让 CLI、docs、schema、doctor、skills 不再分裂，转成单一元数据驱动的产品体系。
5. 提供从 `../awiki-agent-id-message/` 到 v2 的凭证、SQLite、本地 runtime 配置迁移路径。

### 1.2 本次规划的输入文档与代码基线

| 类别 | 路径 | 用途 |
|---|---|---|
| 总体架构 | `docs/architecture/awiki-v2-architecture.md` | v2 总体分层、域模型、runtime、安全、发布 |
| 本地状态升级 | `docs/architecture/local-state-upgrade.md` | workspace schema、meta/journal、backup、legacy 导入与统一升级入口 |
| 命令与执行方案 | `docs/architecture/awiki-command-v2.md` | 最终命令树、参数、输出、目录、阶段划分 |
| 输出契约 | `docs/architecture/output-format.md` | JSON envelope、dry-run、schema、exit code |
| 飞书 CLI 参考 | `../cli/` | Cobra 命令组织、schema/doctor/completion、skills、shortcuts、发布 |
| awiki v1 Python CLI 参考 | `../awiki-agent-id-message/` | 能力映射、listener/runtime、本地 SQLite、凭证布局、迁移逻辑 |
| 用户服务 API | `../user-service/docs/api/` | 身份、handle、profile、relationships、group |
| 消息服务 API | `../message-service/docs/api/` | direct/group/attachment、local view、WS 通知 |

### 1.3 发生冲突时的优先级

实现时按照下面的优先级裁决：

1. `docs/architecture/awiki-command-v2.md`
2. `docs/architecture/awiki-v2-architecture.md`
3. `docs/architecture/output-format.md`
4. `../user-service/docs/api/` 与 `../message-service/docs/api/`
5. `../awiki-agent-id-message/`
6. `../cli/`

> 说明：`../awiki-agent-id-message/` 是实现参考与迁移基线，不是 v2 命令契约的最终真相。

---

## 2. 规划前先冻结的决定

本节是实施前必须明确、不允许实现阶段再反复摇摆的决定。

### 2.1 产品与命令冻结

- 主二进制名固定为：`awiki-cli`
- canonical 顶级命令固定为：
  - `status`
  - `docs`
  - `schema`
  - `doctor`
  - `version`
  - `completion`
  - `config`
  - `id`
  - `msg`
  - `group`
  - `runtime`
  - `people`
  - `page`
  - `debug`
- 发消息唯一 canonical 入口固定为：`awiki-cli msg send`
- transport 只允许出现在 `runtime` 域，不允许泄漏到 `msg` / `group` 命令参数面。

### 2.2 术语冻结

- 用户层主术语使用 **identity**。
- `credential` 仅作为兼容 v1 的内部实现术语或 alias，不再作为 v2 主文案。
- 本地数据隔离主键继续使用 `owner_did`。

### 2.3 User 生命周期冻结

- **handle 是对外用户主流程的必填项。**
- **v2 首版不支持 pure DID 作为对外用户完成态。**
- **对外公共身份标识只使用 handle；`did` 仅在协议级定位需要时出现；`user_id` 为内部字段，不对 CLI / docs / schema / 输出透出。**
- `id create` 只保留为本地 bootstrap / 迁移辅助能力，不作为消息、runtime、群组主链路的前置完成态；默认从公开 help 中隐藏。
- 用户完成态固定定义为：**本地 DID 材料已生成 + 远端 user 已创建 + handle 已创建或恢复 + 本地凭证已记录**。
- `id register` / `id recover` 是进入可用用户态的 canonical 入口；`msg` 与 `runtime listener` 默认要求当前 identity 已完成该用户态。

### 2.4 输出与全局参数冻结

- 全局格式参数统一为：`--format`
- 结构化输出以 JSON envelope 为准。
- 更新提示字段统一为：`_notice`
- 所有有副作用的命令必须支持：`--dry-run`
- 支持：`--jq`
- exit code 与错误码统一收敛到 v2 新协议。

### 2.5 配置入口冻结

存在一个已发现冲突：

- 历史文档里同时存在多套环境变量前缀
- 工作区路径与业务配置都曾允许环境变量注入
- 主配置文件历史上使用 `config.yaml`

实现规划采用以下冻结规则：

- **仅保留 `AWIKI_CLI_WORKSPACE_HOME_DIR` 作为工作区环境变量入口**
- **所有业务配置统一收口到 `config.yaml`**
- **旧变量（`AWIKI_*` / `AVIKI_*` / `E2E_*`）全部停止兼容读取**
- **检测到旧变量或旧 `config.json` 时，CLI 直接报错并要求迁移**
- doctor 需要显式提示当前工作区来源与主配置文件路径

### 2.6 参考基线冻结

- **SQLite 表设计以 `../awiki-agent-id-message/scripts/local_store.py` 与 `../awiki-agent-id-message/references/local-store-schema.md` 为基线参考。**
- **凭证文件设计以 `../awiki-agent-id-message/scripts/credential_layout.py` 与 `../awiki-agent-id-message/scripts/credential_store.py` 为基线参考。**
- 首版实现优先保证“稳定迁移”和“兼容导入”，不主动重构这些数据模型。

### 2.7 已知审计项

这些问题不阻塞规划，但必须在 Phase 0 记录为审计任务：

1. `local-store-schema.md` 当前未列出 `e2ee_outbox`，但 `local_store.py` 中该表是权威存在的。
2. 历史环境变量入口已废弃，后续实现只允许 `AWIKI_CLI_WORKSPACE_HOME_DIR` + `config.yaml`。
3. E2EE 协议文档存在历史冲突，v2 必须先冻结具体协议再编码实现。

---

## 3. 目标工程结构

建议把本仓库直接落成单仓 Go CLI，目录结构如下：

```text
.
├── cmd/
│   └── awiki-cli/
├── internal/
│   ├── app/
│   ├── cli/
│   ├── cmdmeta/
│   ├── config/
│   ├── output/
│   ├── schema/
│   ├── doctor/
│   ├── identity/
│   ├── messaging/
│   ├── group/
│   ├── people/
│   ├── page/
│   ├── runtime/
│   ├── transport/
│   │   ├── http/
│   │   ├── websocket/
│   │   └── ipc/
│   ├── secure/
│   ├── store/
│   ├── serviceapi/
│   ├── migrate/
│   └── buildinfo/
├── schemas/
├── skills/
│   ├── awiki-shared/
│   ├── awiki-id/
│   ├── awiki-msg/
│   ├── awiki-runtime/
│   ├── awiki-people/
│   ├── awiki-page/
│   └── awiki-debug/
├── docs/
│   ├── architecture/
│   └── plan/
├── testdata/
│   ├── credentials/
│   ├── sqlite/
│   ├── rpc/
│   └── golden/
└── .goreleaser.yaml
```

### 3.1 模块职责

| 模块 | 职责 |
|---|---|
| `internal/cli` | Cobra 命令树、flag 绑定、命令执行入口 |
| `internal/cmdmeta` | 命令元数据、schema/help/docs 生成的单一事实来源 |
| `internal/output` | JSON envelope、pretty/table/ndjson、错误输出、_notice |
| `internal/config` | 单根目录工作区路径、env 兼容、配置加载、默认 identity 选择 |
| `internal/identity` | DID、注册、绑定、恢复、profile、多 identity 管理 |
| `internal/messaging` | direct/group 消息收发、history、mark-read |
| `internal/group` | group 生命周期与本地快照管理 |
| `internal/secure` | E2EE session、secure send、outbox、repair/retry/drop |
| `internal/runtime` | mode、listener、heartbeat、service lifecycle |
| `internal/transport/*` | HTTP、WSS、IPC 抽象与实现 |
| `internal/store` | SQLite DAO、migrations、identity store、runtime state |
| `internal/serviceapi` | user-service / message-service 客户端与请求映射 |
| `internal/migrate` | 从 v1 导入 credentials / SQLite / runtime state |

---

## 4. 参考基线：SQLite 与凭证文件

这是本规划里必须明确记录的项目约束。

### 4.1 凭证文件设计基线

v2 的 identity 存储设计，参考以下实现：

- `../awiki-agent-id-message/scripts/credential_layout.py`
- `../awiki-agent-id-message/scripts/credential_store.py`
- `../awiki-agent-id-message/scripts/credential_migration.py`

#### 4.1.1 目录布局基线

v2 采用单根目录工作区模型，identity 内部文件布局继续参考 v1 的 indexed multi-credential layout：

```text
~/.awiki-cli/config.yaml
~/.awiki-cli/identities/index.json
~/.awiki-cli/identities/<identity-dir>/identity.json
~/.awiki-cli/identities/<identity-dir>/auth.json
~/.awiki-cli/identities/<identity-dir>/did_document.json
~/.awiki-cli/identities/<identity-dir>/key-1-private.pem
~/.awiki-cli/identities/<identity-dir>/key-1-public.pem
~/.awiki-cli/identities/<identity-dir>/e2ee-signing-private.pem
~/.awiki-cli/identities/<identity-dir>/e2ee-agreement-private.pem
~/.awiki-cli/identities/<identity-dir>/e2ee-state.json
~/.awiki-cli/data/awiki-cli.db
~/.awiki-cli/runtime/
~/.awiki-cli/cache/
~/.awiki-cli/upgrade/
```

#### 4.1.2 index.json 基线

首版实现遵循以下约束：

- `schema_version` 与 v1 索引版本兼容读取
- `default_identity_name` 语义等价于 v1 的 `default_credential_name`
- index 中保存 identity 元信息，不保存私钥原文
- `default` 作为 alias 时优先解析显式 `default`，再 fallback 到当前默认 identity

#### 4.1.3 文件权限基线

- identity 目录：`0700`
- 私钥 / token / json 凭证：`0600`
- 文档与日志中不得打印私钥、JWT、E2EE 私钥

#### 4.1.4 迁移基线

需要兼容识别 v1 flat legacy layout：

- `<credential>.json`
- `e2ee_<credential>.json`
- `<credential>_did_document.json`
- `<credential>_private_key.pem`

并提供统一入口：

```bash
awiki-cli migrate from-v1
```

### 4.2 SQLite 表设计基线

v2 本地 SQLite 设计参考以下来源：

- `../awiki-agent-id-message/scripts/local_store.py`
- `../awiki-agent-id-message/references/local-store-schema.md`
- `../awiki-agent-id-message/scripts/database_migration.py`
- `../awiki-agent-id-message/scripts/e2ee_session_store.py`
- `../awiki-agent-id-message/scripts/e2ee_outbox.py`

#### 4.2.1 首版必须保留的表

| 表名 | 用途 | 基线来源 |
|---|---|---|
| `contacts` | 联系人、沉淀关系、follow-up 信息 | `local_store.py` / `local-store-schema.md` |
| `messages` | direct/group 收发消息本地缓存 | `local_store.py` / `local-store-schema.md` |
| `e2ee_outbox` | secure 失败重试 / drop / resend | `local_store.py` / `e2ee_outbox.py` |
| `groups` | group 快照、本地 membership 状态 | `local_store.py` / `local-store-schema.md` |
| `group_members` | group 成员快照 | `local_store.py` / `local-store-schema.md` |
| `relationship_events` | 关系沉淀事件流 | `local_store.py` / `local-store-schema.md` |
| `e2ee_sessions` | 私聊 E2EE session 持久化 | `local_store.py` / `e2ee_session_store.py` |

#### 4.2.2 首版必须保留的视图

| 视图 | 用途 |
|---|---|
| `threads` | 聚合线程摘要 |
| `inbox` | 所有 incoming message 视图 |
| `outbox` | 所有 outgoing message 视图 |

#### 4.2.3 首版必须保留的关键设计原则

- 本地快照隔离维度必须继续使用 `owner_did`
- `credential_name` / `identity_name` 字段继续保留，用于诊断、迁移与兼容
- thread id 规则保持对称：
  - 私聊：`dm:{min_did}:{max_did}`
  - 群聊：`group:{group_id}`
- schema version 继续使用 `PRAGMA user_version`
- 首版 migration 优先保证与 v1 数据可导入，不主动做大规模重构

#### 4.2.4 规划中的文档补齐任务

需要在后续实现中补齐以下差异：

- `local-store-schema.md` 需补充 `e2ee_outbox` 的正式表说明，避免文档与实现脱节

---

## 5. 外部接口与命令映射基线

### 5.1 user-service 相关命令映射

| CLI 命令 | 服务/API 文档 | 说明 |
|---|---|---|
| `id create` | `../user-service/docs/api/did-auth.md`（仅 bootstrap 背景） | 本地 DID 材料生成；不是对外用户完成态 |
| `id register` | `../user-service/docs/api/did-auth.md` + `authentication.md` + `handle.md` | 创建远端 user、注册 handle、写回凭证 |
| `id bind` | `../user-service/docs/api/authentication.md` | 手机/邮箱绑定 |
| `id resolve` | `../user-service/docs/api/did-profile.md` + `handle.md` + `users.md` | DID/Handle 解析与用户摘要查询 |
| `id recover` | `../user-service/docs/api/did-auth.md` + `handle.md` | 通过手机验证码恢复 handle DID 绑定 |
| `id profile get/set` | `../user-service/docs/api/profile.md` + `did-profile.md` + `users.md` | 自己/公开 profile 与当前用户查询 |
| `people follow/unfollow/status` | `../user-service/docs/api/relationships.md` | follow/unfollow/status |
| `group *` 生命周期 | `../user-service/docs/api/group.md` | create/get/update/join/leave/kick/list members 等 |

### 5.2 message-service 相关命令映射

| CLI 命令 | 服务/API 文档 | 说明 |
|---|---|---|
| `msg send --to` | `../message-service/docs/api/ANP-client-server-api-direct.md` | `direct.send` |
| `msg send --group` | `../message-service/docs/api/ANP-client-server-api-group.md` | `group.send` |
| `msg inbox` | `ANP-client-server-api-direct.md` | `inbox.get` local-view 方法 |
| `msg mark-read` | `ANP-client-server-api-direct.md` | `inbox.mark_read` |
| `msg history --with` | `ANP-client-server-api-direct.md` | `direct.get_history` |
| `group messages` | `ANP-client-server-api-group.md` | `group.list_messages` |
| `msg secure *` | `ANP-client-server-api-direct.md` | prekey/session/init/ack/e2ee_msg |
| 附件增强 | `ANP-client-server-api-attachment.md` | `attachment.create_slot` / `commit_object` / download ticket |

### 5.3 飞书 CLI 中需要借鉴的部分

`../cli/` 只作为产品结构和工程组织参考，不直接复制业务体量。需要借鉴的点包括：

1. `cmd/root.go` 的 Cobra 根命令组织方式
2. `schema` / `doctor` / `completion` 作为一级产品命令
3. `internal/output` 式的统一输出与错误 envelope
4. `shortcuts` 与 domain service commands 分离
5. `skills/lark-shared` + domain skill 的分层方式
6. update notice 的 `_notice` 注入策略
7. GoReleaser + GitHub Releases + npm wrapper 的发布链路

---

## 6. 分阶段实施计划

本节是本规划的核心。每个阶段都以“可交付物”和“验收标准”为中心，而不是只写方向。

### Phase 0：冻结与审计

**目标**：把会影响后续所有实现的契约问题全部冻结。

**主要任务**：

1. 把最终命令树、全局 flags、输出 envelope、错误码写成一份实现约束表。
2. 对照 `awiki-command-v2.md` 与 `awiki-v2-architecture.md`，标出所有实现需要遵守的冻结项。
3. 完成三类差异审计：
   - v2 文档内部冲突
   - v2 文档与 v1 Python 行为差异
   - v2 文档与 API 文档差异
4. 明确 E2EE 协议冻结结果。
5. 明确环境变量兼容顺序与单根目录工作区规则。

**交付物**：

- 一份实现约束表（可并入本规划附录）
- 一份 API/命令/脚本能力对照表
- 一份风险清单与 ADR 列表

**验收标准**：

- 后续阶段不再允许更改主命令树与主输出协议
- 后续阶段不再允许更改 identity/credential 命名策略
- 已列出所有阻塞实现的协议冲突项并给出裁决

### Phase 1：CLI 产品壳

**目标**：搭起可运行的 v2 CLI 外壳，先稳定产品面，再做业务实现。

**主要任务**：

1. 初始化 Go 模块与 `cobra` 根命令。
2. 建立顶级命令骨架：
   - `status`
   - `docs`
   - `schema`
   - `doctor`
   - `version`
   - `completion`
   - `config`
   - `id`
   - `msg`
   - `group`
   - `runtime`
   - `people`
   - `page`
   - `debug`
3. 建立全局 flags：
   - `--format`
   - `--jq`
   - `--dry-run`
   - `--identity`
   - `--verbose`
4. 实现统一 JSON success/error envelope。
5. 实现 update notice 注入 `_notice`。
6. 实现 `schema` 基础框架，哪怕最开始只输出静态元数据。
7. 实现 `doctor` 基础检查框架，先检查路径、配置、identity store、SQLite 可达性。

**交付物**：

- 可编译的 `awiki-cli`
- 完整 help 树
- 统一输出层
- schema/doctor 基础命令

**验收标准**：

- `awiki-cli --help` 展示最终顶级命令树
- `awiki-cli schema` 能输出结构化命令元数据
- `awiki-cli doctor` 能输出基础诊断结果
- 所有命令在失败时都遵循统一错误 envelope

### Phase 2：配置、Identity 与凭证存储

**目标**：先落地本地 identity 基础设施，为后续 User 完成态做准备。

**主要任务**：

1. 落地单根目录工作区解析：
   - workspace home
   - config
   - data
   - runtime
   - cache
2. 落地配置入口收口：
   - `AWIKI_CLI_WORKSPACE_HOME_DIR`
   - `config.yaml`
   - 旧环境变量检测与报错
3. 实现 identity index store。
4. 实现 identity create/list/use/current。
5. 明确 `id create` 只负责本地 DID / 密钥 / did_document 生成，不作为对外用户完成态。
6. 完成 v1 credential import。
7. 完成旧 flat legacy credential 扫描与导入提示。
8. 设计 token / daemon token 与 keychain 的接口层，但首版可先用受权限保护的文件存储。

**交付物**：

- identity store
- v2 index.json + per-identity dir
- 本地 bootstrap 命令
- migrate from-v1（credential 部分）

**验收标准**：

- `id create/list/use/current` 可用
- 旧 `.credentials` 能被识别、提示、导入
- 文件权限符合最小权限要求

### Phase 3：User、Handle 与 Credential 完整化

**目标**：跑通“可用用户态”主链路：创建远端 user、注册 handle、绑定联系方式、记录本地凭证。

> 说明：旧版本规划把这部分隐含在 identity 阶段里，导致“本地 DID bootstrap”和“远端 user 完成态”混在一起。本阶段将两者显式拆开，并冻结 handle-first 约束。

**主要任务**：

1. 明确主流程为 handle-first：
   - `id register` 是 canonical 用户创建入口
   - `handle` 为必填
   - 不支持 pure DID 作为对外用户完成态
2. 对接 user-service API：
   - `POST /did-auth/rpc` `register`
   - `POST /handle/rpc` `send_otp`
   - `POST /did-auth/rpc` `recover_handle`
   - `POST /auth/phone-bind-send`
   - `POST /auth/phone-bind-verify`
   - `POST /auth/email-send`
   - `GET /auth/email-status`
   - `POST /did/profile/rpc` `get_me` / `update_me` / `get_public_profile`
   - `POST /users/rpc` `get_me` / `get_by_did` / `get_by_handle`
3. 参考 Python 版本补齐非交互 CLI 流程：
   - `register_handle.py`
   - `bind_contact.py`
   - `recover_handle.py`
   - `get_profile.py`
   - `update_profile.py`
   - `credential_store.py`
4. 本地凭证落盘与索引补齐：
   - `identity.json` 内部记录 `did / user_id / handle / created_at`
   - `auth.json` 记录 token
   - `did_document.json` 记录当前 DID 文档
   - index 中内部同步 `user_id`、`handle` 与默认 identity 解析
   - `user_id` 只作为内部映射字段保存，不进入公共 CLI 输出
5. 建立当前 identity 的“用户完成态”判断：
   - local-only identity
   - registered user
   - partial user / incomplete user
6. 对 `doctor`、`msg`、`runtime listener` 增加 gating：
   - 未完成 handle 注册的 identity 不能进入消息和 realtime 主链路
   - CLI 需要明确提示先完成 `id register` 或 `id recover`

**交付物**：

- handle-backed user lifecycle 命令
- OTP / email verification / bind / recover 流程
- 完整 credential 持久化
- 用户完成态检查与 gating

**验收标准**：

- `id register` 能完成：
  - 创建远端 user
  - 创建或恢复 handle
  - 保存本地凭证
  - 形成可复用 identity
- `id bind`、`id recover`、`id profile get/set` 能与 user-service 跑通
- `id current` 与 `doctor` 能识别 local-only identity 与 registered user
- 未完成 handle 注册的 identity 不能直接执行 `msg *` 与 `runtime listener *`

### Phase 4：SQLite 本地状态与迁移

**目标**：把 v1 的本地状态模型迁到 v2，同时保留 owner_did 隔离和 v1 可导入能力。

**主要任务**：

1. 建立 SQLite 连接层、WAL 模式、schema version 管理。
2. 落地首版表：
   - `contacts`
   - `messages`
   - `e2ee_outbox`
   - `groups`
   - `group_members`
   - `relationship_events`
   - `e2ee_sessions`
3. 落地首版视图：
   - `threads`
   - `inbox`
   - `outbox`
4. 建立 DAO 层与查询接口。
5. 建立从 v1 SQLite 导入的 migration。
6. 支持 DID 恢复后的 owner rebind。
7. 建立 test fixtures：
   - v1 DB 样本
   - 多 identity 样本
   - 含 secure outbox / group / relationship 的样本

**交付物**：

- SQLite schema
- migration runner
- DAO 层
- v1 DB 导入能力

**验收标准**：

- 本地 SQLite 可初始化、升级、查询
- v1 DB 可导入到 v2 schema
- owner_did 隔离语义不丢失
- `threads/inbox/outbox` 查询结果符合预期

### Phase 5：Messaging 与 Group 基础域

**目标**：先跑通 plain direct / plain group 的主链路。

**主要任务**：

1. 实现 message-service 客户端：
   - `direct.send`
   - `inbox.get`
   - `inbox.mark_read`
   - `direct.get_history`
2. 实现 user-service group 客户端：
   - `create`
   - `get`
   - `update`
   - `refresh_join_code`
   - `get_join_code`
   - `set_join_enabled`
   - `join`
   - `leave`
   - `kick_member`
   - `list_members`
   - `post_message`
   - `list_messages`
3. 落地 CLI 命令：
   - `msg send`
   - `msg inbox`
   - `msg history`
   - `msg mark-read`
   - `group create/show/update/join/leave/kick/members/messages/code*`
4. 本地消息与群组快照持久化。
5. group 命令与消息命令的边界收敛：
   - 群生命周期在 `group`
   - 群发消息仍从 `msg send --group` 进入
6. 把 Phase 3 的 user gating 作为消息主链路前置条件，默认拒绝 local-only identity。

**交付物**：

- plain direct/group 全链路
- 本地缓存与历史
- group 本地快照

**验收标准**：

- 可以完成 direct 发消息、收件箱、历史、标记已读
- 可以完成 group 创建、入群、看成员、看消息、更新、离开、踢人
- 相关数据能稳定写入 SQLite

### Phase 6：Secure / E2EE 域

**目标**：补齐 awiki 的 secure messaging 差异化能力。

**主要任务**：

1. 根据冻结后的协议实现 secure session store。
2. 完成以下命令：
   - `msg secure status`
   - `msg secure init`
   - `msg secure repair`
   - `msg secure failed`
   - `msg secure retry`
   - `msg secure drop`
3. 保留 v1 的关键行为：
   - auto-init
   - inbox auto-processing
   - session persistence
   - outbox resend/drop
   - peer failure feedback
4. 将 secure 状态与本地消息表、本地 outbox 表关联。
5. 处理 listener 与 secure auto-processing 的边界。

**交付物**：

- secure 命令面
- session store
- outbox retry/drop 机制
- E2EE 收发主路径

**验收标准**：

- direct E2EE 可以建立 session、发送、处理 incoming、重试失败项
- secure 状态能通过 CLI 与 doctor 观察
- secure 失败不会污染 plain message 主路径

> 默认假设：group E2EE 不作为首发阻塞项，待 direct E2EE 稳定后再进入后续里程碑。

### Phase 7：Runtime、Listener、Heartbeat 与 IPC

**目标**：把 v1 的 realtime/runtime 机制收敛成 v2 独立 runtime 域。

**主要任务**：

1. 实现 runtime mode：
   - `http`
   - `websocket`
2. websocket 模式下实现：
   - listener 持有唯一远端连接
   - 本地 IPC / daemon 转发 CLI RPC
   - 后台服务 install/start/stop/restart/status
3. http 模式下实现：
   - 业务命令直接走服务端 RPC
   - listener 可选关闭
4. 实现 heartbeat 任务。
5. 实现 runtime setup/status/doctor 深度检查。
6. 兼容导入 v1 listener/settings 相关配置。
7. listener 启动前校验当前 identity 已完成 User 阶段，拒绝 local-only identity。

**交付物**：

- runtime mode 管理
- listener 服务管理
- IPC / local daemon
- heartbeat

**验收标准**：

- `runtime status/setup/mode/listener/heartbeat` 命令可用
- websocket 模式下所有消息命令能通过本地 daemon 工作
- http 模式下无需 listener 也可执行业务命令
- doctor 能报告 runtime 当前状态和异常原因

### Phase 8：扩展域、skills、docs、schema 生成

**目标**：建立“命令元数据驱动产品文档”的闭环。

**主要任务**：

1. 实现扩展域：
   - `people`
   - `page`
   - `debug`
   - `discovery`（如果保留在扩展域）
2. 构建 `cmdmeta` 元数据层，作为以下输出的单一事实来源：
   - CLI help
   - `schema`
   - `docs` 生成
   - skills 命令引用
3. 拆分 skills：
   - `awiki-shared`
   - `awiki-id`
   - `awiki-msg`
   - `awiki-runtime`
   - `awiki-people`
   - `awiki-page`
   - `awiki-debug`
4. 建立 docs / schema / skills 引用校验。
5. 生成：
   - `schemas/cli.json`
   - `schemas/commands/*.json`
   - shell completion
   - man page

**交付物**：

- 扩展域命令
- skills 分层目录
- schema/docs/help 生成链路

**验收标准**：

- 命令帮助、schema、docs、skills 不再相互漂移
- 新增命令只需要更新一处元数据即可生成多处产物
- AI 不依赖外部 skill 也能理解核心 CLI 行为

### Phase 9：发布、切换与收尾

**目标**：把 v2 从“开发完成”转成“可发布、可切换、可回滚”的产品。

**主要任务**：

1. 接入 GoReleaser。
2. 产出多平台二进制与 checksums。
3. 可选接入 npm wrapper。
4. 生成安装说明、升级说明、迁移指南。
5. 落地 `migrate from-v1` 的完整入口。
6. 建立 shadow / verify / cutover 策略：
   - 影子校验
   - 双跑对比
   - 切默认
   - 回滚策略

**交付物**：

- release pipeline
- migration guide
- cutover checklist

**验收标准**：

- release 可生成 macOS / Linux / Windows 包
- 用户可从 v1 导入 credentials 和 SQLite 数据
- 文档明确说明切换、回滚和兼容边界

---

## 7. 里程碑与建议拆包

为了便于拆成 Epic/Issue，建议按下列工作包推进。

| 编号 | 工作包 | 对应阶段 | 完成定义 |
|---|---|---|---|
| EPIC-01 | 命令壳与输出协议 | Phase 0-1 | 根命令、输出 envelope、schema/doctor 骨架完成 |
| EPIC-02 | 配置与路径体系 | Phase 2 | 单根目录工作区、env 兼容、default identity 解析完成 |
| EPIC-03 | identity store 与迁移 | Phase 2 | index.json、identity dir、v1 credential import 完成 |
| EPIC-04 | user + handle lifecycle | Phase 3 | register/bind/recover/profile/current + user gating 完成 |
| EPIC-05 | SQLite schema 与 DAO | Phase 4 | 表/视图/migration/fixtures 完成 |
| EPIC-06 | direct messaging | Phase 5 | send/inbox/history/mark-read 全链路完成 |
| EPIC-07 | group lifecycle | Phase 5 | create/join/show/members/messages/update/leave/kick 完成 |
| EPIC-08 | secure direct messaging | Phase 6 | session/outbox/retry/drop/auto-process 完成 |
| EPIC-09 | runtime 与 listener | Phase 7 | http/websocket、listener、IPC、heartbeat 完成 |
| EPIC-10 | docs/schema/skills 生成 | Phase 8 | cmdmeta 驱动链路闭环完成 |
| EPIC-11 | 发布与切换 | Phase 9 | goreleaser、迁移指南、cutover checklist 完成 |

---

## 8. 测试计划

### 8.1 单元测试

覆盖：

- config merge 与 env fallback
- identity index 解析与默认 identity 选择
- local-only identity vs registered user 状态判断
- output envelope / error mapping / exit code
- schema 生成与 help 生成
- thread id 生成
- SQLite DAO 与 view 查询
- secure session / outbox 状态机

### 8.2 迁移测试

覆盖：

- 从 `../awiki-agent-id-message/.credentials/` 样本导入
- 从 v1 SQLite 样本导入
- legacy flat file 检测与修复
- owner_did rebind
- secure session / outbox 导入

### 8.3 API 集成测试

覆盖：

- `authentication.md`
- `did-auth.md`
- `handle.md`
- `profile.md`
- `did-profile.md`
- `users.md`
- `relationships.md`
- `group.md`
- `ANP-client-server-api-direct.md`
- `ANP-client-server-api-group.md`
- `ANP-client-server-api-attachment.md`（若首版纳入附件）

### 8.4 runtime 测试

覆盖：

- http mode
- websocket mode
- listener install/start/stop/status
- 本地 IPC / daemon
- heartbeat run/status
- runtime doctor 检查

### 8.5 系统测试

建议通过同级服务仓完成端到端联调，主链路至少覆盖：

1. `id create`（仅验证本地 bootstrap）
2. `id register --handle ...`
3. `id bind`
4. `id current` / `doctor`
5. `msg send --to`
6. `msg inbox`
7. `msg history`
8. `group create` / `join` / `members` / `messages`
9. `msg secure init` / `msg secure retry`
10. `runtime mode set websocket` + listener

### 8.6 文档与生成校验

覆盖：

- 每个命令都有 schema
- 每个命令 help 可生成
- skills 中引用的命令必须存在
- docs / schema / help / generated reference 不允许漂移

---

## 9. 最终验收标准

以下条件全部满足时，才认为 v2 首版达到“可发布”标准：

1. 所有核心能力都通过 `awiki-cli` 统一入口暴露。
2. `id / msg / group / runtime` 的主链路可实际执行，不仅有命令壳。
3. handle-backed user 阶段是显式能力：可以区分 local-only identity 与 registered user。
4. `msg` 与 `runtime listener` 默认拒绝未完成 handle 注册的 identity。
5. `schema`、`doctor`、`docs` 是 CLI 本体能力，而不是外部补丁。
6. 所有副作用命令支持 `--dry-run`。
7. 所有命令遵循统一输出协议和错误协议。
8. direct plain、group plain、direct secure 三条主路径可用。
9. websocket runtime + listener + IPC 可用。
10. SQLite 本地状态可创建、升级、迁移、诊断。
11. 凭证目录布局支持 v2 新格式，并兼容从 v1 导入。
12. docs / schema / skills / help 由统一元数据驱动，避免文档漂移。
13. 发布链路可生成多平台包，并有明确迁移与回滚说明。

---

## 10. 默认实现假设

除非后续明确调整，本规划默认采用以下实现假设：

1. 首版语言固定为 Go。
2. CLI 框架固定为 Cobra。
3. 首版采用 handle-first 用户流程；handle 为必填，不支持 pure DID 作为对外用户完成态。
4. `id create` 仅作为本地 bootstrap / 迁移辅助能力，不作为 `msg`、`group`、`runtime listener` 的前置完成态。
5. 首版优先实现 direct E2EE，不把 group E2EE 作为首发阻塞项。
6. 首版不强制接入系统 keychain，可先采用受权限保护的文件存储；但接口层需要预留 keychain 扩展点。
7. 首版必须兼容导入 `../awiki-agent-id-message/` 的凭证与 SQLite 数据。
8. 首版不追求覆盖 v1 所有边角脚本，而是先覆盖架构文档定义的 canonical 命令面。
9. 首版发布主渠道为 GitHub Releases；npm wrapper 为可选增强项。
10. Go 核心实现固定使用 Go 1.22，并保持 pure Go，禁止依赖 CGO；如果后续需要做系统兼容性壳层，可放在 TypeScript/Node 的薄壳中实现。

---

## 11. 本文档维护规则

- 当 `docs/architecture/awiki-v2-architecture.md` 或 `docs/architecture/awiki-command-v2.md` 发生影响实施范围的变化时，必须同步更新本文档。
- 当 v1 SQLite schema 或凭证布局的参考基线发生变化时，需要同步更新“参考基线”章节。
- 当新增一级命令或调整 canonical contract 时，必须同步更新 Phase、工作包、验收标准与测试计划。
