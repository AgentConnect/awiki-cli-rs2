# awiki-cli 快速开始

[English](getting-started.md) | [简体中文](getting-started.zh-CN.md)

本文提供当前仓库内可验证的源码构建和首次使用路径。正式发布后，应在 README 顶部增加 stable channel 的一行安装命令，但不能保留模板变量。

## 1. 安装方式状态

发布系统设计为每个 channel 提供：

- `manifest.json`；
- `awiki-cli.tgz`；
- 平台二进制 artifacts；
- `awiki-cli-skill.tar.gz`；
- `.well-known/agent-skills/index.json`。

当前 `release/0710` 的 onboarding 仍使用 `{{AWIKI_CLI_CHANNEL_BASE_URL}}`。在真实 URL 由发布负责人确认前，公共文档必须以源码构建为主路径。

## 2. 环境要求

- Rust 1.88+，以 `rust-toolchain.toml` 为准；
- Cargo；
- Node.js 18+，用于安装包装和发布脚本；
- sibling ANP Rust SDK：`../anp/anp/rust`；
- Flutter/Dart 仅在修改 `packages/awiki_im_core` 或 `crates/im-core-dart` 时需要。

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 3. 构建

开发构建：

```bash
cargo build -p awiki-cli --locked
cargo run -p awiki-cli -- version
```

Release 构建只用于发布或排查 release-only 问题：

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
```

## 4. 初始化 workspace

默认目录：

```text
~/.awiki-cli/
```

为单个 Agent 或测试隔离 workspace：

```bash
export AWIKI_CLI_WORKSPACE_HOME_DIR=~/awiki-workspaces/agent-1
```

初始化：

```bash
cargo run -p awiki-cli -- init
cargo run -p awiki-cli -- status
cargo run -p awiki-cli -- doctor
```

`init` 创建当前租户的配置和本地 SQLite schema，但不保证 listener 已安装或启动。

## 5. 准备身份

### 5.1 邮箱注册

```bash
cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --email you@example.com \
  --wait
```

`--wait` 会等待邮箱激活状态完成或超时。

### 5.2 手机号注册

先请求验证码：

```bash
cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --phone +8613800138000
```

收到 OTP 后：

```bash
cargo run -p awiki-cli -- id register \
  --handle <your-handle> \
  --phone +8613800138000 \
  --otp <otp-code>
```

示例号码和 OTP 不应出现在真实截图或日志中。

### 5.3 恢复身份

```bash
cargo run -p awiki-cli -- id recover \
  --handle <your-handle> \
  --phone <bound-phone>

cargo run -p awiki-cli -- id recover \
  --handle <your-handle> \
  --phone <bound-phone> \
  --otp <otp-code>
```

### 5.4 检查身份

```bash
cargo run -p awiki-cli -- id status
cargo run -p awiki-cli -- id list
cargo run -p awiki-cli -- id current
```

## 6. Runtime

### WebSocket 模式（推荐）

```bash
cargo run -p awiki-cli -- runtime setup --mode websocket
cargo run -p awiki-cli -- runtime listener status
```

默认 listener policy 可能安装并启动系统服务，因此这是有副作用的操作。

如 listener 未启动：

```bash
cargo run -p awiki-cli -- runtime listener start
```

### HTTP 模式

只需要一次性调用时：

```bash
cargo run -p awiki-cli -- runtime setup --mode http
```

HTTP 模式不依赖常驻 listener，但不会提供 WebSocket 下行消息接收。

## 7. 第一条消息

### 7.1 Dry-run

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --text "hello from AWiki" \
  --dry-run
```

检查 `data.plan.target`、远端调用和当前 identity，不要仅阅读 `summary`。

### 7.2 实际发送

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --text "hello from AWiki"
```

### 7.3 Inbox 与 History

```bash
cargo run -p awiki-cli -- msg inbox
cargo run -p awiki-cli -- msg history --with <recipient-handle>
```

### 7.4 附件

```bash
cargo run -p awiki-cli -- msg send \
  --to <recipient-handle> \
  --file ./hello.txt \
  --text "hello attachment"
```

下载：

```bash
cargo run -p awiki-cli -- msg attachment download \
  --with <recipient-handle> \
  --message-id <message-id> \
  --output ./downloads/hello.txt
```

下载会写本地文件，Agent 执行前必须确认目标路径。

## 8. 输出格式

```bash
awiki-cli msg inbox --format json
awiki-cli msg inbox --format pretty
awiki-cli msg inbox --format table
awiki-cli msg inbox --format ndjson
awiki-cli msg inbox --jq '.data.messages[] | .id'
```

Agent 和脚本应优先使用 JSON。失败时按 `error.code`、`hint` 与 `retryable` 分支，并同步检查进程 exit code。

## 9. 连接自托管 AWiki Open Server

为自托管域创建独立租户：

```bash
awiki-cli tenant setup community \
  --backend-base-url https://community.example.com \
  --did-host community.example.com

awiki-cli init
awiki-cli tenant current
```

重要限制：

- Open Server 不支持 E2EE；
- 生产短信/邮件验证默认不存在；
- 群组是参与者能力，不包含完整管理方法；
- 使用自托管服务前应阅读 Open Server 的 Client Compatibility 文档。

本地开发也可以将 backend 指向 `http://127.0.0.1:<port>` 并使用对应 DID host，但不要把开发 bypass 开关用于公网。

## 10. 高频诊断

```bash
awiki-cli status
awiki-cli doctor
awiki-cli config show
awiki-cli tenant current
awiki-cli runtime status
awiki-cli runtime listener status
awiki-cli schema msg.send
```

如命令形状不确定，先使用：

```bash
awiki-cli --help
awiki-cli <domain> --help
awiki-cli schema [command]
```

不要猜测 flag 或 raw RPC 方法。

## 11. 本地数据

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

私钥、token、E2EE 状态、runtime token 和本地数据库都属于敏感数据。不要上传整个 workspace 作为排障附件。
