# awiki-cli Review 规范

## 1. 文档定位

本文是当前仓库的 review 检查表。它用于快速判断 PR 是否符合 SDK 重构后的架构边界、命令契约、输出契约、安全规则和文档维护要求。

使用原则：

- 先按本文做一级筛查。
- 细节不清楚时，回读本文列出的一级源文档。
- API 文档和外部服务契约优先于实现猜测；不要在 review 中凭空发明字段或协议。

## 2. 裁决优先级

当文档或实现出现冲突时，按以下顺序裁决：

1. 外部服务 API 契约：`../user-service/docs/api/`、`../message-service/docs/api/`、ANP SDK 文档。
2. SDK public/API 文档：`docs/api/im-core-public-api.md`、`docs/api/im-core-interface/*`。
3. 当前架构文档：`docs/architecture/awiki-v2-architecture.md`、`docs/architecture/im-core-sdk-architecture.md`。
4. CLI 契约文档：`docs/architecture/awiki-command-v2.md`、`docs/architecture/output-format.md`。
5. 功能文档：`docs/architecture/*.md`、`docs/flutter-sdk/*.md`、`docs/installation.md`。
6. 历史 plan/phase 文档仅作追溯，不作为当前事实来源。

## 3. 必读源文档

| 主题 | 文档 |
| --- | --- |
| 文档入口 | `docs/README.md` |
| 系统总架构 | `docs/architecture/awiki-v2-architecture.md` |
| SDK 架构 | `docs/architecture/im-core-sdk-architecture.md` |
| CLI/SDK 边界 | `docs/architecture/im-core-sdk-architecture.md` |
| SDK API | `docs/api/im-core-public-api.md`、`docs/api/im-core-interface/*` |
| CLI 命令面 | `docs/architecture/awiki-command-v2.md` |
| 输出协议 | `docs/architecture/output-format.md` |
| 安装与工作区 | `docs/installation.md` |
| 本地状态升级 | `docs/architecture/local-state-upgrade.md` |
| Skill 架构 | `docs/architecture/awiki-skill-architecture.md` |
| Flutter SDK | `docs/flutter-sdk/awiki-im-core-flutter-sdk.md` |
| 发布 | `docs/publish.md` |

## 4. Review 主检查项

### 4.1 SDK 边界

必须检查：

- [ ] 业务流程是否落在 `im-core`，CLI 是否只做 parse/build/call/render。
- [ ] `im-core` 是否没有引用 `ParsedCommand`、`GlobalOptions`、`ExitError`、CLI config resolver、CLI workspace resolver。
- [ ] SDK public API 是否表达业务意图，而不是 raw RPC、wire DTO、SQLite row、WebSocket frame 或 crypto artifact。
- [ ] Flutter/Dart facade 是否只暴露 SDK 语义，没有引入 app UI/cache DTO。

回读：

- `docs/architecture/im-core-sdk-architecture.md`
- `docs/api/im-core-public-api.md`
- `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`

### 4.2 命令面与领域归属

必须检查：

- [ ] 是否保持当前顶级命令面：`status/docs/schema/doctor/version/init/completion/config/id/msg/mail/group/runtime/people/page/site/debug`。
- [ ] `group` 是否仍是独立领域，群发消息是否仍通过 `msg send --group` 表达。
- [ ] `page` 与 `site` 是否保持分离：handle page 不等于 tenant site page。
- [ ] 是否新增了低层 protocol/debug 命令作为默认公共产品面。

回读：

- `docs/architecture/awiki-command-v2.md`
- `docs/architecture/awiki-site-pages.md`
- `docs/architecture/im-core-sdk-architecture.md`

### 4.3 输出、dry-run 与 schema

必须检查：

- [ ] canonical output 是否仍是 JSON envelope。
- [ ] 是否使用 `ok`、`data` / `error`、`_notice`、`meta`。
- [ ] 是否保留 `--format`、`--jq`、`--dry-run` 的契约。
- [ ] 副作用命令是否提供 dry-run plan。
- [ ] pretty/table/ndjson 是否只是 JSON 的渲染视图。

回读：

- `docs/architecture/output-format.md`
- `docs/architecture/awiki-command-v2.md`

### 4.4 Identity、Config、Path

必须检查：

- [ ] 用户层术语是否使用 identity，legacy credential 只作为兼容存储概念。
- [ ] 是否只通过 `AWIKI_CLI_WORKSPACE_HOME_DIR` 切换工作区根目录。
- [ ] 配置优先级是否仍是 `flag > config.yaml > default`。
- [ ] 私钥、auth/session、SQLite、runtime、MLS state 是否在明确路径下按身份隔离。
- [ ] workspace 或旧身份导入是否避免上传/输出敏感备份。

