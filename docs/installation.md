# awiki-cli 安装说明

## 概述

awiki-cli 是 awiki 的命令行客户端。当前仓库是 Awiki CLI 合约的 Rust CLI port，通过 CLI 命令编排对后端服务的 API 调用，并保留早期 Go 设计中的命令面、输出契约和发布产物命名。它支持 DID 身份管理、消息收发（私聊/群聊）、群组管理、WebSocket 实时监听等能力。

**技术栈**: Rust 1.78+ workspace + Cargo + bundled SQLite (`rusqlite`) + ANP Rust SDK

---

## 1. 编译工具

### 1.1 Rust toolchain

`Cargo.toml` 中的最低 Rust 版本为 1.78。发布脚本默认使用 `AWIKI_CLI_RUST_TOOLCHAIN=1.79.0`，如本机已安装兼容 toolchain，也可以通过环境变量覆盖。

```bash
# macOS (Homebrew)
brew install rustup-init
rustup-init

# 或从官网下载
# https://www.rust-lang.org/tools/install

# 验证版本
rustc --version
cargo --version
```

### 1.2 ANP Rust SDK（P5 secure direct）

awiki-cli 的 ANP 依赖来自同级 Rust workspace 路径 `../anp/rust`。拉取或构建本仓库时，需要保证该 sibling repository 可用：

```bash
ls ../anp/rust/Cargo.toml
```

P5 secure direct / OPK 客户端能力由 Rust SDK 依赖提供。首次拉取依赖时请确保本机可以访问 crates.io 和对应源码仓库。

Direct E2EE 的 CLI 编排、session/prekey/outbox 本地状态和 discovery 收口见 [`docs/architecture/direct-e2ee-operations.md`](architecture/direct-e2ee-operations.md)。

### 1.3 Docker 备选（无本地 Rust 时）

如本机未安装 Rust，可使用 Rust Docker 镜像：

```bash
docker run --rm -v "$PWD":/app -w /app rust:1.79 cargo build --workspace --locked
docker run --rm -v "$PWD":/app -w /app rust:1.79 cargo test --workspace --all-features
```

---

## 2. 数据库

awiki-cli 使用 bundled SQLite 作为本地存储，无需安装外部数据库。

### 2.1 自动初始化

数据库文件在首次运行时自动创建和初始化（`EnsureSchema`），位于工作区数据目录下：

```
~/.awiki-cli/data/awiki-cli.db
```

Schema 版本为 v11，包含以下本地表：

| 表名 | 用途 |
|------|------|
| `contacts` | 联系人（owner_did 分区） |
| `messages` | 消息本地缓存（私聊 + 群聊） |
| `e2ee_outbox` | E2EE 加密消息发件箱 |
| `groups` | 群组本地缓存 |
| `group_members` | 群成员本地缓存 |
| `relationship_events` | 关系事件记录 |
| `e2ee_sessions` | E2EE 会话密钥状态 |

视图：`threads`（会话列表）、`inbox`（收件箱）、`outbox`（发件箱）

### 2.2 SQLite 配置

自动设置以下 PRAGMA：

| PRAGMA | 值 | 说明 |
|--------|-----|------|
| `journal_mode` | WAL | 写前日志，支持并发读 |
| `foreign_keys` | ON | 外键约束 |
| `busy_timeout` | 5000ms | 锁等待超时 |

### 2.3 数据库路径覆盖

推荐优先使用工作区根目录覆盖：

```bash
awiki-cli init
```

如需显式切换工作区根目录，只支持：

```bash
export AWIKI_CLI_WORKSPACE_HOME_DIR=~/my-awiki
# 数据库将位于 ~/my-awiki/data/awiki-cli.db
```

`config / data / runtime / cache / logs / identities` 都会固定派生在该工作区下，不再支持单独的目录级环境变量覆盖。

---

## 3. 配置文件

### 3.1 工作区目录布局

awiki-cli 默认采用单根目录工作区模型，默认路径如下：

