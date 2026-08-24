# Docs Index

本文档目录按 SDK 重构后的当前事实重新整理。稳定文档用于日常开发、发布、集成和 review；已完成的迁移计划、PR closeout、逐次验证流水和 Go parity 记录不再保留在 `docs/` 中。

## 推荐阅读顺序

1. `architecture/awiki-v2-architecture.md`：当前系统总架构，说明 `im-core`、`awiki-cli`、Flutter SDK、runtime 和本地状态的职责。
2. `architecture/im-core-sdk-architecture.md`：`im-core` SDK 架构、CLI thin shell 边界、模块职责、路径/依赖约束。
3. `architecture/identity-secret-storage.md`：端侧 identity SecretVault、CLI/App/daemon root key、私钥/JWT/E2EE local secret 加密持久化边界。
4. `architecture/awiki-command-v2.md` 与 `architecture/output-format.md`：CLI 命令面与结构化输出契约。
5. `installation.md`：本地构建、配置、工作区和常见运行方式。
6. `publish.md`：本地/服务器构建、文件服务 staging 和回滚流程。
7. `api/im-core-public-api.md` 与 `api/im-core-interface/*`：SDK API / interface 规格；只有 API 变化时才修改。

## 稳定文档分区

### 架构文档

- `architecture/awiki-v2-architecture.md`：系统总览。
- `architecture/im-core-sdk-architecture.md`：SDK 架构、CLI/SDK 边界、模块职责和安全/路径约束。
- `architecture/identity-secret-storage.md`：identity SecretVault 当前方案、CLI/App/daemon root key source、私钥/JWT/E2EE local secret 加密持久化边界和剩余风险。
- `architecture/awiki-command-v2.md`：CLI 命令契约。
- `architecture/output-format.md`：输出 envelope、dry-run、错误码和格式化规则。
- `architecture/local-state-upgrade.md`：工作区、本地状态和历史数据升级策略。
- `architecture/anp-service-discovery.md`：DID document 中 ANP message service 的生成规则。
- `architecture/direct-e2ee-operations.md`、`architecture/group-e2ee-operations.md`：secure direct / group E2EE 的产品边界。
- `architecture/hermes-host-notify-v1.md`、`architecture/hermes-host-notify-v1-runbook.md`、`architecture/hermes-host-notify-implementation-notes.md`：host notification 与 Hermes 集成。
- `architecture/openclaw-host-adapter-v1.md`、`architecture/websocket-host-notification-v1.md`：OpenClaw / WebSocket host notification。
- `architecture/contracts/*`：JSON Schema / OpenAPI 契约，不属于本次整理修改范围。

### API 文档

- `api/im-core-public-api.md`：`ImCore`、`ImClient` 和各 service 的 API 总览。
- `api/im-core-interface/*`：接口级规格和历史可实施接口清单。
- `architecture/contracts/*`：JSON Schema / OpenAPI 契约。
- `harness/legacy-path-baseline.md`：测试引用的 legacy path 基线，不要删除；更新路径时需同步测试。

### 功能与使用说明

- `installation.md`：安装、配置、工作区、runtime 和常见问题。
- `flutter-sdk/awiki-im-core-flutter-sdk.md`：Flutter/Dart SDK 使用与边界。
- `flutter-sdk/awiki-me-future-integration.md`：未来接入 `awiki-me` 的建议。
- `node-sdk/dsh-device-join-extension.md`：DSH 新设备加入与 ready-admin 设备管理所需的 Node
  facade 增量合同；当前状态为 planned，不代表 API 或 npm 制品已发布。
- `architecture/awiki-mail-cli.md`、`architecture/awiki-site-pages.md`、`architecture/awiki-skill-architecture.md`：邮件、站点页和 skill 文档。

### 维护说明

- `harness/review-spec.md`：当前 review 检查表。
- `file-size-exceptions.md`：Rust 文件大小例外，供 `xtask check-structure` 使用。
- `plan/*`：仅保留少量被 CLI 内置 docs 引用的历史入口，内容已归档并指向稳定文档。

## 清理策略

以下类型不再放在 `docs/` 主树中维护：一次性迁移计划、Codex goal、PR closeout、逐次 system-test 验证流水、Go parity 大表、临时 dependency decision 记录和过期草案。需要追溯时使用 Git 历史；当前开发以本索引列出的稳定文档为准。
