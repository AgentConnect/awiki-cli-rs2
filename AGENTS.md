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
- 临时源码构建使用 `scripts/release/daemon/_build-artifact.sh --local-core`，合同见 `docs/publish.md` 的“临时集成当前 Core 源码”。该模式在隔离的已提交源码中集成 Core，ANP/Identity 仍为固定 registry 版本，并使用独立提交的 `local-core.Cargo.lock`；默认入口继续强制 registry SDK。
- 2026-09-17 用户明确授权本次新加坡 ACP 测试用源码包经 `https://anpclaw.com/daemon` 发布，仅提供 macOS arm64、Linux amd64。可使用 `_stage-downloads.sh --allow-partial`；保留最低支持版本及旧包，更新公网 manifest 与租户推荐政策。APP 原命令的最终人工安装验收由用户执行，不要求其他平台。
- 历史 `publish-local-nginx.sh` 已删除，不再引用它作为可执行发布入口。其他临时源码或不完整平台发布仍必须匹配用户明确的范围，不能静默绕过默认正式 registry 流程。
- 测试代码尽量不要放到业务代码文件中。优先放在 `tests/`、`crates/<crate>/tests/` 等独立测试目录；确实需要访问私有 helper 的单元测试，可放到相邻的 `*_tests.rs` / `tests.rs` 测试专用文件中，并在业务文件里只保留最小的 `#[cfg(test)] mod tests;` 引用。
- 发布 `awiki-deamon` 下载通道时，`scripts/release/daemon/publish-multi-platform.toml` 的 `base_url` 必须与签发 Daemon registration token 的 user-service 域名一致，例如 awiki.info 环境使用 `https://awiki.info`。`download_base_url` 默认应为同域 `/daemon`；镜像只作为备用下载源，不应让 installer 默认把 awiki.info token 发送到 awiki.ai 验证。
