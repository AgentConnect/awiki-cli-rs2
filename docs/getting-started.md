# awiki-cli Quick Start

[English](getting-started.md) | [简体中文](getting-started.zh-CN.md)

This guide provides a verifiable source build and first-use path. After a formal release, add the stable channel's one-line installer to the README without leaving template variables.

## 1. Installation status

The release system is designed to publish `manifest.json`, `awiki-cli.tgz`, platform binaries, `awiki-cli-skill.tar.gz`, and `.well-known/agent-skills/index.json` for each channel.

The current `release/0710` onboarding still uses `{{AWIKI_CLI_CHANNEL_BASE_URL}}`. Until the release owner confirms a real URL, public documentation must use the source build as the primary path.

## 2. Requirements

- Rust 1.88+ as selected by `rust-toolchain.toml`
- Cargo
- Node.js 18+ for installer wrappers and release scripts
- Sibling ANP Rust SDK at `../anp/anp/rust`
- Flutter/Dart only when changing `packages/awiki_im_core` or `crates/im-core-dart`

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 3. Build

Development build:

```bash
cargo build -p awiki-cli --locked
cargo run -p awiki-cli -- version
```

Use a release build only for publishing or a release-only issue:

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
```

## 4. Initialize a workspace

The default location is `~/.awiki-cli/`. Isolate one Agent or test with:

```bash
export AWIKI_CLI_WORKSPACE_HOME_DIR=~/awiki-workspaces/agent-1
```

Initialize and inspect it:

```bash
cargo run -p awiki-cli -- init
cargo run -p awiki-cli -- status
cargo run -p awiki-cli -- doctor
```

`init` creates the current tenant configuration and local SQLite schema. It does not guarantee that the listener is installed or running.

## 5. Prepare an identity

### 5.1 Email registration

```bash
cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --email you@example.com \
  --wait
```

`--wait` waits for email activation or times out.

### 5.2 Phone registration

Request a code, then submit the OTP:

```bash
cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --phone +8613800138000

cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --phone +8613800138000 \
  --otp <otp-code>
```

Never include example phone numbers or OTPs in real screenshots or logs.

CLI 在注册完成后，会先为本次注册的身份建立消息同步基线，再返回成功；
之后到达的普通消息可以直接通过 `msg inbox` / `msg history` 的 HTTP 同步接收，
无需先启动 WebSocket listener。仅发送验证码、等待邮箱验证或进入设备 Join
准备阶段不会执行此同步。

如果返回 `registration_receive_pending`，身份注册已经提交，接收初始化尚未完成。
保留工作区，运行 `--identity <registered-identity> msg inbox` 重试，成功后再开始收消息；
不要重新注册或删除本地状态。该重试不承诺补回首次基线建立之前的消息。
收件箱或历史查询若因时间/分页预算耗尽而未同步完成，会返回包含
`sync.budget_exhausted` 的错误；再次查询会从已提交的同步进度继续。

### 5.3 Recover an identity

```bash
cargo run -p awiki-cli -- id recover \
  --handle <your-handle> \
  --phone <bound-phone>

cargo run -p awiki-cli -- id recover \
  --handle <your-handle> \
  --phone <bound-phone> \
  --otp <otp-code>
```

### 5.4 Inspect identity state

```bash
cargo run -p awiki-cli -- id status
cargo run -p awiki-cli -- id list
cargo run -p awiki-cli -- id current
```

## 6. Runtime

### WebSocket mode (recommended)

```bash
cargo run -p awiki-cli -- runtime setup --mode websocket
cargo run -p awiki-cli -- runtime listener status
```

The default listener policy may install and start a system service, so this operation has side effects. If it is not running:

```bash
cargo run -p awiki-cli -- runtime listener start
```

### HTTP mode

For one-shot calls:

```bash
cargo run -p awiki-cli -- runtime setup --mode http
```

HTTP mode does not need a resident listener, but it cannot receive WebSocket downstream messages.

## 7. First message

### 7.1 Dry run

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --text "hello from AWiki" \
  --dry-run
```

Inspect `data.plan.target`, remote calls, and the current identity, not only `summary`.

### 7.2 Send

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --text "hello from AWiki"
```

### 7.3 Inbox and history

```bash
cargo run -p awiki-cli -- msg inbox
cargo run -p awiki-cli -- msg history --with <recipient-handle>
```

### 7.4 Attachments

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --file ./hello.txt \
  --text "hello attachment"

cargo run -p awiki-cli -- msg attachment download \
  --with <recipient-handle> \
  --message-id <message-id> \
  --output ./downloads/hello.txt
```

Downloading writes a local file. Agents must confirm the target path first.

## 8. Output formats

```bash
awiki-cli msg inbox --format json
awiki-cli msg inbox --format pretty
awiki-cli msg inbox --format table
awiki-cli msg inbox --format ndjson
awiki-cli msg inbox --jq '.data.messages[] | .id'
```

Agents and scripts should prefer JSON. On failure, branch on `error.code`, `hint`, and `retryable`, and also inspect the process exit code.

## 9. Connect to a self-hosted AWiki Open Server

Create an isolated tenant:

```bash
awiki-cli tenant setup community \
  --backend-base-url https://community.example.com \
  --did-host community.example.com

awiki-cli init
awiki-cli tenant current
```

Open Server has no E2EE or production SMS/email verification by default, and provides participant group capabilities rather than complete group administration. Read its Client Compatibility documentation first. Local development may use `http://127.0.0.1:<port>` and the corresponding DID host, but public deployments must never enable development bypass switches.

## 10. Common diagnostics

```bash
awiki-cli status
awiki-cli doctor
awiki-cli config show
awiki-cli tenant current
awiki-cli runtime status
awiki-cli runtime listener status
awiki-cli schema msg.send
```

When command shape is uncertain, use `awiki-cli --help`, `awiki-cli <domain> --help`, or `awiki-cli schema [command]`. Do not guess flags or raw RPC methods.

## 11. Local data

```text
~/.awiki-cli/
├── global.json
├── cache/
└── tenants/
    ├── registry.json
    └── <tenant>/
        ├── config.yaml
        ├── identities/
        ├── data/awiki-cli.db
        ├── cache/
        ├── runtime/
        └── logs/
```

Private keys, tokens, E2EE state, runtime tokens, and local databases are sensitive. Never upload an entire workspace for troubleshooting.

### 身份索引与工作区升级

CLI 工作区版本与 Core 身份索引版本是两个独立版本。工作区 `v3 → v4`
升级通过 Core 的只读兼容接口读取身份索引和 owner/DID 元数据，不再使用
旧版 CLI 身份管理器解析新版索引。支持范围及 schema 6 的 ANP Identity
迁移标记由 Core 统一校验；未知版本或不合法标记会在数据库变更前拒绝。
该流程不改写身份索引、不读取密钥，也不自动迁移身份密钥托管方式。

升级中断后，修复版本继续使用原升级日志和备份完成续跑；不要手动降低
`identities/index.json` 的版本号或删除身份目录。`awiki-cli doctor` 可检查
索引版本和升级日志。此修复尚需随下一版 CLI 发布，现有安装包不会自动改变。
