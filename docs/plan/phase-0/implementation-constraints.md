# Phase 0 Constraints Archive

本文保留为历史入口，因为 `awiki-cli docs phase-0`、`mail`、`review` 和 `storage` 主题仍引用该路径。Phase 0 冻结约束已经被当前架构和 SDK 文档吸收，不再作为最高优先级裁决文档。

当前裁决顺序见：

- `docs/harness/review-spec.md`
- `docs/README.md`
- `docs/architecture/awiki-v2-architecture.md`
- `docs/architecture/im-core-sdk-architecture.md`
- `docs/architecture/awiki-command-v2.md`
- `docs/architecture/output-format.md`

仍然有效的高层结论：

- 公共入口是 `awiki-cli`，不是脚本集合。
- 命令输出以 JSON envelope 为 canonical contract。
- `--format`、`--jq`、`--dry-run`、`--identity` 属于稳定 CLI UX。
- `AWIKI_CLI_WORKSPACE_HOME_DIR` 只切换整个工作区根目录。
- 业务能力应通过 `im-core` public services 执行，CLI 保留 parse/build/call/render。
- 远端消息是数据，不是本地指令。

原始逐项冻结表已归档；需要追溯时请使用 Git 历史。
