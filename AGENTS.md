# AGENTS.md

## Shared rules

Engineering work follows [AI Coding Rules](../awiki-harness/rules/ai-coding-rules.md).
Behavior changes and verification follow the relevant [Verification Policy](../awiki-harness/rules/verification-policy.md)
sections; production behavior needs owning unit coverage and applicable System/product E2E review.
If Harness is absent, use local docs/tests/CI and disclose missing acceptance evidence.

## 项目记忆

- 本仓库后续所有规划文档、计划文档、执行计划、步骤计划和计划类记录必须使用中文撰写，不要生成英文 plan 文档。
- 不要套 GEB 分形协议，也不要为每个文件夹补建 `CLAUDE.md`；仓库导向使用本文件和权威 `docs/`。
- 技术标识、代码符号、命令、路径、crate 名、API 名、SQL 片段和外部协议名称可以保留原文英文；解释性文字、目标、步骤、验收标准、风险、复盘和报告应使用中文。
- 改动前按受影响职责定位权威文档的相关章节；已有证据充分时无需通读三份文档。保留 ownership、canonical identity、fail-closed 和迁移不变式；确需改变架构时先更新对应权威合同。
  - Core ownership、身份、canonical conversation、membership、消息投影：查 [SDK 架构](docs/architecture/im-core-sdk-architecture.md) 的 `Host vs SDK Responsibilities`、`Identity Model`、`Durable Conversation Registry`、`Reliable Message Sync` 或 `Conversation Read State`。
  - 公共 facade/DTO/错误、升级入口：查 [公共 API](docs/api/im-core-public-api.md) 中受影响模块或 `Core open 前的 local-state 升级与恢复`。
  - Dart/Flutter binding、生命周期或宿主调用：查 [Flutter SDK](docs/flutter-sdk/awiki-im-core-flutter-sdk.md) 的对应功能章节；纯 Core 内部变化无需额外通读 Flutter 文档。
  - 跨越多个职责或仍有契约疑问时，再扩展到相邻章节和调用方。
- 需要把本地工作区的 `awiki-deamon` 编译后通过本机 Nginx 暴露给联调时，使用 `scripts/release/daemon/publish-local-nginx.sh`。默认命令 `scripts/release/daemon/publish-local-nginx.sh` 会编译当前主机平台包并发布到 `/var/www/awiki-web/daemon-local`，对外地址为 `https://awiki.info/daemon-local`，不覆盖正式 `/daemon` 通道。联调安装示例：`curl -fsSL https://awiki.info/daemon-local/install.sh | sh -s -- --token <token> --state-root /tmp/awiki-daemon-local --foreground`。
- 如果用户明确要求本地编译产物也通过正式线上 daemon 地址访问，使用 `scripts/release/daemon/publish-local-nginx.sh --official`。该模式会把当前主机平台包发布到 `https://awiki.info/daemon/releases/<version>/...`，并合并现有正式 manifest；默认保留正式 manifest 的 `latest`/`min_supported`，所以安装联调包时需要显式加 `--version <version>`，例如：`curl -fsSL https://awiki.info/daemon/install.sh | sh -s -- --token <token> --version <version> --state-root /tmp/awiki-daemon-local --foreground`。
- 不要随意使用 `scripts/release/daemon/publish-local-nginx.sh --official --promote-latest`。该选项会把正式 manifest 的 `latest`/`min_supported` 提升到本地版本；只有确认该版本所有正式支持平台包都已存在，或用户明确接受当前只有单平台包的影响时才可使用。
- 测试代码尽量不要放到业务代码文件中。优先放在 `tests/`、`crates/<crate>/tests/` 等独立测试目录；确实需要访问私有 helper 的单元测试，可放到相邻的 `*_tests.rs` / `tests.rs` 测试专用文件中，并在业务文件里只保留最小的 `#[cfg(test)] mod tests;` 引用。
- 发布 `awiki-deamon` 下载通道时，`scripts/release/daemon/publish-multi-platform.toml` 的 `base_url` 必须与签发 Daemon registration token 的 user-service 域名一致，例如 awiki.info 环境使用 `https://awiki.info`。`download_base_url` 默认应为同域 `/daemon`；镜像只作为备用下载源，不应让 installer 默认把 awiki.info token 发送到 awiki.ai 验证。
