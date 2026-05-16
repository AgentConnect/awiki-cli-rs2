# Phase 0 ADR Index

**状态**：Index frozen, ADR bodies pending as needed  
**用途**：记录 Phase 0 已识别的强约束决策，供后续阶段引用。  
**最后更新**：2026-04-04

---

## 1. 使用方式

- 本文件先冻结 ADR 编号、主题和已决定的方向。
- 若后续需要单独 ADR 文件，可按编号扩展到 `docs/plan/phase-0/adrs/ADR-xxxx-*.md`。
- 在独立 ADR 文件补齐之前，`implementation-constraints.md` 和 `audit-findings.md` 中的冻结结论视为有效决策正文。

---

## 2. ADR 列表

| ADR | 主题 | 状态 | 影响阶段 | 冻结结论 |
|---|---|---|---|---|
| ADR-0001 | 公共命令面冻结 | Frozen | Phase 1+ | 顶级命令以 `status/docs/schema/doctor/version/init/completion/config/id/msg/mail/group/runtime/people/page/debug` 为准 |
| ADR-0002 | `group` 域归属 | Frozen | Phase 1+ | `group` 为 canonical 顶级域；`msg send --group` 负责群发消息；`msg group` 只可作为兼容 alias |
| ADR-0003 | raw API 暴露方式 | Frozen | Phase 1+ | 首发不暴露顶级 `api`；raw RPC 挂在 `debug raw rpc` |
| ADR-0004 | 用户术语与存储术语 | Frozen | Phase 1+ | 用户层使用 `identity`；存储层 Phase 1 保留 `credential_name` / `default_credential_name` |
| ADR-0005 | 输出协议与 `_notice` 字段 | Frozen | Phase 1+ | 统一 JSON envelope；更新提示字段固定为 `_notice` |
| ADR-0006 | 配置入口与路径收口 | Frozen | Phase 1+ | 仅保留 `AWIKI_CLI_WORKSPACE_HOME_DIR` 作为工作区环境变量；用户主配置固定为 `config.yaml`；旧 `AWIKI_*` / `AVIKI_*` / `E2E_*` 与 `config.json` 全部停止兼容 |
| ADR-0007 | 凭证文件基线 | Frozen | Phase 2+ | 凭证布局以 `credential_layout.py` / `credential_store.py` 为基线，兼容 indexed multi-credential layout |
| ADR-0008 | SQLite 基线与 source of truth | Frozen | Phase 3+ | SQLite 以 `local_store.py` 为 source of truth；`e2ee_outbox` 是首版必保留表 |
| ADR-0009 | runtime mode 与 listener 边界 | Frozen | Phase 1 / 6+ | transport 只在 `runtime` 暴露；websocket mode 下 listener 持有唯一远端连接 |
| ADR-0010 | secure 首发范围 | Frozen | Phase 5+ | 首发只做 direct E2EE；group E2EE 不阻塞首发 |
| ADR-0011 | Go 兼容性与构建策略 | Frozen | Phase 1+ | Go 核心固定使用 Go 1.22，必须 pure Go，无 CGO；系统兼容性壳层可放在 TS shell |

---

## 3. 后续建议的独立 ADR 正文优先级

如果后续要把 ADR 写成独立文件，建议按以下顺序补齐：

1. `ADR-0001` 公共命令面冻结
2. `ADR-0006` 环境变量与路径兼容
3. `ADR-0008` SQLite 基线与 source of truth
4. `ADR-0011` Go 兼容性与构建策略
5. `ADR-0010` secure 首发范围
6. `ADR-0009` runtime mode 与 listener 边界

---

## 4. 引用约定

后续文档或代码实现说明里，建议直接引用如下格式：

- `ADR-0001`：公共命令面冻结
- `ADR-0006`：环境变量与路径兼容
- `ADR-0011`：Go 兼容性与构建策略

这样可以避免在每个阶段重复解释同一个决定。
