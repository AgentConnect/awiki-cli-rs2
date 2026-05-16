# Phase 0 审计结论

**状态**：Resolved / Frozen  
**用途**：记录 Phase 0 审计发现、裁决结果和后续同步动作。  
**最后更新**：2026-04-04

---

## 1. 审计摘要

本次审计重点比对了四类来源：

1. `docs/architecture/awiki-command-v2.md`
2. `docs/architecture/awiki-v2-architecture.md`
3. `../awiki-agent-id-message/`
4. `../user-service/docs/api/` 与 `../message-service/docs/api/`

审计结论：

- 大方向一致，主要冲突集中在 **命令归属、环境变量命名、raw API 暴露方式、v1 数据基线表述不完整**。
- 这些冲突都不会阻塞 Phase 1，只要先冻结裁决。
- 已冻结项必须以后续实现文档和代码为准，原始草案需要后补同步。

---

## 2. 审计发现与裁决

### AF-001：`group` 顶级命令 vs `msg group` 子命令冲突

**证据**：

- `docs/architecture/awiki-v2-architecture.md` 附录 B 使用顶级 `awiki-cli group`
- `docs/plan/awiki-v2-implementation-plan.md` 也使用顶级 `group`
- `docs/architecture/awiki-command-v2.md` 的 canonical tree 使用 `awiki-cli msg group ...`

**裁决**：

- canonical 公共命令面采用 **顶级 `group`**
- `msg group ...` 不作为 Phase 1 必做公共 surface
- 如果后续实现 `msg group ...`，只能作为兼容 alias

**原因**：

- `group` 是独立领域对象，不只是消息目标
- 与 `user-service/docs/api/group.md` 更一致
- 与主实施计划和总体架构更一致

**后续动作**：

- 同步 `awiki-command-v2.md` 的命令树描述

### AF-002：顶级 `api` vs `debug raw rpc` 冲突

**证据**：

- `docs/architecture/awiki-v2-architecture.md` 附录 B 包含顶级 `awiki-cli api`
- `docs/architecture/awiki-command-v2.md` 只保留 `debug raw rpc`

**裁决**：

- Phase 1 不暴露顶级 `api`
- raw RPC 统一放在 `debug raw rpc`
- 顶级 `api` 仅作为未来保留选项，不进入首发契约

**原因**：

- 避免过早暴露额外公共 surface
- 减少和业务命令层的竞争

**后续动作**：

- 在总体架构附录中把 `api` 标注为 reserve / non-phase-1

### AF-003：配置入口过多，工作区与业务配置来源分裂

**证据**：

- 历史文档同时出现 `AWIKI_*`、`AVIKI_*`、`E2E_*`
- 工作区路径和业务配置都可以通过环境变量注入
- 主配置文件历史上使用 `config.yaml`

**裁决**：

- 唯一保留的环境变量是 **`AWIKI_CLI_WORKSPACE_HOME_DIR`**
- 所有业务配置统一写入 **`config.yaml`**
- 目录级 override 环境变量全部废弃
- 旧变量与旧 `config.json` 全部停止兼容，检测到即报错

**原因**：

- 避免多入口导致的配置漂移
- 让工作区定位和业务配置边界清晰
- 降低排障成本，形成单一事实来源

**后续动作**：

- 同步命令文档、安装文档和实现计划中的配置约束描述

### AF-004：用户层术语 `identity` 与存储层术语 `credential` 不一致

**证据**：

- v2 文档主张使用 `identity`
- v1 credential layout 与 SQLite 字段中仍使用 `credential_name`、`default_credential_name`

**裁决**：

- 用户层统一使用 `identity`
- 存储层 Phase 1 保留 `credential_name` / `default_credential_name`，以兼容导入与最小迁移
- CLI `--identity` 为 canonical 选项
- `--credential` 如果提供，只能作为 deprecated alias

**原因**：

- 避免对 v1 现有数据和导入器造成不必要破坏
- 用户层与存储层可以阶段性脱钩

**后续动作**：

- 在 Go 类型层显式建立 identity/credential 桥接

### AF-005：SQLite schema 文档缺失 `e2ee_outbox`

**证据**：

