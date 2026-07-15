# awiki-cli 安装与开发说明

## 概述

`awiki-cli` 是 awiki 的命令行客户端和 Skill backend。本仓库当前是 Rust workspace：`crates/im-core` 提供可复用 IM SDK，`crates/awiki-cli` 提供 CLI 产品壳，`crates/im-core-dart` 与 `packages/awiki_im_core` 提供 Flutter/Dart SDK 入口。

主要能力包括 DID/Handle 身份管理、auth/session、消息收发、群组、附件、邮件、内容页、site pages、direct/group E2EE、realtime listener、本地缓存和 host notification。

**技术栈**: Rust 1.88+ workspace + Cargo + bundled SQLite (`rusqlite`) + ANP Rust SDK

## 1. 环境要求

- Rust toolchain：使用仓库根目录 `rust-toolchain.toml` 固定版本。
- Node.js 18+：用于本地脚本、install script 和 release manifest 生成。
- 同级 ANP Rust SDK：`Cargo.toml` 通过 path dependency 读取 `../anp/anp/rust`。
- Flutter SDK：仅在修改 `packages/awiki_im_core` 或 `crates/im-core-dart` 时需要。

`Cargo.toml` 中的最低 Rust 版本为 1.88。发布脚本默认使用 `AWIKI_CLI_RUST_TOOLCHAIN=1.88.0`，如本机已安装兼容 toolchain，也可以通过环境变量覆盖。

检查本地依赖：

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 2. 构建与测试

常用 Rust 命令：

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p awiki-cli -- --help
```

如本机未安装 Rust，可使用 Docker 备选：

```bash
docker run --rm -v "$PWD":/app -w /app rust:1.88 cargo build --workspace --locked
docker run --rm -v "$PWD":/app -w /app rust:1.88 cargo test --workspace --all-features
```

结构与版本检查：

```bash
cargo run -p xtask -- check-structure
cargo run -p xtask -- check-version
```

发布产物请通过脚本构建，确保 feature graph 和版本检查一致：

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
scripts/release/build-release-artifact.sh --os darwin --arch arm64
```

Daemon 发布推荐在目标服务器上执行高层发布脚本：

```bash
scripts/release/daemon/publish-multi-platform.sh
```

脚本不接受参数，也不依赖外部环境变量。发布前复制并填写
`scripts/release/daemon/publish-multi-platform.toml.template` 为同级
`publish-multi-platform.toml`。脚本从 `crates/awiki-deamon/Cargo.toml`
读取发布版本，校验 `Cargo.lock` 中的 `awiki-deamon` 版本一致，并要求该版本高于
Nginx daemon 静态目录中 `releases/manifest.json` 的 `latest`。GitHub Actions
构建 Linux amd64、macOS arm64 和 macOS amd64 三个平台包；目标服务器下载 artifacts
后发布到本机 `/var/www/awiki-web/daemon`。
发布配置文件不配置当前版本号或最低可用版本号；第一版流程中，manifest 的
`min_supported` 自动等于当前 Daemon 版本。`base_url` 是后端服务/API 根地址；
`download_base_url` 是当前发布机器提供的 daemon 静态下载根地址，省略时默认使用
`base_url/daemon`；`download_mirror_urls` 是可选镜像源列表，只写入安装脚本。
镜像节点通过 `scripts/release/daemon/sync-download-mirror.sh` 从主源 HTTP(S) 拉取同步，
同步脚本不接受命令行参数，只读取同目录 `sync-download-mirror.toml`。

Flutter SDK 变更还需要：

```bash
scripts/flutter/build-sdk-native.sh
```

该脚本会检查 Flutter/Rust bridge 生成文件，并重新构建 macOS、iOS、Android 的 `awiki_im_core` native 产物。只需要单平台验证时可使用 `--macos-only`、`--ios-only` 或 `--android-only`。

## 3. 工作区布局

默认产品目录为 `~/.awiki-cli/`，Windows 对应 `%USERPROFILE%\.awiki-cli\`。只支持通过 `AWIKI_CLI_WORKSPACE_HOME_DIR` 切换整个产品目录根路径。

CLI 以租户为单位隔离后端地址、DID host 和本地数据。租户是 `backend_base_url + did_host` 的原子组合；要使用新的组合，必须先创建租户，再按租户名切换。

```text
~/.awiki-cli/
  global.json
  cache/
  tenants/
    registry.json
    default/
      config.yaml
      identities/
      data/awiki-cli.db
      cache/
      runtime/
      logs/
```

用途：

- `global.json`：记录当前激活租户。
- `tenants/registry.json`：记录所有租户的 `backend_base_url`、`did_host`、展示名和目录名。
- `tenants/<name>/config.yaml`：租户内本地运行配置，例如 runtime、输出格式、secret storage、ANP/mail override。
- `tenants/<name>/identities/`：该租户下的身份索引、DID document、私钥和 auth/session 文件。
- `tenants/<name>/data/awiki-cli.db`：该租户的 SQLite 本地缓存，使用 bundled SQLite，不需要系统 SQLite。
- `tenants/<name>/runtime/`：该租户的 listener socket、pid/status、host-notify 事件等运行时文件。
- `cache/`：全局缓存，例如 CLI 更新元数据；CLI 二进制版本本身也是全局的，不随租户切换。

## 4. 配置文件

推荐通过 `awiki-cli init` 创建默认租户配置：

```bash
awiki-cli init
awiki-cli config show
```

租户管理：

```bash
awiki-cli tenant list
awiki-cli tenant current
awiki-cli tenant create acme --backend-base-url https://api.acme.example --did-host acme.example
awiki-cli tenant setup acme --backend-base-url https://api.acme.example --did-host acme.example
awiki-cli tenant use acme
awiki-cli tenant reconfigure acme --backend-base-url https://api2.acme.example --did-host acme.example
```

租户名会 trim 并规范化为小写；只允许 ASCII 字母、数字和单个 `-` 分隔符，最长 64 个字符，不能以 `-` 开头或结尾，也不能包含 `--`。如果需要中文、空格或展示用大小写，使用 `--display-name`。

`tenant setup` 是面向 Onboarding 的幂等入口：不存在时创建并切换，已存在且 endpoint 完全一致时直接切换，配置不一致时拒绝覆盖。它不会执行 `init`，切换后仍需运行 `awiki-cli init`。

`tenant reconfigure` 只允许修改还没有身份或本地数据库数据的租户；如果租户已经有数据，请创建一个新租户。

最小配置示例：

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
    hermes:
      notify_url: http://127.0.0.1:8765/notify/host-event
      secret: ""
output:
  format: json
  no_color: false
services:
  anp_service_endpoint: ""
  anp_service_did: ""
  ca_bundle: ""
  mail_service_url: ""
```

