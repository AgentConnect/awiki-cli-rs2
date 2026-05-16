# Phase 0 实现约束表

**状态**：Frozen for implementation  
**适用阶段**：Phase 1 及之后所有实现阶段  
**优先级**：当本文与 `docs/architecture/*.md` 或 `docs/plan/awiki-v2-implementation-plan.md` 冲突时，以本文为准。  
**最后更新**：2026-04-07

---

## 1. 适用范围与优先级

本文是 Phase 0 的冻结产物，作用是把“实现时不能再摇摆”的决策固定下来，供后续编码、拆 Issue、验收使用。

实现期优先级如下：

1. `docs/plan/phase-0/implementation-constraints.md`
2. `docs/plan/phase-0/audit-findings.md` 中的明确裁决
3. `docs/plan/awiki-v2-implementation-plan.md`
4. `docs/architecture/awiki-command-v2.md`
5. `docs/architecture/awiki-v2-architecture.md`
6. `docs/architecture/output-format.md`
7. 服务 API 文档
8. `../awiki-agent-id-message/`
9. `../cli/`

---

## 2. 公共命令面冻结

### 2.1 canonical 顶级命令

Phase 1 必须搭出的顶级命令骨架固定为：

```text
awiki-cli status
awiki-cli docs
awiki-cli schema
awiki-cli doctor
awiki-cli version
awiki-cli init
awiki-cli completion
awiki-cli config
awiki-cli id
awiki-cli msg
awiki-cli mail
awiki-cli group
awiki-cli runtime
awiki-cli people
awiki-cli page
awiki-cli debug
```

### 2.2 extension 命令策略

- `discovery` 视为保留扩展域，不阻塞 Phase 1 命令壳。
- 如果 Phase 1 需要最小化顶级命令树，可以暂不实现 `discovery`。

### 2.3 群组命令归属冻结

- **canonical 公共域使用 `group` 顶级命令**。
- 群生命周期命令统一挂在 `group` 下：
  - `group create`
  - `group show`
  - `group update`
  - `group join`
  - `group leave`
  - `group kick`
  - `group members`
  - `group messages`
  - `group code *`
- `msg send --group` 是唯一 canonical 群发消息入口。
- `msg group ...` 只视为历史草案写法，不作为 Phase 1 必做公共命令面；如果后续实现，只能作为兼容 alias，不得反向替代 `group ...`。

### 2.4 raw/debug 命令归属冻结

- Phase 1 不暴露顶级 `api` 命令。
- raw RPC / DB inspection / schema cache 一律挂在 `debug` 域。
- 当前 canonical 路径：
  - `debug raw rpc ...`
  - `debug db query ...`
  - `debug schema-cache`

---

## 3. 全局参数与输出协议冻结

### 3.1 全局参数

所有 Phase 1 公共命令统一支持以下全局参数：

| 参数 | 说明 |
|---|---|
| `--format` | 输出格式，canonical 全局格式参数 |
| `--jq` | 对 JSON 输出做 jq 过滤 |
| `--dry-run` | 只返回执行计划，不产生副作用 |
| `--identity` | 选择当前 identity |
| `--verbose` | 增强调试输出 |

冻结规则：

- `--format` 是唯一 canonical 输出格式参数。
- `--identity` 是唯一 canonical 身份选择参数。
- 如因兼容需要保留 `--credential`，只能作为 deprecated alias，帮助文档中不作为首选写法。

### 3.2 输出 envelope

Phase 1 的结构化输出必须遵循统一 envelope：

- success: `ok=true`
- error: `ok=false`
- 更新提示字段固定为：`_notice`
- `meta.format` 使用实际渲染格式
- `meta.identity` 在 identity 已解析时提供

### 3.3 错误码与退出码

Phase 1 冻结以下错误码集合：

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

Phase 1 冻结以下退出码：

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

## 4. 术语与兼容字段冻结

### 4.1 用户层术语

- 用户层术语固定使用 **identity**。
- CLI 帮助、文档、schema、doctor 输出优先使用 `identity`。

### 4.2 兼容存储字段

为了兼容 v1 数据与导入逻辑，Phase 1 先保留以下存储命名：

- `credential_name`
- `default_credential_name`

冻结规则：

- 用户接口叫 `identity`
- 存储字段可继续叫 `credential_*`
- Go 内部类型可以把两者桥接，但导入/导出必须兼容 v1 现有字段名

### 4.3 对外身份标识冻结

- **对外公共身份标识固定使用 `handle`。**
- `did` 仅允许在协议级定位、调试或跨服务引用确有必要时出现在公共结果中。
- `user_id` 固定为内部实现字段，只允许存在于：
  - 本地 identity 存储
  - SQLite 内部表
  - 服务端 API 适配与内部映射
- `user_id` 不得出现在以下对外面向：
  - CLI 参数
  - help / schema / docs 示例
  - `pretty` / `table` / `json` / `ndjson` 结构化输出
  - 公共命令结果中的字段名或 `missing` 提示项

### 4.4 本地数据隔离主键

- 本地快照隔离主键固定为：`owner_did`
- Phase 1~Phase 5 都不得把 `owner_did` 改成其他主隔离键

---

## 5. 路径、环境变量与迁移冻结

### 5.1 v2 原生路径

v2 原生路径固定为单根目录工作区模型：