- `../awiki-agent-id-message/references/local-store-schema.md` 未列出 `e2ee_outbox`
- `../awiki-agent-id-message/scripts/local_store.py` 中明确创建并使用 `e2ee_outbox`
- `../awiki-agent-id-message/scripts/e2ee_outbox.py` 的 secure resend/drop 完全依赖该表

**裁决**：

- v2 SQLite 基线以 `local_store.py` 为 source of truth
- `e2ee_outbox` 是首版必保留表，不得省略

**原因**：

- 否则 secure retry / drop / failure recovery 无法保留

**后续动作**：

- 后补同步 `local-store-schema.md`

### AF-006：单根目录工作区 vs `.openclaw` 旧路径冲突

**证据**：

- v2 文档要求单根目录工作区路径
- v1 Python CLI 实际使用 `~/.openclaw/credentials/awiki-agent-id-message/` 与 `~/.openclaw/workspace/data/awiki-agent-id-message/`

**裁决**：

- v2 原生写入 `~/.awiki-cli/` 工作区
- `AWIKI_CLI_WORKSPACE_HOME_DIR` 是唯一工作区根目录环境变量入口
- `doctor` / `runtime setup` / `migrate from-v1` 负责检测旧路径
- 默认只提示导入，不原地修改旧数据

**原因**：

- 清晰区分 v2 新布局与 v1 遗留目录
- 降低误改旧环境的风险

**后续动作**：

- 在 migration 和 doctor 中提供显式旧路径检测结果

### AF-007：Go 兼容性策略新增要求——禁止 CGO

**证据**：

- 用户新增要求：Go 核心实现必须 pure Go，保证兼容性，不能使用 CGO
- 总体架构技术选型中已有“无 CGO SQLite”的方向，但未升级为硬约束

**裁决**：

- `awiki-cli` 主二进制必须保持 **pure Go / no CGO**
- Phase 1 及之后所有依赖选型都必须默认满足 `CGO_ENABLED=0`
- 如果需要做系统兼容性、平台安装器或壳层集成，可以放到 **TS shell** 完成，而不是引入 CGO

**原因**：

- 降低多平台发布复杂度
- 保持二进制可移植性和构建稳定性
- 避免后续被系统原生依赖锁死

**后续动作**：

- 在技术选型与发布阶段明确验证 `CGO_ENABLED=0`

### AF-008：secure 协议和首发范围仍需冻结

**证据**：

- 总体架构文档明确记录了 E2EE 协议历史冲突
- v1 secure 流程复杂，涉及 session、outbox、listener auto-processing

**裁决**：

- Phase 0 只冻结首发范围：**direct E2EE 必做，group E2EE 不阻塞首发**
- 协议具体版本必须由独立 ADR 冻结后再编码

**原因**：

- direct secure 是差异化核心能力
- group secure 不应阻塞 CLI 壳与 plain path

**后续动作**：

- 进入 Phase 5 前先写完 E2EE 协议 ADR

### AF-009：更新提示字段 `notice` vs `_notice` 冲突

**证据**：

- `docs/architecture/awiki-command-v2.md` 提到冲突收敛
- `docs/architecture/output-format.md` 使用 `_notice`
- 飞书 CLI 的输出装饰也倾向于单独 notice 注入字段

**裁决**：

- v2 canonical 输出字段固定为 `_notice`
- `notice` 不进入新 envelope

**原因**：

- 避免与业务 `data` 混淆
- 保持统一与稳定

**后续动作**：

- Phase 1 输出层只实现 `_notice`

### AF-010：`discovery` 是否进入 Phase 1 顶级壳

**证据**：

- `awiki-command-v2.md` 的 canonical tree 含 `discovery`
- 主实施计划的 Phase 1 命令壳列表未把 `discovery` 设为必须

**裁决**：

- `discovery` 视为保留扩展域
- Phase 1 可以不实现顶级 `discovery` 壳
- 真正进入实现时不晚于 Phase 7/扩展域

**原因**：

- 避免在核心壳阶段引入未稳定工作流
- 不影响 `id/msg/group/runtime` 主链路

**后续动作**：

- 在 docs/help 中将 `discovery` 标注为 reserved extension
