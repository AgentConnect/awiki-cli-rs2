# AGENTS.md

## 项目记忆

- 本仓库后续所有规划文档、计划文档、执行计划、步骤计划和计划类记录必须使用中文撰写，不要生成英文 plan 文档。
- 技术标识、代码符号、命令、路径、crate 名、API 名、SQL 片段和外部协议名称可以保留原文英文；解释性文字、目标、步骤、验收标准、风险、复盘和报告应使用中文。
- 如果用户明确要求生成计划文档，默认按中文输出，除非用户在同一请求中明确要求使用其他语言。
- 文档跟随本仓库 `AGENTS.md`、`README.md` 和 `docs/`。不要套 GEB 分形协议，不要为每个 crate 或目录新建 `CLAUDE.md`。
- 修改 canonical conversation、Persona/alias、WireIdentity、membership、消息投影或 release/0710 升级前，必须先阅读 [`docs/architecture/im-core-sdk-architecture.md`](docs/architecture/im-core-sdk-architecture.md)、[`docs/api/im-core-public-api.md`](docs/api/im-core-public-api.md) 和 [`docs/flutter-sdk/awiki-im-core-flutter-sdk.md`](docs/flutter-sdk/awiki-im-core-flutter-sdk.md)。禁止引入与其中 ownership、canonical identity、fail-closed 或迁移约束冲突的方案；架构确需改变时先更新这些权威文档。
- 涉及系统测试的最终集成步骤必须在 `../awiki-system-test` 路径下执行完整系统测试，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 并通过 `AWIKI_SYSTEM_TEST_TARGET` 显式选择该任务批准的远程环境；不得依赖隐式默认域名。必须记录实际命令、目标环境、通过/失败/跳过数量、失败或跳过原因以及关键环境配置。
- 需要把本地工作区的 `awiki-deamon` 编译后通过本机 Nginx 暴露给联调时，使用 `scripts/release/daemon/publish-local-nginx.sh`。默认命令 `scripts/release/daemon/publish-local-nginx.sh` 会编译当前主机平台包并发布到 `/var/www/awiki-web/daemon-local`，对外地址为 `https://awiki.info/daemon-local`，不覆盖正式 `/daemon` 通道。联调安装示例：`curl -fsSL https://awiki.info/daemon-local/install.sh | sh -s -- --token <token> --state-root /tmp/awiki-daemon-local --foreground`。
- 如果用户明确要求本地编译产物也通过正式线上 daemon 地址访问，使用 `scripts/release/daemon/publish-local-nginx.sh --official`。该模式会把当前主机平台包发布到 `https://awiki.info/daemon/releases/<version>/...`，并合并现有正式 manifest；默认保留正式 manifest 的 `latest`/`min_supported`，所以安装联调包时需要显式加 `--version <version>`，例如：`curl -fsSL https://awiki.info/daemon/install.sh | sh -s -- --token <token> --version <version> --state-root /tmp/awiki-daemon-local --foreground`。
- 不要随意使用 `scripts/release/daemon/publish-local-nginx.sh --official --promote-latest`。该选项会把正式 manifest 的 `latest`/`min_supported` 提升到本地版本；只有确认该版本所有正式支持平台包都已存在，或用户明确接受当前只有单平台包的影响时才可使用。
- 测试代码尽量不要放到业务代码文件中。优先放在 `tests/`、`crates/<crate>/tests/` 等独立测试目录；确实需要访问私有 helper 的单元测试，可放到相邻的 `*_tests.rs` / `tests.rs` 测试专用文件中，并在业务文件里只保留最小的 `#[cfg(test)] mod tests;` 引用。
- 任何代码的新增或修改，都必须在同一任务中同步新增或修改对应的单元测试、系统测试和端到端（E2E）测试。测试应放在其权威仓库或测试框架中；需要跨仓库覆盖时，必须在同一任务中同步修改对应仓库。
- 发布 `awiki-deamon` 下载通道时，`scripts/release/daemon/publish-multi-platform.toml` 的 `base_url` 必须与签发 Daemon registration token 的 user-service 域名一致，例如 awiki.info 环境使用 `https://awiki.info`。`download_base_url` 默认应为同域 `/daemon`；镜像只作为备用下载源，不应让 installer 默认把 awiki.info token 发送到 awiki.ai 验证。