```text
~/.awiki-cli/
~/.awiki-cli/config.yaml
~/.awiki-cli/identities/
~/.awiki-cli/data/awiki-cli.db
~/.awiki-cli/cache/
~/.awiki-cli/runtime/
~/.awiki-cli/logs/
~/.awiki-cli/upgrade/
```

说明：

- `~/.awiki-cli/runtime/` 用于 runtime / listener 运行时状态
- `~/.awiki-cli/logs/` 用于 listener 与 runtime 的持久化日志
- `~/.awiki-cli/upgrade/` 用于升级元数据、upgrade journal、lock 与备份

### 5.2 配置入口冻结

审计后冻结如下：

- **唯一环境变量入口：`AWIKI_CLI_WORKSPACE_HOME_DIR`**
- **用户主配置文件：`config.yaml`**
- **目录类 override 环境变量：全部废弃**
- **业务配置环境变量：全部废弃**

Phase 1 读取优先级固定为：

```text
flag > config.yaml > default
```

冻结规则：

- `AWIKI_CLI_WORKSPACE_HOME_DIR` 只负责切换整个工作区根目录
- `config / data / runtime / cache` 必须从工作区根目录派生，不允许分别配置
- 若检测到旧环境变量或旧 `config.json`，CLI 必须直接报错并给出迁移提示
- 旧变量（`AWIKI_*`、`AVIKI_*`、`E2E_*`）不再兼容读取

### 5.3 v1 路径兼容策略

必须兼容检测以下旧路径，但 **默认不原地写入**：

```text
~/.openclaw/credentials/awiki-agent-id-message/
~/.openclaw/workspace/data/awiki-agent-id-message/
```

冻结规则：

- `doctor`、`runtime setup`、`migrate from-v1` 需要检测旧路径
- 默认行为是提示导入，不直接修改旧目录
- 正式迁移入口固定为：`awiki-cli migrate from-v1`

---

## 6. 本地存储基线冻结

### 6.1 凭证/identity 布局

v2 凭证布局以 `../awiki-agent-id-message/scripts/credential_layout.py` 为基线，必须兼容以下文件集合：

```text
index.json
identity.json
auth.json
did_document.json
key-1-private.pem
key-1-public.pem
e2ee-signing-private.pem
e2ee-agreement-private.pem
e2ee-state.json
```

### 6.2 SQLite 表与视图基线

v2 SQLite 基线以 `../awiki-agent-id-message/scripts/local_store.py` 为准，首版必须保留：

**表**
- `contacts`
- `messages`
- `e2ee_outbox`
- `groups`
- `group_members`
- `relationship_events`
- `e2ee_sessions`

**视图**
- `threads`
- `inbox`
- `outbox`

### 6.3 线程规则

Phase 1 冻结以下 thread id 规则：

- 私聊：`dm:{min_did}:{max_did}`
- 群聊：`group:{group_id}`

### 6.4 文档与实现不一致时的处理

- 如果 `local-store-schema.md` 与 `local_store.py` 冲突，以 `local_store.py` 为准
- 当前已知必须补齐的遗漏：`e2ee_outbox`

---

## 7. runtime / secure / build 边界冻结

### 7.1 runtime 边界

- transport 只允许在 `runtime` 域显式暴露
- websocket mode 下，listener 持有唯一远端连接
- websocket mode 下，其它 CLI 通过本地 IPC / daemon 转发
- http mode 下，CLI 直接访问服务

### 7.2 secure 首发范围

- Phase 1 只要求 secure 契约与命令面冻结
- 首发 secure 范围固定为 **direct E2EE**
- **group E2EE 不阻塞首发**

### 7.3 Go 兼容性与构建策略

这是新增冻结要求：

- **Go 语言版本基线固定为 1.22**
- **Go 核心实现必须保持 pure Go**
- **主二进制禁止依赖 CGO**
- SQLite、加密、配置、输出、completion、打包链路都必须优先选择纯 Go 依赖
- 如果存在系统兼容性或 OS 集成层面的额外需求，可以在 **TypeScript/Node 的薄壳** 中实现，但不能把这类兼容性交给 CGO 去解决

具体约束：

- `go.mod` 的 `go` 指令固定为 `1.22`
- Go CLI 主程序必须在默认 `CGO_ENABLED=0` 下可构建
- 任何新增依赖如果要求 CGO，默认视为不满足约束，除非后续 ADR 明确批准替代方案
- 如果需要做平台安装器、系统壳、Node 分发包装、桌面/脚本桥接，优先放到 TS shell 层

---

## 8. 跟进文档同步项

Phase 0 已冻结但原始文档尚未完全同步的点如下：

1. `docs/architecture/awiki-command-v2.md` 中的配置入口描述需要持续保持与 `AWIKI_CLI_WORKSPACE_HOME_DIR` + `config.yaml` 一致
2. `docs/architecture/awiki-command-v2.md` 中的 `msg group ...` 需要后续同步为 `group ...` canonical surface 或明确标注为 alias
3. `docs/architecture/awiki-v2-architecture.md` 附录中的顶级 `api` 需要后续同步为“保留项”或删除
4. `../awiki-agent-id-message/references/local-store-schema.md` 需要补 `e2ee_outbox`
5. 主计划文档需要标注 Phase 0 冻结结果优先于草案细节