| 用途 | 默认路径 | 环境变量覆盖 |
|------|----------|-------------|
| 工作区目录 | `~/.awiki-cli/` | `AWIKI_CLI_WORKSPACE_HOME_DIR` |
| 配置目录 | `~/.awiki-cli/` | 无 |
| 数据目录 | `~/.awiki-cli/data/` | 无 |
| runtime 目录 | `~/.awiki-cli/runtime/` | 无 |
| 缓存目录 | `~/.awiki-cli/cache/` | 无 |
| 日志目录 | `~/.awiki-cli/logs/` | 无 |
| MLS 状态目录（Group E2EE / `im-core` native provider） | `~/.awiki-cli/mls/` | 无 |

> 说明：`~/.awiki-cli/` 是跨平台固定的工作区目录（Windows 对应 `%USERPROFILE%\.awiki-cli\`），也是默认唯一入口。
> `AWIKI_CLI_WORKSPACE_HOME_DIR` 只负责切换整个工作区根目录；`config / data / runtime / cache` 不再允许分别配置。
> `AWIKI_CLI_WORKSPACE_HOME_DIR` 之外的旧 `AWIKI_* / AVIKI_* / E2E_*` 业务环境变量不再驱动 awiki-cli；若工作区仍保留上一版的 `config.json`，CLI 会在首次访问工作区时自动迁移到 `config.yaml`。
>
> 工作区内容包括：
>
> - `config.yaml`
> - `identities/`
> - `data/awiki-cli.db`
> - `cache/`
> - `runtime/`
> - `mls/`（由 `im-core` group E2EE native provider 使用；MLS 私有状态不写入主业务 SQLite）
> - `logs/`
> - workspace upgrade 元数据
> - upgrade lock / journal
> - 备份快照


### 3.3 Group E2EE release staging

Supported Group E2EE is now part of the `im-core` public API path. `awiki-cli` only parses user intent such as `--secure required` and calls `client.messages()`, `client.groups()`, or `client.secure().group()`. MLS state, KeyPackage handling, notice processing, and repair are owned by `im-core`; the default supported product path does not invoke an `anp-mls` process and does not use `AWIKI_ANP_MLS_BINARY`.

For Linux/macOS release artifacts, the build must include:

```text
awiki-cli -> im-core feature "group-e2ee" -> anp feature "mls"
```

The release artifact script checks this feature graph before building Linux/macOS archives. Windows E2EE package/release validation is explicitly deferred for this stage; Windows artifacts may still be built, but Windows E2EE package validation is not a blocker for Linux/macOS rollout.

The MLS private state root remains `~/.awiki-cli/mls/` (or the current `AWIKI_CLI_WORKSPACE_HOME_DIR` equivalent). Runtime OpenMLS state is agent/device-scoped under that root (`mls/agents/<agent-hash>/<device>/state.db`) so two local identities do not share private KeyPackage storage. User-facing recovery should use `group secure status` and `group secure repair`; low-level `group e2ee *` commands are hidden/internal or stable unsupported and are not part of the default schema/help/completion surface.

### 3.2 config.yaml

配置文件位于 `~/.awiki-cli/config.yaml`。推荐先执行 `awiki-cli init` 自动创建最小配置；如需手动创建，可参考仓库根目录的 `config.template.yaml`，或直接使用下面的模板：

```yaml
schema_version: 1
identity:
  active: default
runtime:
  mode: websocket
  socket_path: ""
  listener:
    enabled: true
    auto_install: true
    auto_start: true
  host_notify:
    enabled: true
    sink: log
    file_path: ""
    openclaw:
      hook_url: ""
      token: ""
    hermes:
      notify_url: http://127.0.0.1:8765/notify/host-event
      secret: ""
output:
  format: json
  no_color: false
services:
  service_base_url: https://awiki.ai
  did_domain: awiki.ai
  anp_service_endpoint: https://awiki.ai/anp-im/rpc
  anp_service_did: did:wba:awiki.ai
  ca_bundle: ""