配置优先级：

```text
flag > tenant config.yaml > tenant registry > default
```

`backend_base_url` 是 CLI 连接 user-service、message-service、content、group 等服务的基础地址；`did_host` 用于补全 bare handle，例如 `alice` 会按当前租户 DID host 补全。它们只来自租户注册表，不在 `config.yaml` 中配置。

`anp_service_endpoint` 和 `anp_service_did` 会写入本地 DID document 的 `ANPMessageService`。为空时会从当前租户的 `backend_base_url` 自动推导。`mail_service_url` 为空时复用当前租户的 `backend_base_url`。

## 5. 身份与本地状态

身份文件位于 `tenants/<name>/identities/<identity-dir>/`：

```text
identity.json
auth.json
did_document.json
key-1-private.pem
key-1-public.pem
e2ee-signing-private.pem
e2ee-agreement-private.pem
```

私钥文件必须保持本地私有，不要上传或分享。CLI 会尽量使用 `0600` 文件权限和 `0700` 目录权限。

SQLite 本地库自动初始化，主要保存 contacts、messages、groups、relationship events、E2EE outbox 等业务缓存。业务状态由 `im-core` 按绑定身份隔离，CLI 不应直接拼 owner 查询；排障可使用 `debug db` 命令。

## 6. 快速上手

初始化与身份：

```bash
awiki-cli init
awiki-cli id list
awiki-cli id current
awiki-cli id register --handle myname
```

消息：

```bash
awiki-cli msg send --to alice --text "hello"
awiki-cli msg send --group <group-id> --text "hello"
awiki-cli msg inbox
awiki-cli msg history --with alice
```

附件：

```bash
awiki-cli msg send --to alice --file ./hello.txt --text "hello attachment"
awiki-cli msg attachment download --with alice --message-id <msg-id> --output ./downloads/hello.txt
```

邮件：

```bash
awiki-cli mail inbox --folder inbox --limit 20
awiki-cli mail send --to a@example.com --subject "Hello" --body "Hi"
```

Site pages：

```bash
awiki-cli site root get --domain example.com
awiki-cli site page create --domain example.com --slug about --markdown-file ./about.md
```

## 7. Runtime 与 Host Notification

默认 runtime mode 是 `websocket`。`awiki-cli init` 和 `runtime setup` 会按配置安装/启动 listener 服务：

```bash
awiki-cli runtime setup --mode websocket
awiki-cli runtime apply
awiki-cli runtime listener status
awiki-cli runtime listener restart
```

关闭自动服务管理：

```yaml
runtime:
  mode: websocket
  listener:
    enabled: true
    auto_install: false
    auto_start: false
```

Host notification 支持 `noop | log | file | openclaw | hermes`：

```bash
awiki-cli runtime host-notify config show
awiki-cli runtime host-notify config set --sink hermes
awiki-cli runtime host-notify hermes guide
awiki-cli runtime host-notify hermes setup
awiki-cli runtime host-notify hermes status
```

Hermes sink 中，awiki-cli 只负责把 host event 转发给 Hermes adapter；最终投递平台由 Hermes 管理。OpenClaw sink 使用本机 loopback webhook，并支持 route add/list/remove。

## 8. E2EE

Direct E2EE 和 Group E2EE 通过 `im-core` 的 secure 能力编排。CLI 只解析用户意图，例如 `--secure required`、`group secure status`、`group secure repair`，不暴露 MLS 私有状态、KeyPackage 原文、prekey 或 ciphertext。

Group E2EE 默认使用 `im-core` native provider。低层 `group e2ee *` 命令属于内部/诊断面，不是默认产品契约。

## 9. 常见问题

### 编译时报 SQLite 相关错误

本项目使用 `rusqlite` bundled SQLite。优先确认 Rust toolchain、linker 和目标平台依赖完整，然后重新运行：

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
```

### 找不到 ANP SDK

确认 sibling path dependency 存在：

```bash
ls ../anp/anp/rust/Cargo.toml
cargo check -p awiki-cli --locked
```

### 连接服务失败

检查 `config.yaml`：

```bash
awiki-cli config show
awiki-cli tenant current
awiki-cli tenant list
```

### 重置本地数据库

确认不需要保留本地缓存后删除：

```bash
rm ~/.awiki-cli/tenants/<tenant-name>/data/awiki-cli.db
```

下次运行会自动重建 schema。

### 从旧 Python CLI 导入身份

旧身份默认从 `~/.openclaw/credentials/awiki-agent-id-message/` 扫描：

```bash
awiki-cli --migration id import-v1
```

导入和自动升级可能生成包含敏感密钥材料的备份目录，不要上传或分享。
