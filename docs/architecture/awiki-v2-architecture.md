# awiki v2 系统架构

## 1. 定位

awiki v2 当前形态是一个 Rust workspace，核心由可复用 SDK 与 CLI 产品壳组成：

```text
crates/im-core       # IM SDK / 产品能力层
crates/awiki-cli     # CLI thin shell
crates/im-core-dart  # Rust-Dart facade
packages/awiki_im_core
                    # Flutter/Dart SDK package
skills/              # Agent skill 入口与 reference 文档
docs/                # 稳定架构、功能、说明和契约文档
```

系统目标是把 DID/Handle 身份、消息、群组、附件、邮件、页面、realtime、secure 和本地状态收敛成可复用产品能力，同时保留 `awiki-cli` 作为人类和 Agent 都能稳定调用的统一入口。

## 2. 核心原则

- **SDK 承载业务**：identity、auth、message、group、attachment、secure、realtime、email、content/site 的业务流程归 `im-core`。
- **CLI 保持薄壳**：命令解析、配置、路径、权限、dry-run、输出、exit code、service 管理和 host notification UX 归 `awiki-cli`。
- **结构化优先**：命令输出以 JSON envelope 为 canonical contract，pretty/table/ndjson 是渲染视图。
- **多身份一等公民**：`ImCore` 是环境入口，`ImClient` 绑定单个身份，业务操作自动使用该身份的 auth、本地 owner 和 secure state。
- **显式 runtime**：`http` / `websocket` 模式、listener、IPC、host notification 和 service 生命周期都必须可配置、可诊断。
- **安全前置**：远端消息永远是不可信数据；私钥、JWT、secure material、host 信息和本地数据库内容不得泄露。
- **文档即产品**：命令契约、SDK 边界、输出协议、安装说明、skill reference 和 review 规范需要保持一致。

## 3. 逻辑分层

```text
+----------------------------------------------------+
| Product hosts                                      |
| awiki-cli / Flutter app / Agent skill              |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| Product adapters                                   |
| CLI parser+renderer / Dart facade / platform loader|
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core public API                                 |
| ImCore, ImClient, identity/auth/messages/...       |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| im-core orchestration                              |
| auth retry, target resolve, projection, secure,    |
| realtime event normalization, local owner binding  |
+---------------------------+------------------------+
                            |
                            v
+----------------------------------------------------+
| Internal implementation                            |
| HTTP/RPC, DID proof, SQLite, WebSocket, MLS state, |
| compatibility helpers and wire DTOs                |
+----------------------------------------------------+
```

`im-core` 不依赖 `awiki-cli`，也不读取 CLI config 或自动发现 CLI workspace。调用方负责传入 `ImCoreConfig` 和 `ImCorePaths`。

## 4. 产品能力域

### Identity / Auth

身份域负责 DID/Handle、多身份 registry、注册、绑定、恢复、replace DID、profile、session login/ensure/refresh/status。私钥和 token 文件是 internal state，不能通过普通输出暴露。

### Messaging

消息域覆盖 direct/group 发送、inbox、history、mark-read、conversation、本地缓存投影和 secure policy。CLI 只把 `--to`、`--group`、`--text`、`--file` 等输入转换成 SDK DTO。

统一消息模型：

```text
Message =
  Target(direct | group)
  x Body(text | attachment | future content)
  x Security(default | plaintext | e2ee-required)
  x ReceiveMode(pull | realtime)
```

`ReceiveMode` 属于 runtime，不应泄漏到业务命令语义中。

### Groups

群组域负责 create/join/leave/list/get/members/messages/profile/policy 以及 group secure state。向群发送消息仍通过 message service 的 `MessageTarget::Group` 表达。

### Attachments

附件域负责读取输入、digest、manifest、上传、commit、消息发送、ticket 获取和下载。CLI 负责本地路径校验、覆盖策略和文件权限；SDK public API 表达附件来源和目的地，不表达 CLI flag。

### Secure