回读：

- `docs/installation.md`
- `docs/architecture/local-state-upgrade.md`
- `docs/architecture/im-core-sdk-architecture.md`

### 4.5 服务 API 与协议契约

必须检查：

- [ ] DID auth、handle、profile、relationship、group、content 的字段和鉴权是否符合外部 API 文档。
- [ ] message-service direct/group/attachment 请求是否保持 hop auth、origin proof 和 client-local 字段边界。
- [ ] 附件对象字节是否只走附件数据面，不塞进 `direct.send`、`group.send` 或 WSS frame。
- [ ] DID document 的 `ANPMessageService` 是否只公开批准的 endpoint/profile/security profile。

回读：

- `../user-service/docs/api/`
- `../message-service/docs/api/`
- `docs/architecture/anp-service-discovery.md`

### 4.6 Runtime 与 Host Notification

必须检查：

- [ ] `http` / `websocket` runtime mode 是否清晰，不把 transport 参数塞进普通业务命令。
- [ ] listener/service lifecycle 是否仍归 CLI，而不是 SDK public API。
- [ ] realtime runner 状态机是否归 `im-core`，CLI 不重复实现 IM 事件投影。
- [ ] OpenClaw/Hermes 是否只作为 host notification UX，不污染 SDK 默认 API。

回读：

- `docs/architecture/awiki-v2-architecture.md`
- `docs/architecture/im-core-sdk-architecture.md`
- `docs/architecture/websocket-host-notification-v1.md`
- `docs/architecture/openclaw-host-adapter-v1.md`
- `docs/architecture/hermes-host-notify-v1.md`

### 4.7 Secure / E2EE

必须检查：

- [ ] CLI 是否只表达 `--secure required`、status、repair 等高层意图。
- [ ] raw ciphertext、prekey、KeyPackage、MLS private state、provider stdout/stderr 是否没有进入普通输出。
- [ ] Group E2EE 是否通过 `im-core` public services 和 native provider 路径执行。
- [ ] public discovery 是否没有自动宣传未批准的 secure capability。

回读：

- `docs/architecture/direct-e2ee-operations.md`
- `docs/architecture/group-e2ee-operations.md`
- `docs/architecture/im-core-sdk-architecture.md`

### 4.8 安全与信息隔离

必须检查：

- [ ] 远端消息是否始终当作不可信数据。
- [ ] 是否有任何路径会把消息当作本地指令执行。
- [ ] 日志、错误、doctor、trace 是否泄露 JWT、私钥、E2EE key、raw secure state 或 host 敏感信息。
- [ ] host notification payload 是否只包含批准的事件摘要。

回读：

- `docs/architecture/awiki-v2-architecture.md`
- `docs/architecture/output-format.md`
- `docs/architecture/websocket-host-notification-v1.md`

### 4.9 文档漂移

必须检查：

- [ ] 改了命令面是否同步 `awiki-command-v2.md`、skill reference 或内置 docs 入口。
- [ ] 改了 SDK 边界或模块职责是否同步 `architecture/im-core-sdk-architecture.md`。
- [ ] 改了 Flutter facade 是否同步 `flutter-sdk/awiki-im-core-flutter-sdk.md`。
- [ ] 是否误改了 API 文档；无 API 变更时不要修改 `api/im-core-public-api.md`、`api/im-core-interface/*`、`architecture/contracts/*`。
- [ ] 是否新增一次性计划/验证流水到 `docs/` 主树；这类内容应使用 issue/PR 或 Git 历史，不再长期维护。

回读：

- `docs/README.md`
- `docs/architecture/im-core-sdk-architecture.md`
- `docs/architecture/awiki-skill-architecture.md`

## 5. 结论模板

```text
Review Summary
- Scope:
- Result: pass | changes-requested | needs-confirmation

Key Findings
1. [category] ...

Constraint Check
- SDK boundary:
- Command surface:
- Output contract:
- Identity/config/path:
- Service/API mapping:
- Runtime/secure:
- Security:
- Docs drift:

Primary References
- ...
```

常用分类：

- `sdk-boundary`
- `command-surface`
- `output-contract`
- `identity-config`
- `service-api`
- `runtime`
- `secure`
- `security`
- `docs-drift`

## 6. 一句话原则

awiki-cli 的 review 目标不是只看代码能不能跑，而是确认它是否继续忠实于当前 SDK 边界、结构化输出、多身份隔离、显式 runtime、安全前置、服务 API 契约和稳定文档入口。