```

默认值说明：

- `runtime.mode` 默认是 `websocket`
- `runtime.socket_path` 默认是：
  - macOS / Linux: `<workspace>/runtime/message-daemon.sock`
  - Windows: `\\\\.\\pipe\\awiki-cli-<workspace-hash>`
- `runtime.listener.enabled` 默认是 `true`
- `runtime.listener.auto_install` 默认是 `true`
- `runtime.listener.auto_start` 默认是 `true`
- 在默认 websocket 模式下，`awiki-cli init` 和 `awiki-cli runtime setup` 会自动安装并启动 listener 系统服务
- `runtime.host_notify.enabled` 默认是 `true`
- `runtime.host_notify.sink` 在启用后默认是 `log`，可选 `noop | log | file | openclaw | hermes`（兼容旧值 `webhook`）
- `runtime.host_notify.file_path` 只在 `sink = file` 时生效；未填写时默认是 `<workspace>/runtime/host-notify.events.jsonl`
- `runtime.host_notify.openclaw.hook_url` 通常不需要手工填写；awiki-cli 会优先读取 `~/.openclaw/openclaw.json` 中的 `gateway.port` 和 `hooks.path` 自动推导有效的 webhook URL
- `runtime.host_notify.openclaw.token` 可直接写入 `config.yaml`，也可通过 `OPENCLAW_HOOK_TOKEN` 环境变量提供；两者都未设置时，awiki-cli 会回退读取 `~/.openclaw/openclaw.json` 中的 `hooks.token`
- `runtime.host_notify.hermes.notify_url` 默认是 `http://127.0.0.1:8765/notify/host-event`
- `runtime.host_notify.hermes.secret` 可直接写入 `config.yaml`，也可通过 `AWIKI_HOST_NOTIFY_HERMES_SECRET` 环境变量提供（兼容旧变量 `AWIKI_HOST_NOTIFY_WEBHOOK_SECRET`）
- 当 `runtime.host_notify.sink = hermes` 时，awiki-cli 只负责把通知转发给 Hermes adapter；最终投递目标由 Hermes 自己配置，不在 awiki-cli 中管理
- 如果 Hermes 最终要投递到 Feishu，推荐在 Hermes 中使用 `FEISHU_HOME_CHANNEL` 或 `/sethome` / `/set-home` 管理默认会话，而不是在 route 里硬编码 `deliver_extra.chat_id`
- `output.format` 默认是 `json`
- `services.service_base_url` 默认是 `https://awiki.ai`
- `services.did_domain` 默认是 `awiki.ai`
- `services.anp_service_endpoint` 默认从 `service_base_url` 推导为 `<service_base_url>/anp-im/rpc`
- `services.anp_service_did` 默认从 `service_base_url` 的 hostname 推导为 `did:wba:<service_base_url-host>`

配置优先级固定为：

```text
flag > config.yaml > default
```

> 该文件可选。未创建时所有配置使用默认值。
> `anp_service_endpoint` 和 `anp_service_did` 用于生成本地 DID 文档中的 `ANPMessageService`，同时 `anp_service_did` 也是 group/attachment 控制面默认使用的 service DID。它们和 `service_base_url` 的职责不同：
>
> - `service_base_url`：CLI 连接 user-service / content / group / message 的统一平台基础地址
> - 域内 message RPC：`<service_base_url>/im/rpc`
> - 域内 message WebSocket：`<service_base_url>/im/ws`
> - `did_domain`：生成 bare-handle DID 的 provider domain；同时，CLI 在所有支持 handle 输入的 id/msg/group 入口里，如果用户只输入 bare handle（如 `alice`），都会先补全成 `alice.<did_domain>` 再做 lookup / register / recover。若用户显式输入 full handle（如 `alice.example.com`），则该次命令以显式 domain 为准，不会被 `did_domain` 覆盖；多租户身份可与 `service_base_url` 不同
> - `anp_service_endpoint`：对外公开到 DID 文档里的 RPC 地址，默认从 `service_base_url` 推导
> - `anp_service_did`：对外公开到 DID 文档里的 bare-domain service DID，默认从 `service_base_url` 推导
>
> 多租户示例：`service_base_url=https://awiki.ai`、`did_domain=a.com` 时，CLI 连接 awiki.ai 后端，但生成的 DID 使用 `a.com`。
>
> - `awiki-cli msg send --to alice --text "hi"` 会先把目标补成 `alice.a.com`
> - `awiki-cli id recover --handle alice` 会按 `alice.a.com` 生成新 DID，并向服务端提交该 canonical full handle
> - `awiki-cli id register --handle alice.partner.com` 会按 `partner.com` 生成 DID，但仍只把 local-part `alice` 发给 `did-auth.register`