Secure 域负责 direct E2EE、group E2EE、secure status/repair、secure outbox summary 和 secure send 编排。raw ciphertext、prekey、KeyPackage、MLS 私有状态、provider stdout/stderr 不进入普通 CLI 输出或 SDK public result。

### Realtime / Runtime

`im-core` 提供可嵌入 realtime session/runner/event stream。`awiki-cli` 负责把它进程化为 listener service，并管理 systemd、launchd、Windows service、Unix socket / named pipe、pid/status/log。

### Email / Content / Site

Email、handle content page 和 tenant site page 都作为高层产品能力进入 SDK。CLI 保留命令 UX、dry-run、输出和附件落盘，不直接构造 raw RPC payload。

## 5. CLI 产品面

当前顶层命令面包括：

```text
awiki-cli status
awiki-cli docs
awiki-cli schema
awiki-cli doctor
awiki-cli version
awiki-cli completion
awiki-cli config
awiki-cli id
awiki-cli msg
awiki-cli mail
awiki-cli group
awiki-cli people
awiki-cli page
awiki-cli site
awiki-cli runtime
awiki-cli debug
```

详细命令设计见 `docs/architecture/awiki-command-v2.md`，输出契约见 `docs/architecture/output-format.md`。

## 6. 本地状态与工作区

默认工作区为 `~/.awiki-cli/`，只支持 `AWIKI_CLI_WORKSPACE_HOME_DIR` 切换整个根目录。

```text
config.yaml
identities/
data/awiki-cli.db
cache/
runtime/
mls/
logs/
```

本地状态分为：

- identity store：DID document、私钥、identity metadata、auth/session。
- SQLite business store：contacts、messages、groups、relationship events、secure outbox 等缓存。
- runtime state：listener socket、pid/status、host notification 事件、服务状态。
- MLS / secure state：identity/device scoped private state，不能混入普通输出。

工作区升级和历史导入说明见 `docs/architecture/local-state-upgrade.md` 与 `docs/installation.md`。

## 7. Skill 与文档

Skill 体系采用单入口 + reference 模型：

```text
skills/SKILL.md
skills/references/*.md
```

`SKILL.md` 负责默认路由、安全规则和常用工作流；领域细节按需读取 reference。Skill 是 Agent 使用增强层，不是 CLI 可用性的前置条件。详细设计见 `docs/architecture/awiki-skill-architecture.md`。

`docs/README.md` 是当前文档入口。已完成的迁移计划、PR closeout、逐次验证记录和 parity 流水不再作为当前事实维护。

## 8. Host Integration

Host notification 是 runtime 的宿主集成层：

- `log` / `file`：本地记录。
- `openclaw`：转发到 OpenClaw loopback webhook。
- `hermes`：转发到 Hermes adapter，再由 Hermes 投递到 Feishu、Telegram 等平台。

相关文档：

- `docs/architecture/websocket-host-notification-v1.md`
- `docs/architecture/openclaw-host-adapter-v1.md`
- `docs/architecture/hermes-host-notify-v1.md`
- `docs/architecture/hermes-host-notify-v1-runbook.md`

## 9. 发布与分发

发布模型：

- Rust release artifact：多平台原生二进制。
- awiki daemon artifact：客户端安装/升级使用的 `awiki-deamon-<os>-<arch>.tar.gz`。
- release scripts：`scripts/release/*` 负责版本一致性、本地/服务器构建、daemon manifest 和文件服务目录 staging。

发布手册见 `docs/publish.md`。

## 10. Review 标准

一个改动只有在同时满足以下条件时才算符合当前架构：

1. 业务流程落在 `im-core`，CLI 只做产品壳适配。
2. `im-core` 不引用 CLI 类型、CLI config resolver 或 workspace resolver。
3. CLI 输出遵守 `output-format.md` 的 JSON envelope 与 exit code 策略。
4. 多身份、本地 owner、auth/session 和 secure state 按身份隔离。
5. runtime/service/host notification 不泄漏到底层业务 API。
6. API 文档和契约文件没有被无关改动漂移。
7. 文档、help、schema、skill reference 没有产生新的事实冲突。

Review 细则见 `docs/harness/review-spec.md`。
