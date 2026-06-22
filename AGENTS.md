# AGENTS.md

## 项目记忆

- 本仓库后续所有规划文档、计划文档、执行计划、步骤计划和计划类记录必须使用中文撰写，不要生成英文 plan 文档。
- 技术标识、代码符号、命令、路径、crate 名、API 名、SQL 片段和外部协议名称可以保留原文英文；解释性文字、目标、步骤、验收标准、风险、复盘和报告应使用中文。
- 如果用户明确要求生成计划文档，默认按中文输出，除非用户在同一请求中明确要求使用其他语言。
- 涉及系统测试的最终集成步骤必须在 `../awiki-system-test` 路径下执行完整系统测试，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名，并记录实际命令、通过/失败/跳过数量、失败或跳过原因以及关键环境配置。
- 需要把本地工作区的 `awiki-deamon` 编译后通过本机 Nginx 暴露给联调时，使用 `scripts/release/daemon/publish-local-nginx.sh`。默认命令 `scripts/release/daemon/publish-local-nginx.sh` 会编译当前主机平台包并发布到 `/var/www/awiki-web/daemon-local`，对外地址为 `https://awiki.info/daemon-local`，不覆盖正式 `/daemon` 通道。联调安装示例：`curl -fsSL https://awiki.info/daemon-local/install.sh | sh -s -- --token <token> --state-root /tmp/awiki-daemon-local --foreground`。
- 如果用户明确要求本地编译产物也通过正式线上 daemon 地址访问，使用 `scripts/release/daemon/publish-local-nginx.sh --official`。该模式会把当前主机平台包发布到 `https://awiki.info/daemon/releases/<version>/...`，并合并现有正式 manifest；默认保留正式 manifest 的 `latest`/`min_supported`，所以安装联调包时需要显式加 `--version <version>`，例如：`curl -fsSL https://awiki.info/daemon/install.sh | sh -s -- --token <token> --version <version> --state-root /tmp/awiki-daemon-local --foreground`。
- 不要随意使用 `scripts/release/daemon/publish-local-nginx.sh --official --promote-latest`。该选项会把正式 manifest 的 `latest`/`min_supported` 提升到本地版本；只有确认该版本所有正式支持平台包都已存在，或用户明确接受当前只有单平台包的影响时才可使用。