### 3.3 本地开发配置

连接本地后端服务时，创建如下 `config.yaml`：

```yaml
schema_version: 1
identity:
  active: default
runtime:
  mode: websocket
  listener:
    enabled: true
    auto_install: true
    auto_start: true
  host_notify:
    enabled: true
    sink: log
    openclaw:
      hook_url: ""
    hermes:
      notify_url: http://127.0.0.1:8765/notify/host-event
services:
  service_base_url: https://xxx.xxx
  did_domain: xxx.xxx
  anp_service_endpoint: https://xxx.xxx/anp-im/rpc
  anp_service_did: did:wba:xxx.xxx
  ca_bundle: ""
```

服务地址、运行模式、输出格式、身份默认值都应通过 `config.yaml` 管理；除了 `AWIKI_CLI_WORKSPACE_HOME_DIR` 以外，不再支持环境变量覆盖这些业务配置。

### 3.4 DID 文档中的 ANP Service 约束

`awiki-cli` 在生成 DID 文档时，会自动写入一个公开的 `ANPMessageService` 条目。为了避免把本地实现细节暴露到 DID 文档里，当前实现会拒绝以下配置：

- `localhost`
- `127.0.0.1` / `::1` 等 loopback 地址
- `ws://` / `wss://` URL
- 带 fragment 的 `serviceDid`
- 非 bare-domain 的 `did:wba` service DID（例如 `did:wba:example.com:services:message:e1_local`）

推荐做法：

- `anp_service_endpoint` 使用公开 HTTPS RPC 地址，例如 `https://awiki.ai/anp-im/rpc`
- `anp_service_did` 使用裸域名 DID，例如 `did:wba:awiki.ai`

### 3.5 身份文件布局

DID 身份存储在 `~/.awiki-cli/identities/` 下，每个身份一个子目录：

```
identities/
├── index.json                    # 身份索引（默认身份、凭证列表）
└── <identity-dir>/
    ├── identity.json             # 身份元数据
    ├── auth.json                 # JWT token 缓存
    ├── did_document.json         # DID 文档
    ├── key-1-private.pem         # Ed25519 身份私钥
    ├── key-1-public.pem          # Ed25519 身份公钥
    ├── e2ee-signing-private.pem  # E2EE 签名私钥
    ├── e2ee-agreement-private.pem # E2EE 密钥协商私钥
    └── e2ee-state.json           # E2EE 会话状态
```

> 私钥文件权限为 `0600`，目录权限为 `0700`。

当前 `awiki-cli` 的活跃身份规范为 `e1` / Ed25519 `key-1`。当你把 Python v1 `awiki-agent-id-message` 本地数据默认升级到 Rust CLI port workspace 时，CLI 会自动尝试把已导入的 handle `k1` DID 通过 `replace_did` 换绑为新的 `e1` DID，并同步重绑本地 SQLite 的 `owner_did`。替换前，旧 DID document、旧私钥和旧 identity 目录会备份到 `identities/.legacy-backup/replace-did/`；这些备份仍包含敏感密钥材料，不要上传或分享。若个别身份无法自动替换，升级会继续完成，但会把失败原因记录到 upgrade warning 与 `doctor` 输出中，后续需要手动处理。

同一 handle 在本地联系人缓存中若经历 DID 切换，`awiki-cli` 会保留对应的历史 DID 映射，并在按 handle 读取 direct inbox/history 时聚合这些历史 DID 关联的消息；如需排查本地记录，可使用 `awiki-cli debug db handle-history <handle>` 查看。

同一轮默认升级还会对旧 `awiki-agent-id-message` skill 做 best-effort 清理：停止并卸载旧 listener service，删除旧 skill 安装目录，并移除旧 OpenClaw `HEARTBEAT.md` 中引用 legacy skill 的 awiki section，避免新旧 skill 同时生效。

### 3.6 环境变量完整列表

| 环境变量 | 用途 | 默认值 |
|----------|------|--------|
| `AWIKI_CLI_WORKSPACE_HOME_DIR` | 工作区根目录 | `~/.awiki-cli` |

> 除 `AWIKI_CLI_WORKSPACE_HOME_DIR` 外，其他 awiki-cli 配置环境变量已停止支持；它们不会再覆盖 `config.yaml` 中的业务配置。

---

## 4. 编译与运行

### 4.1 安装依赖

```bash
cd awiki-cli-rs2
cargo fetch --locked
```

### 4.2 编译

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
cp target/release/awiki-cli ./awiki-cli
```

Linux/macOS release artifacts should be built through the release script so the E2EE feature graph check runs:

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
scripts/release/build-release-artifact.sh --os darwin --arch arm64
```

### 4.3 验证

```bash
# 版本信息
./awiki-cli version

# 系统诊断（检查配置、身份、数据库、运行环境）
./awiki-cli doctor

# 查看某个 handle 在本地记录过哪些历史 DID
./awiki-cli debug db handle-history alice

# 查看当前配置
./awiki-cli config show

# 更新 did_domain（修改后不需要重启 listener）
./awiki-cli config set --did-domain tenant.example
```

### 4.4 运行测试

```bash
cargo test --workspace --all-features
```

### 4.5 代码格式化

```bash
cargo fmt --all
```

---

## 5. 快速上手

### 5.1 创建身份

```bash
# 创建本地 DID 身份
./awiki-cli id create --name my-identity

# 查看身份列表
./awiki-cli id list

# 查看当前身份
./awiki-cli id current
```

### 5.2 注册 Handle

```bash
# 注册 handle（需要后端服务可用）
./awiki-cli id register --handle myname
```

### 5.3 发送消息

```bash
# 私聊
./awiki-cli msg send --to <handle> --text "hello"

# 群聊
./awiki-cli msg send --group <group-id> --text "hello"

# 发送附件（caption 可选）
./awiki-cli msg send --to <handle> --file ./hello.txt --text "hello attachment"

# 下载附件
./awiki-cli msg attachment download --with <handle> --message-id <msg-id> --output ./downloads/hello.txt

# 查看收件箱
./awiki-cli msg inbox
```

### 5.4 WebSocket 模式

```bash
# 初始化工作区，并自动安装/启动 listener 系统服务
./awiki-cli init

# 显式执行 runtime bootstrap（也会按配置自动 install/start）
./awiki-cli runtime setup --mode websocket

# 按当前 config.yaml 重新收敛 runtime / listener 真实状态
./awiki-cli runtime apply

# 查看监听器状态
./awiki-cli runtime listener status

# 可选：只安装服务定义，不自动启动
./awiki-cli runtime listener install

# 启动 listener 服务；若服务尚未安装，会自动补 install
./awiki-cli runtime listener start

# 停止 / 重启 / 卸载
./awiki-cli runtime listener stop
./awiki-cli runtime listener restart
./awiki-cli runtime listener uninstall

# 查看 / 修改 listener 配置
./awiki-cli runtime listener config show
./awiki-cli runtime listener config set --enabled false
./awiki-cli runtime listener config set --auto-install false --auto-start false

# 高阶快捷开关：改配置后自动 apply
./awiki-cli runtime listener enable
./awiki-cli runtime listener disable

# 查看 / 修改 host notify 配置
./awiki-cli runtime host-notify config show
./awiki-cli runtime host-notify enable
./awiki-cli runtime host-notify disable
./awiki-cli runtime host-notify config set --sink openclaw
./awiki-cli runtime host-notify openclaw set --hook-url http://127.0.0.1:18789/hooks/agent
./awiki-cli runtime host-notify openclaw set-token --value <token>
./awiki-cli runtime host-notify openclaw clear-token
./awiki-cli runtime host-notify openclaw route add --session-key <session-key>
./awiki-cli runtime host-notify openclaw route list
./awiki-cli runtime host-notify openclaw route remove --session-key <session-key>
./awiki-cli runtime host-notify config set --sink hermes
./awiki-cli runtime host-notify hermes guide
./awiki-cli runtime host-notify hermes setup
./awiki-cli runtime host-notify hermes status
./awiki-cli runtime host-notify hermes set --notify-url http://127.0.0.1:8765/notify/host-event
./awiki-cli runtime host-notify hermes set-secret --value <secret>
./awiki-cli runtime host-notify hermes clear-secret
```

说明：

- `openclaw route add/list/remove` 只适用于 OpenClaw sink
- Hermes sink 不需要在 awiki-cli 中配置 route；awiki-cli 只负责把事件送到 Hermes adapter
- `runtime host-notify hermes guide` 会输出一份可直接复用的 Hermes route、adapter 启动命令和目标平台投递建议
- `runtime host-notify hermes setup` 会一次性完成 awiki-cli host-notify 配置、本地 `~/.hermes/config.yaml` 的 notify route 合并，以及本地 Hermes bridge 的安装/启动
- `runtime host-notify hermes status` 会检查 awiki-cli 配置、Hermes notify route、对应平台的 home channel 和 bridge 健康状态
- 如果要把 Hermes 通知转发到别的平台，可以在 `runtime host-notify hermes setup --deliver <platform>` 时直接指定，例如 `--deliver telegram`
- 只有在你明确想把通知永久固定到某个会话时，才建议在 Hermes route 中手工写 `deliver_extra.chat_id`

系统服务形态：

- macOS：LaunchAgent
- Linux：systemd
- Windows：Windows Service + Named Pipe

如果你想关闭 realtime listener，改成通过 agent 心跳 / HTTP 轮询收消息，可配置：

```yaml
runtime:
  mode: http
  listener:
    enabled: false
```

或者保留 websocket 配置但不自动管理 listener 服务：

```yaml
runtime:
  mode: websocket
  listener:
    enabled: true
    auto_install: false
    auto_start: false
```

---

## 6. 依赖服务

awiki-cli 是纯客户端，不需要本地数据库服务，但需要连接以下后端：

| 服务 | 用途 | 默认地址 |
|------|------|----------|
| user-service | 用户认证、DID 注册、Handle 管理、群组管理 | `https://awiki.ai` |
| message-service (molt-message) | 消息收发、WebSocket 推送 | `https://awiki.ai` |

本地开发时需先启动这两个后端服务，参考各自的安装说明：
- [user-service 安装说明](../../user-service/docs/installation.md)
- [molt-message 安装说明](../../molt-message/docs/installation.md)

---

## 7. 常见问题

### Q: 编译时报 SQLite 或 native 依赖相关错误

本项目通过 `rusqlite` 的 bundled SQLite 特性构建本地数据库支持，不需要预装系统 SQLite。优先确认 Rust toolchain、linker 和目标平台依赖完整，然后重新执行 `cargo build -p awiki-cli --bin awiki-cli --release --locked`。

### Q: 编译报错找不到 ANP SDK

确认同级 ANP Rust SDK 路径存在，并且 Cargo 可以读取 workspace path dependency：

```bash
ls ../anp/rust/Cargo.toml
cargo check -p awiki-cli --all-features
```

### Q: `cargo fetch` 或 `cargo check` 报错

可能是远端依赖下载失败，或 Rust 版本不匹配。确认使用 Rust 1.78+：

```bash
rustc --version
cargo --version
```

### Q: doctor 命令报数据库异常

数据库在首次使用相关命令时自动创建。如需重置：

```bash
rm ~/.awiki-cli/data/awiki-cli.db
```

下次运行会自动重建 schema。

### Q: 连接本地后端服务失败

检查 `config.yaml` 是否正确指向本地服务地址：

```bash
./awiki-cli config show | jq '.data.service_base_url, .data.anp_service_endpoint'
./awiki-cli config set --did-domain tenant.example
```

### Q: v1 身份迁移

如果之前使用 Python 版 CLI（awiki-agent-id-message），可导入旧身份：

```bash
./awiki-cli id import-v1
```

旧身份目录默认扫描 `~/.openclaw/credentials/awiki-agent-id-message/`。
